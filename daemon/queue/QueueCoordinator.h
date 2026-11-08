/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef QUEUECOORDINATOR_H

#ifndef ARTICLEDOWNLOADER_H
#define ARTICLEDOWNLOADER_H

#include <atomic>
#include <functional>
#include <memory>
#include <set>
#include <string>
#include <vector>
#include "Util.h"
#include "DiskState.h"
#include "DiskState.h"
#include "Util.h"
#include "Util.h"

class ArticleContentAnalyzer
{
public:
	virtual ~ArticleContentAnalyzer() {};
	virtual void Reset() = 0;
	virtual void Append(const void* buffer, int len) = 0;
};

class ArticleDownloader final : public Thread, public Subject
{
public:
	enum EStatus
	{
		adUndefined,
		adRunning,
		adWaiting,
		adFinished,
		adFailed,
		adRetry,
		adCrcError,
		adNotFound,
		adConnectError,
		adTransientError,
		adFatalError
	};

	ArticleDownloader();
	virtual ~ArticleDownloader();
	void SetFileInfo(FileInfo* fileInfo) { m_fileInfo = fileInfo; }
	FileInfo* GetFileInfo() { return m_fileInfo; }
	void SetArticleInfo(ArticleInfo* articleInfo) { m_articleInfo = articleInfo; }
	ArticleInfo* GetArticleInfo() { return m_articleInfo; }
	EStatus GetStatus() { return m_status; }
	ServerStatList* GetServerStats() { return &m_serverStats; }
	void Run() override;
	void Stop() override;
	time_t GetLastUpdateTime() { return m_lastUpdateTime; }
	void SetLastUpdateTimeNow();
	const char* GetArticleFilename() { return m_articleFilename; }
	void SetInfoName(const char* infoName);
	const char* GetInfoName() { return m_infoName; }
	const char* GetConnectionName() { return m_connectionName; }
	void SetConnection(NntpConnection* connection) { m_connection = connection; }
	bool CompleteFileParts() { return m_articleWriter.CompleteFileParts(); }
	int GetDownloadedSize() { return m_downloadedSize; }
	int64 GetDecodedFileSize() { return m_decodedFileSize; }
	void SetContentAnalyzer(std::unique_ptr<ArticleContentAnalyzer> contentAnalyzer) { m_contentAnalyzer = std::move(contentAnalyzer); }
	ArticleContentAnalyzer* GetContentAnalyzer() { return m_contentAnalyzer.get(); }

	void LogDebugInfo();

private:
	FileInfo* m_fileInfo;
	ArticleInfo* m_articleInfo;
	NntpConnection* m_connection = nullptr;
	EStatus m_status = adUndefined;
	Mutex m_connectionMutex;
	CString m_infoName;
	CString m_connectionName;
	CString m_articleFilename;
	std::atomic<time_t> m_lastUpdateTime;
	Decoder m_decoder;
	ArticleWriter m_articleWriter;
	ServerStatList m_serverStats;
	bool m_writingStarted;
	// Distinguish local output failures from malformed/source articles.  Only
	// the former must bypass duplicate fallback.
	bool m_localWriteError = false;
	bool m_contentRejected = false;
	int m_downloadedSize = 0;
	int64 m_decodedFileSize = 0;
	std::unique_ptr<ArticleContentAnalyzer> m_contentAnalyzer;

	EStatus Download();
	EStatus DecodeCheck();
	void FreeConnection(bool keepConnected);
	EStatus CheckResponse(const char* response, const char* comment);
	void SetStatus(EStatus status) { m_status = status; }
	bool Write(char* buffer, int len);
	void AddServerStats();
};

#endif

#ifndef DIRECTRENAMER_H
#define DIRECTRENAMER_H

class DirectRenamer
{
public:
	class FileHash
	{
	public:
		FileHash(const char* filename, const char* hash) :
			m_filename(filename), m_hash(hash) {}
		const char* GetFilename() { return m_filename; }
		const char* GetHash() { return m_hash; }

	private:
		CString m_filename;
		CString m_hash;
	};

	typedef std::deque<FileHash> FileHashList;

	class ParFile
	{
	public:
		ParFile(int id, const char* filename, const char* setId, bool completed) :
			m_id(id), m_filename(filename), m_setId(setId), m_completed(completed) {}
		int GetId() { return m_id; }
		const char* GetFilename() { return m_filename; }
		const char* GetSetId() { return m_setId; }
		bool GetCompleted() { return m_completed; }

	private:
		int m_id;
		CString m_filename;
		CString m_setId;
		bool m_completed = false;
	};

	typedef std::deque<ParFile> ParFileList;

	std::unique_ptr<ArticleContentAnalyzer> MakeArticleContentAnalyzer();
	/* does the article the analyzer read start with a par2 packet */
	static bool AnalyzedParFile(ArticleContentAnalyzer* articleContentAnalyzer);
	void ArticleDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo,
		ArticleInfo* articleInfo, ArticleContentAnalyzer* articleContentAnalyzer);
	void FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo);

protected:
	virtual void RenameCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) = 0;

private:
	void CheckState(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void UnpausePars(NzbInfo* nzbInfo);
	void RenameFiles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileHashList* parHashes);
	bool RenameFile(NzbInfo* nzbInfo, const std::string& oldFullFilename, const std::string& newFullFilename);
	bool NeedRenamePars(NzbInfo* nzbInfo);
	void CollectPars(NzbInfo* nzbInfo, ParFileList* parFiles);
	std::string BuildNewRegularName(const char* oldName, FileHashList* parHashes, const char* hash16k);
	std::string BuildNewParName(const char* oldName, const char* destDir, const char* setId, int& vol);
	int RenameCompletedFiles(NzbInfo* nzbInfo, FileHashList* parHashes, bool needRenamePars, int& vol);
	int RenameFilesInProgress(NzbInfo* nzbInfo, FileHashList* parHashes, bool needRenamePars, int& vol);

	friend class DirectParLoader;
};

#include <cstddef>
#include <memory>
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

class RenameContentAnalyzer : public ArticleContentAnalyzer
{
public:
	virtual void Reset();
	virtual void Append(const void* buffer, int len);
	void Finish();
	const char* GetHash16k() { return m_hash16k; }
	bool GetParFile() { return m_parFile; }
	const char* GetParSetId() { return m_parSetId; }

private:
#ifndef DISABLE_PARCHECK
	// rust/src/renameanalyzer.rs
	std::unique_ptr<NzbgetRsRenameAnalyzer, void (*)(NzbgetRsRenameAnalyzer*)> m_rs{
		nzbget_rs_rename_analyzer_new(), nzbget_rs_rename_analyzer_free};
#endif
	CString m_hash16k;
	CString m_parSetId;
	bool m_parFile = false;
};

#ifndef DISABLE_PARCHECK
class DirectParLoader : public Thread
{
public:
	static void StartLoader(DirectRenamer* owner, NzbInfo* nzbInfo);
	virtual void Run();

private:
	typedef std::vector<CString> ParFiles;

	DirectRenamer* m_owner;
	ParFiles m_parFiles;
	DirectRenamer::FileHashList m_parHashes;
	int m_nzbId;

	void LoadParFile(const char* parFile);
};

inline void DirectParLoader::StartLoader(DirectRenamer* owner, NzbInfo* nzbInfo)
{
	if (nzbInfo->GetSkipDiskWrite() || g_Options->GetSkipWrite())
	{
		return;
	}

	nzbInfo->PrintMessage(Message::mkInfo, "Directly checking renamed files for %s", nzbInfo->GetName());

	DirectParLoader* directParLoader = new DirectParLoader();
	directParLoader->m_owner = owner;
	directParLoader->m_nzbId = nzbInfo->GetId();

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetParFile())
		{
			NzbgetRsBuf parPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, completedFile.GetFilename());
			directParLoader->m_parFiles.emplace_back(parPathBuf.data ? parPathBuf.data : "");
			nzbget_rs_free(parPathBuf);
		}
	}

	directParLoader->SetAutoDestroy(true);
	directParLoader->Start();
}

inline void DirectParLoader::Run()
{
	debug("Started DirectParLoader");

	for (CString& parFile : m_parFiles)
	{
		LoadParFile(parFile);
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (nzbInfo && !nzbInfo->GetSkipDiskWrite() && !g_Options->GetSkipWrite())
	{
		// nzb is still in queue
		m_owner->RenameFiles(downloadQueue, nzbInfo, &m_parHashes);
	}
}

inline void DirectParLoader::LoadParFile(const char* parFile)
{
	struct LoaderCtx
	{
		DirectParLoader* self;
		const char* parFile;
		NzbInfo* nzbInfo;
		std::optional<GuardedDownloadQueue> guard;
	} ctx{this, parFile, nullptr, std::nullopt};

	if (!nzbget_rs_par_renamer_load_par_entries(
		parFile,
		&ctx,
		[](void* rawCtx) noexcept -> int
		{
			auto* c = static_cast<LoaderCtx*>(rawCtx);
			c->guard.emplace(DownloadQueue::Guard());
			c->nzbInfo = (*c->guard)->GetQueue()->Find(c->self->m_nzbId);
			if (!c->nzbInfo)
			{
				return 0;
			}
			c->nzbInfo->PrintMessage(Message::mkInfo, "Loaded par2-file %s for direct-rename",
				FileSystem::BaseFileName(c->parFile));
			return 1;
		},
		[](void* rawCtx, int damaged, const char* name, const char* hash) noexcept -> int
		{
			auto* c = static_cast<LoaderCtx*>(rawCtx);
			if (c->self->IsStopped())
			{
				return 0;
			}
			if (damaged != 0)
			{
				c->nzbInfo->PrintMessage(Message::mkWarning, "Damaged par2-file detected: %s",
					FileSystem::BaseFileName(c->parFile));
				return 0;
			}
			debug("file: %s, hash-16k: %s", name ? name : "", hash ? hash : "");
			c->self->m_parHashes.emplace_back(name ? name : "", hash ? hash : "");
			return 1;
		}))
	{
		warn("Could not load par2-file %s", parFile);
	}
}
#endif

inline std::unique_ptr<ArticleContentAnalyzer> DirectRenamer::MakeArticleContentAnalyzer()
{
	return std::make_unique<RenameContentAnalyzer>();
}

inline bool DirectRenamer::AnalyzedParFile(ArticleContentAnalyzer* articleContentAnalyzer)
{
	return ((RenameContentAnalyzer*)articleContentAnalyzer)->GetParFile();
}

inline void DirectRenamer::ArticleDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo,
	ArticleInfo* articleInfo, ArticleContentAnalyzer* articleContentAnalyzer)
{
	debug("Applying analyzer data %s for ", fileInfo->GetFilename());

	RenameContentAnalyzer* contentAnalyzer = (RenameContentAnalyzer*)articleContentAnalyzer;
	contentAnalyzer->Finish();

	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();

	NzbgetRsDirectRenameParDelta parDelta{};
	nzbget_rs_direct_renamer_eval_article(
		articleInfo->GetSize(),
		fileInfo->GetArticles()->size(),
		fileInfo->GetParFile() ? 1 : 0,
		contentAnalyzer->GetParFile() ? 1 : 0,
		fileInfo->GetPaused() ? 1 : 0,
		fileInfo->GetSize(),
		fileInfo->GetSuccessSize(),
		fileInfo->GetFailedSize(),
		fileInfo->GetMissedSize(),
		nzbInfo->GetParSize(),
		nzbInfo->GetParCurrentSuccessSize(),
		nzbInfo->GetParCurrentFailedSize(),
		nzbInfo->GetParFailedSize(),
		nzbInfo->GetRemainingParCount(),
		&parDelta);

	if (parDelta.apply_hash16k)
	{
		fileInfo->SetHash16k(contentAnalyzer->GetHash16k());
		debug("file: %s; article-hash16k: %s", fileInfo->GetFilename(), fileInfo->GetHash16k());
	}

	fileInfo->GetNzbInfo()->PrintMessage(Message::mkDetail, "Detected %s %s",
		contentAnalyzer->GetParFile() ? "par2-file" : "non-par2-file",
		fileInfo->GetFilename());

	if (parDelta.par_flag_changed)
	{
		debug("Changing par2-flag for %s", fileInfo->GetFilename());
		fileInfo->SetParFile(contentAnalyzer->GetParFile());

		nzbInfo->SetParSize(parDelta.par_size);
		nzbInfo->SetParCurrentSuccessSize(parDelta.par_current_success_size);
		nzbInfo->SetParCurrentFailedSize(parDelta.par_current_failed_size);
		nzbInfo->SetParFailedSize(parDelta.par_failed_size);
		nzbInfo->SetRemainingParCount(parDelta.remaining_par_count);

		if (parDelta.unpause_non_par)
		{
			fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Resuming non-par2-file %s", fileInfo->GetFilename());
			fileInfo->SetPaused(false);
		}

		nzbInfo->SetChanged(true);
		downloadQueue->SaveChanged();
	}

	if (fileInfo->GetParFile())
	{
		fileInfo->SetParSetId(contentAnalyzer->GetParSetId());
		debug("file: %s; setid: %s", fileInfo->GetFilename(), fileInfo->GetParSetId());
	}

	CheckState(downloadQueue, nzbInfo);
}

inline void DirectRenamer::FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	CheckState(downloadQueue, fileInfo->GetNzbInfo());
}

inline void DirectRenamer::CheckState(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
#ifndef DISABLE_PARCHECK
	if (nzbInfo->GetDirectRenameStatus() > NzbInfo::tsRunning)
	{
		return;
	}

	// check if all first articles are successfully downloaded (1)
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (Util::EmptyStr(fileInfo->GetHash16k()) ||
			(fileInfo->GetParFile() && Util::EmptyStr(fileInfo->GetParSetId())))
		{
			return;
		}
	}

	// check if all first articles are successfully downloaded (2)
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (Util::EmptyStr(completedFile.GetHash16k()) ||
			(completedFile.GetParFile() && Util::EmptyStr(completedFile.GetParSetId())))
		{
			return;
		}
	}

	if (!nzbInfo->GetWaitingPar())
	{
		// all first articles downloaded
		UnpausePars(nzbInfo);
		nzbInfo->SetWaitingPar(true);
		nzbInfo->SetChanged(true);
		downloadQueue->SaveChanged();
	}

	if (nzbInfo->GetWaitingPar() && !nzbInfo->GetLoadingPar())
	{
		// check if all par2-files scheduled for downloading already completed
		FileList::iterator pos = std::find_if(
			nzbInfo->GetFileList()->begin(), nzbInfo->GetFileList()->end(),
			[](std::unique_ptr<FileInfo>& fileInfo)
		{
			return fileInfo->GetExtraPriority();
		});

		if (pos == nzbInfo->GetFileList()->end())
		{
			// all wanted par2-files are downloaded
			nzbInfo->SetLoadingPar(true);
			DirectParLoader::StartLoader(this, nzbInfo);
			return;
		}
	}
#endif
}

// Unpause smallest par-files from each par-set
inline void DirectRenamer::UnpausePars(NzbInfo* nzbInfo)
{
	ParFileList parFiles;
	CollectPars(nzbInfo, &parFiles);

	size_t n = parFiles.size();
	if (n == 0)
	{
		return;
	}
	std::vector<int> ids;
	std::vector<const char*> setIds;
	std::vector<uint8_t> completed;
	std::vector<uint8_t> inFileList;
	std::vector<int64> sizes;
	std::vector<int> outFileIds(n, 0);
	ids.reserve(n);
	setIds.reserve(n);
	completed.reserve(n);
	inFileList.reserve(n);
	sizes.reserve(n);
	for (ParFile& parFile : parFiles)
	{
		FileInfo* fileInfo = nzbInfo->GetFileList()->Find(parFile.GetId());
		ids.push_back(parFile.GetId());
		setIds.push_back(parFile.GetSetId());
		completed.push_back(parFile.GetCompleted() ? 1 : 0);
		inFileList.push_back(fileInfo ? 1 : 0);
		sizes.push_back(fileInfo ? fileInfo->GetSize() : 0);
	}
	size_t selected = nzbget_rs_direct_renamer_select_pars_to_unpause(
		ids.data(),
		setIds.data(),
		completed.data(),
		inFileList.data(),
		sizes.data(),
		n,
		outFileIds.data());
	for (size_t i = 0; i < selected; i++)
	{
		FileInfo* fileInfo = nzbInfo->GetFileList()->Find(outFileIds[i]);
		if (fileInfo)
		{
			nzbInfo->PrintMessage(Message::mkInfo, "Increasing priority for par2-file %s", fileInfo->GetFilename());
			fileInfo->SetPaused(false);
			fileInfo->SetExtraPriority(true);
		}
	}
}

inline void DirectRenamer::CollectPars(NzbInfo* nzbInfo, ParFileList* parFiles)
{
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (fileInfo->GetParFile())
		{
			parFiles->emplace_back(fileInfo->GetId(), fileInfo->GetFilename(), fileInfo->GetParSetId(), false);
		}
	}

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetParFile())
		{
			parFiles->emplace_back(completedFile.GetId(), completedFile.GetFilename(), completedFile.GetParSetId(), true);
		}
	}
}

inline void DirectRenamer::RenameFiles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileHashList* parHashes)
{
	int renamedCount = 0;
	int vol = 1;
	bool needRenamePars = NeedRenamePars(nzbInfo);

	renamedCount += RenameFilesInProgress(nzbInfo, parHashes, needRenamePars, vol);
	renamedCount += RenameCompletedFiles(nzbInfo, parHashes, needRenamePars, vol);

	if (renamedCount > 0)
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Successfully renamed %i file(s) for %s", renamedCount, nzbInfo->GetName());
	}
	else
	{
		nzbInfo->PrintMessage(Message::mkInfo, "No renamed files found for %s", nzbInfo->GetName());
	}

	RenameCompleted(downloadQueue, nzbInfo);
}

/**
 * @brief Renames (metadata only) partially downloaded files using info from PAR files.
 * Doesn't rename the files themselves. The actual renaming happens only when the download is complete.
 * Renaming incomplete files can cause DirectUnpack to fail, as it may try to unpack incomplete .rar files.
*/
inline int DirectRenamer::RenameFilesInProgress(NzbInfo* nzbInfo, FileHashList* parHashes, bool needRenamePars, int& vol)
{
	int renamedFiles = 0;

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		std::string newName;
		if (fileInfo->GetParFile() && needRenamePars)
		{
			newName = BuildNewParName(fileInfo->GetFilename(), nzbInfo->GetDestDir(), fileInfo->GetParSetId(), vol);
		}
		else if (!fileInfo->GetParFile())
		{
			newName = BuildNewRegularName(fileInfo->GetFilename(), parHashes, fileInfo->GetHash16k());
		}

		if (newName.empty())
		{
			if (!fileInfo->GetParFile() && g_Options->GetHardLinking())
			{
				newName = fileInfo->GetFilename();
			}
			else
			{
				continue;
			}
		}

		const std::string& currOutputFilename = fileInfo->GetOutputFilename();
		const std::string nzbDestDir = fileInfo->GetNzbInfo()->GetDestDir();
		NzbgetRsBuf oldOutBuf = nzbget_rs_direct_renamer_resolve_old_output_path(
			currOutputFilename.c_str(), nzbDestDir.c_str(), PATH_SEPARATOR, fileInfo->GetFilename());
		const std::string oldOutputFilename(oldOutBuf.data ? oldOutBuf.data : "", oldOutBuf.len);
		nzbget_rs_free(oldOutBuf);

		nzbInfo->PrintMessage(Message::mkInfo,
			"Renaming in-progress file %s to %s",
			fileInfo->GetFilename(), newName.c_str()
		);

		// Create hardlink and save the path in fileInfo
		if (g_Options->GetHardLinking() &&
			!Util::MatchFileExt(newName.c_str(), g_Options->GetHardLinkingIgnoreExt(), ","))
		{
			fs::error_code ec;
			if (!fs::exists(oldOutputFilename, ec))
			{
				nzbInfo->PrintMessage(Message::mkDetail,
					"Skipping hardlink for %s: file not assembled yet",
					fileInfo->GetFilename()
				);
			}
			else
			{
				std::string nzbFinalDir = fileInfo->GetNzbInfo()->BuildFinalDirName().Str();
				NzbgetRsBuf finalOutBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					nzbFinalDir.c_str(), PATH_SEPARATOR, newName.c_str());
				std::string finalOutputFilename(finalOutBuf.data ? finalOutBuf.data : "", finalOutBuf.len);
				nzbget_rs_free(finalOutBuf);

				nzbInfo->PrintMessage(Message::mkInfo,
					"HardLinking in-progress file %s to %s",
					fileInfo->GetFilename(), newName.c_str()
				);

				CString errmsg;
				if (FileSystem::CreateHardLink(oldOutputFilename.c_str(), finalOutputFilename.c_str(), errmsg))
				{
					fileInfo->SetHardLinkPath(std::move(finalOutputFilename));
					nzbInfo->SetHardLinkPath(std::move(nzbFinalDir));
				}
				else
				{
					nzbInfo->PrintMessage(Message::mkError,
						"Could not create hardlink %s to %s: %s",
						fileInfo->GetFilename(), newName.c_str(), *errmsg
					);
				}				
			}
		}

		if (Util::EmptyStr(fileInfo->GetOrigname()))
		{
			fileInfo->SetOrigname(fileInfo->GetFilename());
		}

		// Set the filename to the full relative path
		fileInfo->SetFilename(std::move(newName));
		fileInfo->SetFilenameConfirmed(true);
		++renamedFiles;
	}

	return renamedFiles;
}

inline int DirectRenamer::RenameCompletedFiles(NzbInfo* nzbInfo, FileHashList* parHashes, bool needRenamePars, int& vol)
{
	int renamedFiles = 0;

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		std::string newName;
		if (completedFile.GetParFile() && needRenamePars)
		{
			newName = BuildNewParName(completedFile.GetFilename(), nzbInfo->GetDestDir(), completedFile.GetParSetId(), vol);
		}
		else if (!completedFile.GetParFile())
		{
			newName = BuildNewRegularName(completedFile.GetFilename(), parHashes, completedFile.GetHash16k());
		}

		if (newName.empty())
		{
			if (!completedFile.GetParFile() && g_Options->GetHardLinking())
			{
				newName = completedFile.GetFilename();
			}
			else
			{
				continue;
			}
		}

		const std::string destDir = nzbInfo->GetDestDir();
		NzbgetRsBuf oldOutBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			destDir.c_str(), PATH_SEPARATOR, completedFile.GetFilename());
		const std::string oldOutputFilename(oldOutBuf.data ? oldOutBuf.data : "", oldOutBuf.len);
		nzbget_rs_free(oldOutBuf);
		NzbgetRsBuf outBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			destDir.c_str(), PATH_SEPARATOR, newName.c_str());
		const std::string outputFilename(outBuf.data ? outBuf.data : "", outBuf.len);
		nzbget_rs_free(outBuf);

		nzbInfo->PrintMessage(Message::mkInfo,
			"Renaming completed file %s to %s",
			completedFile.GetFilename(), newName.c_str()
		);

		if (!Util::EmptyStr(g_Options->GetInterDir()) &&
			g_Options->GetHardLinking() &&
			!Util::MatchFileExt(newName.c_str(), g_Options->GetHardLinkingIgnoreExt(), ","))
		{
			fs::error_code ec;
			if (!fs::exists(oldOutputFilename, ec))
			{
				nzbInfo->PrintMessage(Message::mkDetail,
					"Skipping hardlink for %s: file not assembled yet",
					completedFile.GetFilename()
				);
			}
			else
			{
				const std::string finalDir = nzbInfo->BuildFinalDirName().Str();
				NzbgetRsBuf finalOutBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					finalDir.c_str(), PATH_SEPARATOR, newName.c_str());
				const std::string finalOutputFilename(finalOutBuf.data ? finalOutBuf.data : "", finalOutBuf.len);
				nzbget_rs_free(finalOutBuf);

				nzbInfo->PrintMessage(Message::mkInfo,
					"HardLinking completed file %s to %s",
					completedFile.GetFilename(), newName.c_str()
				);

				CString errmsg;
				if (!FileSystem::CreateHardLink(oldOutputFilename.c_str(), finalOutputFilename.c_str(), errmsg))
				{
					nzbInfo->PrintMessage(Message::mkError,
						"Could not create hardlink %s to %s: %s",
						completedFile.GetFilename(), newName.c_str(), *errmsg
					);
				}				
			}
		}

		if (RenameFile(nzbInfo, oldOutputFilename, outputFilename))
		{
			if (Util::EmptyStr(completedFile.GetOrigname()))
			{
				completedFile.SetOrigname(completedFile.GetFilename());
			}
			// Set the filename to the full relative path
			completedFile.SetFilename(std::move(newName));
			++renamedFiles;
		}
	}

	return renamedFiles;
}

inline std::string DirectRenamer::BuildNewRegularName(const char* oldName, FileHashList* parHashes, const char* hash16k)
{
	// rust/src/directrenamer.rs
	ptrdiff_t index = nzbget_rs_direct_regular_name(oldName, hash16k, parHashes->size(),
		[](void* ctx, size_t index, const char** filename, const char** hash)
		{
			FileHash& parHash = (*static_cast<FileHashList*>(ctx))[index];
			*filename = parHash.GetFilename();
			*hash = parHash.GetHash();
		}, parHashes);
	return index < 0 ? "" : (*parHashes)[index].GetFilename();
}

inline std::string DirectRenamer::BuildNewParName(const char* oldName, const char* destDir, const char* setId, int& vol)
{
	NzbgetRsBuf name = nzbget_rs_direct_par_name(oldName, destDir, setId, PATH_SEPARATOR, &vol);
	std::string res(name.data ? name.data : "", name.len);
	nzbget_rs_free(name);
	return res;
}

inline bool DirectRenamer::NeedRenamePars(NzbInfo* nzbInfo)
{
	// rust/src/directrenamer.rs
	ParFileList parFiles;
	CollectPars(nzbInfo, &parFiles);

	std::vector<NzbgetRsParName> names;
	names.reserve(parFiles.size());
	for (ParFile& parFile : parFiles)
	{
		names.push_back({parFile.GetFilename(), parFile.GetSetId()});
	}
	return nzbget_rs_direct_need_rename_pars(names.data(), names.size()) != 0;
}

inline bool DirectRenamer::RenameFile(NzbInfo* nzbInfo, const std::string& oldFullFilename, const std::string& newFullFilename)
{
	const auto [oldPath, oldName] = FileSystem::SplitPathAndFilename(oldFullFilename);
	const auto [newPath, newName] = FileSystem::SplitPathAndFilename(newFullFilename);

	if (oldPath != newPath)
	{
		CString errmsg;
		if (!FileSystem::ForceDirectories(newPath.c_str(), errmsg))
		{
			nzbInfo->PrintMessage(Message::mkError, 
				"Failed to rename file %s to %s. Could not create the target directory \"%s\"",
				oldName.c_str(), newName.c_str(), newPath.c_str()
			);

			return false;
		}
	}

	if (!FileSystem::MoveFile(oldFullFilename.c_str(), newFullFilename.c_str()))
	{
		nzbInfo->PrintMessage(Message::mkError, 
			"Failed to rename file %s to %s: %s",
			oldName.c_str(), newName.c_str(), *FileSystem::GetLastErrorMessage()
		);

		return false;
	}

	return true;
}

inline void RenameContentAnalyzer::Reset()
{
#ifndef DISABLE_PARCHECK
	nzbget_rs_rename_analyzer_reset(m_rs.get());
#endif
}

inline void RenameContentAnalyzer::Append(const void* buffer, int len)
{
#ifndef DISABLE_PARCHECK
	char setId[33];
	if (nzbget_rs_rename_analyzer_append(m_rs.get(), (const char*)buffer, len, setId))
	{
		m_parFile = true;
		m_parSetId = setId;
	}
#else
	(void)buffer;
	(void)len;
#endif
}

// Must be called with locked DownloadQueue
inline void RenameContentAnalyzer::Finish()
{
#ifndef DISABLE_PARCHECK
	char hash[33];
	nzbget_rs_rename_analyzer_finish(m_rs.get(), hash);
	m_hash16k = hash;
#endif
}

#endif


#ifndef QUEUECOORDINATOR_H
#define QUEUECOORDINATOR_H

class QueueEditor
{
public:
	bool EditEntry(DownloadQueue* downloadQueue, int ID, DownloadQueue::EEditAction action, const char* args);
	bool EditList(DownloadQueue* downloadQueue, IdList* idList, NameList* nameList, DownloadQueue::EMatchMode matchMode, DownloadQueue::EEditAction action, const char* args);

private:
	class EditItem
	{
	public:
		FileInfo* m_fileInfo;
		NzbInfo* m_nzbInfo;
		int m_offset;

		EditItem(FileInfo* fileInfo, NzbInfo* nzbInfo, int offset) :
			m_fileInfo(fileInfo), m_nzbInfo(nzbInfo), m_offset(offset) {}
	};

	typedef std::vector<EditItem> ItemList;

	DownloadQueue* m_downloadQueue;

	FileInfo* FindFileInfo(int id);
	bool InternEditList(ItemList* itemList, IdList* idList, DownloadQueue::EEditAction action, const char* args);
	void PrepareList(ItemList* itemList, IdList* idList, DownloadQueue::EEditAction action, int offset);
	bool BuildIdListFromNameList(IdList* idList, NameList* nameList, DownloadQueue::EMatchMode matchMode, DownloadQueue::EEditAction action);
	bool EditGroup(NzbInfo* nzbInfo, DownloadQueue::EEditAction action, const char* args);
	void PauseParsInGroups(ItemList* itemList, bool extraParsOnly);
	void PausePars(RawFileList* fileList, bool extraParsOnly);
	void SetNzbPriority(NzbInfo* nzbInfo, const char* priority);
	void SetNzbCategory(NzbInfo* nzbInfo, const char* category, bool applyParams);
	void SetNzbName(NzbInfo* nzbInfo, const char* name);
	bool MergeGroups(ItemList* itemList);
	bool SortGroups(ItemList* itemList, const char* sort);
	void AlignGroups(ItemList* itemList);
	bool MoveGroupsTo(ItemList* itemList, IdList* idList, bool before, const char* args);
	bool SplitGroup(ItemList* itemList, const char* name);
	bool DeleteUrl(NzbInfo* nzbInfo, DownloadQueue::EEditAction action);
	void ReorderFiles(ItemList* itemList);
	void SetNzbParameter(NzbInfo* nzbInfo, const char* paramString);
	void SetNzbDupeParam(NzbInfo* nzbInfo, DownloadQueue::EEditAction action, const char* args);
	void PauseUnpauseEntry(FileInfo* fileInfo, bool pause);
	void DeleteEntry(FileInfo* fileInfo);
	void MoveEntry(FileInfo* fileInfo, int offset);
	void MoveGroup(NzbInfo* nzbInfo, int offset);
	void SortGroupFiles(NzbInfo* nzbInfo);
	bool ItemListContainsItem(ItemList* itemList, int id);

	friend class GroupSorter;
};

class DupeProbe : public Thread
{
public:
	struct Sample
	{
		CString MessageId;
		std::shared_ptr<std::vector<CString>> Groups;
	};

	enum EAnswer
	{
		daExists,
		daMissing,
		daUnknown
	};

	static constexpr int SampleCount = 10;
	static constexpr int MinArticles = 4;
	static constexpr int MinAliveSamples = 2;
	static constexpr int RecentBlockSec = 60;

	static std::vector<size_t> SampleIndexes(size_t total, int count);
	static EAnswer Classify(const char* response);
	static bool IsDead(int existing, int definitiveServers, int activeServers);
	static std::vector<Sample> SamplesOf(const std::vector<FileInfo*>& files,
		const std::function<void(FileInfo*)>& loadArticles = nullptr);
	static void Start(int nzbId, std::vector<Sample> samples, int recoveredAtStart = 0);

	struct Verdict
	{
		int Existing = 0;
		int MissingServers = 0;
		std::string FoundOn;
		int ActiveServers = 0;
		int UnreachableServers = 0;
		int ReachedServers = 0;
		bool Finished = true;
		bool Dead() const { return Finished && IsDead(Existing, MissingServers, ActiveServers); }
	};

	static Verdict Check(std::vector<Sample> samples, int limitSec);
	static void StartRecheck(int nzbId, std::vector<Sample> samples);
	static constexpr int RecheckSampleCount = 10;
	static constexpr int RecheckLimitSec = 60;

	static void StopAll();
	static int ProbingFor(int nzbId);
	static void WaitAll();
	static void Reset();

protected:
	void Run() override;

private:
	DupeProbe(int nzbId, std::vector<Sample> samples) :
		m_nzbId(nzbId), m_samples(std::move(samples)), m_found(m_samples.size(), 0) {}

	struct ServerResult
	{
		int Exists = 0;
		int Missing = 0;
		int Unknown = 0;
		bool Unreachable = false;
	};

	int m_nzbId;
	time_t m_started = 0;
	int m_recoveredAtStart = 0;
	std::vector<Sample> m_samples;
	bool m_countAll = false;
	std::vector<char> m_found;
	std::string m_foundOn;
	NntpConnection* m_connection = nullptr;

	bool ProbeServer(int serverId, std::set<int>& probed, ServerResult& result);
	void Note(Message::EKind kind, const char* format, ...);
	Verdict Measure(int limitSec);
	void Recheck();
	bool Register();
	void Unregister();
	void Cancel();
	void Abandon(int missingServers, int activeServers);
};

class QueueCoordinator : public Thread, public Observer, public Debuggable
{
public:
	using ActiveDownloads = std::list<ArticleDownloader*>;

	QueueCoordinator();
	~QueueCoordinator() override;
	void Run() override;
	void Stop() override;
	/* the dead-pick probe (see DupeProbe) found none of the download's sampled
	 * articles on any server: fail over to the best duplicate in history */
	void FailOverDeadPick(DownloadQueue* downloadQueue, int nzbId, int samples,
		int missingServers, int activeServers, int recoveredAtStart = 0);
	void Update(Subject* caller, void* aspect) override;

	// editing queue
	NzbInfo* AddNzbFileToQueue(std::unique_ptr<NzbInfo> nzbInfo, NzbInfo* urlInfo, bool addFirst);
	void CheckDupeFileInfos(NzbInfo* nzbInfo);
	bool HasMoreJobs() const { return m_hasMoreJobs; }
	void DiscardTempFiles(FileInfo* fileInfo);
	bool DeleteQueueEntry(DownloadQueue* downloadQueue, FileInfo* fileInfo);
	bool SetQueueEntryCategory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* category);
	bool SetQueueEntryName(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* name);
	bool MergeQueueEntries(DownloadQueue* downloadQueue, NzbInfo* destNzbInfo, NzbInfo* srcNzbInfo);
	bool SplitQueueEntries(DownloadQueue* downloadQueue, RawFileList* fileList, const char* name, NzbInfo** newNzbInfo);
	// release info, checksums, images, links: no stream repair, no proof of a download
	static bool SideFile(const char* filename);

	// files posted less than propagationDelay seconds ago are held back from downloading
	static bool IsPropagationWait(time_t postTime, int propagationDelay, time_t curDate);

protected:
	void LogDebugInfo() override;

private:
	class CoordinatorDownloadQueue : public DownloadQueue
	{
	public:
		CoordinatorDownloadQueue(QueueCoordinator* owner) : m_owner(owner) {}
		bool EditEntry(int ID, EEditAction action, const char* args) override;
		bool EditList(IdList* idList, NameList* nameList, EMatchMode matchMode,
			EEditAction action, const char* args) override;
		void HistoryChanged() override { m_historyChanged = true; }
		void Save() override;
		void SaveChanged() override;
	private:
		QueueCoordinator* m_owner;
		bool m_massEdit = false;
		bool m_wantSave = false;
		bool m_historyChanged = false;
		bool m_stateChanged = false;
		friend class QueueCoordinator;
	};

	class CoordinatorDirectRenamer : public DirectRenamer
	{
	public:
		CoordinatorDirectRenamer(QueueCoordinator* owner) : m_owner(owner) {}
	protected:
		void RenameCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) override
			{ m_owner->DirectRenameCompleted(downloadQueue, nzbInfo); }
	private:
		QueueCoordinator* m_owner;
	};

	CoordinatorDownloadQueue m_downloadQueue{this};
	ActiveDownloads m_activeDownloads;
	QueueEditor m_queueEditor;
	CoordinatorDirectRenamer m_directRenamer{this};
	DupeArticleFallback m_dupeArticleFallback;
	std::atomic<bool> m_hasMoreJobs{true};
	int m_downloadsLimit;
	int m_serverConfigGeneration = 0;
	int m_propagationWaitLoggedId = 0;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	// set by WakeUp (under m_waitMutex): ends the stand-by wait
	bool m_wakeUp = false;

	bool GetNextArticle(DownloadQueue* downloadQueue, FileInfo* &fileInfo, ArticleInfo* &articleInfo);
	bool GetNextFirstArticle(NzbInfo* nzbInfo, FileInfo* &fileInfo, ArticleInfo* &articleInfo);
	void StartArticleDownload(FileInfo* fileInfo, ArticleInfo* articleInfo, NntpConnection* connection);
	void ArticleCompleted(ArticleDownloader* articleDownloader);
	void DeleteDownloader(DownloadQueue* downloadQueue, ArticleDownloader* articleDownloader, bool fileCompleted);
	void DeleteFileInfo(DownloadQueue* downloadQueue, FileInfo* fileInfo, bool completed);
	void DemoteMisalignedDupeNeighbors(FileInfo* fileInfo, ArticleInfo* articleInfo);
	void ValidateCompletedFileTiling(FileInfo* fileInfo);
	void DemoteFinishedArticle(FileInfo* fileInfo, ArticleInfo* articleInfo);
	void DiscardArticleSegment(FileInfo* fileInfo, ArticleInfo* articleInfo);
	void DirectRenameCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void DiscardDirectRename(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void DiscardDownloadedArticles(NzbInfo* nzbInfo, FileInfo* fileInfo);
	void CheckHealth(DownloadQueue* downloadQueue, FileInfo* fileInfo);
	void CheckDupeFailover(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void StartDeadPickProbe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void CheckDeadDownload(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	bool CheckParDamage(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	int DownloadCriticalHealth(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	static bool CertainlyParless(NzbInfo* nzbInfo);
	static bool HasSizeTwin(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	bool HasActiveDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) const;
	static int FilesTried(NzbInfo* nzbInfo);
	void RejectBorrowedArticles(FileInfo* fileInfo, const std::vector<ArticleInfo*>& articles);
	void CountAsFailed(FileInfo* fileInfo, ArticleInfo* articleInfo);
	// failed articles the duplicates must have been asked for before the
	// early dupe failover judges the download-time recovery
	static constexpr int DupeFailoverSample = 32;
	// failed articles beyond the first of each file, with none of its own downloaded,
	// after which a download counts as dead (HealthCheck=dupe)
	static constexpr int DeadDownloadFailures = 64;
	// articles of its own tried before a download's final health is projected (B48)
	static constexpr int ProjectedFailureSample = 200;
	// how much wholer (per mille) a backup must be than a doomed download's projection
	static constexpr int BackupLead = 100;
	// articles tried before a download projected below critical (not far below) is
	// parked without a backup (B74)
	static constexpr int ParkBelowCriticalSample = 1000;
	// seconds a dead-pick probe runs before a download nothing arrives of stops waiting for it (B83)
	static constexpr int SlowProbeSec = 10;
	// failed articles in a row (first articles of files aside) after which a download is dead (B49)
	static constexpr int DeadRunFailures = 40;
	// a download without a better duplicate is parked once this share of its
	// articles was tried (percent) and fewer than this share of the tried
	// ones existed (percent; the ParScan=dupe park ratio): nothing of it
	// exists for par2 or byte-level repair to build on
	static constexpr int DupeHopelessTriedPercent = 10;
	static constexpr int DupeHopelessAlivePercent = 10;
	static bool DownloadHopeless(NzbInfo* nzbInfo);
	void ResetHangingDownloads();
	void AdjustDownloadsLimit();
	void Load();
	void SaveQueueIfChanged();
	void SaveAllPartialState();
	void SavePartialState(FileInfo* fileInfo);
	void LoadPartialState(FileInfo* fileInfo);
	void SaveAllFileState();
	void WaitJobs();
	void WakeUp();
};

extern QueueCoordinator* g_QueueCoordinator;


#ifndef HISTORYCOORDINATOR_H
#define HISTORYCOORDINATOR_H
class HistoryCoordinator : public Service
{
public:
	void AddToHistory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void RecheckFailedArticles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action, const char* args);
	void DeleteDiskFiles(NzbInfo* nzbInfo);
	void HistoryHide(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, int rindex);
	void Redownload(DownloadQueue* downloadQueue, HistoryInfo* historyInfo);

protected:
	virtual int ServiceInterval() { return 60 * 60; }
	virtual void ServiceWork();

private:
	void HistoryDelete(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool final);
	void HistoryReturn(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo);
	void HistoryProcess(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo);
	void HistoryRedownload(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool restorePauseState);
	void HistoryRetry(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool resetFailed, bool reprocess);
	bool HistorySetParameter(HistoryInfo* historyInfo, const char* text);
	void HistorySetDupeParam(HistoryInfo* historyInfo, DownloadQueue::EEditAction action, const char* text);
	bool HistorySetCategory(HistoryInfo* historyInfo, const char* text);
	bool HistorySetName(HistoryInfo* historyInfo, const char* text);
	void MoveToQueue(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool reprocess);
	void PrepareEdit(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action);
	void ResetArticles(FileInfo* fileInfo, bool allFailed, bool resetFailed);
};

extern HistoryCoordinator* g_HistoryCoordinator;

class DupeCoordinator
{
public:
	enum EDupeStatus
	{
		dsNone = 0,
		dsQueued = 1,
		dsDownloading = 2,
		dsSuccess = 4,
		dsWarning = 8,
		dsFailure = 16
	};

	void NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void HistoryMark(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, NzbInfo::EMarkStatus markStatus);
	EDupeStatus GetDupeStatus(DownloadQueue* downloadQueue, const char* name, const char* dupeKey);
	RawNzbList ListHistoryDupes(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	static bool SameNameOrKey(const char* name1, const char* dupeKey1, const char* name2, const char* dupeKey2);
	/* The dupe-backup in history ReturnBestDupe would move to the queue for
	 * this title (nullptr if none, or if a good-duplicate already exists). */
	HistoryInfo* FindDupeBackup(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey);
	/* Whether a queued item at the given health (permille) and duplicate score
	 * should be abandoned early for a backup with the given score (option
	 * <HealthCheck> value "dupe"): the backup's score must not fall below the
	 * score the item still warrants at its remaining health. */
	static bool DupeFailoverWarranted(int itemScore, int health, int backupScore);
	/* a dupe tool's health check of a duplicate (parameter <name>, in percent,
	 * fractions allowed) in per-mille, clamped to 0..1000; -1 if not set or not a number */
	static int AlivePermille(NzbInfo* nzbInfo, const char* name = "DupeAlive");
	/* the download gets a duplicate in its place when it fails or turns out dead:
	 * DupeMode score; with HealthCheck=dupe a forced one too - it never fails
	 * outright while a viable duplicate waits (B46) */
	static bool FailsOver(NzbInfo* nzbInfo);
	/* With HealthCheck=dupe: a successful item of the same (non-empty) duplicate key
	 * whose files are still on disk, or nullptr (B70, B77). Within DownloadQueue-lock. */
	static NzbInfo* DownloadedOnDisk(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	/* the item's final (or destination) directory holds a file other than a side file */
	static bool FilesOnDisk(NzbInfo* nzbInfo);
	/* why no duplicate in history can take the download's place, for the log */
	std::string NoBackupReason(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);

private:
	void ReturnBestDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey);
	void HistoryCleanup(DownloadQueue* downloadQueue, HistoryInfo* markHistoryInfo);
};

extern DupeCoordinator* g_DupeCoordinator;




#ifndef SCANNER_H
#define SCANNER_H
#include <mutex>
#include <string>

class Scanner final : public Service
{
public:
	static constexpr std::string_view PROCESSED_DIR = "_processed";
	static constexpr std::string_view BROKEN_DIR = "_broken";
	static constexpr std::string_view UNPACK_DIR = "_unpack";

	enum EAddStatus
	{
		asSkipped,
		asSuccess,
		asFailed
	};

	void InitOptions();
	void ScanNzbDir(bool syncMode);
	EAddStatus AddArchive(
		const char* filename,
		const char* category,
		bool autoCategory,
		int priority,
		const char* dupeKey,
		int dupeScore,
		EDupeMode dupeMode,
		NzbParameterList* parameters,
		bool addTop,
		bool addPaused,
		NzbInfo* urlInfo,
		const char* buffer,
		int bufSize
	);
	EAddStatus AddArchive(
		const char* filename,
		const char* category,
		bool autoCategory,
		int priority,
		const char* dupeKey,
		int dupeScore,
		EDupeMode dupeMode,
		NzbParameterList* parameters,
		bool addTop,
		bool addPaused,
		NzbInfo* urlInfo,
		const char* tmpFilename
	);
	EAddStatus AddExternalFile(
		const char* nzbName, 
		const char* category,
		bool autoCategory,
		int priority,
		const char* dupeKey, 
		int dupeScore, 
		EDupeMode dupeMode,
		NzbParameterList* parameters, 
		bool addTop, 
		bool addPaused, 
		NzbInfo* urlInfo,
		const char* fileName, 
		const char* buffer, 
		int bufSize, 
		int* nzbId
	);
	void InitPPParameters(const char* category, NzbParameterList* parameters, bool reset);
	CString ResolveCategory(const char* category, const char* filename);

protected:
	int ServiceInterval() override;
	void ServiceWork() override;

private:
	class FileData
	{
	public:
		FileData(const char* filename, int64 size, time_t lastChange)
			: m_filename{ filename ? filename : "" }
			, m_size{ size }
			, m_lastChange{ lastChange } {}
		const char* GetFilename() const { return m_filename.c_str(); }
		int64 GetSize() { return m_size; }
		void SetSize(int64 size) { m_size = size; }
		time_t GetLastChange() { return m_lastChange; }
		void SetLastChange(time_t lastChange) { m_lastChange = lastChange; }
	private:
		std::string m_filename;
		int64 m_size;
		time_t m_lastChange;
	};

	using FileList = std::deque<FileData>;

	class QueueData
	{
	public:
		QueueData(
			const char* filename,
			const char* nzbName,
			const char* category,
			bool autoCategory,
			int priority,
			const char* dupeKey,
			int dupeScore,
			EDupeMode dupeMode,
			NzbParameterList* parameters,
			bool addTop,
			bool addPaused,
			NzbInfo* urlInfo,
			EAddStatus* addStatus,
			int* nzbId
		);
		const char* GetFilename() const { return m_filename.c_str(); }
		const char* GetNzbName() const { return m_nzbName.c_str(); }
		const char* GetCategory() const { return m_category.c_str(); }
		int GetPriority() { return m_priority; }
		const char* GetDupeKey() const { return m_dupeKey.c_str(); }
		int GetDupeScore() { return m_dupeScore; }
		EDupeMode GetDupeMode() { return m_dupeMode; }
		NzbParameterList* GetParameters() { return &m_parameters; }
		bool GetAddTop() { return m_addTop; }
		bool GetAddPaused() { return m_addPaused; }
		bool GetAutoCategory() const { return m_autoCategory; }
		NzbInfo* GetUrlInfo() { return m_urlInfo; }
		void SetAddStatus(EAddStatus addStatus);
		void SetNzbId(int nzbId);
	private:
		NzbParameterList m_parameters;
		std::string m_filename;
		std::string m_nzbName;
		std::string m_category;
		std::string m_dupeKey;
		int m_priority;
		int m_dupeScore;
		bool m_autoCategory;
		bool m_addTop;
		bool m_addPaused;
		EDupeMode m_dupeMode;
		NzbInfo* m_urlInfo;
		EAddStatus* m_addStatus;
		int* m_nzbId;
	};

	using QueueList = std::deque<QueueData>;

	FileList m_fileList;
	QueueList m_queueList;
	std::mutex m_scanMutex;
	std::atomic<bool> m_requestedNzbDirScan{false};
	std::atomic<bool> m_scanning{false};
	inline static int m_idGen = 0;
	int m_nzbDirInterval = 0;
	int m_pass = 0;
	bool m_scanScript = false;
	
	/**
	 * @brief Detects the category from the NZB file and sets it in the NzbInfo object.
	 *
	 * If a category is found in the NZB file and matches a configured category, it is set in nzbInfo.
	 * Otherwise, the detected category (if any) is set directly.
	 */
	void DetectAndSetCategory(const NzbFile& nzbFile, NzbInfo& nzbInfo, const char* nzbName);
	void CheckIncomingArchives(const fs::path& dir, bool checkStat);
	std::vector<fs::path> FindArchives(const fs::path& dir, bool checkStat);
	void UnpackArchives(const std::vector<fs::path>& archives);
	std::optional<std::vector<fs::path>> ProcessArchive(const fs::path& archiveFile, const fs::path& destDir);
	void CleanupStaleUnpackDir();
	void CheckIncomingNzbs(const char* directory, const char* category, bool checkStat);
	bool ShouldSkipItem(std::string_view filename);
	bool AddFileToQueue(
		const char* filename, 
		const char* nzbName, 
		const char* category,
		bool autoCategory,
		int priority, 
		const char* dupeKey, 
		int dupeScore, 
		EDupeMode dupeMode,
		NzbParameterList* parameters, 
		bool addTop, 
		bool addPaused, 
		NzbInfo* urlInfo, 
		int* nzbId
	);
	void ProcessIncomingFile(
		const char* directory, 
		const char* baseFilename,
		const char* fullFilename,
		const char* category
	);

	/**
	 * @brief Process subdirectories recursively to build category paths
	 *
	 * When a subdirectory is encountered, it's treated as a category name.
	 * The category path is built by concatenating the parent category with
	 * the subdirectory name. This allows for nested
	 * category structures like "Movies/Action" or "TV/Comedy".
	 */
	void ProcessSubdirectory(
		const char* fullPath, 
		const char* filename, 
		const char* category, 
		bool checkStat
	);
	bool CanProcessFile(const char* fullFilename, bool checkStat);
	void DropOldFiles();
};

extern Scanner* g_Scanner;

class UrlDownloader;

class UrlCoordinator : public Thread, public Observer, public Debuggable
{
public:
	UrlCoordinator();
	~UrlCoordinator() override;
	void Run() override;
	void Stop() override;
	void Update(Subject* caller, void* aspect) override;

	// Editing the queue
	void AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst);
	bool HasMoreJobs() const { return m_hasMoreJobs; }
	bool DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory);

protected:
	void LogDebugInfo() override;

private:
	using ActiveDownloads = std::list<UrlDownloader*>;

	class DownloadQueueObserver final : public Observer
	{
	public:
		UrlCoordinator* m_owner;
		void Update(Subject* caller, void* aspect) override { m_owner->DownloadQueueUpdate(caller, aspect); }
	};

	ActiveDownloads m_activeDownloads;
	std::atomic<bool> m_hasMoreJobs{true};
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	DownloadQueueObserver m_downloadQueueObserver;

	NzbInfo* GetNextUrl(DownloadQueue* downloadQueue);
	void StartUrlDownload(NzbInfo* nzbInfo);
	void UrlCompleted(UrlDownloader* urlDownloader);
	void ResetHangingDownloads();
	void WaitJobs();
	void DownloadQueueUpdate(Subject* caller, void* aspect);
};

extern UrlCoordinator* g_UrlCoordinator;

class UrlDownloader : public WebDownloader
{
public:
	void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
	NzbInfo* GetNzbInfo() { return m_nzbInfo; }
	const char* GetCategory() const { return m_category; }

protected:
	void ProcessHeader(const char* line) override;

private:
	NzbInfo* m_nzbInfo;
	CString m_category;
};




#include <algorithm>
#include <cstdarg>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef PREPOSTPROCESSOR_H
#define PREPOSTPROCESSOR_H

class PrePostProcessor : public Thread, public Observer
{
public:
	PrePostProcessor();
	void Run() override;
	void Stop() override;
	bool HasMoreJobs() { return m_queuedJobs > 0; }
	int GetJobCount() { return m_queuedJobs; }
	bool HasActiveUnpack(bool includeWaitingVolume = true);
	bool HasActiveUnpack(DownloadQueue* downloadQueue, bool includeWaitingVolume = true);
	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action,
		const char* args);
	void NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	// A late rename or a draining paused PAR request can make the collection
	// ready without completing another file. Requires the download-queue lock.
	void CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);

protected:
	void Update(Subject*, void* aspect) override { DownloadQueueUpdate(aspect); }

private:
	int m_queuedJobs = 0;
	RawNzbList m_activeJobs;
	std::recursive_mutex m_activeJobsMutex;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;

	void CheckPostQueue();
	void CheckRequestPar(DownloadQueue* downloadQueue);
	void CleanupJobs(DownloadQueue* downloadQueue);
	bool CanRunMoreJobs(bool* allowPar);
	NzbInfo* PickNextJob(DownloadQueue* downloadQueue, bool allowPar);
	void StartJob(DownloadQueue* downloadQueue, PostInfo* postInfo, bool allowPar);
	void EnterStage(DownloadQueue* downloadQueue, PostInfo* postInfo, PostInfo::EStage stage);
	void SanitisePostQueue();
	void UpdatePauseState();
	void NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool saveQueue);
	void JobCompleted(DownloadQueue* downloadQueue, PostInfo* postInfo);
	bool PostQueueDelete(DownloadQueue* downloadQueue, IdList* idList);
	void DownloadQueueUpdate(void* aspect);
	void DeleteCleanup(NzbInfo* nzbInfo);
	void WaitJobs();
	void FileDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileInfo* fileInfo);
};

extern PrePostProcessor* g_PrePostProcessor;

#include <atomic>
#include <condition_variable>
#include <deque>
#include <map>
#include <mutex>
#include <set>
#include <thread>
#include <vector>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2023-2025 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

#ifndef EXTENSION_MANAGER_H
#define EXTENSION_MANAGER_H

#include <atomic>
#include <vector>
#include <utility>
#include <memory>
#include <shared_mutex>
#include <optional>
#include <functional>
#include "nzbget_rs.h"

namespace Extension
{
	struct Kind
	{
		bool post = false;
		bool scan = false;
		bool queue = false;
		bool scheduler = false;
		bool feed = false;
	};

	class Script final
	{
	public:
		Script() = default;
		~Script() = default;

		Script& operator=(const Script&) = delete;
		Script(const Script&) = delete;

		Script(Script&& other) = default;
		Script& operator=(Script&& other) = default;

		void SetEntry(std::string entry) { m_entry = std::move(entry); }
		const char* GetEntry() const { return m_entry.c_str(); }
		void SetLocation(std::string location) { m_location = std::move(location); }
		const char* GetLocation() const { return m_location.c_str(); }
		void SetRootDir(std::string dir) { m_rootDir = std::move(dir); }
		const char* GetRootDir() const { return m_rootDir.c_str(); }
		void SetName(std::string name) { m_name = std::move(name); }
		const char* GetName() const { return m_name.c_str(); }
		void SetDisplayName(std::string displayName) { m_displayName = std::move(displayName); }
		const char* GetDisplayName() const { return m_displayName.c_str(); }
		void SetKind(Kind kind) { m_kind = std::move(kind); }
		bool GetPostScript() const { return m_kind.post; }
		bool GetScanScript() const { return m_kind.scan; }
		bool GetQueueScript() const { return m_kind.queue; }
		bool GetSchedulerScript() const { return m_kind.scheduler; }
		bool GetFeedScript() const { return m_kind.feed; }
		void SetQueueEvents(std::string queueEvents) { m_queueEvents = std::move(queueEvents); }
		const char* GetQueueEvents() const { return m_queueEvents.c_str(); }
		void SetTaskTime(std::string taskTime) { m_taskTime = std::move(taskTime); }
		const char* GetTaskTime() const { return m_taskTime.c_str(); }
		void SetJsonStr(std::string json) { m_json = std::move(json); }
		const std::string& GetJsonStr() const { return m_json; }
		void SetXmlStr(std::string xml) { m_xml = std::move(xml); }
		const std::string& GetXmlStr() const { return m_xml; }

	private:
		Kind m_kind;
		std::string m_entry;
		std::string m_location;
		std::string m_rootDir;
		std::string m_name;
		std::string m_displayName;
		std::string m_queueEvents;
		std::string m_taskTime;
		std::string m_json;
		std::string m_xml;
	};

	inline std::string ToJsonStr(const Script& script)
	{
		return script.GetJsonStr();
	}

	inline std::string ToXmlStr(const Script& script)
	{
		return script.GetXmlStr();
	}
}

namespace ExtensionManager
{
	using ExtensionPtr = std::shared_ptr<const Extension::Script>;
	using Extensions = std::vector<ExtensionPtr>;

	class Manager final
	{
	public:
		Manager() = default;
		~Manager() = default;

		Manager(const Manager&) = delete;
		Manager& operator=(const Manager&) = delete;

		Manager(Manager&&) = delete;
		Manager& operator=(Manager&&) = delete;

		std::optional<std::string> 
		InstallExtension(const fs::path& file, const fs::path& dest)
		{
			std::string fileStr = fs::u8string(file);
			std::string destStr = fs::u8string(dest);
			NzbgetRsBuf errBuf = nzbget_rs_ext_mgr_install_extension(
				fileStr.c_str(),
				destStr.c_str(),
				[](void*, int kind, const char* msg) {
					const char* text = msg ? msg : "";
					if (kind == 1)
					{
						error("%s", text);
					}
					else if (kind == 2)
					{
						warn("%s", text);
					}
					else if (kind == 0)
					{
						info("%s", text);
					}
					else
					{
						detail("%s", text);
					}
				},
				nullptr);
			if (errBuf.data)
			{
				std::string err(errBuf.data, errBuf.len);
				nzbget_rs_free(errBuf);
				return err;
			}
			nzbget_rs_free(errBuf);
			return std::nullopt;
		}

		std::optional<std::string> 
		UpdateExtension(const fs::path& filename, const std::string& extName)
		{
			std::unique_lock<std::shared_mutex> lock{m_mutex};

			auto extensionIt = GetByName(extName);
			bool found = extensionIt != std::end(m_extensions);
			int64_t useCount = found ? extensionIt->use_count() : 0;
			fs::path targetPath;
			if (found)
			{
				GetExtensionPath(*(*extensionIt), targetPath);
			}
			std::string targetPathStr = fs::u8string(targetPath);
			std::string archiveStr = fs::u8string(filename);

			NzbgetRsBuf backupBuf{};
			NzbgetRsBuf prepErrBuf = nzbget_rs_ext_mgr_prepare_update(
				extName.c_str(),
				found ? 1 : 0,
				useCount,
				targetPathStr.c_str(),
				archiveStr.c_str(),
				&backupBuf);
			std::string backupPathStr(backupBuf.data ? backupBuf.data : "", backupBuf.len);
			nzbget_rs_free(backupBuf);

			if (prepErrBuf.data)
			{
				std::string err(prepErrBuf.data, prepErrBuf.len);
				nzbget_rs_free(prepErrBuf);
				return err;
			}
			nzbget_rs_free(prepErrBuf);

			const auto rootDir = (*extensionIt)->GetRootDir();
			const auto installExtError = InstallExtension(filename, rootDir);
			NzbgetRsBuf finishErrBuf = nzbget_rs_ext_mgr_finish_update(
				targetPathStr.c_str(),
				backupPathStr.c_str(),
				archiveStr.c_str(),
				installExtError ? installExtError->c_str() : nullptr);
			if (finishErrBuf.data)
			{
				std::string err(finishErrBuf.data, finishErrBuf.len);
				nzbget_rs_free(finishErrBuf);
				return err;
			}
			nzbget_rs_free(finishErrBuf);

			m_extensions.erase(extensionIt);
			return std::nullopt;
		}

		std::optional<std::string>
		DeleteExtension(const std::string& name)
		{
			std::unique_lock<std::shared_mutex> lock{m_mutex};

			auto extensionIt = GetByName(name);
			bool found = extensionIt != std::end(m_extensions);
			int64_t useCount = found ? extensionIt->use_count() : 0;
			fs::path targetPath;
			if (found)
			{
				GetExtensionPath(*(*extensionIt), targetPath);
			}
			std::string targetPathStr = fs::u8string(targetPath);

			NzbgetRsBuf errBuf = nzbget_rs_ext_mgr_delete_extension(
				name.c_str(),
				found ? 1 : 0,
				useCount,
				targetPathStr.c_str());
			if (errBuf.data)
			{
				std::string err(errBuf.data, errBuf.len);
				nzbget_rs_free(errBuf);
				return err;
			}
			nzbget_rs_free(errBuf);

			m_extensions.erase(extensionIt);
			return std::nullopt;
		}

		std::optional<std::string>
		LoadExtensions()
		{
			const char* scriptDir = g_Options->GetScriptDir();
			if (Util::EmptyStr(scriptDir))
			{
				return std::string("\"ScriptDir\" is not specified");
			}

			std::unique_lock<std::shared_mutex> lock{m_mutex};

			m_extensions.clear();

			NzbgetRsBuf errBuf = nzbget_rs_ext_mgr_load_extensions(
				scriptDir,
				g_Options->GetScriptOrder(),
				PATH_SEPARATOR,
				this,
				[](void*, const char* msg)
				{
					error("%s", msg ? msg : "");
				},
				[](
					void* ctx,
					const char* entryPtr, size_t entryLen,
					const char* locPtr, size_t locLen,
					const char* rootPtr, size_t rootLen,
					const char* namePtr, size_t nameLen,
					const char* dispPtr, size_t dispLen,
					const char* qeventsPtr, size_t qeventsLen,
					const char* taskTimePtr, size_t taskTimeLen,
					int postScript, int scanScript, int queueScript, int schedScript, int feedScript,
					const char* jsonPtr, size_t jsonLen,
					const char* xmlPtr, size_t xmlLen)
				{
					auto* mgr = static_cast<Manager*>(ctx);
					Extension::Script ext;
					ext.SetEntry(std::string(entryPtr ? entryPtr : "", entryLen));
					ext.SetLocation(std::string(locPtr ? locPtr : "", locLen));
					ext.SetRootDir(std::string(rootPtr ? rootPtr : "", rootLen));
					ext.SetName(std::string(namePtr ? namePtr : "", nameLen));
					ext.SetDisplayName(std::string(dispPtr ? dispPtr : "", dispLen));
					ext.SetKind({
						postScript != 0,
						scanScript != 0,
						queueScript != 0,
						schedScript != 0,
						feedScript != 0
					});
					ext.SetQueueEvents(std::string(qeventsPtr ? qeventsPtr : "", qeventsLen));
					ext.SetTaskTime(std::string(taskTimePtr ? taskTimePtr : "", taskTimeLen));
					ext.SetJsonStr(std::string(jsonPtr ? jsonPtr : "", jsonLen));
					ext.SetXmlStr(std::string(xmlPtr ? xmlPtr : "", xmlLen));
					mgr->m_extensions.emplace_back(std::make_shared<Extension::Script>(std::move(ext)));
				});
			if (errBuf.data)
			{
				std::string err(errBuf.data, errBuf.len);
				nzbget_rs_free(errBuf);
				return err;
			}
			nzbget_rs_free(errBuf);

			CreateTasks();
			m_extensions.shrink_to_fit();

			return std::nullopt;
		}

		std::pair<WebDownloader::EStatus, fs::path>
		DownloadExtension(const std::string& url, const std::string& extName)
		{
			// a name of our own, not the extension's: that one comes from the request
			// ("../x" wrote anywhere the daemon can), and two downloads of one
			// extension at once wrote the same file
			static std::atomic<int> counter{0};
			fs::path tmpFileName = g_Options->GetTempDirPath() /
				("ext-" + std::to_string(static_cast<int64_t>(Util::CurrentTime())) + "-" + std::to_string(++counter) + ".zip");

			const auto tmpFileNameStr = fs::u8string(tmpFileName);

			std::unique_ptr<WebDownloader> downloader = std::make_unique<WebDownloader>();
			downloader->SetUrl(url.c_str());
			downloader->SetForce(true);
			downloader->SetRetry(false);
			downloader->SetOutputFilename(tmpFileNameStr.c_str());
			downloader->SetInfoName(extName.c_str());

			WebDownloader::EStatus status = downloader->DownloadWithRedirects(5);
			downloader.reset();

			return std::make_pair(status, std::move(tmpFileName));
		}

		void ForEach(std::function<void(const ExtensionPtr)> callback) const
		{
			std::shared_lock<std::shared_mutex> lock{ m_mutex };

			std::for_each(std::cbegin(m_extensions), std::cend(m_extensions), callback);
		}

		std::optional<ExtensionPtr> 
		FindIf(std::function<bool(ExtensionPtr)> comparator) const
		{
			std::shared_lock<std::shared_mutex> lock{ m_mutex };

			auto found = std::find_if(std::cbegin(m_extensions), std::cend(m_extensions), comparator);
			if (found != std::cend(m_extensions))
			{
				return *found;
			}

			return std::nullopt;
		}

	private:
		void CreateTasks() const
		{
			for (const auto& extension : m_extensions)
			{
				if (extension->GetSchedulerScript() &&
					!Util::EmptyStr(extension->GetTaskTime()) &&
					nzbget_rs_ext_is_in_list(g_Options->GetExtensions(), extension->GetName()) != 0)
				{
					g_Options->CreateSchedulerTask(
						0,
						extension->GetTaskTime(),
						nullptr,
						Options::scScript,
						extension->GetName()
					);
				}
			}
		}

		Extensions::const_iterator GetByName(const std::string& name) const
		{
			return std::find_if(
				std::begin(m_extensions),
				std::end(m_extensions),
				[&name](const auto& ext)
				{
					return ext->GetName() == name;
				}
			);
		}

		std::optional<std::string>
		GetExtensionPath(const Extension::Script& extension, fs::path& path)
		{
			const char* location = extension.GetLocation() ? extension.GetLocation() : "";
			const char* entry = extension.GetEntry() ? extension.GetEntry() : "";

			std::vector<const char*> allLocs;
			std::vector<size_t> allLens;
			allLocs.reserve(m_extensions.size());
			allLens.reserve(m_extensions.size());
			for (const auto& ext : m_extensions)
			{
				const char* l = ext->GetLocation() ? ext->GetLocation() : "";
				allLocs.push_back(l);
				allLens.push_back(strlen(l));
			}

			NzbgetRsBuf buf = nzbget_rs_ext_mgr_resolve_extension_path(
				location, strlen(location),
				entry, strlen(entry),
				allLocs.empty() ? nullptr : allLocs.data(),
				allLens.empty() ? nullptr : allLens.data(),
				allLocs.size());
			path = fs::path(std::string(buf.data ? buf.data : "", buf.len));
			nzbget_rs_free(buf);
			return std::nullopt;
		}

		Extensions m_extensions;
		mutable std::shared_mutex m_mutex;
	};
}

extern ExtensionManager::Manager* g_ExtensionManager;

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef SCRIPTCONTROLLER_H
#define SCRIPTCONTROLLER_H

#include <atomic>

class EnvironmentStrings
{
public:
	void Clear();
	void InitFromCurrentProcess();
	void Append(const char* envstr);
	void Append(CString&& envstr);
	std::vector<char*> GetStrings();

private:
	typedef std::vector<CString> Strings;

	Strings m_strings;
};

class ScriptController
{
public:
	typedef std::vector<CString> ArgList;

	ScriptController();
	virtual ~ScriptController();
	int Execute();
	void Terminate();
	bool Break();
	void Resume();
	void Detach();
	static void TerminateAll();

	const char* GetScript() { return !m_args.empty() ? *m_args[0] : nullptr; }
	void SetWorkingDir(const char* workingDir) { m_workingDir = workingDir; }
	void SetArgs(ArgList&& args) { m_args = std::move(args); }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
	const char* GetInfoName() { return m_infoName; }
	void SetLogPrefix(const char* logPrefix) { m_logPrefix = logPrefix; }
	void SetEnvVar(const char* name, const char* value);
	void SetEnvVarSpecial(const char* prefix, const char* name, const char* value);
	void SetIntEnvVar(const char* name, int value);
	bool IsTerminated() const { return m_terminated; }
	bool HasActiveProcess();

protected:
	void ProcessOutput(char* text);
	virtual bool ReadLine(char* buf, int bufSize, FILE* stream);
	void PrintMessage(Message::EKind kind, const char* format, ...) PRINTF_SYNTAX(3);
	virtual void AddMessage(Message::EKind kind, const char* text);
	bool GetTerminated() { return m_terminated; }
	void ResetEnv();
	void PrepareEnvOptions(const char* stripPrefix);
	void PrepareArgs();
	virtual const char* GetOptValue([[maybe_unused]] const char* name, const char* value) { return value; }
	void StartProcess(int* pipein, int* pipeout);
	int WaitProcess();
	void SetNeedWrite(bool needWrite) { m_needWrite = needWrite; }
	void Write(const char* str);
	void UnregisterRunningScript();

private:
	void CheckEnvSize(const std::vector<char*>& envs);

	ArgList m_args;
	ArgList m_cmdArgs;
	const char* m_workingDir = nullptr;
	CString m_infoName;
	const char* m_logPrefix = nullptr;
	EnvironmentStrings m_environmentStrings;
	std::atomic<bool> m_terminated{false};
	std::atomic<bool> m_completed{false};
	std::atomic<bool> m_detached{false};
	std::atomic<bool> m_needWrite{false};
	// taken with exchange(): Detach() from another thread and Execute() each
	// close it, and only the one that gets it may
	std::atomic<FILE*> m_readpipe{nullptr};
	FILE* m_writepipe = 0;
	char m_cmdLine[2048];
	std::atomic<pid_t> m_processId{0};

	inline static const long ARGS_LIMIT_SIZE = []() -> long
	{
		return sysconf(_SC_ARG_MAX);
	}();

	typedef std::vector<ScriptController*> RunningScripts;

	inline static RunningScripts m_runningScripts{};
	inline static Mutex m_runningMutex{};
};

#include "nzbget_rs.h"

// System global variable holding environments variables
extern char** environ;
extern char* (*g_EnvironmentVariables)[];

inline constexpr int FORK_ERROR_EXIT_CODE = 254;

#ifdef CHILD_WATCHDOG
class ChildWatchDog : public Thread
{
public:
	void SetProcessId(pid_t processId) { m_processId = processId; }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
protected:
	virtual void Run();
private:
	pid_t m_processId;
	CString m_infoName;
};

inline void ChildWatchDog::Run()
{
	static const int WAIT_SECONDS = 60;
	time_t start = Util::CurrentTime();
	while (!IsStopped() && (Util::CurrentTime() - start) < WAIT_SECONDS)
	{
		Util::Sleep(10);
	}

	if (!IsStopped())
	{
		info("Restarting hanging child process for %s", *m_infoName);
		kill(m_processId, SIGKILL);
	}
}
#endif

inline void EnvironmentStrings::Clear()
{
	m_strings.clear();
}

inline void EnvironmentStrings::InitFromCurrentProcess()
{
	if (!g_EnvironmentVariables)
		return;

	for (int i = 0; (*g_EnvironmentVariables)[i]; i++)
	{
		char* var = (*g_EnvironmentVariables)[i];
		// Ignore all env vars set by NZBGet.
		// This is to avoid the passing of env vars after program update (when NZBGet is
		// started from a script which was started by a previous instance of NZBGet).
		// Format: NZBXX_YYYY (XX are any two characters, YYYY are any number of any characters).
		if (!nzbget_rs_sus_is_nzbget_internal_env_var(var))
		{
			Append(var);
		}
	}
}

inline void EnvironmentStrings::Append(const char* envstr)
{
	m_strings.emplace_back(envstr);
}

inline void EnvironmentStrings::Append(CString&& envstr)
{
	m_strings.push_back(std::move(envstr));
}

/*
 * Returns environment block in format suitable for using with execve.
 */
inline std::vector<char*> EnvironmentStrings::GetStrings()
{
	std::vector<char*> strings;
	strings.reserve(m_strings.size() + 1);
	std::copy(m_strings.begin(), m_strings.end(), std::back_inserter(strings));
	strings.push_back(nullptr);
	return strings;
}

inline ScriptController::ScriptController()
{
	ResetEnv();

	Guard guard(m_runningMutex);
	m_runningScripts.push_back(this);
}

inline ScriptController::~ScriptController()
{
	UnregisterRunningScript();
}

inline void ScriptController::UnregisterRunningScript()
{
	Guard guard(m_runningMutex);
	m_runningScripts.erase(std::remove(m_runningScripts.begin(), m_runningScripts.end(), this), m_runningScripts.end());
}

inline void ScriptController::ResetEnv()
{
	m_environmentStrings.Clear();
	m_environmentStrings.InitFromCurrentProcess();
}

inline void ScriptController::SetEnvVar(const char* name, const char* value)
{
	NzbgetRsBuf envBuf = nzbget_rs_dqh_script_ctrl_format_env_assignment(name, value);
	m_environmentStrings.Append(CString(envBuf.data ? envBuf.data : "", (int)envBuf.len));
	nzbget_rs_free(envBuf);
}

inline void ScriptController::SetIntEnvVar(const char* name, int value)
{
	NzbgetRsBuf valBuf = nzbget_rs_dqh_conn_format_port_str(value);
	SetEnvVar(name, valBuf.data ? valBuf.data : "0");
	nzbget_rs_free(valBuf);
}

/**
 * If szStripPrefix is not nullptr, only options, whose names start with the prefix
 * are processed. The prefix is then stripped from the names.
 * If szStripPrefix is nullptr, all options are processed; without stripping.
 */
inline void ScriptController::PrepareEnvOptions(const char* stripPrefix)
{
	int prefixLen = stripPrefix ? strlen(stripPrefix) : 0;

	for (Options::OptEntry& optEntry : g_Options->GuardOptEntries())
	{
		const char* value = GetOptValue(optEntry.GetName(), optEntry.GetValue());
		if (nzbget_rs_ufs_script_ctrl_matches_env_prefix(optEntry.GetName(), stripPrefix, prefixLen) != 0)
		{
			SetEnvVarSpecial("NZBPO", optEntry.GetName() + prefixLen, value);
		}
		else if (!stripPrefix)
		{
			SetEnvVarSpecial("NZBOP", optEntry.GetName(), value);
		}
	}
}

inline void ScriptController::SetEnvVarSpecial(const char* prefix, const char* name, const char* value)
{
	NzbgetRsBuf varBuf = nzbget_rs_dqh_script_ctrl_format_prefixed_varname(prefix, name);
	BString<1024> varname;
	varname.Set(varBuf.data ? varBuf.data : "", (int)varBuf.len);
	nzbget_rs_free(varBuf);

	// Original name
	SetEnvVar(varname, value);

	NzbgetRsBuf normBuf = nzbget_rs_sus_normalize_special_env_var_name(varname);
	if (normBuf.data && nzbget_rs_dqh_bytes_equal(varname, normBuf.data) == 0)
	{
		SetEnvVar(normBuf.data, value);
	}
	nzbget_rs_free(normBuf);
}

inline void ScriptController::PrepareArgs()
{
	*m_cmdLine = '\0';
	m_cmdArgs.clear();

	if (m_args.size() == 1)
	{
		const auto found = Util::FindExecutorProgram(m_args[0].Str(), g_Options->GetShellOverride());
		int useFound = 0, prependArg = 0;
		nzbget_rs_tpr_script_resolve_single_arg_executor(
			found.has_value() ? 1 : 0,
			found ? found.value().c_str() : nullptr,
			m_args[0].Str(),
			&useFound, &prependArg);
		strncpy(m_cmdLine, useFound ? found.value().c_str() : (const char*)m_args[0], sizeof(m_cmdLine) - 1);
		// an interpreter goes before the script; a script run directly is
		// argv[0] already (m_args), not its own first argument too
		if (prependArg)
		{
			m_cmdArgs.emplace_back(found.value().c_str());
		}

		debug("CmdLine: %s", m_cmdLine);
	}
	else
	{
		strncpy(m_cmdLine, m_args[0], sizeof(m_cmdLine) - 1);
	}
}

inline int ScriptController::Execute()
{
	PrepareEnvOptions(nullptr);
	PrepareArgs();

	m_completed = false;
	m_processId = 0;
	if (m_terminated)
	{
		m_completed = true;
		return -1;
	}
	int exitCode = 0;

#ifdef CHILD_WATCHDOG
	bool childConfirmed = false;
	while (!childConfirmed && !m_terminated)
	{
#endif

	int pipein = -1, pipeout = -1;
	StartProcess(&pipein, &pipeout);
	if (pipein == -1)
	{
		m_completed = true;
		return -1;
	}

	// open the read end
	m_readpipe = fdopen(pipein, "r");
	if (!m_readpipe)
	{
		PrintMessage(Message::mkError, "Could not open read pipe to %s", *m_infoName);
		close(pipein);
		if (pipeout != -1)
		{
			close(pipeout);
		}
		m_completed = true;
		return -1;
	}

	m_writepipe = 0;
	if (m_needWrite)
	{
		// open the write end
		m_writepipe = fdopen(pipeout, "w");
		if (!m_writepipe)
		{
			PrintMessage(Message::mkError, "Could not open write pipe to %s", *m_infoName);
			fclose(m_readpipe.exchange(nullptr));
			close(pipeout);
			m_completed = true;
			return -1;
		}
	}

#ifdef CHILD_WATCHDOG
	debug("Creating child watchdog");
	ChildWatchDog watchDog;
	watchDog.SetAutoDestroy(false);
	watchDog.SetProcessId(m_processId);
	watchDog.SetInfoName(m_infoName);
	watchDog.Start();
#endif

	CharBuffer buf(1024 * 10);

	debug("Entering pipe-loop");
	bool firstLine = true;
	bool startError = false;
	while (!m_terminated && !m_detached)
	{
		FILE* readpipe = m_readpipe;
		if (!readpipe || feof(readpipe))
		{
			break;
		}
		if (ReadLine(buf, buf.Size(), readpipe) && m_readpipe)
		{
#ifdef CHILD_WATCHDOG
			if (!childConfirmed)
			{
				childConfirmed = true;
				watchDog.Stop();
				debug("Child confirmed");
				continue;
			}
#endif
			if (nzbget_rs_ufs_script_ctrl_is_fork_start_error_line(firstLine ? 1 : 0, buf) != 0)
			{
				startError = true;
			}
			ProcessOutput(buf);
			firstLine = false;
		}
	}
	debug("Exited pipe-loop");

#ifdef CHILD_WATCHDOG
	debug("Destroying WatchDog");
	if (!childConfirmed)
	{
		watchDog.Stop();
	}
	while (watchDog.IsRunning())
	{
		Util::Sleep(5);
	}
#endif

	if (FILE* readpipe = m_readpipe.exchange(nullptr))
	{
		fclose(readpipe);
	}

	if (m_writepipe)
	{
		fclose(m_writepipe);
	}

	if (m_terminated && m_infoName)
	{
		warn("Interrupted %s", *m_infoName);
	}

	exitCode = 0;

	if (!m_detached)
	{
		exitCode = WaitProcess();
		exitCode = nzbget_rs_ufs_script_ctrl_normalize_fork_exit_code(
			exitCode, FORK_ERROR_EXIT_CODE, startError ? 1 : 0);
	}

#ifdef CHILD_WATCHDOG
	}	// while (!bChildConfirmed && !m_bTerminated)
#endif

	debug("Exit code %i", exitCode);
	m_completed = true;
	m_processId = 0;
	return exitCode;
}

/*
* Returns file descriptor of the read-pipe or -1 on error.
*/
inline void ScriptController::StartProcess(int* pipein, int* pipeout)
{
	CString workingDir = m_workingDir;
	if (workingDir.Empty())
	{
		workingDir = FileSystem::GetCurrentDirectory();
	}

	const char* script = m_args[0];

	int pin[] = {-1, -1};
	int pout[] = {-1, -1};

	// create the pipes, close-on-exec: a script another thread starts at the
	// same time would otherwise inherit our write end and keep it open for as
	// long as it runs - we'd see no end of output until that script ended too.
	// dup2() onto the child's stdin/stdout/stderr clears the flag there.
	auto makePipe = [](int fds[2])
	{
#ifdef __linux__
		return pipe2(fds, O_CLOEXEC);
#else
		if (pipe(fds))
		{
			return -1;
		}
		fcntl(fds[0], F_SETFD, FD_CLOEXEC);
		fcntl(fds[1], F_SETFD, FD_CLOEXEC);
		return 0;
#endif
	};
	if (makePipe(pin))
	{
		PrintMessage(Message::mkError, "Could not open read pipe: errno %i", errno);
		return;
	}
	if (m_needWrite && makePipe(pout))
	{
		PrintMessage(Message::mkError, "Could not open write pipe: errno %i", errno);
		close(pin[0]);
		close(pin[1]);
		return;
	}

	std::vector<char*> environmentStrings = m_environmentStrings.GetStrings();
	char** envdata = environmentStrings.data();

	CheckEnvSize(environmentStrings);

	std::copy(std::begin(m_args), std::end(m_args), std::back_inserter(m_cmdArgs));
	m_cmdArgs.emplace_back(nullptr);
	char* const* argdata = (char* const*)m_cmdArgs.data();

#ifdef DEBUG
	debug("Starting  process: %s", script);
	for (const char* arg : m_args)
	{
		debug("arg: %s", arg);
	}
#endif

	debug("forking");
	pid_t pid = fork();

	if (pid == -1)
	{
		PrintMessage(Message::mkError, "Could not start %s: errno %i", *m_infoName, errno);
		close(pin[0]);
		close(pin[1]);
		if (m_needWrite)
		{
			close(pout[0]);
			close(pout[1]);
		}
		return;
	}
	else if (pid == 0)
	{
		// here goes the second instance

		// only certain functions may be used here or the program may hang.
		// for a list of functions see chapter "async-signal-safe functions" in
		// http://man7.org/linux/man-pages/man7/signal.7.html

		// create new process group (see Terminate() where it is used)
		setsid();

		// make the pipeout to be the same as stdout and stderr
		dup2(pin[1], 1);
		dup2(pin[1], 2);
		close(pin[0]);
		close(pin[1]);

		if (m_needWrite)
		{
			// make the pipein to be the same as stdin
			dup2(pout[0], 0);
			close(pout[0]);
			close(pout[1]);
		}

#ifdef CHILD_WATCHDOG
		write(1, "\n", 1);
		fsync(1);
#endif

		chdir(workingDir);
		environ = envdata;

		execvp(m_cmdLine, argdata);

		if (errno == EACCES)
		{
			write(1, "[WARNING] Fixing permissions for", 32);
			write(1, script, strlen(script));
			write(1, "\n", 1);
			fsync(1);
			FileSystem::FixExecPermission(script);
			execvp(m_cmdLine, argdata);
		}

		// NOTE: the text "[ERROR] Could not start " is checked later,
		// by changing adjust the dependent code below.
		write(1, "[ERROR] Could not start ", 24);
		write(1, script, strlen(script));
		write(1, ": ", 2);
		char* errtext = strerror(errno);
		write(1, errtext, strlen(errtext));
		write(1, "\n", 1);
		fsync(1);
		_exit(FORK_ERROR_EXIT_CODE);
	}

	// continue the first instance
	debug("forked");
	debug("Child Process-ID: %i", (int)pid);

	m_processId = pid;

	// close unused pipe ends; the caller gets the others only now: on a
	// failed fork they were closed already
	close(pin[1]);
	if (m_needWrite)
	{
		close(pout[0]);
	}
	*pipein = pin[0];
	*pipeout = pout[1];
}

inline int ScriptController::WaitProcess()
{
	int status = 0;
	pid_t res;
	do
	{
		res = waitpid(m_processId, &status, 0);
	} while (res == -1 && errno == EINTR);
	if (res == -1)
	{
		return -1;
	}
	if (WIFEXITED(status))
	{
		int exitCode = WEXITSTATUS(status);
		return exitCode;
	}
	if (WIFSIGNALED(status))
	{
		// not a success: a crashed or killed program used to read as exit
		// code 0. The shell's convention, 128 + signal.
		if (!m_terminated)
		{
			PrintMessage(Message::mkWarning, "%s was killed by signal %i", *m_infoName, WTERMSIG(status));
		}
		return nzbget_rs_ufs_script_ctrl_calc_signal_exit_code(WTERMSIG(status));
	}
	return -1;
}

inline bool ScriptController::HasActiveProcess()
{
	return m_processId.load() > 0 && !m_completed;
}

inline void ScriptController::Terminate()
{
	debug("Stopping %s", *m_infoName);
	m_terminated = true;

	pid_t childPid = m_processId.load();
	pid_t killId = (pid_t)nzbget_rs_ufs_script_ctrl_select_kill_pid(
		(int)childPid, childPid > 0 ? (int)getpgid(childPid) : 0);
	int killRes = childPid > 0 ? kill(killId, SIGKILL) : 0;
	bool ok = nzbget_rs_dqh_script_ctrl_is_terminate_ok(
		(int)childPid, killRes, m_completed ? 1 : 0) != 0;

	if (ok)
	{
		debug("Terminated %s", *m_infoName);
	}
	else
	{
		error("Could not terminate %s", *m_infoName);
	}

	debug("Stopped %s", *m_infoName);
}

inline void ScriptController::TerminateAll()
{
	Guard guard(m_runningMutex);
	for (ScriptController* script : m_runningScripts)
	{
		if (script->HasActiveProcess() && !script->m_detached)
		{
			// send break signal and wait up to 5 seconds for graceful termination
			if (script->Break())
			{
				time_t curtime = Util::CurrentTime();
				while (!script->m_completed && std::abs(curtime - Util::CurrentTime()) <= 10)
				{
					Util::Sleep(100);
				}
			}
			script->Terminate();
		}
	}
}

inline bool ScriptController::Break()
{
	debug("Sending break signal to %s", *m_infoName);

	pid_t pid = m_processId.load();
	bool ok = nzbget_rs_dqh_script_ctrl_can_send_break((int)pid, m_completed ? 1 : 0) != 0 &&
		kill(pid, SIGINT) == 0;

	if (ok)
	{
		debug("Sent break signal to %s", *m_infoName);
	}
	else
	{
		warn("Could not send break signal to %s", *m_infoName);
	}

	return ok;
}

inline void ScriptController::Detach()
{
	debug("Detaching %s", *m_infoName);
	m_detached = true;
	if (FILE* readpipe = m_readpipe.exchange(nullptr))
	{
		fclose(readpipe);
	}
}

inline void ScriptController::Resume()
{
	m_terminated = false;
	m_detached = false;
	m_processId = 0;
}

inline bool ScriptController::ReadLine(char* buf, int bufSize, FILE* stream)
{
	return fgets(buf, bufSize, stream);
}

inline void ScriptController::ProcessOutput(char* text)
{
	debug("Processing output received from script");

	int msgKind = 0;
	size_t msgOffset = 0;
	if (!nzbget_rs_sus_classify_script_output_line(text, &msgKind, &msgOffset))
	{
		return;
	}
	Message::EKind kind = Message::mkInfo;
	switch (msgKind)
	{
		case 1: kind = Message::mkWarning; break;
		case 2: kind = Message::mkError; break;
		case 3: kind = Message::mkDetail; break;
		case 4: kind = Message::mkDebug; break;
		default: kind = Message::mkInfo; break;
	}
	PrintMessage(kind, "%s", text + msgOffset);

	debug("Processing output received from script - completed");
}

inline void ScriptController::AddMessage(Message::EKind kind, const char* text)
{
	switch (kind)
	{
		case Message::mkDetail:
			detail("%s", text);
			break;

		case Message::mkInfo:
			info("%s", text);
			break;

		case Message::mkWarning:
			warn("%s", text);
			break;

		case Message::mkError:
			error("%s", text);
			break;

		case Message::mkDebug:
			debug("%s", text);
			break;
	}
}

inline void ScriptController::PrintMessage(Message::EKind kind, const char* format, ...)
{
	// not cut to a fixed size: subclasses parse commands ("[NZB] ...") from it
	CString tmp2;

	va_list ap;
	va_start(ap, format);
	tmp2.FormatV(format, ap);
	va_end(ap);

	if (m_logPrefix)
	{
		NzbgetRsBuf prefBuf = nzbget_rs_dqh_script_format_prefixed_message(m_logPrefix, *tmp2);
		AddMessage(kind, prefBuf.data ? prefBuf.data : "");
		nzbget_rs_free(prefBuf);
	}
	else
	{
		AddMessage(kind, tmp2);
	}
}

inline void ScriptController::Write(const char* str)
{
	if (m_writepipe && str)
	{
		fwrite(str, 1, strlen(str), m_writepipe);
		fflush(m_writepipe);
	}
}

inline void ScriptController::CheckEnvSize(const std::vector<char*>& envs)
{
	if (ARGS_LIMIT_SIZE <= 0) return;

	size_t limit = static_cast<size_t>(ARGS_LIMIT_SIZE);
	size_t exceededTotal = 0;
	if (nzbget_rs_sus_check_env_size_limit(
		reinterpret_cast<const char* const*>(envs.data()),
		envs.size(),
		sizeof(void*),
		limit,
		&exceededTotal))
	{
		PrintMessage(Message::EKind::mkWarning,
					 "Total environment size (%zu bytes) exceeds OS limit (%zu bytes): %s",
					 exceededTotal, limit, "This may cause process creation to fail");
	}
}

#endif


class NzbScriptController : public ScriptController
{
protected:
	void PrepareEnvScript(NzbParameterList* parameters, const char* scriptName)
	{
		std::vector<const char*> pnames;
		std::vector<const char*> pvals;
		if (parameters)
		{
			pnames.reserve(parameters->size());
			pvals.reserve(parameters->size());
			for (NzbParameter& parameter : parameters)
			{
				pnames.push_back(parameter.GetName());
				pvals.push_back(parameter.GetValue());
			}
		}
		nzbget_rs_nzb_script_prepare_env(
			scriptName ? scriptName : "",
			parameters != nullptr ? 1 : 0,
			pnames.data(),
			pvals.data(),
			pnames.size(),
			this,
			[](void* ctx, const char* prefix, const char* name, const char* value)
			{
				static_cast<NzbScriptController*>(ctx)->SetEnvVarSpecial(prefix, name, value);
			},
			[](void* ctx, const char* stripPrefix)
			{
				static_cast<NzbScriptController*>(ctx)->PrepareEnvOptions(stripPrefix);
			});
	}

	void ExecuteScriptList(const char* scriptList)
	{
		if (Util::EmptyStr(scriptList))
		{
			return;
		}

		Tokenizer tok(scriptList, ",;");
		while (const char* scriptName = tok.Next())
		{
			auto found = g_ExtensionManager->FindIf([&scriptName](auto script)
				{
					return strcmp(scriptName, script->GetName()) == 0;
				}
			);

			if (found)
			{
				ExecuteScript(std::move(found.value()));
			}
		}
	}

	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script) = 0;
};

#ifndef COMMANDSCRIPT_H
#define COMMANDSCRIPT_H

class CommandScriptLog
{
public:
	GuardedMessageList GuardMessages() { return GuardedMessageList(&m_messages, &m_logMutex); }

	int Reset()
	{
		Guard guard(m_logMutex);
		m_messages.clear();
		return ++m_idScriptGen;
	}

	void AddMessage(int scriptId, Message::EKind kind, const char* text)
	{
		Guard guard(m_logMutex);

		// save only messages from the last started script
		if (scriptId == m_idScriptGen)
		{
			m_messages.emplace_back(++m_idMessageGen, kind, Util::CurrentTime(), text);
		}
	}

private:
	MessageList m_messages;
	Mutex m_logMutex;
	int m_idMessageGen;
	int m_idScriptGen;
};

extern CommandScriptLog* g_CommandScriptLog;

class CommandScriptController : public Thread, public NzbScriptController
{
public:
	static constexpr int COMMANDPROCESS_SUCCESS = 93;
	static constexpr int COMMANDPROCESS_ERROR = 94;

	virtual void Run()
	{
		ExecuteScriptList(m_script);
	}

	static bool StartScript(const char* scriptName, const char* command, std::unique_ptr<Options::OptEntries> modifiedOptions)
	{
		CommandScriptController* scriptController = new CommandScriptController();
		scriptController->m_script = scriptName;
		scriptController->m_command = command;
		scriptController->m_logId = g_CommandScriptLog->Reset();
		scriptController->m_modifiedOptions = std::move(modifiedOptions);

		scriptController->SetAutoDestroy(true);

		scriptController->Start();

		auto found = g_ExtensionManager->FindIf([&](auto script)
			{
				return strcmp(scriptName, script->GetName()) == 0;
			}
		);

		return found.has_value();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		PrintMessage(Message::mkInfo, "Executing script %s with command %s", script->GetName(), *m_command);

		SetArgs({script->GetEntry()});

		BString<1024> infoName("script %s with command %s", script->GetName(), *m_command);
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		PrepareParams(script->GetName());

		int exitCode = Execute();

		infoName[0] = 'S';
		SetLogPrefix(nullptr);

		switch (exitCode)
		{
			case COMMANDPROCESS_SUCCESS:
				PrintMessage(Message::mkInfo, "%s successful", *infoName);
				break;

			case COMMANDPROCESS_ERROR:
			case -1:
				PrintMessage(Message::mkError, "%s failed", *infoName);
				break;

			default:
				PrintMessage(Message::mkError, "%s failed (terminated with unknown status)", *infoName);
				break;
		}
	}

	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		NzbScriptController::AddMessage(kind, text);
		g_CommandScriptLog->AddMessage(m_logId, kind, text);
	}

	virtual const char* GetOptValue(const char* name, const char* value)
	{
		Options::OptEntry* entry = m_modifiedOptions->FindOption(name);
		return entry ? entry->GetValue() : value;
	}

private:
	CString m_script;
	CString m_command;
	int m_logId;
	std::unique_ptr<Options::OptEntries> m_modifiedOptions;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		SetEnvVar("NZBCP_COMMAND", m_command);

		PrepareEnvScript(nullptr, scriptName);
	}
};

#endif


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef QUEUESCRIPT_H
#define QUEUESCRIPT_H


class QueueScriptCoordinator
{
public:
	enum EEvent
	{
		qeFileDownloaded, // lowest priority
		qeUrlCompleted,
		qeNzbMarked,
		qeNzbAdded,
		qeNzbNamed,
		qeNzbDownloaded,
		qeNzbDeleted // highest priority
	};

	void Stop() { m_stopped = true; }
	void InitOptions();
	void EnqueueScript(NzbInfo* nzbInfo, EEvent event);
	void CheckQueue();
	bool HasJob(int nzbId, bool* active);
	int GetQueueSize();
	static NzbInfo* FindNzbInfo(DownloadQueue* downloadQueue, int nzbId);

private:
	class QueueItem
	{
	public:
		QueueItem(int nzbId, std::shared_ptr<const Extension::Script> script, EEvent event) :
			m_nzbId(nzbId), m_script(std::move(script)), m_event(event) {}
		int GetNzbId() { return m_nzbId; }
		const std::shared_ptr<const Extension::Script>& GetScript() const { return m_script; }
		EEvent GetEvent() { return m_event; }
	private:
		int m_nzbId;
		std::shared_ptr<const Extension::Script> m_script;
		EEvent m_event;
	};

	typedef std::deque<std::unique_ptr<QueueItem>> Queue;

	Queue m_queue;
	Mutex m_queueMutex;
	std::unique_ptr<QueueItem> m_curItem;
	bool m_hasQueueScripts = false;
	bool m_stopped = false;

	bool UsableScript(std::shared_ptr<const Extension::Script> script, NzbInfo* nzbInfo, EEvent event);
};

extern QueueScriptCoordinator* g_QueueScriptCoordinator;

#include <algorithm>
#include "nzbget_rs.h"

class QueueScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run() override;
	static void StartScript(NzbInfo* nzbInfo, std::shared_ptr<const Extension::Script> script, QueueScriptCoordinator::EEvent event);

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script) override;
	virtual void AddMessage(Message::EKind kind, const char* text) override;

private:
	CString m_nzbName;
	CString m_nzbFilename;
	CString m_url;
	CString m_category;
	CString m_destDir;
	CString m_queuedFilename;
	int m_id;
	int m_priority;
	CString m_dupeKey;
	EDupeMode m_dupeMode;
	int m_dupeScore;
	NzbParameterList m_parameters;
	int m_prefixLen;
	std::shared_ptr<const Extension::Script> m_script;
	QueueScriptCoordinator::EEvent m_event;
	bool m_markBad;
	NzbInfo::EDeleteStatus m_deleteStatus;
	NzbInfo::EUrlStatus m_urlStatus;
	NzbInfo::EMarkStatus m_markStatus;

	void PrepareParams(const char* scriptName);
};

inline void QueueScriptController::StartScript(NzbInfo* nzbInfo, std::shared_ptr<const Extension::Script> script, QueueScriptCoordinator::EEvent event)
{
	if (!nzbInfo || nzbInfo->GetSkipScriptProcessing())
	{
		return;
	}

	QueueScriptController* scriptController = new QueueScriptController();

	scriptController->m_nzbName = nzbInfo->GetName();
	scriptController->m_nzbFilename = nzbInfo->GetFilename();
	scriptController->m_url = nzbInfo->GetUrl();
	scriptController->m_category = nzbInfo->GetCategory();
	scriptController->m_destDir = nzbInfo->GetDestDir();
	scriptController->m_queuedFilename = nzbInfo->GetQueuedFilename();
	scriptController->m_id = nzbInfo->GetId();
	scriptController->m_priority = nzbInfo->GetPriority();
	scriptController->m_dupeKey = nzbInfo->GetDupeKey();
	scriptController->m_dupeMode = nzbInfo->GetDupeMode();
	scriptController->m_dupeScore = nzbInfo->GetDupeScore();
	scriptController->m_parameters.CopyFrom(nzbInfo->GetParameters());
	scriptController->m_script = std::move(script);
	scriptController->m_event = event;
	scriptController->m_prefixLen = 0;
	scriptController->m_markBad = false;
	scriptController->m_deleteStatus = nzbInfo->GetDeleteStatus();
	scriptController->m_urlStatus = nzbInfo->GetUrlStatus();
	scriptController->m_markStatus = nzbInfo->GetMarkStatus();
	scriptController->SetAutoDestroy(true);

	scriptController->Start();
}

inline void QueueScriptController::Run()
{
	ExecuteScript(m_script);

	SetLogPrefix(nullptr);

	if (m_markBad)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_id);
		if (nzbInfo)
		{
			nzbInfo->PrintMessage(Message::mkWarning, "Cancelling download and deleting %s", *m_nzbName);
			nzbInfo->SetDeleteStatus(NzbInfo::dsBad);
			downloadQueue->EditEntry(m_id, DownloadQueue::eaGroupDelete, nullptr);
		}
	}

	g_QueueScriptCoordinator->CheckQueue();
}

inline void QueueScriptController::ExecuteScript(std::shared_ptr<const Extension::Script> script)
{
	PrintMessage(m_event == QueueScriptCoordinator::qeFileDownloaded ? Message::mkDetail : Message::mkInfo,
		"Executing queue-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbName));

	SetArgs({script->GetEntry()});

	BString<1024> infoName("queue-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbName));
	SetInfoName(infoName);

	SetLogPrefix(script->GetDisplayName());
	m_prefixLen = static_cast<int>(strlen(script->GetDisplayName())) + 2; // 2 = strlen(": ");
	PrepareParams(script->GetName());

	Execute();

	SetLogPrefix(nullptr);
}

inline void QueueScriptController::PrepareParams(const char* scriptName)
{
	ResetEnv();

	NzbgetRsQueueScriptEnv env{};
	env.nzb_name = m_nzbName;
	env.nzb_id = m_id;
	env.filename = m_nzbFilename;
	env.dest_dir = m_destDir;
	env.queued_filename = m_queuedFilename;
	env.url = m_url;
	env.category = m_category;
	env.priority = m_priority;
	env.dupe_key = m_dupeKey;
	env.dupe_score = m_dupeScore;
	env.dupe_mode = static_cast<int>(m_dupeMode);
	env.event = static_cast<int>(m_event);
	env.delete_status = static_cast<int>(m_deleteStatus);
	env.url_status = static_cast<int>(m_urlStatus);
	env.mark_status = static_cast<int>(m_markStatus);

	nzbget_rs_queue_script_prepare_env(
		&env,
		this,
		[](void* ctx, const char* name, const char* value)
		{
			static_cast<QueueScriptController*>(ctx)->SetEnvVar(name, value);
		},
		[](void* ctx, const char* name, int value)
		{
			static_cast<QueueScriptController*>(ctx)->SetIntEnvVar(name, value);
		});

	PrepareEnvScript(&m_parameters, scriptName);
}

inline void QueueScriptController::AddMessage(Message::EKind kind, const char* text)
{
	const char* msgText = text + m_prefixLen;

	NzbgetRsQueueScriptCmd cmd{};
	int cmdKind = nzbget_rs_sus_parse_queue_script_cmd(
		msgText,
		static_cast<int>(m_event),
		static_cast<int>(QueueScriptCoordinator::qeNzbDownloaded),
		&cmd);
	if (cmdKind != 0)
	{
		debug("Command %s detected", msgText + 6);
		if (cmdKind == 1)
		{
			std::string param(msgText + cmd.key_offset, cmd.key_len);
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->GetParameters()->SetParameter(param.c_str(), msgText + cmd.val_offset);
			}
		}
		else if (cmdKind == 2)
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->SetFinalDir(msgText + cmd.val_offset);
			}
		}
		else if (cmdKind == 3)
		{
			m_markBad = true;
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->PrintMessage(Message::mkWarning, "Marking %s as bad", *m_nzbName);
				nzbInfo->SetMarkStatus(NzbInfo::ksBad);
			}
		}
		else
		{
			error("Invalid command \"%s\" received from %s", msgText, GetInfoName());
		}
		return;
	}
	{
		NzbInfo* nzbInfo = nullptr;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->AddMessage(kind, text);
			}
		}

		if (!nzbInfo)
		{
			ScriptController::AddMessage(kind, text);
		}
	}
}

inline void QueueScriptCoordinator::InitOptions()
{
	auto found = g_ExtensionManager->FindIf([&](auto script)
		{
			return script->GetQueueScript() == true;
		}
	);

	m_hasQueueScripts = found.has_value();
}

inline void QueueScriptCoordinator::EnqueueScript(NzbInfo* nzbInfo, EEvent event)
{
	if (!m_hasQueueScripts || !nzbInfo || nzbInfo->GetSkipScriptProcessing())
	{
		return;
	}

	Guard guard(m_queueMutex);

	if (event == qeNzbDownloaded)
	{
		m_queue.erase(std::remove_if(m_queue.begin(), m_queue.end(),
			[nzbInfo](std::unique_ptr<QueueItem>& queueItem)
			{
				return queueItem->GetNzbId() == nzbInfo->GetId();
			}),
			m_queue.end());
	}

	time_t curTime = Util::CurrentTime();
	if (nzbget_rs_sus_should_skip_queue_event_interval(
		static_cast<int>(event),
		static_cast<int>(qeFileDownloaded),
		g_Options->GetEventInterval(),
		static_cast<int64>(curTime),
		static_cast<int64>(nzbInfo->GetQueueScriptTime())))
	{
		return;
	}

	g_ExtensionManager->ForEach([&](auto script)
		{
			if (UsableScript(script, nzbInfo, event))
			{
				bool alreadyQueued = false;
				if (event == qeFileDownloaded)
				{
					for (QueueItem* queueItem : &m_queue)
					{
						if (queueItem->GetNzbId() == nzbInfo->GetId() && queueItem->GetScript() == script)
						{
							alreadyQueued = true;
							break;
						}
					}
				}

				if (!alreadyQueued)
				{
					std::unique_ptr<QueueItem> queueItem = std::make_unique<QueueItem>(nzbInfo->GetId(), script, event);
					if (m_curItem)
					{
						m_queue.push_back(std::move(queueItem));
					}
					else
					{
						m_curItem = std::move(queueItem);
						QueueScriptController::StartScript(nzbInfo, m_curItem->GetScript(), m_curItem->GetEvent());
					}
				}

				nzbInfo->SetQueueScriptTime(Util::CurrentTime());
			}
		}
	);
}

inline bool QueueScriptCoordinator::UsableScript(std::shared_ptr<const Extension::Script> script, NzbInfo* nzbInfo, EEvent event)
{
	size_t pcount = nzbInfo->GetParameters()->size();
	std::vector<const char*> pnames;
	std::vector<const char*> pvals;
	pnames.reserve(pcount);
	pvals.reserve(pcount);
	for (NzbParameter& parameter : nzbInfo->GetParameters())
	{
		pnames.push_back(parameter.GetName());
		pvals.push_back(parameter.GetValue());
	}
	const char* postScript = nullptr;
	if (event == qeUrlCompleted)
	{
		postScript = g_Options->GetExtensions();
		if (!Util::EmptyStr(nzbInfo->GetCategory()))
		{
			Options::Category* categoryObj = g_Options->FindCategory(nzbInfo->GetCategory(), false);
			if (categoryObj && !Util::EmptyStr(categoryObj->GetExtensions()))
			{
				postScript = categoryObj->GetExtensions();
			}
		}
	}
	return nzbget_rs_sus_is_usable_queue_script(
		script->GetQueueScript() ? 1 : 0,
		script->GetQueueEvents(),
		script->GetName(),
		static_cast<int>(event),
		static_cast<int>(qeUrlCompleted),
		pnames.data(),
		pvals.data(),
		pcount,
		postScript) != 0;
}

inline NzbInfo* QueueScriptCoordinator::FindNzbInfo(DownloadQueue* downloadQueue, int nzbId)
{
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(nzbId);
	if (nzbInfo)
	{
		return nzbInfo;
	}

	HistoryInfo* historyInfo = downloadQueue->GetHistory()->Find(nzbId);
	if (historyInfo)
	{
		return historyInfo->GetNzbInfo();
	}

	return nullptr;
}

inline void QueueScriptCoordinator::CheckQueue()
{
	if (m_stopped)
	{
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	Guard guard(m_queueMutex);

	m_curItem.reset();
	NzbInfo* curNzbInfo = nullptr;
	EEvent maxEvent = EEvent::qeFileDownloaded;
	Queue::iterator itCurItem;

	for (Queue::iterator it = m_queue.begin(); it != m_queue.end(); )
	{
		std::unique_ptr<QueueItem>& queueItem = *it;

		NzbInfo* nzbInfo = FindNzbInfo(downloadQueue, queueItem->GetNzbId());

		EEvent event = queueItem->GetEvent();
		bool ignoreEvent = nzbget_rs_sus_should_ignore_queued_script_event(
			nzbInfo ? 1 : 0,
			nzbInfo ? static_cast<int>(nzbInfo->GetDeleteStatus()) : 0,
			static_cast<int>(NzbInfo::dsNone),
			nzbInfo ? static_cast<int>(nzbInfo->GetMarkStatus()) : 0,
			static_cast<int>(NzbInfo::ksBad),
			static_cast<int>(event),
			static_cast<int>(qeNzbDeleted),
			static_cast<int>(qeNzbMarked)) != 0;

		if (ignoreEvent)
		{
			it = m_queue.erase(it);
			if (curNzbInfo)
			{
				curNzbInfo = nullptr;
				it = m_queue.begin();
			}
			continue;
		}

		if (!curNzbInfo || queueItem->GetEvent() > maxEvent)
		{
			itCurItem = it;
			curNzbInfo = nzbInfo;
			maxEvent = queueItem->GetEvent();
		}

		++it;
	}

	if (curNzbInfo)
	{
		m_curItem = std::move(*itCurItem);
		m_queue.erase(itCurItem);
		QueueScriptController::StartScript(curNzbInfo, m_curItem->GetScript(), m_curItem->GetEvent());
	}
}

inline bool QueueScriptCoordinator::HasJob(int nzbId, bool* active)
{
	Guard guard(m_queueMutex);

	bool working = m_curItem && m_curItem->GetNzbId() == nzbId;
	if (active)
	{
		*active = working;
	}
	if (!working)
	{
		for (QueueItem* queueItem : &m_queue)
		{
			working = queueItem->GetNzbId() == nzbId;
			if (working)
			{
				break;
			}
		}
	}

	return working;
}

inline int QueueScriptCoordinator::GetQueueSize()
{
	Guard guard(m_queueMutex);

	return static_cast<int>(m_queue.size()) + (m_curItem ? 1 : 0);
}

#endif

#endif

#ifndef DISABLE_PARCHECK
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef PARCHECKER_H
#define PARCHECKER_H

#ifndef DISABLE_PARCHECK

#include <atomic>
#include <deque>
#include <memory>
#include <mutex>
#include <string>
#include <vector>


class Repairer;

class ParChecker
{
public:
	virtual ~ParChecker() = default;

	enum EStatus
	{
		psFailed,
		psRepairPossible,
		psRepaired,
		psRepairNotNeeded
	};

	enum EStage
	{
		ptLoadingPars,
		ptVerifyingSources,
		ptRepairing,
		ptVerifyingRepaired,
	};

	void Execute();
	void SetDestDir(const char* destDir) { m_destDir = destDir ? destDir : ""; }
	const char* GetDestDir() { return m_destDir.c_str(); }
	const char* GetParFilename() { return m_parFilename.c_str(); }
	const char* GetInfoName() { return m_infoName.c_str(); }
	void SetInfoName(const char* infoName) { m_infoName = infoName ? infoName : ""; }
	void SetNzbName(const char* nzbName) { m_nzbName = nzbName ? nzbName : ""; }
	void SetParQuick(bool parQuick) { m_parQuick = parQuick; }
	bool GetParQuick() { return m_parQuick; }
	void SetForceRepair(bool forceRepair) { m_forceRepair = forceRepair; }
	bool GetForceRepair() { return m_forceRepair; }
	void SetParFull(bool parFull) { m_parFull = parFull; }
	bool GetParFull() { return m_parFull; }
	EStatus GetStatus() { return m_status; }
	void AddParFile(const char* parFilename);
	void QueueChanged();
	void Cancel();

protected:
	class Segment
	{
	public:
		Segment(bool success, int64 offset, int size, uint32 crc) 
			: m_success(success)
			, m_offset(offset)
			, m_size(size)
			, m_crc(crc) 
		{}
		bool GetSuccess() { return m_success; }
		int64 GetOffset() { return m_offset; }
		int GetSize() { return m_size; }
		uint32 GetCrc() { return m_crc; }

	private:
		bool m_success;
		int64 m_offset;
		int m_size;
		uint32 m_crc;
	};

	typedef std::deque<Segment> SegmentList;

	class DupeSource
	{
	public:
		DupeSource(int id, const char* directory) 
			: m_id(id)
			, m_directory(directory ? directory : "") 
		{}
		int GetId() { return m_id; }
		const char* GetDirectory() { return m_directory.c_str(); }
		int GetUsedBlocks() { return m_usedBlocks; }
		void SetUsedBlocks(int usedBlocks) { m_usedBlocks = usedBlocks; }

	private:
		int m_id;
		int m_usedBlocks = 0;
		std::string m_directory;
	};

	typedef std::deque<DupeSource> DupeSourceList;

	enum EFileStatus
	{
		fsUnknown,
		fsSuccess,
		fsPartial,
		fsFailure
	};

	/**
	* Unpause par2-files
	* returns true, if the files with required number of blocks were unpaused,
	* or false if there are no more files in queue for this collection or not enough blocks
	*/
	virtual bool RequestMorePars(int blockNeeded, int* blockFound) = 0;
	virtual void UpdateProgress() {}
	virtual bool IsStopped() { return false; }
	virtual void Completed() {}
	virtual void PrintMessage([[maybe_unused]] Message::EKind kind,
		[[maybe_unused]] const char* format, ...) PRINTF_SYNTAX(3) {}
	virtual void RegisterParredFile([[maybe_unused]] const char* filename) {}
	virtual void RegisterVerifiedFile([[maybe_unused]] const char* filename) {}
	virtual bool IsParredFile([[maybe_unused]] const char* filename) { return false; }
	virtual EFileStatus FindFileCrc([[maybe_unused]] const char* filename, [[maybe_unused]] uint32* crc,
		[[maybe_unused]] SegmentList* segments) { return fsUnknown; }
	virtual const char* FindFileOrigname([[maybe_unused]] const char* filename) { return nullptr; }
	virtual void RequestDupeSources([[maybe_unused]] DupeSourceList* dupeSourceList) {}
	virtual void StatDupeSources([[maybe_unused]] DupeSourceList* dupeSourceList) {}
	/* when its own par2-files can't repair the set: the smallest par2-file of a twin
	 * (a duplicate of byte-identical files, with its own par2 set) fetched into the
	 * destination; its path, or "" for none */
	virtual std::string RequestTwinIndex() { return ""; }
	/* par2-volumes of that twin holding at least <blockNeeded> blocks of its set,
	 * fetched into the destination; their paths */
	virtual std::vector<std::string> RequestTwinVolumes([[maybe_unused]] int blockNeeded) { return {}; }
	EStage GetStage() { return m_stage; }
	const char* GetProgressLabel() { return m_progressLabel.c_str(); }
	int GetFileProgress() { return m_fileProgress; }
	int GetStageProgress() { return m_stageProgress; }

private:
	typedef std::deque<std::string> FileList;

	std::string m_infoName;
	std::string m_destDir;
	std::string m_nzbName;
	std::string m_parFilename;
	std::string m_progressLabel;
	std::string m_errMsg;

	int m_processedCount;
	int m_filesToRepair;
	int m_extraFiles;
	int m_fileProgress;
	int m_stageProgress;

	EStatus m_status = psFailed;

	std::atomic<EStage> m_stage{ptLoadingPars};
	std::atomic<int> m_quickFiles{0};

	FileList m_queuedParFiles;
	FileList m_processedFiles;
	std::vector<std::string> m_verifiedFiles;
	DupeSourceList m_dupeSources;
	std::mutex m_queuedParFilesMtx;
	std::mutex m_repairerMtx;
	std::mutex m_signalMtx;

	std::unique_ptr<Repairer> m_repairer;

	std::atomic<bool> m_parQuick{false};
	std::atomic<bool> m_parFull{false};

	bool m_queuedParFilesChanged;
	bool m_verifyingExtraFiles;
	bool m_cancelled;
	bool m_hasDamagedFiles;
	bool m_forceRepair = false;

	Repairer* GetRepairer() { return m_repairer.get(); }

public:
	// the par-set's block size once a par2-file is loaded, else 0
	int64 GetBlockSize();
private:

	void Cleanup();
	EStatus RunParCheckAll();
	EStatus RunParCheck(std::string parFilename);
	int PreProcessPar();
	bool LoadMainParBak();
	int ProcessMorePars();
	int ProcessTwinSet();
	bool LoadMorePars();
	bool AddSplittedFragments();
	bool AddMissingFiles();
	bool AddDupeFiles();
	bool AddExtraFiles(bool onlyMissing, bool externalDir, const char* directory);
	bool IsProcessedFile(const char* filename);
	void DeleteLeftovers();
	void SignalFilename(std::string str);
	void SignalProgress(int progress);
	void SignalDone(std::string str, int available, int total);
	void CheckEmptyFiles();
	std::string GetPacketCreator();

	friend class Repairer;
};

#endif

#ifndef DISABLE_PARCHECK

#include <vector>
#include <string>
#include <algorithm>
#include "nzbget_rs.h"

class Repairer final
{
public:
	Repairer(ParChecker* owner)
		: m_owner(owner)
	{
		int configuredParBuffer = g_Options->GetParBuffer();
		int monitorMem = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveParBufferMB(configuredParBuffer) : 0;
		nzbget_rs_parchecker_calc_repairer_resources(
			g_Options->GetParThreads(), Util::NumberOfCpuCores(),
			g_MemoryMonitor != nullptr ? 1 : 0, monitorMem, configuredParBuffer,
			&m_threadsToUse, &m_memToUse);
		m_handle = nzbget_rs_par_repairer_new(
			m_owner->GetDestDir(),
			m_threadsToUse,
			m_memToUse,
			m_owner->GetParQuick() ? 1 : 0,
			g_Options->GetParScan() == Options::psFull ? 1 : 0,
			g_Options->GetParIgnoreExt());
	}

	~Repairer()
	{
		if (m_handle)
		{
			nzbget_rs_par_repairer_free(m_handle);
			m_handle = nullptr;
		}
	}

	int PreProcess(const std::string& parFilename)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		return nzbget_rs_par_repairer_preprocess(m_handle, parFilename.c_str(), &cbs);
	}

	int Process(bool dorepair)
	{
		if (dorepair)
		{
			m_owner->PrintMessage(Message::mkInfo, "Using %i thread(s) and %i MB to repair %u block(s) for %s",
				m_threadsToUse, m_memToUse, MissingBlockCount(), m_owner->m_nzbName.c_str());
		}
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		int res = nzbget_rs_par_repairer_process(m_handle, dorepair ? 1 : 0, &cbs);
		debug("ParChecker: Process-result=%i", res);
		return res;
	}

	bool LoadPacketsFromFile(const char* filename)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		return nzbget_rs_par_repairer_load_extra_par2_file(m_handle, filename, &cbs) != 0;
	}

	bool VerifyExtraFiles(const std::vector<std::string>& extrafiles, bool isSplitFragment)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		bool anyAdded = false;
		for (const std::string& extra : extrafiles)
		{
			if (nzbget_rs_par_repairer_verify_extra_file(
				m_handle, extra.c_str(), isSplitFragment ? 1 : 0, &cbs) != 0)
			{
				anyAdded = true;
			}
		}
		return anyAdded;
	}

	void Cancel()
	{
		nzbget_rs_par_repairer_cancel(m_handle);
	}

	uint32_t MissingFileCount() const
	{
		return nzbget_rs_par_repairer_missing_file_count(m_handle);
	}

	uint32_t DamagedFileCount() const
	{
		return nzbget_rs_par_repairer_damaged_file_count(m_handle);
	}

	uint32_t MissingBlockCount() const
	{
		return nzbget_rs_par_repairer_missing_block_count(m_handle);
	}

	uint32_t RecoveryBlockCount() const
	{
		return nzbget_rs_par_repairer_recovery_block_count(m_handle);
	}

	int64_t BlockSize() const
	{
		return nzbget_rs_par_repairer_block_size(m_handle);
	}

	std::string Creator() const
	{
		NzbgetRsBuf buf = nzbget_rs_par_repairer_creator(m_handle);
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	size_t SourceFileCount() const
	{
		return nzbget_rs_par_repairer_source_file_count(m_handle);
	}

	const NzbgetRsParRepairer* Handle() const { return m_handle; }

private:
	NzbgetRsParCallbacks MakeCallbacks()
	{
		NzbgetRsParCallbacks cbs{};
		cbs.ctx = this;
		cbs.on_filename = [](void* ctx, const char* name) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalFilename(name ? name : "");
		};
		cbs.on_progress = [](void* ctx, int progress) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalProgress(progress);
		};
		cbs.on_done = [](void* ctx, const char* name, int available, int total, int /*targetExists*/, int /*isComplete*/) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalDone(name ? name : "", available, total);
		};
		cbs.on_quick_verify = [](void* ctx, const char* name, uint32_t* outCrc, const NzbgetRsParSegment** outSegs, size_t* outSegLen) -> int {
			auto* self = static_cast<Repairer*>(ctx);
			uint32 crc = 0;
			ParChecker::SegmentList segments;
			ParChecker::EFileStatus st = self->m_owner->FindFileCrc(name ? name : "", &crc, &segments);
			if (outCrc)
			{
				*outCrc = crc;
			}
			self->m_lastSegments.clear();
			for (ParChecker::Segment& seg : segments)
			{
				self->m_lastSegments.push_back({
					seg.GetSuccess() ? 1 : 0,
					seg.GetOffset(),
					seg.GetSize(),
					seg.GetCrc()
				});
			}
			if (outSegs)
			{
				*outSegs = self->m_lastSegments.empty() ? nullptr : self->m_lastSegments.data();
			}
			if (outSegLen)
			{
				*outSegLen = self->m_lastSegments.size();
			}
			return (int)st;
		};
		cbs.on_quick_verified_inc = [](void* ctx) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->m_quickFiles++;
			self->m_owner->SetParFull(false);
		};
		return cbs;
	}

	ParChecker* m_owner;
	NzbgetRsParRepairer* m_handle = nullptr;
	int m_threadsToUse = 1;
	int m_memToUse = 16;
	std::vector<NzbgetRsParSegment> m_lastSegments;

	friend class ParChecker;
};


inline void ParChecker::Cleanup()
{
	std::lock_guard guard{ m_repairerMtx };

	m_repairer.reset();
	m_queuedParFiles.clear();
	m_processedFiles.clear();
	m_dupeSources.clear();
	m_errMsg = "";

	MemoryMonitor::TrimHeap();
}

inline void ParChecker::Execute()
{
	m_verifiedFiles.clear();
	m_status = RunParCheckAll();

	if (m_status == psRepairNotNeeded && m_parQuick && m_forceRepair && !IsStopped())
	{
		PrintMessage(Message::mkInfo, "Performing full par-check for %s", m_nzbName.c_str());
		m_parQuick = false;
		m_verifiedFiles.clear();
		m_status = RunParCheckAll();
	}

	// Publish only the final pass: a forced full check can overturn a quick
	// result, and its pending duplicate jobs must survive until then.
	for (const std::string& filename : m_verifiedFiles)
	{
		RegisterVerifiedFile(filename.c_str());
	}
	Completed();
}

inline ParChecker::EStatus ParChecker::RunParCheckAll()
{
	ParParser::ParFileList fileList;
	if (!ParParser::FindMainPars(m_destDir.c_str(), &fileList))
	{
		PrintMessage(Message::mkError, "Could not start par-check for %s. Could not find any par-files", m_nzbName.c_str());
		return psFailed;
	}

	EStatus allStatus = psRepairNotNeeded;
	m_parFull = true;

	for (std::string& parFilename : fileList)
	{
		debug("Found par: %s", parFilename.c_str());

		if (!IsStopped())
		{
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				m_destDir.c_str(), PATH_SEPARATOR, parFilename.c_str());
			std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
			nzbget_rs_free(pathBuf);
			std::string fullParFilename = FileSystem::GetRealPath(path)
				.value_or(std::move(path));

			int baseLen = 0;
			ParParser::ParseParFilename(parFilename.c_str(), true, &baseLen, nullptr);
			BString<1024> infoName;
			infoName.Set(parFilename.c_str(), baseLen);

			NzbgetRsBuf parInfoBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				m_nzbName.c_str(), PATH_SEPARATOR, *infoName);
			BString<1024> parInfoName;
			parInfoName.Set(parInfoBuf.data ? parInfoBuf.data : "", (int)parInfoBuf.len);
			nzbget_rs_free(parInfoBuf);
			SetInfoName(parInfoName);

			EStatus status = RunParCheck(std::move(fullParFilename));

			// accumulate total status, the worst status has priority
			if (allStatus > status)
			{
				allStatus = status;
			}
		}
	}

	return allStatus;
}

inline ParChecker::EStatus ParChecker::RunParCheck(std::string parFilename)
{
	Cleanup();
	m_parFilename = std::move(parFilename);
	m_stage = ptLoadingPars;
	m_processedCount = 0;
	m_extraFiles = 0;
	m_quickFiles = 0;
	m_verifyingExtraFiles = false;
	m_hasDamagedFiles = false;
	EStatus status = psFailed;

	PrintMessage(Message::mkInfo, "Verifying %s", m_infoName.c_str());

	debug("par: %s", m_parFilename.c_str());

	m_progressLabel = std::string("Verifying ") + m_infoName;
	m_fileProgress = 0;
	m_stageProgress = 0;
	UpdateProgress();

	int res = PreProcessPar();
	if (IsStopped() || res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		Cleanup();
		return psFailed;
	}

	std::string creator = GetPacketCreator();
	info("Recovery files created by: %s", creator.empty() ? "unknown" : creator.c_str());

	m_stage = ptVerifyingSources;
	res = GetRepairer()->Process(false);

	if (!m_parQuick)
	{
		CheckEmptyFiles();
	}

	bool addedSplittedFragments = false;
	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		addedSplittedFragments = AddSplittedFragments();
		if (addedSplittedFragments)
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (m_hasDamagedFiles && !IsStopped() && GetRepairer()->MissingFileCount() > 0 &&
		!(addedSplittedFragments && res == NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE) &&
		(g_Options->GetParScan() == Options::psExtended || g_Options->GetParScan() == Options::psDupe))
	{
		if (AddMissingFiles())
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		res = ProcessMorePars();
	}

	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE &&
		g_Options->GetParScan() == Options::psDupe)
	{
		if (AddDupeFiles())
		{
			res = GetRepairer()->Process(false);
			if (res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE && !IsStopped())
			{
				res = ProcessMorePars();
			}
		}
	}

	// a twin's par2 set (other block size, other parity level, same files) may hold
	// the recovery blocks its own lacks
	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		res = ProcessTwinSet();
	}

	if (IsStopped())
	{
		Cleanup();
		return psFailed;
	}

	status = psFailed;

	int postVerifyAction = 0;
	status = (EStatus)nzbget_rs_parchecker_eval_post_verify_action(
		res, NZBGET_RS_PAR_RESULT_SUCCESS, NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE,
		m_hasDamagedFiles ? 1 : 0, g_Options->GetParRepair() ? 1 : 0,
		(int)psFailed, (int)psRepairNotNeeded, (int)psRepairPossible,
		&postVerifyAction);
	if (postVerifyAction == 0)
	{
		PrintMessage(Message::mkInfo, "Repair not needed for %s", m_infoName.c_str());
	}
	else if (postVerifyAction == 1)
	{
		PrintMessage(Message::mkInfo, "Repairing %s", m_infoName.c_str());

		m_progressLabel = std::string("Repairing ") + m_infoName;
		m_fileProgress = 0;
		m_stageProgress = 0;
		m_processedCount = 0;
		m_stage = ptRepairing;
		m_filesToRepair = static_cast<int>(GetRepairer()->DamagedFileCount() + GetRepairer()->MissingFileCount());
		UpdateProgress();

		res = GetRepairer()->Process(true);
		if (res == NZBGET_RS_PAR_RESULT_SUCCESS)
		{
			PrintMessage(Message::mkInfo, "Successfully repaired %s", m_infoName.c_str());
			status = psRepaired;
			StatDupeSources(&m_dupeSources);
			DeleteLeftovers();
		}
		else
		{
			status = psFailed;
		}
	}
	else if (postVerifyAction == 2)
	{
		PrintMessage(Message::mkInfo, "Repair possible for %s", m_infoName.c_str());
	}

	int outcomeKind = 0;
	int useCmdErr = 0;
	status = (EStatus)nzbget_rs_parchecker_eval_cancel_or_fail_outcome(
		IsStopped() ? 1 : 0, (int)m_stage, (int)ptRepairing, (int)status,
		(int)psFailed, (int)psRepairPossible, m_errMsg.empty() ? 1 : 0, res,
		&outcomeKind, &useCmdErr);
	if (outcomeKind == 1)
	{
		PrintMessage(Message::mkWarning, "Repair cancelled for %s", m_infoName.c_str());
		m_errMsg = "repair cancelled";
	}
	else if (outcomeKind == 2)
	{
		PrintMessage(Message::mkWarning, "Par-check cancelled for %s", m_infoName.c_str());
		m_errMsg = "par-check cancelled";
	}
	else if (outcomeKind == 3)
	{
		if (useCmdErr != 0)
		{
			m_errMsg = nzbget_rs_parchecker_cmdline_err_str(res);
		}
		PrintMessage(Message::mkError, "Repair failed for %s: %s. Recovery files created by: %s",
			m_infoName.c_str(), m_errMsg.c_str(), creator.empty() ? "unknown" : creator.c_str());
	}

	nzbget_rs_par_repairer_for_each_target_verification(
		GetRepairer()->Handle(),
		(status == psRepaired || status == psRepairNotNeeded) ? 1 : 0,
		&m_verifiedFiles,
		[](void* rawCtx, const char* targetPath, int keepVerified)
		{
			auto* verifiedFiles = static_cast<std::vector<std::string>*>(rawCtx);
			verifiedFiles->erase(std::remove_if(verifiedFiles->begin(), verifiedFiles->end(),
				[targetPath](const std::string& previous)
				{
					return FileSystem::SameFilename(previous.c_str(), targetPath);
				}), verifiedFiles->end());
			if (keepVerified != 0)
			{
				verifiedFiles->emplace_back(targetPath);
			}
		});

	Cleanup();
	return status;
}

inline int ParChecker::PreProcessPar()
{
	int res = NZBGET_RS_PAR_RESULT_REPAIR_FAILED;
	while (!IsStopped() && res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		Cleanup();

		{
			std::lock_guard guard{ m_repairerMtx };
			m_repairer = std::make_unique<Repairer>(this);
		}

		res = GetRepairer()->PreProcess(m_parFilename);
		debug("ParChecker: PreProcess-result=%i", res);

		if (IsStopped())
		{
			PrintMessage(Message::mkError, "Could not verify %s: stopping", m_infoName.c_str());
			m_errMsg = "par-check was stopped";
			return NZBGET_RS_PAR_RESULT_REPAIR_FAILED;
		}

		if (res == NZBGET_RS_PAR_RESULT_INVALID_ARGS)
		{
			PrintMessage(Message::mkError, "Could not start par-check for %s. Par-file: %s", m_infoName.c_str(), m_parFilename.c_str());
			m_errMsg = "Command line could not be parsed";
			return res;
		}

		if (res != NZBGET_RS_PAR_RESULT_SUCCESS)
		{
			PrintMessage(Message::mkWarning, "Could not verify %s: par2-file could not be processed", m_infoName.c_str());
			PrintMessage(Message::mkInfo, "Requesting more par2-files for %s", m_infoName.c_str());
			bool hasMorePars = LoadMainParBak();
			if (!hasMorePars)
			{
				PrintMessage(Message::mkWarning, "No more par2-files found");
				break;
			}
		}
	}

	if (res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		PrintMessage(Message::mkError, "Could not verify %s: par2-file could not be processed", m_infoName.c_str());
		m_errMsg = "par2-file could not be processed";
		return res;
	}

	return res;
}

inline bool ParChecker::LoadMainParBak()
{
	while (!IsStopped())
	{
		bool hasMorePars = false;
		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
			m_queuedParFiles.clear();
		}

		if (hasMorePars)
		{
			return true;
		}

		int blockFound = 0;
		bool requested = RequestMorePars(1, &blockFound);
		if (requested)
		{
			m_progressLabel = "Awaiting additional par-files";
			m_fileProgress = 0;
			UpdateProgress();
		}

		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
			m_queuedParFilesChanged = false;
		}

		if (!requested && !hasMorePars)
		{
			return false;
		}

		if (!hasMorePars)
		{
			// wait until new files are added by "AddParFile" or a change is signaled by "QueueChanged"
			bool queuedParFilesChanged = false;
			while (!queuedParFilesChanged && !IsStopped())
			{
				{
					std::lock_guard guard{ m_queuedParFilesMtx };
					queuedParFilesChanged = m_queuedParFilesChanged;
				}
				Util::Sleep(100);
			}
		}
	}

	return false;
}

inline int ParChecker::ProcessMorePars()
{
	int res = NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;

	bool moreFilesLoaded = true;
	while (!IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		int missingblockcount = static_cast<int>(GetRepairer()->MissingBlockCount()) -
			static_cast<int>(GetRepairer()->RecoveryBlockCount());
		if (missingblockcount <= 0)
		{
			return NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE;
		}

		if (moreFilesLoaded)
		{
			PrintMessage(Message::mkInfo, "Need more %i par-block(s) for %s", missingblockcount, m_infoName.c_str());
		}

		bool hasMorePars;
		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
		}

		if (!hasMorePars)
		{
			int blockFound = 0;
			bool requested = RequestMorePars(missingblockcount, &blockFound);
			if (requested)
			{
				m_progressLabel = "Awaiting additional par-files";
				m_fileProgress = 0;
				UpdateProgress();
			}

			{
				std::lock_guard guard{ m_queuedParFilesMtx };
				hasMorePars = !m_queuedParFiles.empty();
				m_queuedParFilesChanged = false;
			}

			if (!requested && !hasMorePars)
			{
				NzbgetRsBuf errBuf = nzbget_rs_parchecker_format_not_enough_blocks_errmsg(
					missingblockcount, blockFound);
				m_errMsg = errBuf.data ? std::string(errBuf.data, errBuf.len) : std::string();
				nzbget_rs_free(errBuf);
				break;
			}

			if (!hasMorePars)
			{
				// wait until new files are added by "AddParFile" or a change is signaled by "QueueChanged"
				bool queuedParFilesChanged = false;
				while (!queuedParFilesChanged && !IsStopped())
				{
					{
						std::lock_guard guard{ m_queuedParFilesMtx };
						queuedParFilesChanged = m_queuedParFilesChanged;
					}
					Util::Sleep(100);
				}
			}
		}

		if (IsStopped())
		{
			break;
		}

		moreFilesLoaded = LoadMorePars();
		if (moreFilesLoaded)
		{
			res = GetRepairer()->Process(false);
		}
	}

	return res;
}

inline int ParChecker::ProcessTwinSet()
{
	std::string index = RequestTwinIndex();
	if (index.empty() || IsStopped())
	{
		return NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;
	}

	// a set of its own (its packets carry another set id): verified anew, with
	// the files on disk found by the names in its FileDesc packets
	std::string ownParFilename = m_parFilename;
	std::string ownErrMsg = m_errMsg;
	m_parFilename = index;
	m_errMsg.clear();
	int res = PreProcessPar();
	if (res != NZBGET_RS_PAR_RESULT_SUCCESS || IsStopped())
	{
		m_parFilename = ownParFilename;
		m_errMsg = ownErrMsg;
		return NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;
	}

	res = GetRepairer()->Process(false);
	if (res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE && !IsStopped())
	{
		int blockNeeded = std::max(1, static_cast<int>(GetRepairer()->MissingBlockCount()) -
			static_cast<int>(GetRepairer()->RecoveryBlockCount()));
		PrintMessage(Message::mkInfo, "Need %i par-block(s) of the twin's set for %s", blockNeeded, m_infoName.c_str());
		std::vector<std::string> volumes = RequestTwinVolumes(blockNeeded);
		for (const std::string& volume : volumes)
		{
			if (GetRepairer()->LoadPacketsFromFile(volume.c_str()))
			{
				PrintMessage(Message::mkInfo, "File %s of the twin loaded for par-check", FileSystem::BaseFileName(volume.c_str()));
			}
		}
		if (!volumes.empty() && !IsStopped())
		{
			res = GetRepairer()->Process(false);
		}
	}

	NzbgetRsBuf twinErrBuf = nzbget_rs_parchecker_format_twin_err_msg(
		res, NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE, ownErrMsg.c_str());
	if (twinErrBuf.data)
	{
		m_errMsg.assign(twinErrBuf.data, twinErrBuf.len);
		nzbget_rs_free(twinErrBuf);
	}
	return res;
}

inline bool ParChecker::LoadMorePars()
{
	FileList moreFiles;
	{
		std::lock_guard guard{ m_queuedParFilesMtx };
		moreFiles = std::move(m_queuedParFiles);
		m_queuedParFiles.clear();
	}

	for (std::string& parFilename : moreFiles)
	{
		bool loadedOK = GetRepairer()->LoadPacketsFromFile(parFilename.c_str());
		if (loadedOK)
		{
			PrintMessage(Message::mkInfo, "File %s successfully loaded for par-check", FileSystem::BaseFileName(parFilename.c_str()));
		}
		else
		{
			PrintMessage(Message::mkInfo, "Could not load file %s for par-check", FileSystem::BaseFileName(parFilename.c_str()));
		}
	}

	return !moreFiles.empty();
}

inline void ParChecker::AddParFile(const char * parFilename)
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFiles.push_back(parFilename);
	m_queuedParFilesChanged = true;
}

inline void ParChecker::QueueChanged()
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFilesChanged = true;
}

inline bool ParChecker::AddSplittedFragments()
{
	std::vector<std::string> extrafiles;
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}

	struct FragCtx
	{
		ParChecker* self;
		std::vector<std::string>* extrafiles;
	} ctx{this, &extrafiles};

	nzbget_rs_par_repairer_discover_splitted_fragments(
		GetRepairer()->Handle(),
		m_destDir.c_str(),
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size(),
		&ctx,
		[](void* rawCtx, const char* filename) -> int
		{
			return static_cast<FragCtx*>(rawCtx)->self->IsParredFile(filename) ? 1 : 0;
		},
		[](void* rawCtx, const char* current) -> const char*
		{
			return static_cast<FragCtx*>(rawCtx)->self->FindFileOrigname(current);
		},
		[](void* /*rawCtx*/, const char* msg)
		{
			detail("%s", msg);
		},
		[](void* rawCtx, const char* fullPath)
		{
			static_cast<FragCtx*>(rawCtx)->extrafiles->emplace_back(fullPath);
		});

	bool fragmentsAdded = false;

	if (!extrafiles.empty())
	{
		m_extraFiles += extrafiles.size();
		m_verifyingExtraFiles = true;
		PrintMessage(Message::mkInfo, "Found %i splitted fragments for %s", (int)extrafiles.size(), m_infoName.c_str());
		fragmentsAdded = GetRepairer()->VerifyExtraFiles(extrafiles, true);
		m_verifyingExtraFiles = false;
	}

	return fragmentsAdded;
}

inline bool ParChecker::AddMissingFiles()
{
	return AddExtraFiles(true, false, m_destDir.c_str());
}

inline bool ParChecker::AddDupeFiles()
{
	// the folder of the par2 file, not the file itself (which is no folder to scan)
	BString<1024> directory;
	const char* lastSep = strrchr(m_parFilename.c_str(), PATH_SEPARATOR);
	size_t dirLen = lastSep ? static_cast<size_t>(lastSep - m_parFilename.c_str()) : m_parFilename.size();
	directory.Set(m_parFilename.c_str(), (int)dirLen);

	bool added = AddExtraFiles(false, false, directory);

	if (GetRepairer()->MissingBlockCount() > 0)
	{
		// scanning directories of duplicates
		RequestDupeSources(&m_dupeSources);

		if (!m_dupeSources.empty())
		{
			uint32_t wasBlocksMissing = GetRepairer()->MissingBlockCount();
			uint32_t totalExtraBlocksFound = 0;
			for (DupeSource& dupeSource : m_dupeSources)
			{
				if (GetRepairer()->MissingBlockCount() > 0 && FileSystem::DirectoryExists(dupeSource.GetDirectory()))
				{
					uint32_t wasBlocksMissing2 = GetRepairer()->MissingBlockCount();
					bool oneAdded = AddExtraFiles(false, true, dupeSource.GetDirectory());
					added |= oneAdded;
					uint32_t blocksMissing2 = GetRepairer()->MissingBlockCount();
					dupeSource.SetUsedBlocks(nzbget_rs_parchecker_calc_dupe_blocks_delta(
						dupeSource.GetUsedBlocks(), wasBlocksMissing2, blocksMissing2,
						wasBlocksMissing, &totalExtraBlocksFound));
				}
			}

			if (totalExtraBlocksFound > 0)
			{
				PrintMessage(Message::mkInfo, "Found extra %u blocks in dupe sources", totalExtraBlocksFound);
			}
			else
			{
				PrintMessage(Message::mkInfo, "No extra blocks found in dupe sources");
			}
		}
	}

	return added;
}

inline bool ParChecker::AddExtraFiles(bool onlyMissing, bool externalDir, const char* directory)
{
	if (externalDir)
	{
		PrintMessage(Message::mkInfo, "Performing dupe par-scan for %s in %s", m_infoName.c_str(), FileSystem::BaseFileName(directory));
	}
	else
	{
		PrintMessage(Message::mkInfo, "Performing extra par-scan for %s", m_infoName.c_str());
	}

	std::vector<std::string> extrafiles;
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}

	struct ExtraCtx
	{
		ParChecker* self;
		std::vector<std::string>* extrafiles;
	} ctx{this, &extrafiles};

	nzbget_rs_par_repairer_find_and_sort_extra_files(
		directory,
		externalDir ? 1 : 0,
		m_parFilename.c_str(),
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size(),
		&ctx,
		[](void* rawCtx, const char* filename) -> int
		{
			return static_cast<ExtraCtx*>(rawCtx)->self->IsParredFile(filename) ? 1 : 0;
		},
		[](void* rawCtx, const char* fullPath)
		{
			static_cast<ExtraCtx*>(rawCtx)->extrafiles->emplace_back(fullPath);
		});

	// Scan files
	bool filesAdded = false;
	if (!extrafiles.empty())
	{
		m_extraFiles += extrafiles.size();
		m_verifyingExtraFiles = true;

		// adding files one by one until all missing files are found
		size_t idx = 0;
		while (!IsStopped() && idx < extrafiles.size())
		{
			const std::string& extraFile = extrafiles[idx];
			++idx;

			uint32_t wasFilesMissing = GetRepairer()->MissingFileCount();
			uint32_t wasBlocksMissing = GetRepairer()->MissingBlockCount();

			GetRepairer()->VerifyExtraFiles({ extraFile }, false);

			int fileAdded = 0;
			int blockAdded = 0;
			uint32_t blocksDelta = 0;
			int registerParred = 0;
			int abortReason = 0;
			filesAdded = nzbget_rs_parchecker_eval_extra_file_step(
				wasFilesMissing, GetRepairer()->MissingFileCount(),
				wasBlocksMissing, GetRepairer()->MissingBlockCount(),
				externalDir ? 1 : 0, filesAdded ? 1 : 0, onlyMissing ? 1 : 0,
				&fileAdded, &blockAdded, &blocksDelta, &registerParred, &abortReason) != 0;
			if (registerParred != 0)
			{
				PrintMessage(Message::mkInfo, "Found missing file %s", FileSystem::BaseFileName(extraFile.c_str()));
				RegisterParredFile(FileSystem::BaseFileName(extraFile.c_str()));
			}
			else if (blockAdded != 0)
			{
				PrintMessage(Message::mkInfo, "Found %u missing blocks", blocksDelta);
			}

			if (abortReason == 1)
			{
				PrintMessage(Message::mkInfo, "All missing files found, aborting par-scan");
				break;
			}
			if (abortReason == 2)
			{
				PrintMessage(Message::mkInfo, "All missing blocks found, aborting par-scan");
				break;
			}
		}

		m_verifyingExtraFiles = false;
	}

	return filesAdded;
}

inline bool ParChecker::IsProcessedFile(const char* filename)
{
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}
	return nzbget_rs_parchecker_is_processed_file(
		filename,
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size()) != 0;
}

inline void ParChecker::SignalFilename(std::string str)
{
	std::lock_guard guard{ m_signalMtx };

	if (m_stage == ptRepairing)
	{
		m_stage = ptVerifyingRepaired;
	}

	// don't print progress messages when verifying repaired files in quick verification mode,
	// because repaired files are not verified in this mode
	const char* stageMsg = nzbget_rs_parchecker_stage_message((int)m_stage);
	if (!(m_stage == ptVerifyingRepaired && m_parQuick))
	{
		PrintMessage(Message::mkInfo, "%s %s", stageMsg, str.c_str());
	}

	m_progressLabel = std::string(stageMsg) + " " + str;

	if (m_stage == ptLoadingPars || m_stage == ptVerifyingSources)
	{
		m_processedFiles.push_back(std::move(str));
	}

	m_fileProgress = 0;
	UpdateProgress();
}

inline void ParChecker::SignalProgress(int progress)
{
	std::lock_guard guard{ m_signalMtx };

	m_fileProgress = progress;

	int sourceFilesCount = (m_stage == ptRepairing || m_stage == ptVerifyingRepaired)
		? 0
		: static_cast<int>(GetRepairer()->SourceFileCount());
	m_stageProgress = nzbget_rs_parchecker_calc_stage_progress(
		(int)m_stage, m_fileProgress, m_processedCount, m_filesToRepair,
		sourceFilesCount, m_extraFiles, m_quickFiles);
	debug("Current-progress: %i, Total-progress: %i", m_fileProgress, m_stageProgress);

	UpdateProgress();
}

inline void ParChecker::SignalDone(std::string fileName, int available, int total)
{
	std::lock_guard guard{ m_signalMtx };

	++m_processedCount;

	bool ignore = (m_stage == ptVerifyingSources && available < total && !m_verifyingExtraFiles)
		? Util::MatchFileExt(fileName.c_str(), g_Options->GetParIgnoreExt(), ",;")
		: false;
	int newHasDamaged = m_hasDamagedFiles ? 1 : 0;
	int badBlocks = 0;
	if (nzbget_rs_parchecker_eval_signal_done(
		(int)m_stage, (int)ptVerifyingSources, available, total,
		m_verifyingExtraFiles ? 1 : 0, ignore ? 1 : 0,
		m_hasDamagedFiles ? 1 : 0, &newHasDamaged, &badBlocks) != 0)
	{
		m_hasDamagedFiles = newHasDamaged != 0;
		bool fileExists = nzbget_rs_par_repairer_target_file_exists_by_basename(
			GetRepairer()->Handle(), fileName.c_str()) != 0;

		const char* ignoreSuffix = ignore ? " (ignored)" : "";
		if (fileExists)
		{
			PrintMessage(Message::mkWarning, "File %s has %i bad block(s) of total %i block(s)%s",
				fileName.c_str(), badBlocks, total, ignoreSuffix);
		}
		else
		{
			PrintMessage(Message::mkWarning, "File %s with %i block(s) is missing%s",
				fileName.c_str(), total, ignoreSuffix);
		}

		if (!IsProcessedFile(fileName.c_str()))
		{
			m_processedFiles.push_back(std::move(fileName));
		}
	}
}

inline void ParChecker::CheckEmptyFiles()
{
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}
	if (nzbget_rs_par_repairer_check_empty_files(
		GetRepairer()->Handle(),
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size(),
		this,
		[](void* rawCtx, const char* msg)
		{
			static_cast<ParChecker*>(rawCtx)->PrintMessage(Message::mkWarning, "%s", msg);
		}) != 0)
	{
		m_hasDamagedFiles = true;
	}
}

inline void ParChecker::Cancel()
{
	{
		std::lock_guard guard{ m_repairerMtx };
		if (m_repairer)
		{
			m_repairer->Cancel();
		}
	}
	QueueChanged();
}

inline void ParChecker::DeleteLeftovers()
{
	nzbget_rs_par_repairer_delete_leftovers(
		GetRepairer()->Handle(),
		this,
		[](void* rawCtx, const char* msg)
		{
			static_cast<ParChecker*>(rawCtx)->PrintMessage(Message::mkInfo, "%s", msg);
		});
}

inline std::string ParChecker::GetPacketCreator()
{
	if (!m_repairer || !GetRepairer())
	{
		return "";
	}
	return GetRepairer()->Creator();
}

inline int64 ParChecker::GetBlockSize()
{
	if (!m_repairer || !GetRepairer())
	{
		return 0;
	}
	return GetRepairer()->BlockSize();
}

#endif

#endif

#endif

class RepairController : public Thread, public ScriptController
{
public:
	RepairController();
	virtual void Stop();

#ifndef DISABLE_PARCHECK
	virtual void Run();
	static void StartJob(PostInfo* postInfo);
	bool AddPar(FileInfo* fileInfo, bool deleted);

protected:
	void UpdateParCheckProgress();
	void ParCheckCompleted();
	void CheckPauseState(PostInfo* postInfo);
	bool RequestMorePars(NzbInfo* nzbInfo, const char* parFilename, int blockNeeded, int* blockFound);

private:
	class PostParChecker final : public ParChecker
	{
	public:
		PostInfo* GetPostInfo() { return m_postInfo; }
		void SetPostInfo(PostInfo* postInfo) { m_postInfo = postInfo; }
		time_t GetParTime() { return m_parTime; }
		void SetParTime(time_t parTime) { m_parTime = parTime; }
		time_t GetRepairTime() { return m_repairTime; }
		void SetRepairTime(time_t repairTime) { m_repairTime = repairTime; }
		int GetDownloadSec() { return m_downloadSec; }
		void SetDownloadSec(int downloadSec) { m_downloadSec = downloadSec; }
	protected:
		bool RequestMorePars(int blockNeeded, int* blockFound) override;
		void UpdateProgress() override;
		bool IsStopped() override { return m_owner->IsStopped(); };
		void Completed() override { m_owner->ParCheckCompleted(); }
		void PrintMessage(Message::EKind kind, const char* format, ...) override PRINTF_SYNTAX(3);
		void RegisterParredFile(const char* filename) override;
		void RegisterVerifiedFile(const char* filename) override;
		bool IsParredFile(const char* filename) override;
		EFileStatus FindFileCrc(const char* filename, uint32* crc, SegmentList* segments) override;
		const char* FindFileOrigname(const char* filename) override;
		void RequestDupeSources(DupeSourceList* dupeSourceList) override;
		void StatDupeSources(DupeSourceList* dupeSourceList) override;
		std::string RequestTwinIndex() override;
		std::vector<std::string> RequestTwinVolumes(int blockNeeded) override;
	public:
		void DeleteTwinFiles();
	private:
		std::vector<TwinCheck::NzbEntry> m_twinEntries;
		std::string m_twinName;
		std::string m_twinIndex;
		uint64 m_twinBlockSize = 0;
		std::vector<std::string> m_twinFiles;
		ArticleFetcher m_twinFetcher;
		std::string FetchTwinFile(const TwinCheck::NzbEntry& entry, std::vector<char>* keep = nullptr);
		RepairController* m_owner;
		PostInfo* m_postInfo;
		time_t m_parTime;
		time_t m_repairTime;
		int m_downloadSec;

		friend class RepairController;
	};

	struct BlockInfo
	{
		FileInfo* m_fileInfo;
		int m_blockCount;
		BlockInfo(FileInfo* fileInfo, int blockCount) :
			m_fileInfo(fileInfo), m_blockCount(blockCount) {}
	};

	typedef std::deque<BlockInfo> Blocks;

	PostInfo* m_postInfo;
	PostParChecker m_parChecker;

	void FindPars(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* parFilename,
		Blocks& blocks, bool strictParName, bool exactParName, int* blockFound);
#endif
};

class StreamRepairController : public Thread, public ScriptController
{
public:
	virtual void Run();
	virtual void Stop();
	static void StartJob(PostInfo* postInfo);
	static void StartLive(NzbInfo* nzbInfo);

protected:
	virtual void AddMessage(Message::EKind kind, const char* text);

private:
	struct RepairTarget
	{
		int FileId;
		CString Filename;
		int64 DecodedFileSize;
		StreamRangeList Holes;
		bool NeedsRepair() const { return !Holes.empty() || DecodedFileSize <= 0; }
		int64 EncodedSize = 0;
		int ArticleCount = 0;
		bool Recreated = false;
		int64 FailedSize = 0;
		int64 MissedSize = 0;
		int FailedArticles = 0;
		bool IsParFile = false;
		bool PatchEligible = true;
		int PositionalRank = -1;
		int PositionalWindow = 0;
		uint64 StepsHash = 0;
	};

	struct DonorSource
	{
		CString QueuedFilename;
		CString InfoName;
		CString Password;
	};

	PostInfo* m_postInfo = nullptr;
	bool m_liveMode = false;
	int m_nzbId = 0;
	static constexpr int SlowRepairFactor = 3;
	static constexpr int MaxRunFactor = 5;
	static constexpr int MaxConnectionWaitSec = 1800;
	std::thread m_watchdog;
	std::mutex m_watchdogMutex;
	std::condition_variable m_watchdogCond;
	bool m_watchdogDone = false;
	std::atomic<bool> m_timedOut{false};
	std::atomic<int64> m_progressBytes{0};
	int64 m_missingBytes = 0;
	CString m_slowReason;
	static constexpr int DonorCheckSec = 20;
	std::map<std::string, bool> m_donorDead;
	std::atomic<int> m_checkingDonor{0};
	std::atomic<int> m_materializing{0};
	std::atomic<int> m_waitingForConnection{0};
	bool DonorDead(const DonorSource& donor, NzbInfo* donorNzb);
	void StartWatchdog(const std::vector<RepairTarget>& targets);
	void StopWatchdog(const char* nzbName, const std::vector<RepairTarget>& targets);
	bool Interrupted() { return IsStopped() && !m_timedOut; }
	ArticleFetcher m_fetcher;
	ArticleBatchFetcher m_batchFetcher;
	CString m_targetPassword;
	int m_recoveredArticles = 0;
	int64 m_recoveredBytes = 0;
	int m_recoveredHoles = 0;
	int64 m_recoveredFailedSize = 0;
	int64 m_recoveredFailedParSize = 0;
	bool m_holesRemain = false;
	bool m_hasParFiles = false;
	std::vector<RepairTarget> m_targets;

	void RunLive();
	void RepairCompletedLive(std::vector<RepairTarget>& targets);
	void RefreshLiveNames(std::vector<RepairTarget>& targets,
		std::vector<CString>& memberNames);
	void RefreshLiveTargetName(RepairTarget& target);
	void CollectTargets(NzbInfo* nzbInfo, std::vector<RepairTarget>& targets,
		std::vector<CString>& memberNames);
	void ComputePositionalRanks(const char* destDir, std::vector<RepairTarget>& targets,
		const std::vector<CString>& memberNames);
	void CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo,
		std::vector<DonorSource>& donors);
	void ExecRepair(const char* destDir, std::vector<RepairTarget>& targets,
		std::vector<DonorSource>& donors, const std::vector<CString>& memberNames);
	static void MarkRecreated(NzbInfo* nzbInfo, const RepairTarget& target);
	static void MarkRepaired(NzbInfo* nzbInfo, const RepairTarget& target);
	void ExecCrossPackRepair(const char* destDir, std::vector<RepairTarget>& targets,
		std::vector<DonorSource>& donors, const std::vector<CString>& memberNames);
	void ReportRemainingHoles(std::vector<RepairTarget>& targets);
	void RepairCompleted();
};

#include <algorithm>
#include <chrono>
#include <exception>
#include <new>
#include <mutex>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2018 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef UNPACK_CONTROLLER_H
#define UNPACK_CONTROLLER_H


class UnpackController final : public Thread, public ScriptController
{
public:
	void Run() override;
	void Stop() override;
	static void StartJob(PostInfo* postInfo);
	static bool HasCompletedArchiveFiles(NzbInfo* nzbInfo);
	static Mutex& GetUnpackMutex();
	bool IsUnpacking() const { return m_unpacking; }

	~UnpackController() override;

protected:
	void AddMessage(Message::EKind kind, const char* text) override;

private:
	enum EUnpacker
	{
		upUnrar,
		upSevenZip
	};

	typedef std::vector<CString> FileList;

	PostInfo* m_postInfo = nullptr;
	CString m_name;
	CString m_infoName;
	CString m_infoNameUp;
	CString m_destDir;
	CString m_finalDir;
	CString m_unpackDir;
	CString m_unpackExtendedDir;
	CString m_password;
	EUnpacker m_unpacker;
	bool m_interDir = false;
	bool m_allOkMessageReceived = false;
	bool m_noFilesMessageReceived = false;
	bool m_hasParFiles = false;
	bool m_hasRarFiles = false;
	bool m_hasNotUnpackedRarFiles = false;
	bool m_hasRenamedArchiveFiles = false;
	bool m_hasSevenZipFiles = false;
	bool m_hasSevenZipMultiFiles = false;
	bool m_hasSplittedFiles = false;
	bool m_unpackOk = false;
	bool m_unpackStartError = false;
	bool m_unpackSpaceError = false;
	bool m_unpackDecryptError = false;
	bool m_unpackPasswordError = false;
	bool m_cleanedUpDisk = false;
	bool m_autoTerminated = false;
	bool m_finalDirCreated = false;
	bool m_unpackDirCreated = false;
	bool m_passListTried = false;
	std::atomic<bool> m_unpacking{false};
	std::atomic<std::thread::id> m_execThreadId{};
	FileList m_joinedFiles;
	FileList m_downloadedFiles;

	void ExecuteUnpack(EUnpacker unpacker, const char* password, bool multiVolumes);
	void UnpackArchives(EUnpacker unpacker, bool multiVolumes);
	void JoinSplittedFiles();
	void Completed();
	void CreateUnpackDir();
	bool Cleanup();
	void CheckArchiveFiles();
	void SetProgressLabel(const char* progressLabel);
#ifndef DISABLE_PARCHECK
	void RequestParCheck(bool forceRepair);
#endif
	bool WillRetryWithPass();
};

#include "nzbget_rs.h"

inline UnpackController::~UnpackController()
{
#ifndef DISABLE_TLS
	OpenSSL::StopSSLThread();
#endif
}

inline Mutex& UnpackController::GetUnpackMutex()
{
	static Mutex s_unpackMutex;
	return s_unpackMutex;
}

inline void UnpackController::StartJob(PostInfo* postInfo)
{
	UnpackController* unpackController = new UnpackController();
	unpackController->m_postInfo = postInfo;
	unpackController->SetAutoDestroy(false);

	postInfo->SetPostThread(unpackController);

	unpackController->Start();
}

inline void UnpackController::Run()
{
	time_t start = Util::CurrentTime();
	m_unpackOk = true;
	bool unpack;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		m_destDir = m_postInfo->GetNzbInfo()->GetDestDir();
		m_finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		m_name = m_postInfo->GetNzbInfo()->GetName();

		for (CompletedFile& completedFile : m_postInfo->GetNzbInfo()->GetCompletedFiles())
		{
			m_downloadedFiles.emplace_back(completedFile.GetFilename());
		}

		NzbParameter* parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:");
		unpack = !(parameter && !strcasecmp(parameter->GetValue(), "no"));

		parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_password = parameter->GetValue();
		}
	}

	m_infoName.Format("unpack for %s", *m_name);
	m_infoNameUp.Format("Unpack for %s", *m_name);

	m_hasParFiles = ParParser::FindMainPars(m_destDir, nullptr);

	if (unpack)
	{
		CheckArchiveFiles();
	}

	SetInfoName(m_infoName);
	SetWorkingDir(m_destDir);

	bool hasFiles = m_hasRarFiles || m_hasSevenZipFiles || m_hasSevenZipMultiFiles || m_hasSplittedFiles;

	if (m_postInfo->GetUnpackTried() && !m_postInfo->GetParRepaired() &&
		(!m_password.Empty() || Util::EmptyStr(g_Options->GetUnpackPassFile()) || m_postInfo->GetPassListTried()))
	{
		PrintMessage(Message::mkInfo, "Second unpack attempt skipped for %s due to par-check not repaired anything", *m_name);
		PrintMessage(Message::mkError,
			m_postInfo->GetLastUnpackStatus() == (int)NzbInfo::usPassword ?
				"%s failed: checksum error in the encrypted file. Corrupt file or wrong password." :
				"%s failed.",
			*m_infoNameUp);
		m_postInfo->GetNzbInfo()->SetUnpackStatus((NzbInfo::EPostUnpackStatus)m_postInfo->GetLastUnpackStatus());
	}
	else if (unpack && hasFiles)
	{
		PrintMessage(Message::mkInfo, "Unpacking %s", *m_name);

		CreateUnpackDir();

		if (m_hasRarFiles)
		{
			if (m_hasNotUnpackedRarFiles || m_unpackDirCreated)
			{
				if (m_postInfo->GetNzbInfo()->GetDirectUnpackStatus() == NzbInfo::nsSuccess)
				{
					PrintMessage(
						m_unpackDirCreated ? Message::mkWarning : Message::mkInfo,
						m_unpackDirCreated ?
							"Could not find files unpacked by direct unpack, unpacking all files again" :
							"Found archive files not processed by direct unpack, unpacking all files again");
				}

				// Discard info about extracted archives to prevent reusing on next unpack attempt
				m_postInfo->GetExtractedArchives()->clear();

				UnpackArchives(upUnrar, false);
			}
			else
			{
				PrintMessage(Message::mkInfo, "Using directly unpacked files");
			}
		}

		if (m_hasSevenZipFiles && m_unpackOk)
		{
			UnpackArchives(upSevenZip, false);
		}

		if (m_hasSevenZipMultiFiles && m_unpackOk)
		{
			UnpackArchives(upSevenZip, true);
		}

		if (m_hasSplittedFiles && m_unpackOk)
		{
			JoinSplittedFiles();
		}

		Completed();

		m_joinedFiles.clear();
	}
	else if (unpack)
	{
#ifndef DISABLE_PARCHECK
		if (m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped &&
			m_postInfo->GetNzbInfo()->GetParRenameStatus() <= NzbInfo::rsSkipped &&
			m_hasParFiles)
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			RequestParCheck(false);
		}
		else
#endif
		if (m_hasRenamedArchiveFiles)
		{
			PrintMessage(Message::mkError, "Could not unpack %s due to renamed archive files", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usFailure);
		}
		else
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
		}
	}
	else
	{
		PrintMessage(Message::mkInfo, "Unpack for %s skipped", *m_name);
		m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
	}

	m_postInfo->GetNzbInfo()->SetUnpackSec(
		m_postInfo->GetNzbInfo()->GetUnpackSec() + (int)(Util::CurrentTime() - start));

	m_postInfo->SetWorking(false);
}

inline void UnpackController::UnpackArchives(EUnpacker unpacker, bool multiVolumes)
{
	if (!m_postInfo->GetUnpackTried() || m_postInfo->GetParRepaired())
	{
		ExecuteUnpack(unpacker, m_password, multiVolumes);
		if (!m_unpackOk && m_hasParFiles && !m_unpackPasswordError &&
			m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped)
		{
			debug("For rar4- or 7z-archives try par-check first, before trying password file");
			return;
		}
	}
	else
	{
		m_unpackOk = false;
		m_unpackDecryptError = m_postInfo->GetLastUnpackStatus() == (int)NzbInfo::usPassword;
	}

	if (!m_unpackOk && !m_unpackStartError && !m_unpackSpaceError &&
		(m_unpackDecryptError || m_unpackPasswordError) &&
		(!GetTerminated() || m_autoTerminated) &&
		m_password.Empty() && !Util::EmptyStr(g_Options->GetUnpackPassFile()))
	{
		struct PassTryCtx
		{
			UnpackController* self;
			EUnpacker unpacker;
			bool multiVolumes;
		} ctx{this, unpacker, multiVolumes};

		auto shouldContinueCb = [](void* rawCtx) -> int
		{
			auto* self = static_cast<PassTryCtx*>(rawCtx)->self;
			return (!self->m_unpackOk && !self->m_unpackStartError && !self->m_unpackSpaceError &&
				(self->m_unpackDecryptError || self->m_unpackPasswordError)) ? 1 : 0;
		};
		auto tryPasswordCb = [](void* rawCtx, const char* password)
		{
			auto* c = static_cast<PassTryCtx*>(rawCtx);
			UnpackController* self = c->self;
			debug("Password line: %s", password);
			if (self->IsStopped() && self->m_autoTerminated)
			{
				self->ScriptController::Resume();
				self->Thread::Resume();
			}
			self->m_unpackDecryptError = false;
			self->m_unpackPasswordError = false;
			self->m_autoTerminated = false;
			self->PrintMessage(Message::mkInfo, "Trying password %s for %s", password, *self->m_name);
			self->ExecuteUnpack(c->unpacker, password, c->multiVolumes);
		};

		if (!nzbget_rs_unpack_controller_try_password_file(
			g_Options->GetUnpackPassFile(), &ctx, shouldContinueCb, tryPasswordCb))
		{
			PrintMessage(Message::mkError, "Could not open file %s", g_Options->GetUnpackPassFile());
			return;
		}

		m_passListTried = !IsStopped() || m_autoTerminated;
	}
}

inline void UnpackController::ExecuteUnpack(EUnpacker unpacker, const char* password, bool multiVolumes)
{
	const bool isSevenZip = unpacker == upSevenZip;
	if (!isSevenZip)
	{
		m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	}
	m_allOkMessageReceived = false;
	m_unpackSpaceError = false;
	m_unpacker = unpacker;

	if (isSevenZip)
	{
		PrintMessage(Message::mkInfo, "Executing 7-Zip");
		SetLogPrefix("7-Zip");
	}
	SetProgressLabel("");
	Guard unpackGuard(GetUnpackMutex());
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;

	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkDetail;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			case 4: msgKind = Message::mkInfo; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->AddMessage(msgKind, msg);
	};

	auto isStoppedCb = [](void* ctx) -> int
	{
		if (!ctx) return 0;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		return (self->IsStopped() || self->GetTerminated()) ? 1 : 0;
	};

	auto resetAllOkCb = [](void* ctx)
	{
		if (ctx) static_cast<UnpackController*>(ctx)->m_allOkMessageReceived = false;
	};

	auto hasAllOkCb = [](void* ctx) -> int
	{
		return (ctx && static_cast<UnpackController*>(ctx)->m_allOkMessageReceived) ? 1 : 0;
	};

	const char* pw = Util::EmptyStr(password) ? "" : password;
	const int cleanupDisk = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
	const int useTempDir = (g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0;
	bool allOk = nzbget_rs_unpack_controller_extract_archives(
		m_destDir,
		m_unpackDir,
		pw,
		isSevenZip ? 1 : 0,
		multiVolumes ? 1 : 0,
		useTempDir,
		cleanupDisk,
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		this,
		resetAllOkCb,
		hasAllOkCb,
		logCb,
		isStoppedCb) != 0;

	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	SetLogPrefix(nullptr);
	SetProgressLabel("");

	m_unpackOk = allOk && m_allOkMessageReceived && !GetTerminated() && !IsStopped();
	m_unpackStartError = false;
}

inline bool UnpackController::WillRetryWithPass()
{
	return (m_unpackDecryptError || m_unpackPasswordError) &&
		m_password.Empty() && !g_Options->GetUnpackPassFilePath().empty();
}

inline void UnpackController::JoinSplittedFiles()
{
	SetLogPrefix("Join");
	SetProgressLabel("");
	m_postInfo->SetStageProgress(0);

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkInfo;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->PrintMessage(msgKind, "%s", msg);
	};

	auto progressCb = [](void* ctx, const char* label, int stageProgress)
	{
		if (!ctx) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		if (label)
		{
			self->SetProgressLabel(label);
		}
		if (stageProgress >= 0 && self->m_postInfo)
		{
			self->m_postInfo->SetStageProgress(stageProgress);
		}
	};

	auto addJoinedCb = [](void* ctx, const char* pieceName)
	{
		if (!ctx || !pieceName) return;
		static_cast<UnpackController*>(ctx)->m_joinedFiles.emplace_back(pieceName);
	};

	const int parSuccess = (m_postInfo && m_postInfo->GetNzbInfo() &&
		m_postInfo->GetNzbInfo()->GetParStatus() == NzbInfo::psSuccess) ? 1 : 0;
	const int writeBufferKb = g_Options ? g_Options->GetWriteBuffer() : 0;

	if (nzbget_rs_unpack_controller_join_splitted_files(
		m_destDir, m_unpackDir, parSuccess, writeBufferKb,
		this, logCb, progressCb, addJoinedCb) == 0)
	{
		m_unpackOk = false;
	}

	SetLogPrefix(nullptr);
	SetProgressLabel("");
}

inline void UnpackController::Completed()
{
	if (!m_autoTerminated && !(m_postInfo && m_postInfo->GetDeleted()) &&
		(IsStopped() || GetTerminated()))
	{
		debug("Unpack stopped due to reload/shutdown, preserving _unpack directory");
		return;
	}

	bool cleanupSuccess = Cleanup();

	if (m_unpackOk && cleanupSuccess)
	{
		PrintMessage(Message::mkInfo, "%s %s", *m_infoNameUp, "successful");
		m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSuccess);
		m_postInfo->GetNzbInfo()->SetUnpackCleanedUpDisk(m_cleanedUpDisk);
		if (g_Options->GetParRename())
		{
			//request par-rename check for extracted files
			m_postInfo->GetNzbInfo()->SetParRenameStatus(NzbInfo::rsNone);
		}
	}
	else
	{
		int parcheckEnabled =
#ifndef DISABLE_PARCHECK
			1;
#else
			0;
#endif
		int forceRepair = 0;
		int unpackStatus = (int)NzbInfo::usFailure;
		int reqPar = nzbget_rs_unpack_controller_eval_completed_failure(
			parcheckEnabled,
			m_unpackOk ? 1 : 0,
			(int)m_postInfo->GetNzbInfo()->GetParStatus(),
			(int)NzbInfo::psSkipped,
			m_postInfo->GetNzbInfo()->GetParFull() ? 1 : 0,
			m_unpackStartError ? 1 : 0,
			m_unpackSpaceError ? 1 : 0,
			m_unpackPasswordError ? 1 : 0,
			m_unpackDecryptError ? 1 : 0,
			GetTerminated() ? 1 : 0,
			IsStopped() ? 1 : 0,
			m_autoTerminated ? 1 : 0,
			m_hasParFiles ? 1 : 0,
			!m_password.Empty() ? 1 : 0,
			!Util::EmptyStr(g_Options->GetUnpackPassFile()) ? 1 : 0,
			m_passListTried ? 1 : 0,
			&forceRepair,
			&unpackStatus);
#ifndef DISABLE_PARCHECK
		if (reqPar != 0)
		{
			RequestParCheck(forceRepair != 0);
		}
		else
#endif
		{
			PrintMessage(Message::mkError, "%s failed", *m_infoNameUp);
			m_postInfo->GetNzbInfo()->SetUnpackStatus((NzbInfo::EPostUnpackStatus)unpackStatus);
		}
	}
}

#ifndef DISABLE_PARCHECK
inline void UnpackController::RequestParCheck(bool forceRepair)
{
	PrintMessage(Message::mkInfo, "%s requested %s", *m_infoNameUp,
		forceRepair ? "par-check with forced repair" : "par-check/repair");
	m_postInfo->SetRequestParCheck(true);
	m_postInfo->SetForceRepair(forceRepair);
	m_postInfo->SetUnpackTried(true);
	m_postInfo->SetPassListTried(m_passListTried);
	m_postInfo->SetLastUnpackStatus(
		m_unpackSpaceError ? (int)NzbInfo::usSpace :
		(m_unpackPasswordError || m_unpackDecryptError) ? (int)NzbInfo::usPassword :
		(int)NzbInfo::usFailure);
}
#endif

inline void UnpackController::CreateUnpackDir()
{
	m_interDir = nzbget_rs_unpack_use_inter_dir(
		m_postInfo->GetNzbInfo()->GetDestDir(), g_Options->GetInterDir(), PATH_SEPARATOR) != 0;

	if (m_interDir && m_finalDir.Empty())
	{
		m_finalDir = m_postInfo->GetNzbInfo()->BuildFinalDirName();

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->SetFinalDir(m_finalDir);
		}

		m_finalDirCreated = !FileSystem::DirectoryExists(m_finalDir);
	}

	const char* destDir = !m_finalDir.Empty() ? *m_finalDir : *m_destDir;

	if (g_Options->GetUseTempUnpackDir())
	{
		m_unpackDir.Format("%s%c%s", destDir, PATH_SEPARATOR, "_unpack");
	}
	else
	{
		m_unpackDir = destDir;
	}

	m_unpackDirCreated = !FileSystem::DirectoryExists(m_unpackDir);

	detail("Unpacking into %s", *m_unpackDir);

	CString errmsg;
	if (!FileSystem::ForceDirectories(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not create directory %s: %s", *m_unpackDir, *errmsg);
	}
}

inline void UnpackController::CheckArchiveFiles()
{
	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	std::vector<const char*> extractedPtrs;
	if (m_postInfo && m_postInfo->GetExtractedArchives())
	{
		extractedPtrs.reserve(m_postInfo->GetExtractedArchives()->size());
		for (const CString& f : *m_postInfo->GetExtractedArchives())
		{
			extractedPtrs.push_back(*f);
		}
	}

	int hasRar = 0, hasNotUnpackedRar = 0, hasRenamed = 0;
	int hasSevenZip = 0, hasSevenZipMulti = 0, hasSplitted = 0;
	nzbget_rs_unpack_controller_check_archive_files(
		m_destDir,
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		extractedPtrs.data(),
		extractedPtrs.size(),
		g_Options ? g_Options->GetUnpackIgnoreExt() : "",
		&hasRar,
		&hasNotUnpackedRar,
		&hasRenamed,
		&hasSevenZip,
		&hasSevenZipMulti,
		&hasSplitted);

	m_hasRarFiles = hasRar != 0;
	m_hasNotUnpackedRarFiles = hasNotUnpackedRar != 0;
	m_hasRenamedArchiveFiles = hasRenamed != 0;
	m_hasSevenZipFiles = hasSevenZip != 0;
	m_hasSevenZipMultiFiles = hasSevenZipMulti != 0;
	m_hasSplittedFiles = hasSplitted != 0;
}

inline bool UnpackController::Cleanup()
{
	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	std::vector<const char*> joinedPtrs;
	joinedPtrs.reserve(m_joinedFiles.size());
	for (const CString& f : m_joinedFiles)
	{
		joinedPtrs.push_back(*f);
	}

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkInfo;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->PrintMessage(msgKind, "%s", msg);
	};

	int cleanedUpDisk = m_cleanedUpDisk ? 1 : 0;
	bool ok = nzbget_rs_unpack_controller_cleanup(
		m_destDir,
		m_finalDir,
		m_unpackDir,
		m_unpackOk ? 1 : 0,
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		m_finalDirCreated ? 1 : 0,
		(g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0,
		m_interDir ? 1 : 0,
		m_hasSplittedFiles ? 1 : 0,
		static_cast<uint32_t>(FileSystem::uMask),
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		joinedPtrs.data(),
		joinedPtrs.size(),
		&cleanedUpDisk,
		this,
		logCb) != 0;
	m_cleanedUpDisk = cleanedUpDisk != 0;
	return ok;
}

inline void UnpackController::AddMessage(Message::EKind kind, const char* text)
{
	BString<1024> msgText = text;
	NzbgetRsUnpackMessageInfo info{};
	nzbget_rs_unpack_parse_message(
		text, m_unpackExtendedDir, m_unpacker == upSevenZip ? 1 : 0, &info);
	if (m_unpacker == upUnrar && info.strip_extracting_dir != 0)
	{
		msgText.Format("Unrar: Extracting %s", text + info.stripped_suffix_offset);
	}

	if (m_postInfo)
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, msgText);
	}

	if (m_postInfo && m_unpacker == upUnrar && info.reset_stage_time != 0)
	{
		m_postInfo->SetStageTime(Util::CurrentTime());
	}

	if (m_postInfo && m_unpacker == upUnrar && info.has_extracting_progress != 0)
	{
		SetProgressLabel(msgText + 7);
	}

	if (m_postInfo && m_unpacker == upUnrar && info.has_extracting_from != 0)
	{
#ifdef DEBUG
		const char *filename = text + 23;
		debug("Filename: %s", filename);
#endif
		SetProgressLabel(text + 7);
	}

	if (info.decrypt_error != 0)
	{
		m_unpackDecryptError = true;
	}

	if (info.password_error != 0)
	{
		m_unpackPasswordError = true;
	}

	if (nzbget_rs_unpack_is_space_error(kind == Message::mkError ? 1 : 0, text) != 0)
	{
		m_unpackSpaceError = true;
	}

	if (!IsStopped() && (m_unpackDecryptError || m_unpackPasswordError || info.crc_or_volume_error))
	{
		if (m_postInfo && !WillRetryWithPass())
		{
			m_postInfo->GetNzbInfo()->AddMessage(
				Message::mkWarning,
				BString<1024>("Cancelling %s due to errors", *m_infoName));
		}

		m_autoTerminated = true;
		Stop();
	}

	if (info.all_ok != 0)
	{
		m_allOkMessageReceived = true;
	}
}

inline void UnpackController::Stop()
{
	debug("Stopping unpack");
	Thread::Stop();
	bool isExecThread = std::this_thread::get_id() == m_execThreadId.load();
	if (m_unpacking && !isExecThread && !m_autoTerminated && HasActiveProcess())
	{
		if (Break())
		{
			for (int i = 0; i < 50 && m_unpacking && HasActiveProcess(); ++i)
			{
				Util::Sleep(100);
			}
		}
		if (m_unpacking)
		{
			Terminate();
		}
	}
	else
	{
		Terminate();
	}
}

inline void UnpackController::SetProgressLabel(const char* progressLabel)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_postInfo->SetProgressLabel(progressLabel);
}

inline bool UnpackController::HasCompletedArchiveFiles(NzbInfo* nzbInfo)
{
	for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_unpack_has_completed_archive_ext(completedFile.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef DIRECTUNPACK_H
#define DIRECTUNPACK_H

#include <atomic>

class DirectUnpack final : public Thread, public ScriptController
{
public:
	virtual void Run();
	static void StartJob(NzbInfo* nzbInfo);
	void Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo);
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	static bool IsArchiveFilename(const char* filename);
	bool IsUnpacking() const { return m_unpacking; }
	bool IsWaitingVolume() const { return m_waitingVolume; }

protected:
	virtual void AddMessage(Message::EKind kind, const char* text);

private:
	/**
	 *  @brief Checks if Direct unpack can proceed.
	 *
	 *  @return True if direct unpack is permitted (either the file is not an
 	 *          archive or the archive contains no failed articles), false otherwise.
	 */
	bool CanProceed(FileInfo* fileInfo) const;

	typedef std::deque<CString> ArchiveList;

	int m_nzbId;
	CString m_name;
	CString m_infoName;
	CString m_infoNameUp;
	CString m_destDir;
	CString m_finalDir;
	CString m_unpackDir;
	CString m_unpackExtendedDir;
	CString m_password;
	CString m_waitingFile;
	CString m_progressLabel;
	std::atomic<bool> m_nzbCompleted{false};
	Mutex m_volumeMutex;
	ArchiveList m_archives;
	time_t m_extraStartTime = 0;
	ArchiveList m_extractedArchives;
	bool m_allOkMessageReceived = false;
	bool m_unpackOk = false;
	bool m_finalDirCreated = false;
	bool m_processed = false;
	std::atomic<bool> m_cancelled{false};
	std::atomic<bool> m_waitingVolume{false};
	std::atomic<bool> m_unpacking{false};
	std::atomic<std::thread::id> m_execThreadId{};
	void CreateUnpackDir();
	void FindArchiveFiles();
	void ExecuteUnrar(const char* archiveName);
	void WaitNextVolume(const char* filename);
	void Cleanup();
	bool IsMainArchive(const char* filename);
	void SetProgressLabel(NzbInfo* nzbInfo, const char* progressLabel);
	void AddExtraTime(NzbInfo* nzbInfo);
};

#include <iterator>
#include "nzbget_rs.h"

inline void DirectUnpack::StartJob(NzbInfo* nzbInfo)
{
	DirectUnpack* directUnpack = new DirectUnpack();
	directUnpack->m_nzbId = nzbInfo->GetId();
	directUnpack->m_unpacking = false;
	directUnpack->m_waitingVolume = false;
	directUnpack->SetAutoDestroy(true);

	nzbInfo->SetUnpackThread(directUnpack);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsRunning);

	directUnpack->Start();
}

inline void DirectUnpack::Run()
{
	debug("Entering DirectUnpack-loop for %i", m_nzbId);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo)
		{
			debug("Could not find NzbInfo for %i", m_nzbId);
			m_unpacking = false;
			return;
		}

		m_name = nzbInfo->GetName();
		m_destDir = nzbInfo->GetDestDir();
		m_finalDir = nzbInfo->BuildFinalDirName();

		NzbParameter* parameter = nzbInfo->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_password = parameter->GetValue();
		}
	}

	m_infoName.Format("direct unpack for %s", *m_name);
	m_infoNameUp.Format("Direct unpack for %s", *m_name);

	FindArchiveFiles();

	SetInfoName(m_infoName);
	SetWorkingDir(m_destDir);

	while (!IsStopped())
	{
		CString archive;
		{
			Guard guard(m_volumeMutex);
			if (!m_archives.empty())
			{
				archive = std::move(m_archives.front());
				m_archives.pop_front();
			}
		}

		if (archive)
		{
			m_waitingVolume = false;
			if (!m_processed)
			{
				PrintMessage(Message::mkInfo, "Directly unpacking %s", *m_name);
				Cleanup();
				CreateUnpackDir();
				m_processed = true;
			}

			size_t committedArchives = m_extractedArchives.size();
			ExecuteUnrar(archive);

			if (!m_unpackOk)
			{
				m_extractedArchives.resize(committedArchives);
				break;
			}
		}
		else
		{
			if (m_nzbCompleted)
			{
				break;
			}
			m_waitingVolume = true;
			Util::Sleep(100);
		}
	}

	m_unpacking = false;
	m_waitingVolume = false;

	bool interrupted = !m_cancelled && (IsStopped() || GetTerminated());
	if ((!m_unpackOk && !interrupted) || m_cancelled)
	{
		Cleanup();
	}

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo || nzbInfo->GetUnpackThread() != this)
		{
			debug("Could not find NzbInfo for %s", *m_infoName);
			return;
		}

		nzbInfo->SetUnpackThread(nullptr);
		nzbInfo->SetDirectUnpackStatus(
			!m_processed ? NzbInfo::nsNone :
			m_unpackOk && !interrupted && !m_cancelled ? NzbInfo::nsSuccess :
			interrupted ? NzbInfo::nsNone : NzbInfo::nsFailure);

		if (interrupted)
		{
			debug("%s stopped due to reload/shutdown, preserving unpack state", *m_infoName);
		}
		else if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess ||
			nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsFailure)
		{
			bool ok = nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess;
			nzbInfo->AddMessage(ok ? Message::mkInfo : Message::mkWarning,
				BString<1024>("%s %s", *m_infoNameUp, ok ? "successful" : "failed"));
		}

		if (nzbInfo->GetExtractedArchives() &&
			(nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess || interrupted))
		{
			std::move(m_extractedArchives.begin(), m_extractedArchives.end(),
				std::back_inserter(*nzbInfo->GetExtractedArchives()));
		}

		AddExtraTime(nzbInfo);

		if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(false);
		}
	}

	debug("Exiting DirectUnpack-loop for %i", m_nzbId);
}

inline void DirectUnpack::ExecuteUnrar(const char* archiveName)
{
	m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	m_allOkMessageReceived = false;

	Guard unpackGuard(UnpackController::GetUnpackMutex());
	if (IsStopped())
	{
		return;
	}
	m_waitingVolume = false;
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;

	BString<1024> fullArchivePath("%s%c%s", *m_destDir, PATH_SEPARATOR, archiveName);
	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		Message::EKind msgKind = Message::mkDetail;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			case 4: msgKind = Message::mkInfo; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->AddMessage(msgKind, msg);
		while (self->m_waitingVolume && !self->IsStopped() && !self->GetTerminated())
		{
			Util::Sleep(20);
		}
	};

	auto isStoppedCb = [](void* ctx) -> int
	{
		if (!ctx) return 0;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		return (self->IsStopped() || self->GetTerminated()) ? 1 : 0;
	};

	const char* pw = m_password.Empty() ? "" : *m_password;
	const int cleanupDisk = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
	int ok = nzbget_rs_unpack_extract_with_zipfast(
		fullArchivePath,
		m_unpackDir,
		pw,
		1,
		cleanupDisk,
		logCb,
		this,
		isStoppedCb);

	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	m_waitingVolume = false;

	m_unpackOk = ok != 0 && m_allOkMessageReceived && !GetTerminated() && !IsStopped();
}

inline void DirectUnpack::CreateUnpackDir()
{
	bool useInterDir = nzbget_rs_unpack_use_inter_dir(
		m_destDir, g_Options->GetInterDir(), PATH_SEPARATOR) != 0;

	m_finalDirCreated = useInterDir && !FileSystem::DirectoryExists(m_finalDir);

	const char* destDir = useInterDir && !m_finalDir.Empty() ? *m_finalDir : *m_destDir;

	if (g_Options->GetUseTempUnpackDir())
	{
		m_unpackDir.Format("%s%c%s", destDir, PATH_SEPARATOR, "_unpack");
	}
	else
	{
		m_unpackDir = destDir;
	}

	detail("Unpacking into %s", *m_unpackDir);

	CString errmsg;
	if (!FileSystem::ForceDirectories(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not create directory %s: %s", *m_unpackDir, *errmsg);
	}
}

inline void DirectUnpack::FindArchiveFiles()
{
	Guard guard(m_volumeMutex);
	auto addArchiveCb = [](void* ctx, const char* archiveName)
	{
		if (!ctx || !archiveName) return;
		static_cast<DirectUnpack*>(ctx)->m_archives.emplace_back(archiveName);
	};
	nzbget_rs_direct_unpack_find_archive_files(m_destDir, this, addArchiveCb);
}

inline bool DirectUnpack::IsMainArchive(const char* filename)
{
	return nzbget_rs_direct_unpack_is_main_archive(filename) != 0;
}

inline void DirectUnpack::AddMessage(Message::EKind kind, const char* text)
{
	debug("%s", text);

	BString<1024> msgText = text;
	NzbgetRsUnpackMessageInfo info{};
	nzbget_rs_unpack_parse_message(text, m_unpackExtendedDir, 0, &info);
	if (info.strip_extracting_dir != 0)
	{
		msgText.Format("Unrar: Extracting %s", text + info.stripped_suffix_offset);
	}
	if (info.is_insert_disk != 0)
	{
		BString<1024> filename;
		if (info.insert_disk_name_len > 0)
		{
			filename.Set(text + info.insert_disk_name_offset, (int)info.insert_disk_name_len);
		}
		WaitNextVolume(filename);
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (nzbInfo)
	{
		nzbInfo->AddMessage(kind, msgText);
	}
	else
	{
		ScriptController::AddMessage(kind, msgText);
	}

	if (info.has_extracting_progress != 0 && nzbInfo)
	{
		SetProgressLabel(nzbInfo, msgText + 7);
	}

	if (info.has_extracting_from != 0 && nzbInfo)
	{
		SetProgressLabel(nzbInfo, text + 7);
		m_extractedArchives.emplace_back(text + 23);
	}

	if (!IsStopped() && (info.decrypt_error || info.direct_password_error || info.crc_or_volume_error))
	{
		Stop(downloadQueue, nzbInfo);
	}

	if (info.all_ok != 0)
	{
		m_allOkMessageReceived = true;
	}
}

inline void DirectUnpack::Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Stopping %s", *m_infoName);
	if (!(g_PrePostProcessor && g_PrePostProcessor->IsStopped()))
	{
		m_cancelled = true;
	}
	if (m_processed)
	{
		if (nzbInfo)
		{
			nzbInfo->AddMessage(Message::mkWarning, BString<1024>("Cancelling %s", *m_infoName));
		}
		else
		{
			warn("Cancelling %s", *m_infoName);
		}
	}
	if (nzbInfo)
	{
		AddExtraTime(nzbInfo);
		if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(false);
		}
	}
	Thread::Stop();
	bool isExecThread = std::this_thread::get_id() == m_execThreadId.load();
	if (m_unpacking)
	{
		if (!isExecThread && !m_cancelled && HasActiveProcess() && Break())
		{
			for (int i = 0; i < 50 && m_unpacking && HasActiveProcess(); ++i)
			{
				Util::Sleep(100);
			}
		}
		if (m_unpacking)
		{
			Terminate();
		}
	}
}

inline void DirectUnpack::WaitNextVolume(const char* filename)
{
	debug("WaitNextVolume for %s", filename);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo && (strcmp(m_destDir, nzbInfo->GetDestDir()) ||
			strcmp(m_finalDir, nzbInfo->BuildFinalDirName())))
		{
			nzbInfo->AddMessage(Message::mkWarning,
				BString<1024>("Stopping direct unpack for %s due to changed destination directory", nzbInfo->GetName()));
			Stop(downloadQueue, nzbInfo);
		}
	}

	BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);
	if (FileSystem::FileExists(fullFilename))
	{
		m_waitingVolume = false;
	}
	else
	{
		bool nzbCompleted;
		{
			Guard guard(m_volumeMutex);
			m_waitingFile = filename;
			m_waitingVolume = true;
			nzbCompleted = m_nzbCompleted;
		}
		if (nzbCompleted)
		{
			PrintMessage(Message::mkWarning, "Could not find volume %s", filename);
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
			Stop(downloadQueue, nzbInfo);
		}
	}
}

inline void DirectUnpack::FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	debug("FileDownloaded for %s/%s", fileInfo->GetNzbInfo()->GetName(), fileInfo->GetFilename());

	if (!CanProceed(fileInfo))
	{
		Stop(downloadQueue, fileInfo->GetNzbInfo());
		return;
	}

	Guard guard(m_volumeMutex);
	if (m_waitingFile && !strcasecmp(m_waitingFile, fileInfo->GetFilename()))
	{
		m_waitingFile = nullptr;
		m_waitingVolume = false;
	}

	if (IsMainArchive(fileInfo->GetFilename()))
	{
		m_archives.emplace_back(fileInfo->GetFilename());
	}
}

inline bool DirectUnpack::CanProceed(FileInfo* fileInfo) const
{
	return !IsArchiveFilename(fileInfo->GetFilename()) || fileInfo->GetFailedArticles() == 0;
}

inline void DirectUnpack::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDownloaded for %s", nzbInfo->GetName());

	Guard guard(m_volumeMutex);
	m_nzbCompleted = true;
	if (m_waitingFile)
	{
		nzbInfo->AddMessage(Message::mkWarning,
			BString<1024>("Could not find volume %s", *m_waitingFile));
		Stop(downloadQueue, nzbInfo);
		return;
	}

	m_extraStartTime = Util::CurrentTime();

	if (nzbInfo->GetPostInfo())
	{
		nzbInfo->GetPostInfo()->SetProgressLabel(m_progressLabel);
	}
}

inline void DirectUnpack::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDeleted for %s", nzbInfo->GetName());

	nzbInfo->SetUnpackThread(nullptr);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsFailure);
	Stop(downloadQueue, nzbInfo);
}

inline void DirectUnpack::Cleanup()
{
	debug("Cleanup for %s", *m_infoName);

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		self->PrintMessage(kind == 1 ? Message::mkError : Message::mkInfo, "%s", msg);
	};

	nzbget_rs_direct_unpack_cleanup(
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		m_unpackDir,
		m_finalDirCreated ? 1 : 0,
		m_finalDir,
		this,
		logCb);
}

inline void DirectUnpack::SetProgressLabel(NzbInfo* nzbInfo, const char* progressLabel)
{
	m_progressLabel = progressLabel;
	if (nzbInfo->GetPostInfo())
	{
		nzbInfo->GetPostInfo()->SetProgressLabel(progressLabel);
	}
}

inline void DirectUnpack::AddExtraTime(NzbInfo* nzbInfo)
{
	if (m_extraStartTime)
	{
		int extraSec = (int)(Util::CurrentTime() - m_extraStartTime);
		nzbInfo->SetUnpackSec(nzbInfo->GetUnpackSec() + extraSec);
		nzbInfo->SetPostTotalSec(nzbInfo->GetPostTotalSec() + extraSec);
		m_extraStartTime = 0;
	}
}

inline bool DirectUnpack::IsArchiveFilename(const char* filename)
{
	if (!filename)
	{
		return false;
	}

	return nzbget_rs_file_type(22, filename, strlen(filename)) != 0;
}

#endif

#endif

#include "nzbget_rs.h"

#ifndef DISABLE_PARCHECK
inline bool RepairController::PostParChecker::RequestMorePars(int blockNeeded, int* blockFound)
{
	return m_owner->RequestMorePars(m_postInfo->GetNzbInfo(), GetParFilename(), blockNeeded, blockFound);
}

inline void RepairController::PostParChecker::UpdateProgress()
{
	m_owner->UpdateParCheckProgress();
}

inline void RepairController::PostParChecker::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, 1024, format, args);
	va_end(args);
	text[1024-1] = '\0';

	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}

inline void RepairController::PostParChecker::RegisterParredFile(const char* filename)
{
	m_postInfo->GetParredFiles()->push_back(filename);
}

inline void RepairController::PostParChecker::RegisterVerifiedFile(const char* filename)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
	StreamRepairJobList* jobs = nzbInfo->GetStreamRepairJobs();
	std::string verifiedPath = FileSystem::GetRealPath(filename).value_or(filename);
	size_t previousSize = jobs->size();
	jobs->erase(std::remove_if(jobs->begin(), jobs->end(),
		[nzbInfo, &verifiedPath](const StreamRepairJob& job)
		{
			std::string currentName = job.GetFilename();
			for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
			{
				if (completedFile.GetId() == job.GetFileId())
				{
					currentName = completedFile.GetFilename();
					break;
				}
			}
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, currentName.c_str());
			std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
			nzbget_rs_free(pathBuf);
			path = FileSystem::GetRealPath(path).value_or(path);
			return FileSystem::SameFilename(path.c_str(), verifiedPath.c_str());
		}), jobs->end());
	if (jobs->size() != previousSize)
	{
		nzbInfo->SetChanged(true);
	}
}

inline bool RepairController::PostParChecker::IsParredFile(const char* filename)
{
	for (CString& parredFile : m_postInfo->GetParredFiles())
	{
		if (nzbget_rs_repair_filename_eq_ignore_case(parredFile, filename) != 0)
		{
			return true;
		}
	}
	return false;
}

inline ParChecker::EFileStatus RepairController::PostParChecker::FindFileCrc(const char* filename,
	uint32* crc, SegmentList* segments)
{
	int id = 0;
	CompletedFile::EStatus status;
	bool reprocess;
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		CompletedFile* completedFile = nullptr;
		for (CompletedFile& completedFile2 : *m_postInfo->GetNzbInfo()->GetCompletedFiles())
		{
			if (nzbget_rs_repair_filename_eq_ignore_case(completedFile2.GetFilename(), filename) != 0)
			{
				completedFile = &completedFile2;
				break;
			}
		}
		if (!completedFile)
		{
			return ParChecker::fsUnknown;
		}
		id = completedFile->GetId();
		*crc = completedFile->GetCrc();
		status = completedFile->GetStatus();
		reprocess = m_postInfo->GetNzbInfo()->GetReprocess();
	}

	debug("Found completed file: %s, CRC: %.8x, Status: %i", filename, *crc, (int)status);

	if (nzbget_rs_repair_should_load_partial_segments(
		(int)status, (int)CompletedFile::cfPartial, id, reprocess ? 1 : 0) != 0)
	{
		FileInfo tmpFileInfo(id);

		if (!g_DiskState->LoadFileState(&tmpFileInfo, nullptr, true))
		{
			return ParChecker::fsUnknown;
		}

		for (ArticleInfo* pa : tmpFileInfo.GetArticles())
		{
			segments->emplace_back(pa->GetStatus() == ArticleInfo::aiFinished,
				pa->GetSegmentOffset(), pa->GetSegmentSize(), pa->GetCrc());
		}
	}

	return (ParChecker::EFileStatus)nzbget_rs_repair_classify_file_crc_status(
		(int)status, reprocess ? 1 : 0, !segments->empty() ? 1 : 0,
		(int)CompletedFile::cfSuccess, (int)CompletedFile::cfFailure, (int)CompletedFile::cfPartial,
		(int)ParChecker::fsSuccess, (int)ParChecker::fsFailure, (int)ParChecker::fsPartial,
		(int)ParChecker::fsUnknown);
}

inline const char* RepairController::PostParChecker::FindFileOrigname(const char* filename)
{
	thread_local std::string origname;
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	for (CompletedFile& completedFile : *m_postInfo->GetNzbInfo()->GetCompletedFiles())
	{
		if (nzbget_rs_repair_filename_eq_ignore_case(completedFile.GetFilename(), filename) != 0)
		{
			if (!completedFile.GetOrigname())
			{
				return nullptr;
			}
			origname = completedFile.GetOrigname();
			return origname.c_str();
		}
	}

	return nullptr;
}

inline void RepairController::PostParChecker::RequestDupeSources(DupeSourceList* dupeSourceList)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	RawNzbList dupeList = g_DupeCoordinator->ListHistoryDupes(downloadQueue, m_postInfo->GetNzbInfo());

	if (!dupeList.empty())
	{
		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		std::vector<const char*> dupeDirs(dupeList.size());
		for (size_t i = 0; i < dupeList.size(); i++)
		{
			dupeDirs[i] = dupeList[i]->GetDestDir();
		}
		std::vector<size_t> usableIndices(dupeList.size());
		size_t usableCount = nzbget_rs_dupematcher_filter_dupe_sources(
			nzbInfo->GetName(),
			nzbInfo->GetDestDir(),
			nzbInfo->GetSize() - nzbInfo->GetParSize(),
			g_Options->GetUnrarCmd(),
			dupeDirs.data(),
			dupeDirs.size(),
			usableIndices.data(),
			[](void* ctx, int kind, const char* msg)
			{
				auto* self = static_cast<PostParChecker*>(ctx);
				self->PrintMessage(kind == 3 ? Message::mkDetail : Message::mkInfo, "%s", msg ? msg : "");
			},
			this);
		for (size_t i = 0; i < usableCount; i++)
		{
			NzbInfo* dupeNzbInfo = dupeList[usableIndices[i]];
			dupeSourceList->emplace_back(dupeNzbInfo->GetId(), dupeNzbInfo->GetDestDir());
		}
	}
}

inline std::string RepairController::PostParChecker::FetchTwinFile(const TwinCheck::NzbEntry& entry, std::vector<char>* keep)
{
	std::vector<char> data;
	if (!TwinCheck::FetchEntry(m_twinFetcher, entry, 1024LL * 1024 * 1024, [this]() { return IsStopped(); }, data))
	{
		return "";
	}
	NzbgetRsBuf nameBuf = nzbget_rs_repair_build_twin_filename(
		entry.filename.c_str(), m_twinFiles.size());
	std::string name(nameBuf.data ? nameBuf.data : "", nameBuf.len);
	nzbget_rs_free(nameBuf);
	NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		GetDestDir(), PATH_SEPARATOR, name.c_str());
	BString<1024> path;
	path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
	nzbget_rs_free(pathBuf);
	if (!FileSystem::SaveBufferIntoFile(*path, data.empty() ? "" : data.data(), data.size()))
	{
		return "";
	}
	m_twinFiles.push_back(*path);
	if (keep)
	{
		*keep = std::move(data);
	}
	return *path;
}

inline std::string RepairController::PostParChecker::RequestTwinIndex()
{
	std::vector<TwinCheck::FileSig> own;
	{
		CharBuffer parBuf;
		if (FileSystem::LoadFileIntoBuffer(GetParFilename(), parBuf, false))
		{
			own = TwinCheck::ParsePar2(parBuf, parBuf.Size());
		}
	}
	std::string ownPrint = TwinCheck::Fingerprint(own);
	if (ownPrint.empty())
	{
		return "";
	}

	struct Candidate
	{
		std::string name;
		std::string nzbFilename;
		bool known;
	};
	std::vector<Candidate> candidates;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		auto consider = [&](NzbInfo* other)
			{
				bool sameNameOrKey = other != nzbInfo && other->GetKind() == NzbInfo::nkNzb &&
					DupeCoordinator::SameNameOrKey(other->GetName(), other->GetDupeKey(),
						nzbInfo->GetName(), nzbInfo->GetDupeKey());
				NzbParameter* print = sameNameOrKey ? other->GetParameters()->Find(TwinCheck::FilesParam) : nullptr;
				if (!nzbget_rs_repair_should_consider_twin_candidate(
					other == nzbInfo ? 1 : 0,
					(int)other->GetKind(),
					(int)NzbInfo::nkNzb,
					sameNameOrKey ? 1 : 0,
					other->GetQueuedFilename(),
					print != nullptr ? 1 : 0,
					(print && ownPrint == print->GetValue()) ? 1 : 0) ||
					!FileSystem::FileExists(other->GetQueuedFilename()))
				{
					return;
				}
				candidates.push_back({other->GetName(), other->GetQueuedFilename(), print != nullptr});
			};
		for (NzbInfo* queued : downloadQueue->GetQueue())
		{
			consider(queued);
		}
		for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				consider(historyInfo->GetNzbInfo());
			}
		}
	}
	std::stable_sort(candidates.begin(), candidates.end(),
		[](const Candidate& a, const Candidate& b) { return a.known > b.known; });

	std::vector<const char*> ownNames;
	std::vector<const char*> ownMd5s;
	ownNames.reserve(own.size());
	ownMd5s.reserve(own.size());
	for (const TwinCheck::FileSig& sig : own)
	{
		ownNames.push_back(sig.name.c_str());
		ownMd5s.push_back(sig.md5.c_str());
	}

	for (const Candidate& candidate : candidates)
	{
		if (IsStopped())
		{
			break;
		}
		std::vector<TwinCheck::NzbEntry> entries = TwinCheck::ReadNzbEntries(candidate.nzbFilename.c_str());
		std::vector<uint8_t> entryPar2(entries.size());
		std::vector<int64_t> entrySizes(entries.size());
		for (size_t i = 0; i < entries.size(); i++)
		{
			entryPar2[i] = entries[i].par2 ? 1 : 0;
			entrySizes[i] = entries[i].size;
		}
		ptrdiff_t bestIdx = nzbget_rs_repair_select_best_twin_par2_index(
			entryPar2.data(), entrySizes.data(), entries.size(), TwinCheck::MaxIndexSize);
		if (bestIdx < 0 || (size_t)bestIdx >= entries.size())
		{
			continue;
		}
		const TwinCheck::NzbEntry* index = &entries[(size_t)bestIdx];

		std::vector<char> data;
		std::string path = FetchTwinFile(*index, &data);
		if (path.empty())
		{
			continue;
		}
		int cmp = nzbget_rs_twin_compare_par2_data(
			ownNames.data(), ownMd5s.data(), own.size(),
			ownPrint.c_str(),
			reinterpret_cast<const uint8_t*>(data.data()), data.size());
		if (cmp != 0)
		{
			PrintMessage(Message::mkDetail, "%s: its par2 set %s", candidate.name.c_str(),
				nzbget_rs_repair_twin_par2_reject_reason(cmp));
			FileSystem::DeleteFile(path.c_str());
			m_twinFiles.pop_back();
			continue;
		}

		m_twinName = candidate.name;
		m_twinEntries = std::move(entries);
		m_twinIndex = index->filename;
		m_twinBlockSize = TwinCheck::BlockSize(data.data(), data.size());
		PrintMessage(Message::mkInfo, "Trying the par2 set of twin %s (blocks of %.1f MB)",
			m_twinName.c_str(), nzbget_rs_dqh_stream_repair_bytes_to_mb(m_twinBlockSize));
		return path;
	}
	return "";
}

inline std::vector<std::string> RepairController::PostParChecker::RequestTwinVolumes(int blockNeeded)
{
	struct Volume
	{
		const TwinCheck::NzbEntry* entry;
		int blocks;
	};
	std::vector<Volume> volumes;
	for (const TwinCheck::NzbEntry& entry : m_twinEntries)
	{
		if (nzbget_rs_repair_should_skip_twin_volume_entry(
			entry.par2 ? 1 : 0, entry.filename.c_str(), m_twinIndex.c_str()) != 0)
		{
			continue;
		}
		int blocks = TwinCheck::VolumeBlocks(entry.filename);
		if (blocks < 0 && m_twinBlockSize > 0)
		{
			blocks = nzbget_rs_repair_estimate_twin_vol_blocks(entry.size, m_twinBlockSize);
		}
		if (blocks > 0)
		{
			volumes.push_back({&entry, blocks});
		}
	}
	std::sort(volumes.begin(), volumes.end(), [](const Volume& a, const Volume& b) { return a.blocks < b.blocks; });

	int available = 0;
	std::vector<int> allVolBlocks;
	allVolBlocks.reserve(volumes.size());
	for (const Volume& volume : volumes)
	{
		allVolBlocks.push_back(volume.blocks);
	}
	bool hasEnoughTwinBlocks = nzbget_rs_repair_sum_and_check_twin_blocks(
		allVolBlocks.data(), allVolBlocks.size(), blockNeeded, &available) != 0;
	std::vector<std::string> paths;
	if (!hasEnoughTwinBlocks)
	{
		PrintMessage(Message::mkInfo, "Twin %s holds %i par-block(s), %i needed: not fetched",
			m_twinName.c_str(), available, blockNeeded);
		return paths;
	}

	int left = blockNeeded;
	while (left > 0 && !volumes.empty() && !IsStopped())
	{
		std::vector<int> volBlocks;
		volBlocks.reserve(volumes.size());
		for (const Volume& v : volumes)
		{
			volBlocks.push_back(v.blocks);
		}
		ptrdiff_t idx = nzbget_rs_repair_pick_twin_volume(volBlocks.data(), volBlocks.size(), left);
		if (idx < 0 || (size_t)idx >= volumes.size())
		{
			break;
		}
		auto it = volumes.begin() + idx;
		PrintMessage(Message::mkInfo, "Fetching %s of twin %s for par-repair (%i block(s))",
			it->entry->filename.c_str(), m_twinName.c_str(), it->blocks);
		std::string path = FetchTwinFile(*it->entry);
		if (!path.empty())
		{
			paths.push_back(path);
			left -= it->blocks;
		}
		volumes.erase(it);
	}
	return paths;
}

inline void RepairController::PostParChecker::DeleteTwinFiles()
{
	for (const std::string& path : m_twinFiles)
	{
		FileSystem::DeleteFile(path.c_str());
	}
	m_twinFiles.clear();
	m_twinEntries.clear();
	m_twinName.clear();
	m_twinIndex.clear();
	m_twinBlockSize = 0;
}

inline void RepairController::PostParChecker::StatDupeSources(DupeSourceList* dupeSourceList)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	int totalExtraParBlocks = 0;
	for (DupeSource& dupeSource : dupeSourceList)
	{
		if (dupeSource.GetUsedBlocks() > 0)
		{
			for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
			{
				if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
					historyInfo->GetNzbInfo()->GetId() == dupeSource.GetId())
				{
					historyInfo->GetNzbInfo()->SetExtraParBlocks(
						historyInfo->GetNzbInfo()->GetExtraParBlocks() - dupeSource.GetUsedBlocks());
				}
			}
		}
		totalExtraParBlocks += dupeSource.GetUsedBlocks();
	}

	m_postInfo->GetNzbInfo()->SetExtraParBlocks(m_postInfo->GetNzbInfo()->GetExtraParBlocks() + totalExtraParBlocks);
}

#endif

inline RepairController::RepairController()
{
	debug("Creating RepairController");

#ifndef DISABLE_PARCHECK
	m_parChecker.m_owner = this;
#endif
}

inline void RepairController::Stop()
{
	debug("Stopping RepairController");
	Thread::Stop();
#ifndef DISABLE_PARCHECK
	m_parChecker.Cancel();
#endif
}

#ifndef DISABLE_PARCHECK

inline void RepairController::StartJob(PostInfo* postInfo)
{
	RepairController* repairController = new RepairController();
	repairController->m_postInfo = postInfo;
	repairController->SetAutoDestroy(false);

	postInfo->SetPostThread(repairController);

	repairController->Start();
}

inline void RepairController::Run()
{
	BString<1024> nzbName;
	CString destDir;
	bool dupeRepaired = false;
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		nzbName = m_postInfo->GetNzbInfo()->GetName();
		destDir = m_postInfo->GetNzbInfo()->GetDestDir();
		dupeRepaired = m_postInfo->GetNzbInfo()->GetDupeRecoveredBytes() > 0;
	}

	m_parChecker.SetPostInfo(m_postInfo);
	m_parChecker.SetDestDir(destDir);
	m_parChecker.SetNzbName(nzbName);
	m_parChecker.SetParTime(Util::CurrentTime());
	m_parChecker.SetDownloadSec(m_postInfo->GetNzbInfo()->GetDownloadSec());
	m_parChecker.SetParQuick(nzbget_rs_repair_should_use_par_quick(
		g_Options->GetParQuick() ? 1 : 0,
		m_postInfo->GetForceParFull() ? 1 : 0,
		dupeRepaired ? 1 : 0) != 0);
	m_parChecker.SetForceRepair(m_postInfo->GetForceRepair());

	m_parChecker.PrintMessage(Message::mkInfo, "Checking pars for %s", *nzbName);

	m_parChecker.Execute();
}

inline bool RepairController::AddPar(FileInfo* fileInfo, bool deleted)
{
	bool sameCollection = fileInfo->GetNzbInfo() == m_parChecker.GetPostInfo()->GetNzbInfo();
	if (sameCollection && !deleted)
	{
		NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
		BString<1024> fullFilename;
		fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
		nzbget_rs_free(fullFnBuf);
		m_parChecker.AddParFile(fullFilename);
	}
	else
	{
		m_parChecker.QueueChanged();
	}
	return sameCollection;
}

inline void RepairController::ParCheckCompleted()
{
	m_parChecker.DeleteTwinFiles();
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	PostInfo* postInfo = m_parChecker.GetPostInfo();

	int rawParStatus = 0, setParRepaired = 0, parRepairedVal = 0, waitTime = 0, parSec = 0;
	nzbget_rs_repair_complete_par_check(
		(int)m_parChecker.GetStatus(),
		(int)postInfo->GetNzbInfo()->GetParStatus(),
		(int)ParChecker::psRepaired, (int)ParChecker::psRepairNotNeeded, (int)ParChecker::psRepairPossible,
		(int)NzbInfo::psSkipped, (int)NzbInfo::psSuccess, (int)NzbInfo::psRepairPossible, (int)NzbInfo::psFailure,
		postInfo->GetNzbInfo()->GetDownloadSec(),
		m_parChecker.GetDownloadSec(),
		(int64_t)Util::CurrentTime(),
		(int64_t)m_parChecker.GetParTime(),
		&rawParStatus, &setParRepaired, &parRepairedVal, &waitTime, &parSec);
	postInfo->GetNzbInfo()->SetParStatus((NzbInfo::EParStatus)rawParStatus);
	if (setParRepaired)
	{
		postInfo->SetParRepaired(parRepairedVal != 0);
	}
	postInfo->SetStartTime(postInfo->GetStartTime() + (time_t)waitTime);
	postInfo->GetNzbInfo()->SetParSec(postInfo->GetNzbInfo()->GetParSec() + parSec);

	postInfo->GetNzbInfo()->SetParFull(m_parChecker.GetParFull());

	postInfo->SetWorking(false);
}

inline bool RepairController::RequestMorePars(NzbInfo* nzbInfo, const char* parFilename, int blockNeeded, int* blockFoundOut)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	Blocks availableBlocks;
	Blocks selectedBlocks;
	int blockFound = 0;
	int curBlockFound = 0;

	FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, true, true, &curBlockFound);
	blockFound += curBlockFound;
	if (blockFound < blockNeeded)
	{
		FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, true, false, &curBlockFound);
		blockFound += curBlockFound;
	}
	if (blockFound < blockNeeded)
	{
		FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, false, false, &curBlockFound);
		blockFound += curBlockFound;
	}

	std::sort(availableBlocks.begin(), availableBlocks.end(),
		[](const BlockInfo& block1, const BlockInfo& block2)
		{
			return block1.m_blockCount < block2.m_blockCount;
		});

	if (blockFound >= blockNeeded)
	{
		std::vector<int> sortedCounts;
		sortedCounts.reserve(availableBlocks.size());
		for (const BlockInfo& b : availableBlocks)
		{
			sortedCounts.push_back(b.m_blockCount);
		}
		std::vector<size_t> selectedIdx(availableBlocks.size());
		int remNeeded = blockNeeded;
		size_t selCount = nzbget_rs_repair_select_more_pars(
			sortedCounts.data(), sortedCounts.size(), blockNeeded,
			selectedIdx.data(), selectedIdx.size(), &remNeeded);
		blockNeeded = remNeeded;
		for (size_t i = 0; i < selCount && i < selectedIdx.size(); ++i)
		{
			selectedBlocks.push_back(availableBlocks[selectedIdx[i]]);
		}

		for (BlockInfo& blockInfo : selectedBlocks)
		{
			if (blockInfo.m_fileInfo->GetPaused())
			{
				m_parChecker.PrintMessage(Message::mkInfo, "Unpausing %s%c%s for par-recovery", nzbInfo->GetName(), PATH_SEPARATOR, blockInfo.m_fileInfo->GetFilename());
				blockInfo.m_fileInfo->SetPaused(false);
				blockInfo.m_fileInfo->SetExtraPriority(true);
			}
		}
	}

	bool hasUnpausedParFiles = false;
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (fileInfo->GetParFile() && !fileInfo->GetPaused())
		{
			hasUnpausedParFiles = true;
			break;
		}
	}

	if (blockFoundOut)
	{
		*blockFoundOut = blockFound;
	}

	return blockNeeded <= 0 || hasUnpausedParFiles;
}

inline void RepairController::FindPars(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* parFilename,
	Blocks& blocks, bool strictParName, bool exactParName, int* blockFound)
{
	*blockFound = 0;

	char* baseParFilename = FileSystem::BaseFileName(parFilename);
	int mainBaseLen = 0;
	if (!ParParser::ParseParFilename(baseParFilename, true, &mainBaseLen, nullptr))
	{
		nzbInfo->PrintMessage(Message::mkError, "Internal error: could not parse filename %s", baseParFilename);
		return;
	}
	BString<1024> mainBaseFilename;
	if (mainBaseLen > 0)
	{
		mainBaseFilename.Set(baseParFilename, mainBaseLen);
	}

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		int parsedBlocks = 0;
		bool parsed = ParParser::ParseParFilename(fileInfo->GetFilename(), fileInfo->GetFilenameConfirmed(), nullptr, &parsedBlocks);
		bool sameCollection = !exactParName ||
			ParParser::SameParCollection(fileInfo->GetFilename(),
				FileSystem::BaseFileName(parFilename), fileInfo->GetFilenameConfirmed());
		int blockCount = 0;
		if (nzbget_rs_repair_eval_find_pars_candidate(
			sameCollection ? 1 : 0,
			0,
			(!exactParName && strictParName) ? 1 : 0,
			fileInfo->GetFilename(),
			mainBaseFilename,
			parsed ? 1 : 0,
			parsedBlocks,
			fileInfo->GetSize(),
			m_parChecker.GetBlockSize(),
			&blockCount))
		{
			bool alreadyAdded = false;
			for (BlockInfo& blockInfo : blocks)
			{
				if (blockInfo.m_fileInfo == fileInfo)
				{
					alreadyAdded = true;
					break;
				}
			}

			if (!alreadyAdded)
			{
				blocks.emplace_back(fileInfo, blockCount);
				*blockFound += blockCount;
			}
		}
	}
}

inline void RepairController::UpdateParCheckProgress()
{
	PostInfo* postInfo;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		postInfo = m_parChecker.GetPostInfo();
		if (m_parChecker.GetFileProgress() == 0)
		{
			postInfo->SetProgressLabel(m_parChecker.GetProgressLabel());
		}
		postInfo->SetFileProgress(m_parChecker.GetFileProgress());
		postInfo->SetStageProgress(m_parChecker.GetStageProgress());
		time_t current = Util::CurrentTime();

		int rawStage = (int)postInfo->GetStage();
		int setRepairTime = 0;
		int newRepairSec = postInfo->GetNzbInfo()->GetRepairSec();
		if (nzbget_rs_repair_eval_par_progress_update(
			(int)postInfo->GetStage(),
			(int)m_parChecker.GetStage(),
			(int)PostInfo::ptLoadingPars,
			(int)PostInfo::ptVerifyingSources,
			(int)PostInfo::ptRepairing,
			(int)PostInfo::ptVerifyingRepaired,
			(int64)current,
			(int64)m_parChecker.GetRepairTime(),
			newRepairSec,
			&rawStage,
			&setRepairTime,
			&newRepairSec) != 0)
		{
			postInfo->SetStage((PostInfo::EStage)rawStage);
			postInfo->SetStageTime(current);
			if (setRepairTime)
			{
				m_parChecker.SetRepairTime(current);
			}
			else if (postInfo->GetStage() == PostInfo::ptVerifyingRepaired)
			{
				postInfo->GetNzbInfo()->SetRepairSec(newRepairSec);
			}
		}

		bool parCancel = false;
		if (!IsStopped())
		{
			int estimatedRepairTime = 0;
			if (nzbget_rs_repair_check_par_time_limit(
				g_Options->GetParTimeLimit(),
				m_parChecker.GetStage() == PostParChecker::ptRepairing ? 1 : 0,
				(int64)(current - postInfo->GetStageTime()),
				postInfo->GetStageProgress(),
				&estimatedRepairTime) != 0)
			{
				debug("Estimated repair time %i seconds", estimatedRepairTime);
				m_parChecker.PrintMessage(Message::mkWarning, "Cancelling par-repair for %s, estimated repair time (%i minutes) exceeds allowed repair time",
					m_parChecker.GetInfoName(),
					estimatedRepairTime / 60);
				parCancel = true;
			}
		}

		if (parCancel)
		{
			Stop();
		}
	}

	CheckPauseState(postInfo);
}

inline void RepairController::CheckPauseState(PostInfo* postInfo)
{
	if (g_WorkState->GetPausePostProcess() && !postInfo->GetNzbInfo()->GetForcePriority())
	{
		time_t stageTime = postInfo->GetStageTime();
		time_t startTime = postInfo->GetStartTime();
		time_t parTime = m_parChecker.GetParTime();
		time_t repairTime = m_parChecker.GetRepairTime();
		time_t waitTime = Util::CurrentTime();

		while (g_WorkState->GetPausePostProcess() &&
			!postInfo->GetNzbInfo()->GetForcePriority() &&
			!IsStopped())
		{
			Util::Sleep(50);

			int64 newStage = 0, newStart = 0, newPar = 0, newRepair = 0;
			nzbget_rs_repair_calc_paused_timestamps(
				(int64)Util::CurrentTime(), (int64)waitTime,
				(int64)stageTime, (int64)startTime, (int64)parTime, (int64)repairTime,
				&newStage, &newStart, &newPar, &newRepair);
			if (stageTime > 0)
			{
				postInfo->SetStageTime((time_t)newStage);
			}
			if (startTime > 0)
			{
				postInfo->SetStartTime((time_t)newStart);
			}
			if (parTime > 0)
			{
				m_parChecker.SetParTime((time_t)newPar);
			}
			if (repairTime > 0)
			{
				m_parChecker.SetRepairTime((time_t)newRepair);
			}
		}
	}
}

#endif

namespace
{
	template <typename Ctx, typename R, typename F>
	R RepairMapCall(void* rawCtx, R failed, F f) noexcept
	{
		Ctx* c = static_cast<Ctx*>(rawCtx);
		if (c->Error)
		{
			return failed;
		}
		try
		{
			return f(c);
		}
		catch (...)
		{
			c->Error = std::current_exception();
			return failed;
		}
	}

	template <typename Ctx, typename F>
	void RepairMapCallVoid(void* rawCtx, F f) noexcept
	{
		Ctx* c = static_cast<Ctx*>(rawCtx);
		if (c->Error)
		{
			return;
		}
		try
		{
			f(c);
		}
		catch (...)
		{
			c->Error = std::current_exception();
		}
	}
}

inline void StreamRepairController::StartJob(PostInfo* postInfo)
{
	StreamRepairController* streamRepairController = new StreamRepairController();
	streamRepairController->m_postInfo = postInfo;
	streamRepairController->SetAutoDestroy(false);

	postInfo->SetPostThread(streamRepairController);

	streamRepairController->Start();
}

inline void StreamRepairController::StartLive(NzbInfo* nzbInfo)
{
	if (DupeArticleFallback::ShouldDeferToPar(nzbInfo))
	{
		return;
	}

	StreamRepairController* streamRepairController = new StreamRepairController();
	streamRepairController->m_liveMode = true;
	streamRepairController->m_nzbId = nzbInfo->GetId();
	streamRepairController->SetAutoDestroy(true);

	nzbInfo->SetLiveRepairThread(streamRepairController);

	streamRepairController->Start();
}

inline void StreamRepairController::Run()
{
	if (m_liveMode)
	{
		RunLive();
		return;
	}

	BString<1024> nzbName;
	CString destDir;
	std::vector<RepairTarget> targets;
	std::vector<DonorSource> donors;
	std::vector<CString> memberNames;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		nzbName = nzbInfo->GetName();
		destDir = nzbInfo->GetDestDir();
		CollectTargets(nzbInfo, targets, memberNames);
		CollectDonors(downloadQueue, nzbInfo, donors);
		m_postInfo->SetProgressLabel("Repairing files from duplicates");
	}

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("stream repair", *nzbName);
	BString<1024> infoName;
	infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	if (targets.empty())
	{
		PrintMessage(Message::mkInfo, "Nothing to stream-repair for %s", *nzbName);
	}
	else if (donors.empty())
	{
		PrintMessage(Message::mkInfo,
			"No duplicate collections found for stream repair of %s", *nzbName);
	}
	else
	{
		StartWatchdog(targets);
		ComputePositionalRanks(destDir, targets, memberNames);
		ExecRepair(destDir, targets, donors, memberNames);

		bool anyHoles = false;
		for (RepairTarget& target : targets)
		{
			anyHoles |= target.NeedsRepair();
		}
		if (anyHoles && !IsStopped())
		{
			ExecCrossPackRepair(destDir, targets, donors, memberNames);
		}
		StopWatchdog(nzbName, targets);
	}

	ReportRemainingHoles(targets);
	m_targets = std::move(targets);
	RepairCompleted();
}

inline void StreamRepairController::RunLive()
{
	BString<1024> nzbName;
	CString destDir;
	std::vector<RepairTarget> targets;
	std::vector<DonorSource> donors;
	std::vector<CString> memberNames;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo || nzbInfo->GetLiveRepairThread() != this)
		{
			return;
		}
		nzbName = nzbInfo->GetName();
		destDir = nzbInfo->GetDestDir();
		CollectTargets(nzbInfo, targets, memberNames);
		CollectDonors(downloadQueue, nzbInfo, donors);

		for (RepairTarget& target : targets)
		{
			for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
			{
				if (job.GetFileId() == target.FileId)
				{
					target.PatchEligible = !job.GetLiveAttempted();
					job.SetLiveAttempted(true);
					break;
				}
			}
		}
	}

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("live stream repair", *nzbName);
	BString<1024> infoName;
	infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	bool anyEligible = false;
	for (RepairTarget& target : targets)
	{
		anyEligible |= target.PatchEligible && target.NeedsRepair();
	}

	if (anyEligible && !donors.empty())
	{
		PrintMessage(Message::mkInfo, "Starting live stream repair for %s", *nzbName);

		StartWatchdog(targets);
		ComputePositionalRanks(destDir, targets, memberNames);
		ExecRepair(destDir, targets, donors, memberNames);

		bool anyHoles = false;
		for (RepairTarget& target : targets)
		{
			anyHoles |= target.PatchEligible && target.NeedsRepair();
		}
		if (anyHoles && !IsStopped())
		{
			RefreshLiveNames(targets, memberNames);
			ExecCrossPackRepair(destDir, targets, donors, memberNames);
		}
		StopWatchdog(nzbName, targets);
	}

	RepairCompletedLive(targets);
}

inline void StreamRepairController::RefreshLiveNames(std::vector<RepairTarget>& targets,
	std::vector<CString>& memberNames)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (!nzbInfo || nzbInfo->GetLiveRepairThread() != this)
	{
		return;
	}

	for (RepairTarget& target : targets)
	{
		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetId() == target.FileId)
			{
				target.Filename = completedFile.GetFilename();
				target.IsParFile = target.IsParFile || completedFile.GetParFile();
				break;
			}
		}
	}

	memberNames.clear();
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		memberNames.emplace_back(completedFile.GetFilename());
	}
}

inline void StreamRepairController::RefreshLiveTargetName(RepairTarget& target)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (!nzbInfo || nzbInfo->GetLiveRepairThread() != this)
	{
		return;
	}

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetId() == target.FileId)
		{
			target.Filename = completedFile.GetFilename();
			break;
		}
	}
}

inline void StreamRepairController::RepairCompletedLive(std::vector<RepairTarget>& targets)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (!nzbInfo)
	{
		return;
	}
	if (nzbInfo->GetLiveRepairThread() != this)
	{
		nzbInfo->SetDupeRecoveredBytes(nzbInfo->GetDupeRecoveredBytes() + m_recoveredBytes);
		return;
	}

	if (m_recoveredBytes > 0 || m_recoveredHoles > 0)
	{
		nzbInfo->SetDupeRecoveredBytes(nzbInfo->GetDupeRecoveredBytes() + m_recoveredBytes);
		nzbInfo->SetDupeRecoveredHoles(nzbInfo->GetDupeRecoveredHoles() + m_recoveredHoles);
	}

	for (RepairTarget& target : targets)
	{
		if (!target.PatchEligible)
		{
			continue;
		}
		for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
		{
			if (job.GetFileId() == target.FileId)
			{
				if (target.Recreated)
				{
					job.SetDecodedFileSize(target.DecodedFileSize);
					MarkRecreated(nzbInfo, target);
				}
				job.SetHoles(std::move(target.Holes));
				break;
			}
		}
	}

	nzbInfo->SetLiveRepairThread(nullptr);
	if (nzbInfo->GetPostInfo() && !nzbInfo->GetUnpackThread())
	{
		nzbInfo->GetPostInfo()->SetWorking(false);
	}

	downloadQueue->Save();
}

inline bool StreamRepairController::DonorDead(const DonorSource& donor, NzbInfo* donorNzb)
{
	auto it = m_donorDead.find(*donor.QueuedFilename);
	if (it != m_donorDead.end())
	{
		return it->second;
	}

	std::vector<FileInfo*> files;
	for (int pass = 0; pass < 2 && files.empty(); pass++)
	{
		for (FileInfo* fileInfo : donorNzb->GetFileList())
		{
			if (fileInfo->GetTotalArticles() > 0 && (pass == 1 || !fileInfo->GetParFile()))
			{
				files.push_back(fileInfo);
			}
		}
	}
	bool dead = false;
	std::vector<DupeProbe::Sample> samples = DupeProbe::SamplesOf(files);
	if ((int)samples.size() >= DupeProbe::MinArticles && !IsStopped())
	{
		m_checkingDonor++;
		DupeProbe::Verdict verdict = DupeProbe::Check(std::move(samples), DonorCheckSec);
		m_checkingDonor--;
		dead = verdict.Dead();
		if (dead)
		{
			PrintMessage(Message::mkInfo,
				"Skipping duplicate %s for stream repair: none of %i sampled articles exists on any server",
				*donor.InfoName, DupeProbe::SampleCount);
		}
	}
	m_donorDead[*donor.QueuedFilename] = dead;
	return dead;
}

inline void StreamRepairController::StartWatchdog(const std::vector<RepairTarget>& targets)
{
	m_fetcher.SetWaitCounter(&m_waitingForConnection);
	m_batchFetcher.SetWaitCounter(&m_waitingForConnection);
	int timeout = g_Options->GetDupeStreamTimeout();
	if (timeout <= 0)
	{
		return;
	}

	m_missingBytes = 0;
	for (const RepairTarget& target : targets)
	{
		m_missingBytes += target.DecodedFileSize > 0
			? DupeStreamRepair::TotalSize(target.Holes)
			: target.EncodedSize;
	}

	int upTimeSec = 0, downloadTimeSec = 0;
	int64 allBytes = 0;
	bool standBy = false;
	g_StatMeter->CalcTotalStat(&upTimeSec, &downloadTimeSec, &allBytes, &standBy);
	int64 downloadRate = downloadTimeSec > 0 ? allBytes / downloadTimeSec : 0;

	m_watchdog = std::thread([this, timeout, downloadRate]()
		{
			std::unique_lock<std::mutex> lock(m_watchdogMutex);
			int64 startProgress = m_progressBytes;
			int64 progress = startProgress;
			auto start = std::chrono::steady_clock::now();
			auto begun = start;
			auto lastProgress = start;
			auto lastTick = start;
			auto waiting = [this]() { return m_waitingForConnection > 0 || m_checkingDonor > 0; };
			while (!m_watchdogDone)
			{
				m_watchdogCond.wait_for(lock, std::chrono::milliseconds(500));
				if (m_watchdogDone)
				{
					break;
				}
				auto now = std::chrono::steady_clock::now();
				if (waiting())
				{
					start += now - lastTick;
				}
				lastTick = now;
				int64 sinceLastProgressSec =
					std::chrono::duration_cast<std::chrono::seconds>(now - lastProgress).count();
				int64 sinceBegunSec =
					std::chrono::duration_cast<std::chrono::seconds>(now - begun).count();
				int64 sinceStartSec =
					std::chrono::duration_cast<std::chrono::seconds>(now - start).count();
				double elapsedSec = std::chrono::duration<double>(now - start).count();
				int64 newProgress = progress;
				int resetLastProgress = 0;
				NzbgetRsBuf reasonBuf{nullptr, 0, 0};
				int shouldStop = nzbget_rs_stream_watchdog_eval_tick(
					m_progressBytes, progress, waiting() ? 1 : 0, m_materializing.load(),
					sinceLastProgressSec, sinceBegunSec, sinceStartSec, elapsedSec,
					timeout, MaxRunFactor, MaxConnectionWaitSec, SlowRepairFactor,
					downloadRate, startProgress, m_missingBytes,
					&newProgress, &resetLastProgress, &reasonBuf);
				progress = newProgress;
				if (resetLastProgress)
				{
					lastProgress = now;
				}
				if (shouldStop && reasonBuf.data && reasonBuf.len > 0)
				{
					m_slowReason.Set(reasonBuf.data, (int)reasonBuf.len);
					nzbget_rs_free(reasonBuf);
					break;
				}
				nzbget_rs_free(reasonBuf);
			}
			if (!m_watchdogDone)
			{
				m_timedOut = true;
				lock.unlock();
				Stop();
			}
		});
}

inline void StreamRepairController::StopWatchdog(const char* nzbName, const std::vector<RepairTarget>& targets)
{
	if (!m_watchdog.joinable())
	{
		return;
	}
	{
		std::lock_guard<std::mutex> lock(m_watchdogMutex);
		m_watchdogDone = true;
	}
	m_watchdogCond.notify_all();
	m_watchdog.join();
	if (m_timedOut)
	{
		int64 missing = 0;
		int unsized = 0;
		for (const RepairTarget& target : targets)
		{
			missing += DupeStreamRepair::TotalSize(target.Holes);
			unsized += target.DecodedFileSize <= 0 ? 1 : 0;
		}
		NzbgetRsBuf wholeFilesBuf = nzbget_rs_stream_format_whole_files_suffix(unsized);
		PrintMessage(Message::mkWarning,
			"Stream repair of %s stopped: %s (option DupeStreamTimeout); %.1f MB recovered, %.1f MB%s still missing",
			nzbName, *m_slowReason,
			nzbget_rs_dqh_stream_repair_bytes_to_mb(m_recoveredBytes),
			nzbget_rs_dqh_stream_repair_bytes_to_mb(missing),
			wholeFilesBuf.data ? wholeFilesBuf.data : "");
		nzbget_rs_free(wholeFilesBuf);
	}
}

inline void StreamRepairController::Stop()
{
	m_batchFetcher.Stop();
	m_fetcher.Stop();
	Thread::Stop();
}

inline void StreamRepairController::CollectTargets(NzbInfo* nzbInfo, std::vector<RepairTarget>& targets,
	std::vector<CString>& memberNames)
{
	m_hasParFiles = nzbInfo->GetParSize() > 0;

	NzbParameter* parameter = nzbInfo->GetParameters()->Find("*Unpack:Password");
	if (parameter)
	{
		m_targetPassword = parameter->GetValue();
	}

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		memberNames.emplace_back(completedFile.GetFilename());
	}

	for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
	{
		if (nzbget_rs_stream_is_par2_job_or_file(
			job.GetParFile() ? 1 : 0, job.GetFilename()) != 0)
		{
			continue;
		}
		RepairTarget target;
		target.FileId = job.GetFileId();
		target.Filename = job.GetFilename();
		target.DecodedFileSize = job.GetDecodedFileSize();
		target.FailedSize = job.GetFailedSize();
		target.MissedSize = job.GetMissedSize();
		target.FailedArticles = job.GetFailedArticles();
		target.IsParFile = job.GetParFile();
		target.Holes = *job.GetHoles();
		target.StepsHash = job.GetStepsHash();
		target.EncodedSize = job.GetFailedSize() + job.GetMissedSize();
		target.ArticleCount = job.GetFailedArticles();

		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetId() == target.FileId)
			{
				target.Filename = completedFile.GetFilename();
				target.IsParFile = target.IsParFile || completedFile.GetParFile();
				break;
			}
		}

		if (nzbget_rs_stream_is_par2_job_or_file(
			target.IsParFile ? 1 : 0, target.Filename))
		{
			continue;
		}
		targets.push_back(std::move(target));
	}
}

inline void StreamRepairController::ComputePositionalRanks(const char* destDir,
	std::vector<RepairTarget>& targets, const std::vector<CString>& memberNames)
{
	std::vector<const char*> names;
	std::vector<int64_t> sizes;
	names.reserve(memberNames.size());
	sizes.reserve(memberNames.size());
	for (const CString& memberName : memberNames)
	{
		NzbgetRsBuf memberPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			destDir, PATH_SEPARATOR, *memberName);
		BString<1024> memberPath;
		memberPath.Set(memberPathBuf.data ? memberPathBuf.data : "", (int)memberPathBuf.len);
		nzbget_rs_free(memberPathBuf);
		names.push_back(*memberName);
		sizes.push_back(FileSystem::FileSize(memberPath));
	}
	for (RepairTarget& target : targets)
	{
		int rank = -1;
		int windowSize = 0;
		nzbget_rs_stream_compute_positional_rank(
			names.data(), sizes.data(), names.size(),
			target.Filename, target.DecodedFileSize, &rank, &windowSize);
		target.PositionalRank = rank;
		target.PositionalWindow = windowSize;
	}
}

inline void StreamRepairController::CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo,
	std::vector<DonorSource>& donors)
{
	RawNzbList donorNzbs;

	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		if (queuedNzbInfo != nzbInfo &&
			queuedNzbInfo->GetKind() == NzbInfo::nkNzb &&
			queuedNzbInfo->GetDupeMode() != dmForce &&
			DupeCoordinator::SameNameOrKey(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			donorNzbs.push_back(queuedNzbInfo);
		}
	}

	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			historyInfo->GetNzbInfo()->GetDupeMode() != dmForce &&
			DupeCoordinator::SameNameOrKey(historyInfo->GetNzbInfo()->GetName(),
				historyInfo->GetNzbInfo()->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			donorNzbs.push_back(historyInfo->GetNzbInfo());
		}
	}

	if (!donorNzbs.empty())
	{
		std::vector<int32_t> dupeScores;
		std::vector<int32_t> donorIds;
		std::vector<size_t> order(donorNzbs.size());
		dupeScores.reserve(donorNzbs.size());
		donorIds.reserve(donorNzbs.size());
		for (NzbInfo* d : donorNzbs)
		{
			dupeScores.push_back(d->GetDupeScore());
			donorIds.push_back(d->GetId());
		}
		size_t n = nzbget_rs_stream_order_donor_indices(
			dupeScores.data(), donorIds.data(), donorNzbs.size(), order.data());
		RawNzbList sortedDonors;
		sortedDonors.reserve(n);
		for (size_t i = 0; i < n; i++)
		{
			sortedDonors.push_back(donorNzbs[order[i]]);
		}
		donorNzbs = std::move(sortedDonors);
	}

	for (NzbInfo* donorNzbInfo : donorNzbs)
	{
		if ((nzbInfo->GetFullContentHash() > 0 &&
				nzbInfo->GetFullContentHash() == donorNzbInfo->GetFullContentHash()) ||
			Util::EmptyStr(donorNzbInfo->GetQueuedFilename()))
		{
			continue;
		}
		{
			DonorSource donor;
			donor.QueuedFilename = donorNzbInfo->GetQueuedFilename();
			donor.InfoName = donorNzbInfo->GetName();
			NzbParameter* parameter = donorNzbInfo->GetParameters()->Find("*Unpack:Password");
			if (parameter)
			{
				donor.Password = parameter->GetValue();
			}
			donors.push_back(std::move(donor));
		}
	}
}

inline void StreamRepairController::ExecRepair(const char* destDir,
	std::vector<RepairTarget>& targets, std::vector<DonorSource>& donors,
	const std::vector<CString>& memberNames)
{
	int primaryConnections = 0;
	for (NewsServer* server : g_ServerPool->GetServers())
	{
		if (server->GetActive() && server->GetNormLevel() == 0)
		{
			primaryConnections += server->GetMaxConnections();
		}
	}
	m_batchFetcher.SetWorkerCount(m_liveMode ? 0 :
		DupeStreamRepair::StreamFetchWorkerCount(primaryConnections));
	std::set<std::string> triedPostings;

	for (DonorSource& donor : donors)
	{
		if (IsStopped())
		{
			break;
		}

		bool anyHoles = false;
		for (RepairTarget& target : targets)
		{
			anyHoles |= target.PatchEligible && target.NeedsRepair();
		}
		if (!anyHoles)
		{
			break;
		}

		std::unique_ptr<NzbInfo> donorNzb = DupeArticleFallback::ParseDonorNzb(donor.QueuedFilename);
		if (!donorNzb)
		{
			continue;
		}
		if (!triedPostings.insert(DupeStreamRepair::BuildDonorKey(
			donorNzb.get(), donor.Password)).second)
		{
			PrintMessage(Message::mkInfo,
				"Skipping another copy of duplicate posting %s", *donor.InfoName);
			continue;
		}
		if (DonorDead(donor, donorNzb.get()))
		{
			continue;
		}

		if (m_postInfo)
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(BString<1024>("Repairing from duplicate %s", *donor.InfoName));
			m_postInfo->SetStageProgress(0);
		}

		std::vector<const char*> memberNamePtrs(memberNames.size());
		for (size_t i = 0; i < memberNames.size(); i++)
		{
			memberNamePtrs[i] = *memberNames[i];
		}

		std::vector<const char*> targetFilenames(targets.size());
		std::vector<int64_t> targetEncodedSizes(targets.size());
		std::vector<int64_t> targetDecodedSizes(targets.size());
		std::vector<int> targetArticleCounts(targets.size());
		std::vector<int> targetPositionalRanks(targets.size());
		std::vector<int> targetPositionalWindows(targets.size());
		std::vector<uint64_t> targetStepsHashes(targets.size());
		std::vector<uint8_t> targetPatchEligible(targets.size());
		std::vector<size_t> targetHoleCounts(targets.size());
		std::vector<NzbgetRsByteRange> targetHolesFlat;
		for (size_t t = 0; t < targets.size(); t++)
		{
			targetFilenames[t] = *targets[t].Filename;
			targetEncodedSizes[t] = targets[t].EncodedSize;
			targetDecodedSizes[t] = targets[t].DecodedFileSize;
			targetArticleCounts[t] = targets[t].ArticleCount;
			targetPositionalRanks[t] = targets[t].PositionalRank;
			targetPositionalWindows[t] = targets[t].PositionalWindow;
			targetStepsHashes[t] = targets[t].StepsHash;
			targetPatchEligible[t] = targets[t].PatchEligible ? 1 : 0;
			targetHoleCounts[t] = targets[t].Holes.size();
			for (const StreamRange& h : targets[t].Holes)
			{
				targetHolesFlat.push_back({h.Offset, h.Size});
			}
		}

		std::vector<FileInfo*> donorFiles;
		std::vector<const char*> donorFileNames;
		std::vector<int64_t> donorFileSizes;
		std::vector<uint8_t> donorFileIsPar;
		std::vector<uint64_t> donorFileStepsHashes;
		std::vector<size_t> donorArticleCounts;
		std::vector<int32_t> donorArticleSizesFlat;
		std::vector<std::shared_ptr<std::vector<CString>>> donorGroupsCache;
		for (FileInfo* df : donorNzb->GetFileList())
		{
			donorFiles.push_back(df);
			donorFileNames.push_back(df->GetFilename());
			donorFileSizes.push_back(df->GetSize());
			donorFileIsPar.push_back(df->GetParFile() ? 1 : 0);
			donorArticleCounts.push_back(df->GetArticles()->size());
			for (const std::unique_ptr<ArticleInfo>& a : *df->GetArticles())
			{
				donorArticleSizesFlat.push_back(a->GetSize());
			}
			const std::vector<int32_t>& artSizes = donorArticleCounts.back() > 0
				? std::vector<int32_t>(
					donorArticleSizesFlat.end() - (ptrdiff_t)donorArticleCounts.back(),
					donorArticleSizesFlat.end())
				: std::vector<int32_t>{};
			donorFileStepsHashes.push_back(
				nzbget_rs_dupe_fallback_steps_hash(artSizes.data(), artSizes.size()));
			auto grp = std::make_shared<std::vector<CString>>();
			for (const CString& g : *df->GetGroups())
			{
				grp->emplace_back(*g);
			}
			donorGroupsCache.push_back(std::move(grp));
		}

		struct M1DonorCtx
		{
			StreamRepairController* Controller;
			std::vector<RepairTarget>* Targets;
			std::vector<FileInfo*>* DonorFiles;
			std::vector<std::shared_ptr<std::vector<CString>>>* DonorGroupsCache;
			ArticleFetcher::FetchedArticle CurrentFetched;
			std::exception_ptr Error;
		} donorCtx{this, &targets, &donorFiles, &donorGroupsCache, {}, nullptr};

		nzbget_rs_stream_repair_process_donor(
			destDir,
			m_liveMode ? 1 : 0,
			memberNamePtrs.data(),
			memberNamePtrs.size(),
			targetFilenames.data(),
			targetEncodedSizes.data(),
			targetDecodedSizes.data(),
			targetArticleCounts.data(),
			targetPositionalRanks.data(),
			targetPositionalWindows.data(),
			targetStepsHashes.data(),
			targetPatchEligible.data(),
			targetHoleCounts.data(),
			targetHolesFlat.data(),
			targets.size(),
			*donor.InfoName,
			donorFileNames.data(),
			donorFileSizes.data(),
			donorFileIsPar.data(),
			donorFileStepsHashes.data(),
			donorArticleCounts.data(),
			donorArticleSizesFlat.data(),
			donorFiles.size(),
			&donorCtx,
			[](void* rawCtx) -> int
			{
				return RepairMapCall<M1DonorCtx>(rawCtx, 1, [](M1DonorCtx* c)
				{
					return c->Controller->IsStopped() ? 1 : 0;
				});
			},
			[](void* rawCtx, size_t targetIdx) -> const char*
			{
				return RepairMapCall<M1DonorCtx>(rawCtx, static_cast<const char*>(nullptr), [=](M1DonorCtx* c) -> const char*
				{
					if (targetIdx >= c->Targets->size())
					{
						return nullptr;
					}
					c->Controller->RefreshLiveTargetName((*c->Targets)[targetIdx]);
					return *(*c->Targets)[targetIdx].Filename;
				});
			},
			[](void* rawCtx, int32_t memberIndex, const int32_t* partIndices, size_t count)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [=](M1DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return;
					}
					FileInfo* df = (*c->DonorFiles)[memberIndex];
					const auto& grp = (*c->DonorGroupsCache)[memberIndex];
					std::vector<ArticleBatchFetcher::Request> requests;
					requests.reserve(count);
					for (size_t i = 0; i < count; i++)
					{
						int32_t pi = partIndices[i];
						if (pi >= 0 && (size_t)pi < df->GetArticles()->size())
						{
							ArticleInfo* article = (*df->GetArticles())[pi].get();
							requests.push_back({article->GetMessageId(), grp});
						}
					}
					c->Controller->m_batchFetcher.Begin(std::move(requests));
				});
			},
			[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
				const uint8_t** outData, size_t* outDataLen) -> int
			{
				return RepairMapCall<M1DonorCtx>(rawCtx, 0, [=](M1DonorCtx* c)
				{
					c->CurrentFetched = ArticleFetcher::FetchedArticle();
					if (!c->Controller->m_batchFetcher.Next(c->CurrentFetched))
					{
						return 0;
					}
					*outSuccess = c->CurrentFetched.Success ? 1 : 0;
					*outOffset = c->CurrentFetched.Offset;
					*outFileSize = c->CurrentFetched.FileSize;
					*outData = reinterpret_cast<const uint8_t*>(c->CurrentFetched.Data.data());
					*outDataLen = c->CurrentFetched.Data.size();
					return 1;
				});
			},
			[](void* rawCtx)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [](M1DonorCtx* c)
				{
					c->Controller->m_batchFetcher.CancelRemaining();
				});
			},
			[](void* rawCtx, int64_t bytes)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [bytes](M1DonorCtx* c)
				{
					c->Controller->m_progressBytes += bytes;
				});
			},
			[](void* rawCtx, int64_t bytes)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [bytes](M1DonorCtx* c)
				{
					c->Controller->m_recoveredBytes += bytes;
				});
			},
			[](void* rawCtx, size_t targetIdx, const char* filename, int64_t decodedFileSize,
				int recreated, const NzbgetRsByteRange* holes, size_t holeCount)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [=](M1DonorCtx* c)
				{
					if (targetIdx >= c->Targets->size())
					{
						return;
					}
					RepairTarget& target = (*c->Targets)[targetIdx];
					if (filename && *filename)
					{
						target.Filename = filename;
					}
					target.DecodedFileSize = decodedFileSize;
					if (recreated != 0)
					{
						target.Recreated = true;
					}
					target.Holes.assign(
						reinterpret_cast<const StreamRange*>(holes),
						reinterpret_cast<const StreamRange*>(holes) + holeCount);
				});
			},
			[](void* rawCtx, int32_t kind, const char* text)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [=](M1DonorCtx* c)
				{
					Message::EKind msgKind = kind == 3 ? Message::mkDetail :
						kind == 2 ? Message::mkError :
						kind == 1 ? Message::mkWarning : Message::mkInfo;
					c->Controller->PrintMessage(msgKind, "%s", text);
				});
			});
		if (donorCtx.Error)
		{
			std::rethrow_exception(donorCtx.Error);
		}
	}
}

inline void StreamRepairController::MarkRepaired(NzbInfo* nzbInfo, const RepairTarget& target)
{
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetId() == target.FileId &&
			completedFile.GetStatus() != CompletedFile::cfSuccess)
		{
			completedFile.SetStatus(CompletedFile::cfSuccess);
			g_DiskState->DiscardFile(target.FileId, true, true, true);
			break;
		}
	}
}

inline void StreamRepairController::MarkRecreated(NzbInfo* nzbInfo, const RepairTarget& target)
{
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetId() == target.FileId &&
			completedFile.GetStatus() == CompletedFile::cfFailure)
		{
			completedFile.SetStatus(CompletedFile::cfPartial);
			break;
		}
	}
}

inline void StreamRepairController::ReportRemainingHoles(std::vector<RepairTarget>& targets)
{
	bool interrupted = Interrupted();
	if (interrupted)
	{
		int unfinished = (int)std::count_if(targets.begin(), targets.end(),
			[](const RepairTarget& target)
			{
				return target.DecodedFileSize <= 0 || !target.Holes.empty();
			});
		m_holesRemain |= unfinished > 0;
		PrintMessage(Message::mkInfo, "Stream repair interrupted with %i file(s) still incomplete", unfinished);
	}

	const char* parDisposition = m_hasParFiles ? "left to par-repair" : "no par2 files to repair it";
	for (RepairTarget& target : targets)
	{
		int targetStatus = nzbget_rs_stream_classify_remaining_hole_target(
			interrupted ? 1 : 0, target.DecodedFileSize, target.Holes.empty() ? 1 : 0,
			target.IsParFile ? 1 : 0, target.FailedArticles, target.FailedSize, target.MissedSize,
			&m_recoveredArticles, &m_recoveredFailedSize, &m_recoveredFailedParSize);
		if (targetStatus == 1)
		{
			continue;
		}
		if (targetStatus == 2)
		{
			m_holesRemain = true;
			PrintMessage(Message::mkInfo,
				"Stream repair of %s: no duplicate could supply the file (%s)",
				*target.Filename, parDisposition);
		}
		else if (targetStatus == 3)
		{
			m_holesRemain = true;
			int64 missing = DupeStreamRepair::TotalSize(target.Holes);
			PrintMessage(Message::mkInfo,
				"Stream repair of %s: %.1f MB still missing after stream repair (%s)",
				*target.Filename, nzbget_rs_dqh_stream_repair_bytes_to_mb(missing),
				parDisposition);
			if (nzbget_rs_stream_is_incomplete_nzb_hole(missing, target.EncodedSize) != 0)
			{
				PrintMessage(Message::mkWarning,
					"%s is %.1f MB according to its articles, but the nzb's articles cover only "
					"%.1f MB of it: the nzb is incomplete",
					*target.Filename, nzbget_rs_dqh_stream_repair_bytes_to_mb(target.DecodedFileSize),
					nzbget_rs_dqh_stream_repair_bytes_to_mb(target.DecodedFileSize - missing));
			}
		}
	}
}

inline void StreamRepairController::ExecCrossPackRepair(const char* destDir,
	std::vector<RepairTarget>& targets, std::vector<DonorSource>& donors,
	const std::vector<CString>& memberNames)
{
	std::vector<const char*> memberNamePtrs(memberNames.size());
	for (size_t i = 0; i < memberNames.size(); i++)
	{
		memberNamePtrs[i] = *memberNames[i];
	}

	std::vector<const char*> targetFilenames(targets.size());
	std::vector<int64_t> targetDecodedSizes(targets.size());
	std::vector<uint8_t> targetPatchEligible(targets.size());
	std::vector<size_t> targetHoleCounts(targets.size());
	std::vector<NzbgetRsByteRange> targetHolesFlat;
	for (size_t t = 0; t < targets.size(); t++)
	{
		targetFilenames[t] = *targets[t].Filename;
		targetDecodedSizes[t] = targets[t].DecodedFileSize;
		targetPatchEligible[t] = targets[t].PatchEligible ? 1 : 0;
		targetHoleCounts[t] = targets[t].Holes.size();
		for (const StreamRange& h : targets[t].Holes)
		{
			targetHolesFlat.push_back({h.Offset, h.Size});
		}
	}

	struct InitCtx
	{
		StreamRepairController* Controller;
		std::exception_ptr Error;
	} initCtx{this, nullptr};

	NzbgetRsCrossPackSession* session = nzbget_rs_cross_pack_session_new(
		destDir,
		memberNamePtrs.data(),
		memberNamePtrs.size(),
		targetFilenames.data(),
		targetDecodedSizes.data(),
		targetPatchEligible.data(),
		targetHoleCounts.data(),
		targetHolesFlat.data(),
		targets.size(),
		m_targetPassword.Empty() ? nullptr : *m_targetPassword,
		&initCtx,
		[](void* rawCtx) -> int
		{
			return RepairMapCall<InitCtx>(rawCtx, 1, [](InitCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t kind, const char* text)
		{
			RepairMapCallVoid<InitCtx>(rawCtx, [=](InitCtx* c)
			{
				Message::EKind msgKind = kind == 2 ? Message::mkError :
					kind == 1 ? Message::mkWarning : Message::mkInfo;
				c->Controller->PrintMessage(msgKind, "%s", text);
			});
		},
#ifdef DEBUG
		[](void* rawCtx, const char* text)
		{
			RepairMapCallVoid<InitCtx>(rawCtx, [=](InitCtx*)
			{
				debug("%s", text ? text : "");
			});
		}
#else
		nullptr
#endif
	);
	if (initCtx.Error)
	{
		nzbget_rs_cross_pack_session_free(session);
		std::rethrow_exception(initCtx.Error);
	}
	if (!session)
	{
		return;
	}
	struct SessionGuard
	{
		NzbgetRsCrossPackSession* Ptr;
		~SessionGuard() { nzbget_rs_cross_pack_session_free(Ptr); }
	} sessionGuard{session};

	std::set<std::string> triedPostings;

	for (DonorSource& donor : donors)
	{
		if (IsStopped() || !nzbget_rs_cross_pack_session_has_inner_holes(session))
		{
			break;
		}

		std::unique_ptr<NzbInfo> donorNzb =
			DupeArticleFallback::ParseDonorNzb(donor.QueuedFilename);
		if (!donorNzb)
		{
			continue;
		}
		if (!triedPostings.insert(DupeStreamRepair::BuildDonorKey(
			donorNzb.get(), donor.Password)).second)
		{
			PrintMessage(Message::mkInfo,
				"Skipping another copy of duplicate posting %s", *donor.InfoName);
			continue;
		}
		if (DonorDead(donor, donorNzb.get()))
		{
			continue;
		}

		std::vector<FileInfo*> donorFiles;
		std::vector<const char*> donorFileNames;
		std::vector<int64_t> donorFileSizes;
		std::vector<size_t> donorArticleCounts;
		std::vector<int32_t> donorArticleSizesFlat;
		for (FileInfo* fileInfo : donorNzb->GetFileList())
		{
			donorFiles.push_back(fileInfo);
			donorFileNames.push_back(fileInfo->GetFilename());
			donorFileSizes.push_back(fileInfo->GetSize());
			size_t artCount = fileInfo->GetArticles() ? fileInfo->GetArticles()->size() : 0;
			donorArticleCounts.push_back(artCount);
			if (fileInfo->GetArticles())
			{
				for (ArticleInfo* article : fileInfo->GetArticles())
				{
					donorArticleSizesFlat.push_back(article->GetSize());
				}
			}
		}

		bool allowDecompress = !m_liveMode && g_Options->GetDupeStreamDecompress();

		struct DonorCtx
		{
			StreamRepairController* Controller;
			const std::vector<FileInfo*>* DonorFiles;
			std::vector<RepairTarget>* Targets;
			ArticleFetcher::FetchedArticle CurrentFetched;
			std::exception_ptr Error;
		} donorCtx{this, &donorFiles, &targets, {}, nullptr};

		nzbget_rs_cross_pack_session_process_donor(
			session,
			*donor.QueuedFilename,
			*donor.InfoName,
			donor.Password.Empty() ? "" : *donor.Password,
			donorFileNames.data(),
			donorFileSizes.data(),
			donorArticleCounts.data(),
			donorArticleSizesFlat.data(),
			donorFiles.size(),
			allowDecompress ? 1 : 0,
			&donorCtx,
			[](void* rawCtx) -> int
			{
				return RepairMapCall<DonorCtx>(rawCtx, 1, [](DonorCtx* c)
				{
					return c->Controller->IsStopped() ? 1 : 0;
				});
			},
			[](void* rawCtx, int32_t memberIndex, int32_t partIndex, int* outSuccess,
				int64_t* outOffset, int64_t* outFileSize, const uint8_t** outData, size_t* outDataLen) -> int
			{
				return RepairMapCall<DonorCtx>(rawCtx, 0, [=](DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return 0;
					}
					FileInfo* donorFile = (*c->DonorFiles)[memberIndex];
					if (!donorFile || !donorFile->GetArticles() || partIndex < 0 ||
						(size_t)partIndex >= donorFile->GetArticles()->size())
					{
						return 0;
					}
					ArticleInfo* article = (*donorFile->GetArticles())[partIndex].get();
					c->CurrentFetched = c->Controller->m_fetcher.Fetch(
						article->GetMessageId(), *donorFile->GetGroups());
					*outSuccess = c->CurrentFetched.Success ? 1 : 0;
					*outOffset = c->CurrentFetched.Offset;
					*outFileSize = c->CurrentFetched.FileSize;
					*outData = reinterpret_cast<const uint8_t*>(c->CurrentFetched.Data.data());
					*outDataLen = c->CurrentFetched.Data.size();
					return 1;
				});
			},
			[](void* rawCtx, int32_t memberIndex, const int32_t* partIndices, size_t count)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return;
					}
					FileInfo* donorFile = (*c->DonorFiles)[memberIndex];
					if (!donorFile || !donorFile->GetArticles())
					{
						return;
					}
					auto groups = std::make_shared<std::vector<CString>>();
					for (const CString& group : *donorFile->GetGroups())
					{
						groups->emplace_back(*group);
					}
					std::vector<ArticleBatchFetcher::Request> requests;
					requests.reserve(count);
					for (size_t i = 0; i < count; i++)
					{
						int partIndex = partIndices[i];
						if (partIndex >= 0 && (size_t)partIndex < donorFile->GetArticles()->size())
						{
							requests.push_back({
								(*donorFile->GetArticles())[partIndex]->GetMessageId(),
								groups
							});
						}
					}
					c->Controller->m_batchFetcher.Begin(std::move(requests));
				});
			},
			[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
				const uint8_t** outData, size_t* outDataLen) -> int
			{
				return RepairMapCall<DonorCtx>(rawCtx, 0, [=](DonorCtx* c)
				{
					c->CurrentFetched = ArticleFetcher::FetchedArticle();
					if (!c->Controller->m_batchFetcher.Next(c->CurrentFetched))
					{
						return 0;
					}
					*outSuccess = c->CurrentFetched.Success ? 1 : 0;
					*outOffset = c->CurrentFetched.Offset;
					*outFileSize = c->CurrentFetched.FileSize;
					*outData = reinterpret_cast<const uint8_t*>(c->CurrentFetched.Data.data());
					*outDataLen = c->CurrentFetched.Data.size();
					return 1;
				});
			},
			[](void* rawCtx)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [](DonorCtx* c)
				{
					c->Controller->m_batchFetcher.CancelRemaining();
				});
			},
			[](void* rawCtx, int32_t memberIndex)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [memberIndex](DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return;
					}
					FileInfo* donorFile = (*c->DonorFiles)[memberIndex];
					if (!donorFile || !donorFile->GetArticles())
					{
						return;
					}
					auto donorGroups = std::make_shared<std::vector<CString>>();
					for (const CString& group : *donorFile->GetGroups())
					{
						donorGroups->emplace_back(*group);
					}
					std::vector<ArticleBatchFetcher::Request> requests;
					requests.reserve(donorFile->GetArticles()->size());
					for (std::unique_ptr<ArticleInfo>& article : *donorFile->GetArticles())
					{
						requests.push_back({article->GetMessageId(), donorGroups});
					}
					c->Controller->m_batchFetcher.Begin(std::move(requests));
				});
			},
			[](void* rawCtx, const char* label, int32_t stageProgress)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					if (c->Controller->m_postInfo)
					{
						GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
						c->Controller->m_postInfo->SetProgressLabel(label ? label : "");
						c->Controller->m_postInfo->SetStageProgress(stageProgress);
					}
				});
			},
			[](void* rawCtx, int32_t delta)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [delta](DonorCtx* c)
				{
					c->Controller->m_materializing += delta;
				});
			},
			[](void* rawCtx, int64_t bytes, int32_t holes)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [bytes, holes](DonorCtx* c)
				{
					c->Controller->m_recoveredBytes += bytes;
					c->Controller->m_progressBytes += bytes;
					c->Controller->m_recoveredHoles += holes;
				});
			},
			[](void* rawCtx, const size_t* updatedCounts, const NzbgetRsByteRange* updatedFlat, size_t)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					size_t cursor = 0;
					for (size_t t = 0; t < c->Targets->size(); t++)
					{
						size_t cnt = updatedCounts ? updatedCounts[t] : 0;
						(*c->Targets)[t].Holes.assign(
							reinterpret_cast<const StreamRange*>(updatedFlat + cursor),
							reinterpret_cast<const StreamRange*>(updatedFlat + cursor + cnt));
						cursor += cnt;
					}
				});
			},
			[](void* rawCtx, int32_t kind, const char* text)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					Message::EKind msgKind = kind == 2 ? Message::mkError :
						kind == 1 ? Message::mkWarning : Message::mkInfo;
					c->Controller->PrintMessage(msgKind, "%s", text);
				});
			},
			[](void* rawCtx, int32_t kind, const char* msg)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx*)
				{
					if (!msg) return;
					switch (kind)
					{
						case 0:
							info("%s", msg);
							break;
						case 1:
							error("%s", msg);
							break;
						case 2:
							warn("%s", msg);
							break;
						default:
							detail("%s", msg);
							break;
					}
				});
			},
#ifdef DEBUG
			[](void* rawCtx, const char* text)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx*)
				{
					debug("%s", text ? text : "");
				});
			}
#else
			nullptr
#endif
		);
		if (donorCtx.Error)
		{
			std::rethrow_exception(donorCtx.Error);
		}
	}
}

inline void StreamRepairController::RepairCompleted()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();

	for (const RepairTarget& target : m_targets)
	{
		if (target.DecodedFileSize > 0 && target.Holes.empty())
		{
			MarkRepaired(nzbInfo, target);
		}
		else if (target.Recreated)
		{
			MarkRecreated(nzbInfo, target);
		}
	}

	if (m_recoveredArticles > 0 || m_recoveredBytes > 0 || m_recoveredHoles > 0)
	{
		nzbInfo->SetDupeRecoveredArticles(nzbInfo->GetDupeRecoveredArticles() + m_recoveredArticles);
		nzbInfo->SetDupeRecoveredBytes(nzbInfo->GetDupeRecoveredBytes() + m_recoveredBytes);
		nzbInfo->SetDupeRecoveredHoles(nzbInfo->GetDupeRecoveredHoles() + m_recoveredHoles);
	}

	if (m_recoveredFailedSize > 0)
	{
		int64 failedSize = nzbInfo->GetFailedSize();
		int64 successSize = nzbInfo->GetSuccessSize();
		int64 curFailedSize = nzbInfo->GetCurrentFailedSize();
		int64 curSuccessSize = nzbInfo->GetCurrentSuccessSize();
		int64 parFailedSize = nzbInfo->GetParFailedSize();
		int64 parSuccessSize = nzbInfo->GetParSuccessSize();
		int64 parCurFailedSize = nzbInfo->GetParCurrentFailedSize();
		int64 parCurSuccessSize = nzbInfo->GetParCurrentSuccessSize();
		nzbget_rs_stream_credit_repaired_sizes(
			m_recoveredFailedSize, m_recoveredFailedParSize,
			&failedSize, &successSize, &curFailedSize, &curSuccessSize,
			&parFailedSize, &parSuccessSize, &parCurFailedSize, &parCurSuccessSize);
		nzbInfo->SetFailedSize(failedSize);
		nzbInfo->SetSuccessSize(successSize);
		nzbInfo->SetCurrentFailedSize(curFailedSize);
		nzbInfo->SetCurrentSuccessSize(curSuccessSize);
		nzbInfo->SetParFailedSize(parFailedSize);
		nzbInfo->SetParSuccessSize(parSuccessSize);
		nzbInfo->SetParCurrentFailedSize(parCurFailedSize);
		nzbInfo->SetParCurrentSuccessSize(parCurSuccessSize);
	}

	for (auto it = nzbInfo->GetStreamRepairJobs()->begin();
		it != nzbInfo->GetStreamRepairJobs()->end(); )
	{
		if (!Interrupted())
		{
			it = nzbInfo->GetStreamRepairJobs()->erase(it);
			continue;
		}
		bool matched = false;
		for (const RepairTarget& target : m_targets)
		{
			if (target.FileId == it->GetFileId())
			{
				matched = true;
				if (target.Holes.empty() && target.DecodedFileSize > 0)
				{
					it = nzbInfo->GetStreamRepairJobs()->erase(it);
				}
				else
				{
					if (it->GetDecodedFileSize() <= 0 && target.DecodedFileSize > 0)
					{
						it->SetDecodedFileSize(target.DecodedFileSize);
					}
					it->SetHoles(target.Holes);
					++it;
				}
				break;
			}
		}
		if (!matched)
		{
			++it;
		}
	}

	bool recovered = m_recoveredBytes > 0;
	m_postInfo->SetStreamRepairDone(true);
	m_postInfo->SetStreamRepairRecovered(recovered);

	{
		int resetParRename = 0, setParStatus = 0, newParStatus = 0;
		int forceParFull = 0, setReqParCheckFalse = 0, checkMainPars = 0;
		nzbget_rs_stream_resolve_completion_par_action(
			m_recoveredBytes, m_holesRemain ? 1 : 0,
			(int)nzbInfo->GetParRenameStatus(), (int)NzbInfo::rsNone,
			(int)nzbInfo->GetParStatus(), (int)NzbInfo::psSkipped,
			(int)g_Options->GetParCheck(), (int)Options::pcManual,
			(int)NzbInfo::psManual, (int)NzbInfo::psNone,
			&resetParRename, &setParStatus, &newParStatus,
			&forceParFull, &setReqParCheckFalse, &checkMainPars);
		if (resetParRename)
		{
			nzbInfo->SetParRenameStatus(NzbInfo::rsNone);
		}
		if (setParStatus)
		{
			nzbInfo->SetParStatus((NzbInfo::EParStatus)newParStatus);
		}
		if (checkMainPars && ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
		{
			m_postInfo->SetRequestParCheck(true);
		}
		if (forceParFull)
		{
			m_postInfo->SetForceParFull(true);
		}
		if (setReqParCheckFalse)
		{
			m_postInfo->SetRequestParCheck(false);
		}
	}

	downloadQueue->Save();

	m_postInfo->SetWorking(false);
}

inline void StreamRepairController::AddMessage(Message::EKind kind, const char* text)
{
	if (m_liveMode)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo)
		{
			nzbInfo->AddMessage(kind, text);
		}
		return;
	}

	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}

class MoveController : public Thread, public ScriptController
{
public:
	void Run() override
	{
		BString<1024> nzbName;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			m_interDir = fs::u8path(m_postInfo->GetNzbInfo()->GetDestDir());
			m_destDir = fs::u8path(m_postInfo->GetNzbInfo()->GetFinalDir());
			if (m_destDir.empty())
			{
				m_destDir = fs::u8path(m_postInfo->GetNzbInfo()->BuildFinalDirName().Str());
			}
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("move", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(*infoName);
		PrintMessage(Message::mkInfo, "Moving completed files for %s", *nzbName);

		bool ok = MoveFiles();

		nzbget_rs_dqh_post_capitalize_first_ascii(*infoName);

		if (ok)
		{
			RemoveStaleHardlinks(*m_postInfo->GetNzbInfo(), m_destDir);
		}

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			if (m_postInfo && m_postInfo->GetNzbInfo())
			{
				int moveStatus = 0;
				int outcome = nzbget_rs_move_controller_decide_outcome(
					ok ? 1 : 0, IsStopped() ? 1 : 0, &moveStatus);
				if (outcome == 1)
				{
					PrintMessage(Message::mkInfo, "%s successful", *infoName);
					m_postInfo->GetNzbInfo()->SetDestDir(fs::u8string(m_destDir).c_str());
					m_postInfo->GetNzbInfo()->SetFinalDir("");
					m_postInfo->GetNzbInfo()->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
				}
				else if (outcome == 2)
				{
					PrintMessage(Message::mkInfo, "%s interrupted, resumes after the restart", *infoName);
				}
				else
				{
					PrintMessage(Message::mkError, "%s failed", *infoName);
					m_postInfo->GetNzbInfo()->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
				}
				m_postInfo->SetWorking(false);
			}
		}
	}

	static void StartJob(PostInfo* postInfo)
	{
		MoveController* moveController = new MoveController();
		moveController->m_postInfo = postInfo;
		moveController->SetAutoDestroy(false);

		postInfo->SetPostThread(moveController);

		moveController->Start();
	}

	static Message::EKind ToMessageKind(int kind)
	{
		switch (kind)
		{
		case 1:
			return Message::mkWarning;
		case 2:
			return Message::mkError;
		default:
			return Message::mkInfo;
		}
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

	bool MoveFiles()
	{
		const std::string interStr = fs::u8string(m_interDir);
		const std::string destStr = fs::u8string(m_destDir);
		return nzbget_rs_move_controller_move_files(
			interStr.c_str(),
			destStr.c_str(),
			this,
			[](void* rawCtx) -> int {
				return static_cast<MoveController*>(rawCtx)->IsStopped() ? 1 : 0;
			},
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<MoveController*>(rawCtx)->PrintMessage(
					ToMessageKind(kind), "%s", msg ? msg : "");
			},
			[](void* rawCtx, const char* oldRelName, const char* newRelName) {
				auto* self = static_cast<MoveController*>(rawCtx);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
				{
					self->m_postInfo->GetNzbInfo()->RenameCompletedFile(
						oldRelName ? oldRelName : "", newRelName ? newRelName : "");
				}
			}) != 0;
	}

	void RemoveStaleHardlinks(NzbInfo& nzbInfo, const fs::path& destDir)
	{
		const auto& hardLinkPath = nzbInfo.GetHardLinkPath();
		if (hardLinkPath.empty()) return;

		const std::string destStr = fs::u8string(destDir);
		nzbget_rs_move_controller_remove_stale_hardlinks(
			hardLinkPath.c_str(),
			destStr.c_str(),
			this,
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<MoveController*>(rawCtx)->PrintMessage(
					ToMessageKind(kind), "%s", msg ? msg : "");
			});
	}

private:
	PostInfo* m_postInfo = nullptr;
	fs::path m_interDir;
	fs::path m_destDir;
};

class CleanupController : public Thread, public ScriptController
{
public:
	void Run() override
	{
		BString<1024> nzbName;
		CString destDir;
		CString finalDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			destDir = m_postInfo->GetNzbInfo()->GetDestDir();
			finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("cleanup", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(*infoName);

		PrintMessage(Message::mkInfo, "Cleaning up %s", *nzbName);

		int cleanupStatus = 0;
		int cleanupOutcome = nzbget_rs_cleanup_controller_run(
			destDir ? *destDir : "",
			finalDir ? *finalDir : "",
			g_Options->GetExtCleanupDisk(),
			this,
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<CleanupController*>(rawCtx)->PrintMessage(
					MoveController::ToMessageKind(kind), "%s", msg ? msg : "");
			},
			&cleanupStatus);

		nzbget_rs_dqh_post_capitalize_first_ascii(*infoName);

		if (cleanupOutcome == 1)
		{
			PrintMessage(Message::mkInfo, "%s successful", *infoName);
		}
		else if (cleanupOutcome == 2)
		{
			PrintMessage(Message::mkInfo, "Nothing to cleanup for %s", *nzbName);
		}
		else
		{
			PrintMessage(Message::mkError, "%s failed", *infoName);
		}
		m_postInfo->GetNzbInfo()->SetCleanupStatus((NzbInfo::ECleanupStatus)cleanupStatus);

		m_postInfo->SetWorking(false);
	}

	static void StartJob(PostInfo* postInfo)
	{
		CleanupController* cleanupController = new CleanupController();
		cleanupController->m_postInfo = postInfo;
		cleanupController->SetAutoDestroy(false);

		postInfo->SetPostThread(cleanupController);

		cleanupController->Start();
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo;
	CString m_destDir;
	CString m_finalDir;
};

class RenameController : public Thread, public ScriptController
{
public:
	enum EJobKind
	{
		jkPar,
		jkRar
	};

	RenameController()
		: m_postInfo(nullptr)
		, m_kind(jkPar)
	{
		debug("Creating RenameController");
	}

	virtual void Run()
	{
		BString<1024> nzbName;
		CString destDir;
		CString finalDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			destDir = m_postInfo->GetNzbInfo()->GetDestDir();
			finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("rename", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(infoName);

		const char* archiveKindPrefix = nzbget_rs_rename_controller_archive_kind_prefix(
			(int)m_kind, (int)jkRar);
		PrintMessage(Message::mkInfo, "Checking renamed %sfiles for %s",
			archiveKindPrefix, *nzbName);

		ExecRename(destDir, finalDir, nzbName);

		int runOutcome = nzbget_rs_rename_controller_classify_run_outcome(
			IsStopped() ? 1 : 0, m_renamedCount);
		if (runOutcome == 1)
		{
			PrintMessage(Message::mkWarning, "Renaming cancelled for %s", *nzbName);
		}
		else if (runOutcome == 2)
		{
			PrintMessage(Message::mkInfo, "Successfully renamed %i %sfile(s) for %s",
				m_renamedCount, archiveKindPrefix, *nzbName);
		}
		else
		{
			PrintMessage(Message::mkInfo, "No renamed %sfiles found for %s",
				archiveKindPrefix, *nzbName);
		}

		RenameCompleted(nzbName);
	}

	static void StartJob(PostInfo* postInfo, EJobKind kind)
	{
		RenameController* renameController = new RenameController();
		renameController->m_postInfo = postInfo;
		renameController->m_kind = kind;
		renameController->SetAutoDestroy(false);

		postInfo->SetPostThread(renameController);

		renameController->Start();
	}

protected:
	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo;
	int m_renamedCount = 0;
	bool m_hasMissedFiles = false;
	bool m_hasDamagedParFiles = false;
	EJobKind m_kind;

	void UpdateProgress(const char* label, int stage)
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		m_postInfo->SetProgressLabel(label ? label : "");
		m_postInfo->SetStageProgress(stage);
	}

	void ExecRename(const char* destDir, const char* finalDir, const char* nzbName)
	{
		if (m_kind == jkPar)
		{
#ifndef DISABLE_PARCHECK
			int detectMissingInt = 0;
			bool useFinal = nzbget_rs_rename_controller_eval_par_config(
				(int)m_postInfo->GetNzbInfo()->GetUnpackStatus(),
				(int)NzbInfo::usSuccess,
				(int)NzbInfo::usNone,
				Util::EmptyStr(finalDir) ? 1 : 0,
				&detectMissingInt) != 0;
			int renamedCount = 0;
			int hasMissedFiles = 0;
			int hasDamagedParFiles = 0;
			nzbget_rs_par_renamer_execute(
				nzbName ? nzbName : "",
				(useFinal ? finalDir : destDir) ? (useFinal ? finalDir : destDir) : "",
				g_Options ? g_Options->GetParIgnoreExt() : "",
				detectMissingInt,
				PATH_SEPARATOR,
				this,
				+[](void* rawCtx) noexcept -> int
				{
					return static_cast<RenameController*>(rawCtx)->IsStopped() ? 1 : 0;
				},
				+[](void* rawCtx, const char* label, int stage) noexcept
				{
					static_cast<RenameController*>(rawCtx)->UpdateProgress(label, stage);
				},
				+[](void* rawCtx, int kind, const char* text) noexcept
				{
					auto* self = static_cast<RenameController*>(rawCtx);
					Message::EKind msgKind = kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError;
					self->PrintMessage(msgKind, "%s", text ? text : "");
				},
				+[](void* rawCtx, const char* name) noexcept
				{
					static_cast<RenameController*>(rawCtx)->m_postInfo->GetParredFiles()->push_back(name ? name : "");
				},
				+[](void* rawCtx, const char* oldRel, const char* newRel) noexcept
				{
					static_cast<RenameController*>(rawCtx)->RegisterRenamedFile(
						oldRel ? oldRel : "", newRel ? newRel : "");
				},
				&renamedCount,
				&hasMissedFiles,
				&hasDamagedParFiles);
			m_hasMissedFiles = hasMissedFiles != 0;
			m_hasDamagedParFiles = hasDamagedParFiles != 0;
#endif
		}
		else if (m_kind == jkRar)
		{
			const char* password = "";
			NzbParameter* parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
			if (parameter)
			{
				password = parameter->GetValue();
			}

			struct Context
			{
				RenameController* self;
				std::exception_ptr error;

				int Fail()
				{
					error = std::current_exception();
					return -1;
				}
			} context{this, nullptr};

			NzbgetRsRarRenamerRunEnv env{};
			env.ctx = &context;
			env.stopped = +[](void* ctx) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { return c.self->IsStopped() ? 1 : 0; } catch (...) { return c.Fail(); }
			};
			env.progress = +[](void* ctx, const char* label, int stage) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { c.self->UpdateProgress(label, stage); return 0; } catch (...) { return c.Fail(); }
			};
			env.message = +[](void* ctx, int kind, const char* text) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try
				{
					c.self->PrintMessage(
						kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError,
						"%s", text ? text : "");
					return 0;
				}
				catch (...) { return c.Fail(); }
			};
			env.registerRenamed = +[](void* ctx, const char* oldBase, const char* newBase) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { c.self->RegisterRenamedFile(oldBase, newBase); return 0; } catch (...) { return c.Fail(); }
			};

#ifndef DISABLE_TLS
			const int tlsEnabled = 1;
#else
			const int tlsEnabled = 0;
#endif
			int renamedCount = 0;
			nzbget_rs_rar_renamer_execute(
				&env, destDir ? destDir : "", nzbName ? nzbName : "", password,
				g_Options ? g_Options->GetUnpackIgnoreExt() : "", tlsEnabled, &renamedCount);
			if (context.error)
			{
				std::rethrow_exception(context.error);
			}
		}
	}

	void RenameCompleted(const char* nzbName)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int reqExtraPars = 0;
		int reqParCheck = 0;
#ifndef DISABLE_PARCHECK
		int hasDamagedParFiles = m_hasDamagedParFiles ? 1 : 0;
		int hasMissedFiles = m_hasMissedFiles ? 1 : 0;
#else
		int hasDamagedParFiles = 0;
		int hasMissedFiles = 0;
#endif
		int initialStatus = nzbget_rs_rename_controller_complete_action(
			(int)m_kind, m_renamedCount,
			hasDamagedParFiles,
			m_postInfo->GetNzbInfo()->GetRemainingParCount(),
			hasMissedFiles,
			(int)m_postInfo->GetNzbInfo()->GetParStatus(),
			&reqExtraPars, &reqParCheck);
		if (m_kind == jkPar)
		{
			m_postInfo->GetNzbInfo()->SetParRenameStatus((NzbInfo::EPostRenameStatus)initialStatus);
#ifndef DISABLE_PARCHECK
			if (reqExtraPars)
			{
				PrintMessage(Message::mkInfo, "Requesting extra par2-files for %s to perform par-rename", nzbName ? nzbName : "");
				downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupResume, nullptr);
				downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
				if (nzbget_rs_rename_controller_should_reset_par_status(
					m_postInfo->GetNzbInfo()->GetRemainingSize()) != 0)
				{
					m_postInfo->GetNzbInfo()->SetParRenameStatus(NzbInfo::rsNone);
				}
			}
#endif
		}
		else if (m_kind == jkRar)
		{
			m_postInfo->GetNzbInfo()->SetRarRenameStatus((NzbInfo::EPostRenameStatus)initialStatus);
		}
#ifndef DISABLE_PARCHECK
		if (reqParCheck)
		{
			PrintMessage(Message::mkInfo, "Requesting par-check/repair for %s to restore missing files ", nzbName ? nzbName : "");
			m_postInfo->SetRequestParCheck(true);
		}
#endif

		m_postInfo->SetWorking(false);
	}

	void RegisterRenamedFile(const char* oldFilename, const char* newFilename)
	{
		bool found;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			found = m_postInfo->GetNzbInfo()->RenameCompletedFile(oldFilename, newFilename);
		}
		if (!found)
		{
			PrintMessage(Message::mkWarning,
				"Could not find completed-file entry for %s while renaming to %s",
				oldFilename, newFilename);
		}
		++m_renamedCount;
	}
};

class PostRenamerController final : public Thread, public ScriptController
{
public:
	static void StartJob(PostInfo* postInfo, bool postUnpack)
	{
		PostRenamerController* controller = new (std::nothrow) PostRenamerController();
		if (!controller)
		{
			error("Failed to allocate memory for PostRenamerController");
			return;
		}
		controller->m_postInfo = postInfo;
		controller->m_postUnpack = postUnpack;
		controller->SetAutoDestroy(false);
		postInfo->SetPostThread(controller);
		controller->Start();
	}

	void Run() override
	{
		std::string metaName;
		std::string nzbName;
		std::string dstDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
			metaName = nzbInfo->GetMetaName();
			nzbName = nzbInfo->GetName() ? nzbInfo->GetName() : "";
			dstDir = nzbInfo->GetDestDir() ? nzbInfo->GetDestDir() : "";
		}

		int status = nzbget_rs_post_renamer_run(
			m_postUnpack ? 1 : 0,
			metaName.c_str(),
			nzbName.c_str(),
			dstDir.c_str(),
			g_Options->GetRenameIgnoreExt(),
			this,
			+[](void* rawCtx, const char* infoName) {
				static_cast<PostRenamerController*>(rawCtx)->SetInfoName(infoName ? infoName : "");
			},
			+[](void* rawCtx) -> int {
				return static_cast<PostRenamerController*>(rawCtx)->IsStopped() ? 1 : 0;
			},
			+[](void* rawCtx, int kind, const char* msg) {
				auto* self = static_cast<PostRenamerController*>(rawCtx);
				Message::EKind msgKind = kind == 1
					? Message::mkWarning
					: kind == 2
						? Message::mkError
						: Message::mkInfo;
				self->PrintMessage(msgKind, "%s", msg ? msg : "");
			},
			+[](void* rawCtx, const char* oldName, const char* newName) {
				auto* self = static_cast<PostRenamerController*>(rawCtx);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
				{
					self->m_postInfo->GetNzbInfo()->RenameCompletedFile(
						oldName ? oldName : "", newName ? newName : "");
				}
			});

		GuardedDownloadQueue guard = DownloadQueue::Guard();
		if (m_postUnpack)
		{
			m_postInfo->GetNzbInfo()->SetPostUnpackRenamingStatus(
				static_cast<NzbInfo::PostUnpackRenamingStatus>(status));
		}
		else
		{
			m_postInfo->GetNzbInfo()->SetPostDownloadRenamingStatus(
				static_cast<NzbInfo::PostDownloadRenamingStatus>(status));
		}
		m_postInfo->SetWorking(false);
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo = nullptr;
	bool m_postUnpack = false;
};

class PostScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run()
	{
		StringBuilder scriptCommaList;

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			size_t pcount = m_postInfo->GetNzbInfo()->GetParameters()->size();
			std::vector<const char*> pnames;
			std::vector<const char*> pvals;
			pnames.reserve(pcount);
			pvals.reserve(pcount);
			for (NzbParameter& parameter : m_postInfo->GetNzbInfo()->GetParameters())
			{
				pnames.push_back(parameter.GetName());
				pvals.push_back(parameter.GetValue());
			}
			NzbgetRsBuf listBuf = nzbget_rs_sus_build_enabled_script_comma_list(pnames.data(), pvals.data(), pcount);
			if (listBuf.data)
			{
				scriptCommaList.Append(listBuf.data);
			}
			nzbget_rs_free(listBuf);
			m_postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
		}

		ExecuteScriptList(scriptCommaList);

		m_postInfo->SetStage(PostInfo::ptFinished);
		m_postInfo->SetWorking(false);
	}

	virtual void Stop()
	{
		debug("Stopping post-process-script");
		Thread::Stop();
		Terminate();
	}

	static void StartJob(PostInfo* postInfo)
	{
		if (!postInfo)
		{
			return;
		}

		if (postInfo->GetNzbInfo() && postInfo->GetNzbInfo()->GetSkipScriptProcessing())
		{
			return;
		}

		PostScriptController* scriptController = new PostScriptController();
		scriptController->m_postInfo = postInfo;
		scriptController->SetAutoDestroy(false);
		scriptController->m_prefixLen = 0;

		postInfo->SetPostThread(scriptController);

		scriptController->Start();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		// if any script has requested par-check, do not execute other scripts
		if (!script->GetPostScript() || m_postInfo->GetRequestParCheck())
		{
			return;
		}

		m_script = std::move(script);

		PrintMessage(Message::mkInfo, "Executing post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());

		BString<1024> progressLabel("Executing post-process-script %s", m_script->GetName());
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(progressLabel);
		}

		SetArgs({m_script->GetEntry()});

		BString<1024> infoName("post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());
		SetInfoName(infoName);

		SetLogPrefix(m_script->GetDisplayName());
		m_prefixLen = static_cast<int>(strlen(m_script->GetDisplayName())) + 2; // 2 = strlen(": ");
		PrepareParams(m_script->GetName());

		int exitCode = Execute();

		infoName[0] = 'P';

		SetLogPrefix(nullptr);
		ScriptStatus::EStatus status = AnalyseExitCode(exitCode, infoName);

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->GetScriptStatuses()->emplace_back(m_script->GetName(), status);
		}
	}

	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		const char* msgText = text + m_prefixLen;

		NzbgetRsPostScriptCmd cmd{};
		int cmdKind = nzbget_rs_sus_parse_post_script_cmd(msgText, &cmd);
		if (cmdKind != 0)
		{
			debug("Command %s detected", msgText + 6);
			if (cmdKind == 1)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->SetFinalDir(msgText + cmd.val_offset);
			}
			else if (cmdKind == 2)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->SetDestDir(msgText + cmd.val_offset);
			}
			else if (cmdKind == 3)
			{
				std::string param(msgText + cmd.key_offset, cmd.key_len);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->GetParameters()->SetParameter(param.c_str(), msgText + cmd.val_offset);
			}
			else if (cmdKind == 4)
			{
				SetLogPrefix(nullptr);
				PrintMessage(Message::mkWarning, "Marking %s as bad", m_postInfo->GetNzbInfo()->GetName());
				SetLogPrefix(m_script->GetDisplayName());
				m_postInfo->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
			}
			else
			{
				m_postInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Invalid command \"%s\" received from %s", msgText, GetInfoName());
			}
		}
		else
		{
			m_postInfo->GetNzbInfo()->AddMessage(kind, text);
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(text);
		}

		if (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority())
		{
			time_t stageTime = m_postInfo->GetStageTime();
			time_t startTime = m_postInfo->GetStartTime();
			time_t waitTime = Util::CurrentTime();

			while (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority() && !IsStopped())
			{
				Util::Sleep(100);

				time_t delta = Util::CurrentTime() - waitTime;
				if (stageTime > 0)
				{
					m_postInfo->SetStageTime(stageTime + delta);
				}
				if (startTime > 0)
				{
					m_postInfo->SetStartTime(startTime + delta);
				}
			}
		}
	}

private:
	PostInfo* m_postInfo;
	int m_prefixLen;
	std::shared_ptr<const Extension::Script> m_script;

	void PrepareParams(const char* scriptName)
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		ResetEnv();

		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		BString<1024> status = nzbInfo->MakeTextStatus(true);
		std::vector<int> serverIds;
		std::vector<int> serverSuccess;
		std::vector<int> serverFailed;
		serverIds.reserve(nzbInfo->GetServerStats()->size());
		serverSuccess.reserve(nzbInfo->GetServerStats()->size());
		serverFailed.reserve(nzbInfo->GetServerStats()->size());
		for (ServerStat& serverStat : nzbInfo->GetServerStats())
		{
			serverIds.push_back(serverStat.GetServerId());
			serverSuccess.push_back(serverStat.GetSuccessArticles());
			serverFailed.push_back(serverStat.GetFailedArticles());
		}

		NzbgetRsPostScriptEnv env{};
		env.nzb_id = nzbInfo->GetId();
		env.nzb_name = nzbInfo->GetName();
		env.dest_dir = nzbInfo->GetDestDir();
		env.filename = nzbInfo->GetFilename();
		env.queued_filename = nzbInfo->GetQueuedFilename();
		env.url = nzbInfo->GetUrl();
		env.final_dir = nzbInfo->GetFinalDir();
		env.category = nzbInfo->GetCategory();
		env.health = nzbInfo->CalcHealth();
		env.critical_health = nzbInfo->CalcCriticalHealth(false);
		env.dupe_key = nzbInfo->GetDupeKey();
		env.dupe_score = nzbInfo->GetDupeScore();
		env.dupe_mode = static_cast<int>(nzbInfo->GetDupeMode());
		env.status = status;
		env.script_status = static_cast<int>(nzbInfo->GetScriptStatuses()->CalcTotalStatus());
		env.par_status = static_cast<int>(nzbInfo->GetParStatus());
		env.unpack_status = static_cast<int>(nzbInfo->GetUnpackStatus());
		env.delete_status = static_cast<int>(nzbInfo->GetDeleteStatus());
		env.mark_status = static_cast<int>(nzbInfo->GetMarkStatus());
		env.total_articles = nzbInfo->GetTotalArticles();
		env.success_articles = nzbInfo->GetSuccessArticles();
		env.failed_articles = nzbInfo->GetFailedArticles();
		env.server_ids = serverIds.data();
		env.server_success_articles = serverSuccess.data();
		env.server_failed_articles = serverFailed.data();
		env.server_count = serverIds.size();

		nzbget_rs_post_script_prepare_env(
			&env,
			this,
			[](void* ctx, const char* name, const char* value)
			{
				static_cast<PostScriptController*>(ctx)->SetEnvVar(name, value);
			},
			[](void* ctx, const char* name, int value)
			{
				static_cast<PostScriptController*>(ctx)->SetIntEnvVar(name, value);
			});

		PrepareEnvScript(nzbInfo->GetParameters(), scriptName);
	}

	ScriptStatus::EStatus AnalyseExitCode(int exitCode, const char* upInfoName)
	{
		// The ScriptStatus is accumulated for all scripts:
		// If any script has failed the status is "failure", etc.

#ifndef DISABLE_PARCHECK
		constexpr int parcheckEnabled = 1;
#else
		constexpr int parcheckEnabled = 0;
#endif
		int msgAction = 0;
		int reqParCheck = 0;
		int status = nzbget_rs_sus_analyse_post_script_exit_code(
			exitCode,
			static_cast<int>(m_postInfo->GetNzbInfo()->GetParStatus()),
			static_cast<int>(NzbInfo::psSkipped),
			parcheckEnabled,
			static_cast<int>(ScriptStatus::srNone),
			static_cast<int>(ScriptStatus::srFailure),
			static_cast<int>(ScriptStatus::srSuccess),
			&msgAction,
			&reqParCheck);
		switch (msgAction)
		{
			case 1:
				PrintMessage(Message::mkInfo, "%s successful", upInfoName);
				break;
			case 2:
				PrintMessage(Message::mkError, "%s failed", upInfoName);
				break;
			case 3:
				PrintMessage(Message::mkInfo, "%s skipped", upInfoName);
				break;
			case 4:
				PrintMessage(Message::mkError, "%s requested par-check/repair, but the collection was already checked", upInfoName);
				break;
			case 5:
				PrintMessage(Message::mkInfo, "%s requested par-check/repair", upInfoName);
				break;
			default:
				PrintMessage(Message::mkError, "%s failed (terminated with unknown status)", upInfoName);
				break;
		}
		if (reqParCheck)
		{
			m_postInfo->SetRequestParCheck(true);
			m_postInfo->SetForceRepair(true);
		}
		return static_cast<ScriptStatus::EStatus>(status);
	}
};

inline PrePostProcessor::PrePostProcessor()
{
	debug("Creating PrePostProcessor");

	DownloadQueue::Guard()->Attach(this);
}

inline void PrePostProcessor::Run()
{
	debug("Entering PrePostProcessor-loop");

	while (!DownloadQueue::IsLoaded())
	{
		Util::Sleep(20);
	}

	if (g_Options->GetServerMode())
	{
		SanitisePostQueue();
	}

	while (!IsStopped())
	{
		if (g_WorkState->GetTempPausePostprocess())
		{
			// Postprocess is paused: just wait and loop
			Util::Sleep(200);
			continue;
		}

		if (m_queuedJobs)
		{
			// check post-queue every 200 msec
			CheckPostQueue();
			Util::Sleep(200);
		}
		else
		{
			// Wait until we get the stop signal or more jobs in the queue
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait(lk, [&]{ return m_queuedJobs || IsStopped(); });
		}
	}

	WaitJobs();

	debug("Exiting PrePostProcessor-loop");
}

inline void PrePostProcessor::WaitJobs()
{
	debug("PrePostProcessor: waiting for jobs to complete");

	// wait 5 seconds until all post-processing jobs gracefully finish
	time_t waitStart = Util::CurrentTime();
	while (Util::CurrentTime() - waitStart < 5)
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			if (m_activeJobs.empty())
			{
				break;
			}
		}
		CheckPostQueue();
		Util::Sleep(200);
	}

	// kill remaining post-processing jobs; not safe but we can't wait any longer
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetPostThread())
			{
				Thread* thread = postJob->GetPostInfo()->GetPostThread();
				postJob->GetPostInfo()->SetPostThread(nullptr);
				warn("Terminating active post-process job for %s", postJob->GetName());
				thread->Kill();
				delete thread;
			}
		}
	}

	// wait 5 seconds until direct unpack and live repair threads gracefully finish
	waitStart = Util::CurrentTime();
	while (Util::CurrentTime() - waitStart < 5)
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			if (std::find_if(downloadQueue->GetQueue()->begin(),
				downloadQueue->GetQueue()->end(),
				[](const std::unique_ptr<NzbInfo>& nzbInfo)
				{
					return nzbInfo->GetUnpackThread() != nullptr ||
						nzbInfo->GetLiveRepairThread() != nullptr;
				}) == downloadQueue->GetQueue()->end())
			{
				break;
			}
		}
		Util::Sleep(200);
	}

	// disconnect remaining direct unpack and live repair jobs
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbInfo->SetUnpackThread(nullptr);
			nzbInfo->SetLiveRepairThread(nullptr);
		}
	}

	debug("PrePostProcessor: Jobs are completed");
}

inline void PrePostProcessor::Stop()
{
	Thread::Stop();

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);

		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetPostThread())
			{
				postJob->GetPostInfo()->GetPostThread()->Stop();
			}
		}

		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetUnpackThread())
			{
				((DirectUnpack*)nzbInfo->GetUnpackThread())->Stop(downloadQueue, nzbInfo);
			}
			if (nzbInfo->GetLiveRepairThread())
			{
				nzbInfo->GetLiveRepairThread()->Stop();
			}
		}
	}

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

inline bool PrePostProcessor::HasActiveUnpack(bool includeWaitingVolume)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	return HasActiveUnpack(downloadQueue, includeWaitingVolume);
}

inline bool PrePostProcessor::HasActiveUnpack(DownloadQueue* downloadQueue, bool includeWaitingVolume)
{
	// Check post-processing active jobs for UnpackController
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetStage() == PostInfo::ptUnpacking)
			{
				return true;
			}
		}
	}

	// Check direct unpack jobs: does a direct unpack own the unpack slot / actively extract?
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetUnpackThread())
		{
			DirectUnpack* directUnpack = (DirectUnpack*)nzbInfo->GetUnpackThread();
			if (includeWaitingVolume ||
				(directUnpack->IsUnpacking() && !directUnpack->IsWaitingVolume()))
			{
				return true;
			}
		}
	}

	return false;
}

/**
 * Reset the state of items after reloading from disk and
 * delete items which could not be resumed.
 * Also count the number of post-jobs.
 */
inline void PrePostProcessor::SanitisePostQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsRunning && !nzbInfo->GetUnpackThread())
		{
			nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
		}
		PostInfo* postInfo = nzbInfo->GetPostInfo();
		if (!postInfo && nzbInfo->GetKind() == NzbInfo::nkNzb &&
			!nzbInfo->GetCompletedFiles()->empty() &&
			nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
			nzbInfo->IsDownloadCompleted(true))
		{
			// its last file finished while the program stopped (a reload, a
			// shutdown): the event that queues it for post-processing was
			// dropped then, and only paused par2-files are left, so nothing
			// else would ever start it
			nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s completely downloaded", nzbInfo->GetName());
			g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
			NzbDownloaded(downloadQueue, nzbInfo);
			postInfo = nzbInfo->GetPostInfo();
			if (postInfo)
			{
				m_queuedJobs--;	// counted again below
			}
		}
		if (postInfo)
		{
			m_queuedJobs++;
			int nextStage = 0;
			int reqParCheck = 0;
			nzbget_rs_prepost_sanitise_post_stage(
				(int)postInfo->GetStage(),
				(int)PostInfo::ptExecutingScript,
				(int)PostInfo::ptFinished,
				(int)PostInfo::ptQueued,
				FileSystem::DirectoryExists(nzbInfo->GetDestDir()) ? 1 : 0,
				nzbInfo->GetUnpackTried() ? 1 : 0,
				(int)nzbInfo->GetParStatus(),
				(int)NzbInfo::psSkipped,
				&nextStage,
				&reqParCheck);
			postInfo->SetStage((PostInfo::EStage)nextStage);
			if (reqParCheck)
			{
				postInfo->SetRequestParCheck(true);
			}
			postInfo->SetWorking(false);
		}
	}
}

inline void PrePostProcessor::DownloadQueueUpdate(void* aspect)
{
	if (IsStopped())
	{
		return;
	}

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaNzbFound ||
		queueAspect->action == DownloadQueue::eaUrlFound)
	{
		NzbFound(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbAdded ||
		queueAspect->action == DownloadQueue::eaUrlAdded)
	{
		NzbAdded(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbNamed)
	{
		g_QueueScriptCoordinator->EnqueueScript(queueAspect->nzbInfo, QueueScriptCoordinator::qeNzbNamed);
		CheckDownloadCompleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbDeleted &&
		queueAspect->nzbInfo->GetDeleting() &&
		!queueAspect->nzbInfo->GetPostInfo() &&
		queueAspect->nzbInfo->GetFileList()->empty())
	{
		// the deleting of nzbs is usually handled via eaFileDeleted-event, but when deleting nzb without
		// any files left the eaFileDeleted-event is not fired and we need to process eaNzbDeleted-event instead
		queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s deleted from queue", queueAspect->nzbInfo->GetName());
		NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaUrlDeleted)
	{
		NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaUrlFailed)
	{
		NzbCompleted(queueAspect->downloadQueue, queueAspect->nzbInfo, true);
	}
	else if (queueAspect->action == DownloadQueue::eaFileCompleted ||
		queueAspect->action == DownloadQueue::eaFileDeleted)
	{
		if (queueAspect->action == DownloadQueue::eaFileCompleted)
		{
			FileDownloaded(queueAspect->downloadQueue, queueAspect->nzbInfo, queueAspect->fileInfo);
		}

#ifndef DISABLE_PARCHECK
		{
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			for (NzbInfo* postJob : m_activeJobs)
			{
				PostInfo* pjInfo = postJob ? postJob->GetPostInfo() : nullptr;
				if (postJob &&
					queueAspect->fileInfo->GetNzbInfo() == postJob &&
					pjInfo &&
					pjInfo->GetPostThread() &&
					pjInfo->GetStage() >= PostInfo::ptLoadingPars &&
					pjInfo->GetStage() <= PostInfo::ptVerifyingRepaired &&
					((RepairController*)pjInfo->GetPostThread())->AddPar(
						queueAspect->fileInfo, queueAspect->action == DownloadQueue::eaFileDeleted))
				{
					return;
				}
			}
		}
#endif

		int eventAction = nzbget_rs_prepost_classify_file_event(
			queueAspect->action == DownloadQueue::eaFileCompleted ? 1 : 0,
			queueAspect->action == DownloadQueue::eaFileDeleted ? 1 : 0,
			queueAspect->fileInfo->GetDupeDeleted() ? 1 : 0,
			(int)queueAspect->fileInfo->GetNzbInfo()->GetDeleteStatus(),
			(int)NzbInfo::dsNone,
			(int)NzbInfo::dsHealth,
			queueAspect->nzbInfo->GetPostInfo() ? 1 : 0,
			queueAspect->nzbInfo->IsDownloadCompleted(true) ? 1 : 0,
			queueAspect->nzbInfo->IsDownloadCompleted(false) ? 1 : 0);
		if (eventAction == 1)
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s completely downloaded", queueAspect->nzbInfo->GetName());
			g_QueueScriptCoordinator->EnqueueScript(queueAspect->nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
			NzbDownloaded(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
		else if (eventAction == 2)
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s deleted from queue", queueAspect->nzbInfo->GetName());
			NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
	}
}

inline void PrePostProcessor::NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetDupeCheck() && nzbInfo->GetDupeMode() != dmForce)
	{
		g_DupeCoordinator->NzbFound(downloadQueue, nzbInfo);
	}
}

inline void PrePostProcessor::NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetParCheck() != Options::pcForce)
	{
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
	}

	if (g_Options->GetReorderFiles() && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Reordering files for %s", nzbInfo->GetName());
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupSortFiles, nullptr);
	}

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsDupe ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsCopy ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsGood ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsScan)
	{
		NzbCompleted(downloadQueue, nzbInfo, false);
	}
	else
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbAdded);
	}
}

inline void PrePostProcessor::CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	// Direct rename can pause the last PAR volumes after the data files
	// finished. Their in-flight articles may still be draining; the last
	// such request does not complete a file, so check both transitions.
	if (nzbget_rs_prepost_should_complete_after_direct_rename(
		IsStopped() ? 1 : 0,
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsSuccess,
		nzbInfo->GetPostInfo() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		!nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
		nzbInfo->IsDownloadCompleted(true) ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0) != 0)
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s completely downloaded", nzbInfo->GetName());
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
		NzbDownloaded(downloadQueue, nzbInfo);
	}
}

inline void PrePostProcessor::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (!nzbInfo)
	{
		return;
	}

	int enqueueDeleted = 0;
	int setParSkipped = 0;
	int dlRoute = nzbget_rs_prepost_classify_nzb_downloaded_entry(
		nzbInfo->GetSkipScriptProcessing() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsHealth,
		(int)NzbInfo::dsBad,
		nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		(int)nzbInfo->GetParStatus(),
		(int)NzbInfo::psNone,
		(int)g_Options->GetParCheck(),
		(int)Options::pcAlways,
		(int)Options::pcForce,
		&enqueueDeleted,
		&setParSkipped);
	if (dlRoute == 1)
	{
		if (nzbInfo->GetLiveRepairThread())
		{
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}
		nzbInfo->SetCleanupDisk(true);
		DeleteCleanup(nzbInfo);
		NzbCompleted(downloadQueue, nzbInfo, true);
		return;
	}
	if (enqueueDeleted)
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}
	if (dlRoute == 2)
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Queueing %s for post-processing", nzbInfo->GetName());

		nzbInfo->EnterPostProcess();

		if (setParSkipped)
		{
			nzbInfo->SetParStatus(NzbInfo::psSkipped);
		}

		if (nzbInfo->GetUnpackThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(true);
			{
				std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
				m_activeJobs.push_back(nzbInfo);
			}
			((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDownloaded(downloadQueue, nzbInfo);
		}

		if (nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(true);
			if (!nzbInfo->GetUnpackThread())
			{
				std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
				m_activeJobs.push_back(nzbInfo);
			}
		}

		nzbInfo->SetChanged(true);
		downloadQueue->SaveChanged();

		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_queuedJobs++;
		m_waitCond.notify_all();
	}
	else
	{
		if (nzbInfo->GetUnpackThread())
		{
			((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDownloaded(downloadQueue, nzbInfo);
		}

		if (nzbInfo->GetLiveRepairThread())
		{
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}

		NzbCompleted(downloadQueue, nzbInfo, true);
	}
}

inline void PrePostProcessor::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (nzbInfo->GetUnpackThread())
	{
		((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDeleted(downloadQueue, nzbInfo);
	}

	if (nzbInfo->GetLiveRepairThread())
	{
		// detach FIRST: the live pass re-checks attachment at every locked
		// touchpoint and aborts without writing anything back
		Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
		nzbInfo->SetLiveRepairThread(nullptr);
		liveRepairThread->Stop();
	}

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
	}
	bool callDownloaded = nzbInfo->GetDeleteStatus() == NzbInfo::dsHealth ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsBad;
	nzbInfo->SetDeleting(false);

	DeleteCleanup(nzbInfo);

	if (callDownloaded)
	{
		NzbDownloaded(downloadQueue, nzbInfo);
	}
	else
	{
		NzbCompleted(downloadQueue, nzbInfo, true);
	}
}

inline void PrePostProcessor::NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool saveQueue)
{
	int addToHistoryInt = 0, callDupeCompletedInt = 0, enqueueDeletedScriptInt = 0, needSaveInt = 0;
	nzbget_rs_prepost_eval_nzb_completed(
		(int)nzbInfo->GetDupeHint(),
		(int)NzbInfo::dhRedownloadAuto,
		g_Options->GetKeepHistory(),
		nzbInfo->GetAvoidHistory() ? 1 : 0,
		g_Options->GetDupeCheck() ? 1 : 0,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce,
		DupeCoordinator::FailsOver(nzbInfo) ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)NzbInfo::dsHealth,
		(int)NzbInfo::dsBad,
		(int)NzbInfo::dsScan,
		(int)NzbInfo::dsCopy,
		&addToHistoryInt,
		&callDupeCompletedInt,
		&enqueueDeletedScriptInt,
		&needSaveInt);
	bool addToHistory = addToHistoryInt != 0;
	if (addToHistory)
	{
		g_HistoryCoordinator->AddToHistory(downloadQueue, nzbInfo);
	}
	nzbInfo->SetAvoidHistory(false);
	bool needSave = needSaveInt != 0;
	if (callDupeCompletedInt)
	{
		g_DupeCoordinator->NzbCompleted(downloadQueue, nzbInfo);
		needSave = true;
	}
	if (enqueueDeletedScriptInt)
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}

	if (!addToHistory)
	{
		g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
		ParDamage::Forget(nzbInfo->GetId());
		downloadQueue->GetQueue()->Remove(nzbInfo);
		// the removal must reach the disk: the saved queue may hold the download
		// with its files already flagged deleted, which a restart before the next
		// save brought back as a queued download without files (B42)
		needSave = true;
	}

	if (saveQueue && needSave)
	{
		downloadQueue->Save();
	}
}

inline void PrePostProcessor::DeleteCleanup(NzbInfo* nzbInfo)
{
	int cleanupMode = nzbget_rs_prepost_classify_delete_cleanup_mode(
		nzbInfo->GetCleanupDisk() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsDupe,
		nzbInfo->GetParking() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0);
	if (cleanupMode != 0)
	{
		if (cleanupMode == 1)
		{
			if (FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
			{
				CString errmsg;
				detail("Deleting dir %s", nzbInfo->GetDestDir());
				if (!FileSystem::DeleteDirectoryWithContent(nzbInfo->GetDestDir(), errmsg))
				{
					error("Could not delete directory %s: %s", nzbInfo->GetDestDir(), *errmsg);
				}
			}
			return;
		}

		// download was cancelled, deleting already downloaded files from disk
		for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, completedFile.GetFilename());
			BString<1024> fullFileName;
			fullFileName.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
			nzbget_rs_free(fullFnBuf);
			if (FileSystem::FileExists(fullFileName))
			{
				detail("Deleting file %s", completedFile.GetFilename());
				FileSystem::DeleteFile(fullFileName);
			}
		}

		// delete .out.tmp-files
		DirBrowser dir(nzbInfo->GetDestDir());
		while (const char* filename = dir.Next())
		{
			size_t fnLen = strlen(filename);
			if (fnLen >= 8 && !strcmp(filename + fnLen - 8, ".out.tmp"))
			{
				NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
				BString<1024> fullFilename;
				fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
				nzbget_rs_free(fullFnBuf);
				detail("Deleting file %s", filename);
				FileSystem::DeleteFile(fullFilename);
			}
		}

		// delete old directory (if empty)
		FileSystem::DeleteDirectory(nzbInfo->GetDestDir());

		// delete final directory (if empty)
		const auto finalDir = nzbInfo->BuildFinalDirName();
		if (nzbInfo->GetDestDir() != finalDir && FileSystem::DirectoryExists(finalDir))
		{
			FileSystem::DeleteDirectory(finalDir);
		}
	}
}

inline void PrePostProcessor::CheckRequestPar(DownloadQueue* downloadQueue)
{
#ifndef DISABLE_PARCHECK
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	for (NzbInfo* postJob : m_activeJobs)
	{
		PostInfo* postInfo = postJob->GetPostInfo();

		int newForceParFull = postInfo->GetForceParFull() ? 1 : 0;
		int reqAction = nzbget_rs_prepost_classify_request_par_action(
			postInfo->GetRequestParCheck() ? 1 : 0,
			(int)postInfo->GetNzbInfo()->GetParStatus(),
			(int)NzbInfo::psSkipped,
			postInfo->GetForceRepair() ? 1 : 0,
			postInfo->GetNzbInfo()->GetParFull() ? 1 : 0,
			(int)g_Options->GetParCheck(),
			(int)Options::pcManual,
			newForceParFull,
			postInfo->GetNzbInfo()->GetFileList()->empty() ? 1 : 0,
			&newForceParFull);
		if (reqAction == 1)
		{
			postInfo->SetForceParFull(newForceParFull != 0);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psNone);
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
			postInfo->SetStage(PostInfo::ptQueued);
			postInfo->SetWorking(false);
		}
		else if (reqAction == 2 || reqAction == 3)
		{
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psManual);

			if (reqAction == 2)
			{
				postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
					"Downloading all remaining files for manual par-check for %s", postInfo->GetNzbInfo()->GetName());
				downloadQueue->EditEntry(postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupResume, nullptr);
				postInfo->SetStage(PostInfo::ptFinished);
			}
			else
			{
				postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
					"There are no par-files remain for download for %s", postInfo->GetNzbInfo()->GetName());
			}
			postInfo->SetWorking(false);
		}
	}
#endif
}

inline void PrePostProcessor::CleanupJobs(DownloadQueue* downloadQueue)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	m_activeJobs.erase(std::remove_if(m_activeJobs.begin(), m_activeJobs.end(),
		[processor = this, downloadQueue](NzbInfo* postJob)
		{
			PostInfo* postInfo = postJob->GetPostInfo();
			bool hasRunningThread = postInfo->GetPostThread() && postInfo->GetPostThread()->IsRunning();
			int64_t curTime = (!postInfo->GetWorking() && !hasRunningThread && postInfo->GetStartTime() > 0)
				? (int64_t)Util::CurrentTime()
				: 0;
			int updateTotalSec = 0;
			int isCompleted = 0;
			int newTotalSec = postJob->GetPostTotalSec();
			if (nzbget_rs_prepost_eval_cleanup_job_state(
				postInfo->GetWorking() ? 1 : 0,
				hasRunningThread ? 1 : 0,
				(int)postInfo->GetStage(),
				(int)PostInfo::ptFinished,
				postInfo->GetDeleted() ? 1 : 0,
				(int64_t)postInfo->GetStartTime(),
				curTime,
				postJob->GetPostTotalSec(),
				&updateTotalSec,
				&isCompleted,
				&newTotalSec) != 0)
			{
				delete postInfo->GetPostThread();
				postInfo->SetPostThread(nullptr);

				postInfo->SetStageTime(0);
				postInfo->SetStageProgress(0);
				postInfo->SetFileProgress(0);
				postInfo->SetProgressLabel("");

				if (updateTotalSec != 0)
				{
					postJob->SetPostTotalSec(newTotalSec);
					postInfo->SetStartTime(0);
				}

				if (isCompleted != 0)
				{
					processor->JobCompleted(downloadQueue, postInfo);
				}
				else
				{
					postInfo->SetStage(PostInfo::ptQueued);
				}
				return true;
			}
			return false;
		}),
		m_activeJobs.end());
}

inline bool PrePostProcessor::CanRunMoreJobs(bool* allowPar)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	int allowParInt = 0;
	int canRun = nzbget_rs_prepost_can_run_more_jobs(
		stages.data(),
		stages.size(),
		(int)PostInfo::ptLoadingPars,
		(int)PostInfo::ptVerifyingRepaired,
		(int)PostInfo::ptRepairing,
		(int)g_Options->GetPostStrategy(),
		&allowParInt);
	*allowPar = allowParInt != 0;
	return canRun != 0;
}

inline NzbInfo* PrePostProcessor::PickNextJob(DownloadQueue* downloadQueue, bool allowPar)
{
	NzbInfo* nzbInfo = nullptr;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);

	bool hasActiveUnpack = HasActiveUnpack(downloadQueue);

	std::vector<NzbInfo*> items;
	std::vector<NzbgetRsPostJobCandidate> candidates;
	items.reserve(downloadQueue->GetQueue()->size());
	candidates.reserve(downloadQueue->GetQueue()->size());
	for (NzbInfo* nzbInfo1 : downloadQueue->GetQueue())
	{
		items.push_back(nzbInfo1);
		NzbgetRsPostJobCandidate c{};
		PostInfo* pi = nzbInfo1->GetPostInfo();
		c.has_post_info = pi ? 1 : 0;
		c.working = (pi && pi->GetWorking()) ? 1 : 0;
		c.direct_unpack_running = nzbInfo1->GetDirectUnpackStatus() == NzbInfo::nsRunning ? 1 : 0;
		c.has_live_repair_thread = nzbInfo1->GetLiveRepairThread() != nullptr ? 1 : 0;
		c.priority = nzbInfo1->GetPriority();
		c.force_priority = nzbInfo1->GetForcePriority() ? 1 : 0;
		c.need_par_check = (pi && pi->GetNeedParCheck()) ? 1 : 0;
		c.need_unpack = (pi && pi->GetNeedUnpack()) ? 1 : 0;
		if (pi && !pi->GetWorking() && !c.direct_unpack_running && !c.has_live_repair_thread)
		{
			c.has_queue_script_job = g_QueueScriptCoordinator->HasJob(nzbInfo1->GetId(), nullptr) ? 1 : 0;
			c.in_active_jobs = std::find(m_activeJobs.begin(), m_activeJobs.end(), nzbInfo1) != m_activeJobs.end() ? 1 : 0;
			c.download_completed = nzbInfo1->IsDownloadCompleted(true) ? 1 : 0;
		}
		candidates.push_back(c);
	}
	int picked = nzbget_rs_prepost_pick_next_post_job(
		candidates.data(),
		candidates.size(),
		allowPar ? 1 : 0,
		hasActiveUnpack ? 1 : 0,
		g_WorkState->GetPausePostProcess() ? 1 : 0);
	if (picked >= 0 && (size_t)picked < items.size())
	{
		nzbInfo = items[(size_t)picked];
	}

	return nzbInfo;
}

inline void PrePostProcessor::CheckPostQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	size_t countBefore;
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		countBefore = m_activeJobs.size();
	}
	CheckRequestPar(downloadQueue);
	CleanupJobs(downloadQueue);
	bool changed;
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		changed = m_activeJobs.size() != countBefore;
	}

	bool allowPar;
	while (CanRunMoreJobs(&allowPar) && !IsStopped())
	{
		NzbInfo* postJob = PickNextJob(downloadQueue, allowPar);
		if (!postJob)
		{
			break;
		}

		{
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			m_activeJobs.push_back(postJob);
		}

		PostInfo* postInfo = postJob->GetPostInfo();
		if (postInfo->GetStage() == PostInfo::ptQueued &&
			(!g_WorkState->GetPausePostProcess() || postInfo->GetNzbInfo()->GetForcePriority()))
		{
			StartJob(downloadQueue, postInfo, allowPar);
			CheckRequestPar(downloadQueue);
			CleanupJobs(downloadQueue);
			changed = true;
		}
	}

	if (changed)
	{
		downloadQueue->Save();
		UpdatePauseState();
	}

	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		Util::SetStandByMode(m_activeJobs.empty());
	}
}

#ifndef DISABLE_PARCHECK
// The collection's par2 files provably cannot cover its damage: the bytes
// still missing from the files they protect exceed all par2 data still
// intact. Health below critical says the same for the whole collection, but
// it also counts damage in files outside the par2 set and never drops for
// partly repaired files, so it only gates the closer look: the remaining
// holes of every stream-repair job whose file the main par2 file lists.
// Several par sets can't be judged from collection-wide sizes, so they are
// always left to par-check.
static inline bool ParCannotCover(NzbInfo* nzbInfo)
{
	int health = nzbInfo->CalcHealth();
	int criticalHealth = nzbInfo->CalcCriticalHealth(false);
	if (health == 0 || health >= criticalHealth)
	{
		return false;
	}

	ParParser::ParFileList mainPars;
	bool foundMainPars = ParParser::FindMainPars(nzbInfo->GetDestDir(), &mainPars);
	if (!foundMainPars || mainPars.size() != 1)
	{
		return false;
	}

	// FindMainPars reports whichever file of the set it meets first; the
	// description packets are cheapest to read from its smallest file (the
	// index, unless every volume was downloaded)
	std::string parPath;
	int64 parFileSize = -1;
	DirBrowser dir(nzbInfo->GetDestDir());
	while (const char* filename = dir.Next())
	{
		if (ParParser::SameParCollection(filename, mainPars[0].c_str(), true))
		{
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
			BString<1024> path;
			path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
			nzbget_rs_free(pathBuf);
			int64 size = FileSystem::FileSize(path);
			if (size >= 0 && (parFileSize < 0 || size < parFileSize))
			{
				parPath = *path;
				parFileSize = size;
			}
		}
	}

	std::vector<std::string> parredFiles;
	if (parFileSize < 0 || !ParParser::ListParredFiles(parPath.c_str(), parredFiles))
	{
		return false;
	}

	int64 protectedDamage = 0;
	for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
	{
		std::string filename = job.GetFilename();
		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetId() == job.GetFileId())
			{
				filename = completedFile.GetFilename();
				break;
			}
		}
		const char* baseName = FileSystem::BaseFileName(filename.c_str());
		if (std::any_of(parredFiles.begin(), parredFiles.end(),
			[baseName](const std::string& parredFile) { return nzbget_rs_dqh_bytes_eq_ignore_case(parredFile.c_str(), baseName) != 0; }))
		{
			// a whole-file job has no decoded geometry yet: its encoded size
			// is the closest measure of its damage
			if (job.GetDecodedFileSize() > 0)
			{
				protectedDamage += DupeStreamRepair::TotalSize(*job.GetHoles());
			}
			else
			{
				protectedDamage += job.GetFailedSize() + job.GetMissedSize();
			}
		}
	}

	int64 intactPar = std::max<int64>(0, nzbInfo->GetParSize() - nzbInfo->GetParCurrentFailedSize());
	return protectedDamage > intactPar;
}

// ParScan=dupe can still find missing blocks in the files of a duplicate
// already downloaded to disk, which the health estimate does not know about
static inline bool HasDupeScanSources(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetParScan() != Options::psDupe)
	{
		return false;
	}
	for (NzbInfo* dupeNzbInfo : g_DupeCoordinator->ListHistoryDupes(downloadQueue, nzbInfo))
	{
		if (!Util::EmptyStr(dupeNzbInfo->GetDestDir()) &&
			FileSystem::DirectoryExists(dupeNzbInfo->GetDestDir()))
		{
			return true;
		}
	}
	return false;
}
#endif

inline void PrePostProcessor::StartJob(DownloadQueue* downloadQueue, PostInfo* postInfo, bool allowPar)
{
	NzbInfo* nzbInfo = postInfo->GetNzbInfo();

	if (!postInfo->GetStartTime())
	{
		postInfo->SetStartTime(Util::CurrentTime());
	}
	postInfo->SetStageTime(Util::CurrentTime());
	postInfo->SetStageProgress(0);
	postInfo->SetFileProgress(0);
	postInfo->SetProgressLabel("");

	if (nzbInfo->GetParRenameStatus() == NzbInfo::rsNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		g_Options->GetParRename())
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptParRenaming);
		RenameController::StartJob(postInfo, RenameController::jkPar);
		return;
	}

	bool streamRepair = g_Options->GetDupeArticleFallback() >= Options::dafStream &&
		!nzbInfo->GetStreamRepairJobs()->empty() &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone;

	// stream repair is the last option (B78): it can take longer than downloading
	// a good backup whole, so with HealthCheck=dupe and a backup still untried in
	// history the download fails over instead (par2 still gets its turn)
	if (streamRepair && !postInfo->GetStreamRepairDone() &&
		g_Options->GetHealthCheck() == Options::hcDupe &&
		DupeCoordinator::FailsOver(nzbInfo) &&
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Not repairing %s from duplicates: a backup in history is tried first", nzbInfo->GetName());
		streamRepair = false;
	}

#ifndef DISABLE_PARCHECK
	// A par-check that cannot succeed still reads the whole collection (and
	// waits for every remaining par2-file): recover from duplicates first,
	// par-check afterwards only if that wrote anything
	if (streamRepair && !postInfo->GetStreamRepairDone() &&
		nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		g_Options->GetParCheck() == Options::pcAuto &&
		ParCannotCover(nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Par2 files of %s cannot cover its damage (health %.1f%%, critical %.1f%%), "
			"recovering from duplicates before par-check",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			nzbInfo->CalcCriticalHealth(false) / 10.0);
		postInfo->SetParCannotCover(true);
		EnterStage(downloadQueue, postInfo, PostInfo::ptStreamRepairing);
		StreamRepairController::StartJob(postInfo);
		return;
	}

	// Duplicates recovered nothing, so the damage is still beyond the par2
	// files: report the failure instead of verifying a collection that
	// cannot be repaired. Any byte ever recovered from duplicates (a live
	// pass, or a pass interrupted by a restart) may have brought the damage
	// within reach, so then par-check still runs.
	if (postInfo->GetParCannotCover() && postInfo->GetStreamRepairDone() &&
		!postInfo->GetStreamRepairRecovered() &&
		nzbInfo->GetDupeRecoveredBytes() <= 0 &&
		g_Options->GetParCheck() == Options::pcAuto &&
		nzbInfo->GetParStatus() <= NzbInfo::psNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		!HasDupeScanSources(downloadQueue, nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping par-check for %s: its par2 files cannot cover the damage "
			"(health %.1f%%, critical %.1f%%) and duplicates recovered nothing",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			nzbInfo->CalcCriticalHealth(false) / 10.0);
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}

	// Captured holes are a fallback, not evidence that donor data is needed.
	// Let the current PAR set verify and repair first, including auto mode
	// which initially marks the check as skipped until damage is known.
	if (streamRepair && nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		postInfo->SetRequestParCheck(true);
		return;
	}

	// damaged, and the par2-files downloaded so far all failed (a lost index file):
	// any par2-volume carries the set's description too, so the smallest one held
	// back is fetched, and the job waits for it (it isn't picked while files download)
	if ((nzbInfo->GetParStatus() == NzbInfo::psNone || nzbInfo->GetParStatus() == NzbInfo::psSkipped) &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		nzbInfo->CalcHealth() < 1000 &&
		!ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		FileInfo* smallest = nullptr;
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (fileInfo->GetParFile() && fileInfo->GetPaused() && !fileInfo->GetDeleted() &&
				(!smallest || fileInfo->GetSize() < smallest->GetSize()))
			{
				smallest = fileInfo;
			}
		}
		if (smallest)
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				"No par2-file of %s on disk to check it with: unpausing %s", nzbInfo->GetName(), smallest->GetFilename());
			smallest->SetPaused(false);
			smallest->SetExtraPriority(true);
			nzbInfo->SetChanged(true);
			downloadQueue->SaveChanged();
			return;
		}
	}

	if (nzbInfo->GetParStatus() == NzbInfo::psNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		if (ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
		{
			if (!allowPar)
			{
				postInfo->SetNeedParCheck(true);
				return;
			}

			EnterStage(downloadQueue, postInfo, PostInfo::ptLoadingPars);
			postInfo->SetNeedParCheck(false);
			RepairController::StartJob(postInfo);
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				"Nothing to par-check for %s", nzbInfo->GetName());
			nzbInfo->SetParStatus(NzbInfo::psSkipped);
		}
		return;
	}
	
	int skippedParAction = nzbget_rs_prepost_classify_skipped_par_health(
		(int)nzbInfo->GetParStatus(),
		(int)NzbInfo::psSkipped,
		(int)g_Options->GetParScan(),
		(int)Options::psDupe,
		nzbInfo->CalcHealth(),
		nzbInfo->CalcCriticalHealth(false),
		nzbInfo->GetFailedSize(),
		nzbInfo->GetParFailedSize());
	if ((skippedParAction == 1 || skippedParAction == 2) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		if (skippedParAction == 1)
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health 0%%", nzbInfo->GetName());
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health %.1f%% below critical %.1f%%",
				nzbInfo->GetName(),
				nzbInfo->CalcHealth() / 10.0,
				nzbInfo->CalcCriticalHealth(false) / 10.0);
		}
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}
	if (skippedParAction == 3 &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s with health %.1f%% needs par-check",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0);
		postInfo->SetRequestParCheck(true);
		return;
	}
#endif

	// Successful PAR sets remove the holes they actually protect. Remaining
	// jobs may belong to unprotected files, or to a set that could not repair.
	// Manual repair and sufficient parity with ParRepair=no remain user choices.
	if (streamRepair &&
		nzbInfo->GetParStatus() != NzbInfo::psRepairPossible &&
		nzbInfo->GetParStatus() != NzbInfo::psManual)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptStreamRepairing);
		StreamRepairController::StartJob(postInfo);
		return;
	}

	NzbParameter* unpackParameter = nzbInfo->GetParameters()->Find("*Unpack:");
	int wantUnpackInt = 0;
	int unpackInt = 0;
	bool startRarRename = nzbget_rs_prepost_eval_want_unpack_and_rar_rename(
		unpackParameter != nullptr ? 1 : 0,
		unpackParameter ? unpackParameter->GetValue() : "",
		(int)nzbInfo->GetUnpackStatus(),
		(int)NzbInfo::usNone,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)nzbInfo->GetRarRenameStatus(),
		(int)NzbInfo::rsNone,
		g_Options->GetRarRename() ? 1 : 0,
		&wantUnpackInt,
		&unpackInt) != 0;
	bool wantUnpack = wantUnpackInt != 0;
	bool unpack = unpackInt != 0;
	if (startRarRename)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptRarRenaming);
		RenameController::StartJob(postInfo, RenameController::jkRar);
		return;
	}

#ifndef DISABLE_PARCHECK
	if (nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		g_Options->GetParCheck() == Options::pcAuto &&
		!UnpackController::HasCompletedArchiveFiles(nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Requesting par-check for collection %s without archive files",
			nzbInfo->GetName());
		postInfo->SetRequestParCheck(true);
		return;
	}
#endif

	NzbgetRsPostStageDecision stageDec{};
	nzbget_rs_prepost_eval_post_stages(
		wantUnpack ? 1 : 0,
		(int)nzbInfo->GetUnpackStatus(),
		(int)nzbInfo->GetDeleteStatus(),
		(int)nzbInfo->GetParStatus(),
		(int)nzbInfo->GetCleanupStatus(),
		!Util::EmptyStr(g_Options->GetExtCleanupDisk()) ? 1 : 0,
		nzbInfo->CalcHealth(),
		(int)nzbInfo->GetMoveStatus(),
		nzbInfo->GetDestDir(),
		g_Options->GetInterDir(),
		PATH_SEPARATOR,
		(int)nzbInfo->GetDirectUnpackStatus(),
		g_Options->GetPostDownloadRename() ? 1 : 0,
		(int)nzbInfo->GetPostDownloadRenamingStatus(),
		!Util::EmptyStr(nzbInfo->GetDestDir()) ? 1 : 0,
		!Util::EmptyStr(nzbInfo->GetName()) ? 1 : 0,
		g_Options->GetRenameAfterUnpack() ? 1 : 0,
		(int)nzbInfo->GetPostUnpackRenamingStatus(),
		&stageDec);
	if (stageDec.skip_unpack_due_to_par)
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping unpack for %s due to %s", nzbInfo->GetName(),
			nzbInfo->GetParStatus() == NzbInfo::psManual ? "required par-repair" : "par-failure");
		nzbInfo->SetUnpackStatus(NzbInfo::usSkipped);
	}
	unpack = stageDec.unpack != 0;
	bool cleanup = stageDec.cleanup != 0;
	bool moveInter = stageDec.move_inter != 0;
	bool postDownloadRenaming = stageDec.post_download_renaming != 0;
	bool postUnpackRenaming = stageDec.post_unpack_renaming != 0;

	if (unpack)
	{
		if (HasActiveUnpack(downloadQueue))
		{
			postInfo->SetNeedUnpack(true);
			return;
		}
		postInfo->SetNeedUnpack(false);
		EnterStage(downloadQueue, postInfo, PostInfo::ptUnpacking);
		UnpackController::StartJob(postInfo);
	}
	else if (cleanup)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptCleaningUp);
		CleanupController::StartJob(postInfo);
	}
	else if (moveInter)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptMoving);
		MoveController::StartJob(postInfo);
	}
	else if (postDownloadRenaming)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptPostDownloadRenaming);
		PostRenamerController::StartJob(postInfo, false);
	}
	else if (postUnpackRenaming)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptPostUnpackRenaming);
		PostRenamerController::StartJob(postInfo, true);
	}
	else
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptExecutingScript);
		PostScriptController::StartJob(postInfo);
	}
}

inline void PrePostProcessor::EnterStage(DownloadQueue* downloadQueue, PostInfo* postInfo, PostInfo::EStage stage)
{
	postInfo->SetWorking(true);
	postInfo->SetStage(stage);
}

inline void PrePostProcessor::JobCompleted(DownloadQueue* downloadQueue, PostInfo* postInfo)
{
	NzbInfo* nzbInfo = postInfo->GetNzbInfo();

	nzbInfo->LeavePostProcess();

	if (nzbInfo->IsDownloadCompleted(true))
	{
		NzbCompleted(downloadQueue, nzbInfo, false);
	}

	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_queuedJobs--;
}

inline void PrePostProcessor::UpdatePauseState()
{
	bool needPause = false;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	needPause = nzbget_rs_prepost_calc_post_pause_state(
		stages.data(),
		stages.size(),
		g_Options->GetParPauseQueue() ? 1 : 0,
		g_Options->GetUnpackPauseQueue() ? 1 : 0,
		g_Options->GetScriptPauseQueue() ? 1 : 0) != 0;

	if (needPause && !g_WorkState->GetTempPauseDownload())
	{
		info("Pausing download before post-processing");
	}
	else if (!needPause && g_WorkState->GetTempPauseDownload())
	{
		info("Unpausing download after post-processing");
	}

	g_WorkState->SetTempPauseDownload(needPause);
}

inline bool PrePostProcessor::EditList(DownloadQueue* downloadQueue, IdList* idList,
	DownloadQueue::EEditAction action, const char* args)
{
	debug("Edit-command for post-processor received");
	switch (action)
	{
		case DownloadQueue::eaPostDelete:
			return PostQueueDelete(downloadQueue, idList);

		default:
			return false;
	}
}

inline bool PrePostProcessor::PostQueueDelete(DownloadQueue* downloadQueue, IdList* idList)
{
	bool ok = false;

	for (int id : *idList)
	{
		for (NzbInfo* nzbInfo: downloadQueue->GetQueue())
		{
			PostInfo* postInfo = nzbInfo->GetPostInfo();
			if (postInfo && nzbInfo->GetId() == id)
			{
				if (postInfo->GetWorking())
				{
					postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
						"Deleting active post-job %s", postInfo->GetNzbInfo()->GetName());
					postInfo->SetDeleted(true);
					if (postInfo->GetPostThread())
					{
						debug("Terminating post-process thread for %s", postInfo->GetNzbInfo()->GetName());
						postInfo->GetPostThread()->Stop();
						ok = true;
					}
					else if (postInfo->GetNzbInfo()->GetUnpackThread())
					{
						((DirectUnpack*)postInfo->GetNzbInfo()->GetUnpackThread())->NzbDeleted(downloadQueue, postInfo->GetNzbInfo());
						ok = true;
					}
					else if (postInfo->GetNzbInfo()->GetLiveRepairThread())
					{
						// the job is waiting for a live stream-repair pass:
						// detach first (the pass aborts at its next locked
						// touchpoint without writing back), stop its fetches
						// and release the job
						Thread* liveRepairThread = postInfo->GetNzbInfo()->GetLiveRepairThread();
						postInfo->GetNzbInfo()->SetLiveRepairThread(nullptr);
						liveRepairThread->Stop();
						postInfo->SetWorking(false);
						ok = true;
					}
					else
					{
						error("Internal error in PrePostProcessor::QueueDelete");
					}
				}
				else
				{
					postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
						"Deleting queued post-job %s", postInfo->GetNzbInfo()->GetName());

					JobCompleted(downloadQueue, postInfo);

					{
						std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
						m_activeJobs.erase(std::remove_if(m_activeJobs.begin(), m_activeJobs.end(),
							[postInfo](NzbInfo* postJob)
							{
								return postInfo == postJob->GetPostInfo();
							}),
							m_activeJobs.end());
					}

					ok = true;
				}
				break;
			}
		}
	}

	if (ok)
	{
		downloadQueue->Save();
	}

	return ok;
}

inline void PrePostProcessor::FileDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileInfo* fileInfo)
{
	if (!nzbInfo->GetPostInfo())
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeFileDownloaded);
	}

	int duAction = nzbget_rs_prepost_classify_file_downloaded_direct_unpack(
		g_Options->GetDirectUnpack() ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		(int)nzbInfo->GetDirectUnpackStatus(),
		(int)NzbInfo::nsNone,
		(int)NzbInfo::nsRunning,
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsRunning,
		nzbInfo->GetUnpackThread() != nullptr ? 1 : 0);
	if (duAction == 1)
	{
		bool allowPar;
		if (DirectUnpack::IsArchiveFilename(fileInfo->GetFilename()) &&
			CanRunMoreJobs(&allowPar) &&
			!HasActiveUnpack(downloadQueue))
		{
			NzbParameter* unpackParameter = nzbInfo->GetParameters()->Find("*Unpack:");
			bool wantUnpack = !unpackParameter || !strcasecmp(unpackParameter->GetValue(), "yes");
			if (wantUnpack && nzbInfo->GetFailedArticles() == 0)
			{
				DirectUnpack::StartJob(nzbInfo);
			}
		}
	}
	else if (duAction == 2)
	{
		((DirectUnpack*)nzbInfo->GetUnpackThread())->FileDownloaded(downloadQueue, fileInfo);
	}
}

#endif

#include <filesystem>
#include <fstream>
#include <memory>
#include <optional>

#include "nzbget_rs.h"

class ScanScriptCheck : public NzbScriptController
{
protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script) { has |= script->GetScanScript(); }
	bool has = false;
	friend class ScanScriptController;
};

class ScanScriptController : public NzbScriptController
{
public:
	ScanScriptController(
		std::string& nzbName,
		std::string& category,
		std::string& dupeKey)
		: m_nzbFilename(nullptr)
		, m_url(nullptr)
		, m_directory(nullptr)
		, m_nzbName{ nzbName }
		, m_category{ category }
		, m_dupeKey{ dupeKey }
		, m_priority(nullptr)
		, m_dupeScore(nullptr)
		, m_addTop(nullptr)
		, m_addPaused(nullptr)
		, m_dupeMode(nullptr)
		, m_parameters(nullptr) {}

	static void ExecuteScripts(
		const char* nzbFilename,
		NzbInfo* nzbInfo,
		const char* directory,
		std::string& nzbName,
		std::string& category,
		int* priority,
		NzbParameterList* parameters,
		bool* addTop,
		bool* addPaused,
		std::string& dupeKey,
		int* dupeScore,
		EDupeMode* dupeMode)
	{
		if (nzbInfo && nzbInfo->GetSkipScriptProcessing())
		{
			return;
		}

		ScanScriptController scriptController(nzbName, category, dupeKey);
		scriptController.m_nzbFilename = nzbFilename;
		scriptController.m_url = nzbInfo ? nzbInfo->GetUrl() : "";
		scriptController.m_directory = directory;
		scriptController.m_parameters = parameters;
		scriptController.m_priority = priority;
		scriptController.m_addTop = addTop;
		scriptController.m_addPaused = addPaused;
		scriptController.m_dupeScore = dupeScore;
		scriptController.m_dupeMode = dupeMode;
		scriptController.m_prefixLen = 0;

		const char* extensions = g_Options->GetExtensions();

		if (!category.empty())
		{
			Options::Category* categoryObj = g_Options->FindCategory(category.c_str(), false);
			if (categoryObj && !Util::EmptyStr(categoryObj->GetExtensions()))
			{
				extensions = categoryObj->GetExtensions();
			}
		}

		scriptController.ExecuteScriptList(extensions);
	}

	static bool HasScripts()
	{
		ScanScriptCheck check;
		check.ExecuteScriptList(g_Options->GetExtensions());
		// a category's extensions replace the global ones for its nzbs
		for (Options::Category& category : g_Options->GetCategories())
		{
			check.ExecuteScriptList(category.GetExtensions());
		}
		return check.has;
	}

protected:
	void ExecuteScript(std::shared_ptr<const Extension::Script> script) override
	{
		if (!script->GetScanScript() || !FileSystem::FileExists(m_nzbFilename))
		{
			return;
		}

		PrintMessage(Message::mkInfo, "Executing scan-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbFilename));

		SetArgs({script->GetEntry()});

		BString<1024> infoName("scan-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbFilename));
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		m_prefixLen = static_cast<int>(strlen(script->GetDisplayName())) + 2; // 2 = strlen(": ");
		PrepareParams(script->GetName());

		Execute();

		SetLogPrefix(nullptr);
	}

	void AddMessage(Message::EKind kind, const char* text) override
	{
		const char* msgText = text + m_prefixLen;

		NzbgetRsScanScriptCmd cmd{};
		int cmdKind = nzbget_rs_sus_parse_scan_script_cmd(msgText, &cmd);
		if (cmdKind != 0)
		{
			debug("Command %s detected", msgText + 6);
			switch (cmdKind)
			{
				case 1:
					m_nzbName = msgText + cmd.val_offset;
					break;
				case 2:
					m_category = msgText + cmd.val_offset;
					g_Scanner->InitPPParameters(m_category.c_str(), m_parameters, true);
					break;
				case 3:
				{
					std::string param(msgText + cmd.key_offset, cmd.key_len);
					m_parameters->SetParameter(param.c_str(), msgText + cmd.val_offset);
					break;
				}
				case 4:
					*m_priority = cmd.int_val;
					break;
				case 5:
					*m_addTop = cmd.int_val != 0;
					break;
				case 6:
					*m_addPaused = cmd.int_val != 0;
					break;
				case 7:
					m_dupeKey = msgText + cmd.val_offset;
					break;
				case 8:
					*m_dupeScore = cmd.int_val;
					break;
				case 9:
					*m_dupeMode = cmd.int_val == 1 ? dmAll : (cmd.int_val == 2 ? dmForce : dmScore);
					break;
				case -2:
					error("Invalid value \"%s\" for command \"DUPEMODE\" received from %s",
						msgText + cmd.val_offset, GetInfoName());
					break;
				default:
					error("Invalid command \"%s\" received from %s", msgText, GetInfoName());
					break;
			}
			return;
		}
		ScriptController::AddMessage(kind, text);
	}

	void SetPriority(int* priority) { m_priority = priority; }
	void SetDupeScore(int* dupeScore) { m_dupeScore = dupeScore; }
	void SetDupeMode(EDupeMode* dupeMode) { m_dupeMode = dupeMode; }
	void SetAddTop(bool* addTop) { m_addTop = addTop; }
	void SetAddPaused(bool* addPaused) { m_addPaused = addPaused; }
	void SetParameters(NzbParameterList* parameters) { m_parameters = parameters; }

private:
	const char* m_nzbFilename;
	const char* m_url;
	const char* m_directory;
	std::string& m_nzbName;
	std::string& m_category;
	std::string& m_dupeKey;
	int* m_priority;
	int* m_dupeScore;
	bool* m_addTop;
	bool* m_addPaused;
	EDupeMode* m_dupeMode;
	NzbParameterList* m_parameters;
	int m_prefixLen = 0;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		nzbget_rs_scan_script_prepare_env(
			m_nzbFilename,
			FileSystem::BaseFileName(m_nzbFilename),
			m_url,
			m_nzbName.c_str(),
			m_category.c_str(),
			*m_priority,
			*m_addTop ? 1 : 0,
			*m_addPaused ? 1 : 0,
			m_dupeKey.c_str(),
			*m_dupeScore,
			static_cast<int>(*m_dupeMode),
			m_directory,
			PATH_SEPARATOR,
			this,
			[](void* ctx, const char* name, const char* value)
			{
				static_cast<ScanScriptController*>(ctx)->SetEnvVar(name, value);
			},
			[](void* ctx, const char* name, int value)
			{
				static_cast<ScanScriptController*>(ctx)->SetIntEnvVar(name, value);
			});

		PrepareEnvScript(m_parameters, scriptName);
	}
};


inline Scanner::QueueData::QueueData(
	const char* filename,
	const char* nzbName,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	EAddStatus* addStatus,
	int* nzbId) 
	: m_filename{ filename ? filename : "" }
	, m_nzbName{ nzbName ? nzbName : "" }
	, m_category{ category ? category : "" }
	, m_autoCategory{ autoCategory }
	, m_priority{ priority }
	, m_dupeKey{ dupeKey ? dupeKey : "" }
	, m_dupeScore{ dupeScore }
	, m_dupeMode{ dupeMode }
	, m_addTop{ addTop }
	, m_addPaused{ addPaused }
	, m_urlInfo{ urlInfo }
	, m_addStatus{ addStatus }
	, m_nzbId{ nzbId }
{
	if (parameters)
	{
		m_parameters.CopyFrom(parameters);
	}
}

inline void Scanner::QueueData::SetAddStatus(EAddStatus addStatus)
{
	if (m_addStatus)
	{
		*m_addStatus = addStatus;
	}
}

inline void Scanner::QueueData::SetNzbId(int nzbId)
{
	if (m_nzbId)
	{
		*m_nzbId = nzbId;
	}
}


inline void Scanner::InitOptions()
{
	m_nzbDirInterval = 1;
	m_scanScript = ScanScriptController::HasScripts();

	CleanupStaleUnpackDir();
}

inline int Scanner::ServiceInterval()
{
	return m_requestedNzbDirScan ? Service::Now :
		g_Options->GetNzbDirInterval() <= 0 ? Service::Sleep :
		m_nzbDirInterval;
}

inline void Scanner::ServiceWork()
{
	debug("Scanner service work");

	if (!DownloadQueue::IsLoaded())
	{
		return;
	}

	m_nzbDirInterval = g_Options->GetNzbDirInterval();

	if (g_WorkState->GetPauseScan() && !m_requestedNzbDirScan)
	{
		return;
	}

	debug("Scanner service work: doing work");

	std::lock_guard<std::mutex> guard{m_scanMutex};

	// check nzbdir every g_Options->GetNzbDirInterval() seconds or if requested
	bool checkStat = !m_requestedNzbDirScan;
	m_requestedNzbDirScan = false;
	m_scanning = true;

	if (g_Options->GetNzbDirArchiveScan())
	{
		CheckIncomingArchives(g_Options->GetNzbDirPath(), checkStat);
	}

	CheckIncomingNzbs(g_Options->GetNzbDir(), "", checkStat);
	if (!checkStat && m_scanScript)
	{
		// if immediate scan requested, we need second scan to process files extracted by scan-scripts
		CheckIncomingNzbs(g_Options->GetNzbDir(), "", checkStat);
	}
	m_scanning = false;

	// if NzbDirFileAge is less than NzbDirInterval (that can happen if NzbDirInterval
	// is set for rare scans like once per hour) we make 4 scans:
	//   - one additional scan is necessary to check sizes of detected files;
	//   - another scan is required to check files which were extracted by scan-scripts;
	//   - third scan is needed to check sizes of extracted files.
	nzbget_rs_dsp_scanner_next_pass(
		g_Options->GetNzbDirInterval(),
		g_Options->GetNzbDirFileAge(),
		m_scanScript ? 1 : 0,
		m_pass,
		&m_nzbDirInterval,
		&m_pass);

	DropOldFiles();
	m_queueList.clear();
}

inline void Scanner::CheckIncomingArchives(const fs::path& dir, bool checkStat)
{
	const auto archives = FindArchives(dir, checkStat);
	UnpackArchives(archives);
}

inline std::vector<fs::path> Scanner::FindArchives(const fs::path& dir, bool checkStat)
{
	// the non-throwing forms: an unreadable subdirectory (lost+found, a
	// folder of another user) or one removed during the walk threw out of
	// the scanner thread and ended the program
	std::vector<fs::path> archives;
	fs::error_code ec;
	auto it = fs::recursive_directory_iterator(dir, fs::directory_options::skip_permission_denied, ec);
	for (; !ec && it != fs::recursive_directory_iterator(); it.increment(ec))
	{
		const auto& path = it->path();
		const std::string filename = fs::u8string(path.filename());
		fs::error_code typeEc;

		if (ShouldSkipItem(filename))
		{
			if (it->is_directory(typeEc))
			{
				it.disable_recursion_pending();
			}
			continue;
		}

		if (!it->is_regular_file(typeEc))
		{
			continue;
		}

		// like an nzb, an archive is taken once it stopped changing: one
		// still being copied in failed to extract and was moved aside for good
		const std::string pathStr = fs::u8string(path);
		if (nzbget_rs_unpack_can_extract_with_zipfast(pathStr.c_str()) != 0 &&
			CanProcessFile(pathStr.c_str(), checkStat))
		{
			archives.push_back(path);
		}
	}
	if (ec)
	{
		warn("Could not scan %s for archives: %s", fs::u8string(dir).c_str(), ec.message().c_str());
	}

	archives.shrink_to_fit();
	return archives;
}

inline std::optional<std::vector<fs::path>> Scanner::ProcessArchive(const fs::path& archiveFile, const fs::path& destDir)
{
	const auto unpackDir = (g_Options->GetTempDirPath() / UNPACK_DIR) / fs::make_unique_filename();
	std::vector<fs::path> to;
	const std::string archiveStr = fs::u8string(archiveFile);
	const std::string unpackStr = fs::u8string(unpackDir);
	const std::string destStr = fs::u8string(destDir);
	const std::string processedStr = fs::u8string(g_Options->GetNzbDirPath() / PROCESSED_DIR);
	const std::string brokenStr = fs::u8string(g_Options->GetNzbDirPath() / BROKEN_DIR);

	if (!nzbget_rs_archive_processor_process(
		archiveStr.c_str(),
		unpackStr.c_str(),
		destStr.c_str(),
		processedStr.c_str(),
		brokenStr.c_str(),
		(int)g_Options->GetNzbDirArchiveAction(),
		&to,
		[](void* /*ctx*/, int kind, const char* msg) {
			const char* text = msg ? msg : "";
			if (kind == 1)
			{
				warn("%s", text);
			}
			else if (kind == 2)
			{
				error("%s", text);
			}
			else if (kind == 3)
			{
				detail("%s", text);
			}
			else
			{
				info("%s", text);
			}
		},
		[](void* rawCtx, const char* movedPath) {
			auto* out = static_cast<std::vector<fs::path>*>(rawCtx);
			if (movedPath)
			{
				out->push_back(fs::u8path(movedPath));
			}
		}))
	{
		return std::nullopt;
	}

	return to;
}

inline void Scanner::UnpackArchives(const std::vector<fs::path>& archives)
{
	if (archives.empty()) return;

	for (const auto& archive : archives)
	{
		ProcessArchive(archive, archive.parent_path());
	}
}

inline void Scanner::CleanupStaleUnpackDir()
{
	const fs::path unpackDir = g_Options->GetTempDirPath() / UNPACK_DIR;
	fs::error_code ec;
	fs::remove_all(unpackDir, ec);
	if (ec)
	{
		warn("Could not remove stale unpack directory '%s/%s': %s (code %d)", 
			g_Options->GetTempDir(), UNPACK_DIR.data(), ec.message().c_str(), ec.value());	
	}
}

/**
* Check if there are files in directory for incoming nzb-files
* and add them to download queue
*/
inline void Scanner::CheckIncomingNzbs(const char* directory, const char* category, bool checkStat)
{
	DirBrowser dir(directory);
	while (const char* filename = dir.Next())
	{
		if (ShouldSkipItem(filename))
		{
			continue;
		}

		BString<1024> fullfilename("%s%c%s", directory, PATH_SEPARATOR, filename);
		bool isDirectory = FileSystem::DirectoryExists(fullfilename);

		if (isDirectory)
		{
			ProcessSubdirectory(fullfilename, filename, category, checkStat);
		}
		else if (!isDirectory && CanProcessFile(fullfilename, checkStat))
		{
			ProcessIncomingFile(directory, filename, fullfilename, category);
		}
	}
}

inline void Scanner::ProcessSubdirectory(const char* fullPath, const char* filename, const char* category, bool checkStat)
{
	BString<1024> subCategory;
	if (category && category[0] != '\0')
	{
		subCategory.Format("%s%c%s", category, PATH_SEPARATOR, filename);
	}
	else
	{
		subCategory = filename;
	}
	CheckIncomingNzbs(fullPath, subCategory, checkStat);
}

/**
 * Only files which were not changed during last g_pOptions->GetNzbDirFileAge() seconds
 * can be processed. That prevents the processing of files, which are currently being
 * copied into nzb-directory (eg. being downloaded in web-browser).
 */
inline bool Scanner::CanProcessFile(const char* fullFilename, bool checkStat)
{
	if (!nzbget_rs_dsp_can_process_extension(fullFilename))
	{
		return false;
	}

	if (!checkStat)
	{
		return true;
	}

	int64 size = FileSystem::FileSize(fullFilename);
	time_t current = Util::CurrentTime();
	bool canProcess = false;
	bool inList = false;

	for (FileList::iterator it = m_fileList.begin(); it != m_fileList.end(); ++it)
	{
		FileData& fileData = *it;
		if (!strcmp(fileData.GetFilename(), fullFilename))
		{
			inList = true;
			int64 updatedSize = fileData.GetSize();
			int64 updatedLastChange = (int64)fileData.GetLastChange();
			if (nzbget_rs_dsp_eval_file_age(
				fileData.GetSize(), size, (int64)fileData.GetLastChange(), (int64)current,
				g_Options->GetNzbDirFileAge(), &updatedSize, &updatedLastChange))
			{
				canProcess = true;
				m_fileList.erase(it);
			}
			else
			{
				fileData.SetSize(updatedSize);
				fileData.SetLastChange((time_t)updatedLastChange);
			}
			break;
		}
	}

	if (!inList)
	{
		m_fileList.emplace_back(fullFilename, size, current);
	}

	return canProcess;
}

/**
 * Remove old files from the list of monitored files.
 * Normally these files are deleted from the list when they are processed.
 * However if a file was detected by function "CanProcessFile" once but wasn't
 * processed later (for example if the user deleted it), it will stay in the list,
 * until we remove it here.
 */
inline void Scanner::DropOldFiles()
{
	time_t current = Util::CurrentTime();

	m_fileList.erase(std::remove_if(m_fileList.begin(), m_fileList.end(),
		[current](FileData& fileData)
		{
			if (nzbget_rs_dsp_should_drop_scan_file(
				(int64)fileData.GetLastChange(), (int64)current,
				g_Options->GetNzbDirInterval(), g_Options->GetNzbDirFileAge()))
			{
				debug("Removing file %s from scan file list", fileData.GetFilename());
				return true;
			}
			return false;
		}),
		m_fileList.end());
}

inline bool Scanner::ShouldSkipItem(std::string_view filename)
{
	std::string nameStr(filename);
	std::string procStr(PROCESSED_DIR);
	std::string brokStr(BROKEN_DIR);
	return nzbget_rs_scanner_should_skip_item(
		nameStr.c_str(), procStr.c_str(), brokStr.c_str()) != 0;
}

inline void Scanner::ProcessIncomingFile(
	const char* directory,
	const char* baseFilename,
	const char* fullFilename,
	const char* category
)
{
	const char* extension = strrchr(baseFilename, '.');
	if (!extension)
	{
		return;
	}

	std::string nzbName;
	std::string nzbCategory = category ? category : "";
	std::string dupeKey;
	NzbParameterList parameters;
	int priority = 0;
	bool addTop = false;
	bool addPaused = false;
	bool autoCategory = nzbCategory.empty();
	int dupeScore = 0;
	EDupeMode dupeMode = dmScore;
	EAddStatus addStatus = asSkipped;
	QueueData* queueData = nullptr;
	NzbInfo* nzbInfo = nullptr;
	int nzbId = 0;

	for (QueueData& queueData1 : m_queueList)
	{
		if (FileSystem::SameFilename(queueData1.GetFilename(), fullFilename))
		{
			queueData = &queueData1;
			nzbName = queueData->GetNzbName();
			nzbCategory = queueData->GetCategory();
			priority = queueData->GetPriority();
			dupeKey = queueData->GetDupeKey();
			dupeScore = queueData->GetDupeScore();
			dupeMode = queueData->GetDupeMode();
			addTop = queueData->GetAddTop();
			addPaused = queueData->GetAddPaused();
			parameters.CopyFrom(queueData->GetParameters());
			nzbInfo = queueData->GetUrlInfo();
			autoCategory = queueData->GetAutoCategory();
		}
	}

	InitPPParameters(nzbCategory.c_str(), &parameters, false);

	bool exists = true;

	bool ranScanScript = m_scanScript && strcasecmp(extension, ".nzb_processed");
	if (ranScanScript)
	{
		ScanScriptController::ExecuteScripts(fullFilename,
			nzbInfo,
			directory,
			nzbName,
			nzbCategory,
			&priority,
			&parameters,
			&addTop,
			&addPaused,
			dupeKey,
			&dupeScore,
			&dupeMode
		);
		exists = FileSystem::FileExists(fullFilename);
	}
	int renameToProcessed = 0;
	int incAction = nzbget_rs_scanner_classify_incoming_extension(
		ranScanScript ? 1 : 0, exists ? 1 : 0, extension, &renameToProcessed);
	if (renameToProcessed)
	{
		CString bakname2;
		bool renameOK = FileSystem::RenameBak(fullFilename, "processed", false, bakname2);
		if (!renameOK)
		{
			error("Could not rename file %s to %s: %s", fullFilename, *bakname2,
				*FileSystem::GetLastErrorMessage());
		}
	}
	if (incAction == 1)
	{
		CString renamedName;
		bool renameOK = FileSystem::RenameBak(fullFilename, "nzb", true, renamedName);
		if (renameOK)
		{
			bool added = AddFileToQueue(
				renamedName,
				nzbName.c_str(),
				nzbCategory.c_str(),
				autoCategory,
				priority,
				dupeKey.c_str(),
				dupeScore,
				dupeMode,
				&parameters,
				addTop,
				addPaused,
				nzbInfo,
				&nzbId
			);
			addStatus = added ? asSuccess : asFailed;
		}
		else
		{
			error("Could not rename file %s to %s: %s", fullFilename, *renamedName,
				*FileSystem::GetLastErrorMessage());
			addStatus = asFailed;
		}
	}
	else if (incAction == 2)
	{
		bool added = AddFileToQueue(
			fullFilename,
			nzbName.c_str(),
			nzbCategory.c_str(),
			autoCategory,
			priority,
			dupeKey.c_str(),
			dupeScore,
			dupeMode,
			&parameters,
			addTop,
			addPaused,
			nzbInfo,
			&nzbId
		);
		addStatus = added ? asSuccess : asFailed;
	}

	if (queueData)
	{
		queueData->SetAddStatus(addStatus);
		queueData->SetNzbId(nzbId);
	}
}

inline void Scanner::InitPPParameters(const char* category, NzbParameterList* parameters, bool reset)
{
	bool unpack = g_Options->GetUnpack();
	const char* extensions = g_Options->GetExtensions();

	if (!Util::EmptyStr(category))
	{
		Options::Category* categoryObj = g_Options->FindCategory(category, false);
		if (categoryObj)
		{
			unpack = categoryObj->GetUnpack();
			if (!Util::EmptyStr(categoryObj->GetExtensions()))
			{
				extensions = categoryObj->GetExtensions();
			}
		}
	}

	if (reset)
	{
		g_ExtensionManager->ForEach([&parameters](auto script)
			{
				BString<1024> paramName("%s:", script->GetName());
				parameters->SetParameter(paramName, nullptr);
			}
		);
	}

	if (!parameters->Find("*Unpack:"))
	{
		parameters->SetParameter("*Unpack:", unpack ? "yes" : "no");
	}

	if (!Util::EmptyStr(extensions))
	{
		// create pp-parameter for each post-processing or queue- script
		g_ExtensionManager->ForEach(
			[&](auto script)
			{
				Tokenizer tok(extensions, ",;");
				while (const char* scriptName = tok.Next())
				{
					if (strcmp(scriptName, script->GetName()) != 0) continue;

					BString<1024> paramName("%s:", scriptName);
					if ((script->GetPostScript() || script->GetQueueScript()) && !parameters->Find(paramName))
					{
						parameters->SetParameter(paramName, "yes");
					}
				}
			});
	}
}

inline CString Scanner::ResolveCategory(const char* category, const char* filename)
{
	CString useCategory = category ? category : "";
	Options::Category* categoryObj = g_Options->FindCategory(useCategory, true);
	if (categoryObj && strcmp(useCategory, categoryObj->GetName()) != 0)
	{
		useCategory = categoryObj->GetName();
		detail("Category '%s' matched to '%s' for '%s'", category, *useCategory, filename);
	}
	return useCategory;
}

inline void Scanner::DetectAndSetCategory(const NzbFile& nzbFile, NzbInfo& nzbInfo, const char* nzbName)
{
	const std::string& nzbCategory = nzbFile.GetCategoryFromFile();
	if (nzbCategory.empty())
	{
		detail("Auto-category: no category detected in '%s'", nzbName);
		return;
	}
	Options::Category* categoryObj = g_Options->FindCategory(nzbCategory.c_str(), true);
	if (categoryObj)
	{
		const char* category = categoryObj->GetName();
		detail("Category '%s' matched to '%s' for '%s'", category, nzbCategory.c_str(), nzbName);
		nzbInfo.SetCategory(category);
	}
	else
	{
		detail("Auto-category: using detected category '%s' for '%s'", nzbCategory.c_str(), nzbName);
		nzbInfo.SetCategory(nzbCategory.c_str());
	}
}

inline bool Scanner::AddFileToQueue(
	const char* filename,
	const char* nzbName,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	int* nzbId
)
{
	const char* basename = FileSystem::BaseFileName(filename);

	info("Adding collection %s to queue", basename);

	NzbFile nzbFile(filename, category);
	bool ok = nzbFile.Parse();
	if (!ok)
	{
		error("Could not add collection %s to queue", basename);
	}

	CString bakname2;
	const char* bakSuffix = ok ? "queued" : "error";
	if (!FileSystem::RenameBak(filename, bakSuffix, false, bakname2))
	{
		ok = false;
		error("Could not rename file %s to %s: %s", filename, *bakname2,
			*FileSystem::GetLastErrorMessage());
	}

	std::unique_ptr<NzbInfo> nzbInfo = nzbFile.DetachNzbInfo();
	nzbInfo->SetQueuedFilename(bakname2);

	if (autoCategory)
	{
		DetectAndSetCategory(nzbFile, *nzbInfo, nzbName);
		InitPPParameters(nzbInfo->GetCategory(), parameters, true);
	}

	if (!Util::EmptyStr(nzbName))
	{
		nzbInfo->SetName(nullptr);
		nzbInfo->SetFilename(nzbName);
		nzbInfo->BuildDestDirName();
	}

	if (ok)
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			// a warning when the nzb stops short of what the subject declares or
			// leaves out much of a file (a fragment listing 1 of 9374 parts); a
			// segment or two an indexer dropped is common and only a detail
			if (fileInfo->GetMissedArticles() > 0)
			{
				bool cutShort = nzbFile.GetCutShortFiles().count(fileInfo) > 0;
				int sev = nzbget_rs_dsp_classify_missed_articles(
					fileInfo->GetMissedArticles(), fileInfo->GetTotalArticles(), cutShort ? 1 : 0);
				int presentArticles = fileInfo->GetTotalArticles() - fileInfo->GetMissedArticles();
				nzbInfo->PrintMessage(
					sev == 2 ? Message::mkWarning : Message::mkDetail,
					"Collection %s lists only %i of the %i articles of %s: the missing ones count as failed",
					nzbInfo->GetName(), presentArticles,
					fileInfo->GetTotalArticles(), fileInfo->GetFilename());
			}
		}
	}

	nzbInfo->SetDupeKey(dupeKey);
	nzbInfo->SetDupeScore(dupeScore);
	nzbInfo->SetDupeMode(dupeMode);
	nzbInfo->SetPriority(priority);
	if (urlInfo)
	{
		nzbInfo->SetUrl(urlInfo->GetUrl());
		nzbInfo->SetUrlStatus(urlInfo->GetUrlStatus());
		nzbInfo->SetFeedId(urlInfo->GetFeedId());
		nzbInfo->SetDupeHint(urlInfo->GetDupeHint());
		nzbInfo->SetDesiredServerId(urlInfo->GetDesiredServerId());
		nzbInfo->SetSkipScriptProcessing(urlInfo->GetSkipScriptProcessing());
		nzbInfo->SetSkipDiskWrite(urlInfo->GetSkipDiskWrite());
	}

	if (!nzbFile.GetPassword().empty())
	{
		nzbInfo->GetParameters()->SetParameter("*Unpack:Password", nzbFile.GetPassword().c_str());
	}

	nzbInfo->GetParameters()->CopyFrom(parameters);

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		fileInfo->SetPaused(addPaused);
	}

	NzbInfo* addedNzb = nullptr;

	if (ok)
	{
		addedNzb = g_QueueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), std::move(urlInfo), addTop);
	}
	else if (urlInfo)
	{
		for (Message& message : nzbInfo->GuardCachedMessages())
		{
			urlInfo->AddMessage(message.GetKind(), message.GetText(), false);
		}
	}
	else
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsScan);
		addedNzb = g_QueueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), std::move(urlInfo), addTop);
	}

	if (nzbId)
	{
		*nzbId = addedNzb ? addedNzb->GetId() : 0;
	}

	return ok;
}

inline void Scanner::ScanNzbDir(bool syncMode)
{
	{
		std::lock_guard<std::mutex> guard{m_scanMutex};
		m_scanning = true;
		m_requestedNzbDirScan = true;
		WakeUp();
	}

	while (syncMode && (m_scanning || m_requestedNzbDirScan))
	{
		Util::Sleep(100);
	}
}

inline Scanner::EAddStatus Scanner::AddArchive(const char* filename, const char* category,
										bool autoCategory, int priority, const char* dupeKey,
										int dupeScore, EDupeMode dupeMode,
										NzbParameterList* parameters, bool addTop, bool addPaused,
										NzbInfo* urlInfo, const char* buffer, int bufSize)
{
	if (Util::EmptyStr(filename))
	{
		error("Failed to add the archive to the download queue: Archive filename cannot be empty");
		return EAddStatus::asFailed;
	}

	if (g_Options->GetTempDirPath().empty())
	{
		error("Failed to create file '%s': '%s' is required and cannot be empty",
			filename, Options::TEMPDIR.data());
		return EAddStatus::asFailed;
	}

	if (g_Options->GetNzbDirPath().empty())
	{
		error("Failed to create file '%s': '%s' is required and cannot be empty",
			filename, Options::NZBDIR.data());
		return EAddStatus::asFailed;
	}

	const auto archiveFilename = FileSystem::MakeValidFilename(filename);
	const auto archiveFilePath = g_Options->GetTempDirPath() / *archiveFilename;

	fs::error_code ec;
	{
		std::ofstream file(archiveFilePath.c_str(), std::ios::binary);
		if (!file.is_open())
		{
			error("Failed to create archive file '%s' in temporary directory: %s",
				*archiveFilename, std::strerror(errno));
			return EAddStatus::asFailed;
		}

		file.write(buffer, bufSize);
		file.close();
		if (!file)
		{
			error("Failed to write data to archive file '%s': %s",
				*archiveFilename, std::strerror(errno));
			fs::remove(archiveFilePath, ec);
			return EAddStatus::asFailed;
		}
	}

	if (!g_Options->GetNzbDirArchiveScan()) 
	{
		const auto finalArchiveFilePath = g_Options->GetNzbDirPath() / *archiveFilename;
		fs::move_file(archiveFilePath, finalArchiveFilePath, ec);
		if (ec)
		{
			error("Failed to move file '%s' to %s: %s",
				*archiveFilename, Options::NZBDIR.data(), ec.message().c_str());
			return EAddStatus::asFailed;
		}

		info("Skipping processing of '%s': option '%s' is disabled",
			*archiveFilename, Options::NZBDIRARCHIVESCAN.data());
		return EAddStatus::asSkipped;
	}

	std::vector<EAddStatus> statuses;
	{
		std::lock_guard<std::mutex> guard{m_scanMutex};

		auto nzbFiles = ProcessArchive(archiveFilePath, g_Options->GetNzbDirPath());
		if (!nzbFiles)
		{
			return EAddStatus::asFailed;
		}

		if (nzbFiles->empty())
		{
			return EAddStatus::asSkipped;
		}

		// the url item goes with the first nzb only: adding it takes the item
		// out of the queue and frees it, and the next nzb of the archive read
		// the freed item (and could take another queue entry out with it).
		// Each nzb reports how it went (sized up front: the entries point here)
		statuses.assign(nzbFiles->size(), EAddStatus::asSkipped);
		size_t index = 0;
		for (const auto& nzbFile : *nzbFiles)
		{
			const auto nzbFilePathStr = fs::u8string(nzbFile);
			const auto fileName = fs::u8string(nzbFile.filename());
			const CString useCategory = ResolveCategory(category, fileName.c_str());
			m_queueList.emplace_back(
				nzbFilePathStr.c_str(), fileName.c_str(), useCategory,
				autoCategory, priority, dupeKey, dupeScore, dupeMode,
				parameters, addTop, addPaused,
				index == 0 ? urlInfo : nullptr,
				&statuses[index], nullptr);
			index++;
		}
		// requested under the lock, as in AddExternalFile: a periodic scan
		// waiting for the lock skipped the young files, cleared their entries
		// and they were added later without category, key, score or url (F14)
		m_requestedNzbDirScan = true;
	}

	ScanNzbDir(true);

	std::vector<int> statusInts;
	statusInts.reserve(statuses.size());
	for (EAddStatus st : statuses)
	{
		statusInts.push_back((int)st);
	}
	return (EAddStatus)nzbget_rs_scanner_resolve_archive_add_status(
		urlInfo ? 1 : 0,
		statusInts.data(),
		statusInts.size(),
		(int)EAddStatus::asSuccess,
		(int)EAddStatus::asSkipped);
}

inline Scanner::EAddStatus Scanner::AddArchive(
	const char* filename,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	const char* tmpFilename)
{
	CharBuffer buffer;
	if (!FileSystem::LoadFileIntoBuffer(tmpFilename, buffer, false))
	{
		error("Could not open file '%s'", tmpFilename);
		return EAddStatus::asFailed;
	}
	fs::error_code ec;
	fs::remove(tmpFilename, ec);
	if (ec)
	{
		const auto msg = ec.message();
		error("Failed to remove '%s': %s (code %d)", tmpFilename, msg.c_str(), ec.value());
	}

	return AddArchive(
		filename,
		category,
		autoCategory,
		priority,
		dupeKey,
		dupeScore,
		dupeMode,
		parameters,
		addTop,
		addPaused,
		urlInfo,
		*buffer,
		buffer.Size()
	);
}

inline Scanner::EAddStatus Scanner::AddExternalFile(
	const char* nzbName,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	const char* fileName,
	const char* buffer,
	int bufSize,
	int* nzbId
)
{
	bool nzb = false;
	BString<1024> tempFileName;

	// the names are picked and taken under the lock: two appends at once picked the
	// same temporary or queue file name, one overwrote the other and returned -1 (F14)
	std::unique_lock<std::mutex> guard{m_scanMutex};

	if (fileName)
	{
		tempFileName = fileName;
	}
	else
	{
		int num = ++m_idGen;
		while (tempFileName.Empty() || FileSystem::FileExists(tempFileName))
		{
			tempFileName.Format("%s%cnzb-%i.tmp", g_Options->GetTempDir(), PATH_SEPARATOR, num);
			num++;
		}

		if (!FileSystem::SaveBufferIntoFile(tempFileName, buffer, bufSize))
		{
			error("Could not create file %s", *tempFileName);
			return asFailed;
		}

		nzb = nzbget_rs_dsp_is_nzb_xml_prefix((const uint8_t*)buffer, (size_t)std::max(0, bufSize)) != 0;
	}

	// move file into NzbDir, make sure the file name is unique
	// a name that is a full path (a client passing its file) drops the folder; any
	// other "/" belongs to the name (indexers post "[hash] [16/22] - Title"), and it
	// lost everything up to it ("22] - Title"). MakeValidFilename makes it safe
	bool absolutePath = nzbget_rs_scanner_is_absolute_nzb_name(nzbName) != 0;
	CString validNzbName = FileSystem::MakeValidFilename(absolutePath ? FileSystem::BaseFileName(nzbName) : nzbName);
	{
		NzbgetRsBuf normBuf = nzbget_rs_dsp_normalize_external_nzb_name(nzbName, validNzbName, nzb ? 1 : 0);
		std::string normStr(normBuf.data ? normBuf.data : "", normBuf.len);
		nzbget_rs_free(normBuf);
		validNzbName = normStr.c_str();
	}

	BString<1024> scanFileName("%s%c%s", g_Options->GetNzbDir(), PATH_SEPARATOR, *validNzbName);

	const char* validCStr = validNzbName ? *validNzbName : "";
	size_t validLen = strlen(validCStr);
	ptrdiff_t dotOff = nzbget_rs_file_extension(validCStr, validLen);
	bool hasExt = dotOff >= 0;
	std::string basePart(validCStr, hasExt ? (size_t)dotOff : validLen);
	std::string extPart = hasExt ? std::string(validCStr + dotOff + 1) : std::string();

	int num = 2;
	while (FileSystem::FileExists(scanFileName))
	{
		if (hasExt)
		{
			scanFileName.Format("%s%c%s_%i.%s", g_Options->GetNzbDir(), PATH_SEPARATOR,
				basePart.c_str(), num, extPart.c_str());
		}
		else
		{
			scanFileName.Format("%s%c%s_%i", g_Options->GetNzbDir(), PATH_SEPARATOR,
				basePart.c_str(), num);
		}
		num++;
	}

	EAddStatus addStatus;

	{
		if (!FileSystem::MoveFile(tempFileName, scanFileName))
		{
			error("Could not move file %s to %s: %s", *tempFileName, *scanFileName,
				*FileSystem::GetLastErrorMessage());
			FileSystem::DeleteFile(tempFileName);
			return asFailed;
		}

		CString useCategory = ResolveCategory(category, nzbName);

		addStatus = asSkipped;
		m_queueList.emplace_back(
			scanFileName,
			nzbName,
			useCategory,
			autoCategory,
			priority,
			dupeKey,
			dupeScore,
			dupeMode,
			parameters,
			addTop,
			addPaused,
			urlInfo,
			&addStatus,
			nzbId
		);
		// requested under the lock: a periodic scan that took the lock first skipped
		// the file (too young) and cleared its entry, and the file was added later
		// without the name, key and score it was sent with (F14)
		m_requestedNzbDirScan = true;
	}
	guard.unlock();

	ScanNzbDir(true);

	return addStatus;
}

#
inline void UrlDownloader::ProcessHeader(const char* line)
{
	WebDownloader::ProcessHeader(line);

	size_t start, length;
	NzbgetRsBuf name{}, value{};
	switch (nzbget_rs_url_dnzb_header(line, &start, &length, &name, &value))
	{
		case 1:
			m_category = length > 0 ? CString(line + start, (int)length) : CString("");
			debug("Category: %s", *m_category);
			break;

		case 2:
			debug("X-DNZB: %s", name.data);
			debug("Value: %s", value.data);
			m_nzbInfo->GetParameters()->SetParameter(name.data, value.data);
			nzbget_rs_free(name);
			nzbget_rs_free(value);
			break;
	}
}

inline UrlCoordinator::UrlCoordinator()
{
	m_downloadQueueObserver.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);
}

inline UrlCoordinator::~UrlCoordinator()
{
	debug("Destroying UrlCoordinator");

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		delete urlDownloader;
	}
	m_activeDownloads.clear();

	debug("UrlCoordinator destroyed");
}

inline void UrlCoordinator::Run()
{
	debug("Entering UrlCoordinator-loop");

	while (!DownloadQueue::IsLoaded())
	{
		Util::Sleep(20);
	}

	time_t lastReset = 0;
	while (!IsStopped())
	{
		bool downloadStarted = false;

		{
			NzbInfo* nzbInfo = nullptr;
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			if ((int)m_activeDownloads.size() < g_Options->GetUrlConnections())
			{
				nzbInfo = GetNextUrl(downloadQueue);
				if (nzbInfo && (!g_WorkState->GetPauseDownload() || g_Options->GetUrlForce()))
				{
					StartUrlDownload(nzbInfo);
					downloadStarted = true;
				}
			}
			m_hasMoreJobs = !m_activeDownloads.empty() || nzbInfo != nullptr;
		}

		if (lastReset != Util::CurrentTime())
		{
			ResetHangingDownloads();
			lastReset = Util::CurrentTime();
		}

		if (!m_hasMoreJobs && !IsStopped())
		{
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait(lk, [&] { return m_hasMoreJobs || IsStopped(); });
		}
		else
		{
			Util::Sleep(downloadStarted ? 0 : 100);
		}
	}

	WaitJobs();

	debug("Exiting UrlCoordinator-loop");
}

inline void UrlCoordinator::WaitJobs()
{
	debug("UrlCoordinator: waiting for Downloads to complete");

	while (true)
	{
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			if (m_activeDownloads.empty())
			{
				break;
			}
		}
		Util::Sleep(100);
		ResetHangingDownloads();
	}

	debug("UrlCoordinator: Downloads are completed");
}

inline void UrlCoordinator::Stop()
{
	Thread::Stop();

	debug("Stopping UrlDownloads");
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		for (UrlDownloader* urlDownloader : m_activeDownloads)
		{
			urlDownloader->Stop();
		}
	}
	debug("UrlDownloads are notified");

	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

inline void UrlCoordinator::DownloadQueueUpdate(Subject*, void* aspect)
{
	debug("Notification from download queue received");

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaUrlAdded ||
		queueAspect->action == DownloadQueue::eaUrlReturned)
	{
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_hasMoreJobs = true;
		m_waitCond.notify_all();
	}
}

inline void UrlCoordinator::ResetHangingDownloads()
{
	if (g_Options->GetUrlTimeout() == 0)
	{
		return;
	}

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	time_t tm = Util::CurrentTime();

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		if (nzbget_rs_url_coord_is_hanging_download(
			static_cast<int64>(tm),
			static_cast<int64>(urlDownloader->GetLastUpdateTime()),
			g_Options->GetUrlTimeout(),
			static_cast<int>(urlDownloader->GetStatus()),
			static_cast<int>(UrlDownloader::adRunning)))
		{
			error("Cancelling hanging url download %s", urlDownloader->GetInfoName());
			urlDownloader->Stop();
		}
	}
}

inline void UrlCoordinator::LogDebugInfo()
{
	info("   ---------- UrlCoordinator");

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	info("    Active Downloads: %i", (int)m_activeDownloads.size());
	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		urlDownloader->LogDebugInfo();
	}
}

inline NzbInfo* UrlCoordinator::GetNextUrl(DownloadQueue* downloadQueue)
{
	size_t n = downloadQueue->GetQueue()->size();
	if (n == 0)
	{
		return nullptr;
	}
	std::vector<NzbInfo*> items;
	std::vector<int> kinds;
	std::vector<int> urlStatuses;
	std::vector<int> deleteStatuses;
	std::vector<int> priorities;
	items.reserve(n);
	kinds.reserve(n);
	urlStatuses.reserve(n);
	deleteStatuses.reserve(n);
	priorities.reserve(n);
	for (NzbInfo* nzbInfo1 : downloadQueue->GetQueue())
	{
		items.push_back(nzbInfo1);
		kinds.push_back(static_cast<int>(nzbInfo1->GetKind()));
		urlStatuses.push_back(static_cast<int>(nzbInfo1->GetUrlStatus()));
		deleteStatuses.push_back(static_cast<int>(nzbInfo1->GetDeleteStatus()));
		priorities.push_back(nzbInfo1->GetPriority());
	}
	int idx = nzbget_rs_url_coord_select_next_url(
		kinds.data(),
		urlStatuses.data(),
		deleteStatuses.data(),
		priorities.data(),
		n,
		static_cast<int>(NzbInfo::nkUrl),
		static_cast<int>(NzbInfo::lsNone),
		static_cast<int>(NzbInfo::dsNone));
	return idx >= 0 && static_cast<size_t>(idx) < items.size() ? items[idx] : nullptr;
}

inline void UrlCoordinator::StartUrlDownload(NzbInfo* nzbInfo)
{
	debug("Starting new UrlDownloader");

	UrlDownloader* urlDownloader = new UrlDownloader();
	urlDownloader->SetAutoDestroy(true);
	urlDownloader->Attach(this);
	urlDownloader->SetNzbInfo(nzbInfo);
	urlDownloader->SetUrl(nzbInfo->GetUrl());
	urlDownloader->SetForce(g_Options->GetUrlForce());
	urlDownloader->SetInfoName(nzbInfo->MakeNiceUrlName(nzbInfo->GetUrl(), nzbInfo->GetFilename()));
	NzbgetRsBuf outPathBuf = nzbget_rs_url_coord_format_temp_path(
		g_Options->GetTempDir(), PATH_SEPARATOR, nzbInfo->GetId());
	urlDownloader->SetOutputFilename(outPathBuf.data ? outPathBuf.data : "");
	nzbget_rs_free(outPathBuf);

	nzbInfo->SetActiveDownloads(1);
	nzbInfo->SetUrlStatus(NzbInfo::lsRunning);

	m_activeDownloads.push_back(urlDownloader);
	urlDownloader->Start();
}

inline void UrlCoordinator::Update(Subject* caller, void*)
{
	debug("Notification from UrlDownloader received");

	UrlDownloader* urlDownloader = (UrlDownloader*)caller;
	if (urlDownloader->GetStatus() == WebDownloader::adFinished ||
		urlDownloader->GetStatus() == WebDownloader::adFailed ||
		urlDownloader->GetStatus() == WebDownloader::adRetry)
	{
		UrlCompleted(urlDownloader);
	}
}

inline void UrlCoordinator::UrlCompleted(UrlDownloader* urlDownloader)
{
	debug("URL downloaded");

	NzbInfo* nzbInfo = urlDownloader->GetNzbInfo();

	const char* origname;
	if (urlDownloader->GetOriginalFilename())
	{
		origname = urlDownloader->GetOriginalFilename();
	}
	else
	{
		origname = FileSystem::BaseFileName(nzbInfo->GetUrl());
	}

	CString filename = FileSystem::MakeValidFilename(origname);

	debug("Filename: [%s]", *filename);

	bool retry;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		m_activeDownloads.erase(std::find(m_activeDownloads.begin(), m_activeDownloads.end(), urlDownloader));

		int urlStatus = nzbInfo->GetUrlStatus();
		int deleteStatus = nzbInfo->GetDeleteStatus();
		int again = 0;
		nzbget_rs_url_completed(urlDownloader->GetStatus(), nzbInfo->GetDeleting(), &urlStatus, &deleteStatus, &again);
		retry = again != 0;
		if (nzbInfo->GetDeleting())
		{
			nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)deleteStatus);
			nzbInfo->SetDeleting(false);
		}
		nzbInfo->SetUrlStatus((NzbInfo::EUrlStatus)urlStatus);

		if (!retry)
		{
			DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlCompleted, downloadQueue, nzbInfo, nullptr};
			downloadQueue->Notify(&aspect);
		}
	}

	if (retry)
	{
		nzbInfo->SetActiveDownloads(0);
		return;
	}

	if (nzbInfo->GetUrlStatus() == NzbInfo::lsFinished)
	{
		const char* name = !Util::EmptyStr(nzbInfo->GetFilename())
			? nzbInfo->GetFilename() : *filename;
		const char* category = !Util::EmptyStr(nzbInfo->GetCategory())
			? nzbInfo->GetCategory() : urlDownloader->GetCategory();

		Scanner::EAddStatus addStatus;
		if (nzbget_rs_unpack_can_extract_with_zipfast(name) != 0)
		{
			addStatus = g_Scanner->AddArchive(
				name,
				category,
				nzbInfo->GetAutoCategory(),
				nzbInfo->GetPriority(),
				nzbInfo->GetDupeKey(),
				nzbInfo->GetDupeScore(),
				nzbInfo->GetDupeMode(),
				nzbInfo->GetParameters(),
				false,
				nzbInfo->GetAddUrlPaused(),
				nzbInfo,
				urlDownloader->GetOutputFilename()
			);
		}
		else 
		{
			addStatus = g_Scanner->AddExternalFile(
				name,
				category,
				nzbInfo->GetAutoCategory(),
				nzbInfo->GetPriority(),
				nzbInfo->GetDupeKey(),
				nzbInfo->GetDupeScore(),
				nzbInfo->GetDupeMode(),
				nzbInfo->GetParameters(),
				false,
				nzbInfo->GetAddUrlPaused(),
				nzbInfo,
				urlDownloader->GetOutputFilename(),
				nullptr,
				0,
				nullptr
			);
		}

		if (addStatus == Scanner::asSuccess)
		{
			return;
		}

		nzbInfo->SetUrlStatus(addStatus == Scanner::asFailed
			? NzbInfo::lsScanFailed : NzbInfo::lsScanSkipped);
	}

	g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeUrlCompleted);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		nzbInfo->SetActiveDownloads(0);

		DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlFailed, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);
	}
}

inline bool UrlCoordinator::DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory)
{
	if (nzbInfo->GetActiveDownloads() > 0)
	{
		info("Deleting active URL %s", nzbInfo->GetName());
		nzbInfo->SetDeleting(true);
		nzbInfo->SetAvoidHistory(avoidHistory);

		for (UrlDownloader* urlDownloader : m_activeDownloads)
		{
			if (urlDownloader->GetNzbInfo() == nzbInfo)
			{
				urlDownloader->Stop();
				return true;
			}
		}

		return false;
	}

	info("Deleting URL %s", nzbInfo->GetName());

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
	}
	nzbInfo->SetUrlStatus(NzbInfo::lsNone);

	DownloadQueue::Aspect deletedAspect = {DownloadQueue::eaUrlDeleted, downloadQueue, nzbInfo, nullptr};
	downloadQueue->Notify(&deletedAspect);

	return true;
}

inline void UrlCoordinator::AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst)
{
	debug("Adding URL to queue");

	NzbInfo* addedNzb = nzbInfo.get();

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	DownloadQueue::Aspect foundAspect = {DownloadQueue::eaUrlFound, downloadQueue, addedNzb, nullptr};
	downloadQueue->Notify(&foundAspect);

	if (addedNzb->GetDeleteStatus() != NzbInfo::dsManual)
	{
		downloadQueue->GetQueue()->Add(std::move(nzbInfo), addFirst);

		DownloadQueue::Aspect addedAspect = {DownloadQueue::eaUrlAdded, downloadQueue, addedNzb, nullptr};
		downloadQueue->Notify(&addedAspect);
	}

	downloadQueue->Save();
}


#endif /* SCANNER_H */

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef DUPESEARCH_H
#define DUPESEARCH_H
#include <condition_variable>
#include <atomic>
#include <ctime>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <set>
#include <string>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef DONORHEALTH_H
#define DONORHEALTH_H

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

/*
 * How much of a posting still exists on the news servers: a sample of its
 * articles is asked of ALL servers at once (STAT, and for a few articles also
 * BODY with the yEnc checksum verified, because a server sometimes says
 * "223" when the data is gone). The first server that really has an article
 * settles it; an article is missing only when every server said no, with at
 * least one definitive "430". Each server walks the sample at its own pace,
 * so a slow server never holds up the fast ones.
 *
 * Details that were real bugs on real servers:
 *  - A reply of 451 is an answer (some providers say it for a missing
 *    article): it keeps the connection and never pauses the server, but it
 *    isn't a definitive miss either.
 *  - A server with three transport errors in a row pauses and abstains
 *    meanwhile, so misses don't wait on it.
 *  - When the time runs out, an unsettled article counts as missing when at
 *    least half of the servers missed it and none had it; articles that exist
 *    settle at once, missing ones slowly, so dropping the unsettled ones
 *    made a 12% alive posting read 57%.
 *  - STATs are pipelined: 4 per round trip at first, doubling while a batch
 *    takes under a second (up to 16), halving above two.
 *  - Servers take turns downloading a body: the next server only if the
 *    previous one's data was bad.
 */
class DonorHealth
{
public:
	enum class Answer
	{
		None,
		Present,
		Missing,
		BodyBad,	// the server has the article but its data is bad or gone
		Error		// no usable answer: 451, an error reply, an outage
	};

	struct Health
	{
		int checked = 0;
		int present = 0;
		int missing = 0;
		int error = 0;
		int bodyChecked = 0;	// articles that also got a BODY check
		int bodyBad = 0;		// ... for which no server delivered valid data

		int Answered() const { return present + missing + error; }
		/* the present share of the answered articles; -1 when too few were
		 * answered to judge. Errors count as not present: a live article still
		 * gets a hit from another server, while a server that errors would
		 * otherwise hide every dead article. */
		double Alive() const;
		Health& operator+=(const Health& other);
	};

	// answered articles needed before a posting is judged
	static constexpr int MinKnown = 5;
	static constexpr int ServerGiveUp = 3;
	static constexpr int PipelineStart = 4;
	static constexpr int PipelineMax = 16;

	/* a probe proves a posting dead only with nothing found and enough
	 * definitive misses (an outage, where every answer is an error, never does) */
	static bool DeadProbe(const Health& health);

	struct Item
	{
		std::string messageId;
		bool body = false;
		std::shared_ptr<std::vector<std::string>> groups;
	};

	struct SampleOptions
	{
		int percent = 5;
		int minimum = 50;
		int maximum = 1000;
		int bodyPercent = 20;	// share of the sample that also gets a BODY check ...
		int maxBody = 20;		// ... at most this many
		unsigned seed = 0;
	};

	/* the articles to check: percent of the ids, at least minimum, at most
	 * maximum (deterministic), about bodyPercent of them marked for a body check */
	static std::vector<std::string> Sample(const std::vector<std::string>& ids, int percent, int minimum,
		int maximum, unsigned seed);
	static std::vector<Item> Plan(const std::vector<std::string>& ids,
		const std::shared_ptr<std::vector<std::string>>& groups, const SampleOptions& options);

	struct BodyGate
	{
		void* rustGate = nullptr;
		std::mutex lock;
		std::atomic<bool> ok{false};
		std::function<bool()> settled;
		bool Done();
	};

	struct Request
	{
		std::string messageId;
		BodyGate* gate = nullptr;	// set when the article also gets a BODY check
		std::shared_ptr<std::vector<std::string>> groups;
	};

	struct ExchangeError
	{
		std::vector<Answer> answers;	// the answers read before the connection broke (may be shorter)
		bool busy = false;	// no connection was free (nzbget's own downloads): not the server's fault
	};

	/*
	 * One news server as the check sees it: the pacing, pause and connection
	 * cap shared by every check that runs; the transport (the NNTP
	 * conversation) is a subclass's Exchange().
	 */
	class Server
	{
	public:
		explicit Server(int cap);
		virtual ~Server();

		int Cap() const;
		int Depth() const;
		bool Paused() const;
		int MaxActive() const;

		/* the answers for a batch of articles; waits for a connection slot. Every
		 * wait ends early, with Error answers and nothing asked, once <cancelled>
		 * says so (the check is over) or DonorHealth::StopAll ran */
		std::vector<Answer> Ask(const std::vector<Request>& batch,
			const std::function<bool()>& cancelled = nullptr);

		void SetRetryAfterMs(int ms);
		void* RustHandle() const { return m_rust; }

	protected:
		/* all STATs of the batch in one write, then their replies in order, then
		 * BODY for the articles that have a gate (see BodyOnce); throws ExchangeError
		 * when the connection fails */
		virtual std::vector<Answer> Exchange(const std::vector<Request>& batch) = 0;

		/* one server at a time downloads a gated article's body, and only until one delivered */
		static Answer BodyOnce(BodyGate* gate, const std::function<Answer()>& download);

	private:
		void* m_rust = nullptr;
		static int ExchangeTrampoline(
			void* cppServer, size_t count, const char* const* msgIds, const size_t* msgLens,
			void* const* rustGates, const char* const* const* groupPtrs,
			const size_t* const* groupLens, const size_t* groupCounts,
			int* outAnswers, size_t* outCount, int* outBusy);
	};

	using ServerList = std::vector<std::shared_ptr<Server>>;

	/* one posting's articles on all servers, within the budget (the time starts now) */
	static Health CheckItems(const ServerList& servers, const std::vector<Item>& items, int budgetMs);

	struct Options
	{
		SampleOptions sample;
		int probe = 10;			// articles checked first; a posting with none found is dead
		int budgetMs = 120000;	// for each posting, counted from when its check starts
		long long deadlineMs = 0;	// CheckPostings: no posting checked past it (NowMs; 0: none)
	};

	/* a posting: a probe first; unless that proves it dead (and full is set)
	 * the rest of the sample */
	static Health CheckPosting(const ServerList& servers, const std::vector<std::string>& ids,
		const std::shared_ptr<std::vector<std::string>>& groups, const Options& options, bool full);

	/* postings checked concurrently, each reported as its check finishes */
	struct Posting
	{
		std::string key;
		std::vector<std::string> ids;
		std::shared_ptr<std::vector<std::string>> groups;
	};
	static void CheckPostings(const ServerList& servers, const std::vector<Posting>& postings,
		const Options& options, bool full, int concurrency,
		const std::function<void(const std::string& key, const Health&)>& emit);

	static long long NowMs();

	/* shutdown: the walkers of every check stop asking and end; WaitAll returns
	 * when they did (before the server pool goes away). Reset allows checks again */
	static void StopAll();
	static void WaitAll();
	static void Reset();
	static bool Stopping();
};

#include <algorithm>
#include <exception>
#include "nzbget_rs.h"

inline long long DonorHealth::NowMs()
{
	return (long long)nzbget_rs_donor_now_ms();
}

inline double DonorHealth::Health::Alive() const
{
	return nzbget_rs_df_donor_health_alive(present, missing, error, MinKnown);
}

inline DonorHealth::Health& DonorHealth::Health::operator+=(const Health& other)
{
	nzbget_rs_dqh_donor_health_accum_health(
		&checked, &present, &missing, &error, &bodyChecked, &bodyBad,
		other.checked, other.present, other.missing, other.error, other.bodyChecked, other.bodyBad);
	return *this;
}

inline bool DonorHealth::DeadProbe(const Health& health)
{
	return nzbget_rs_df_donor_health_dead_probe(health.present, health.missing, MinKnown) != 0;
}

inline std::vector<std::string> DonorHealth::Sample(const std::vector<std::string>& ids, int percent, int minimum,
	int maximum, unsigned seed)
{
	std::vector<const char*> ptrs(ids.size());
	std::vector<size_t> lens(ids.size());
	for (size_t i = 0; i < ids.size(); i++)
	{
		ptrs[i] = ids[i].data();
		lens[i] = ids[i].size();
	}
	std::vector<std::string> out;
	nzbget_rs_donor_sample(
		ptrs.data(), lens.data(), ids.size(), percent, minimum, maximum, seed, &out,
		[](void* rawCtx, const char* idPtr, size_t idLen) {
			static_cast<std::vector<std::string>*>(rawCtx)->emplace_back(idPtr ? idPtr : "", idLen);
		});
	return out;
}

inline std::vector<DonorHealth::Item> DonorHealth::Plan(const std::vector<std::string>& ids,
	const std::shared_ptr<std::vector<std::string>>& groups, const SampleOptions& options)
{
	std::vector<const char*> ptrs(ids.size());
	std::vector<size_t> lens(ids.size());
	for (size_t i = 0; i < ids.size(); i++)
	{
		ptrs[i] = ids[i].data();
		lens[i] = ids[i].size();
	}
	struct PlanCtx
	{
		const std::shared_ptr<std::vector<std::string>>& groups;
		std::vector<Item> items;
	} ctx{ groups, {} };
	nzbget_rs_donor_plan(
		ptrs.data(), lens.data(), ids.size(),
		options.percent, options.minimum, options.maximum, options.bodyPercent, options.maxBody,
		options.seed, &ctx,
		[](void* rawCtx, const char* idPtr, size_t idLen, int body) {
			auto* c = static_cast<PlanCtx*>(rawCtx);
			Item item;
			item.messageId.assign(idPtr ? idPtr : "", idLen);
			item.groups = c->groups;
			item.body = body != 0;
			c->items.push_back(std::move(item));
		});
	return ctx.items;
}

inline bool DonorHealth::BodyGate::Done()
{
	if (rustGate)
	{
		return nzbget_rs_donor_body_gate_done(rustGate) != 0;
	}
	return ok || (settled && settled());
}

inline DonorHealth::Server::Server(int cap)
{
	m_rust = nzbget_rs_donor_server_new(this, cap, &Server::ExchangeTrampoline);
}

inline DonorHealth::Server::~Server()
{
	nzbget_rs_donor_server_free(m_rust);
}

inline int DonorHealth::Server::Cap() const { return nzbget_rs_donor_server_cap(m_rust); }
inline int DonorHealth::Server::Depth() const { return nzbget_rs_donor_server_depth(m_rust); }
inline bool DonorHealth::Server::Paused() const { return nzbget_rs_donor_server_paused(m_rust) != 0; }
inline int DonorHealth::Server::MaxActive() const { return nzbget_rs_donor_server_max_active(m_rust); }
inline void DonorHealth::Server::SetRetryAfterMs(int ms) { nzbget_rs_donor_server_set_retry_after_ms(m_rust, ms); }

inline int DonorHealth::Server::ExchangeTrampoline(
	void* cppServer, size_t count, const char* const* msgIds, const size_t* msgLens,
	void* const* rustGates, const char* const* const* groupPtrs,
	const size_t* const* groupLens, const size_t* groupCounts,
	int* outAnswers, size_t* outCount, int* outBusy)
{
	auto* self = static_cast<Server*>(cppServer);
	std::vector<BodyGate> gates(count);
	std::vector<Request> batch(count);
	for (size_t i = 0; i < count; i++)
	{
		batch[i].messageId.assign(msgIds[i] ? msgIds[i] : "", msgLens[i]);
		if (rustGates[i])
		{
			gates[i].rustGate = rustGates[i];
			batch[i].gate = &gates[i];
		}
		if (groupCounts && groupCounts[i] > 0 && groupPtrs && groupPtrs[i] && groupLens && groupLens[i])
		{
			auto g = std::make_shared<std::vector<std::string>>();
			g->reserve(groupCounts[i]);
			for (size_t j = 0; j < groupCounts[i]; j++)
			{
				g->emplace_back(groupPtrs[i][j] ? groupPtrs[i][j] : "", groupLens[i][j]);
			}
			batch[i].groups = std::move(g);
		}
	}
	try
	{
		std::vector<Answer> answers = self->Exchange(batch);
		size_t n = std::min(count, answers.size());
		for (size_t i = 0; i < n; i++)
		{
			outAnswers[i] = (int)answers[i];
		}
		*outCount = n;
		*outBusy = 0;
		return 0;
	}
	catch (ExchangeError& error)
	{
		size_t n = std::min(count, error.answers.size());
		for (size_t i = 0; i < n; i++)
		{
			outAnswers[i] = (int)error.answers[i];
		}
		*outCount = n;
		*outBusy = error.busy ? 1 : 0;
		return 1;
	}
	catch (...)
	{
		*outCount = 0;
		*outBusy = 0;
		return 1;
	}
}

inline DonorHealth::Answer DonorHealth::Server::BodyOnce(BodyGate* gate, const std::function<Answer()>& download)
{
	if (gate && gate->rustGate)
	{
		struct BodyOnceCtx
		{
			const std::function<Answer()>* download;
			std::exception_ptr error;
		} ctx{&download, nullptr};
		int ans = nzbget_rs_donor_body_once(
			gate->rustGate,
			&ctx,
			[](void* rawCtx) -> int {
				auto* c = static_cast<BodyOnceCtx*>(rawCtx);
				try
				{
					return c->download && *c->download ? (int)(*c->download)() : (int)Answer::Error;
				}
				catch (...)
				{
					c->error = std::current_exception();
					return -1;
				}
			});
		if (ctx.error)
		{
			std::rethrow_exception(ctx.error);
		}
		return static_cast<Answer>(ans);
	}
	std::lock_guard<std::mutex> guard(gate->lock);
	if (gate->Done())
	{
		return Answer::Present;
	}
	Answer answer = download();
	gate->ok = answer == Answer::Present;
	return answer;
}

inline void DonorHealth::StopAll() { nzbget_rs_donor_stop_all(); }
inline void DonorHealth::Reset() { nzbget_rs_donor_reset(); }
inline bool DonorHealth::Stopping() { return nzbget_rs_donor_stopping() != 0; }
inline void DonorHealth::WaitAll() { nzbget_rs_donor_wait_all(); }

inline std::vector<DonorHealth::Answer> DonorHealth::Server::Ask(const std::vector<Request>& batch,
	const std::function<bool()>& cancelled)
{
	size_t count = batch.size();
	std::vector<const char*> msgIds(count);
	std::vector<size_t> msgLens(count);
	std::vector<void*> rustGates(count, nullptr);
	std::vector<std::vector<const char*>> groupStrs(count);
	std::vector<std::vector<size_t>> groupLens(count);
	std::vector<const char* const*> groupPtrsOuter(count, nullptr);
	std::vector<const size_t*> groupLensOuter(count, nullptr);
	std::vector<size_t> groupCounts(count, 0);

	for (size_t i = 0; i < count; i++)
	{
		msgIds[i] = batch[i].messageId.data();
		msgLens[i] = batch[i].messageId.size();
		if (batch[i].gate)
		{
			rustGates[i] = batch[i].gate->rustGate;
		}
		if (batch[i].groups && !batch[i].groups->empty())
		{
			const auto& g = *batch[i].groups;
			groupStrs[i].resize(g.size());
			groupLens[i].resize(g.size());
			for (size_t j = 0; j < g.size(); j++)
			{
				groupStrs[i][j] = g[j].data();
				groupLens[i][j] = g[j].size();
			}
			groupPtrsOuter[i] = groupStrs[i].data();
			groupLensOuter[i] = groupLens[i].data();
			groupCounts[i] = g.size();
		}
	}

	std::vector<int> outAnswers(count, (int)Answer::Error);
	nzbget_rs_donor_server_ask(
		m_rust, count, msgIds.data(), msgLens.data(), rustGates.data(),
		groupPtrsOuter.data(), groupLensOuter.data(), groupCounts.data(),
		const_cast<std::function<bool()>*>(&cancelled),
		[](void* rawCtx) -> int {
			auto* fn = static_cast<const std::function<bool()>*>(rawCtx);
			return fn && *fn && (*fn)() ? 1 : 0;
		},
		outAnswers.data());

	std::vector<Answer> res(count);
	for (size_t i = 0; i < count; i++)
	{
		res[i] = static_cast<Answer>(outAnswers[i]);
	}
	return res;
}

inline DonorHealth::Health DonorHealth::CheckItems(const ServerList& servers, const std::vector<Item>& items, int budgetMs)
{
	std::vector<void*> handles(servers.size());
	for (size_t i = 0; i < servers.size(); i++)
	{
		handles[i] = servers[i] ? servers[i]->RustHandle() : nullptr;
	}
	auto* keepalive = new ServerList(servers);
	size_t count = items.size();
	std::vector<const char*> msgIds(count);
	std::vector<size_t> msgLens(count);
	std::vector<int> bodies(count);
	std::vector<std::vector<const char*>> groupStrs(count);
	std::vector<std::vector<size_t>> groupLens(count);
	std::vector<const char* const*> groupPtrsOuter(count, nullptr);
	std::vector<const size_t*> groupLensOuter(count, nullptr);
	std::vector<size_t> groupCounts(count, 0);

	for (size_t i = 0; i < count; i++)
	{
		msgIds[i] = items[i].messageId.data();
		msgLens[i] = items[i].messageId.size();
		bodies[i] = items[i].body ? 1 : 0;
		if (items[i].groups && !items[i].groups->empty())
		{
			const auto& g = *items[i].groups;
			groupStrs[i].resize(g.size());
			groupLens[i].resize(g.size());
			for (size_t j = 0; j < g.size(); j++)
			{
				groupStrs[i][j] = g[j].data();
				groupLens[i][j] = g[j].size();
			}
			groupPtrsOuter[i] = groupStrs[i].data();
			groupLensOuter[i] = groupLens[i].data();
			groupCounts[i] = g.size();
		}
	}

	Health health;
	nzbget_rs_donor_check_items(
		handles.data(), handles.size(),
		keepalive, [](void* p) { delete static_cast<ServerList*>(p); },
		msgIds.data(), msgLens.data(), bodies.data(),
		groupPtrsOuter.data(), groupLensOuter.data(), groupCounts.data(),
		count, budgetMs,
		&health.checked, &health.present, &health.missing, &health.error,
		&health.bodyChecked, &health.bodyBad);
	return health;
}

inline DonorHealth::Health DonorHealth::CheckPosting(const ServerList& servers, const std::vector<std::string>& ids,
	const std::shared_ptr<std::vector<std::string>>& groups, const Options& options, bool full)
{
	std::vector<void*> handles(servers.size());
	for (size_t i = 0; i < servers.size(); i++)
	{
		handles[i] = servers[i] ? servers[i]->RustHandle() : nullptr;
	}
	auto* keepalive = new ServerList(servers);
	std::vector<const char*> idPtrs(ids.size());
	std::vector<size_t> idLens(ids.size());
	for (size_t i = 0; i < ids.size(); i++)
	{
		idPtrs[i] = ids[i].data();
		idLens[i] = ids[i].size();
	}
	std::vector<const char*> groupPtrs;
	std::vector<size_t> groupLens;
	if (groups)
	{
		groupPtrs.resize(groups->size());
		groupLens.resize(groups->size());
		for (size_t i = 0; i < groups->size(); i++)
		{
			groupPtrs[i] = (*groups)[i].data();
			groupLens[i] = (*groups)[i].size();
		}
	}
	Health health;
	nzbget_rs_donor_check_posting(
		handles.data(), handles.size(),
		keepalive, [](void* p) { delete static_cast<ServerList*>(p); },
		idPtrs.data(), idLens.data(), ids.size(),
		groupPtrs.data(), groupLens.data(), groupPtrs.size(),
		options.sample.percent, options.sample.minimum, options.sample.maximum,
		options.sample.bodyPercent, options.sample.maxBody, options.sample.seed,
		options.probe, options.budgetMs, (int64_t)options.deadlineMs, full ? 1 : 0,
		&health.checked, &health.present, &health.missing, &health.error,
		&health.bodyChecked, &health.bodyBad);
	return health;
}

inline void DonorHealth::CheckPostings(const ServerList& servers, const std::vector<Posting>& postings,
	const Options& options, bool full, int concurrency,
	const std::function<void(const std::string&, const Health&)>& emit)
{
	std::vector<void*> handles(servers.size());
	for (size_t i = 0; i < servers.size(); i++)
	{
		handles[i] = servers[i] ? servers[i]->RustHandle() : nullptr;
	}
	auto* keepalive = new ServerList(servers);
	size_t pcount = postings.size();
	std::vector<const char*> keyPtrs(pcount);
	std::vector<size_t> keyLens(pcount);
	std::vector<std::vector<const char*>> idStrs(pcount);
	std::vector<std::vector<size_t>> idLens(pcount);
	std::vector<const char* const*> idPtrsOuter(pcount, nullptr);
	std::vector<const size_t*> idLensOuter(pcount, nullptr);
	std::vector<size_t> idCounts(pcount, 0);
	std::vector<std::vector<const char*>> groupStrs(pcount);
	std::vector<std::vector<size_t>> groupLens(pcount);
	std::vector<const char* const*> groupPtrsOuter(pcount, nullptr);
	std::vector<const size_t*> groupLensOuter(pcount, nullptr);
	std::vector<size_t> groupCounts(pcount, 0);

	for (size_t i = 0; i < pcount; i++)
	{
		keyPtrs[i] = postings[i].key.data();
		keyLens[i] = postings[i].key.size();
		idStrs[i].resize(postings[i].ids.size());
		idLens[i].resize(postings[i].ids.size());
		for (size_t j = 0; j < postings[i].ids.size(); j++)
		{
			idStrs[i][j] = postings[i].ids[j].data();
			idLens[i][j] = postings[i].ids[j].size();
		}
		idPtrsOuter[i] = idStrs[i].data();
		idLensOuter[i] = idLens[i].data();
		idCounts[i] = postings[i].ids.size();

		if (postings[i].groups && !postings[i].groups->empty())
		{
			const auto& g = *postings[i].groups;
			groupStrs[i].resize(g.size());
			groupLens[i].resize(g.size());
			for (size_t j = 0; j < g.size(); j++)
			{
				groupStrs[i][j] = g[j].data();
				groupLens[i][j] = g[j].size();
			}
			groupPtrsOuter[i] = groupStrs[i].data();
			groupLensOuter[i] = groupLens[i].data();
			groupCounts[i] = g.size();
		}
	}

	nzbget_rs_donor_check_postings(
		handles.data(), handles.size(),
		keepalive, [](void* p) { delete static_cast<ServerList*>(p); },
		pcount, keyPtrs.data(), keyLens.data(),
		idPtrsOuter.data(), idLensOuter.data(), idCounts.data(),
		groupPtrsOuter.data(), groupLensOuter.data(), groupCounts.data(),
		options.sample.percent, options.sample.minimum, options.sample.maximum,
		options.sample.bodyPercent, options.sample.maxBody, options.sample.seed,
		options.probe, options.budgetMs, (int64_t)options.deadlineMs,
		full ? 1 : 0, concurrency,
		const_cast<std::function<void(const std::string&, const Health&)>*>(&emit),
		[](void* rawCtx, const char* keyPtr, size_t keyLen,
			int checked, int present, int missing, int error, int bodyChecked, int bodyBad) {
			auto* fn = static_cast<const std::function<void(const std::string&, const Health&)>*>(rawCtx);
			if (fn && *fn)
			{
				Health h;
				h.checked = checked;
				h.present = present;
				h.missing = missing;
				h.error = error;
				h.bodyChecked = bodyChecked;
				h.bodyBad = bodyBad;
				(*fn)(std::string(keyPtr ? keyPtr : "", keyLen), h);
			}
		});
}

#include <algorithm>
#include <cstdint>
#include <ctime>
#include <map>
#include <set>
#include <thread>
#include <utility>
#include "nzbget_rs.h"

class ReleaseName
{
public:
	static constexpr size_t MaxNameLength = 512;
	struct Attrs
	{
		std::set<std::string> resolution;
		std::set<std::string> quality;
		std::set<std::string> codec;
		std::set<std::string> bitDepth;
		std::set<std::string> audio;
		std::set<std::string> channels;
		std::set<std::string> network;
		std::set<std::string> edition;
		std::set<std::string> hdr;
		std::string title;
		std::string group;
		std::vector<int> seasons;
		std::vector<int> episodes;
		int year = 0;
		bool repack = false;
		bool proper = false;
	};

	static std::string Clean(const std::string& name)
	{
		return Take(nzbget_rs_releasename_clean(name.data(), name.size()));
	}

	static std::string Normalize(const std::string& name)
	{
		return Take(nzbget_rs_releasename_normalize(name.data(), name.size()));
	}

	static std::string ShortQuery(const std::string& title)
	{
		return Take(nzbget_rs_releasename_short_query(title.data(), title.size()));
	}

	static Attrs Parse(const std::string& name)
	{
		std::string raw = Take(nzbget_rs_releasename_parse(name.data(), name.size()));
		struct Reader
		{
			const char* p;
			const char* end;
			uint32_t U32()
			{
				if (end - p < 4) { p = end; return 0; }
				const unsigned char* b = (const unsigned char*)p;
				p += 4;
				return (uint32_t)b[0] | (uint32_t)b[1] << 8 | (uint32_t)b[2] << 16 | (uint32_t)b[3] << 24;
			}
			std::string Str() { uint32_t n = U32(); n = std::min<uint32_t>(n, end - p); std::string s(p, n); p += n; return s; }
			int I32() { return (int)U32(); }
			bool Byte() { return p < end ? *p++ != 0 : false; }
		} in{raw.data(), raw.data() + raw.size()};

		Attrs attrs;
		for (std::set<std::string>* set : {&attrs.resolution, &attrs.quality, &attrs.codec, &attrs.bitDepth, &attrs.audio,
			&attrs.channels, &attrs.network, &attrs.edition, &attrs.hdr})
		{
			for (uint32_t n = in.U32(); n > 0 && in.p < in.end; n--)
			{
				set->insert(in.Str());
			}
		}
		attrs.title = in.Str();
		attrs.group = in.Str();
		for (std::vector<int>* list : {&attrs.seasons, &attrs.episodes})
		{
			for (uint32_t n = in.U32(); n > 0 && in.p < in.end; n--)
			{
				list->push_back(in.I32());
			}
		}
		attrs.year = in.I32();
		attrs.repack = in.Byte();
		attrs.proper = in.Byte();
		return attrs;
	}

	static bool Readable(const std::string& name)
	{
		return nzbget_rs_releasename_readable(name.data(), name.size()) != 0;
	}

	static bool SameRelease(const std::string& aName, const std::string& bName)
	{
		return nzbget_rs_releasename_same_release(aName.data(), aName.size(), bName.data(), bName.size()) != 0;
	}

private:
	static std::string Take(NzbgetRsBuf buf)
	{
		std::string out(buf.data, buf.len);
		nzbget_rs_free(buf);
		return out;
	}
};

namespace DupeUtil
{
	inline bool WriteAtomic(const std::string& path, const std::string& data)
	{
		return nzbget_rs_dupe_util_write_atomic(path.data(), path.size(), data.data(), data.size()) != 0;
	}

	inline std::string Lower(std::string text)
	{
		NzbgetRsBuf buf = nzbget_rs_dqh_dupe_util_lower(text.data(), text.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	inline std::string ReadAll(const std::string& path)
	{
		NzbgetRsBuf buf = nzbget_rs_dupe_util_read_all(path.data(), path.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}
}

struct NzbSummary
{
	struct Head
	{
		std::string name;
		std::string messageId;
		long long bytes = 0;
	};
	int files = 0;
	long long totalBytes = 0;
	std::set<std::string> filenames;
	std::string poster;
	std::vector<std::string> messageIds;
	std::vector<std::string> groups;
	std::map<std::string, std::string> meta;
	std::string mainName;
	uint64_t listedParts = 0;
	uint64_t declaredParts = 0;
	uint64_t articleBytes = 0;
	Head parIndex;
	std::vector<uint64_t> recoveryBytes;
	long long dataBytes = 0;
	int dataFiles = 0;
	std::vector<Head> heads;
	std::vector<std::string> dataMessageIds;
	uint64_t dataListedParts = 0;
	uint64_t dataDeclaredParts = 0;

	std::string Fingerprint() const
	{
		std::vector<const char*> ptrs;
		std::vector<size_t> lens;
		ptrs.reserve(messageIds.size());
		lens.reserve(messageIds.size());
		for (const std::string& id : messageIds)
		{
			ptrs.push_back(id.data());
			lens.push_back(id.size());
		}
		char buffer[17] = {};
		nzbget_rs_dupesearch_fnv1a_fingerprint(ptrs.data(), lens.data(), ptrs.size(), buffer);
		return std::string(buffer, 16);
	}
};

class NzbReader
{
public:
	static bool Parse(const std::string& data, NzbSummary& summary)
	{
		summary = NzbSummary();
		RsNzbSummaryScalars scalars{};
		int ok = nzbget_rs_nzb_reader_parse(
			reinterpret_cast<const uint8_t*>(data.data()),
			data.size(),
			&scalars,
			&summary,
			NzbReader::OnStrings,
			NzbReader::OnSlice,
			NzbReader::OnRecoveryBytes);
		if (ok == 0)
		{
			summary = NzbSummary();
			return false;
		}
		ApplyScalars(summary, scalars);
		return true;
	}

	static void OnStrings(void* ctx,
		const uint8_t* posterPtr, size_t posterLen,
		const uint8_t* mainNamePtr, size_t mainNameLen,
		const uint8_t* parNamePtr, size_t parNameLen,
		const uint8_t* parIdPtr, size_t parIdLen,
		int64_t parBytes)
	{
		NzbSummary* s = static_cast<NzbSummary*>(ctx);
		s->poster.assign(reinterpret_cast<const char*>(posterPtr), posterLen);
		s->mainName.assign(reinterpret_cast<const char*>(mainNamePtr), mainNameLen);
		s->parIndex.name.assign(reinterpret_cast<const char*>(parNamePtr), parNameLen);
		s->parIndex.messageId.assign(reinterpret_cast<const char*>(parIdPtr), parIdLen);
		s->parIndex.bytes = parBytes;
	}

	static void OnSlice(void* ctx, int kind,
		const uint8_t* ptr1, size_t len1,
		const uint8_t* ptr2, size_t len2,
		int64_t bytes)
	{
		NzbSummary* s = static_cast<NzbSummary*>(ctx);
		const char* c1 = reinterpret_cast<const char*>(ptr1);
		const char* c2 = reinterpret_cast<const char*>(ptr2);
		switch (kind)
		{
			case 1:
				s->groups.emplace_back(c1, len1);
				break;
			case 2:
				s->meta.emplace(std::string(c1, len1), std::string(c2, len2));
				break;
			case 3:
				s->filenames.emplace(c1, len1);
				break;
			case 4:
				s->messageIds.emplace_back(c1, len1);
				break;
			case 5:
				s->dataMessageIds.emplace_back(c1, len1);
				break;
			case 6:
				s->heads.push_back({std::string(c1, len1), std::string(c2, len2), bytes});
				break;
		}
	}

	static void OnRecoveryBytes(void* ctx, const uint64_t* ptr, size_t len)
	{
		NzbSummary* s = static_cast<NzbSummary*>(ctx);
		s->recoveryBytes.assign(ptr, ptr + len);
	}

	static void ApplyScalars(NzbSummary& summary, const RsNzbSummaryScalars& scalars)
	{
		summary.files = scalars.files;
		summary.totalBytes = scalars.total_bytes;
		summary.dataFiles = scalars.data_files;
		summary.dataBytes = scalars.data_bytes;
		summary.listedParts = scalars.listed_parts;
		summary.declaredParts = scalars.declared_parts;
		summary.dataListedParts = scalars.data_listed_parts;
		summary.dataDeclaredParts = scalars.data_declared_parts;
		summary.articleBytes = scalars.article_bytes;
	}
};

class HttpGet
{
public:
	struct Reply
	{
		bool ok = false;
		int status = 0;
		std::string body;
	};

	static Reply Fetch(const std::string& url, const std::string& infoName, size_t maxBytes,
		long long deadlineMs = 0)
	{
		Reply reply;
		if (g_stopped)
		{
			return reply;
		}

		NzbgetRsBuf pathBuf = nzbget_rs_dqh_http_get_format_temp_path(
			g_Options->GetTempDir(), PATH_SEPARATOR, (int)Util::CurrentTime(), ++g_counter);
		std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
		nzbget_rs_free(pathBuf);

		WebDownloader downloader;
		downloader.SetUrl(url.c_str());
		downloader.SetInfoName(infoName.c_str());
		downloader.SetOutputFilename(path.c_str());
		downloader.SetForce(true);
		downloader.SetRetry(false);

		{
			Guard guard(g_mutex);
			if (g_stopped)
			{
				return reply;
			}
			g_active.insert(&downloader);
		}
		std::mutex doneMutex;
		std::condition_variable doneCond;
		bool done = false;
		std::thread watchdog([&]()
			{
				std::unique_lock<std::mutex> lock(doneMutex);
				while (!done)
				{
					int64_t waitMs = 250;
					if (nzbget_rs_snh_http_get_calc_watchdog_wait(
						deadlineMs, DonorHealth::NowMs(), FileSystem::FileSize(path.c_str()), maxBytes, &waitMs))
					{
						downloader.Stop();
						return;
					}
					doneCond.wait_for(lock, std::chrono::milliseconds(waitMs), [&]() { return done; });
				}
			});
		WebDownloader::EStatus status = downloader.DownloadWithRedirects(5);
		if (watchdog.joinable())
		{
			{
				std::lock_guard<std::mutex> lock(doneMutex);
				done = true;
			}
			doneCond.notify_all();
			watchdog.join();
		}
		{
			Guard guard(g_mutex);
			g_active.erase(&downloader);
		}

		reply.status = downloader.GetHttpStatus();
		int readOk = 0;
		NzbgetRsBuf bodyBuf = nzbget_rs_http_get_read_and_delete(
			path.data(), path.size(), maxBytes, status == WebDownloader::adFinished ? 1 : 0, &readOk);
		if (readOk != 0)
		{
			reply.body.assign(bodyBuf.data ? bodyBuf.data : "", bodyBuf.len);
			reply.ok = true;
		}
		nzbget_rs_free(bodyBuf);
		return reply;
	}

	static void StopAll()
	{
		Guard guard(g_mutex);
		g_stopped = true;
		for (WebDownloader* downloader : g_active)
		{
			downloader->Stop();
		}
	}

	static void Reset()
	{
		g_stopped = false;
	}

	static bool Stopped()
	{
		return g_stopped;
	}

private:
	inline static Mutex g_mutex;
	inline static std::set<WebDownloader*> g_active;
	inline static std::atomic<bool> g_stopped{false};
	inline static std::atomic<int> g_counter{0};
};

class Newznab
{
public:
	struct Result
	{
		std::string title;
		std::string link;
		long long size = 0;
		int grabs = 0;
		time_t date = 0;
		std::string indexer;
	};

	struct Page
	{
		std::vector<Result> items;
		bool error = false;
		int errorCode = 0;
		std::string errorText;
	};

	using Params = std::vector<std::pair<std::string, std::string>>;

	static constexpr int PageSize = 100;
	static constexpr int MaxPages = 5;
	static constexpr int Parallel = 4;

	static bool ParseResponse(const std::string& xml, Page& page)
	{
		page = Page();
		return nzbget_rs_newznab_parse_response(
			reinterpret_cast<const uint8_t*>(xml.data()),
			xml.size(),
			&page,
			[](void* ctx, int errorCode, const uint8_t* errorTextPtr, size_t errorTextLen) noexcept
			{
				auto* out = static_cast<Page*>(ctx);
				out->error = true;
				out->errorCode = errorCode;
				out->errorText.assign(reinterpret_cast<const char*>(errorTextPtr), errorTextLen);
			},
			[](void* ctx, const uint8_t* titlePtr, size_t titleLen,
			   const uint8_t* linkPtr, size_t linkLen, int64_t size, int32_t grabs,
			   int64_t date, const uint8_t* indexerPtr, size_t indexerLen) noexcept
			{
				auto* out = static_cast<Page*>(ctx);
				Result r;
				r.title.assign(reinterpret_cast<const char*>(titlePtr), titleLen);
				r.link.assign(reinterpret_cast<const char*>(linkPtr), linkLen);
				r.size = (long long)size;
				r.grabs = grabs;
				r.date = (time_t)date;
				r.indexer.assign(reinterpret_cast<const char*>(indexerPtr), indexerLen);
				out->items.push_back(std::move(r));
			}) != 0;
	}

	static time_t ParseDate(const std::string& text)
	{
		return (time_t)nzbget_rs_df_parse_newznab_date(text.data(), text.size());
	}

	static std::vector<Params> BuildQueries(const std::string& title, const std::string& imdb,
		const std::string& tvdb)
	{
		std::vector<Params> queries;
		nzbget_rs_df_build_newznab_queries(
			title.data(), title.size(),
			imdb.data(), imdb.size(),
			tvdb.data(), tvdb.size(),
			&queries,
			[](void* ctx, const char* const* keys, const size_t* keyLens,
			   const char* const* vals, const size_t* valLens, size_t count) noexcept
			{
				auto* out = static_cast<std::vector<Params>*>(ctx);
				Params p;
				p.reserve(count);
				for (size_t i = 0; i < count; i++)
				{
					p.emplace_back(std::string(keys[i], keyLens[i]), std::string(vals[i], valLens[i]));
				}
				out->push_back(std::move(p));
			});
		return queries;
	}

	static std::string BuildUrl(const std::string& base, const Params& params, const std::string& apiKey)
	{
		std::vector<const char*> keys(params.size());
		std::vector<const char*> vals(params.size());
		for (size_t i = 0; i < params.size(); i++)
		{
			keys[i] = params[i].first.c_str();
			vals[i] = params[i].second.c_str();
		}
		NzbgetRsBuf buf = nzbget_rs_df_build_newznab_url(
			base.c_str(), keys.data(), vals.data(), params.size(), apiKey.c_str());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	static std::string Mask(const std::string& text)
	{
		NzbgetRsBuf buf = nzbget_rs_df_mask_newznab_text(text.data(), text.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	struct SearchStats
	{
		int queries = 0;
		int failed = 0;
		int pages = 0;
	};

	static std::vector<Result> Search(const std::string& base, const std::string& apiKey,
		const std::vector<Params>& queries, time_t deadline, SearchStats* stats)
	{
		std::vector<std::vector<Result>> found(queries.size());
		std::atomic<size_t> next{0};
		std::atomic<int> failed{0};
		std::atomic<int> pages{0};

		auto worker = [&]()
		{
			for (size_t index = next++; index < queries.size(); index = next++)
			{
				std::vector<const char*> lKeys;
				std::vector<const char*> lVals;
				lKeys.reserve(queries[index].size());
				lVals.reserve(queries[index].size());
				for (const auto& param : queries[index])
				{
					lKeys.push_back(param.first.c_str());
					lVals.push_back(param.second.c_str());
				}
				NzbgetRsBuf lblBuf = nzbget_rs_dqh_newznab_format_query_label(
					lKeys.data(), lVals.data(), lKeys.size());
				std::string label(lblBuf.data ? lblBuf.data : "", lblBuf.len);
				nzbget_rs_free(lblBuf);

				for (int page = 0; page < MaxPages; page++)
				{
					if (Util::CurrentTime() >= deadline)
					{
						detail("DupeSearch: no time left for the search %s", label.c_str());
						break;
					}

					Params params = queries[index];
					NzbgetRsBuf limBuf = nzbget_rs_dqh_conn_format_port_str(PageSize);
					params.emplace_back("limit", std::string(limBuf.data ? limBuf.data : "", limBuf.len));
					nzbget_rs_free(limBuf);
					NzbgetRsBuf offBuf = nzbget_rs_dqh_conn_format_port_str(page * PageSize);
					params.emplace_back("offset", std::string(offBuf.data ? offBuf.data : "", offBuf.len));
					nzbget_rs_free(offBuf);
					NzbgetRsBuf infoBuf = nzbget_rs_dqh_newznab_format_search_info_name(label.c_str(), page + 1);
					std::string infoName(infoBuf.data ? infoBuf.data : "", infoBuf.len);
					nzbget_rs_free(infoBuf);
					long long pageDeadlineMs = std::max(1LL, DonorHealth::NowMs() + (long long)(deadline - Util::CurrentTime()) * 1000LL);
					HttpGet::Reply reply = HttpGet::Fetch(BuildUrl(base, params, apiKey), infoName, 16 * 1024 * 1024,
						pageDeadlineMs);
					if (!reply.ok && reply.status == 0 && !HttpGet::Stopped() && Util::CurrentTime() < deadline)
					{
						reply = HttpGet::Fetch(BuildUrl(base, params, apiKey), infoName, 16 * 1024 * 1024,
							pageDeadlineMs);
					}
					pages++;

					Page result;
					if (!reply.ok)
					{
						warn("DupeSearch: the search %s failed (HTTP %i)", label.c_str(), reply.status);
						failed++;
						break;
					}
					if (!ParseResponse(reply.body, result))
					{
						warn("DupeSearch: the answer to the search %s isn't XML (is option DupeSearchUrl the "
							"indexer's api address, like http://host:5076/api?)", label.c_str());
						failed++;
						break;
					}
					if (result.error)
					{
						warn("DupeSearch: the indexer answered the search %s with error %i: %s",
							label.c_str(), result.errorCode, Mask(result.errorText).c_str());
						failed++;
						break;
					}

					found[index].insert(found[index].end(), result.items.begin(), result.items.end());
					if ((int)result.items.size() < PageSize)
					{
						break;
					}
				}
			}
		};

		std::vector<std::thread> threads;
		size_t count = std::min(queries.size(), (size_t)Parallel);
		for (size_t i = 0; i < count; i++)
		{
			threads.emplace_back(worker);
		}
		for (std::thread& thread : threads)
		{
			thread.join();
		}

		std::vector<Result> merged;
		std::set<std::string> links;
		for (const std::vector<Result>& results : found)
		{
			for (const Result& result : results)
			{
				if (!result.link.empty() && links.insert(result.link).second)
				{
					merged.push_back(result);
				}
			}
		}

		if (stats)
		{
			stats->queries = (int)queries.size();
			stats->failed = failed;
			stats->pages = pages;
		}
		return merged;
	}
};

class Posting
{
public:
	static constexpr int RelistWindowSec = 120;
	static constexpr double SamePostingShare = 0.01;
	static constexpr size_t SketchSize = 64;
	static constexpr double ListingMismatchShare = 0.02;

	using Sketch = std::vector<uint32_t>;
	using Group = std::vector<Newznab::Result>;

	static Sketch MakeSketch(const std::vector<std::string>& messageIds)
	{
		Strings ids(messageIds);
		Sketch sketch(SketchSize);
		size_t len = 0;
		nzbget_rs_posting_sketch(ids.data.data(), ids.lens.data(), ids.data.size(), sketch.data(), &len);
		sketch.resize(len);
		return sketch;
	}

	static bool SameSketch(const Sketch& a, const Sketch& b)
	{
		return nzbget_rs_posting_same_sketch(a.data(), a.size(), b.data(), b.size()) != 0;
	}

	static bool SketchOfFile(const std::string& path, Sketch& sketch)
	{
		sketch.assign(SketchSize, 0);
		size_t len = 0;
		if (!nzbget_rs_posting_sketch_of_file(path.c_str(), sketch.data(), SketchSize, &len))
		{
			sketch.clear();
			return false;
		}
		sketch.resize(len);
		return true;
	}

	static bool SamePosting(const std::vector<std::string>& a, const std::vector<std::string>& b)
	{
		Strings x(a), y(b);
		return nzbget_rs_posting_same_posting(x.data.data(), x.lens.data(), x.data.size(), y.data.data(), y.lens.data(),
			y.data.size()) != 0;
	}

	static std::vector<Group> GroupListings(std::vector<Newznab::Result> results)
	{
		std::vector<NzbgetRsListing> raw = Listings(results);
		std::vector<size_t> indices(raw.size()), sizes(raw.size());
		size_t count = 0;
		nzbget_rs_posting_group(raw.data(), raw.size(), indices.data(), sizes.data(), &count);
		return Groups(results, indices, sizes, count);
	}

	static bool ListingMismatch(const Newznab::Result& listing, long long nzbBytes)
	{
		return nzbget_rs_posting_listing_mismatch((int64_t)listing.size, (int64_t)nzbBytes) != 0;
	}

	static std::vector<Group> OrderPostings(const std::vector<Newznab::Result>& candidates,
		long long pickBytes, int maxPostings)
	{
		std::vector<NzbgetRsListing> raw = Listings(candidates);
		std::vector<size_t> indices(raw.size()), sizes(raw.size());
		size_t count = 0;
		nzbget_rs_posting_order(raw.data(), raw.size(), (int64_t)pickBytes, maxPostings, indices.data(), sizes.data(), &count);
		return Groups(candidates, indices, sizes, count);
	}

private:
	struct Strings
	{
		std::vector<const char*> data;
		std::vector<size_t> lens;
		explicit Strings(const std::vector<std::string>& texts)
		{
			for (const std::string& t : texts) { data.push_back(t.data()); lens.push_back(t.size()); }
		}
	};

	static std::vector<NzbgetRsListing> Listings(const std::vector<Newznab::Result>& results)
	{
		std::vector<NzbgetRsListing> raw;
		for (const Newznab::Result& r : results)
		{
			raw.push_back({r.link.data(), r.link.size(), (int64_t)r.size, (int32_t)r.grabs, (int64_t)r.date});
		}
		return raw;
	}

	static std::vector<Group> Groups(const std::vector<Newznab::Result>& results, const std::vector<size_t>& indices,
		const std::vector<size_t>& sizes, size_t count)
	{
		std::vector<Group> groups;
		size_t at = 0;
		for (size_t g = 0; g < count; g++)
		{
			Group group;
			for (size_t k = 0; k < sizes[g]; k++) group.push_back(results[indices[at++]]);
			groups.push_back(std::move(group));
		}
		return groups;
	}
};

class NntpHealthServer : public DonorHealth::Server
{
public:
	static constexpr int MaxCap = 20;
	static constexpr int ConnectionWaitSec = 10;

	NntpHealthServer(int serverId, int cap) : Server(cap), m_serverId(serverId) {}

	int ServerId() const { return m_serverId; }

	static int CapFor(int connections)
	{
		return nzbget_rs_snh_nntp_health_cap_for(connections, MaxCap);
	}

	static DonorHealth::ServerList Servers()
	{
		static std::mutex mutex;
		static std::map<int, std::shared_ptr<NntpHealthServer>> known;

		std::lock_guard<std::mutex> guard(mutex);
		DonorHealth::ServerList list;
		std::set<int> groups;
		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (!server->GetActive() || server->GetMaxConnections() == 0)
			{
				continue;
			}
			if (server->GetGroup() > 0 && !groups.insert(server->GetGroup()).second)
			{
				continue;
			}
			std::shared_ptr<NntpHealthServer>& entry = known[server->GetId()];
			int wantedCap = CapFor(server->GetMaxConnections());
			if (!entry || entry->Cap() != wantedCap)
			{
				entry = std::make_shared<NntpHealthServer>(server->GetId(), wantedCap);
			}
			list.push_back(entry);
		}
		return list;
	}

protected:
	std::vector<DonorHealth::Answer> Exchange(const std::vector<DonorHealth::Request>& batch) override
	{
		using Answer = DonorHealth::Answer;

		NewsServer* server = g_ServerPool->GetServerById(m_serverId);
		if (!server)
		{
			throw DonorHealth::ExchangeError();
		}

		NntpConnection* connection = nullptr;
		time_t waitStart = Util::CurrentTime();
		while (!connection && !HttpGet::Stopped() && Util::CurrentTime() - waitStart <= ConnectionWaitSec)
		{
			connection = g_ServerPool->GetConnection(server->GetNormLevel(), server, nullptr);
			if (!connection)
			{
				Util::Sleep(20);
			}
		}
		if (!connection)
		{
			DonorHealth::ExchangeError busy;
			busy.busy = true;
			throw busy;
		}

		struct Release
		{
			NntpConnection* connection;
			bool broken = false;
			~Release()
			{
				if (broken)
				{
					connection->Disconnect();
				}
				g_ServerPool->FreeConnection(connection, true);
			}
		} release{ connection };

		if (!connection->Connect())
		{
			release.broken = true;
			throw DonorHealth::ExchangeError();
		}

		if (server->GetJoinGroup() && batch.front().groups)
		{
			const char* joined = nullptr;
			for (const std::string& group : *batch.front().groups)
			{
				joined = connection->JoinGroup(group.c_str());
				if (joined && joined[0] == '2')
				{
					break;
				}
			}
			if (!joined || joined[0] != '2')
			{
				release.broken = !joined;
				throw DonorHealth::ExchangeError();
			}
		}

		std::vector<const char*> statIds;
		std::vector<size_t> statLens;
		statIds.reserve(batch.size());
		statLens.reserve(batch.size());
		for (const DonorHealth::Request& request : batch)
		{
			statIds.push_back(request.messageId.data());
			statLens.push_back(request.messageId.size());
		}
		NzbgetRsBuf cmdBuf = nzbget_rs_dqh_nntp_health_format_stat_batch(
			statIds.data(), statLens.data(), statIds.size());
		std::string commands(cmdBuf.data ? cmdBuf.data : "", cmdBuf.len);
		nzbget_rs_free(cmdBuf);
		if (!connection->Send(commands.c_str(), (int)commands.size()))
		{
			release.broken = true;
			throw DonorHealth::ExchangeError();
		}

		std::vector<Answer> answers;
		for (size_t i = 0; i < batch.size(); i++)
		{
			if (HttpGet::Stopped() || DonorHealth::Stopping())
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				throw error;
			}
			char line[1024];
			int length = 0;
			if (!connection->ReadLine(line, sizeof(line), &length))
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				throw error;
			}
			int statReply = nzbget_rs_snh_nntp_health_classify_stat_reply(line);
			answers.push_back(statReply == 0 ? Answer::Present : statReply == 1 ? Answer::Missing : Answer::Error);
		}

		for (size_t i = 0; i < batch.size(); i++)
		{
			if (HttpGet::Stopped() || DonorHealth::Stopping())
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				for (size_t j = i; j < batch.size(); j++)
					if (batch[j].gate && error.answers[j] == Answer::Present)
						error.answers[j] = Answer::Error;
				throw error;
			}
			if (!batch[i].gate || answers[i] != Answer::Present)
			{
				continue;
			}
			try
			{
				answers[i] = BodyOnce(batch[i].gate, [&]() -> Answer
					{
						std::vector<CString> groups;
						if (batch[i].groups)
						{
							for (const std::string& group : *batch[i].groups)
							{
								groups.emplace_back(group.c_str());
							}
						}
						ArticleFetcher fetcher;
						ArticleFetcher::FetchedArticle article = fetcher.FetchFromConnection(connection, Bracketed(batch[i].messageId).c_str(), groups);
						int throwError = 0;
						int classified = nzbget_rs_nntp_health_classify_body_fetch_result(
							article.Success ? 1 : 0, article.Transient ? 1 : 0,
							(int)Answer::Present, (int)Answer::BodyBad, &throwError);
						if (throwError != 0)
						{
							throw DonorHealth::ExchangeError();
						}
						return (Answer)classified;
					});
			}
			catch (DonorHealth::ExchangeError&)
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				error.answers[i] = Answer::Error;
				for (size_t j = i + 1; j < batch.size(); j++)
				{
					if (batch[j].gate && answers[j] == Answer::Present)
					{
						error.answers[j] = Answer::Error;
					}
				}
				throw error;
			}
		}
		return answers;
	}

private:
	int m_serverId;

	static std::string Bracketed(const std::string& messageId)
	{
		NzbgetRsBuf buf = nzbget_rs_dqh_nntp_health_format_bracketed_id(
			messageId.data(), messageId.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}
};

#endif

#include "nzbget_rs.h"

struct NzbgetRsDeadPostingsStore;

class DeadPostings
{
public:
	static constexpr int DeadTtlSec = 3 * 24 * 3600;

	DeadPostings()
		: m_store(nzbget_rs_deadpostings_store_new())
	{
	}

	~DeadPostings()
	{
		nzbget_rs_deadpostings_store_free(m_store);
	}

	DeadPostings(const DeadPostings&) = delete;
	DeadPostings& operator=(const DeadPostings&) = delete;

	void SetStatePath(const std::string& path)
	{
		m_statePath = path;
		nzbget_rs_deadpostings_store_set_state_path(
			m_store, reinterpret_cast<const uint8_t*>(path.data()), path.size());
	}

	void Load()
	{
		nzbget_rs_deadpostings_store_load(m_store, (int64_t)Util::CurrentTime());
	}

	bool IsDead(const Posting::Sketch& sketch, time_t now = 0)
	{
		now = now ? now : Util::CurrentTime();
		return nzbget_rs_deadpostings_store_is_dead(
			m_store, sketch.data(), sketch.size(), (int64_t)now) != 0;
	}

	void Add(const Posting::Sketch& sketch, time_t now = 0, bool save = true)
	{
		now = now ? now : Util::CurrentTime();
		if (nzbget_rs_deadpostings_store_add(
			m_store, sketch.data(), sketch.size(), (int64_t)now, save ? 1 : 0) == 0)
		{
			warn("Could not save the DupeSearch dead postings to %s", m_statePath.c_str());
		}
	}

private:
	NzbgetRsDeadPostingsStore* m_store;
	std::string m_statePath;
};

class NzbFetcher
{
public:
	enum EReason
	{
		frOk,
		frFetch,		// the download failed
		frParse,		// not an nzb-file (an error answer, an empty body, HTML)
		frRefused,		// the indexer is at its grab limit: not asked
		frBudget
	};

	struct Fetched
	{
		Newznab::Result listing;
		EReason reason = frFetch;
		std::string data;
		NzbSummary info;
		bool sameInner = false;
		bool lenient = false;
	};

	struct Stats
	{
		int ok = 0;
		int fetch = 0;
		int parse = 0;
		int refused = 0;
		int relisted = 0;			// listings of a fetched posting that weren't needed
		int listingMismatch = 0;	// postings only an nzb-file of another size was served for
		int budget = 0;
		void Add(const Stats& other)
		{
			ok += other.ok;
			fetch += other.fetch;
			parse += other.parse;
			refused += other.refused;
			relisted += other.relisted;
			listingMismatch += other.listingMismatch;
			budget += other.budget;
		}
	};

	static constexpr int CooldownSec = 30 * 60;
	static constexpr int RetryDelaySec = 2;
	static constexpr size_t MaxNzbBytes = 64 * 1024 * 1024;
	class Budget
	{
	public:
		explicit Budget(int maxDonors) : m_left(maxDonors > 0 ? (unsigned long long)maxDonors * 3 : 0), m_unlimited(maxDonors <= 0)
		{
			m_rust = nzbget_rs_dupe_budget_new(maxDonors > 0 ? (uint32_t)maxDonors : 0);
		}
		~Budget()
		{
			nzbget_rs_dupe_budget_free(m_rust);
		}
		Budget(const Budget&) = delete;
		Budget& operator=(const Budget&) = delete;
		bool Take() { return nzbget_rs_dupe_budget_take(m_rust) != 0; }
		bool Spent() const { return nzbget_rs_dupe_budget_spent(m_rust) != 0; }
		void* RustHandle() const { return m_rust; }
	private:
		void* m_rust = nullptr;
		std::atomic<unsigned long long> m_left;
		bool m_unlimited;
	};
	using FetchFunc = std::function<HttpGet::Reply(const std::string&, const std::string&, size_t, long long)>;
	NzbFetcher() : m_rust(nzbget_rs_nzb_fetcher_new()) {}
	explicit NzbFetcher(FetchFunc fetch) : m_rust(nzbget_rs_nzb_fetcher_new()), m_fetch(std::move(fetch)) {}
	~NzbFetcher()
	{
		nzbget_rs_nzb_fetcher_free(m_rust);
	}
	NzbFetcher(const NzbFetcher&) = delete;
	NzbFetcher& operator=(const NzbFetcher&) = delete;

	void SetStatePath(const std::string& path)
	{
		nzbget_rs_nzb_fetcher_set_state_path(m_rust, path.data(), path.size());
	}

	void Load()
	{
		nzbget_rs_nzb_fetcher_load(m_rust, (int64_t)Util::CurrentTime());
	}

	Fetched Fetch(const Newznab::Result& listing, time_t deadline, int retries = 1, Budget* budget = nullptr)
	{
		Fetched fetched;
		fetched.listing = listing;
		struct FetchCtx
		{
			NzbFetcher* self;
			HttpGet::Reply lastReply;
		} ctx{this, {}};
		NzbgetRsBuf dataBuf{};
		RsNzbSummaryScalars scalars{};
		int reason = nzbget_rs_nzb_fetcher_fetch(
			m_rust,
			listing.indexer.data(), listing.indexer.size(),
			listing.title.data(), listing.title.size(),
			listing.link.data(), listing.link.size(),
			listing.size, listing.grabs, (int64_t)listing.date,
			(int64_t)deadline, retries,
			budget ? budget->RustHandle() : nullptr,
			&ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void*) { Util::Sleep(100); },
			[](void* rawCtx, const char* url, size_t urlLen, const char* label, size_t labelLen,
				size_t maxBytes, int64_t deadlineMs, int* outStatus,
				const char** outBodyPtr, size_t* outBodyLen) -> int {
				auto* c = static_cast<FetchCtx*>(rawCtx);
				std::string urlStr(url ? url : "", urlLen);
				std::string labelStr(label ? label : "", labelLen);
				c->lastReply = c->self->m_fetch
					? c->self->m_fetch(urlStr, labelStr, maxBytes, deadlineMs)
					: HttpGet::Fetch(urlStr, labelStr, maxBytes, deadlineMs);
				*outStatus = c->lastReply.status;
				*outBodyPtr = c->lastReply.body.empty() ? nullptr : c->lastReply.body.data();
				*outBodyLen = c->lastReply.body.size();
				return c->lastReply.ok ? 1 : 0;
			},
			[](void*, const char* msg) {
				info("%s", msg ? msg : "");
			},
			[](void*, const char* msg) {
				warn("%s", msg ? msg : "");
			},
			&dataBuf,
			&scalars,
			&fetched.info,
			NzbReader::OnStrings,
			NzbReader::OnSlice,
			NzbReader::OnRecoveryBytes);
		fetched.reason = static_cast<EReason>(reason);
		if (fetched.reason == frOk && dataBuf.data)
		{
			fetched.data.assign(dataBuf.data, dataBuf.len);
			NzbReader::ApplyScalars(fetched.info, scalars);
		}
		nzbget_rs_free(dataBuf);
		return fetched;
	}

	Fetched FetchPosting(const Posting::Group& listings, time_t deadline, Stats& stats, Budget* budget = nullptr)
	{
		Fetched last;
		if (listings.empty()) { stats.parse++; return last; }
		last.listing = listings.front();
		bool haveOther = false;
		Fetched other;

		for (size_t i = 0; i < listings.size(); i++)
		{
			Fetched fetched = Fetch(listings[i], deadline, 1, budget);
			if (fetched.reason == frBudget) { stats.budget++; last = std::move(fetched); break; }
			if (fetched.reason == frOk && !Posting::ListingMismatch(listings[i], fetched.info.totalBytes))
			{
				stats.ok++;
				stats.relisted += (int)(listings.size() - i - 1);
				return fetched;
			}
			if (fetched.reason == frOk)
			{
				if (!haveOther)
				{
					other = std::move(fetched);
					haveOther = true;
				}
				continue;
			}
			switch (fetched.reason)
			{
				case frFetch: stats.fetch++; break;
				case frParse: stats.parse++; break;
				case frRefused: stats.refused++; break;
				default: break;
			}
			last = std::move(fetched);
		}

		if (haveOther)
		{
			stats.ok++;
			stats.listingMismatch++;
			return other;
		}
		return last;
	}

	bool IsRefused(const std::string& indexer)
	{
		return nzbget_rs_nzb_fetcher_is_refused(
			m_rust, indexer.data(), indexer.size(), (int64_t)Util::CurrentTime()) != 0;
	}

private:
	void* m_rust = nullptr;
	FetchFunc m_fetch;
};

/*
 * Option <DupeSearch>: looks for other postings of a new download on a
 * Newznab indexer and queues them as duplicates (see DupeSearchJob).
 *
 * This part decides WHICH downloads are searched. A download added to the
 * queue is noted, and after <DupeSearchDelay> seconds - a client that
 * submits backups of its own has added them by then - it is searched if it
 * is still the top-scored item of its duplicate key, hasn't passed the
 * download stage, isn't a duplicate this search queued itself, and its key
 * wasn't searched lately (at most one search per key within SearchWindowSec
 * unless a pick with a higher score arrives). What was searched is kept
 * across restarts, and a restart looks at the queue once for downloads whose
 * add event it missed.
 */
class DupeSearch : public Thread
{
public:
	DupeSearch();
	~DupeSearch() override;
	void Run() override;
	void Stop() override;
    void NotifyNameChanged(int nzbId);

	// seconds a key is not searched again, unless a higher-scored pick arrives
	static constexpr int SearchWindowSec = 6 * 3600;
	// a pick still in post-processing when its search is due is asked again this much later
	static constexpr int PostProcessRetrySec = 30;
	// a pick that failed this recently is searched from history at start (B64)
	static constexpr int RecentFailureSec = 3600;
	// keys are forgotten after this long
	static constexpr int StateTtlSec = 30 * 24 * 3600;
	// the score of a managed pick, and the room below it for donors
	static constexpr int BasePickScore = 1000000;
	// search and fetch of one pick are meant to fit in this long
	static constexpr int SearchDeadlineSec = 60;

	// parameters of the duplicates this search queued: a marker, and the
	// share of the posting found alive (percent)
	static constexpr const char* DonorParam = "DupeSearch";
	static constexpr const char* AliveParam = "DupeAlive";
	// the measured share alive (percent) of a duplicate in history this search
	// ranked but didn't add (a client's own backup, an earlier donor)
	static constexpr const char* HealthParam = "DupeHealth";
    static constexpr const char* OwnedKeyParam = "DupeSearchOwnedKey";

	/* "dupes:" + the normalized title: the duplicate key of a download that has none */
	static std::string MakeDupeKey(const std::string& name);

	/* has this search queued the nzb itself, or has the nzbget-dupe-proxy
	 * (it marks its duplicates with DupeAlive)? */
	static bool IsDonor(NzbInfo* nzbInfo);

private:
	class DownloadQueueObserver final : public Observer
	{
	public:
		DupeSearch* m_owner;
		void Update(Subject* caller, void* aspect) override { m_owner->DownloadQueueUpdate(static_cast<DownloadQueue*>(caller), aspect); }
	};

	struct Searched
	{
		int score = 0;
		time_t time = 0;
		std::string title;
		bool retryable = false;
        std::string rawName; // Runtime-only normalization cache; never inferred from disk.
	};

	struct Job
	{
		int nzbId = 0;
		bool failed = false;
		bool rankOnly = false;
		std::string name;
		std::string dupeKey;
		std::string category;
		std::string imdb;	// from the nzb-file's meta data
		std::string tvdb;
		std::string queuedFile;	// the nzb-file of the pick
		std::vector<std::string> knownFiles;	// the nzb-files nzbget keeps for this release (queue and history)
		// the duplicates of the key nzbget keeps in history (DELETED/DUPE),
		// whoever sent them: id and the item's own nzb-file
		std::vector<std::pair<int, std::string>> members;
		std::vector<int> memberScores;	// their scores when the search began
		int score = 0;
	};

	// postings fetched at the same time
	static constexpr int FetchParallel = 4;

	DownloadQueueObserver m_observer;
	NzbFetcher m_fetcher;
	DeadPostings m_dead;
	std::mutex m_mutex;
	std::condition_variable m_cond;
	std::set<int> m_rankPending;
	std::map<int, time_t> m_pending;			// nzb id -> when to look at it
	std::map<std::string, Searched> m_searched;	// lowercase dupe key -> last search
    std::map<std::pair<std::string, std::string>, Searched> m_titleSearched;
	// pick id -> fingerprints its search queued; per pick, not per key: a pick sent
	// again after the user deleted it and its duplicates gets them back (B41)
	std::map<int, std::set<std::string>> m_sent;

	void DownloadQueueUpdate(DownloadQueue* downloadQueue, void* aspect);
	void Schedule(int nzbId, time_t due);
	void ScanQueue();
	bool Prepare(DownloadQueue* downloadQueue, int nzbId, Job& job);
	bool PrepareNative(DownloadQueue* downloadQueue, int nzbId, Job& job);
	void Expand(const Job& job, const NzbSummary& pick, const std::vector<Newznab::Result>& prior,
        const std::vector<Posting::Sketch>& known, time_t deadline, NzbFetcher::Budget& budget,
		std::vector<NzbFetcher::Fetched>& fetched, std::set<std::string>& attempted, NzbFetcher::Stats& stats, Newznab::SearchStats& queryStats,
		int& results, int& candidates, int& postings);
	void Search(const Job& job);
	/* health checks, adds and ranks the verified postings and the key's duplicates in history */
	void Place(const Job& job, const NzbSummary& pick, std::vector<NzbFetcher::Fetched>& verified,
		std::map<std::string, int>& rejected, int results, int candidates, int postings);
	void Collect(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, Job& job);

	/*
	 * A search in progress survives a restart: QueueDir/dupesearch-pending/<nzb id>/
	 * holds a phase file ("searching", then "fetched" once the fetched nzb-files
	 * are saved beside it as <n>.nzb and <n>.meta). At startup a fetched search
	 * resumes from the saved files (no search, no fetch: grabs are scarce); one
	 * still searching is searched again; one whose pick is gone is dropped.
	 */
	/* a line of the search of pick <nzbId>: in the pick's own log and the global log */
	void Note(int nzbId, Message::EKind kind, const char* format, ...) PRINTF_SYNTAX(4);
	std::string PendingDir(int nzbId);
	void MarkSearching(const Job& job);
	void SavePending(const Job& job, const NzbSummary& pick, const std::vector<NzbFetcher::Fetched>& verified);
	void RemovePending(int nzbId);
	void ResumePending();
	/* why the search may no longer add duplicates for the pick (deleted by the user,
	 * another duplicate key), empty while it may */
	std::string PickGone(const Job& job);
	/* queues a posting as a duplicate of the pick; the download id, 0 when not added */
	int AddDonor(const Job& job, const NzbSummary& pick, const NzbFetcher::Fetched& posting, int score, double alive);
	/* the DupeScore (and a parameter "Name=Value") of a duplicate that sits in history or in the queue */
	bool SetScore(int id, int score, const std::vector<std::string>& params);
	void LoadState();
	bool SaveState();
	// the key may be searched again (a search that learned nothing)
    // Callers hold m_mutex; the flat map is only the latest-name cache.
    Searched* FindSearch(const std::string& lowerKey, const std::string& title);
    void RecordSearch(const std::string& lowerKey, const Searched& searched);
    void SetRetryable(const std::string& lowerKey, const std::string& title, bool retryable);
	bool ForgetSearch(const std::string& dupeKey, const std::string& title = "");
	std::string StatePath();
};

extern DupeSearch* g_DupeSearch;

#include <chrono>
#include <limits>
#include <fstream>
#include <sstream>
#include <algorithm>
#include <thread>
#include "nzbget_rs.h"

class DonorScore
{
public:
	static constexpr int Dead = 1;
	static constexpr int Top = 90;

	static int Base(int pickScore)
	{
		return nzbget_rs_donorscore_base(pickScore);
	}

	static int Target(double alive, bool twin)
	{
		return nzbget_rs_donorscore_target(alive, twin ? 1 : 0);
	}

	class Ranks
	{
	public:
		int Take(double alive, bool twin)
		{
			std::vector<int32_t> used(m_used.begin(), m_used.end());
			int score = nzbget_rs_donorscore_take_rank(alive, twin ? 1 : 0, used.data(), used.size());
			m_used.insert(score);
			return score;
		}
		void Release(int score) { m_used.erase(score); }
		void Use(int score) { m_used.insert(score); }
	private:
		std::set<int> m_used;
	};

	struct Entry
	{
		int score = 0;
		double alive = -1;
		bool twin = false;
		long long bytes = 0;
		int grabs = 0;
	};

	static std::vector<int> Rerank(const std::vector<Entry>& entries, long long pickBytes)
	{
		std::vector<NzbgetRsDonorEntry> raw;
		raw.reserve(entries.size());
		for (const Entry& e : entries)
		{
			raw.push_back({e.score, e.alive, e.twin ? 1 : 0, (int64_t)e.bytes, e.grabs});
		}
		std::vector<int> scores(entries.size());
		nzbget_rs_donorscore_rerank(raw.data(), raw.size(), (int64_t)pickBytes, scores.data());
		return scores;
	}
};

class DupeIdentity
{
public:
	struct Identity
	{
		std::string name;
		std::string extension;
		unsigned long long bytes = 0;
		explicit operator bool() const { return bytes != 0 && !extension.empty(); }
	};
	using FetchFunc = std::function<ArticleFetcher::FetchedArticle(const std::string&, const std::vector<CString>&)>;

	static Identity FromArticle(const std::vector<char>& data, const std::string& name,
		unsigned long long total, unsigned long long minimum)
	{
		Identity identity;
		NzbgetRsBuf raw = nzbget_rs_dupe_inner(data.data(), data.size(), name.data(), name.size(), total, minimum);
		uint64_t bytes = 0;
		size_t extensionSize = 0;
		size_t nameOffset = 0;
		size_t nameLen = 0;
		if (nzbget_rs_dupe_identity_decode_inner_buf(
			raw.data, raw.len, &bytes, &extensionSize, &nameOffset, &nameLen) != 0)
		{
			identity.bytes = bytes;
			identity.extension.assign(raw.data + 12, extensionSize);
			identity.name.assign(raw.data + nameOffset, nameLen);
		}
		nzbget_rs_free(raw);
		return identity;
	}

	static ArticleFetcher::FetchedArticle FetchArticle(const NzbSummary::Head& head,
		const std::vector<std::string>& names, time_t deadline, const FetchFunc& fetch = FetchFunc())
	{
		struct FetchCtx
		{
			const std::vector<std::string>* names;
			const FetchFunc* fetch;
			ArticleFetcher fetcher;
			ArticleFetcher::FetchedArticle article;
		} ctx{&names, &fetch, {}, {}};

		int ok = nzbget_rs_dupe_identity_fetch_article(
			head.messageId.data(), head.messageId.size(), (int64_t)deadline, &ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void* raw, const char* bracketedId, size_t bracketedLen,
			   int* outSuccess, int64_t* outOffset, int64_t* outFileSize) -> int {
				auto* c = static_cast<FetchCtx*>(raw);
				std::vector<CString> groups;
				for (const std::string& name : *c->names) groups.emplace_back(name.c_str());
				std::string id(bracketedId ? bracketedId : "", bracketedLen);
				try
				{
					c->article = (*c->fetch) ? (*c->fetch)(id, groups) : c->fetcher.Fetch(id.c_str(), groups);
				}
				catch (...)
				{
					c->article = ArticleFetcher::FetchedArticle();
					return 0;
				}
				*outSuccess = c->article.Success ? 1 : 0;
				*outOffset = c->article.Offset;
				*outFileSize = c->article.FileSize;
				return 1;
			},
			[](void* raw) {
				static_cast<FetchCtx*>(raw)->fetcher.Stop();
			});
		if (!ok)
			return ArticleFetcher::FetchedArticle();
		return ctx.article;
	}

	static Identity Read(const NzbSummary& info, time_t deadline, const FetchFunc& fetch = FetchFunc())
	{
		std::vector<const char*> headNames(info.heads.size());
		std::vector<size_t> headNameLens(info.heads.size());
		std::vector<const char*> headMsgIds(info.heads.size());
		std::vector<size_t> headMsgIdLens(info.heads.size());
		std::vector<int64_t> headBytes(info.heads.size());
		for (size_t i = 0; i < info.heads.size(); ++i)
		{
			headNames[i] = info.heads[i].name.data();
			headNameLens[i] = info.heads[i].name.size();
			headMsgIds[i] = info.heads[i].messageId.data();
			headMsgIdLens[i] = info.heads[i].messageId.size();
			headBytes[i] = info.heads[i].bytes;
		}
		struct ReadCtx
		{
			const NzbSummary* info;
			const FetchFunc* fetch;
			ArticleFetcher::FetchedArticle lastArticle;
		} ctx{&info, &fetch, {}};

		NzbgetRsBuf nameBuf{nullptr, 0, 0};
		NzbgetRsBuf extBuf{nullptr, 0, 0};
		uint64_t bytes = 0;
		int found = nzbget_rs_dupe_identity_read(
			info.dataBytes,
			headNames.empty() ? nullptr : headNames.data(),
			headNameLens.empty() ? nullptr : headNameLens.data(),
			headMsgIds.empty() ? nullptr : headMsgIds.data(),
			headMsgIdLens.empty() ? nullptr : headMsgIdLens.data(),
			headBytes.empty() ? nullptr : headBytes.data(),
			info.heads.size(), (int64_t)deadline, &ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void* raw, const char* msgId, size_t msgIdLen, int64_t dl,
			   int* outSuccess, int64_t* outFileSize,
			   const char** outDataPtr, size_t* outDataLen) {
				auto* c = static_cast<ReadCtx*>(raw);
				NzbSummary::Head head;
				head.messageId.assign(msgId ? msgId : "", msgIdLen);
				c->lastArticle = FetchArticle(head, c->info->groups, (time_t)dl, *c->fetch);
				*outSuccess = c->lastArticle.Success ? 1 : 0;
				*outFileSize = c->lastArticle.FileSize;
				*outDataPtr = c->lastArticle.Data.empty() ? nullptr : c->lastArticle.Data.data();
				*outDataLen = c->lastArticle.Data.size();
			},
			&nameBuf, &extBuf, &bytes);
		Identity identity;
		if (found)
		{
			if (nameBuf.data && nameBuf.len > 0) identity.name.assign(nameBuf.data, nameBuf.len);
			if (extBuf.data && extBuf.len > 0) identity.extension.assign(extBuf.data, extBuf.len);
			identity.bytes = bytes;
		}
		nzbget_rs_free(nameBuf);
		nzbget_rs_free(extBuf);
		return identity;
	}

	static unsigned long long WholeBytes(const NzbSummary& info)
	{
		if (info.totalBytes <= 0) return 0;
		return nzbget_rs_dupe_whole_bytes((uint64_t)info.totalBytes, info.listedParts, info.declaredParts);
	}

	static double Alive(const DonorHealth::Health& health, const NzbSummary& info)
	{
		return nzbget_rs_dupe_effective_alive(health.present, health.missing, health.error, info.listedParts, info.declaredParts);
	}

	static bool IsDead(const DonorHealth::Health& health, const NzbSummary& info, int minimum)
	{
		return nzbget_rs_dupe_is_dead(health.present, health.missing, health.error, info.listedParts, info.declaredParts, minimum) != 0;
	}

	static int UnlistedMissing(const DonorHealth::Health& health, const NzbSummary& info)
	{
		return nzbget_rs_dupe_unlisted_missing(health.Answered(), info.listedParts, info.declaredParts);
	}

	static bool ParDoomed(const NzbSummary& info, double alive, time_t deadline)
	{
		if (info.parIndex.messageId.empty() || info.recoveryBytes.empty() || info.dataBytes <= 0 || info.articleBytes == 0) return false;
		auto article = FetchArticle(info.parIndex, info.groups, deadline);
		if (!article.Success) return false;
		uint64_t block = nzbget_rs_dupe_par_block(article.Data.data(), article.Data.size());
		return nzbget_rs_dupe_par_doomed((uint64_t)info.dataBytes, info.articleBytes,
			info.recoveryBytes.data(), info.recoveryBytes.size(), alive, block) != 0;
	}

	static bool OuterCompatible(const std::string& title, const std::string& name)
	{
		return nzbget_rs_dupe_outer_ok(title.data(), title.size(), name.data(), name.size()) != 0;
	}

	static bool SameData(const NzbSummary& a, const NzbSummary& b)
	{
		if (a.dataBytes <= 0 || b.dataBytes <= 0 || a.dataFiles <= 0 || b.dataFiles <= 0) return false;
		return nzbget_rs_dupe_same_data((uint64_t)a.dataBytes, (uint32_t)a.dataFiles,
			(uint64_t)b.dataBytes, (uint32_t)b.dataFiles) != 0;
	}

	static bool Candidate(const std::string& title, long long bytes, const std::string& name, long long size)
	{
		return nzbget_rs_dupe_candidate(title.data(), title.size(), bytes > 0 ? bytes : 0,
			name.data(), name.size(), size > 0 ? size : 0) != 0;
	}

	static bool Matches(const std::string& title, const std::string& mainName,
		const Identity& mine, const Identity& theirs)
	{
		NzbgetRsDupeIdentityInput a{ mine.bytes, mine.name.data(), mine.name.size(), mine.extension.data(), mine.extension.size() };
		NzbgetRsDupeIdentityInput b{ theirs.bytes, theirs.name.data(), theirs.name.size(), theirs.extension.data(), theirs.extension.size() };
		return nzbget_rs_dupe_lenient_ok(title.data(), title.size(), mainName.data(), mainName.size(),
			mine ? &a : nullptr, theirs ? &b : nullptr) != 0;
	}

	static std::vector<std::string> Hints(const NzbSummary& pick, const NzbSummary& donor, bool sameInner)
	{
		std::vector<std::string> hints;
		if (SameData(pick, donor)) hints.emplace_back("DupeSameSize=1");
		if (sameInner) hints.emplace_back("DupeSameInner=1");
		return hints;
	}
};

inline DupeSearch* g_DupeSearch = nullptr;

namespace
{
// a download by id, in the queue or else in history
NzbInfo* FindNzb(DownloadQueue* downloadQueue, int nzbId)
{
	if (NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(nzbId))
	{
		return nzbInfo;
	}
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == nzbId)
		{
			return historyInfo->GetNzbInfo();
		}
	}
	return nullptr;
}

bool SavedHistoryBackup(NzbInfo* pick, NzbInfo* candidate)
{
    return nzbget_rs_df_saved_history_backup(
        pick != nullptr ? 1 : 0, pick ? pick->GetFullContentHash() : 0,
        (int)candidate->GetDeleteStatus(), candidate->GetFullContentHash(),
        (int)NzbInfo::dsDupe, (int)NzbInfo::dsCopy) != 0;
}

bool PlacementAvailable(DownloadQueue* queue, NzbInfo* pick, NzbInfo* backup, bool wholeBackup = false)
{
    if (!backup || backup->GetKind() != NzbInfo::nkNzb || backup->GetDeleting() ||
        backup->GetDeleteStatus() == NzbInfo::dsManual || backup->GetMarkStatus() == NzbInfo::ksBad ||
        backup->GetDupeMode() == dmForce) return false;
    bool inHistory = queue->GetQueue()->Find(backup->GetId()) != backup;
    bool isDupeSuccess = inHistory && backup->IsDupeSuccess();
    bool hasPostInfo = backup->GetPostInfo() != nullptr;
    bool savedBackup = SavedHistoryBackup(pick, backup);
    bool needDiskAndHealth = inHistory && !isDupeSuccess && (!wholeBackup || !hasPostInfo) && savedBackup;
    bool queuedFileExists = needDiskAndHealth && FileSystem::FileExists(backup->GetQueuedFilename());
    int health = (needDiskAndHealth && queuedFileExists && wholeBackup) ? backup->CalcHealth() : 0;
    int critHealth = (needDiskAndHealth && queuedFileExists && wholeBackup) ? backup->CalcCriticalHealth(true) : 0;
    NzbParameter* markedAlive = backup->GetParameters()->Find(DupeSearch::AliveParam);
    return nzbget_rs_df_placement_available(
        1, 1, 0, (int)backup->GetDeleteStatus(),
        (int)NzbInfo::dsNone, (int)NzbInfo::dsManual, (int)NzbInfo::dsDupe, (int)NzbInfo::dsCopy,
        (int)backup->GetMarkStatus(), (int)NzbInfo::ksBad,
        (int)backup->GetDupeMode(), (int)dmForce,
        inHistory ? 1 : 0, wholeBackup ? 1 : 0, hasPostInfo ? 1 : 0, isDupeSuccess ? 1 : 0,
        pick != nullptr ? 1 : 0, pick ? pick->GetFullContentHash() : 0, backup->GetFullContentHash(),
        queuedFileExists ? 1 : 0, health, critHealth,
        g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0,
        markedAlive != nullptr ? 1 : 0,
        markedAlive ? nzbget_rs_dqh_parse_c_atoi(markedAlive->GetValue()) : 0) != 0;
}


std::string LowerKey(const std::string& key)
{
	NzbgetRsBuf buf = nzbget_rs_df_lower_key(key.c_str());
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

// the key a download is searched under: its own, or one made from its name
std::string EffectiveKey(NzbInfo* nzbInfo)
{
	if (!Util::EmptyStr(nzbInfo->GetDupeKey()))
	{
		return nzbInfo->GetDupeKey();
	}
	return DupeSearch::MakeDupeKey(nzbInfo->GetName());
}

}

inline std::string DupeSearch::MakeDupeKey(const std::string& name)
{
	return "dupes:" + ReleaseName::Normalize(name);
}

inline bool DupeSearch::IsDonor(NzbInfo* nzbInfo)
{
	return nzbInfo->GetParameters()->Find(DonorParam) || nzbInfo->GetParameters()->Find(AliveParam);
}

inline DupeSearch::DupeSearch()
{
	HttpGet::Reset();
	DonorHealth::Reset();
	m_observer.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_observer);
}

inline DupeSearch::~DupeSearch()
{
	debug("Destroying DupeSearch");
	// the queue (QueueCoordinator's) outlives this, and would notify freed memory
	DownloadQueue::Guard()->Detach(&m_observer);
}

inline void DupeSearch::Stop()
{
	Thread::Stop();
	HttpGet::StopAll();
	DonorHealth::StopAll();
	std::lock_guard<std::mutex> guard(m_mutex);
	m_cond.notify_all();
}

inline void DupeSearch::NotifyNameChanged(int nzbId)
{
    if (g_Options->GetDupeSearch() && !IsStopped())
        Schedule(nzbId, Util::CurrentTime() + g_Options->GetDupeSearchDelay());
}

inline void DupeSearch::DownloadQueueUpdate(DownloadQueue* downloadQueue, void* aspect)
{
	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if ((queueAspect->action == DownloadQueue::eaNzbAdded || queueAspect->action == DownloadQueue::eaNzbNamed) &&
		queueAspect->nzbInfo && g_Options->GetDupeSearch() && !IsStopped())
	{
		Schedule(queueAspect->nzbInfo->GetId(), Util::CurrentTime() + g_Options->GetDupeSearchDelay());
        // An external lower-scored backup can arrive after the initial search.
        // Queue notifications run under the queue lock; only schedule work here.
        NzbInfo* added = queueAspect->nzbInfo;
        if (nzbget_rs_dqh_dupesearch_should_check_added_for_parent_rank(
            (int)queueAspect->action,
            (int)DownloadQueue::eaNzbAdded,
            IsDonor(added) ? 1 : 0,
            (int)added->GetDupeMode(),
            (int)dmScore,
            added->GetParameters()->Find("DupeFleet") != nullptr ? 1 : 0) != 0)
        {
            NzbInfo* parent = nullptr;
            std::string addedKey = LowerKey(EffectiveKey(added));
            for (NzbInfo* item : downloadQueue->GetQueue())
            {
                bool sameLowerKey = item != added && LowerKey(EffectiveKey(item)) == addedKey;
                if (nzbget_rs_dqh_dupesearch_is_better_parent_candidate(
                    item == added ? 1 : 0,
                    (int)item->GetKind(),
                    (int)NzbInfo::nkNzb,
                    item->GetDeleting() ? 1 : 0,
                    (int)item->GetDeleteStatus(),
                    (int)NzbInfo::dsNone,
                    item->GetPostInfo() != nullptr ? 1 : 0,
                    sameLowerKey ? 1 : 0,
                    item->GetDupeScore(),
                    added->GetDupeScore(),
                    parent != nullptr ? 1 : 0,
                    parent ? parent->GetDupeScore() : 0) != 0)
                {
                    parent = item;
                }
            }
            if (parent)
            {
                std::lock_guard<std::mutex> guard(m_mutex);
                m_rankPending.insert(parent->GetId());
                m_pending[parent->GetId()] = Util::CurrentTime() + g_Options->GetDupeSearchDelay();
                m_cond.notify_all();
            }
        }
	}
}

inline void DupeSearch::Schedule(int nzbId, time_t due)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	m_pending[nzbId] = due;
	m_cond.notify_all();
}

// an event isn't fired for a download that was queued before a restart; nor for
// a pick that failed over while nzbget was stopping or starting (B64): one that
// failed lately is asked again (Prepare searches it from history once, unless
// its key was searched already)
inline void DupeSearch::ScanQueue()
{
	std::vector<int> ids;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetKind() == NzbInfo::nkNzb)
			{
				ids.push_back(nzbInfo->GetId());
			}
		}
		time_t recent = Util::CurrentTime() - RecentFailureSec;
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
				historyInfo->GetTime() >= recent &&
				!historyInfo->GetNzbInfo()->IsDupeSuccess())
			{
				ids.push_back(historyInfo->GetNzbInfo()->GetId());
			}
		}
	}
	for (int id : ids)
	{
		Schedule(id, Util::CurrentTime() + g_Options->GetDupeSearchDelay());
	}
}

inline void DupeSearch::Run()
{
	debug("Entering DupeSearch-loop");

	while (!DownloadQueue::IsLoaded() && !IsStopped())
	{
		Util::Sleep(20);
	}
	if (IsStopped())
	{
		return;
	}

	LoadState();
	m_fetcher.SetStatePath(std::string(g_Options->GetQueueDir()) + PATH_SEPARATOR + "dupesearch-indexers");
	m_fetcher.Load();
	m_dead.SetStatePath(std::string(g_Options->GetQueueDir()) + PATH_SEPARATOR + "dupesearch-dead");
	m_dead.Load();
	if (g_Options->GetDupeSearch())
	{
		ResumePending();
		ScanQueue();
	}

	while (!IsStopped())
	{
		std::vector<int> due;
		{
			std::unique_lock<std::mutex> lock(m_mutex);
			m_cond.wait_for(lock, std::chrono::milliseconds(500));
			time_t now = Util::CurrentTime();
			for (auto it = m_pending.begin(); it != m_pending.end();)
			{
				if (it->second <= now)
				{
					due.push_back(it->first);
					it = m_pending.erase(it);
				}
				else
				{
					++it;
				}
			}
		}

		for (int nzbId : due)
		{
			if (IsStopped())
			{
				break;
			}
			Job job;
			bool search;
			{
				GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
				search = Prepare(downloadQueue, nzbId, job);
			}
			if (search)
			{
				MarkSearching(job);
				Search(job);
			}
		}
	}

	// a health check's walkers may still be in a request: they end before this
	// thread does, so before the server pool goes away
	DonorHealth::WaitAll();
	debug("Exiting DupeSearch-loop");
}

/*
 * Decides whether a download is searched now. Runs under the queue lock.
 */
inline bool DupeSearch::Prepare(DownloadQueue* downloadQueue, int nzbId, Job& job)
{
    return PrepareNative(downloadQueue, nzbId, job);
}

inline bool DupeSearch::PrepareNative(DownloadQueue* downloadQueue, int nzbId, Job& job)
{
    if (!g_Options->GetDupeSearch() || !g_Options->GetDupeCheck()) return false;
    NzbInfo* pick = FindNzb(downloadQueue, nzbId);
    if (!pick) { std::lock_guard<std::mutex> guard(m_mutex); m_rankPending.erase(nzbId); return false; }
    bool history = downloadQueue->GetQueue()->Find(nzbId) != pick;
    std::string key = EffectiveKey(pick);
    std::string lowerKey = LowerKey(key);
    const std::string ownerPrefix = std::to_string(nzbId) + ":";
    NzbParameter* owner = pick->GetParameters()->Find(OwnedKeyParam);
    bool generatedKey = nzbget_rs_dqh_dupesearch_is_generated_key(
        Util::EmptyStr(pick->GetDupeKey()) ? 1 : 0,
        owner != nullptr ? 1 : 0,
        owner ? owner->GetValue() : nullptr,
        nzbId,
        key.c_str()) != 0;
    std::string title;
    Searched previous;
    bool havePrevious = false, changedIdentity = false, rankOnly = false, cachedName = false, rankRequested = false;
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        auto found = m_searched.find(lowerKey);
        if (found != m_searched.end()) {
            previous = found->second;
            havePrevious = true;
            // Missing legacy title is ambiguous unless this exact NZB carries
            // the persisted marker written when native discovery generated it.
            if (previous.title.empty() && generatedKey && key.compare(0, 6, "dupes:")==0)
                previous.title = key.substr(6);
            cachedName = !previous.title.empty() && previous.rawName == pick->GetName();
        }
        rankRequested = m_rankPending.erase(nzbId) != 0;
    }
    title = cachedName ? previous.title :
        Util::EmptyStr(pick->GetDupeKey()) ? key.substr(6) : ReleaseName::Normalize(pick->GetName());
    changedIdentity = havePrevious && !previous.title.empty() && previous.title != title;
    bool ownedKeyChanged = false;
    // Only rewrite a key this search previously generated; explicit client keys stay.
    if (generatedKey && !Util::EmptyStr(pick->GetDupeKey()) &&
        key.compare(0, 6, "dupes:") == 0 && key != "dupes:" + title)
    {
        key = "dupes:" + title;
        lowerKey = LowerKey(key);
        ownedKeyChanged = true;
        havePrevious = false;
    }
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        if (Searched* found = FindSearch(lowerKey, title))
        {
            previous = *found;
            havePrevious = true;
            changedIdentity = false;
            if (!previous.title.empty()) previous.rawName = pick->GetName();
            cachedName = !previous.title.empty();
            *found = previous;
            m_searched[lowerKey] = previous;
            rankOnly = rankRequested && previous.title == title;
        }
        else { havePrevious = false; cachedName = false; rankOnly = false; }
    }
    if (ownedKeyChanged)
    {
        rankOnly = false;
        if (!g_Options->GetDupeSearchDryRun())
        {
            pick->SetDupeKey(key.c_str());
            pick->GetParameters()->SetParameter(OwnedKeyParam, (ownerPrefix + key).c_str());
            pick->SetChanged(true);
            if (history) { downloadQueue->HistoryChanged(); downloadQueue->Save(); }
            else downloadQueue->SaveChanged();
        }
    }
    auto flags = [&](NzbInfo* item, bool inHistory) -> uint32_t {
        return nzbget_rs_dupe_watch_build_item_flags(
            (int)item->GetKind(),
            inHistory ? 1 : 0,
            item->IsDupeSuccess() ? 1 : 0,
            item->GetDeleting() ? 1 : 0,
            (int)item->GetDeleteStatus(),
            item->GetPostInfo() ? 1 : 0,
            (int)item->GetDupeMode(),
            IsDonor(item) ? 1 : 0,
            (int)item->GetDupeHint(),
            item->GetParameters()->Find("DupeFleet") ? 1 : 0);
    };
    uint32_t pickFlags = flags(pick, history);
    time_t now = Util::CurrentTime();
    const uint32_t preliminaryFlags = pickFlags;
    int preliminary = -1;
    if (cachedName && havePrevious && !previous.retryable && !rankOnly)
    {
        // A known unchanged title can be suppressed before building snapshots.
        // Any higher peer/fleet could only suppress it too; a Search still needs
        // the complete peer check below. Post-process retry precedes peer rules.
        preliminary = nzbget_rs_dupe_watch_stamped(pickFlags, pick->GetDupeScore(),
            (int64_t)now, previous.score, (int64_t)previous.time);
        if (preliminary == 0) return false;
        if (preliminary == 1) {
            Schedule(nzbId, now + std::max(PostProcessRetrySec, g_Options->GetDupeSearchDelay()));
            return false;
        }
    }
    NzbgetRsDupeWatchPeer snapshot{ nzbId, pick->GetDupeScore(), pickFlags, lowerKey.data(), lowerKey.size() };
    std::vector<NzbInfo*> others;
    std::vector<bool> histories;
    auto retain = [&](NzbInfo* item, bool inHistory) {
        // Lower/equal ordinary peers cannot affect the Rust highest-score rule.
        // Fleet markers still matter at any score, including history entries.
        if (nzbget_rs_dqh_dupesearch_should_retain_peer(
            item == pick ? 1 : 0,
            item->GetDupeScore(),
            pick->GetDupeScore(),
            item->GetParameters()->Find("DupeFleet") != nullptr ? 1 : 0) != 0)
        { others.push_back(item); histories.push_back(inHistory); }
    };
    for (NzbInfo* item : downloadQueue->GetQueue()) retain(item, false);
    for (HistoryInfo* item : downloadQueue->GetHistory()) if (item->GetKind() == HistoryInfo::hkNzb)
        retain(item->GetNzbInfo(), true);
    std::vector<std::string> keys;
    keys.reserve(others.size());
    for (NzbInfo* other : others) keys.push_back(LowerKey(EffectiveKey(other)));
    std::vector<NzbgetRsDupeWatchPeer> peers;
    for (size_t i = 0; i < others.size(); i++)
    {
        uint32_t f = flags(others[i], histories[i]);
        // Only a live eligible fleet member represents current management;
        // retired history/deleting entries must not poison future plain picks.
        if (nzbget_rs_dupe_watch_is_live_fleet_peer(
            keys[i].data(), keys[i].size(), lowerKey.data(), lowerKey.size(), f))
            pickFlags |= rsDupeManagedFleet;
        peers.push_back({ others[i]->GetId(), others[i]->GetDupeScore(), f, keys[i].data(), keys[i].size() });
    }
    snapshot.flags = pickFlags;
    // With no retained peers the snapshot is unchanged: the accepted early
    // decision already checked the exact same stamp, time and identity.
    int decision = preliminary == 2 && peers.empty() && snapshot.flags == preliminaryFlags ? preliminary :
        nzbget_rs_dupe_watch(&snapshot, peers.data(), peers.size(), (int64_t)now,
        previous.score, (int64_t)previous.time, havePrevious && !previous.retryable && !rankOnly, changedIdentity);
    if (decision == 1) {
        if (rankOnly) { std::lock_guard<std::mutex> guard(m_mutex); m_rankPending.insert(nzbId); }
        Schedule(nzbId, now + std::max(PostProcessRetrySec, g_Options->GetDupeSearchDelay()));
        return false;
    }
    if (decision != 2) return false;
    int score = std::max(pick->GetDupeScore(), BasePickScore);
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        if (!rankOnly) RecordSearch(lowerKey, { score, now, title, false, pick->GetName() });
    }
    if (!g_Options->GetDupeSearchDryRun() && !rankOnly) SaveState();
    bool edit = !g_Options->GetDupeSearchDryRun() && !history;
    if (edit)
    {
        pick->SetDupeKey(key.c_str());
        pick->GetParameters()->SetParameter(OwnedKeyParam, generatedKey ? (ownerPrefix + key).c_str() : nullptr);
        if (pick->GetDupeScore() < BasePickScore) pick->SetDupeScore(BasePickScore);
        pick->SetChanged(true);
        downloadQueue->SaveChanged();
    }
    job.rankOnly = rankOnly;
    Collect(downloadQueue, pick, job);
    job.dupeKey = key;
    job.score = score;
    job.failed = history;
    job.rankOnly = rankOnly;
    pick->PrintMessage(Message::mkInfo, "DupeSearch: searching duplicates of %s (key %s)%s",
        pick->GetName(), key.c_str(),
        nzbget_rs_dqh_dupesearch_prepare_status_suffix(rankOnly ? 1 : 0, history ? 1 : 0));
    return true;
}

/*
 * What a search of a pick needs to know of the queue and history. Runs under the queue lock.
 */
inline void DupeSearch::Collect(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, Job& job)
{
	std::string key = EffectiveKey(nzbInfo);
	std::string lowerKey = LowerKey(key);
	job.nzbId = nzbInfo->GetId();
	job.name = nzbInfo->GetName();
	job.dupeKey = key;
	job.category = nzbInfo->GetCategory();
	job.queuedFile = nzbInfo->GetQueuedFilename() ? nzbInfo->GetQueuedFilename() : "";

	// the nzb-files nzbget keeps for this release: queue and history items
	// with this duplicate key, or whose name is the same release
	auto addKnown = [&](NzbInfo* item)
	{
		// one the user deleted is no duplicate nzbget would fetch: its posting may
		// be found and added again (B41)
		if (item->GetKind() != NzbInfo::nkNzb ||
			Util::EmptyStr(item->GetQueuedFilename()) ||
			item->GetDeleteStatus() == NzbInfo::dsManual)
		{
			return;
		}
		if (LowerKey(EffectiveKey(item)) == lowerKey || ReleaseName::SameRelease(nzbInfo->GetName(), item->GetName()))
		{
			// the file of a merged group names several, separated by "|"
			const char* qf = item->GetQueuedFilename() ? item->GetQueuedFilename() : "";
			nzbget_rs_dqh_fleet_visit_pipe_names(qf, strlen(qf), &job.knownFiles,
				[](void* ctx, const char* namePtr, size_t nameLen)
				{
					static_cast<std::vector<std::string>*>(ctx)->emplace_back(namePtr, nameLen);
				});
		}
	};
	for (NzbInfo* item : downloadQueue->GetQueue())
	{
		addKnown(item);
        // Zero-delay ranking can precede asynchronous duplicate filing. The
        // score editor handles either live or history IDs, so include that
        // snapshot rather than losing the only add notification.
        if (nzbget_rs_dupesearch_is_rank_only_queued_member(
            job.rankOnly ? 1 : 0,
            item == nzbInfo ? 1 : 0,
            (int)item->GetKind(),
            (int)NzbInfo::nkNzb,
            (int)item->GetDupeMode(),
            (int)dmScore,
            item->GetPostInfo() != nullptr ? 1 : 0,
            (int)item->GetDeleteStatus(),
            (int)NzbInfo::dsNone,
            (int)NzbInfo::dsDupe,
            item->GetDupeScore(),
            nzbInfo->GetDupeScore()) != 0 &&
            LowerKey(EffectiveKey(item))==lowerKey &&
            ReleaseName::SameRelease(nzbInfo->GetName(),item->GetName()) &&
            !Util::EmptyStr(item->GetQueuedFilename()) &&
            strchr(item->GetQueuedFilename(), '|') == nullptr)
        {
            job.members.emplace_back(item->GetId(),item->GetQueuedFilename());
            job.memberScores.push_back(item->GetDupeScore());
        }
	}
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			addKnown(item);
			// the same release only: a client's key can be broad (one key for two editions);
			// a copy (an nzb-file sent again) too, unless it is a copy of the pick
			bool backup = SavedHistoryBackup(nzbInfo, item);
			if (item != nzbInfo && backup && LowerKey(EffectiveKey(item)) == lowerKey &&
				ReleaseName::SameRelease(nzbInfo->GetName(), item->GetName()) &&
				!Util::EmptyStr(item->GetQueuedFilename()) &&
				strchr(item->GetQueuedFilename(), '|') == nullptr)
			{
				job.members.emplace_back(item->GetId(), item->GetQueuedFilename());
				job.memberScores.push_back(item->GetDupeScore());
			}
		}
	}
}

inline void DupeSearch::Expand(const Job& job, const NzbSummary& pick, const std::vector<Newznab::Result>& prior,
    const std::vector<Posting::Sketch>& known, time_t deadline, NzbFetcher::Budget& budget, std::vector<NzbFetcher::Fetched>& fetched,
    std::set<std::string>& attempted, NzbFetcher::Stats& stats, Newznab::SearchStats& queryStats, int& results, int& candidates, int& postings)
{
    if (IsStopped() || Util::CurrentTime() >= deadline || budget.Spent() || pick.messageIds.empty()) return;
    auto servers=NntpHealthServer::Servers();
    if (servers.empty()) return;
    auto healthOptions = [&]() {
        DonorHealth::Options options;
        options.sample.percent=g_Options->GetDupeHealthPercent();
        options.sample.minimum=g_Options->GetDupeHealthMin();
        options.sample.maximum=g_Options->GetDupeHealthMax();
        options.sample.maxBody=g_Options->GetDupeBodyChecks();
        options.budgetMs=nzbget_rs_dupesearch_calc_expand_budget_ms(
            g_Options->GetDupeHealthBudget(), (int64_t)deadline, (int64_t)Util::CurrentTime());
        return options;
    };
    auto check = [&](const NzbSummary& info) {
        return DonorHealth::CheckPosting(servers,info.messageIds,
            std::make_shared<std::vector<std::string>>(info.groups),healthOptions(),true);
    };
    DonorHealth::Health health;
    double alive=-1;
    bool doomed=false;
    int missing=0;
    if (!job.failed)
    {
        health=check(pick);
        alive=DupeIdentity::Alive(health,pick);
        long long combined=(long long)health.missing+DupeIdentity::UnlistedMissing(health,pick);
        missing=(int)std::min(combined,(long long)std::numeric_limits<int>::max());
        if (alive >= 0.85 && !pick.dataMessageIds.empty() &&
            !pick.parIndex.messageId.empty() && !pick.recoveryBytes.empty())
        {
            // Recovery articles are capacity, never evidence that source data
            // exists. Sample only retained data IDs for damaged-block estimates.
            NzbSummary data = pick;
            data.messageIds = pick.dataMessageIds;
            data.listedParts = pick.dataListedParts;
            data.declaredParts = pick.dataDeclaredParts;
            auto dataHealth = check(data);
            double dataAlive = DupeIdentity::Alive(dataHealth, data);
            long long dataMissing = (long long)dataHealth.missing + DupeIdentity::UnlistedMissing(dataHealth, data);
            if (dataAlive >= 0.0 && dataMissing >= DonorHealth::MinKnown)
                doomed = DupeIdentity::ParDoomed(pick, dataAlive, deadline);
        }
    }
    if (!nzbget_rs_dupe_should_expand(alive,missing,job.failed || doomed,false)) return;
    // Do not spend indexer grabs when a usable whole backup already exists.
    std::set<std::string> probed;
    auto whole=[&](const NzbSummary& info) {
        // A failed pick is not its own backup, even if all of its articles exist.
        // Match the same-posting rejection used later before adding donors.
        if (info.Fingerprint()==pick.Fingerprint() || Posting::SamePosting(pick.messageIds,info.messageIds)) return false;
        if (Util::CurrentTime()>=deadline || IsStopped() || !probed.insert(info.Fingerprint()).second) return false;
        auto h=check(info);
        return DupeIdentity::Alive(h,info)>=0.999;
    };
    for (const auto& posting : fetched)
    {
        // Anything rejected as already held or known-dead later cannot be a
        // newly usable backup. Eligible held backups are checked below.
        Posting::Sketch sketch=Posting::MakeSketch(posting.info.messageIds);
        if (m_dead.IsDead(sketch) || std::any_of(known.begin(),known.end(),
            [&](const Posting::Sketch& k) { return Posting::SameSketch(sketch,k); })) continue;
        if ((!ReleaseName::Readable(posting.info.mainName) || ReleaseName::SameRelease(job.name,posting.info.mainName)) && whole(posting.info)) return;
    }
    auto memberAvailable = [&](int id) {
        GuardedDownloadQueue queue = DownloadQueue::Guard();
        NzbInfo* currentPick = FindNzb(queue, job.nzbId);
        return currentPick && PlacementAvailable(queue, currentPick, FindNzb(queue, id), true);
    };
    for (const auto& member : job.members)
    {
        // Ineligible members must not consume the posting's probe slot. Queue
        // ownership can change during parsing or NNTP, so check it again after.
        if (!memberAvailable(member.first)) continue;
        NzbSummary info;
        if (NzbReader::Parse(DupeUtil::ReadAll(member.second),info) && whole(info))
        {
            if (memberAvailable(member.first)) return;
            probed.erase(info.Fingerprint());
        }
    }
    if (IsStopped() || budget.Spent() || Util::CurrentTime()>=deadline) return;
    std::vector<Newznab::Params> queries;
    auto strict=Newznab::BuildQueries(job.name,pick.meta.count("imdb")?pick.meta.at("imdb"):"",pick.meta.count("tvdb")?pick.meta.at("tvdb"):"");
    struct ExpandQueryCtx
    {
        std::vector<Newznab::Params>& queries;
        const std::vector<Newznab::Params>& strict;
    } expandCtx{queries, strict};
    nzbget_rs_dqh_dupesearch_visit_expand_queries(
        job.name.data(), job.name.size(), &expandCtx,
        [](void* ctx, const char* qPtr, size_t qLen)
        {
            auto* c = static_cast<ExpandQueryCtx*>(ctx);
            std::string q(qPtr, qLen);
            bool duplicate = std::any_of(c->strict.begin(), c->strict.end(), [&](const Newznab::Params& params) {
                return std::any_of(params.begin(), params.end(), [&](const auto& p) { return p.first == "q" && p.second == q; });
            });
            if (!duplicate)
            {
                c->queries.push_back({{"t", "search"}, {"q", q}});
            }
        });
    Newznab::SearchStats searchStats;
    auto wider=Newznab::Search(g_Options->GetDupeSearchUrl(),g_Options->GetDupeSearchApiKey(),queries,deadline,&searchStats);
    queryStats.queries += searchStats.queries;
    queryStats.failed += searchStats.failed;
    queryStats.pages += searchStats.pages;
    std::vector<Newznab::Result> all=prior;
    std::set<std::string> seen;
    for (const auto& r:prior) seen.insert(r.link);
    for (auto& r:wider) if (seen.insert(r.link).second) { results++;all.push_back(std::move(r)); }
    unsigned long long estimate=DupeIdentity::WholeBytes(pick);
    long long bytes=(long long)std::min(estimate,(unsigned long long)std::numeric_limits<long long>::max());
    std::vector<Newznab::Result> accepted;
    for (const auto& r:all)
        if (!attempted.count(r.link) && DupeIdentity::Candidate(job.name,bytes,r.title,r.size)) accepted.push_back(r);
    candidates+=(int)accepted.size();
    auto order=Posting::OrderPostings(accepted,bytes,g_Options->GetDupeSearchMaxDonors());
    postings+=(int)order.size();
    Note(job.nzbId,Message::mkInfo,"DupeSearch: %s: failing pick has no whole backup; broader discovery (%i candidate(s))",job.name.c_str(),(int)accepted.size());
    DupeIdentity::Identity mine;
    bool readMine=false;
    for (const auto& group:order)
    {
        if (IsStopped() || budget.Spent() || Util::CurrentTime()>=deadline) break;
        for (const auto& r:group) attempted.insert(r.link);
        NzbFetcher::Stats counts;
        auto posting=m_fetcher.FetchPosting(group,deadline,counts,&budget);
        stats.Add(counts);
        if (posting.reason!=NzbFetcher::frOk || !DupeIdentity::OuterCompatible(job.name,posting.info.mainName)) continue;
        // Known-release names retain the strict path. Everything else needs inner proof or a fully matching readable data name.
        if (ReleaseName::SameRelease(job.name,posting.listing.title) &&
            (!ReleaseName::Readable(posting.info.mainName) || ReleaseName::SameRelease(job.name,posting.info.mainName)))
        { fetched.push_back(std::move(posting));continue; }
        if (!readMine) { mine=DupeIdentity::Read(pick,deadline);readMine=true; }
        auto theirs=DupeIdentity::Read(posting.info,deadline);
        if (!DupeIdentity::Matches(job.name,posting.info.mainName,mine,theirs)) continue;
        posting.lenient=true;
        posting.sameInner=mine && theirs && mine.bytes==theirs.bytes && mine.extension==theirs.extension;
        fetched.push_back(std::move(posting));
    }
}

inline void DupeSearch::Search(const Job& job)
{
	// search and fetch together are meant to fit in about this long
	time_t deadline = Util::CurrentTime() + SearchDeadlineSec;

	// The pick's ids narrow the search. Infer its whole size when a truncated
    // NZB declares unlisted parts, so complete postings rank before partial relists.
	NzbSummary pick;
	if (!job.queuedFile.empty())
	{
		if (!NzbReader::Parse(DupeUtil::ReadAll(job.queuedFile), pick))
		{
			Note(job.nzbId, Message::mkDetail, "DupeSearch: could not read the nzb-file of %s", job.name.c_str());
		}
	}
    if (job.rankOnly)
    {
        std::vector<NzbFetcher::Fetched> verified;
        std::map<std::string,int> rejected;
        Place(job,pick,verified,rejected,0,0,0);
        return;
    }
    NzbFetcher::Budget budget(g_Options->GetDupeSearchMaxDonors());
    std::set<std::string> attempted;
	std::string imdb = pick.meta.count("imdb") ? pick.meta["imdb"] : "";
	std::string tvdb = pick.meta.count("tvdb") ? pick.meta["tvdb"] : "";

	std::vector<Newznab::Params> queries = Newznab::BuildQueries(job.name, imdb, tvdb);
	Newznab::SearchStats stats;
	std::vector<Newznab::Result> results = Newznab::Search(g_Options->GetDupeSearchUrl(),
		g_Options->GetDupeSearchApiKey(), queries, deadline, &stats);

	// the same release by name; the size isn't a criterion (postings of one
	// release differ by gigabytes in par2 and packaging)
	std::vector<Newznab::Result> candidates;
	for (const Newznab::Result& result : results)
	{
		if (ReleaseName::SameRelease(job.name, result.title))
		{
			candidates.push_back(result);
		}
	}

	Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: %i result(s) from %i search(es) (%i failed, %i page(s)), %i of them the same release",
		job.name.c_str(), (int)results.size(), stats.queries, stats.failed, stats.pages, (int)candidates.size());

	// the postings to fetch, best first: fetching each costs a grab of an indexer
    unsigned long long wholeBytes = DupeIdentity::WholeBytes(pick);
    long long orderBytes = (long long)std::min(wholeBytes,
        (unsigned long long)std::numeric_limits<long long>::max());
	std::vector<Posting::Group> order = Posting::OrderPostings(candidates, orderBytes,
		g_Options->GetDupeSearchMaxDonors());

	// the sketches of what nzbget already holds, read beside the fetches
	std::vector<Posting::Sketch> known;
	std::thread knownReader([&]()
		{
			for (const std::string& path : job.knownFiles)
			{
				Posting::Sketch sketch;
				if (Posting::SketchOfFile(path, sketch))
				{
					known.push_back(std::move(sketch));
				}
			}
		});

	NzbFetcher::Stats fetchStats;
	std::vector<NzbFetcher::Fetched> fetched;
	int notTried = 0;
	size_t parallel = g_Options->GetDupeSearchMaxDonors() > 0 ? 1 : (size_t)FetchParallel;
    for (size_t from = 0; from < order.size(); from += parallel)
	{
		if (IsStopped() || Util::CurrentTime() >= deadline || budget.Spent())
		{
			notTried = (int)(order.size() - from);
			break;
		}

		size_t count = std::min(parallel, order.size() - from);
		std::vector<NzbFetcher::Fetched> chunk(count);
		std::vector<NzbFetcher::Stats> chunkStats(count);
		std::vector<std::thread> threads;
		for (size_t i = 0; i < count; i++)
		{
            for (const Newznab::Result& listing : order[from+i]) attempted.insert(listing.link);
			threads.emplace_back([&, i]()
				{ chunk[i] = m_fetcher.FetchPosting(order[from + i], deadline, chunkStats[i], &budget); });
		}
		for (std::thread& thread : threads)
		{
			thread.join();
		}
		for (size_t i = 0; i < count; i++)
		{
			fetchStats.Add(chunkStats[i]);
			if (chunk[i].reason == NzbFetcher::frOk)
			{
				fetched.push_back(std::move(chunk[i]));
			}
		}
	}

	knownReader.join();
    int resultCount=(int)results.size(), candidateCount=(int)candidates.size(), postingCount=(int)order.size();
    Expand(job,pick,results,known,deadline,budget,fetched,attempted,fetchStats,stats,resultCount,candidateCount,postingCount);

	// what is worth keeping, in this order: another release (by the name of
	// its largest data file, when that name is readable), a posting nzbget
	// already holds, one found dead before, one that shares articles with
	// the pick or an accepted duplicate
	std::vector<std::vector<std::string>> postings;
	postings.push_back(pick.messageIds);
	std::vector<NzbFetcher::Fetched> verified;
	std::map<std::string, int> rejected;
	for (NzbFetcher::Fetched& posting : fetched)
	{
		Posting::Sketch sketch = Posting::MakeSketch(posting.info.messageIds);
		const std::vector<std::string>& ids = posting.info.messageIds;
		bool mainReadable = !posting.lenient && ReleaseName::Readable(posting.info.mainName);
		bool sameRel = mainReadable && ReleaseName::SameRelease(job.name, posting.info.mainName);
		if (mainReadable && !sameRel)
		{
			rejected["other-release"]++;
		}
		else if (std::any_of(known.begin(), known.end(), [&](const Posting::Sketch& k) { return Posting::SameSketch(sketch, k); }))
		{
			rejected["in-nzbget"]++;
		}
		else if (m_dead.IsDead(sketch))
		{
			rejected["known-dead"]++;
		}
		else if (std::any_of(postings.begin(), postings.end(), [&](const std::vector<std::string>& p) { return Posting::SamePosting(ids, p); }))
		{
			rejected["same-posting"]++;
		}
		else
		{
			postings.push_back(ids);
			verified.push_back(std::move(posting));
		}
	}
	rejected["listing-mismatch"] += fetchStats.listingMismatch;
	rejected["refused"] += fetchStats.refused;
	rejected["relisted"] += fetchStats.relisted;
	rejected["fetch"] += fetchStats.fetch;
	rejected["parse"] += fetchStats.parse;
	rejected["deadline"] += notTried;
    rejected["budget"] += fetchStats.budget;

	// a search that learned nothing - a query failed and nothing came back, or no
	// posting could be fetched (indexers refusing, the deadline) - doesn't hold the
	// key for SearchWindowSec: the next add of the key, or a restart, searches it again
	int retryReason = nzbget_rs_dupesearch_classify_search_retry_reason(
		stats.queries, stats.failed, verified.empty() ? 1 : 0, fetched.empty() ? 1 : 0,
		fetchStats.refused, fetchStats.fetch, notTried);
	if (retryReason > 0)
	{
		ForgetSearch(job.dupeKey, ReleaseName::Normalize(job.name));
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: %s, the key may be searched again", job.name.c_str(),
			retryReason == 1 ? "no indexer answered" : retryReason == 2 ? "an indexer query failed and nothing came back" :
			"no posting could be fetched");
	}

	// what was fetched is kept until it is placed: a restart resumes from it
	// without fetching again (every fetch costs a grab)
	SavePending(job, pick, verified);
	Place(job, pick, verified, rejected, resultCount, candidateCount, postingCount);
}

inline void DupeSearch::Place(const Job& job, const NzbSummary& pick, std::vector<NzbFetcher::Fetched>& verified,
	std::map<std::string, int>& rejected, int results, int candidates, int postings)
{
	// closest size first: the likeliest byte-identical repost; then the most grabbed
	long long pickBytes = (long long)std::min(DupeIdentity::WholeBytes(pick),
        (unsigned long long)std::numeric_limits<long long>::max());
	if (verified.size() > 1)
	{
		std::vector<int64_t> totalBytes(verified.size());
		std::vector<int> grabs(verified.size());
		std::vector<int64_t> dates(verified.size());
		std::vector<size_t> orderIdx(verified.size());
		for (size_t i = 0; i < verified.size(); i++)
		{
			totalBytes[i] = verified[i].info.totalBytes;
			grabs[i] = verified[i].listing.grabs;
			dates[i] = (int64_t)verified[i].listing.date;
		}
		nzbget_rs_df_sort_verified_postings(
			totalBytes.data(), grabs.data(), dates.data(), verified.size(), pickBytes, orderIdx.data());
		std::vector<NzbFetcher::Fetched> sorted;
		sorted.reserve(verified.size());
		for (size_t idx : orderIdx)
		{
			sorted.push_back(std::move(verified[idx]));
		}
		verified = std::move(sorted);
	}

	DonorHealth::ServerList servers;
	if (!verified.empty() || !job.members.empty())
	{
		servers = NntpHealthServer::Servers();
		if (servers.empty())
		{
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: no active news servers, the postings are added unchecked", job.name.c_str());
		}
	}
	DonorHealth::Options healthOptions;
	healthOptions.sample.percent = g_Options->GetDupeHealthPercent();
	healthOptions.sample.minimum = g_Options->GetDupeHealthMin();
	healthOptions.sample.maximum = g_Options->GetDupeHealthMax();
	healthOptions.sample.maxBody = g_Options->GetDupeBodyChecks();
	healthOptions.budgetMs = g_Options->GetDupeHealthBudget() * 1000;

	auto groupsOf = [](const NzbSummary& summary)
		{ return std::make_shared<std::vector<std::string>>(summary.groups); };

	// the pick's own posting: found dead, it is remembered (the dead-pick probe of
	// the queue is what swaps it out)
	std::thread pickCheck;
	if (!servers.empty() && !pick.messageIds.empty())
	{
		pickCheck = std::thread([&]()
			{
				DonorHealth::Health health = DonorHealth::CheckPosting(servers, pick.messageIds,
					groupsOf(pick), healthOptions, false);
				if (DonorHealth::DeadProbe(health))
				{
					m_dead.Add(Posting::MakeSketch(pick.messageIds), 0, !g_Options->GetDupeSearchDryRun());
					Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: the pick is dead (%i of %i probe articles on no server)",
						job.name.c_str(), health.missing, health.Answered());
				}
			});
	}

	int dead = 0;
	int overCap = 0;
	int rescoreFailed = 0;
	int added = 0;
	int cap = g_Options->GetDupeSearchMaxDonors();
	std::mutex placeMutex;
	std::map<size_t, DonorHealth::Health> probe;

	// the quick probe of every posting
	if (!servers.empty())
	{
		std::vector<DonorHealth::Posting> list;
		for (size_t i = 0; i < verified.size(); i++)
		{
			NzbgetRsBuf keyBuf = nzbget_rs_dqh_dupesearch_format_usize_str(i);
			std::string keyStr(keyBuf.data ? keyBuf.data : "", keyBuf.len);
			nzbget_rs_free(keyBuf);
			list.push_back({ std::move(keyStr), verified[i].info.messageIds, groupsOf(verified[i].info) });
		}
		DonorHealth::CheckPostings(servers, list, healthOptions, false, FetchParallel,
			[&](const std::string& key, const DonorHealth::Health& health)
			{
				std::lock_guard<std::mutex> guard(placeMutex);
				probe[(size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str())] = health;
			});
	}

	std::vector<size_t> live;
	for (size_t i = 0; i < verified.size(); i++)
	{
		auto it = probe.find(i);
		if (it != probe.end() && DonorHealth::DeadProbe(it->second))
		{
			m_dead.Add(Posting::MakeSketch(verified[i].info.messageIds), 0, !g_Options->GetDupeSearchDryRun());
			dead++;
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: dropping dead posting %s [%s] after the probe: %i of %i probe articles on no server",
				job.name.c_str(), verified[i].listing.title.c_str(), verified[i].listing.indexer.c_str(),
				it->second.missing, it->second.Answered());
			continue;
		}
		live.push_back(i);
	}

	struct Placed
	{
		int id;
		DonorScore::Entry entry;
		std::string title;
		bool member = false;	// a duplicate that was already in history: always rescored
        bool valid = true; // Final score/parameter update must still succeed.
		int oldScore = 0;		// a member's score before: never raised unless it is whole
	};
	std::map<size_t, Placed> placed;
    std::map<size_t, double> deferred;
    auto slotsUsed = [&]() {
        GuardedDownloadQueue queue = DownloadQueue::Guard();
        NzbInfo* currentPick = FindNzb(queue, job.nzbId);
        return (int)std::count_if(placed.begin(), placed.end(), [&](const auto& item) {
            bool avail = !item.second.member && item.second.valid &&
                item.second.entry.score > DonorScore::Dead && item.second.id >= 0 &&
                PlacementAvailable(queue, currentPick, FindNzb(queue, item.second.id));
            return nzbget_rs_dupesearch_slot_counts_as_used(
                item.second.member ? 1 : 0, item.second.valid ? 1 : 0,
                item.second.entry.score, DonorScore::Dead, item.second.id,
                g_Options->GetDupeSearchDryRun() ? 1 : 0, avail ? 1 : 0) != 0;
        });
    };
	DonorScore::Ranks ranks;
	int base = DonorScore::Base(job.score);

	auto twinOf = [&](const NzbFetcher::Fetched& f)
		{ return DupeIdentity::SameData(pick, f.info); };
	auto aliveOf = [&](const DonorHealth::Health& health, const NzbSummary& summary) { return DupeIdentity::Alive(health, summary); };
    auto cacheableLoss = [&](DonorHealth::Health health) {
        // Only confirmed article loss may invalidate overlapping postings.
        // Structural gaps and provider errors belong to this NZB's score.
        health.error=0;
        return DupeIdentity::IsDead(health,NzbSummary{},g_Options->GetDupeMinAlive());
    };
	auto entryOf = [&](size_t i, int score, double alive)
		{
			DonorScore::Entry entry;
			entry.score = score;
			entry.alive = alive;
			entry.twin = twinOf(verified[i]);
			entry.bytes = verified[i].info.totalBytes;
			entry.grabs = verified[i].listing.grabs;
			return entry;
		};
	// nzbget shuts down: a check cut short knows nothing, and adding a duplicate now
	// would wait for the stopped scanner for good (B37). The search keeps its saved
	// postings and resumes after the restart.
	auto interrupted = [&]()
		{
			if (!IsStopped())
			{
				return false;
			}
			if (pickCheck.joinable())
			{
				pickCheck.join();
			}
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: RustyGet shuts down, the search resumes after the restart",
				job.name.c_str());
			return true;
		};
	auto place = [&](size_t i, double alive, const char* how)
		{
			int score = ranks.Take(alive, twinOf(verified[i]));
			int id = AddDonor(job, pick, verified[i], base + score, alive);
			if (id == 0)
			{
				ranks.Release(score);
				return;
			}
			added++;
			placed[i] = { id, entryOf(i, score, alive), verified[i].listing.title };
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: added %s [%s] score=%i alive=%i%% (%s)", job.name.c_str(),
				verified[i].listing.title.c_str(), verified[i].listing.indexer.c_str(), base + score,
				nzbget_rs_dupesearch_alive_to_percent(alive), how);
		};

	// the probe-alive postings go in at once, then each is rescored when its
	// full sample lands; the rest are added as their samples finish
	size_t fast = nzbget_rs_dupesearch_calc_fast_donor_count(
		g_Options->GetDupeFastDonors(), live.size(), cap);
	std::set<size_t> fastSet;
	for (size_t n = 0; n < fast; n++)
	{
		auto it = probe.find(live[n]);
		place(live[n], it == probe.end() ? -1.0 : aliveOf(it->second, verified[live[n]].info), "fast");
		fastSet.insert(live[n]);
	}

	std::set<size_t> todo(live.begin(), live.end());
	if (!servers.empty() && !todo.empty())
	{
		std::vector<DonorHealth::Posting> list;
		for (size_t i : todo)
		{
			NzbgetRsBuf keyBuf = nzbget_rs_dqh_dupesearch_format_usize_str(i);
			std::string keyStr(keyBuf.data ? keyBuf.data : "", keyBuf.len);
			nzbget_rs_free(keyBuf);
			list.push_back({ std::move(keyStr), verified[i].info.messageIds, groupsOf(verified[i].info) });
		}
		DonorHealth::CheckPostings(servers, list, healthOptions, true, FetchParallel,
			[&](const std::string& key, const DonorHealth::Health& health)
			{
				std::lock_guard<std::mutex> guard(placeMutex);
				size_t i = (size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str());
				if (IsStopped() || !todo.erase(i))
				{
					return;
				}
				double alive = aliveOf(health, verified[i].info);
				bool isDead = DupeIdentity::IsDead(health, verified[i].info, g_Options->GetDupeMinAlive());
				Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: %s [%s]: alive=%i%% (%i of %i articles)", job.name.c_str(),
					verified[i].listing.title.c_str(), verified[i].listing.indexer.c_str(),
					nzbget_rs_dupesearch_alive_to_percent(alive), health.present, health.Answered());

				auto it = placed.find(i);
				if (it != placed.end())
				{
					// a fast donor: its real health moves its score
					Placed& donor = it->second;
					ranks.Release(donor.entry.score);
					int score = isDead ? DonorScore::Dead : ranks.Take(alive, donor.entry.twin);
					NzbgetRsBuf paramBuf = nzbget_rs_dqh_dupesearch_format_key_val_param(
						AliveParam,
						nzbget_rs_dupesearch_fast_donor_alive_param(isDead ? 1 : 0, alive));
					std::string param(paramBuf.data ? paramBuf.data : "", paramBuf.len);
					nzbget_rs_free(paramBuf);
					if (alive < 0 || SetScore(donor.id, base + score,
						param.empty() ? std::vector<std::string>() : std::vector<std::string>{ param }))
					{
						if (alive < 0)
						{
							// inconclusive: the donor keeps its score, and the rank taken for
							// the new one is given back (F19: it stayed taken, the old one free)
							ranks.Release(score);
							ranks.Use(donor.entry.score);
						}
						else
						{
							donor.entry.score = score;
						}
						donor.entry.alive = alive;
						if (isDead)
						{
							if (cacheableLoss(health)) m_dead.Add(Posting::MakeSketch(verified[i].info.messageIds), 0, !g_Options->GetDupeSearchDryRun());
							dead++;
						}
					}
					else
					{
						ranks.Release(score);
						ranks.Use(donor.entry.score);
						rescoreFailed++;
                        donor.valid = false;
						Note(job.nzbId, Message::mkWarning, "DupeSearch: could not rescore %s: keeps score %i", verified[i].listing.title.c_str(),
							base + donor.entry.score);
					}
				}
				else if (isDead)
				{
					if (cacheableLoss(health)) m_dead.Add(Posting::MakeSketch(verified[i].info.messageIds), 0, !g_Options->GetDupeSearchDryRun());
					dead++;
					Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: dropping dead posting %s: alive=%i%%", job.name.c_str(),
						verified[i].listing.title.c_str(), (int)std::lround(100 * alive));
				}
				else
				{
                    // A healthy answer may arrive before a fast donor's dead
                    // answer. Retain it until all checks settle the cap slots.
                    if (cap > 0 && slotsUsed() >= cap) deferred[i] = alive;
                    else place(i, alive, "checked");
				}
			});
	}

	if (interrupted())
	{
		return;
	}

    for (auto it = deferred.begin(); it != deferred.end();)
    {
        if (cap > 0 && slotsUsed() >= cap) { ++it; continue; }
        auto entry = *it;
        it = deferred.erase(it);
        place(entry.first, entry.second, "checked");
    }

	// whatever no check reported on (no servers, a failed check) is added unchecked
	for (size_t i : std::set<size_t>(todo))
	{
		todo.erase(i);
		if (placed.count(i))
		{
			continue;
		}
		if (cap > 0 && slotsUsed() >= cap)
		{
			overCap++;
			continue;
		}
		place(i, -1.0, "unchecked");
	}

	// the duplicates nzbget already kept for the key (a client's own backups,
	// earlier searches' donors) are measured the same way and ranked with the
	// new ones: nzbget tries them by score, whoever sent them. Each is read
	// from its own nzb-file, never matched by size: two postings can have one size.
	std::set<size_t> uncheckedMembers;
	if (!job.members.empty() && (!servers.empty() || job.rankOnly))
	{
		std::vector<NzbSummary> summaries(job.members.size());
		std::vector<DonorHealth::Posting> list;
		std::map<size_t, size_t> sameAs;	// a member whose posting another member already is
		for (size_t m = 0; m < job.members.size(); m++)
		{
			uncheckedMembers.insert(m);
			if (!NzbReader::Parse(DupeUtil::ReadAll(job.members[m].second), summaries[m]))
			{
				continue;
			}
            if (Posting::SamePosting(summaries[m].messageIds, pick.messageIds))
            {
                uncheckedMembers.erase(m);
                continue;
            }
			// each posting is checked once: the client's copies of it share the answer
			auto first = std::find_if(list.begin(), list.end(), [&](const DonorHealth::Posting& posting)
				{ return Posting::SamePosting(summaries[m].messageIds, posting.ids); });
			if (first != list.end())
			{
				sameAs[m] = (size_t)nzbget_rs_dqh_parse_c_atoi(first->key.c_str());
				continue;
			}
			NzbgetRsBuf keyBuf = nzbget_rs_dqh_dupesearch_format_usize_str(m);
			std::string keyStr(keyBuf.data ? keyBuf.data : "", keyBuf.len);
			nzbget_rs_free(keyBuf);
			list.push_back({ std::move(keyStr), summaries[m].messageIds, groupsOf(summaries[m]) });
		}
		std::map<size_t, DonorHealth::Health> measured;
		if (!servers.empty()) DonorHealth::CheckPostings(servers, list, healthOptions, true, FetchParallel,
			[&](const std::string& key, const DonorHealth::Health& health)
			{
				std::lock_guard<std::mutex> guard(placeMutex);
				measured[(size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str())] = health;
			});
		for (const auto& alias : sameAs)
		{
			auto health = measured.find(alias.second);
			if (health != measured.end())
			{
				measured[alias.first] = health->second;
			}
		}
		for (const auto& entry : measured)
		{
			const NzbSummary& summary = summaries[entry.first];
			double alive = aliveOf(entry.second, summary);
			if (alive < 0)
			{
				continue;	// too few answers to judge: its score stays
			}
			bool isDead = DupeIdentity::IsDead(entry.second, summary, g_Options->GetDupeMinAlive());
			if (isDead)
			{
				if (cacheableLoss(entry.second)) m_dead.Add(Posting::MakeSketch(summary.messageIds), 0, !g_Options->GetDupeSearchDryRun());
			}
			Placed member;
			member.id = job.members[entry.first].first;
			member.title = FileSystem::BaseFileName(job.members[entry.first].second.c_str());
			member.member = true;
			member.oldScore = entry.first < job.memberScores.size() ? job.memberScores[entry.first] : 0;
			member.entry.score = isDead ? DonorScore::Dead : DonorScore::Dead + 1;
			member.entry.alive = alive;
			member.entry.twin = DupeIdentity::SameData(pick, summary);
			member.entry.bytes = summary.totalBytes;
			placed[verified.size() + entry.first] = member;
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: duplicate %s in history: alive=%i%% (%i of %i articles)%s", job.name.c_str(),
				member.title.c_str(), (int)std::lround(100 * alive), entry.second.present, entry.second.Answered(),
				isDead ? ", dead" : "");
		}
	}

	if (interrupted())
	{
		return;
	}

    auto rerank = [&]() {
	// once every check is in, the scores in order of wholeness
	std::vector<size_t> order2;
	std::vector<DonorScore::Entry> entries;
	for (auto& entry : placed)
	{
        if (!entry.second.valid) continue;
		order2.push_back(entry.first);
		entries.push_back(entry.second.entry);
	}
	std::vector<int> wanted = DonorScore::Rerank(entries, pickBytes);
	// with every one of them dead there is nothing to order: scores stay, the dead are marked
	bool allDead = std::all_of(wanted.begin(), wanted.end(), [](int score) { return score == DonorScore::Dead; });
	for (size_t n = 0; n < order2.size(); n++)
	{
		Placed& donor = placed[order2[n]];
		if (wanted[n] == donor.entry.score && !donor.member)
		{
			continue;
		}
		std::vector<std::string> params;
		int score = base + wanted[n];
		if (donor.member)
		{
			int percent = 0;
			int addAliveZero = 0;
			nzbget_rs_df_eval_member_rerank(
				base, wanted[n], DonorScore::Dead, donor.oldScore, donor.entry.alive, allDead ? 1 : 0,
				&score, &percent, &addAliveZero);
			NzbgetRsBuf hpBuf = nzbget_rs_dqh_dupesearch_format_key_val_param(HealthParam, percent);
			params.emplace_back(hpBuf.data ? hpBuf.data : "", hpBuf.len);
			nzbget_rs_free(hpBuf);
			if (addAliveZero)
			{
				params.push_back(std::string(AliveParam) + "=0");
			}
		}
		if (SetScore(donor.id, score, params))
		{
            if (donor.member) uncheckedMembers.erase(order2[n] - verified.size());
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: reranked %s: score %i", job.name.c_str(), donor.title.c_str(), score);
			donor.entry.score = wanted[n];
		}
		else
		{
            donor.valid = false;
			rescoreFailed++;
		}
	}

    };
    rerank();
    // A final failed score edit can invalidate another occupied slot. Keep
    // measured candidates until that decision and attempt each at most once.
    while (!deferred.empty() && (cap <= 0 || slotsUsed() < cap))
    {
        auto entry = *deferred.begin();
        deferred.erase(deferred.begin());
        place(entry.first, entry.second, "checked");
        rerank();
    }
    overCap += (int)deferred.size();

	if (pickCheck.joinable())
	{
		pickCheck.join();
	}

	rejected["dead"] += dead;
	rejected["over-cap"] += overCap;
	rejected["rescore"] += rescoreFailed;
	std::vector<const char*> rejKeys;
	std::vector<int> rejCounts;
	rejKeys.reserve(rejected.size());
	rejCounts.reserve(rejected.size());
	for (const auto& entry : rejected)
	{
		rejKeys.push_back(entry.first.c_str());
		rejCounts.push_back(entry.second);
	}
	NzbgetRsBuf outcomeBuf = nzbget_rs_dqh_dupesearch_format_rejected_outcome(
		rejKeys.data(), rejCounts.data(), rejected.size());
	std::string outcome(outcomeBuf.data ? outcomeBuf.data : "", outcomeBuf.len);
	nzbget_rs_free(outcomeBuf);
	Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: results=%i candidates=%i postings=%i verified=%i added=%i rejected={%s}",
		job.name.c_str(), results, candidates, postings, (int)verified.size(), added, outcome.c_str());
	RemovePending(job.nzbId);
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		m_sent.erase(job.nzbId);
	}
    // Without a usable whole backup discovery remains retryable while the
    // pick is queued, even if a fast-added donor was later declared dead.
    bool usableBackup = false;
    {
        GuardedDownloadQueue queue = DownloadQueue::Guard();
        NzbInfo* currentPick = FindNzb(queue, job.nzbId);
        usableBackup=std::any_of(placed.begin(),placed.end(),[&](const auto& item) {
            bool avail = item.second.id >= 0 && item.second.valid &&
                item.second.entry.score > DonorScore::Dead && item.second.entry.alive >= 0.999 &&
                PlacementAvailable(queue, currentPick, FindNzb(queue, item.second.id), true);
            return nzbget_rs_dupesearch_is_usable_whole_backup(
                item.second.valid ? 1 : 0, item.second.entry.score, DonorScore::Dead,
                item.second.entry.alive, item.second.id,
                g_Options->GetDupeSearchDryRun() ? 1 : 0, avail ? 1 : 0) != 0;
        });
    }
    bool stateRetryable = false;
    if (job.rankOnly)
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        Searched* state = FindSearch(LowerKey(job.dupeKey), ReleaseName::Normalize(job.name));
        stateRetryable = state && state->retryable;
    }
    if (usableBackup)
    {
        { std::lock_guard<std::mutex> guard(m_mutex);
          SetRetryable(LowerKey(job.dupeKey), ReleaseName::Normalize(job.name), false); }
        if (!g_Options->GetDupeSearchDryRun()) SaveState();
    }
    int retryDiscoveryInt = 0;
    int rankIncompleteInt = 0;
    nzbget_rs_dupesearch_eval_post_place_retry(
        job.rankOnly ? 1 : 0, stateRetryable ? 1 : 0, usableBackup ? 1 : 0,
        uncheckedMembers.size(), &retryDiscoveryInt, &rankIncompleteInt);
    bool rankIncomplete = rankIncompleteInt != 0;
    bool retryDiscovery = retryDiscoveryInt != 0;
    // A rename/deletion may make this pick ineligible for scheduling, but must
    // not falsely complete the original title's unfulfilled discovery stamp.
    if (retryDiscovery && !IsStopped()) ForgetSearch(job.dupeKey, ReleaseName::Normalize(job.name));
    if ((retryDiscovery || rankIncomplete) && !IsStopped())
    {
        GuardedDownloadQueue queue=DownloadQueue::Guard();
        NzbInfo* current=queue->GetQueue()->Find(job.nzbId);
        if (current && !current->GetDeleting() && current->GetDeleteStatus() == NzbInfo::dsNone &&
            !current->GetPostInfo() &&
            LowerKey(EffectiveKey(current))==LowerKey(job.dupeKey) &&
            ReleaseName::Normalize(current->GetName())==ReleaseName::Normalize(job.name))
        {
            if (retryDiscovery)
            {
                // An owed discovery retry takes priority over ranking an
                // inconclusive backup; otherwise rank-only retries can starve it.
                { std::lock_guard<std::mutex> guard(m_mutex);
                  m_rankPending.erase(job.nzbId); }
            }
            else
            {
                // Retain the completed search stamp: only recheck existing
                // backups, without repeating indexer searches or paid grabs.
                std::lock_guard<std::mutex> guard(m_mutex);
                m_rankPending.insert(job.nzbId);
            }
            if (!retryDiscovery) MarkSearching(job);
            Schedule(job.nzbId,Util::CurrentTime()+900);
            if (!retryDiscovery)
                current->PrintMessage(Message::mkInfo, "DupeSearch: %s: retrying backup health in 15 minutes", job.name.c_str());
        }
    }
}

inline std::string DupeSearch::PickGone(const Job& job)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	// in history after a download or a failure it still wants its duplicates;
	// deleted by the user it doesn't: the first duplicate would download
	NzbInfo* pick = FindNzb(downloadQueue, job.nzbId);
	if (!pick || pick->GetDeleting() || pick->GetDeleteStatus() == NzbInfo::dsManual)
	{
		return "the pick was deleted";
	}
	std::string pickEffKey = EffectiveKey(pick);
	std::string pickNormName = ReleaseName::Normalize(pick->GetName());
	std::string jobNormName = ReleaseName::Normalize(job.name);
	int reason = nzbget_rs_df_eval_pick_gone(
		1, 0, (int)pick->GetDeleteStatus(), (int)NzbInfo::dsManual,
		pickEffKey.c_str(), job.dupeKey.c_str(), pickNormName.c_str(), jobNormName.c_str());
	switch (reason)
	{
		case 1: return "the pick was deleted";
		case 2: return "the duplicate key of the pick changed";
		case 3: return "the name of the pick changed";
		default: return "";
	}
}

inline int DupeSearch::AddDonor(const Job& job, const NzbSummary& pick, const NzbFetcher::Fetched& posting, int score, double alive)
{
	if (IsStopped())
	{
		return 0;
	}
	std::string gone = PickGone(job);
	if (!gone.empty())
	{
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: not adding %s [%s]: %s", job.name.c_str(),
			posting.listing.title.c_str(), posting.listing.indexer.c_str(), gone.c_str());
		return 0;
	}
	if (g_Options->GetDupeSearchDryRun())
	{
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: dry run, would add %s [%s] score=%i alive=%i%%", job.name.c_str(),
			posting.listing.title.c_str(), posting.listing.indexer.c_str(), score,
			nzbget_rs_dupesearch_alive_to_percent(alive));
		return -1;
	}
	std::string fingerprint = posting.info.Fingerprint();
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		if (!m_sent[job.nzbId].insert(fingerprint).second)
		{
			return 0;
		}
	}

	NzbParameterList parameters;
	parameters.SetParameter(DonorParam, "yes");
	for (const std::string& hint : DupeIdentity::Hints(pick, posting.info, posting.sameInner))
	{
		size_t nameLen = 0, valOff = 0;
		if (nzbget_rs_tpr_split_param_assignment(hint.c_str(), &nameLen, &valOff) != 0)
		{
			parameters.SetParameter(hint.substr(0, nameLen).c_str(), hint.c_str() + valOff);
		}
	}
	if (alive >= 0)
	{
		NzbgetRsBuf aliveBuf = nzbget_rs_dqh_conn_format_port_str(
			nzbget_rs_dupesearch_alive_to_percent(alive));
		parameters.SetParameter(AliveParam, aliveBuf.data ? aliveBuf.data : "0");
		nzbget_rs_free(aliveBuf);
	}
	std::string name = posting.listing.title;
	if (nzbget_rs_dupesearch_needs_nzb_ext(name.data(), name.size()) != 0)
	{
		name += ".nzb";
	}

	int nzbId = 0;
	Scanner::EAddStatus status = g_Scanner->AddExternalFile(name.c_str(), job.category.c_str(), false, 0,
		job.dupeKey.c_str(), score, dmScore, &parameters, false, false, nullptr, nullptr,
		posting.data.data(), (int)posting.data.size(), &nzbId);
	if (status != Scanner::asSuccess || nzbId == 0)
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		m_sent[job.nzbId].erase(fingerprint);
		return 0;
	}

	// the pick may have been deleted, or have got another key, while the add ran
	// (it takes the queue lock itself, after PickGone let it go): the new duplicate
	// goes again (B33)
	gone = PickGone(job);
	if (!gone.empty())
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			IdList ids{ nzbId };
			bool queued = downloadQueue->GetQueue()->Find(nzbId) != nullptr;
			downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId,
				queued ? DownloadQueue::eaGroupFinalDelete : DownloadQueue::eaHistoryFinalDelete, nullptr);
		}
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: removed the duplicate %s just added: %s",
			job.name.c_str(), posting.listing.title.c_str(), gone.c_str());
		return 0;
	}

	// remembered with the search in progress: a resume after a restart must not add
	// it again, even when the user deleted it meanwhile (then nzbget keeps only a
	// hidden duplicate record, which the resume can't recognize)
	std::string sentPath = PendingDir(job.nzbId) + PATH_SEPARATOR + "sent";
	if (FileSystem::DirectoryExists(PendingDir(job.nzbId).c_str()))
	{
		std::ofstream sent(fs::u8path(sentPath), std::ios::app);
		sent << fingerprint << "\n";
	}
	return nzbId;
}

inline bool DupeSearch::SetScore(int id, int score, const std::vector<std::string>& params)
{
	if (g_Options->GetDupeSearchDryRun() || id < 0)
	{
		std::string text;
		for (const std::string& param : params)
		{
			text += " " + param;
		}
		info("DupeSearch: dry run, would set the score of download %i to %i%s", id, score, text.c_str());
		return true;
	}
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	// a duplicate the user deleted is kept as a hidden record (hkDup): its score
	// edit succeeds but means nothing, so it counts as gone
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetId() == id && historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			return false;
		}
	}
	IdList ids{ id };
	NzbgetRsBuf scoreBuf = nzbget_rs_dqh_conn_format_port_str(score);
	std::string text(scoreBuf.data ? scoreBuf.data : "", scoreBuf.len);
	nzbget_rs_free(scoreBuf);
	const std::pair<DownloadQueue::EEditAction, DownloadQueue::EEditAction> kinds[] = {
		{ DownloadQueue::eaHistorySetDupeScore, DownloadQueue::eaHistorySetParameter },
		{ DownloadQueue::eaGroupSetDupeScore, DownloadQueue::eaGroupSetParameter } };
	for (const auto& kind : kinds)
	{
		if (downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId, kind.first, text.c_str()))
		{
			for (const std::string& param : params)
			{
				downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId, kind.second, param.c_str());
			}
			return true;
		}
	}
	return false;
}

inline void DupeSearch::Note(int nzbId, Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list ap;
	va_start(ap, format);
	vsnprintf(text, sizeof(text), format, ap);
	va_end(ap);

	// into the pick's own log as well (with InfoTarget=log and no log file,
	// the global log keeps no info lines); the global log alone once it's gone
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = FindNzb(downloadQueue, nzbId);
	if (nzbInfo)
	{
		nzbInfo->AddMessage(kind, text);
		return;
	}
	switch (kind)
	{
		case Message::mkWarning: warn("%s", text); break;
		case Message::mkDetail: detail("%s", text); break;
		default: info("%s", text); break;
	}
}

inline std::string DupeSearch::PendingDir(int nzbId)
{
	NzbgetRsBuf dirBuf = nzbget_rs_dqh_dupesearch_format_pending_dir(
		g_Options->GetQueueDir(), PATH_SEPARATOR, nzbId);
	std::string dir(dirBuf.data ? dirBuf.data : "", dirBuf.len);
	nzbget_rs_free(dirBuf);
	return dir;
}

namespace
{

void RemoveDir(const std::string& dir)
{
	CString errmsg;
	FileSystem::DeleteDirectoryWithContent(dir.c_str(), errmsg);
}


}

inline void DupeSearch::MarkSearching(const Job& job)
{
	if (g_Options->GetDupeSearchDryRun())
	{
		return;
	}
	std::string dir = PendingDir(job.nzbId);
	RemoveDir(dir);
	CString errmsg;
	if (!FileSystem::ForceDirectories(dir.c_str(), errmsg) ||
        !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-title", ReleaseName::Normalize(job.name)) ||
        !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-key", LowerKey(job.dupeKey)) ||
        !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "phase", job.rankOnly ? "ranking\n" : "searching\n"))
	{
		warn("Could not save the DupeSearch state to %s", dir.c_str());
	}
}

inline void DupeSearch::SavePending(const Job& job, const NzbSummary& pick, const std::vector<NzbFetcher::Fetched>& verified)
{
	if (g_Options->GetDupeSearchDryRun())
	{
		return;
	}
	std::string dir = PendingDir(job.nzbId);
    bool ok = DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-title", ReleaseName::Normalize(job.name)) &&
        DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-key", LowerKey(job.dupeKey)) &&
        DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-posting", pick.Fingerprint());
	for (size_t i = 0; i < verified.size() && ok; i++)
	{
		const Newznab::Result& listing = verified[i].listing;
		NzbgetRsBuf baseBuf = nzbget_rs_dqh_dupesearch_format_indexed_base_path(
			dir.c_str(), PATH_SEPARATOR, i);
		std::string base(baseBuf.data ? baseBuf.data : "", baseBuf.len);
		nzbget_rs_free(baseBuf);
		NzbgetRsBuf metaBuf = nzbget_rs_df_format_pending_meta(
			listing.title.c_str(), listing.indexer.c_str(), listing.grabs, listing.size, (int64_t)listing.date);
		std::string metaStr(metaBuf.data ? metaBuf.data : "", metaBuf.len);
		nzbget_rs_free(metaBuf);
		ok = DupeUtil::WriteAtomic(base + ".nzb", verified[i].data) && DupeUtil::WriteAtomic(base + ".meta", metaStr) &&
            DupeUtil::WriteAtomic(base + ".inner", verified[i].sameInner ? "1\n" : "0\n");
	}
	if (!ok || !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "phase", "fetched\n"))
	{
		warn("Could not save the DupeSearch state to %s", dir.c_str());
	}
}

inline void DupeSearch::RemovePending(int nzbId)
{
	if (IsStopped())
	{
		return;	// interrupted: resumed after the restart
	}
	RemoveDir(PendingDir(nzbId));
}

inline void DupeSearch::ResumePending()
{
	std::vector<int> ids;
	DirBrowser dirBrowser(PendingDir(0).c_str());
	while (const char* name = dirBrowser.Next())
	{
		int parsedId = nzbget_rs_dqh_dupesearch_parse_pending_dir_id(name);
		if (parsedId > 0)
		{
			ids.push_back(parsedId);
		}
	}

	for (int nzbId : ids)
	{
		if (IsStopped())
		{
			return;
		}
		std::string dir = PendingDir(nzbId);
        std::string phase = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "phase");
		int fetchedInt = 0;
		int rankingInt = 0;
		nzbget_rs_dupesearch_classify_pending_phase(
			phase.data(), phase.size(), &fetchedInt, &rankingInt);
		bool fetched = fetchedInt != 0;
        bool ranking = rankingInt != 0;

		Job job;
        bool stateErased = false;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = FindNzb(downloadQueue, nzbId);
			if (nzbInfo && fetched)
			{
				Collect(downloadQueue, nzbInfo, job);
				job.score = std::max(nzbInfo->GetDupeScore(), BasePickScore);
			}
			else if (nzbInfo && ranking)
            {
                std::string key = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key");
                std::string title = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title");
                std::lock_guard<std::mutex> guard(m_mutex);
                if (key == LowerKey(EffectiveKey(nzbInfo)) && title == ReleaseName::Normalize(nzbInfo->GetName()))
                    if (Searched* stamp = FindSearch(key, title))
                        if (stamp->title == title) m_rankPending.insert(nzbId);
                m_pending[nzbId] = Util::CurrentTime() + g_Options->GetDupeSearchDelay();
                m_cond.notify_all();
            }
			else if (nzbInfo)
			{
				// the search itself was cut off: it runs again
				std::lock_guard<std::mutex> guard(m_mutex);
                // A rename/key edit during the interrupted search must not
                // invalidate another edition's completed suppression stamp.
                std::string key = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key");
                std::string title = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title");
                if (!key.empty() && !title.empty())
                    stateErased = m_titleSearched.erase({key, title}) != 0;
                // Unknown old checkpoints cannot name an edition to invalidate.
                std::string cacheKey = key.empty() ? LowerKey(EffectiveKey(nzbInfo)) : key;
                auto latest = m_searched.find(cacheKey);
                if (latest != m_searched.end() && (latest->second.title.empty() ||
                    (!key.empty() && !title.empty() && latest->second.title == title)))
                { m_searched.erase(latest); stateErased = true; }
			}
		}
        if (stateErased && !g_Options->GetDupeSearchDryRun() && !SaveState()) continue;
		if (!fetched || job.nzbId == 0)
		{
			RemoveDir(dir);
			continue;
		}

		Note(job.nzbId, Message::mkInfo, "DupeSearch: resuming the search of %s after a restart", job.name.c_str());
		NzbSummary pick;
		NzbReader::Parse(DupeUtil::ReadAll(job.queuedFile), pick);
        // Proof belongs to the original pick. Old state without that binding is
        // searched again rather than assigning its donors to an unknown release.
        bool titleExists = FileSystem::FileExists((dir + PATH_SEPARATOR + "pick-title").c_str());
        std::string savedTitle = titleExists ? DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title") : std::string();
        std::string savedKey = titleExists ? DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key") : std::string();
        std::string savedPosting = titleExists ? DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-posting") : std::string();
        std::string currentTitle = ReleaseName::Normalize(job.name);
        std::string currentKey = LowerKey(job.dupeKey);
        std::string currentPosting = pick.Fingerprint();
        if (nzbget_rs_dqh_dupesearch_pending_identity_matches(
            titleExists ? 1 : 0,
            (const uint8_t*)savedTitle.data(), savedTitle.size(),
            (const uint8_t*)currentTitle.data(), currentTitle.size(),
            (const uint8_t*)savedKey.data(), savedKey.size(),
            (const uint8_t*)currentKey.data(), currentKey.size(),
            (const uint8_t*)savedPosting.data(), savedPosting.size(),
            (const uint8_t*)currentPosting.data(), currentPosting.size()) == 0)
        {
            Note(job.nzbId, Message::mkInfo, "DupeSearch: saved search identity changed; searching the current pick again");
            std::string oldKey = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key");
            std::string oldTitle = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title");
            if (!oldKey.empty() && !oldTitle.empty())
            {
                if (!ForgetSearch(oldKey, oldTitle)) continue;
            }
            else
            {
                bool erased = false;
                { std::lock_guard<std::mutex> guard(m_mutex);
                  auto latest = m_searched.find(LowerKey(job.dupeKey));
                  if (latest != m_searched.end() && latest->second.title.empty())
                  { m_searched.erase(latest); erased = true; } }
                if (erased && !g_Options->GetDupeSearchDryRun() && !SaveState()) continue;
            }
            RemoveDir(dir);
            Schedule(job.nzbId, Util::CurrentTime() + g_Options->GetDupeSearchDelay());
            continue;
        }
		std::vector<Posting::Sketch> known;
		for (const std::string& path : job.knownFiles)
		{
			Posting::Sketch sketch;
			if (Posting::SketchOfFile(path, sketch))
			{
				known.push_back(std::move(sketch));
			}
		}

		std::set<std::string> sent;
		{
			std::stringstream lines(DupeUtil::ReadAll(dir + PATH_SEPARATOR + "sent"));
			for (std::string line; std::getline(lines, line);)
			{
				sent.insert(line);
			}
		}

		std::vector<NzbFetcher::Fetched> verified;
		std::map<std::string, int> rejected;
		for (int i = 0;; i++)
		{
			NzbgetRsBuf baseBuf = nzbget_rs_dqh_dupesearch_format_indexed_base_path(
				dir.c_str(), PATH_SEPARATOR, (size_t)i);
			std::string base(baseBuf.data ? baseBuf.data : "", baseBuf.len);
			nzbget_rs_free(baseBuf);
			if (!FileSystem::FileExists((base + ".meta").c_str()))
			{
				break;
			}
			NzbFetcher::Fetched posting;
			std::string metaRaw = DupeUtil::ReadAll(base + ".meta");
			NzbgetRsPendingMetaView view{};
			nzbget_rs_df_parse_pending_meta(metaRaw.data(), metaRaw.size(), &view);
			posting.listing.title.assign(metaRaw.data() + view.title_offset, view.title_len);
			posting.listing.indexer.assign(metaRaw.data() + view.indexer_offset, view.indexer_len);
			posting.listing.grabs = view.grabs;
			posting.listing.size = view.size;
			posting.listing.date = (time_t)view.date;
			posting.data = DupeUtil::ReadAll(base + ".nzb");
			if (!NzbReader::Parse(posting.data, posting.info))
			{
				continue;
			}
			posting.reason = NzbFetcher::frOk;
            posting.sameInner = DupeUtil::ReadAll(base + ".inner") == "1\n";
			// the postings added before the restart are in nzbget now (ranked with
			// the key's other duplicates), or were deleted by the user since
			Posting::Sketch sketch = Posting::MakeSketch(posting.info.messageIds);
			if (sent.count(posting.info.Fingerprint()))
			{
				rejected["already-sent"]++;
				continue;
			}
			if (std::any_of(known.begin(), known.end(), [&](const Posting::Sketch& k) { return Posting::SameSketch(sketch, k); }))
			{
				rejected["in-nzbget"]++;
				continue;
			}
			verified.push_back(std::move(posting));
		}
		Place(job, pick, verified, rejected, 0, 0, 0);
	}
}

inline std::string DupeSearch::StatePath()
{
	return std::string(g_Options->GetQueueDir()) + PATH_SEPARATOR + "dupesearch";
}

inline void DupeSearch::LoadState()
{
	std::ifstream file(fs::u8path(StatePath()));
	std::string line;
	time_t now = Util::CurrentTime();
	std::lock_guard<std::mutex> guard(m_mutex);
	while (std::getline(file, line))
	{
		NzbgetRsDupeSearchStateLine parsed{};
		if (nzbget_rs_df_parse_dupesearch_state_line(line.data(), line.size(), (int64_t)now, StateTtlSec, &parsed))
		{
			if (parsed.within_ttl)
			{
				std::string key(line.data() + parsed.key_offset, parsed.key_len);
				Searched searched;
				searched.score = parsed.score;
				searched.time = (time_t)parsed.time;
				searched.title.assign(line.data() + parsed.title_offset, parsed.title_len);
				searched.retryable = parsed.retryable != 0;
				RecordSearch(key, searched);
			}
		}
	}
}

inline DupeSearch::Searched* DupeSearch::FindSearch(const std::string& lowerKey, const std::string& title)
{
    auto exact = m_titleSearched.find({lowerKey, title});
    if (exact != m_titleSearched.end()) return &exact->second;
    auto latest = m_searched.find(lowerKey);
    return latest != m_searched.end() && (latest->second.title.empty() || latest->second.title == title) ?
        &latest->second : nullptr;
}

inline void DupeSearch::RecordSearch(const std::string& lowerKey, const Searched& searched)
{
    m_searched[lowerKey] = searched;
    if (!searched.title.empty()) m_titleSearched[{lowerKey, searched.title}] = searched;
}

inline void DupeSearch::SetRetryable(const std::string& lowerKey, const std::string& title, bool retryable)
{
    if (Searched* searched = FindSearch(lowerKey, title)) searched->retryable = retryable;
    auto latest = m_searched.find(lowerKey);
    if (latest != m_searched.end() && (latest->second.title.empty() || latest->second.title == title))
        latest->second.retryable = retryable;
}

inline bool DupeSearch::ForgetSearch(const std::string& dupeKey, const std::string& title)
{
	{
		std::lock_guard<std::mutex> guard(m_mutex);
        std::string lowerKey = LowerKey(dupeKey);
        auto latest = m_searched.find(lowerKey);
        SetRetryable(lowerKey, title.empty() && latest != m_searched.end() ? latest->second.title : title, true);
	}
	if (!g_Options->GetDupeSearchDryRun())
	{
		return SaveState();
	}
    return true;
}

inline bool DupeSearch::SaveState()
{
	std::ostringstream text;
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		time_t now = Util::CurrentTime();
        auto append = [&](const std::string& key, const Searched& searched) {
            NzbgetRsBuf line = nzbget_rs_dupe_watch_format_searched_state_line(
                (int64_t)now, StateTtlSec, key.c_str(), searched.score,
                (int64_t)searched.time, searched.title.c_str(), searched.retryable ? 1 : 0);
            if (line.data && line.len > 0)
                text.write(line.data, (std::streamsize)line.len);
            nzbget_rs_free(line);
        };
        for (const auto& entry : m_titleSearched) append(entry.first.first, entry.second);
        for (const auto& entry : m_searched)
            if (!m_titleSearched.count({entry.first, entry.second.title})) append(entry.first, entry.second);
	}
	if (!DupeUtil::WriteAtomic(StatePath(), text.str()))
	{
		warn("Could not save the DupeSearch state to %s", StatePath().c_str());
        return false;
	}
    return true;
}


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef FLEET_H
#define FLEET_H

#include <string>
#include <vector>

/*
 * A fleet: several nzb-files a client believes are copies of one release,
 * sent at once (JSON-RPC method "appendfleet"). Nothing downloads before
 * their postings were checked on the configured news servers: the wholest
 * is queued, the others are kept in history as backups, already scored in
 * the order failover should try them. Same postings are checked once. One
 * fleet of a key at a time: a second waits for the first. With a download of
 * the key running, a fleet only adds backups, below it; a posting the key
 * holds already isn't added again. A member is dead below option DupeMinAlive.
 */
class Fleet
{
public:
	struct Member
	{
		std::string name;		// the nzb-file's name
		std::string data;		// its content (fetched first when url is set)
		std::string url;
	};
	struct Request
	{
		std::string dupeKey;
		std::string category;
		int priority = 0;
		int timeoutSec = DefaultTimeoutSec;
		std::vector<Member> members;
	};
	struct Entry
	{
		int nzbId = 0;
		std::string name;
		int rank = 0;
		int alive = -1;			// percent of the sampled articles found, -1 unknown
		std::string status;		// QUEUED, BACKUP, SAME_POSTING, DEAD, ERROR, SKIPPED, COPY
		int sameAs = 0;			// SAME_POSTING: the NZBID of the item it is a copy of (0: not added)
		int sameAsRank = 0;		// SAME_POSTING: the rank of the fleet member it is a copy of
		int member = -1;		// its index in the request
		std::string reason;
	};
	struct Result
	{
		int chosen = 0;			// the download now queued, 0 for none
		bool complete = true;	// every posting was checked within the time limit
		std::string reason;		// ALREADY_QUEUED (chosen: the key's running download); when chosen is 0:
							// ALREADY_DOWNLOADED, ALL_DEAD, NO_USABLE_MEMBERS, NO_MEMBERS, SHUTDOWN, NOT_QUEUED,
							// KEY_BUSY (another fleet of the key held it past the time limit),
							// INCOMPLETE (none found alive, but the time limit cut the check short)
		std::vector<Entry> members;	// in rank order
	};

	static constexpr int DefaultTimeoutSec = 45;
	static constexpr int MaxTimeoutSec = 120;
	static constexpr int MaxMembers = 50;
	// a parameter of every item a fleet adds: its rank in the fleet. A queue extension
	// (NZBPR_DupeFleet) can tell that nzbget ranked the key's copies already: ranking
	// them again tied two live backups at one score
	static constexpr const char* FleetParam = "DupeFleet";

	/* checks, ranks and adds the members; blocks for up to request.timeoutSec */
	static Result Append(Request request);
};

#include <algorithm>
#include <map>
#include <chrono>
#include <mutex>
#include <thread>
#include "nzbget_rs.h"

namespace FleetDetail
{
	inline constexpr size_t MaxNzbBytes = 64 * 1024 * 1024;
	inline constexpr int MaxParallel = 16;
	inline constexpr int FleetSampleMax = 200;
	inline constexpr long long FinishReserveMs = 2000;

	struct Candidate
	{
		size_t index;
		NzbSummary summary;
		DonorHealth::Health health;
		bool checked = false;
		int alive = -1;
		bool dead = false;
		int sameAs = -1;
		int existingId = 0;
		std::string error;
	};

	inline std::timed_mutex g_checkMutex;
	inline std::mutex g_keyLocksMutex;
	inline std::map<std::string, std::shared_ptr<std::timed_mutex>> g_keyLocks;

	inline std::shared_ptr<std::timed_mutex> KeyLock(const std::string& key)
	{
		std::lock_guard<std::mutex> guard(g_keyLocksMutex);
		std::shared_ptr<std::timed_mutex>& lock = g_keyLocks[DupeUtil::Lower(key)];
		if (!lock)
		{
			lock = std::make_shared<std::timed_mutex>();
		}
		return lock;
	}
}

inline Fleet::Result Fleet::Append(Request request)
{
	using namespace FleetDetail;
	Result result;
	request.timeoutSec = nzbget_rs_dqh_fleet_clamp_timeout_sec(
		request.timeoutSec, (int)MaxTimeoutSec);
	if (request.members.empty())
	{
		result.reason = "NO_MEMBERS";
		return result;
	}
	if (request.dupeKey.empty())
	{
		request.dupeKey = DupeSearch::MakeDupeKey(request.members[0].name);
	}
	long long deadlineMs = DonorHealth::NowMs() + request.timeoutSec * 1000LL;
	std::shared_ptr<std::timed_mutex> keyLock = KeyLock(request.dupeKey);
	std::unique_lock<std::timed_mutex> keyGuard(*keyLock, std::defer_lock);
	if (!keyGuard.try_lock_for(std::chrono::milliseconds(std::max(0LL,
		deadlineMs - DonorHealth::NowMs() - FinishReserveMs))))
	{
		result.reason = "KEY_BUSY";
		result.complete = false;
		return result;
	}

	std::vector<HttpGet::Reply> fetched(request.members.size());
	{
		std::vector<std::thread> fetchers;
		for (size_t i = 0; i < request.members.size(); i++)
		{
			if (!request.members[i].url.empty())
			{
				fetchers.emplace_back([&, i]()
					{
						fetched[i] = HttpGet::Fetch(request.members[i].url, "fleet member " + request.members[i].name,
							MaxNzbBytes, deadlineMs - FinishReserveMs);
					});
			}
		}
		for (std::thread& fetcher : fetchers)
		{
			fetcher.join();
		}
	}

	std::vector<Candidate> candidates(request.members.size());
	for (size_t i = 0; i < request.members.size(); i++)
	{
		Member& member = request.members[i];
		Candidate& candidate = candidates[i];
		candidate.index = i;
		if (!member.url.empty())
		{
			HttpGet::Reply& reply = fetched[i];
			if (!reply.ok)
			{
				NzbgetRsBuf errBuf = nzbget_rs_dqh_fleet_format_fetch_error(reply.status);
				candidate.error.assign(errBuf.data ? errBuf.data : "", errBuf.len);
				nzbget_rs_free(errBuf);
				continue;
			}
			member.data = std::move(reply.body);
		}
		if (!NzbReader::Parse(member.data, candidate.summary) || candidate.summary.messageIds.empty())
		{
			candidate.error = "the nzb-file is unreadable";
			continue;
		}
		for (size_t j = 0; j < i; j++)
		{
			if (candidates[j].error.empty() && candidates[j].sameAs < 0 &&
				Posting::SamePosting(candidates[j].summary.messageIds, candidate.summary.messageIds))
			{
				candidate.sameAs = (int)j;
				break;
			}
		}
	}

	long long top = DupeSearch::BasePickScore;
	int runningId = 0;
	int runningScore = 0;
	int lowest = 0;
	bool any = false;
	std::vector<std::pair<int, std::string>> existing;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		auto consider = [&](NzbInfo* nzbInfo, bool queued)
			{
				if (nzbInfo->GetKind() != NzbInfo::nkNzb ||
					!nzbget_rs_dqh_bytes_eq_ignore_case(nzbInfo->GetDupeKey(), request.dupeKey.c_str()))
				{
					return;
				}
				int64_t newTop = top;
				nzbget_rs_dqh_fleet_update_key_score_bounds(
					top, lowest, any ? 1 : 0, queued ? 1 : 0,
					nzbInfo->GetDeleting() ? 1 : 0,
					(int)nzbInfo->GetDeleteStatus(),
					(int)NzbInfo::dsNone,
					runningId, runningScore,
					nzbInfo->GetId(), nzbInfo->GetDupeScore(),
					&newTop, &lowest, &runningId, &runningScore);
				top = newTop;
				any = true;
				std::string names = nzbInfo->GetQueuedFilename() ? nzbInfo->GetQueuedFilename() : "";
				struct PipeCtx
				{
					std::vector<std::pair<int, std::string>>& existing;
					int id;
				} pipeCtx{existing, nzbInfo->GetId()};
				nzbget_rs_dqh_fleet_visit_pipe_names(
					names.data(), names.size(), &pipeCtx,
					[](void* ctx, const char* partPtr, size_t partLen)
					{
						auto* c = static_cast<PipeCtx*>(ctx);
						c->existing.emplace_back(c->id, std::string(partPtr, partLen));
					});
			};
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			consider(nzbInfo, true);
		}
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				consider(historyInfo->GetNzbInfo(), false);
			}
		}
	}
	for (const auto& item : existing)
	{
		NzbSummary summary;
		if (!NzbReader::Parse(DupeUtil::ReadAll(item.second), summary))
		{
			continue;
		}
		for (Candidate& candidate : candidates)
		{
			if (candidate.error.empty() && candidate.sameAs < 0 && !candidate.existingId &&
				Posting::SamePosting(summary.messageIds, candidate.summary.messageIds))
			{
				candidate.existingId = item.first;
			}
		}
	}
	{
		std::vector<Candidate*> ranked;
		for (Candidate& candidate : candidates)
		{
			ranked.push_back(&candidate);
		}
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			NzbInfo* item = historyInfo->GetKind() == HistoryInfo::hkNzb ? historyInfo->GetNzbInfo() : nullptr;
			if (!item || nzbget_rs_dqh_fleet_is_already_downloaded_history_item(
				item != nullptr ? 1 : 0,
				(int)item->GetDeleteStatus(),
				(int)NzbInfo::dsNone,
				item->IsDupeSuccess() ? 1 : 0,
				(int)item->GetMarkStatus(),
				(int)NzbInfo::ksBad) == 0 ||
				!ReleaseName::Readable(item->GetName()))
			{
				continue;
			}
			bool same = std::any_of(ranked.begin(), ranked.end(), [&](const Candidate* c)
				{
					const std::string& name = request.members[c->index].name;
					return c->error.empty() && ReleaseName::Readable(name) &&
						ReleaseName::SameRelease(name, item->GetName());
				});
			if (same && DupeCoordinator::FilesOnDisk(item))
			{
				result.reason = "ALREADY_DOWNLOADED";
				result.chosen = 0;
				int rank = 0;
				for (Candidate* candidate : ranked)
				{
					Entry entry;
					entry.name = request.members[candidate->index].name;
					entry.rank = ++rank;
					entry.alive = candidate->alive;
					entry.status = candidate->error.empty() ? "SKIPPED" : "ERROR";
					entry.reason = candidate->error.empty() ? "downloaded as " + std::string(item->GetName()) : candidate->error;
					result.members.push_back(entry);
				}
				info("Fleet for %s: %s is downloaded already", request.dupeKey.c_str(), item->GetName());
				return result;
			}
		}
	}

	std::vector<DonorHealth::Posting> postings;
	for (Candidate& candidate : candidates)
	{
		if (candidate.error.empty() && candidate.sameAs < 0 && !candidate.existingId)
		{
			NzbgetRsBuf idxBuf = nzbget_rs_dqh_conn_format_port_str(candidate.index);
			std::string idxStr(idxBuf.data ? idxBuf.data : "", idxBuf.len);
			nzbget_rs_free(idxBuf);
			postings.push_back({ std::move(idxStr), candidate.summary.messageIds,
				std::make_shared<std::vector<std::string>>(candidate.summary.groups) });
		}
	}
	DonorHealth::ServerList servers = NntpHealthServer::Servers();
	std::unique_lock<std::timed_mutex> checkGuard(g_checkMutex, std::defer_lock);
	bool mayCheck = !servers.empty() && !postings.empty() && checkGuard.try_lock_for(
		std::chrono::milliseconds(nzbget_rs_dqh_fleet_calc_check_lock_wait_ms(
			deadlineMs, DonorHealth::NowMs(), FinishReserveMs)));
	if (!servers.empty() && !postings.empty() && !mayCheck)
	{
		result.complete = false;
	}
	if (mayCheck)
	{
		DonorHealth::Options options;
		options.sample.percent = g_Options->GetDupeHealthPercent();
		options.sample.minimum = g_Options->GetDupeHealthMin();
		options.sample.maxBody = g_Options->GetDupeBodyChecks();
		long long checkStart = DonorHealth::NowMs();
		int64_t checkDeadlineMs = 0;
		nzbget_rs_dqh_fleet_calc_check_budget_and_deadline(
			g_Options->GetDupeHealthMax(), FleetSampleMax,
			deadlineMs, checkStart, FinishReserveMs,
			&options.sample.maximum, &options.budgetMs, &checkDeadlineMs);
		options.deadlineMs = checkDeadlineMs;
		std::mutex mutex;
		{
			g_WorkState->HoldDownloadForFleet(true);
			struct HoldRelease
			{
				~HoldRelease() { g_WorkState->HoldDownloadForFleet(false); }
			} holdRelease;
			DonorHealth::CheckPostings(servers, postings, options, true,
				std::min((int)postings.size(), MaxParallel),
				[&](const std::string& key, const DonorHealth::Health& health)
				{
					std::lock_guard<std::mutex> guard(mutex);
					Candidate& candidate = candidates[(size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str())];
					candidate.health = health;
					candidate.checked = true;
				});
		}
		result.complete = DonorHealth::NowMs() - checkStart < options.budgetMs;
	}
	for (Candidate& candidate : candidates)
	{
		if (!candidate.error.empty() || candidate.sameAs >= 0)
		{
			continue;
		}
		int alivePct = -1, isDead = 0, memberComplete = 0;
		nzbget_rs_df_eval_fleet_member_health(
			candidate.checked ? 1 : 0, result.complete ? 1 : 0,
			candidate.health.present, candidate.health.missing, candidate.health.error,
			DonorHealth::MinKnown, g_Options->GetDupeMinAlive(),
			&alivePct, &isDead, &memberComplete);
		candidate.alive = alivePct;
		candidate.dead = isDead != 0;
		result.complete &= memberComplete != 0;
	}
	for (Candidate& candidate : candidates)
	{
		if (candidate.sameAs >= 0)
		{
			candidate.alive = candidates[candidate.sameAs].alive;
			candidate.dead = candidates[candidate.sameAs].dead;
		}
	}

	std::vector<uint8_t> hasError(candidates.size());
	std::vector<int> sameAs(candidates.size());
	std::vector<uint8_t> isDeadArr(candidates.size());
	std::vector<int> aliveArr(candidates.size());
	std::vector<int64_t> totalBytesArr(candidates.size());
	std::vector<size_t> rankedIdx(candidates.size());
	for (size_t i = 0; i < candidates.size(); i++)
	{
		hasError[i] = candidates[i].error.empty() ? 0 : 1;
		sameAs[i] = candidates[i].sameAs;
		isDeadArr[i] = candidates[i].dead ? 1 : 0;
		aliveArr[i] = candidates[i].alive;
		totalBytesArr[i] = candidates[i].summary.totalBytes;
	}
	size_t rankedCount = nzbget_rs_df_rank_fleet_candidates(
		hasError.data(), sameAs.data(), isDeadArr.data(), aliveArr.data(), totalBytesArr.data(),
		candidates.size(), rankedIdx.data());
	std::vector<Candidate*> order;
	std::vector<Candidate*> ranked;
	ranked.reserve(rankedCount);
	for (size_t i = 0; i < rankedCount; i++)
	{
		Candidate* c = &candidates[rankedIdx[i]];
		ranked.push_back(c);
		if (c->sameAs < 0)
		{
			order.push_back(c);
		}
	}
	auto tier = [](const Candidate& c)
	{
		return nzbget_rs_dqh_fleet_candidate_tier(
			!c.error.empty() ? 1 : 0, c.dead ? 1 : 0, c.alive);
	};

	if (DonorHealth::Stopping() || HttpGet::Stopped())
	{
		result.reason = "SHUTDOWN";
		result.complete = false;
		return result;
	}

	bool anyAlive = std::any_of(order.begin(), order.end(),
		[&](const Candidate* c) { return tier(*c) <= 1; });
	std::vector<int> added(candidates.size(), 0);
	int rank = 0;
	for (Candidate* candidate : ranked)
	{
		Member& member = request.members[candidate->index];
		Entry entry;
		entry.member = (int)candidate->index;
		entry.name = member.name;
		entry.rank = ++rank;
		entry.alive = candidate->alive;
		if (!candidate->error.empty())
		{
			entry.status = "ERROR";
			entry.reason = candidate->error;
		}
		else if (candidate->sameAs >= 0 || candidate->existingId)
		{
			entry.status = "SAME_POSTING";
			entry.sameAs = candidate->existingId ? candidate->existingId : added[candidate->sameAs];
			for (const Entry& other : result.members)
			{
				if (!candidate->existingId && other.member == candidate->sameAs)
				{
					entry.sameAsRank = other.rank;
				}
			}
		}
		else if (!anyAlive)
		{
			entry.status = "DEAD";
		}
		else
		{
			NzbParameterList parameters;
			NzbgetRsBuf rankBuf = nzbget_rs_dqh_conn_format_port_str(entry.rank);
			parameters.SetParameter(FleetParam, rankBuf.data ? rankBuf.data : "0");
			nzbget_rs_free(rankBuf);
			if (candidate->alive >= 0)
			{
				NzbgetRsBuf aliveBuf = nzbget_rs_dqh_conn_format_port_str(
					candidate->dead ? 0 : candidate->alive);
				parameters.SetParameter(DupeSearch::AliveParam, aliveBuf.data ? aliveBuf.data : "0");
				nzbget_rs_free(aliveBuf);
			}
			int nzbId = 0;
			NzbgetRsBuf nameBuf = nzbget_rs_dqh_fleet_ensure_nzb_ext(member.name.data(), member.name.size());
			std::string name(nameBuf.data ? nameBuf.data : "", nameBuf.len);
			nzbget_rs_free(nameBuf);
			int memberScore = nzbget_rs_dqh_fleet_calc_member_add_score(
				runningId, lowest, top, entry.rank);
			Scanner::EAddStatus status = g_Scanner->AddExternalFile(name.c_str(), request.category.c_str(),
				false, request.priority, request.dupeKey.c_str(),
				memberScore, dmScore, &parameters,
				false, false, nullptr, nullptr, member.data.data(), (int)member.data.size(), &nzbId);
			entry.nzbId = status == Scanner::asSuccess ? nzbId : 0;
			added[candidate->index] = entry.nzbId;
			entry.status = entry.nzbId == 0 ? "ERROR" : candidate->dead ? "DEAD" : "BACKUP";
			if (entry.nzbId == 0)
			{
				entry.reason = "RustyGet could not add the nzb-file";
			}
		}
		result.members.push_back(entry);
	}

	bool onDisk = false;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (runningId && downloadQueue->GetQueue()->Find(runningId))
		{
			result.chosen = runningId;
			result.reason = "ALREADY_QUEUED";
		}
		for (Entry& entry : result.members)
		{
			if (!runningId && !result.chosen && entry.nzbId > 0 && downloadQueue->GetQueue()->Find(entry.nzbId))
			{
				entry.status = "QUEUED";
				result.chosen = entry.nzbId;
			}
		}
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() != HistoryInfo::hkNzb ||
				historyInfo->GetNzbInfo()->GetDeleteStatus() != NzbInfo::dsCopy)
			{
				continue;
			}
			for (Entry& entry : result.members)
			{
				if (entry.nzbId == historyInfo->GetNzbInfo()->GetId())
				{
					entry.status = "COPY";
					entry.reason = "the same nzb-file as an item RustyGet has already";
				}
			}
		}
		NzbInfo probe;
		probe.SetDupeKey(request.dupeKey.c_str());
		onDisk = DupeCoordinator::DownloadedOnDisk(downloadQueue, &probe) != nullptr;
	}
	if (result.chosen == 0 && result.reason.empty())
	{
		bool anyUsable = std::any_of(candidates.begin(), candidates.end(),
			[](const Candidate& c) { return c.error.empty(); });
		bool anyUnknown = std::any_of(order.begin(), order.end(),
			[&](const Candidate* c) { return c->error.empty() && c->alive < 0; });
		int reasonCode = nzbget_rs_df_eval_fleet_outcome_reason(
			anyUsable ? 1 : 0, anyAlive ? 1 : 0, anyUnknown ? 1 : 0, onDisk ? 1 : 0);
		static const char* const kReasons[] = {
			"NO_USABLE_MEMBERS", "INCOMPLETE", "ALL_DEAD", "ALREADY_DOWNLOADED", "NOT_QUEUED"
		};
		result.reason = kReasons[(reasonCode >= 0 && reasonCode < 5) ? reasonCode : 4];
	}
	NzbgetRsBuf sumBuf = nzbget_rs_dqh_fleet_format_summary_log(
		(int)request.members.size(), request.dupeKey.c_str(),
		result.chosen, result.reason.c_str(), result.complete ? 1 : 0);
	info("%s", sumBuf.data ? sumBuf.data : "");
	nzbget_rs_free(sumBuf);
	return result;
}

#endif

#endif

#include <set>
#include "nzbget_rs.h"

/**
 * Removes old entries from (recent) history
 */
inline void HistoryCoordinator::ServiceWork()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	// in 64 bits: past 24,855 days the int product went negative, and every item was
	// hidden as too old
	time_t minTime = (time_t)nzbget_rs_history_coord_calc_min_time(
		(int64)Util::CurrentTime(), g_Options->GetKeepHistory());
	bool changed = false;
	int index = 0;

	// traversing in a reverse order to delete items in order they were added to history
	// (just to produce the log-messages in a more logical order)
	for (HistoryList::reverse_iterator it = downloadQueue->GetHistory()->rbegin(); it != downloadQueue->GetHistory()->rend(); )
	{
		HistoryInfo* historyInfo = (*it).get();
		int action = nzbget_rs_history_coord_classify_service_work_item(
			(int)historyInfo->GetKind(),
			(int)HistoryInfo::hkDup,
			(int)HistoryInfo::hkNzb,
			(int64)historyInfo->GetTime(),
			(int64)minTime,
			g_Options->GetDupeCheck() ? 1 : 0);
		if (action != 0)
		{
			if (action == 1)
			{
				HistoryHide(downloadQueue, historyInfo, index);
				index++;
			}
			else
			{
				if (action == 2)
				{
					DeleteDiskFiles(historyInfo->GetNzbInfo());
				}
				info("Collection %s removed from history", historyInfo->GetName());

				downloadQueue->GetHistory()->erase(downloadQueue->GetHistory()->end() - 1 - index);
			}

			it = downloadQueue->GetHistory()->rbegin() + index;
			changed = true;
		}
		else
		{
			++it;
			index++;
		}
	}

	if (changed)
	{
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

inline void HistoryCoordinator::DeleteDiskFiles(NzbInfo* nzbInfo)
{
	if (g_Options->GetServerMode())
	{
		// delete parked files
		g_DiskState->DiscardFiles(nzbInfo);
	}
	nzbInfo->GetFileList()->clear();

	// delete nzb-file
	if (!g_Options->GetNzbCleanupDisk())
	{
		return;
	}

	// QueuedFile may contain one filename or several filenames separated
	// with "|"-character (for merged groups)
	const char* filename = nzbInfo->GetQueuedFilename() ? nzbInfo->GetQueuedFilename() : "";
	Tokenizer tok(filename, "|");
	while (const char* name1 = tok.Next())
	{
		if (FileSystem::FileExists(name1))
		{
			info("Deleting file %s", name1);
			FileSystem::DeleteFile(name1);
		}
	}
}

inline void HistoryCoordinator::AddToHistory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	// catch-all for every path into history: a still-attached live
	// stream-repair thread (option <DupeArticleFallback> value "live") could
	// never find this collection by id again (the pass searches the QUEUE),
	// so it would self-destruct and leave this slot a dangling pointer
	if (nzbInfo->GetLiveRepairThread())
	{
		Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
		nzbInfo->SetLiveRepairThread(nullptr);
		liveRepairThread->Stop();
	}

	std::unique_ptr<NzbInfo> oldNzbInfo = downloadQueue->GetQueue()->Remove(nzbInfo);
	std::unique_ptr<HistoryInfo> historyInfo = std::make_unique<HistoryInfo>(std::move(oldNzbInfo));
	historyInfo->SetTime(Util::CurrentTime());
	downloadQueue->GetHistory()->Add(std::move(historyInfo), true);
	downloadQueue->HistoryChanged();

	// park remaining files
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		nzbInfo->UpdateCompletedStats(fileInfo);
		nzbInfo->GetCompletedFiles()->emplace_back(
			fileInfo->GetId(),
			fileInfo->GetFilename(),
			fileInfo->GetOrigname() ? fileInfo->GetOrigname() : "",
			CompletedFile::cfNone,
			0,
			fileInfo->GetParFile(),
			fileInfo->GetHash16k() ? fileInfo->GetHash16k() : "",
			fileInfo->GetParSetId() ? fileInfo->GetParSetId() : ""
		);
	}

	// Cleaning up parked files if par-check was successful or unpack was successful or
	// health is 100% (if unpack and par-check were not performed) or if deleted
	bool cleanupParkedFiles = nzbget_rs_qhp_should_cleanup_parked_files(
		(int32_t)nzbInfo->GetParStatus(),
		(int32_t)nzbInfo->GetUnpackStatus(),
		nzbInfo->GetFailedSize(),
		nzbInfo->GetParFailedSize(),
		(int32_t)nzbInfo->GetDeleteStatus(),
		nzbInfo->GetParking() ? 1 : 0,
		nzbInfo->GetUnpackCleanedUpDisk() ? 1 : 0
	) != 0;

	if (cleanupParkedFiles)
	{
		g_DiskState->DiscardFiles(nzbInfo, false);
		nzbInfo->GetCompletedFiles()->clear();
	}

	nzbInfo->SetParkedFileCount(0);
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		bool isLast = &completedFile == &*nzbInfo->GetCompletedFiles()->rbegin();
		if (nzbget_rs_history_coord_should_park_completed_file(
			(int)completedFile.GetStatus(),
			(int)CompletedFile::cfNone,
			(int)CompletedFile::cfPartial,
			isLast ? 1 : 0) != 0)
		{
			nzbInfo->PrintMessage(Message::mkDetail, "Parking file %s", completedFile.GetFilename());
			nzbInfo->SetParkedFileCount(nzbInfo->GetParkedFileCount() + 1);
		}
	}

	nzbInfo->GetFileList()->clear();
	nzbInfo->SetRemainingParCount(0);
	nzbInfo->SetParking(false);

	if (nzbInfo->GetDirectRenameStatus() == NzbInfo::tsRunning)
	{
		nzbInfo->SetDirectRenameStatus(NzbInfo::tsFailure);
	}
	if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsRunning)
	{
		nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
	}

	nzbInfo->SetDupeHint(NzbInfo::dhNone);

	nzbInfo->PrintMessage(Message::mkInfo, "Collection %s added to history", nzbInfo->GetName());

	RecheckFailedArticles(downloadQueue, nzbInfo);
	ParDamage::Forget(nzbInfo->GetId());
}

/*
 * A download that failed with articles missing gets its failed articles asked of
 * the servers once (DupeProbe::StartRecheck): when most of them exist after all,
 * a timeout or a dropped connection lost them, and they are retried. Only where
 * duplicate handling judges downloads by their article results (option
 * <DupeArticleFallback>, HealthCheck=dupe), only from files whose state was kept,
 * and not while a duplicate downloads in its place.
 */
inline void HistoryCoordinator::RecheckFailedArticles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (nzbget_rs_history_coord_should_recheck_failed_articles(
		g_Options->GetDupeArticleFallback() == Options::dafNone ? 1 : 0,
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0,
		nzbInfo->GetKind() == NzbInfo::nkNzb ? 1 : 0,
		nzbInfo->GetFailedArticles(),
		nzbInfo->MakeTextStatus(true),
		nzbInfo->GetParameters()->Find("*DupeRecheck") != nullptr ? 1 : 0) == 0)
	{
		return;
	}
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (DupeCoordinator::SameNameOrKey(queued->GetName(), queued->GetDupeKey(),
			nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			return;	// a duplicate downloads in its place
		}
	}
	// a duplicate waits in history: nzbget fetches it in its place (B39)
	if (g_Options->GetDupeCheck() &&
		nzbInfo->GetDupeMode() == dmScore &&
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()))
	{
		return;
	}
	// once per download
	nzbInfo->GetParameters()->SetParameter("*DupeRecheck", "1");

	std::vector<DupeProbe::Sample> failed;
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if ((completedFile.GetStatus() != CompletedFile::cfPartial &&
			 completedFile.GetStatus() != CompletedFile::cfFailure) ||
			completedFile.GetId() <= 0)
		{
			continue;
		}
		bool partial = completedFile.GetStatus() == CompletedFile::cfPartial;
		FileInfo fileInfo(completedFile.GetId());
		if (!g_DiskState->LoadFile(&fileInfo, false, true) ||
			(partial && !g_DiskState->LoadFileState(&fileInfo, g_ServerPool->GetServers(), true)))
		{
			continue;
		}
		auto groups = std::make_shared<std::vector<CString>>();
		for (const CString& group : *fileInfo.GetGroups())
		{
			groups->emplace_back(*group);
		}
		for (ArticleInfo* article : fileInfo.GetArticles())
		{
			if ((!partial || article->GetStatus() == ArticleInfo::aiFailed) &&
				!Util::EmptyStr(article->GetMessageId()))
			{
				DupeProbe::Sample sample;
				sample.MessageId = article->GetMessageId();
				sample.Groups = groups;
				failed.push_back(std::move(sample));
			}
		}
	}
	if (failed.size() < 2)
	{
		return;
	}

	std::vector<DupeProbe::Sample> samples;
	for (size_t index : DupeProbe::SampleIndexes(failed.size(), DupeProbe::RecheckSampleCount))
	{
		samples.push_back(std::move(failed[index]));	// the indexes are distinct
	}
	DupeProbe::StartRecheck(nzbInfo->GetId(), std::move(samples));
}

inline void HistoryCoordinator::HistoryHide(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, int rindex)
{
	// replace history element
	std::unique_ptr<DupInfo> dupInfo = std::make_unique<DupInfo>();
	dupInfo->SetId(historyInfo->GetNzbInfo()->GetId());
	dupInfo->SetName(historyInfo->GetNzbInfo()->GetName());
	dupInfo->SetDupeKey(historyInfo->GetNzbInfo()->GetDupeKey());
	dupInfo->SetDupeScore(historyInfo->GetNzbInfo()->GetDupeScore());
	dupInfo->SetDupeMode(historyInfo->GetNzbInfo()->GetDupeMode());
	dupInfo->SetSize(historyInfo->GetNzbInfo()->GetSize());
	dupInfo->SetFullContentHash(historyInfo->GetNzbInfo()->GetFullContentHash());
	dupInfo->SetFilteredContentHash(historyInfo->GetNzbInfo()->GetFilteredContentHash());

	dupInfo->SetStatus((DupInfo::EStatus)nzbget_rs_qhp_classify_dup_status(
		(int32_t)historyInfo->GetNzbInfo()->GetMarkStatus(),
		(int32_t)historyInfo->GetNzbInfo()->GetDeleteStatus(),
		historyInfo->GetNzbInfo()->IsDupeSuccess() ? 1 : 0
	));

	std::unique_ptr<HistoryInfo> newHistoryInfo = std::make_unique<HistoryInfo>(std::move(dupInfo));
	newHistoryInfo->SetTime(historyInfo->GetTime());

	DeleteDiskFiles(historyInfo->GetNzbInfo());

	info("Collection %s removed from history", historyInfo->GetName());

	(*downloadQueue->GetHistory())[downloadQueue->GetHistory()->size() - 1 - rindex] = std::move(newHistoryInfo);
}

inline void HistoryCoordinator::PrepareEdit(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action)
{
	// First pass: when marking multiple items - mark them bad without performing the mark-logic,
	// this will later (on second step) avoid moving other items to download queue, if they are marked bad too.
	for (int id : *idList)
	{
		HistoryInfo* historyInfo = downloadQueue->GetHistory()->Find(id);
		if (action == DownloadQueue::eaHistoryMarkBad &&
			historyInfo && historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			historyInfo->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
		}
	}
}

inline bool HistoryCoordinator::EditList(DownloadQueue* downloadQueue, IdList* idList,
	DownloadQueue::EEditAction action, const char* args)
{
	bool ok = false;
	PrepareEdit(downloadQueue, idList, action);

	for (int id : *idList)
	{
		for (HistoryList::iterator itHistory = downloadQueue->GetHistory()->begin(); itHistory != downloadQueue->GetHistory()->end(); ++itHistory)
		{
			HistoryInfo* historyInfo = (*itHistory).get();
			if (historyInfo->GetId() == id)
			{
				// one item's edit failing doesn't undo (or leave unsaved) the
				// others: the list counts as edited when any item was
				bool done = true;

				switch (action)
				{
					case DownloadQueue::eaHistoryDelete:
					case DownloadQueue::eaHistoryFinalDelete:
						HistoryDelete(downloadQueue, itHistory, historyInfo, action == DownloadQueue::eaHistoryFinalDelete);
						break;

					case DownloadQueue::eaHistoryReturn:
						HistoryReturn(downloadQueue, itHistory, historyInfo);
						break;

					case DownloadQueue::eaHistoryProcess:
						HistoryProcess(downloadQueue, itHistory, historyInfo);
						break;

					case DownloadQueue::eaHistoryRedownload:
						HistoryRedownload(downloadQueue, itHistory, historyInfo, false);
						break;

					case DownloadQueue::eaHistoryRetryFailed:
						HistoryRetry(downloadQueue, itHistory, historyInfo, true, false);
						break;

					case DownloadQueue::eaHistorySetParameter:
						done = HistorySetParameter(historyInfo, args);
						break;

 					case DownloadQueue::eaHistorySetCategory:
						done = HistorySetCategory(historyInfo, args);
						break;

 					case DownloadQueue::eaHistorySetName:
						done = HistorySetName(historyInfo, args);
						break;

					case DownloadQueue::eaHistorySetDupeKey:
					case DownloadQueue::eaHistorySetDupeScore:
					case DownloadQueue::eaHistorySetDupeMode:
					case DownloadQueue::eaHistorySetDupeBackup:
						HistorySetDupeParam(historyInfo, action, args);
						break;

					case DownloadQueue::eaHistoryMarkBad:
						g_DupeCoordinator->HistoryMark(downloadQueue, historyInfo, NzbInfo::ksBad);
						break;

					case DownloadQueue::eaHistoryMarkGood:
						g_DupeCoordinator->HistoryMark(downloadQueue, historyInfo, NzbInfo::ksGood);
						break;

					case DownloadQueue::eaHistoryMarkSuccess:
						g_DupeCoordinator->HistoryMark(downloadQueue, historyInfo, NzbInfo::ksSuccess);
						break;

					default:
						// nothing, just to avoid compiler warning
						break;
				}

				ok = ok || done;
				break;
			}
		}
	}

	if (ok)
	{
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}

	return ok;
}

inline void HistoryCoordinator::HistoryDelete(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool final)
{
	info("Deleting %s from history", historyInfo->GetName());

	if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		DeleteDiskFiles(historyInfo->GetNzbInfo());
	}

	bool isNzb = historyInfo->GetKind() == HistoryInfo::hkNzb;
	if (isNzb && nzbget_rs_history_coord_should_delete_dest_dir(
		1,
		(int)historyInfo->GetNzbInfo()->GetDeleteStatus(), (int)NzbInfo::dsNone,
		(int)historyInfo->GetNzbInfo()->GetParStatus(), (int)NzbInfo::psFailure,
		(int)historyInfo->GetNzbInfo()->GetUnpackStatus(), (int)NzbInfo::usFailure,
		(int)NzbInfo::usPassword,
		FileSystem::DirectoryExists(historyInfo->GetNzbInfo()->GetDestDir()) ? 1 : 0) != 0)
	{
		info("Deleting %s", historyInfo->GetNzbInfo()->GetDestDir());
		CString errmsg;
		if (!FileSystem::DeleteDirectoryWithContent(historyInfo->GetNzbInfo()->GetDestDir(), errmsg))
		{
			error("Could not delete directory %s: %s", historyInfo->GetNzbInfo()->GetDestDir(), *errmsg);
		}
	}

	if (final || !g_Options->GetDupeCheck() || historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		downloadQueue->GetHistory()->erase(itHistory);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		// replace history element
		int rindex = (int)downloadQueue->GetHistory()->size() - 1 - (int)(itHistory - downloadQueue->GetHistory()->begin());
		HistoryHide(downloadQueue, historyInfo, rindex);
	}
}

inline void HistoryCoordinator::MoveToQueue(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool reprocess)
{
	debug("Returning %s from history back to download queue", historyInfo->GetName());
	CString nicename = historyInfo->GetName();
	NzbInfo* nzbInfo = historyInfo->GetNzbInfo();

	// unpark files
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		nzbInfo->PrintMessage(Message::mkDetail, "Unparking file %s", fileInfo->GetFilename());
	}

	downloadQueue->GetQueue()->Add(std::unique_ptr<NzbInfo>(nzbInfo), true);
	historyInfo->DiscardNzbInfo();

	// reset postprocessing status variables
	bool hasMainPars = !nzbInfo->GetUnpackCleanedUpDisk() &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr);
	int newPostTotalSec = nzbInfo->GetPostTotalSec();
	int newMoveStatus = (int)nzbInfo->GetMoveStatus();
	nzbget_rs_history_coord_calc_move_to_queue_reset(
		nzbInfo->GetUnpackCleanedUpDisk() ? 1 : 0,
		nzbInfo->GetPostTotalSec(),
		nzbInfo->GetUnpackSec(),
		hasMainPars ? 1 : 0,
		nzbInfo->GetParSec(),
		(int)nzbInfo->GetMoveStatus(),
		(int)NzbInfo::msFailure,
		(int)NzbInfo::msNone,
		&newPostTotalSec,
		&newMoveStatus);
	if (!nzbInfo->GetUnpackCleanedUpDisk())
	{
		nzbInfo->SetUnpackStatus(NzbInfo::usNone);
		nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
		nzbInfo->SetParRepaired(false);
		nzbInfo->SetUnpackTried(false);
		nzbInfo->SetLastUnpackStatus(0);
		nzbInfo->GetExtractedArchives()->clear();
		nzbInfo->SetCleanupStatus(NzbInfo::csNone);
		nzbInfo->SetParRenameStatus(NzbInfo::rsNone);
		nzbInfo->SetRarRenameStatus(NzbInfo::rsNone);
		nzbInfo->SetPostTotalSec(newPostTotalSec);
		nzbInfo->SetUnpackSec(0);

		if (hasMainPars)
		{
			nzbInfo->SetParStatus(NzbInfo::psNone);
			nzbInfo->SetParSec(0);
			nzbInfo->SetRepairSec(0);
			nzbInfo->SetParFull(false);
		}
	}
	nzbInfo->SetDeleteStatus(NzbInfo::dsNone);
	nzbInfo->SetDeletePaused(false);
	nzbInfo->SetDupeFailoverChecked(-1);
	nzbInfo->SetDeadPickProbed(false);
	nzbInfo->SetDupeAttemptedArticles(0);
	nzbInfo->SetDupeUnsourcedArticles(0);
	nzbInfo->SetDupeFailedRun(0);
	nzbInfo->SetDupeParDeferState(NzbInfo::dpNone);
	nzbInfo->SetMarkStatus(NzbInfo::ksNone);
	nzbInfo->GetScriptStatuses()->clear();
	nzbInfo->SetParkedFileCount(0);
	nzbInfo->SetMoveStatus((NzbInfo::EMoveStatus)newMoveStatus);
	nzbInfo->SetReprocess(reprocess);
	nzbInfo->SetFinalDir("");

	downloadQueue->GetHistory()->erase(itHistory);
	// the object "pHistoryInfo" is released few lines later, after the call to "NZBDownloaded"
	nzbInfo->PrintMessage(Message::mkInfo, "%s returned from history back to download queue", *nicename);

	if (reprocess)
	{
		// start postprocessing
		debug("Restarting postprocessing for %s", *nicename);
		g_PrePostProcessor->NzbDownloaded(downloadQueue, nzbInfo);
		DownloadQueue::Aspect aspect = {DownloadQueue::eaNzbReturned, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);
	}
}

inline void HistoryCoordinator::HistoryRedownload(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool restorePauseState)
{
	if (historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		historyInfo->DiscardNzbInfo();
		nzbInfo->SetUrlStatus(NzbInfo::lsNone);
		nzbInfo->SetDeleteStatus(NzbInfo::dsNone);
		if (nzbInfo->GetDupeHint() == NzbInfo::dhNone)
		{
			nzbInfo->SetDupeHint(NzbInfo::dhRedownloadManual);
		}
		downloadQueue->GetQueue()->Add(std::unique_ptr<NzbInfo>(nzbInfo), true);
		downloadQueue->GetHistory()->erase(itHistory);

		DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlReturned, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);

		return;
	}

	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not download again %s: history item has wrong type", historyInfo->GetName());
		return;
	}

	NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
	bool paused = restorePauseState && nzbInfo->GetDeletePaused();

	if (!FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
	{
		error("Could not download again %s: could not find source nzb-file %s",
			nzbInfo->GetName(), nzbInfo->GetQueuedFilename());
		return;
	}

	NzbFile nzbFile(nzbInfo->GetQueuedFilename(), "");
	if (!nzbFile.Parse())
	{
		error("Could not download again %s: could not parse nzb-file", nzbInfo->GetName());
		return;
	}

	info("Downloading again %s", nzbInfo->GetName());

	std::unique_ptr<NzbInfo> newNzbInfo = nzbFile.DetachNzbInfo();

	for (FileInfo* fileInfo : newNzbInfo->GetFileList())
	{
		fileInfo->SetPaused(paused);
	}

	if (FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
	{
		detail("Deleting %s", nzbInfo->GetDestDir());
		CString errmsg;
		if (!FileSystem::DeleteDirectoryWithContent(nzbInfo->GetDestDir(), errmsg))
		{
			error("Could not delete directory %s: %s", nzbInfo->GetDestDir(), *errmsg);
		}
	}

	nzbInfo->BuildDestDirName();
	if (FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
	{
		detail("Deleting %s", nzbInfo->GetDestDir());
		CString errmsg;
		if (!FileSystem::DeleteDirectoryWithContent(nzbInfo->GetDestDir(), errmsg))
		{
			error("Could not delete directory %s: %s", nzbInfo->GetDestDir(), *errmsg);
		}
	}

	g_DiskState->DiscardFiles(nzbInfo);

	// reset status fields (which are not reset by "MoveToQueue")
	nzbInfo->SetMoveStatus(NzbInfo::msNone);
	nzbInfo->SetUnpackCleanedUpDisk(false);
	nzbInfo->SetParStatus(NzbInfo::psNone);
	nzbInfo->SetParRenameStatus(NzbInfo::rsNone);
	nzbInfo->SetRarRenameStatus(NzbInfo::rsNone);
	nzbInfo->SetDirectRenameStatus(NzbInfo::tsNone);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
	nzbInfo->SetParRepaired(false);
	nzbInfo->SetUnpackTried(false);
	nzbInfo->SetLastUnpackStatus(0);
	nzbInfo->GetExtractedArchives()->clear();
	nzbInfo->SetHealthPaused(false);
	nzbInfo->SetDownloadedSize(0);
	nzbInfo->SetDownloadSec(0);
	nzbInfo->SetPostTotalSec(0);
	nzbInfo->SetParSec(0);
	nzbInfo->SetRepairSec(0);
	nzbInfo->SetUnpackSec(0);
	nzbInfo->SetExtraParBlocks(0);
	nzbInfo->SetAllFirst(false);
	nzbInfo->SetWaitingPar(false);
	nzbInfo->SetLoadingPar(false);
	nzbInfo->GetCompletedFiles()->clear();
	// stream-repair jobs captured for the previous download attempt are stale
	nzbInfo->GetStreamRepairJobs()->clear();
	// and so are its duplicate recoveries: the failover gates weigh them against
	// this attempt's article counts, which start at 0 - a stale count made a
	// download that arrives look dead (failed over to the next backup), or a dead
	// one look borrowed enough to be spared. Blocked donors stay blocked: they
	// were proven to hold other bytes
	nzbInfo->SetDupeRecoveredArticles(0);
	nzbInfo->SetDupeRecoveredBytes(0);
	nzbInfo->SetDupeRecoveredHoles(0);
	nzbInfo->SetDupeParDeferState(NzbInfo::dpNone);
	nzbInfo->SetDupeNoParNoted(false);
	nzbInfo->GetServerStats()->clear();
	nzbInfo->GetCurrentServerStats()->clear();

	nzbInfo->MoveFileList(newNzbInfo.get());

	g_QueueCoordinator->CheckDupeFileInfos(nzbInfo);

	MoveToQueue(downloadQueue, itHistory, historyInfo, false);

	g_PrePostProcessor->NzbAdded(downloadQueue, nzbInfo);

	DownloadQueue::Aspect aspect = {DownloadQueue::eaNzbReturned, downloadQueue, nzbInfo, nullptr};
	downloadQueue->Notify(&aspect);
}

inline void HistoryCoordinator::HistoryReturn(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo)
{
	if (historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		HistoryRedownload(downloadQueue, itHistory, historyInfo, false);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetParkedFileCount() == 0)
	{
		warn("Could not download remaining files for %s: history item does not have any files left for download", historyInfo->GetName());
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		HistoryRetry(downloadQueue, itHistory, historyInfo, false, false);
	}
	else
	{
		error("Could not download remaining files for %s: history item has wrong type", historyInfo->GetName());
	}
}

inline void HistoryCoordinator::HistoryProcess(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo)
{
	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not post-process again %s: history item has wrong type", historyInfo->GetName());
		return;
	}

	HistoryRetry(downloadQueue, itHistory, historyInfo, false, true);
}

inline void HistoryCoordinator::HistoryRetry(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool resetFailed, bool reprocess)
{
	const char* wrongTypeVerb = nullptr;
	const char* missingDirVerb = nullptr;
	const char* startMsgVerb = nullptr;
	nzbget_rs_history_coord_retry_action_labels(
		resetFailed ? 1 : 0, reprocess ? 1 : 0,
		&wrongTypeVerb, &missingDirVerb, &startMsgVerb);

	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not %s for %s: history item has wrong type",
			wrongTypeVerb,
			historyInfo->GetName());
		return;
	}

	NzbInfo* nzbInfo = historyInfo->GetNzbInfo();

	if (!FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
	{
		error("Could not %s %s: destination directory %s doesn't exist",
			missingDirVerb,
			historyInfo->GetName(), nzbInfo->GetDestDir());
		return;
	}

	nzbInfo->PrintMessage(Message::mkInfo, "%s %s",
		startMsgVerb,
		nzbInfo->GetName());

	// the files that download again: their repair jobs describe the old attempt
	std::set<int> requeued;

	// move failed completed files to (parked) file list
	for (CompletedFileList::iterator it = nzbInfo->GetCompletedFiles()->begin(); it != nzbInfo->GetCompletedFiles()->end(); )
	{
		CompletedFile& completedFile = *it;
		if (completedFile.GetStatus() != CompletedFile::cfSuccess &&
			(completedFile.GetStatus() != CompletedFile::cfFailure || resetFailed) &&
			completedFile.GetId() > 0)
		{
			std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>();
			fileInfo->SetId(completedFile.GetId());
			bool fileLoaded = g_DiskState->LoadFile(fileInfo.get(), true, true);
			bool stateLoaded = fileLoaded &&
				completedFile.GetStatus() == CompletedFile::cfPartial &&
				g_DiskState->LoadFileState(fileInfo.get(), g_ServerPool->GetServers(), true);
			if (fileLoaded &&
				nzbget_rs_history_coord_eval_requeue_loaded_file(
					(int)completedFile.GetStatus(),
					(int)CompletedFile::cfNone,
					(int)CompletedFile::cfFailure,
					(int)CompletedFile::cfPartial,
					resetFailed ? 1 : 0,
					stateLoaded ? 1 : 0,
					fileInfo->GetRemainingSize()) != 0)
			{
				// a file parked while it downloaded is recorded by its temporary output
				// name ("<id>.out.tmp"): its data is there, but it keeps the name its
				// nzb-file gives it, which a recreation from a duplicate pairs by
				std::string recordedName = completedFile.GetFilename();
				ArticleList* articles = fileInfo->GetArticles();
				std::vector<int> artStatuses;
				artStatuses.reserve(articles->size());
				for (const std::unique_ptr<ArticleInfo>& article : *articles)
				{
					artStatuses.push_back((int)article->GetStatus());
				}

				auto fileSizeCb = [](void*, const char* path) -> int64_t {
					return FileSystem::FileSize(path);
				};
				auto deleteFileCb = [](void*, const char* path) {
					FileSystem::DeleteFile(path);
				};
				auto fileExistsCb = [](void*, const char* path) -> int {
					return FileSystem::FileExists(path) ? 1 : 0;
				};

				int setRecordedFilename = 0;
				int newPartialState = (int)fileInfo->GetPartialState();
				int warnMissingOnDisk = 0;
				int noRetryableArticles = 0;
				nzbget_rs_history_coord_prepare_requeued_loaded_file(
					nzbInfo->GetDestDir(),
					PATH_SEPARATOR,
					recordedName.c_str(),
					fileInfo->GetFilename() ? fileInfo->GetFilename() : "",
					fileInfo->GetSuccessArticles(),
					(int)fileInfo->GetPartialState(),
					(int)FileInfo::psNone,
					(int)FileInfo::psCompleted,
					reprocess ? 1 : 0,
					resetFailed ? 1 : 0,
					artStatuses.data(),
					artStatuses.size(),
					(int)ArticleInfo::aiUndefined,
					(int)ArticleInfo::aiFailed,
					nullptr,
					fileSizeCb,
					deleteFileCb,
					fileExistsCb,
					&setRecordedFilename,
					&newPartialState,
					&warnMissingOnDisk,
					&noRetryableArticles);

				if (setRecordedFilename)
				{
					fileInfo->SetFilename(recordedName.c_str());
				}
				fileInfo->SetNzbInfo(nzbInfo);
				if (warnMissingOnDisk)
				{
					nzbInfo->PrintMessage(Message::mkWarning, "File %s could not be found on disk, downloading again", fileInfo->GetFilename());
				}
				fileInfo->SetPartialState((FileInfo::EPartialState)newPartialState);

				// A partial file may contain only entries missing from the NZB.
				// They have no message IDs to retry. Keep its completed record and
				// statistics intact instead of queueing an all-finished article list.
				if (noRetryableArticles)
				{
					++it;
					continue;
				}

				ResetArticles(fileInfo.get(), completedFile.GetStatus() == CompletedFile::cfFailure, resetFailed);

				g_DiskState->DiscardFile(fileInfo->GetId(), false, true, fileInfo->GetPartialState() != FileInfo::psCompleted);
				if (fileInfo->GetPartialState() == FileInfo::psCompleted)
				{
					g_DiskState->SaveFileState(fileInfo.get(), true);
				}
				fileInfo->GetArticles()->clear();

				requeued.insert(fileInfo->GetId());
				nzbInfo->GetFileList()->Add(std::move(fileInfo), false);

				it = nzbInfo->GetCompletedFiles()->erase(it);
				continue;
			}
		}
		++it;
	}

	// the repair jobs of files that download again are stale (a new one is built
	// when the file completes); the others keep their holes for post-processing:
	// a finished file that isn't downloaded again would otherwise never be repaired
	StreamRepairJobList* jobs = nzbInfo->GetStreamRepairJobs();
	jobs->erase(std::remove_if(jobs->begin(), jobs->end(),
		[&requeued](const StreamRepairJob& job) { return requeued.count(job.GetFileId()) > 0; }), jobs->end());

	nzbInfo->UpdateCurrentStats();
	if (!resetFailed && !reprocess)
	{
		// The user explicitly requested the remaining files; do not park the
		// download again because of the health failure being overridden.
		nzbInfo->SetHealthPaused(true);
	}

	// With no download work left there will be no article-completion event to
	// start post-processing. Paused spare PAR files do not block that transition.
	MoveToQueue(downloadQueue, itHistory, historyInfo, reprocess || nzbInfo->IsDownloadCompleted(true));

	if (g_Options->GetParCheck() != Options::pcForce)
	{
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
	}
}

inline void HistoryCoordinator::ResetArticles(FileInfo* fileInfo, bool allFailed, bool resetFailed)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	ArticleList* articles = fileInfo->GetArticles();
	std::vector<int> artStatuses;
	std::vector<int> artSizes;
	std::vector<uint8_t> resetFlags(articles->size(), 0);
	artStatuses.reserve(articles->size());
	artSizes.reserve(articles->size());
	for (ArticleInfo* pa : articles)
	{
		artStatuses.push_back((int)pa->GetStatus());
		artSizes.push_back(pa->GetSize());
	}

	int64 fileFailedSize = fileInfo->GetFailedSize();
	int fileFailedArticles = fileInfo->GetFailedArticles();
	int64 fileRemainingSize = fileInfo->GetRemainingSize();
	int fileCompletedArticles = fileInfo->GetCompletedArticles();
	int64 fileSuccessSize = fileInfo->GetSuccessSize();
	int fileSuccessArticles = fileInfo->GetSuccessArticles();
	int64 nzbFailedSize = nzbInfo->GetFailedSize();
	int64 nzbSuccessSize = nzbInfo->GetSuccessSize();
	int nzbFailedArticles = nzbInfo->GetFailedArticles();
	int nzbSuccessArticles = nzbInfo->GetSuccessArticles();
	int64 nzbParFailedSize = nzbInfo->GetParFailedSize();
	int64 nzbParSuccessSize = nzbInfo->GetParSuccessSize();

	nzbget_rs_history_coord_reset_articles_batch(
		allFailed ? 1 : 0,
		resetFailed ? 1 : 0,
		fileInfo->GetParFile() ? 1 : 0,
		(int)fileInfo->GetPartialState(),
		fileInfo->GetSize(),
		fileInfo->GetMissedSize(),
		fileInfo->GetTotalArticles(),
		fileInfo->GetMissedArticles(),
		&fileFailedSize,
		&fileFailedArticles,
		&fileRemainingSize,
		&fileCompletedArticles,
		&fileSuccessSize,
		&fileSuccessArticles,
		&nzbFailedSize,
		&nzbSuccessSize,
		&nzbFailedArticles,
		&nzbSuccessArticles,
		&nzbParFailedSize,
		&nzbParSuccessSize,
		artStatuses.data(),
		artSizes.data(),
		resetFlags.data(),
		articles->size());

	nzbInfo->GetServerStats()->ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
	nzbInfo->SetFailedSize(nzbFailedSize);
	nzbInfo->SetSuccessSize(nzbSuccessSize);
	nzbInfo->SetFailedArticles(nzbFailedArticles);
	nzbInfo->SetSuccessArticles(nzbSuccessArticles);
	if (fileInfo->GetParFile())
	{
		nzbInfo->SetParFailedSize(nzbParFailedSize);
		nzbInfo->SetParSuccessSize(nzbParSuccessSize);
	}
	fileInfo->SetFailedSize(fileFailedSize);
	fileInfo->SetFailedArticles(fileFailedArticles);
	fileInfo->SetRemainingSize(fileRemainingSize);
	fileInfo->SetCompletedArticles(fileCompletedArticles);
	fileInfo->SetSuccessSize(fileSuccessSize);
	fileInfo->SetSuccessArticles(fileSuccessArticles);

	for (size_t i = 0; i < articles->size(); ++i)
	{
		if (resetFlags[i])
		{
			ArticleInfo* pa = (*articles)[i].get();
			pa->SetStatus((ArticleInfo::EStatus)artStatuses[i]);
			pa->SetCrc(0);
			pa->SetSegmentOffset(0);
			pa->SetSegmentSize(0);
			pa->SetDupeExpectedOffset(-1);
			pa->SetDupeExpectedEnd(-1);
		}
	}
}

inline bool HistoryCoordinator::HistorySetParameter(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting post-process-parameter '%s' for '%s'", text, historyInfo->GetName());

	if (historyInfo->GetKind() != HistoryInfo::hkNzb && historyInfo->GetKind() != HistoryInfo::hkUrl)
	{
		error("Could not set post-process-parameter for %s: history item has wrong type", historyInfo->GetName());
		return false;
	}

	size_t nameLen = 0;
	const char* val = nzbget_rs_history_coord_split_param_assignment(text, &nameLen);
	if (val)
	{
		std::string name(text, nameLen);
		historyInfo->GetNzbInfo()->GetParameters()->SetParameter(name.c_str(), val);
	}
	else
	{
		error("Could not set post-process-parameter for %s: invalid argument: %s", historyInfo->GetNzbInfo()->GetName(), text);
	}

	return true;
}

inline bool HistoryCoordinator::HistorySetCategory(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting category '%s' for '%s'", text, historyInfo->GetName());

	if (historyInfo->GetKind() != HistoryInfo::hkNzb && historyInfo->GetKind() != HistoryInfo::hkUrl)
	{
		error("Could not set category for %s: history item has wrong type", historyInfo->GetName());
		return false;
	}

	historyInfo->GetNzbInfo()->SetCategory(text);

	return true;
}

inline bool HistoryCoordinator::HistorySetName(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting name '%s' for '%s'", text, historyInfo->GetName());

	if (Util::EmptyStr(text))
	{
		error("Could not rename %s. The new name cannot be empty", historyInfo->GetName());
		return false;
	}

	if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		std::string previous = historyInfo->GetNzbInfo()->GetName();
		historyInfo->GetNzbInfo()->SetName(text);
		if (historyInfo->GetKind() == HistoryInfo::hkNzb && g_DupeSearch && previous != historyInfo->GetNzbInfo()->GetName())
            g_DupeSearch->NotifyNameChanged(historyInfo->GetNzbInfo()->GetId());
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		historyInfo->GetDupInfo()->SetName(text);
	}

	return true;
}

inline void HistoryCoordinator::HistorySetDupeParam(HistoryInfo* historyInfo, DownloadQueue::EEditAction action, const char* text)
{
	debug("Setting dupe-parameter '%i'='%s' for '%s'", (int)action, text, historyInfo->GetName());

	EDupeMode mode = dmScore;
	if (action == DownloadQueue::eaHistorySetDupeMode)
	{
		int parsedMode = 0;
		if (!nzbget_rs_qhp_parse_dupe_mode(text, &parsedMode))
		{
			error("Could not set duplicate mode for %s: incorrect mode (%s)", historyInfo->GetName(), text);
			return;
		}
		mode = (EDupeMode)parsedMode;
	}

	if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		switch (action)
		{
			case DownloadQueue::eaHistorySetDupeKey:
                historyInfo->GetNzbInfo()->GetParameters()->SetParameter(DupeSearch::OwnedKeyParam, nullptr);
				historyInfo->GetNzbInfo()->SetDupeKey(text);
				break;

			case DownloadQueue::eaHistorySetDupeScore:
				historyInfo->GetNzbInfo()->SetDupeScore(text ? atoi(text) : 0);
				break;

			case DownloadQueue::eaHistorySetDupeMode:
				historyInfo->GetNzbInfo()->SetDupeMode(mode);
				break;

			case DownloadQueue::eaHistorySetDupeBackup:
				if (historyInfo->GetKind() == HistoryInfo::hkUrl)
				{
					error("Could not set duplicate parameter for %s: history item has wrong type", historyInfo->GetName());
					return;
				}
				else if (historyInfo->GetNzbInfo()->GetDeleteStatus() != NzbInfo::dsDupe &&
					historyInfo->GetNzbInfo()->GetDeleteStatus() != NzbInfo::dsManual)
				{
					error("Could not set duplicate parameter for %s: history item has wrong delete status", historyInfo->GetName());
					return;
				}
				historyInfo->GetNzbInfo()->SetDeleteStatus(
					(NzbInfo::EDeleteStatus)nzbget_rs_history_coord_parse_dupe_backup_flag(
						text, (int)NzbInfo::dsDupe, (int)NzbInfo::dsManual));
				break;

			default:
				// suppress compiler warning
				break;
		}
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		switch (action)
		{
			case DownloadQueue::eaHistorySetDupeKey:
				historyInfo->GetDupInfo()->SetDupeKey(text);
				break;

			case DownloadQueue::eaHistorySetDupeScore:
				historyInfo->GetDupInfo()->SetDupeScore(text ? atoi(text) : 0);
				break;

			case DownloadQueue::eaHistorySetDupeMode:
				historyInfo->GetDupInfo()->SetDupeMode(mode);
				break;

			case DownloadQueue::eaHistorySetDupeBackup:
				error("Could not set duplicate parameter for %s: history item has wrong type", historyInfo->GetName());
				return;

			default:
				// suppress compiler warning
				break;
		}
	}
}

inline void HistoryCoordinator::Redownload(DownloadQueue* downloadQueue, HistoryInfo* historyInfo)
{
	HistoryList::iterator it = downloadQueue->GetHistory()->Find(historyInfo);
	HistoryRedownload(downloadQueue, it, historyInfo, true);
}

#
inline bool DupeCoordinator::SameNameOrKey(const char* name1, const char* dupeKey1,
	const char* name2, const char* dupeKey2)
{
	return nzbget_rs_dupecoord_same_name_or_key(name1, dupeKey1, name2, dupeKey2) != 0;
}

inline bool DupeCoordinator::FilesOnDisk(NzbInfo* nzbInfo)
{
	return nzbget_rs_dupecoord_files_on_disk(
		nzbInfo->GetFinalDir(),
		nzbInfo->GetDestDir(),
		g_Options->GetExtCleanupDisk()) != 0;
}

inline NzbInfo* DupeCoordinator::DownloadedOnDisk(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	const int healthCheck = (int)g_Options->GetHealthCheck();
	const char* targetDupeKey = nzbInfo->GetDupeKey();
	const char* extCleanupDisk = g_Options->GetExtCleanupDisk();
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() != HistoryInfo::hkNzb)
		{
			continue;
		}
		NzbInfo* item = historyInfo->GetNzbInfo();
		if (nzbget_rs_dupecoord_is_downloaded_on_disk_candidate(
			healthCheck,
			targetDupeKey,
			item == nzbInfo ? 1 : 0,
			(int)item->GetDeleteStatus(),
			item->IsDupeSuccess() ? 1 : 0,
			(int)item->GetMarkStatus(),
			item->GetDupeKey(),
			item->GetFinalDir(),
			item->GetDestDir(),
			extCleanupDisk) != 0)
		{
			return item;
		}
	}
	return nullptr;
}

inline void DupeCoordinator::NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Checking duplicates for %s", nzbInfo->GetName());

	// the release is already downloaded and on disk: another posting of it isn't
	// needed (B70: Knife Edge S02E01 - asked again after its success, a different
	// posting of the same release, scored higher, downloaded a second copy). Kept
	// in history as a backup, in case the download on disk is deleted or marked bad.
	if (nzbInfo->GetKind() == NzbInfo::nkNzb && nzbInfo->GetDupeMode() != dmForce)
	{
		if (NzbInfo* downloaded = DownloadedOnDisk(downloadQueue, nzbInfo))
		{
			nzbInfo->PrintMessage(Message::mkInfo, "Not downloading %s: %s is downloaded and on disk",
				nzbInfo->GetName(), downloaded->GetName());
			nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
			return;
		}
	}

	// find duplicates in download queue with exactly same content
	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		bool sameContent = nzbget_rs_dupecoord_same_content_hash(
			nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
			queuedNzbInfo->GetFullContentHash(), queuedNzbInfo->GetFilteredContentHash()) != 0;

		// if there is a duplicate with exactly same content (via hash-check)
		// in queue - the new item is skipped
		if (queuedNzbInfo != nzbInfo && sameContent && nzbInfo->GetKind() == NzbInfo::nkNzb)
		{
			NzbgetRsBuf msgBuf = nzbget_rs_dupecoord_format_dupe_skip_message(
				nzbInfo->GetName(), queuedNzbInfo->GetName(), 0, 1, 0);
			std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
			nzbget_rs_free(msgBuf);

			if (nzbInfo->GetFeedId())
			{
				warn("%s", message.c_str());
				// Flag saying QueueCoordinator to skip nzb-file
				nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
				g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
			}
			else
			{
				nzbInfo->SetDeleteStatus(NzbInfo::dsCopy);
				nzbInfo->AddMessage(Message::mkWarning, message.c_str());
			}

			return;
		}
	}

	// if download has empty dupekey and empty dupescore - check if download queue
	// or history have an item with the same name and non empty dupekey or dupescore and
	// take these properties from this item
	if (Util::EmptyStr(nzbInfo->GetDupeKey()) && nzbInfo->GetDupeScore() == 0)
	{
		for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dupecoord_should_inherit_dupe_props(
				nzbInfo->GetName(), queuedNzbInfo->GetName(),
				queuedNzbInfo->GetDupeKey(), queuedNzbInfo->GetDupeScore()) != 0)
			{
				nzbInfo->SetDupeKey(queuedNzbInfo->GetDupeKey());
				nzbInfo->SetDupeScore(queuedNzbInfo->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing queue item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
		}
	}
	if (Util::EmptyStr(nzbInfo->GetDupeKey()) && nzbInfo->GetDupeScore() == 0)
	{
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
				nzbget_rs_dupecoord_should_inherit_dupe_props(
					nzbInfo->GetName(), historyInfo->GetNzbInfo()->GetName(),
					historyInfo->GetNzbInfo()->GetDupeKey(), historyInfo->GetNzbInfo()->GetDupeScore()) != 0)
			{
				nzbInfo->SetDupeKey(historyInfo->GetNzbInfo()->GetDupeKey());
				nzbInfo->SetDupeScore(historyInfo->GetNzbInfo()->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing history item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
			if (historyInfo->GetKind() == HistoryInfo::hkDup &&
				nzbget_rs_dupecoord_should_inherit_dupe_props(
					nzbInfo->GetName(), historyInfo->GetDupInfo()->GetName(),
					historyInfo->GetDupInfo()->GetDupeKey(), historyInfo->GetDupInfo()->GetDupeScore()) != 0)
			{
				nzbInfo->SetDupeKey(historyInfo->GetDupInfo()->GetDupeKey());
				nzbInfo->SetDupeScore(historyInfo->GetDupInfo()->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing history item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
		}
	}

	// find duplicates in history

	bool skip = false;
	bool good = false;
	bool sameContent = false;
	const char* dupeName = nullptr;

	// find duplicates in history having exactly same content
	// also: nzb-files having duplicates marked as good are skipped
	// also (only in score mode): nzb-files having success-duplicates in dup-history but not having duplicates in recent history are skipped
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		int outSameContent = 0;
		int outGood = 0;
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			if (nzbget_rs_dupecoord_eval_history_dupe_match(
				(int)HistoryInfo::hkNzb,
				nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
				(int)nzbInfo->GetDupeMode(), nzbInfo->GetDupeScore(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey(),
				item->GetFullContentHash(), item->GetFilteredContentHash(),
				(int)item->GetDupeMode(), (int)item->GetMarkStatus(), 0, item->GetDupeScore(),
				item->GetName(), item->GetDupeKey(),
				&outSameContent, &outGood) != 0)
			{
				skip = true;
				sameContent = outSameContent != 0;
				good = outGood != 0;
				dupeName = item->GetName();
				break;
			}
		}
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			DupInfo* dup = historyInfo->GetDupInfo();
			if (nzbget_rs_dupecoord_eval_history_dupe_match(
				(int)HistoryInfo::hkDup,
				nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
				(int)nzbInfo->GetDupeMode(), nzbInfo->GetDupeScore(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey(),
				dup->GetFullContentHash(), dup->GetFilteredContentHash(),
				(int)dup->GetDupeMode(), 0, (int)dup->GetStatus(), dup->GetDupeScore(),
				dup->GetName(), dup->GetDupeKey(),
				&outSameContent, &outGood) != 0)
			{
				skip = true;
				sameContent = outSameContent != 0;
				good = outGood != 0;
				dupeName = dup->GetName();
				break;
			}
		}
	}

	// With HealthCheck=dupe (B43) the same content in history decides by what became
	// of it: only untried backups (and items the user deleted) - the nzb-file is
	// downloaded as sent, not skipped as a copy (a client re-sending it would get
	// nothing); a success - skipped, as before; a failure - skipped too (the same dead
	// posting), but the best viable backup is fetched in its place when nothing of the
	// key is queued, so the re-send still leads to a download
	bool returnBackup = false;
	if (skip && sameContent &&
		g_Options->GetHealthCheck() == Options::hcDupe &&
		nzbInfo->GetKind() == NzbInfo::nkNzb &&
		nzbInfo->GetDupeHint() == NzbInfo::dhNone)
	{
		int succeeded = 0;
		int failed = 0;
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				NzbInfo* item = historyInfo->GetNzbInfo();
				nzbget_rs_dupecoord_accum_hc_dupe_same_content_outcome(
					(int)HistoryInfo::hkNzb,
					nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
					item->GetFullContentHash(), item->GetFilteredContentHash(),
					(int)item->GetDeleteStatus(), item->IsDupeSuccess() ? 1 : 0, 0,
					&succeeded, &failed);
			}
			else if (historyInfo->GetKind() == HistoryInfo::hkDup)
			{
				DupInfo* dup = historyInfo->GetDupInfo();
				nzbget_rs_dupecoord_accum_hc_dupe_same_content_outcome(
					(int)HistoryInfo::hkDup,
					nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
					dup->GetFullContentHash(), dup->GetFilteredContentHash(),
					0, 0, (int)dup->GetStatus(),
					&succeeded, &failed);
			}
		}
		if (!succeeded && !failed)
		{
			info("Adding %s: the same content waits in history only as an untried backup", nzbInfo->GetName());
			skip = false;
			sameContent = false;
		}
		else if (failed && !succeeded)
		{
			returnBackup = !std::any_of(downloadQueue->GetQueue()->begin(), downloadQueue->GetQueue()->end(),
				[nzbInfo](std::unique_ptr<NzbInfo>& queued)
				{
					return queued.get() != nzbInfo &&
						queued->GetDeleteStatus() == NzbInfo::dsNone &&
						SameNameOrKey(queued->GetName(), queued->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey());
				});
		}
	}

	if (!sameContent && nzbInfo->GetDupeHint() != NzbInfo::dhNone)
	{
		// dupe check when "download again" URLs: checking same content only
		return;
	}

	if (!sameContent && !good && nzbInfo->GetDupeMode() == dmScore)
	{
		// nzb-files having success-duplicates in recent history (with different content) are added to history for backup
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if ((historyInfo->GetKind() == HistoryInfo::hkNzb ||
				 historyInfo->GetKind() == HistoryInfo::hkUrl) &&
				nzbget_rs_dupecoord_should_skip_recent_history_score_dupe(
					sameContent ? 1 : 0, good ? 1 : 0,
					(int)nzbInfo->GetDupeMode(), nzbInfo->GetDupeScore(),
					nzbInfo->GetName(), nzbInfo->GetDupeKey(),
					(int)historyInfo->GetKind(),
					(int)historyInfo->GetNzbInfo()->GetDupeMode(),
					historyInfo->GetNzbInfo()->GetDupeScore(),
					historyInfo->GetNzbInfo()->IsDupeSuccess() ? 1 : 0,
					historyInfo->GetNzbInfo()->GetName(),
					historyInfo->GetNzbInfo()->GetDupeKey()) != 0)
			{
				// Flag saying QueueCoordinator to skip nzb-file
				nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
				info("Collection %s is a duplicate to %s", nzbInfo->GetName(), historyInfo->GetNzbInfo()->GetName());
				return;
			}
		}
	}

	if (skip)
	{
		NzbgetRsBuf msgBuf = nzbget_rs_dupecoord_format_dupe_skip_message(
			nzbInfo->GetName(), dupeName, 1, sameContent ? 1 : 0, good ? 1 : 0);
		std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
		nzbget_rs_free(msgBuf);

		if (nzbInfo->GetFeedId() && nzbInfo->GetDupeHint() == NzbInfo::dhNone)
		{
			nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
			warn("%s", message.c_str());
			g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
		}
		else
		{
			nzbInfo->SetDeleteStatus(sameContent ? NzbInfo::dsCopy : NzbInfo::dsGood);
			nzbInfo->AddMessage(Message::mkWarning, message.c_str());
		}

		if (returnBackup)
		{
			info("%s is the same posting as a failed download: fetching the best backup instead", nzbInfo->GetName());
			ReturnBestDupe(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey());
		}

		return;
	}

	// find duplicates in download queue and post-queue and handle both items according to their scores:
	// only one item remains in queue and another one is moved to history as dupe-backup
	if (nzbInfo->GetDupeMode() == dmScore)
	{
		// find duplicates in download queue
		int index = 0;
		for (NzbList::iterator it = downloadQueue->GetQueue()->begin(); it != downloadQueue->GetQueue()->end(); index++)
		{
			NzbInfo* queuedNzbInfo = (*it++).get();
			int action = nzbget_rs_dupecoord_eval_queue_score_dupe(
				queuedNzbInfo == nzbInfo ? 1 : 0,
				(int)queuedNzbInfo->GetDeleteStatus(),
				queuedNzbInfo->GetKind() == NzbInfo::nkNzb ? 1 : 0,
				(queuedNzbInfo->GetKind() == NzbInfo::nkUrl && nzbInfo->GetKind() == NzbInfo::nkUrl) ? 1 : 0,
				(int)queuedNzbInfo->GetDupeMode(),
				queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey(),
				nzbInfo->GetDupeScore(), queuedNzbInfo->GetDupeScore(),
				(int)g_Options->GetHealthCheck(),
				queuedNzbInfo->GetDupeHint() == NzbInfo::dhRedownloadAuto ? 1 : 0,
				queuedNzbInfo->GetCurrentSuccessArticles(),
				queuedNzbInfo->CalcHealth(),
				queuedNzbInfo->CalcCriticalHealth(true),
				queuedNzbInfo->GetPostInfo() != nullptr ? 1 : 0);
			if (action == 1)
			{
				// Flag saying QueueCoordinator to skip nzb-file
				nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
				info("Collection %s is a duplicate to %s", nzbInfo->GetName(), queuedNzbInfo->GetName());
				return;
			}
			else if (action == 2)
			{
				// the existing queue item is moved to history as dupe-backup
				info("Moving collection %s with lower duplicate score to history", queuedNzbInfo->GetName());
				queuedNzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
				int oldSize = downloadQueue->GetQueue()->size();
				downloadQueue->EditEntry(queuedNzbInfo->GetId(),
					DownloadQueue::eaGroupDelete, nullptr);
				int newSize = downloadQueue->GetQueue()->size();
				if (oldSize == newSize)
				{
					index++;
				}
				it = downloadQueue->GetQueue()->begin() + index;
				index--;
			}
		}
	}
}

inline void DupeCoordinator::NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Processing duplicates for %s", nzbInfo->GetName());

	if (FailsOver(nzbInfo) && !nzbInfo->IsDupeSuccess())
	{
		ReturnBestDupe(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey());
	}

	// one copy is downloaded and on disk: the others of its key still downloading,
	// repairing or checking are stopped and kept as backups (B77: Sugar S01E01 - a
	// copy kept repairing for minutes after another one had succeeded, then failed)
	bool dupeHealth = g_Options->GetHealthCheck() == Options::hcDupe ||
		g_Options->GetHealthCheck() == Options::hcPark;
	if (dupeHealth && !Util::EmptyStr(nzbInfo->GetDupeKey()) &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		nzbInfo->IsDupeSuccess() && FilesOnDisk(nzbInfo))
	{
		DownloadQueue::EEditAction deleteAction = g_Options->GetHealthCheck() == Options::hcPark
			? DownloadQueue::eaGroupParkDelete
			: DownloadQueue::eaGroupDupeDelete;
		std::vector<std::pair<int, bool>> others;	// id, in post-processing
		for (NzbInfo* queued : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dupecoord_is_other_to_stop_on_completed(
				queued == nzbInfo ? 1 : 0,
				queued->GetKind() == NzbInfo::nkNzb ? 1 : 0,
				queued->GetDeleting() ? 1 : 0,
				(int)queued->GetDeleteStatus(),
				(int)queued->GetDupeMode(),
				queued->GetDupeKey(),
				nzbInfo->GetDupeKey()) != 0)
			{
				queued->PrintMessage(Message::mkInfo, "Stopping %s: %s is downloaded and on disk",
					queued->GetName(), nzbInfo->GetName());
				queued->SetDeleteStatus(NzbInfo::dsDupe);
				others.emplace_back(queued->GetId(), queued->GetPostInfo() != nullptr);
			}
		}
		for (const auto& other : others)
		{
			downloadQueue->EditEntry(other.first,
				other.second ? DownloadQueue::eaPostDelete : deleteAction,
				nullptr);
		}
	}
}

inline std::string DupeCoordinator::NoBackupReason(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	std::vector<HistoryInfo*> historyItems;
	std::vector<NzbgetRsDupeHistoryCandidate> historyCandidates;
	historyItems.reserve(downloadQueue->GetHistory()->size());
	historyCandidates.reserve(downloadQueue->GetHistory()->size());
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		historyItems.push_back(historyInfo);
		NzbgetRsDupeHistoryCandidate c{};
		c.kind = (int)historyInfo->GetKind();
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			c.dupe_mode = (int)item->GetDupeMode();
			c.mark_status = (int)item->GetMarkStatus();
			c.delete_status = (int)item->GetDeleteStatus();
			c.alive_permille = AlivePermille(item);
			c.health = item->CalcHealth();
			c.critical_health = item->CalcCriticalHealth(true);
			c.same_name_or_key = (item != nzbInfo &&
				SameNameOrKey(item->GetName(), item->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey())) ? 1 : 0;
		}
		historyCandidates.push_back(c);
	}

	auto fileExistsCb = [](void* ctx, size_t idx) -> int {
		auto* items = static_cast<std::vector<HistoryInfo*>*>(ctx);
		return FileSystem::FileExists((*items)[idx]->GetNzbInfo()->GetQueuedFilename()) ? 1 : 0;
	};

	NzbgetRsBuf buf = nzbget_rs_dupecoord_no_backup_reason(
		FailsOver(nzbInfo) ? 1 : 0,
		(int)nzbInfo->GetDupeMode(),
		historyCandidates.data(),
		historyCandidates.size(),
		&historyItems,
		fileExistsCb);
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

inline bool DupeCoordinator::FailsOver(NzbInfo* nzbInfo)
{
	return nzbget_rs_dupecoord_fails_over(
		g_Options->GetDupeCheck() ? 1 : 0, (int)nzbInfo->GetDupeMode(), (int)g_Options->GetHealthCheck()) != 0;
}

inline void DupeCoordinator::ReturnBestDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey)
{
	HistoryInfo* historyDupe = FindDupeBackup(downloadQueue, nzbInfo, nzbName, dupeKey);

	// move that dupe-backup from history to download queue
	if (historyDupe)
	{
		info("Found duplicate %s for %s", historyDupe->GetNzbInfo()->GetName(), nzbName);
		historyDupe->GetNzbInfo()->SetDupeHint(NzbInfo::dhRedownloadAuto);
		g_HistoryCoordinator->Redownload(downloadQueue, historyDupe);
	}
}

inline HistoryInfo* DupeCoordinator::FindDupeBackup(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey)
{
	std::vector<HistoryInfo*> historyItems;
	std::vector<NzbgetRsDupeHistoryCandidate> historyCandidates;
	historyItems.reserve(downloadQueue->GetHistory()->size());
	historyCandidates.reserve(downloadQueue->GetHistory()->size());
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		historyItems.push_back(historyInfo);
		NzbgetRsDupeHistoryCandidate c{};
		c.kind = (int)historyInfo->GetKind();
		if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			c.dupe_mode = (int)item->GetDupeMode();
			c.dupe_score = item->GetDupeScore();
			c.is_dupe_success = item->IsDupeSuccess() ? 1 : 0;
			c.mark_status = (int)item->GetMarkStatus();
			c.delete_status = (int)item->GetDeleteStatus();
			c.full_content_hash = item->GetFullContentHash();
			c.alive_permille = AlivePermille(item);
			c.health = item->CalcHealth();
			c.critical_health = item->CalcCriticalHealth(true);
			c.same_name_or_key = SameNameOrKey(item->GetName(), item->GetDupeKey(), nzbName, dupeKey) ? 1 : 0;
		}
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			DupInfo* dup = historyInfo->GetDupInfo();
			c.dupe_mode = (int)dup->GetDupeMode();
			c.dupe_score = dup->GetDupeScore();
			c.dup_status = (int)dup->GetStatus();
			c.same_name_or_key = SameNameOrKey(dup->GetName(), dup->GetDupeKey(), nzbName, dupeKey) ? 1 : 0;
		}
		historyCandidates.push_back(c);
	}

	std::vector<NzbgetRsDupeQueueCandidate> queueCandidates;
	queueCandidates.reserve(downloadQueue->GetQueue()->size());
	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		NzbgetRsDupeQueueCandidate q{};
		q.is_self = (queuedNzbInfo == nzbInfo) ? 1 : 0;
		q.is_nzb_kind = (queuedNzbInfo->GetKind() == NzbInfo::nkNzb) ? 1 : 0;
		q.dupe_mode = (int)queuedNzbInfo->GetDupeMode();
		q.dupe_score = queuedNzbInfo->GetDupeScore();
		q.same_name_or_key = SameNameOrKey(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(), nzbName, dupeKey) ? 1 : 0;
		queueCandidates.push_back(q);
	}

	auto fileExistsCb = [](void* ctx, size_t idx) -> int {
		auto* items = static_cast<std::vector<HistoryInfo*>*>(ctx);
		return FileSystem::FileExists((*items)[idx]->GetNzbInfo()->GetQueuedFilename()) ? 1 : 0;
	};

	int winner = nzbget_rs_dupecoord_select_dupe_backup(
		nzbInfo != nullptr ? 1 : 0,
		nzbInfo ? nzbInfo->GetFullContentHash() : 0,
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0,
		historyCandidates.data(),
		historyCandidates.size(),
		queueCandidates.data(),
		queueCandidates.size(),
		&historyItems,
		fileExistsCb);

	return winner >= 0 && (size_t)winner < historyItems.size() ? historyItems[(size_t)winner] : nullptr;
}

inline bool DupeCoordinator::DupeFailoverWarranted(int itemScore, int health, int backupScore)
{
	return nzbget_rs_dupecoord_failover_warranted(itemScore, health, backupScore) != 0;
}

inline void DupeCoordinator::HistoryMark(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, NzbInfo::EMarkStatus markStatus)
{
	int itemDupeMode = historyInfo->GetKind() == HistoryInfo::hkNzb ? (int)historyInfo->GetNzbInfo()->GetDupeMode() :
		historyInfo->GetKind() == HistoryInfo::hkDup ? (int)historyInfo->GetDupInfo()->GetDupeMode() : 0;
	int dupStatus = 0;
	int postAction = 0;
	const char* markStatusName = nzbget_rs_dupecoord_resolve_history_mark(
		(int)markStatus,
		g_Options->GetDupeCheck() ? 1 : 0,
		(int)historyInfo->GetKind(),
		itemDupeMode,
		&dupStatus,
		&postAction);
	info("Marking %s as %s", historyInfo->GetName(), markStatusName);

	if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		historyInfo->GetNzbInfo()->SetMarkStatus(markStatus);
		g_QueueScriptCoordinator->EnqueueScript(historyInfo->GetNzbInfo(), QueueScriptCoordinator::qeNzbMarked);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		historyInfo->GetDupInfo()->SetStatus((DupInfo::EStatus)dupStatus);
	}
	else
	{
		error("Could not mark %s as bad: history item has wrong type", historyInfo->GetName());
		return;
	}

	if (postAction == 1)
	{
		HistoryCleanup(downloadQueue, historyInfo);
	}
	else if (postAction == 2)
	{
		const char* dupeKey = historyInfo->GetKind() == HistoryInfo::hkNzb ? historyInfo->GetNzbInfo()->GetDupeKey() :
			historyInfo->GetKind() == HistoryInfo::hkDup ? historyInfo->GetDupInfo()->GetDupeKey() :
			nullptr;
		ReturnBestDupe(downloadQueue, nullptr, historyInfo->GetName(), dupeKey);
	}
}

inline void DupeCoordinator::HistoryCleanup(DownloadQueue* downloadQueue, HistoryInfo* markHistoryInfo)
{
	const char* dupeKey = markHistoryInfo->GetKind() == HistoryInfo::hkNzb ? markHistoryInfo->GetNzbInfo()->GetDupeKey() :
		markHistoryInfo->GetKind() == HistoryInfo::hkDup ? markHistoryInfo->GetDupInfo()->GetDupeKey() :
		nullptr;
	const char* nzbName = markHistoryInfo->GetKind() == HistoryInfo::hkNzb ? markHistoryInfo->GetNzbInfo()->GetName() :
		markHistoryInfo->GetKind() == HistoryInfo::hkDup ? markHistoryInfo->GetDupInfo()->GetName() :
		nullptr;
	bool changed = false;
	int index = 0;

	// traversing in a reverse order to delete items in order they were added to history
	// (just to produce the log-messages in a more logical order)
	for (HistoryList::reverse_iterator it = downloadQueue->GetHistory()->rbegin(); it != downloadQueue->GetHistory()->rend(); )
	{
		HistoryInfo* historyInfo = (*it).get();
		NzbInfo* nzbInfo = (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
			? historyInfo->GetNzbInfo()
			: nullptr;

		if (nzbInfo && nzbget_rs_dupecoord_should_cleanup_history_dupe(
			(int)historyInfo->GetKind(),
			(int)nzbInfo->GetDupeMode(),
			(int)nzbInfo->GetDeleteStatus(),
			historyInfo == markHistoryInfo ? 1 : 0,
			nzbInfo->GetName(),
			nzbInfo->GetDupeKey(),
			nzbName,
			dupeKey) != 0)
		{
			g_HistoryCoordinator->HistoryHide(downloadQueue, historyInfo, index);
			index++;
			it = downloadQueue->GetHistory()->rbegin() + index;
			changed = true;
		}
		else
		{
			++it;
			index++;
		}
	}

	if (changed)
	{
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

inline DupeCoordinator::EDupeStatus DupeCoordinator::GetDupeStatus(DownloadQueue* downloadQueue,
	const char* name, const char* dupeKey)
{
	int statuses = (int)dsNone;

	// find duplicates in download queue
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		statuses = nzbget_rs_dupecoord_accumulate_queue_dupe_status(
			statuses,
			name,
			dupeKey,
			nzbInfo->GetName(),
			nzbInfo->GetDupeKey(),
			nzbInfo->GetSuccessArticles(),
			nzbInfo->GetFailedArticles());
	}

	// find duplicates in history
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			statuses = nzbget_rs_dupecoord_accumulate_history_dupe_status(
				statuses,
				(int)HistoryInfo::hkNzb,
				name,
				dupeKey,
				item->GetName(),
				item->GetDupeKey(),
				item->MakeTextStatus(true),
				0);
		}
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			DupInfo* dup = historyInfo->GetDupInfo();
			statuses = nzbget_rs_dupecoord_accumulate_history_dupe_status(
				statuses,
				(int)HistoryInfo::hkDup,
				name,
				dupeKey,
				dup->GetName(),
				dup->GetDupeKey(),
				nullptr,
				(int)dup->GetStatus());
		}
	}

	return (EDupeStatus)statuses;
}

inline RawNzbList DupeCoordinator::ListHistoryDupes(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	RawNzbList dupeList;

	// find duplicates in history
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		NzbInfo* item = historyInfo->GetKind() == HistoryInfo::hkNzb ? historyInfo->GetNzbInfo() : nullptr;
		if (item && nzbget_rs_dupecoord_should_list_history_dupe(
			(int)nzbInfo->GetDupeMode(),
			(int)historyInfo->GetKind(),
			(int)item->GetDupeMode(),
			item->GetName(),
			item->GetDupeKey(),
			nzbInfo->GetName(),
			nzbInfo->GetDupeKey()) != 0)
		{
			dupeList.push_back(item);
		}
	}

	return dupeList;
}

inline int DupeCoordinator::AlivePermille(NzbInfo* nzbInfo, const char* name)
{
	NzbParameter* parameter = nzbInfo->GetParameters()->Find(name);
	if (!parameter || !parameter->GetValue())
	{
		return -1;
	}
	return nzbget_rs_dupecoord_parse_alive_permille(parameter->GetValue());
}


#endif /* HISTORYCOORDINATOR_H */


inline ArticleDownloader::ArticleDownloader()
	: m_fileInfo(nullptr)
	, m_articleInfo(nullptr)
	, m_writingStarted(false)
{
	debug("Creating ArticleDownloader");

	SetLastUpdateTimeNow();
}

inline ArticleDownloader::~ArticleDownloader()
{
	debug("Destroying ArticleDownloader");

#ifndef DISABLE_TLS
	OpenSSL::StopSSLThread();
#endif
}

inline void ArticleDownloader::SetInfoName(const char* infoName)
{
	m_infoName = infoName;
	m_articleWriter.SetInfoName(m_infoName);
}

inline void ArticleDownloader::Run()
{
	debug("Entering ArticleDownloader-loop");

	SetStatus(adRunning);

	m_articleWriter.SetFileInfo(m_fileInfo);
	m_articleWriter.SetArticleInfo(m_articleInfo);
	m_articleWriter.Prepare();

	EStatus status = adFailed;
	int retries = g_Options->GetArticleRetries() > 0 ? g_Options->GetArticleRetries() : 1;
	int remainedRetries = retries;
	std::unique_ptr<NzbgetRsTransientRetries, decltype(&nzbget_rs_transient_retries_free)> transientRetries(
		nzbget_rs_transient_retries_new(retries, g_Options->GetArticleInterval()),
		nzbget_rs_transient_retries_free);
	ServerPool::RawServerList failedServers;
	failedServers.reserve(g_ServerPool->GetServers()->size());
	NewsServer* wantServer = nullptr;
	NewsServer* lastServer = nullptr;
	int level = 0;
	int serverConfigGeneration = g_ServerPool->GetGeneration();
	bool force = m_fileInfo->GetNzbInfo()->GetForcePriority();

	while (!IsStopped())
	{
		status = adFailed;

		SetStatus(adWaiting);
		while (!m_connection && !IsStopped() && serverConfigGeneration == g_ServerPool->GetGeneration())
		{
			if (nzbget_rs_transient_retries_is_empty(transientRetries.get()) != 0)
			{
				m_connection = g_ServerPool->GetConnection(level, wantServer, &failedServers);
			}
			else
			{
				ServerPool::RawServerList ignoreServers = failedServers;
				std::vector<uintptr_t> waitingPtrs(g_ServerPool->GetServers()->size());
				size_t waitingCount = nzbget_rs_transient_retries_waiting_servers(
					transientRetries.get(), (int64)Util::CurrentTime(),
					waitingPtrs.data(), waitingPtrs.size());
				bool wantServerWaiting = false;
				for (size_t wi = 0; wi < waitingCount; ++wi)
				{
					NewsServer* waitingServer = reinterpret_cast<NewsServer*>(waitingPtrs[wi]);
					ignoreServers.push_back(waitingServer);
					if (waitingServer == wantServer)
					{
						wantServerWaiting = true;
					}
				}
				if (!wantServerWaiting)
				{
					m_connection = g_ServerPool->GetConnection(level, wantServer, &ignoreServers);
				}
			}
			Util::Sleep(5);
		}
		SetLastUpdateTimeNow();
		SetStatus(adRunning);

		if (IsStopped() ||
			((g_WorkState->GetPauseDownload() || g_WorkState->GetQuotaReached()) && !force) ||
			(g_WorkState->GetDownloadHeld() && !m_fileInfo->GetExtraPriority()) ||
			serverConfigGeneration != g_ServerPool->GetGeneration())
		{
			status = adRetry;
			break;
		}

		lastServer = m_connection->GetNewsServer();
		level = lastServer->GetNormLevel();
		remainedRetries = nzbget_rs_transient_retries_remained_retries(
			transientRetries.get(), reinterpret_cast<uintptr_t>(lastServer));

		m_connection->SetSuppressErrors(false);

#ifndef DISABLE_TLS
		m_connection->SetCertVerifLevel(lastServer->GetCertVerificationLevel());
#endif

		{
			NzbgetRsBuf connNameBuf = nzbget_rs_dqh_article_dl_format_connection_name(
				m_connection->GetNewsServer()->GetName(), m_connection->GetHost());
			m_connectionName.Set(connNameBuf.data ? connNameBuf.data : "", (int)connNameBuf.len);
			nzbget_rs_free(connNameBuf);
		}

		// check server retention
		int fileAgeDays = 0;
		bool retentionFailure = nzbget_rs_nntp_article_dl_is_retention_failure(
			m_connection->GetNewsServer()->GetRetention(),
			(int64)m_fileInfo->GetTime(),
			(int64)Util::CurrentTime(),
			&fileAgeDays) != 0;
		if (retentionFailure)
		{
			detail("Article %s @ %s failed: out of server retention (file age: %i, configured retention: %i)",
				*m_infoName, *m_connectionName, fileAgeDays,
				m_connection->GetNewsServer()->GetRetention());
			status = adFailed;
			FreeConnection(true);
		}

		if (m_connection && !IsStopped())
		{
			detail("Downloading %s @ %s", *m_infoName, *m_connectionName);
		}

		// test connection
		bool connected = m_connection && m_connection->Connect();
		bool transientRetryScheduled = false;
		bool authRejected = false;
		if (!connected && m_connection && m_connection->GetAuthRejected())
		{
			if (nzbget_rs_transient_retries_register(
				transientRetries.get(), reinterpret_cast<uintptr_t>(lastServer), (int64)Util::CurrentTime()) != 0)
			{
				transientRetryScheduled = true;
			}
			else
			{
				authRejected = true;
			}
		}
		if (connected && !IsStopped())
		{
			NewsServer* newsServer = m_connection->GetNewsServer();

			// Download article
			status = Download();

			if (status == adTransientError)
			{
				if (nzbget_rs_transient_retries_register(
					transientRetries.get(), reinterpret_cast<uintptr_t>(newsServer), (int64)Util::CurrentTime()) != 0)
				{
					status = adConnectError;
					transientRetryScheduled = true;
				}
				else
				{
					status = adFailed;
					remainedRetries = 0;
				}
			}

			int success = 0, failed = 0;
			if (nzbget_rs_nntp_article_dl_classify_attempt_stat(
				(int)status,
				(int)adFinished,
				(int)adFailed,
				(int)adNotFound,
				(int)adCrcError,
				&success,
				&failed) != 0)
			{
				int serverId = newsServer->GetId();
				m_serverStats.StatOp(serverId, success, failed, ServerStatList::soSet);
				ServerVolume::Stats stats;
				stats.bytes = 0;
				stats.articles.failed = failed;
				stats.articles.success = success;
				g_StatMeter->AddServerStats(stats, serverId);
			}
		}

		if (m_connection)
		{
			AddServerStats();
		}

		{
			int shouldBreak = 0;
			int statusInt = (int)status;
			int crInt = m_contentRejected ? 1 : 0;
			if (nzbget_rs_nntp_article_dl_handle_content_rejected(
				crInt,
				m_articleInfo->GetDupeFallbackRound(),
				statusInt,
				(int)adFailed,
				remainedRetries,
				&shouldBreak,
				&statusInt,
				&crInt,
				&remainedRetries) != 0)
			{
				FreeConnection(false);
				status = (EStatus)statusInt;
				if (shouldBreak != 0)
				{
					break;
				}
				m_contentRejected = crInt != 0;
			}
		}

		if (!connected && m_connection)
		{
			detail("Article %s @ %s failed: could not establish connection", *m_infoName, *m_connectionName);
		}

		int connInt = connected ? 1 : 0;
		int statusInt = (int)status;
		nzbget_rs_nntp_article_dl_normalize_connect_and_decrement_retries(
			connInt, statusInt, (int)adConnectError, (int)adFailed, remainedRetries,
			&connInt, &statusInt, &remainedRetries);
		connected = connInt != 0;
		status = (EStatus)statusInt;
		nzbget_rs_transient_retries_set_remained_retries(
			transientRetries.get(), reinterpret_cast<uintptr_t>(lastServer), remainedRetries);

		bool optionalBlocked = false;
		if (!connected && m_connection && !IsStopped())
		{
			g_ServerPool->BlockServer(lastServer);
			optionalBlocked = lastServer->GetOptional();
		}

		wantServer = nullptr;
		if (connected && status == adFailed && remainedRetries > 0 && !retentionFailure)
		{
			wantServer = lastServer;
		}
		else
		{
			FreeConnection(status == adFinished || status == adNotFound);
			if (transientRetryScheduled && m_fileInfo->GetNzbInfo()->HasDesiredServer())
			{
				wantServer = lastServer;
			}
		}

		if (nzbget_rs_dqh_article_dl_is_terminal_loop_status(
			(int)status, (int)adFinished, (int)adFatalError) != 0)
		{
			break;
		}

		if (IsStopped() ||
			((g_WorkState->GetPauseDownload() || g_WorkState->GetQuotaReached()) && !force) ||
			(g_WorkState->GetDownloadHeld() && !m_fileInfo->GetExtraPriority()) ||
			serverConfigGeneration != g_ServerPool->GetGeneration())
		{
			status = adRetry;
			break;
		}

		if (nzbget_rs_dqh_article_dl_should_fail_for_desired_server(
			wantServer ? 1 : 0,
			m_fileInfo->GetNzbInfo()->HasDesiredServer() ? 1 : 0) != 0)
		{
			status = adFailed;
			break;
		}

		int pushFailedServer = 0;
		if (nzbget_rs_nntp_article_dl_should_advance_server_list(
			wantServer ? 1 : 0,
			connected ? 1 : 0,
			retentionFailure ? 1 : 0,
			optionalBlocked ? 1 : 0,
			authRejected ? 1 : 0,
			&pushFailedServer) != 0)
		{
			if (pushFailedServer)
			{
				failedServers.push_back(lastServer);
			}

			size_t candCount = g_ServerPool->GetServers()->size();
			std::vector<int32_t> candIds(candCount), candLevels(candCount), candGroups(candCount);
			std::vector<uint8_t> candAvail(candCount);
			size_t ci = 0;
			for (NewsServer* candidateServer : g_ServerPool->GetServers())
			{
				candIds[ci] = candidateServer->GetId();
				candLevels[ci] = candidateServer->GetNormLevel();
				candGroups[ci] = candidateServer->GetGroup();
				candAvail[ci] = (uint8_t)nzbget_rs_dqh_article_dl_is_server_available(
					candidateServer->GetActive() ? 1 : 0,
					candidateServer->GetMaxConnections(),
					candidateServer->GetOptional() ? 1 : 0,
					(candidateServer->GetOptional() && g_ServerPool->IsServerBlocked(candidateServer)) ? 1 : 0);
				++ci;
			}
			size_t failCount = failedServers.size();
			std::vector<int32_t> failIds(failCount), failLevels(failCount), failGroups(failCount);
			for (size_t fi = 0; fi < failCount; ++fi)
			{
				failIds[fi] = failedServers[fi]->GetId();
				failLevels[fi] = failedServers[fi]->GetNormLevel();
				failGroups[fi] = failedServers[fi]->GetGroup();
			}
			bool allServersOnLevelFailed = nzbget_rs_nntp_article_dl_all_servers_on_level_failed(
				level, candIds.data(), candLevels.data(), candGroups.data(), candAvail.data(), candCount,
				failIds.data(), failLevels.data(), failGroups.data(), failCount) != 0;

			int advanceLevel = 0;
			int newLevel = level;
			int breakAllFailed = 0;
			nzbget_rs_nntp_article_dl_eval_level_exhaustion(
				allServersOnLevelFailed ? 1 : 0, level, g_ServerPool->GetMaxNormLevel(),
				&advanceLevel, &newLevel, &breakAllFailed);
			if (advanceLevel != 0)
			{
				detail("Article %s @ all level %i servers failed, increasing level", *m_infoName, level);
				level = newLevel;
			}
			else if (breakAllFailed != 0)
			{
				detail("Article %s @ all servers failed", *m_infoName);
				status = adFailed;
				break;
			}

			remainedRetries = retries;
		}
	}

	FreeConnection(status == adFinished);

	bool stopped = IsStopped();
	status = (EStatus)nzbget_rs_nntp_article_dl_resolve_final_status(
		(int)status,
		m_articleWriter.GetDuplicate() ? 1 : 0,
		stopped ? 1 : 0,
		(int)adFinished, (int)adFailed, (int)adRetry, (int)adFatalError);
	if (stopped)
	{
		detail("Download %s cancelled", *m_infoName);
	}

	if (status == adFailed && nzbget_rs_transient_retries_is_exhausted(transientRetries.get()) != 0)
	{
		detail("Download %s failed: temporary server errors outlasted all attempts, the article may still exist", *m_infoName);
	}
	else if (status == adFailed)
	{
		detail("Download %s failed", *m_infoName);
	}

	SetStatus(status);
	Notify(nullptr);

	debug("Exiting ArticleDownloader-loop");
}

inline ArticleDownloader::EStatus ArticleDownloader::Download()
{
	const char* response = nullptr;
	EStatus status = adRunning;
	m_writingStarted = false;
	m_localWriteError = false;
	m_contentRejected = false;
	m_articleInfo->SetCrc(0);

	if (m_contentAnalyzer)
	{
		m_contentAnalyzer->Reset();
	}

	if (m_connection->GetNewsServer()->GetJoinGroup() && !m_fileInfo->GetGroups()->empty())
	{
		for (CString& group : m_fileInfo->GetGroups())
		{
			response = m_connection->JoinGroup(group);
			if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) != 0)
			{
				break;
			}
		}

		status = CheckResponse(response, "could not join group");
		if (status != adFinished)
		{
			return status;
		}
	}

	// retrieve article
	{
		NzbgetRsBuf cmdBuf = nzbget_rs_dqh_article_dl_format_fetch_command(
			g_Options->GetRawArticle() ? 1 : 0, m_articleInfo->GetMessageId());
		response = m_connection->Request(cmdBuf.data ? cmdBuf.data : "");
		nzbget_rs_free(cmdBuf);
	}

	status = CheckResponse(response, "could not fetch article");
	if (status != adFinished)
	{
		return status;
	}

	m_decoder.Clear();
	m_decoder.SetCrcCheck(g_Options->GetCrcCheck());
	m_decoder.SetRawMode(g_Options->GetRawArticle());

	status = adRunning;
	int readChunk = g_Options->GetArticleReadChunkSize();
	CharBuffer lineBuf(readChunk + 128);

	while (nzbget_rs_dqh_article_dl_should_continue_read_loop(
		IsStopped() ? 1 : 0, m_decoder.GetEof() ? 1 : 0) != 0)
	{
		if (!IsStopped() && !m_articleWriter.HasCachedBuffer() &&
			g_MemoryMonitor && g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL &&
			g_ArticleCache && g_ArticleCache->GetFlushing())
		{
			Util::Sleep(10);
		}

		// throttle the bandwidth
		while (!IsStopped() && (g_WorkState->GetSpeedLimit() > 0.0f) &&
			(g_StatMeter->CalcCurrentDownloadSpeed() > g_WorkState->GetSpeedLimit() ||
			 g_StatMeter->CalcMomentaryDownloadSpeed() > g_WorkState->GetSpeedLimit()))
		{
			SetLastUpdateTimeNow();
			Util::Sleep(10);
		}

		char* buffer;
		int len;
		m_connection->ReadBuffer(&buffer, &len);
		if (len == 0)
		{
			len = m_connection->TryRecv(lineBuf, readChunk);
			buffer = lineBuf;
		}

		// have we encountered a timeout?
		if (len <= 0)
		{
			if (!IsStopped())
			{
				detail("Article %s @ %s failed: Unexpected end of article", *m_infoName, *m_connectionName);
			}
			m_connection->SetBroken();
			m_connection->Disconnect();
			status = adFailed;
			break;
		}

		g_StatMeter->AddSpeedReading(len);
		time_t oldTime = m_lastUpdateTime;
		SetLastUpdateTimeNow();
		if (oldTime != m_lastUpdateTime)
		{
			AddServerStats();
		}

		// decode article data
		len = m_decoder.DecodeBuffer(buffer, len);

		// write to output file
		if (len > 0 && !Write(buffer, len))
		{
			int crInt = 0;
			status = (EStatus)nzbget_rs_nntp_article_dl_classify_write_failure(
				m_localWriteError ? 1 : 0, (int)adFatalError, (int)adFailed, &crInt);
			if (crInt != 0)
			{
				m_contentRejected = true;
			}
			break;
		}
	}

	if (IsStopped())
	{
		status = adFailed;
	}

	if (nzbget_rs_dqh_article_dl_should_run_decode_check(
		(int)status, (int)adRunning) != 0)
	{
		FreeConnection(true);
		status = DecodeCheck();
	}

	if (m_writingStarted)
	{
		m_articleWriter.Finish(status == adFinished);
	}

	if (status == adFinished)
	{
		detail("Successfully downloaded %s", *m_infoName);
	}

	return status;
}

inline ArticleDownloader::EStatus ArticleDownloader::CheckResponse(const char* response, const char* comment)
{
	EStatus status = (EStatus)nzbget_rs_article_downloader_check_response(
		response, m_connection->GetAuthError() ? 1 : 0);
	if (status != adFinished)
	{
		if (!response)
		{
			if (!IsStopped())
			{
				detail("Article %s @ %s failed, %s: Connection closed by remote host",
					*m_infoName, *m_connectionName, comment);
			}
		}
		else
		{
			detail("Article %s @ %s failed, %s: %s", *m_infoName, *m_connectionName, comment, response);
		}
	}
	return status;
}

inline bool ArticleDownloader::Write(char* buffer, int len)
{
	const char* articleFilename = nullptr;
	int64 articleFileSize = 0;
	int64 articleOffset = 0;
	int articleSize = 0;

	if (!m_writingStarted)
	{
		if (!g_Options->GetRawArticle())
		{
			articleFilename = m_decoder.GetArticleFilename();
			if (m_decoder.GetFormat() == Decoder::efYenc)
			{
				int64 beginPos = m_decoder.GetBeginPos();
				int64 endPos = m_decoder.GetEndPos();
				articleFileSize = m_decoder.GetSize();
				int64 expectedFileSize = m_fileInfo->GetDecodedFileSize();
				int64 dupeExpectedOffset = m_articleInfo->GetDupeExpectedOffset();
				int64 dupeExpectedEnd = m_articleInfo->GetDupeExpectedEnd();
				int rc = nzbget_rs_article_downloader_validate_yenc(
					beginPos, endPos, articleFileSize, expectedFileSize,
					m_articleInfo->GetDupeFallbackRound(), dupeExpectedOffset, dupeExpectedEnd,
					&articleOffset, &articleSize);
				if (rc == 1)
				{
					return false;
				}
				if (rc == 2)
				{
					warn("Malformed article %s: range %lli-%lli out of range (file size %lli)", *m_infoName,
						(long long)beginPos, (long long)endPos, (long long)articleFileSize);
					return false;
				}
				m_decodedFileSize = articleFileSize;
				if (rc == 3)
				{
					detail("Discarding article %s from duplicate: file size mismatch (%lli vs %lli)",
						*m_infoName, (long long)articleFileSize, (long long)expectedFileSize);
					return false;
				}
				if (rc == 4)
				{
					detail("Discarding article %s from duplicate: decoded offset mismatch (%lli vs %lli)",
						*m_infoName, (long long)(beginPos - 1), (long long)dupeExpectedOffset);
					return false;
				}
				if (rc == 5)
				{
					detail("Discarding article %s from duplicate: decoded end mismatch (%lli vs %lli)",
						*m_infoName, (long long)endPos, (long long)dupeExpectedEnd);
					return false;
				}
			}
		}

		if (!m_articleWriter.Start(m_decoder.GetFormat(), articleFilename, articleFileSize, articleOffset, articleSize))
		{
			m_localWriteError = true;
			return false;
		}
		m_writingStarted = true;
	}

	bool ok = m_articleWriter.Write(buffer, len);
	if (!ok)
	{
		m_localWriteError = true;
	}

	if (m_contentAnalyzer)
	{
		m_contentAnalyzer->Append(buffer, len);
	}

	return ok;
}

inline ArticleDownloader::EStatus ArticleDownloader::DecodeCheck()
{
	bool rawArticle = g_Options->GetRawArticle();
	if (rawArticle)
	{
		return adFinished;
	}
	Decoder::EStatus status = m_decoder.Check();
	bool isYenc = m_decoder.GetFormat() == Decoder::efYenc;
	switch (nzbget_rs_nntp_article_dl_classify_decode_status(
		0, (int)status, isYenc ? 1 : 0, m_articleWriter.GetRangeExceeded() ? 1 : 0))
	{
		case 1:
			if (m_decoder.GetArticleFilename())
			{
				m_articleFilename = m_decoder.GetArticleFilename();
			}
			if (isYenc)
			{
				m_articleInfo->SetCrc(nzbget_rs_dqh_article_dl_select_yenc_crc(
					g_Options->GetCrcCheck() ? 1 : 0,
					m_decoder.GetCalculatedCrc(), m_decoder.GetExpectedCrc()));
			}
			return adFinished;
		case 2:
			detail("Decoding %s failed: more data than its range", *m_infoName);
			return adFailed;
		case 3:
			detail("Decoding %s failed: CRC-Error", *m_infoName);
			return adCrcError;
		case 4:
			detail("Decoding %s failed: article incomplete", *m_infoName);
			return adFailed;
		case 5:
			detail("Decoding %s failed: size mismatch", *m_infoName);
			return adFailed;
		case 6:
			detail("Decoding %s failed: no binary data found", *m_infoName);
			return adFailed;
		default:
			detail("Decoding %s failed", *m_infoName);
			return adFailed;
	}
}

inline void ArticleDownloader::SetLastUpdateTimeNow()
{
	m_lastUpdateTime = Util::CurrentTime();
}

inline void ArticleDownloader::LogDebugInfo()
{
	info("      Download: status=%i, LastUpdateTime=%s, InfoName=%s", m_status,
		 *Util::FormatTime(m_lastUpdateTime.load()), *m_infoName);
}

inline void ArticleDownloader::Stop()
{
	debug("Trying to stop ArticleDownloader");
	Thread::Stop();
	Guard guard(m_connectionMutex);
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
	debug("ArticleDownloader stopped successfully");
}

inline void ArticleDownloader::FreeConnection(bool keepConnected)
{
	if (m_connection)
	{
		debug("Releasing connection");
		Guard guard(m_connectionMutex);
		if (!keepConnected || m_connection->GetStatus() == Connection::csCancelled)
		{
			m_connection->Disconnect();
		}
		AddServerStats();
		g_ServerPool->FreeConnection(m_connection, true);
		m_connection = nullptr;
	}
}

inline void ArticleDownloader::AddServerStats()
{
	int serverId = m_connection->GetNewsServer()->GetId();
	int bytesRead = m_connection->FetchTotalBytesRead();
	ServerVolume::Stats stats;
	stats.bytes = Util::SafeIntCast<int, uint32>(bytesRead);
	stats.articles.failed = 0;
	stats.articles.success = 0;
	g_StatMeter->AddServerStats(stats, serverId);
	m_downloadedSize += bytesRead;
}

static const int MAX_ID = 1000000000;

class GroupSorter
{
public:
	GroupSorter(NzbList* nzbList, QueueEditor::ItemList* sortItemList) :
		m_nzbList(nzbList), m_sortItemList(sortItemList), m_sortCriteria(scName), m_sortOrder(soAscending) {}
	bool Execute(const char* sort);
	bool operator()(const std::unique_ptr<NzbInfo>& refNzbInfo1, const std::unique_ptr<NzbInfo>& refNzbInfo2) const;

private:
	enum ESortCriteria
	{
		scName,
		scSize,
		scRemainingSize,
		scAge,
		scCategory,
		scPriority
	};

	enum ESortOrder
	{
		soAscending,
		soDescending,
		soAuto
	};

	NzbList* m_nzbList;
	QueueEditor::ItemList* m_sortItemList;
	ESortCriteria m_sortCriteria;
	ESortOrder m_sortOrder;
};

inline bool GroupSorter::Execute(const char* sort)
{
	int32_t rsCriteria = 0, rsOrder = 0;
	if (!nzbget_rs_qhp_parse_sort_spec(sort, &rsCriteria, &rsOrder))
	{
		error("Could not sort groups: incorrect sort order (%s)", sort);
		return false;
	}
	m_sortCriteria = (ESortCriteria)rsCriteria;
	m_sortOrder = (ESortOrder)rsOrder;

	RawNzbList tempList;
	for (NzbInfo* nzbInfo : m_nzbList)
	{
		tempList.push_back(nzbInfo);
	}

	ESortOrder origSortOrder = m_sortOrder;
	if (m_sortOrder == soAuto)
	{
		m_sortOrder = m_sortCriteria == scPriority ? soDescending : soAscending;
	}

	std::stable_sort(m_nzbList->begin(), m_nzbList->end(), *this);

	if (origSortOrder == soAuto &&
		std::equal(tempList.begin(), tempList.end(), m_nzbList->begin(),
		[](NzbInfo* nzbInfo1, std::unique_ptr<NzbInfo>& nzbInfo2)
		{
			return nzbInfo1 == nzbInfo2.get();
		}))
	{
		m_sortOrder = m_sortOrder == soDescending ? soAscending : soDescending;
		std::stable_sort(m_nzbList->begin(), m_nzbList->end(), *this);
	} 

	return true;
}

inline bool GroupSorter::operator()(const std::unique_ptr<NzbInfo>& refNzbInfo1, const std::unique_ptr<NzbInfo>& refNzbInfo2) const
{
	NzbInfo* nzbInfo1 = refNzbInfo1.get();
	NzbInfo* nzbInfo2 = refNzbInfo2.get();

	bool initialSortState = m_sortItemList->empty();
	bool sortItem1 = initialSortState;
	bool sortItem2 = initialSortState;

	for (QueueEditor::EditItem& item : m_sortItemList)
	{
		sortItem1 |= item.m_nzbInfo == nzbInfo1;
		sortItem2 |= item.m_nzbInfo == nzbInfo2;
	}

	if (!sortItem1 || !sortItem2)
	{
		return false;
	}

	return nzbget_rs_queue_editor_group_sorter_compare_values(
		(int)m_sortCriteria,
		(int)m_sortOrder,
		(int)soDescending,
		strcmp(nzbInfo1->GetName(), nzbInfo2->GetName()),
		nzbInfo1->GetSize(),
		nzbInfo2->GetSize(),
		nzbInfo1->GetRemainingSize() - nzbInfo1->GetPausedSize(),
		nzbInfo2->GetRemainingSize() - nzbInfo2->GetPausedSize(),
		(int64)nzbInfo1->GetMinTime(),
		(int64)nzbInfo2->GetMinTime(),
		strcmp(nzbInfo1->GetCategory(), nzbInfo2->GetCategory()),
		nzbInfo1->GetPriority(),
		nzbInfo2->GetPriority()) != 0;
}

inline FileInfo* QueueEditor::FindFileInfo(int id)
{
	for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
	{
		FileInfo* fileInfo = nzbInfo->GetFileList()->Find(id);
		if (fileInfo)
		{
			return fileInfo;
		}
	}
	return nullptr;
}

inline void QueueEditor::PauseUnpauseEntry(FileInfo* fileInfo, bool pause)
{
	fileInfo->SetPaused(pause);
}

inline void QueueEditor::DeleteEntry(FileInfo* fileInfo)
{
	if (!fileInfo->GetDeleted())
	{
		fileInfo->GetNzbInfo()->PrintMessage(
			fileInfo->GetNzbInfo()->GetDeleting() ? Message::mkDetail : Message::mkInfo,
			"Deleting file %s from download queue", fileInfo->GetFilename());
		g_QueueCoordinator->DeleteQueueEntry(m_downloadQueue, fileInfo);
	}
}

inline void QueueEditor::MoveEntry(FileInfo* fileInfo, int offset)
{
	int entry = 0;
	for (FileInfo* fileInfo2 : fileInfo->GetNzbInfo()->GetFileList())
	{
		if (fileInfo2 == fileInfo)
		{
			break;
		}
		entry++;
	}

	int size = (int)fileInfo->GetNzbInfo()->GetFileList()->size();
	int newEntry = 0;
	if (nzbget_rs_queue_editor_clamp_move_target(entry, offset, size, &newEntry) != 0)
	{
		std::unique_ptr<FileInfo> movedFileInfo = std::move(*(fileInfo->GetNzbInfo()->GetFileList()->begin() + entry));
		fileInfo->GetNzbInfo()->GetFileList()->erase(fileInfo->GetNzbInfo()->GetFileList()->begin() + entry);
		fileInfo->GetNzbInfo()->GetFileList()->insert(fileInfo->GetNzbInfo()->GetFileList()->begin() + newEntry, std::move(movedFileInfo));
	}
}

inline void QueueEditor::MoveGroup(NzbInfo* nzbInfo, int offset)
{
	int entry = 0;
	for (NzbInfo* nzbInfo2 : m_downloadQueue->GetQueue())
	{
		if (nzbInfo2 == nzbInfo)
		{
			break;
		}
		entry++;
	}

	int size = (int)m_downloadQueue->GetQueue()->size();
	int newEntry = 0;
	if (nzbget_rs_queue_editor_clamp_move_target(entry, offset, size, &newEntry) != 0)
	{
		std::unique_ptr<NzbInfo> movedNzbInfo = std::move(*(m_downloadQueue->GetQueue()->begin() + entry));
		m_downloadQueue->GetQueue()->erase(m_downloadQueue->GetQueue()->begin() + entry);
		m_downloadQueue->GetQueue()->insert(m_downloadQueue->GetQueue()->begin() + newEntry, std::move(movedNzbInfo));
	}
}

inline bool QueueEditor::EditEntry(DownloadQueue* downloadQueue, int ID, DownloadQueue::EEditAction action, const char* args)
{
	m_downloadQueue = downloadQueue;
	IdList cIdList;
	cIdList.push_back(ID);
	return InternEditList(nullptr, &cIdList, action, args);
}

inline bool QueueEditor::EditList(DownloadQueue* downloadQueue, IdList* idList, NameList* nameList, DownloadQueue::EMatchMode matchMode,
	DownloadQueue::EEditAction action, const char* args)
{
	if (!idList &&
		(action == DownloadQueue::eaPostDelete ||
		 (DownloadQueue::eaHistoryDelete <= action && action <= DownloadQueue::eaHistorySetName)))
	{
		return false;
	}

	if (action == DownloadQueue::eaPostDelete)
	{
		return g_PrePostProcessor->EditList(downloadQueue, idList, action, args);
	}
	else if (DownloadQueue::eaHistoryDelete <= action && action <= DownloadQueue::eaHistorySetName)
	{
		return g_HistoryCoordinator->EditList(downloadQueue, idList, action, args);
	}

	m_downloadQueue = downloadQueue;
	bool ok = true;

	std::unique_ptr<IdList> nameIdList;

	if (nameList)
	{
		nameIdList = std::make_unique<IdList>();
		idList = nameIdList.get();
		ok = BuildIdListFromNameList(idList, nameList, matchMode, action);
	}

	ok = ok && (InternEditList(nullptr, idList, action, args) || matchMode == DownloadQueue::mmRegEx);

	m_downloadQueue->Save();

	return ok;
}

inline bool QueueEditor::InternEditList(ItemList* itemList,
	IdList* idList, DownloadQueue::EEditAction action, const char* args)
{
	IdList uniqueIds;
	if (idList)
	{
		uniqueIds.resize(idList->size(), 0);
		size_t uniqueLen = nzbget_rs_queue_editor_dedup_id_list(
			idList->empty() ? nullptr : idList->data(),
			idList->size(),
			uniqueIds.empty() ? nullptr : uniqueIds.data());
		uniqueIds.resize(uniqueLen);
		idList = &uniqueIds;
	}

	ItemList workItems;
	if (!itemList)
	{
		itemList = &workItems;
		int offset = args && (action == DownloadQueue::eaFileMoveOffset ||
			action == DownloadQueue::eaGroupMoveOffset) ? atoi(args) : 0;
		PrepareList(itemList, idList, action, offset);
	}

	if (action == DownloadQueue::eaFilePauseAllPars || action == DownloadQueue::eaFilePauseExtraPars)
	{
		bool any = !itemList->empty();
		PauseParsInGroups(itemList, action == DownloadQueue::eaFilePauseExtraPars);
		return any;
	}

	switch (action)
	{
		case DownloadQueue::eaGroupMerge:
			return MergeGroups(itemList);

		case DownloadQueue::eaGroupSort:
			return SortGroups(itemList, args);

		case DownloadQueue::eaGroupMoveAfter:
		case DownloadQueue::eaGroupMoveBefore:
			return MoveGroupsTo(itemList, idList, action == DownloadQueue::eaGroupMoveBefore, args);

		case DownloadQueue::eaFileSplit:
			return SplitGroup(itemList, args);

		case DownloadQueue::eaFileReorder:
			ReorderFiles(itemList);
			break;

		default:
			for (EditItem& item : itemList)
			{
				switch (action)
				{
					case DownloadQueue::eaFilePause:
						PauseUnpauseEntry(item.m_fileInfo, true);
						break;

					case DownloadQueue::eaFileResume:
						PauseUnpauseEntry(item.m_fileInfo, false);
						break;

					case DownloadQueue::eaFileMoveOffset:
					case DownloadQueue::eaFileMoveTop:
					case DownloadQueue::eaFileMoveBottom:
						MoveEntry(item.m_fileInfo, item.m_offset);
						break;

					case DownloadQueue::eaFileDelete:
						DeleteEntry(item.m_fileInfo);
						break;

					case DownloadQueue::eaGroupSetPriority:
						SetNzbPriority(item.m_nzbInfo, args);
						break;

					case DownloadQueue::eaGroupSetCategory:
					case DownloadQueue::eaGroupApplyCategory:
						SetNzbCategory(item.m_nzbInfo, args, action == DownloadQueue::eaGroupApplyCategory);
						break;

					case DownloadQueue::eaGroupSetName:
						SetNzbName(item.m_nzbInfo, args);
						break;

					case DownloadQueue::eaGroupSetDupeKey:
					case DownloadQueue::eaGroupSetDupeScore:
					case DownloadQueue::eaGroupSetDupeMode:
						SetNzbDupeParam(item.m_nzbInfo, action, args);
						break;

					case DownloadQueue::eaGroupSetParameter:
						SetNzbParameter(item.m_nzbInfo, args);
						break;

					case DownloadQueue::eaGroupMoveTop:
					case DownloadQueue::eaGroupMoveBottom:
					case DownloadQueue::eaGroupMoveOffset:
						MoveGroup(item.m_nzbInfo, item.m_offset);
						break;

					case DownloadQueue::eaGroupPause:
					case DownloadQueue::eaGroupResume:
					case DownloadQueue::eaGroupPauseAllPars:
					case DownloadQueue::eaGroupPauseExtraPars:
						EditGroup(item.m_nzbInfo, action, args);
						break;

					case DownloadQueue::eaGroupDelete:
					case DownloadQueue::eaGroupParkDelete:
					case DownloadQueue::eaGroupDupeDelete:
					case DownloadQueue::eaGroupFinalDelete:
						if (item.m_nzbInfo->GetKind() == NzbInfo::nkUrl)
						{
							DeleteUrl(item.m_nzbInfo, action);
						}
						else
						{
							EditGroup(item.m_nzbInfo, action, args);
						}
						break;

					case DownloadQueue::eaGroupSortFiles:
						SortGroupFiles(item.m_nzbInfo);
						break;

					default:
						break;
				}
			}
	}

	return itemList->size() > 0;
}

inline void QueueEditor::PrepareList(ItemList* itemList, IdList* idList,
	DownloadQueue::EEditAction action, int offset)
{
	if (!itemList || !idList)
	{
		return;
	}

	if (action == DownloadQueue::eaFileMoveTop || action == DownloadQueue::eaGroupMoveTop)
	{
		offset = -MAX_ID;
	}
	else if (action == DownloadQueue::eaFileMoveBottom || action == DownloadQueue::eaGroupMoveBottom)
	{
		offset = MAX_ID;
	}

	itemList->reserve(idList->size());
	if (offset != 0 &&
		(action == DownloadQueue::eaFileMoveOffset ||
		 action == DownloadQueue::eaFileMoveTop ||
		 action == DownloadQueue::eaFileMoveBottom))
	{
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			int nrEntries = (int)nzbInfo->GetFileList()->size();
			int lastDestPos = -1;
			int start = offset < 0 ? 0 : nrEntries - 1;
			int end = offset < 0 ? nrEntries : -1;
			int step = offset < 0 ? 1 : -1;
			for (int index = start; index != end; index += step)
			{
				std::unique_ptr<FileInfo>& fileInfo = nzbInfo->GetFileList()->at(index);
				IdList::iterator it2 = std::find(idList->begin(), idList->end(), fileInfo->GetId());
				if (it2 != idList->end())
				{
					int workOffset = 0;
					nzbget_rs_qhp_clamp_move_offset(index, nrEntries, offset, lastDestPos, &workOffset, &lastDestPos);
					itemList->emplace_back(fileInfo.get(), nullptr, workOffset);
				}
			}
		}
	}
	else if ((offset != 0 &&
		(action == DownloadQueue::eaGroupMoveOffset ||
		 action == DownloadQueue::eaGroupMoveTop ||
		 action == DownloadQueue::eaGroupMoveBottom)) ||
		action == DownloadQueue::eaGroupMoveBefore ||
		action == DownloadQueue::eaGroupMoveAfter)
	{
		int nrEntries = (int)m_downloadQueue->GetQueue()->size();
		int lastDestPos = -1;
		int start = offset <= 0 ? 0 : nrEntries - 1;
		int end = offset <= 0 ? nrEntries : -1;
		int step = offset <= 0 ? 1 : -1;
		for (int index = start; index != end; index += step)
		{
			std::unique_ptr<NzbInfo>& nzbInfo = m_downloadQueue->GetQueue()->at(index);
			IdList::iterator it2 = std::find(idList->begin(), idList->end(), nzbInfo->GetId());
			if (it2 != idList->end())
			{
				int workOffset = 0;
				nzbget_rs_qhp_clamp_move_offset(index, nrEntries, offset, lastDestPos, &workOffset, &lastDestPos);
				itemList->emplace_back(nullptr, nzbInfo.get(), workOffset);
			}
		}
	}
	else if (action < DownloadQueue::eaGroupMoveOffset)
	{
		int maxId = 0;
		int minId = MAX_ID;
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				int ID = fileInfo->GetId();
				if (ID > maxId) maxId = ID;
				if (ID < minId) minId = ID;
			}
		}

		for (int id : *idList)
		{
			if (minId <= id && id <= maxId)
			{
				FileInfo* fileInfo = FindFileInfo(id);
				if (fileInfo)
				{
					itemList->emplace_back(fileInfo, nullptr, offset);
				}
			}
		}
	}
	else
	{
		int maxId = 0;
		int minId = MAX_ID;
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			int ID = nzbInfo->GetId();
			if (ID > maxId) maxId = ID;
			if (ID < minId) minId = ID;
		}

		for (int id : *idList)
		{
			if (minId <= id && id <= maxId)
			{
				for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
				{
					if (id == nzbInfo->GetId())
					{
						itemList->emplace_back(nullptr, nzbInfo, offset);
					}
				}
			}
		}
	}
}

inline bool QueueEditor::BuildIdListFromNameList(IdList* idList, NameList* nameList, DownloadQueue::EMatchMode matchMode, DownloadQueue::EEditAction action)
{
#ifndef HAVE_REGEX_H
	if (matchMode == DownloadQueue::mmRegEx)
	{
		return false;
	}
#endif

	std::set<int> uniqueIds;

	for (CString& name : nameList)
	{
		std::unique_ptr<RegEx> regEx;
		if (matchMode == DownloadQueue::mmRegEx)
		{
			regEx = std::make_unique<RegEx>(name);
			if (!regEx->IsValid())
			{
				return false;
			}
		}

		bool found = false;

		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (action < DownloadQueue::eaGroupMoveOffset)
				{
					BString<1024> filename("%s/%s", fileInfo->GetNzbInfo()->GetName(),
						FileSystem::BaseFileName(fileInfo->GetFilename()));
					if (((regEx && regEx->Match(filename)) || (!regEx && !strcmp(filename, name))) &&
						uniqueIds.find(fileInfo->GetId()) == uniqueIds.end())
					{
						uniqueIds.insert(fileInfo->GetId());
						idList->push_back(fileInfo->GetId());
						found = true;
					}
				}
			}

			if (action >= DownloadQueue::eaGroupMoveOffset)
			{
				const char *filename = nzbInfo->GetName();
				if (((regEx && regEx->Match(filename)) || (!regEx && !strcmp(filename, name))) &&
					uniqueIds.find(nzbInfo->GetId()) == uniqueIds.end())
				{
					uniqueIds.insert(nzbInfo->GetId());
					idList->push_back(nzbInfo->GetId());
					found = true;
				}
			}
		}

		if (!found && (matchMode == DownloadQueue::mmName))
		{
			return false;
		}
	}

	return true;
}

inline bool QueueEditor::EditGroup(NzbInfo* nzbInfo, DownloadQueue::EEditAction action, const char* args)
{
	ItemList itemList;
	bool allPaused = true;
	int id = nzbInfo->GetId();

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		itemList.emplace_back(fileInfo, nullptr, 0);
		allPaused &= fileInfo->GetPaused();
	}

	bool isGroupDel = action == DownloadQueue::eaGroupDelete ||
		action == DownloadQueue::eaGroupParkDelete ||
		action == DownloadQueue::eaGroupDupeDelete ||
		action == DownloadQueue::eaGroupFinalDelete;
	if (isGroupDel)
	{
		nzbInfo->SetDeleting(true);
		int parking = 0, avoidHistory = 0, setDupeStatus = 0, cleanupDisk = 0;
		nzbget_rs_queue_editor_eval_group_delete_flags(
			(int)action,
			(int)DownloadQueue::eaGroupParkDelete,
			(int)DownloadQueue::eaGroupDupeDelete,
			(int)DownloadQueue::eaGroupFinalDelete,
			g_Options->GetKeepHistory(),
			nzbInfo->GetUnpackCleanedUpDisk() ? 1 : 0,
			nzbInfo->GetCurrentSuccessArticles(),
			&parking, &avoidHistory, &setDupeStatus, &cleanupDisk);
		nzbInfo->SetParking(parking != 0);
		nzbInfo->SetAvoidHistory(avoidHistory != 0);
		nzbInfo->SetDeletePaused(allPaused);
		if (setDupeStatus)
		{
			nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
		}
		nzbInfo->SetCleanupDisk(cleanupDisk != 0);
	}

	DownloadQueue::EEditAction fileAction =
		(DownloadQueue::EEditAction)nzbget_rs_queue_editor_map_group_to_file_action((int32_t)action);
	bool ok = InternEditList(&itemList, nullptr, fileAction, args);

	if (isGroupDel &&
		m_downloadQueue->GetQueue()->Find(id))
	{
		DownloadQueue::Aspect deleteAspect = { DownloadQueue::eaNzbDeleted, m_downloadQueue, nzbInfo, nullptr };
		m_downloadQueue->Notify(&deleteAspect);
	}

	return ok;
}

inline void QueueEditor::PauseParsInGroups(ItemList* itemList, bool extraParsOnly)
{
	while (true)
	{
		RawFileList GroupFileList;
		FileInfo* firstFileInfo = nullptr;

		for (ItemList::iterator it = itemList->begin(); it != itemList->end(); )
		{
			EditItem& item = *it;
			if (!firstFileInfo ||
				(firstFileInfo->GetNzbInfo() == item.m_fileInfo->GetNzbInfo()))
			{
				GroupFileList.push_back(item.m_fileInfo);
				if (!firstFileInfo)
				{
					firstFileInfo = item.m_fileInfo;
				}
				itemList->erase(it);
				it = itemList->begin();
				continue;
			}
			++it;
		}

		if (!GroupFileList.empty())
		{
			PausePars(&GroupFileList, extraParsOnly);
		}
		else
		{
			break;
		}
	}
}

inline void QueueEditor::PausePars(RawFileList* fileList, bool extraParsOnly)
{
	debug("QueueEditor: Pausing pars");

	size_t count = fileList->size();
	std::vector<uint8_t> rsIsPar;
	std::vector<uint8_t> rsHasVol;
	std::vector<int64_t> rsSizes;
	rsIsPar.reserve(count);
	rsHasVol.reserve(count);
	rsSizes.reserve(count);
	std::vector<uint8_t> rsOutPause(count, 0);
	for (FileInfo* fileInfo : fileList)
	{
		rsIsPar.push_back(fileInfo->GetParFile() ? 1 : 0);
		rsHasVol.push_back(
			nzbget_rs_queue_editor_filename_has_vol(fileInfo->GetFilename()) != 0 ? 1 : 0);
		rsSizes.push_back(fileInfo->GetSize());
	}
	nzbget_rs_qhp_select_pars_to_pause(
		rsIsPar.empty() ? nullptr : rsIsPar.data(),
		rsHasVol.empty() ? nullptr : rsHasVol.data(),
		rsSizes.empty() ? nullptr : rsSizes.data(),
		count,
		extraParsOnly ? 1 : 0,
		rsOutPause.empty() ? nullptr : rsOutPause.data()
	);
	for (size_t i = 0; i < count; i++)
	{
		if (rsOutPause[i])
		{
			(*fileList)[i]->SetPaused(true);
		}
	}
}

inline void QueueEditor::SetNzbPriority(NzbInfo* nzbInfo, const char* priority)
{
	debug("Setting priority %s for %s", priority, nzbInfo->GetName());

	nzbInfo->SetPriority(priority ? atoi(priority) : 0);
}

inline void QueueEditor::SetNzbCategory(NzbInfo* nzbInfo, const char* category, bool applyParams)
{
	debug("QueueEditor: setting category '%s' for '%s'", category, nzbInfo->GetName());

	bool oldUnpack = g_Options->GetUnpack();
	const char* oldExtensions = g_Options->GetExtensions();
	if (applyParams && !Util::EmptyStr(nzbInfo->GetCategory()))
	{
		Options::Category* categoryObj = g_Options->FindCategory(nzbInfo->GetCategory(), false);
		if (categoryObj)
		{
			oldUnpack = categoryObj->GetUnpack();
			if (!Util::EmptyStr(categoryObj->GetExtensions()))
			{
				oldExtensions = categoryObj->GetExtensions();
			}
		}
	}

	g_QueueCoordinator->SetQueueEntryCategory(m_downloadQueue, nzbInfo, category);

	if (!applyParams)
	{
		return;
	}

	bool newUnpack = g_Options->GetUnpack();
	const char* newExtensions = g_Options->GetExtensions();
	if (!Util::EmptyStr(nzbInfo->GetCategory()))
	{
		Options::Category* categoryObj = g_Options->FindCategory(nzbInfo->GetCategory(), false);
		if (categoryObj)
		{
			newUnpack = categoryObj->GetUnpack();
			if (!Util::EmptyStr(categoryObj->GetExtensions()))
			{
				newExtensions = categoryObj->GetExtensions();
			}
		}
	}

	if (oldUnpack != newUnpack)
	{
		nzbInfo->GetParameters()->SetParameter("*Unpack:", newUnpack ? "yes" : "no");
	}

	nzbget_rs_queue_editor_visit_category_ext_diff(
		oldExtensions,
		newExtensions,
		nzbInfo->GetParameters(),
		[](void* ctx, const char* paramName, const char* paramVal)
		{
			static_cast<NzbParameterList*>(ctx)->SetParameter(paramName, paramVal);
		});
}

inline void QueueEditor::SetNzbName(NzbInfo* nzbInfo, const char* name)
{
	debug("QueueEditor: renaming '%s' to '%s'", nzbInfo->GetName(), name);

	std::string previous = nzbInfo->GetName();
	g_QueueCoordinator->SetQueueEntryName(m_downloadQueue, nzbInfo, name);
	if (previous != nzbInfo->GetName() && g_DupeSearch)
		g_DupeSearch->NotifyNameChanged(nzbInfo->GetId());
}

inline bool QueueEditor::MergeGroups(ItemList* itemList)
{
	if (itemList->size() == 0)
	{
		return false;
	}

	bool ok = true;

	EditItem& destItem = itemList->front();

	for (EditItem& item : itemList) 
	{
		if (item.m_nzbInfo != destItem.m_nzbInfo)
		{
			debug("merge %s to %s", item.m_nzbInfo->GetFilename(), destItem.m_nzbInfo->GetFilename());
			if (!g_QueueCoordinator->MergeQueueEntries(m_downloadQueue, destItem.m_nzbInfo, item.m_nzbInfo))
			{
				ok = false;
			}
		}
	}

	return ok;
}

inline bool QueueEditor::SplitGroup(ItemList* itemList, const char* name)
{
	if (itemList->size() == 0)
	{
		return false;
	}

	RawFileList fileList;

	for (EditItem& item : itemList)
	{
		fileList.push_back(item.m_fileInfo);
	}

	NzbInfo* newNzbInfo = nullptr;
	bool ok = g_QueueCoordinator->SplitQueueEntries(m_downloadQueue, &fileList, name, &newNzbInfo);

	return ok;
}

inline bool QueueEditor::SortGroups(ItemList* itemList, const char* sort)
{
	AlignGroups(itemList);
	if (!sort)
	{
		return false;
	}
	GroupSorter sorter(m_downloadQueue->GetQueue(), itemList);
	return sorter.Execute(sort);
}

inline void QueueEditor::AlignGroups(ItemList* itemList)
{
	NzbList* nzbList = m_downloadQueue->GetQueue();
	NzbInfo* lastNzbInfo = nullptr;
	uint32 lastNum = 0;
	uint32 num = 0;
	while (num < nzbList->size())
	{
		std::unique_ptr<NzbInfo>& nzbInfo = nzbList->at(num);

		bool selected = false;
		for (QueueEditor::EditItem& item : itemList)
		{
			if (item.m_nzbInfo == nzbInfo.get())
			{
				selected = true;
				break;
			}
		}

		if (selected)
		{
			if (lastNzbInfo && num - lastNum > 1)
			{
				std::unique_ptr<NzbInfo> movedNzbInfo = std::move(*(nzbList->begin() + num));
				nzbList->erase(nzbList->begin() + num);
				nzbList->insert(nzbList->begin() + lastNum + 1, std::move(movedNzbInfo));
				lastNum++;
			}
			else
			{
				lastNum = num;
			}
			lastNzbInfo = nzbInfo.get();
		}
		num++;
	}
}

inline bool QueueEditor::ItemListContainsItem(ItemList* itemList, int id)
{
	return std::find_if(itemList->begin(), itemList->end(),
		[id](const EditItem& item)
		{
			return item.m_nzbInfo->GetId() == id;
		}) != itemList->end();
}

inline bool QueueEditor::MoveGroupsTo(ItemList* itemList, IdList* idList, bool before, const char* args)
{
	if (itemList->size() == 0 || Util::EmptyStr(args))
	{
		return false;
	}

	int targetId = atoi(args);
	int offset = 0;

	if (!m_downloadQueue->GetQueue()->Find(targetId))
	{
		return false;
	}

	if (ItemListContainsItem(itemList, targetId))
	{
		std::vector<int32_t> queueIds;
		queueIds.reserve(m_downloadQueue->GetQueue()->size());
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			queueIds.push_back(nzbInfo->GetId());
		}
		std::vector<int32_t> movedIds;
		movedIds.reserve(itemList->size());
		for (const EditItem& item : *itemList)
		{
			movedIds.push_back(item.m_nzbInfo->GetId());
		}
		int32_t newTargetId = targetId, newBefore = before ? 1 : 0, newOffset = offset;
		nzbget_rs_queue_editor_resolve_move_groups_target(
			targetId,
			queueIds.empty() ? nullptr : queueIds.data(),
			queueIds.size(),
			movedIds.empty() ? nullptr : movedIds.data(),
			movedIds.size(),
			before ? 1 : 0,
			MAX_ID,
			&newTargetId,
			&newBefore,
			&newOffset);
		targetId = newTargetId;
		before = newBefore != 0;
		offset = newOffset;
	}

	AlignGroups(itemList);

	if (offset == 0)
	{
		int moveId = itemList->at(0).m_nzbInfo->GetId();
		std::vector<int> queueIds;
		queueIds.reserve(m_downloadQueue->GetQueue()->size());
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			queueIds.push_back(nzbInfo->GetId());
		}
		offset = nzbget_rs_queue_editor_calc_move_groups_to_offset(
			queueIds.data(), queueIds.size(), targetId, moveId,
			before ? 1 : 0, itemList->size());
	}

	BString<100> offStr("%i", offset);
	return InternEditList(nullptr, idList, DownloadQueue::eaGroupMoveOffset, offStr);
}

inline void QueueEditor::ReorderFiles(ItemList* itemList)
{
	if (itemList->size() == 0)
	{
		return;
	}

	EditItem& firstItem = itemList->front();
	NzbInfo* nzbInfo = firstItem.m_fileInfo->GetNzbInfo();
	uint32 insertPos = 0;

	for (EditItem& item : itemList)
	{
		FileInfo* fileInfo = item.m_fileInfo;

		FileList::iterator it2 = nzbInfo->GetFileList()->Find(fileInfo);
		if (it2 != nzbInfo->GetFileList()->end())
		{
			std::unique_ptr<FileInfo> movedFileInfo = std::move(*it2);
			nzbInfo->GetFileList()->erase(it2);
			nzbInfo->GetFileList()->insert(nzbInfo->GetFileList()->begin() + insertPos, std::move(movedFileInfo));
			insertPos++;
		}
	}
}

inline void QueueEditor::SetNzbParameter(NzbInfo* nzbInfo, const char* paramString)
{
	debug("QueueEditor: setting nzb parameter '%s' for '%s'", paramString, FileSystem::BaseFileName(nzbInfo->GetFilename()));

	const char* eq = paramString ? strchr(paramString, '=') : nullptr;
	if (eq)
	{
		std::string name(paramString, (size_t)(eq - paramString));
		nzbInfo->GetParameters()->SetParameter(name.c_str(), eq + 1);
	}
	else
	{
		error("Could not set nzb parameter for %s: invalid argument: %s", nzbInfo->GetName(), paramString);
	}
}

inline void QueueEditor::SetNzbDupeParam(NzbInfo* nzbInfo, DownloadQueue::EEditAction action, const char* text)
{
	debug("QueueEditor: setting dupe parameter %i='%s' for '%s'", (int)action, text, nzbInfo->GetName());

	switch (action)
	{
		case DownloadQueue::eaGroupSetDupeKey:
			nzbInfo->GetParameters()->SetParameter(DupeSearch::OwnedKeyParam, nullptr);
			nzbInfo->SetDupeKey(text);
			break;

		case DownloadQueue::eaGroupSetDupeScore:
			nzbInfo->SetDupeScore(text ? atoi(text) : 0);
			break;

		case DownloadQueue::eaGroupSetDupeMode:
			{
				int parsedMode = 0;
				if (!nzbget_rs_qhp_parse_dupe_mode(text, &parsedMode))
				{
					error("Could not set duplicate mode for %s: incorrect mode (%s)", nzbInfo->GetName(), text);
					return;
				}
				nzbInfo->SetDupeMode((EDupeMode)parsedMode);
				break;
			}

		default:
			break;
	}
}

inline void QueueEditor::SortGroupFiles(NzbInfo* nzbInfo)
{
	debug("QueueEditor: sorting inner files for '%s'", nzbInfo->GetName());

	std::sort(nzbInfo->GetFileList()->begin(), nzbInfo->GetFileList()->end(),
		[](const std::unique_ptr<FileInfo>& fileInfo1, const std::unique_ptr<FileInfo>& fileInfo2)
		{
			bool samePar = fileInfo1->GetParFile() && fileInfo2->GetParFile() &&
				ParParser::SameParCollection(fileInfo1->GetFilename(), fileInfo2->GetFilename(),
					fileInfo1->GetFilenameConfirmed() && fileInfo2->GetFilenameConfirmed());
			return nzbget_rs_qhp_compare_inner_files(
				fileInfo1->GetFilename(),
				fileInfo1->GetParFile() ? 1 : 0,
				fileInfo1->GetSize(),
				fileInfo2->GetFilename(),
				fileInfo2->GetParFile() ? 1 : 0,
				fileInfo2->GetSize(),
				samePar ? 1 : 0
			) != 0;
		});
}

inline bool QueueEditor::DeleteUrl(NzbInfo* nzbInfo, DownloadQueue::EEditAction action)
{
	return g_UrlCoordinator->DeleteQueueEntry(m_downloadQueue, nzbInfo, action == DownloadQueue::eaGroupFinalDelete);
}

namespace DupeProbeDetail
{
	inline constexpr int ConnectionWaitSec = 10;
	inline constexpr int ProbeLimitSec = 90;

	inline Mutex g_probeMutex;
	inline std::set<DupeProbe*> g_probes;
	inline std::atomic<int> g_probeCount{0};
	inline bool g_probesStopping = false;
}

inline std::vector<size_t> DupeProbe::SampleIndexes(size_t total, int count)
{
	std::vector<size_t> indexes;
	if (total == 0 || count <= 0)
	{
		return indexes;
	}

	size_t wanted = std::min(total, (size_t)count);
	indexes.resize(wanted);
	nzbget_rs_dupe_probe_sample_indexes(total, count, indexes.data(), indexes.size());
	return indexes;
}

inline DupeProbe::EAnswer DupeProbe::Classify(const char* response)
{
	return (EAnswer)nzbget_rs_dupe_probe_classify(response);
}

inline bool DupeProbe::IsDead(int existing, int definitiveServers, int activeServers)
{
	return nzbget_rs_dupe_probe_is_dead(existing, definitiveServers, activeServers) != 0;
}

inline void DupeProbe::Start(int nzbId, std::vector<Sample> samples, int recoveredAtStart)
{
	DupeProbe* probe = new DupeProbe(nzbId, std::move(samples));
	probe->m_recoveredAtStart = recoveredAtStart;
	if (!probe->Register())
	{
		delete probe;
		return;
	}
	probe->SetAutoDestroy(true);
	probe->Thread::Start();
}

inline DupeProbe::Verdict DupeProbe::Check(std::vector<Sample> samples, int limitSec)
{
	DupeProbe probe(0, std::move(samples));
	if (!probe.Register())
	{
		Verdict stopping;
		stopping.Finished = false;
		return stopping;
	}
	Verdict verdict = probe.Measure(limitSec);
	probe.Unregister();
	return verdict;
}

inline void DupeProbe::Reset()
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	g_probesStopping = false;
}

inline int DupeProbe::ProbingFor(int nzbId)
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	for (DupeProbe* probe : g_probes)
	{
		if (probe->m_nzbId == nzbId && !probe->m_countAll)
		{
			return std::max(0, (int)(Util::CurrentTime() - probe->m_started));
		}
	}
	return -1;
}

inline bool DupeProbe::Register()
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	if (g_probesStopping)
	{
		return false;
	}
	m_started = Util::CurrentTime();
	g_probes.insert(this);
	g_probeCount++;
	return true;
}

inline void DupeProbe::Unregister()
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	g_probes.erase(this);
	g_probeCount--;
}

inline std::vector<DupeProbe::Sample> DupeProbe::SamplesOf(const std::vector<FileInfo*>& files,
	const std::function<void(FileInfo*)>& loadArticles)
{
	size_t total = 0;
	for (FileInfo* fileInfo : files)
	{
		total += fileInfo->GetTotalArticles();
	}

	std::vector<Sample> samples;
	for (size_t position : SampleIndexes(total, SampleCount))
	{
		size_t offset = 0;
		for (FileInfo* fileInfo : files)
		{
			size_t count = fileInfo->GetTotalArticles();
			if (position >= offset + count)
			{
				offset += count;
				continue;
			}
			size_t index = position - offset;

			if (fileInfo->GetArticles()->empty() && loadArticles)
			{
				loadArticles(fileInfo);
			}
			if (index < fileInfo->GetArticles()->size())
			{
				Sample sample;
				sample.MessageId = fileInfo->GetArticles()->at(index)->GetMessageId();
				sample.Groups = std::make_shared<std::vector<CString>>();
				for (const CString& group : *fileInfo->GetGroups())
				{
					sample.Groups->emplace_back(*group);
				}
				samples.push_back(std::move(sample));
			}
			break;
		}
	}
	return samples;
}

inline void DupeProbe::StopAll()
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	g_probesStopping = true;
	for (DupeProbe* probe : g_probes)
	{
		probe->Thread::Stop();
		probe->Cancel();
	}
}

inline void DupeProbe::WaitAll()
{
	using namespace DupeProbeDetail;
	while (g_probeCount > 0)
	{
		Util::Sleep(20);
	}
}

inline void DupeProbe::Cancel()
{
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
}

inline bool DupeProbe::ProbeServer(int serverId, std::set<int>& probed, ServerResult& result)
{
	using namespace DupeProbeDetail;
	NewsServer* wanted = g_ServerPool->GetServerById(serverId);
	if (!wanted)
	{
		return false;
	}

	NntpConnection* connection = nullptr;
	time_t waitStart = Util::CurrentTime();
	while (!connection && !IsStopped() && Util::CurrentTime() - waitStart <= ConnectionWaitSec)
	{
		connection = g_ServerPool->GetConnection(wanted->GetNormLevel(), wanted, nullptr);
		if (!connection)
		{
			Util::Sleep(50);
		}
	}
	if (!connection)
	{
		time_t blockTime = wanted->GetBlockTime();
		result.Unreachable = g_ServerPool->IsServerBlocked(wanted) ||
			(blockTime > 0 && Util::CurrentTime() - blockTime < RecentBlockSec);
		return false;
	}

	NewsServer* server = connection->GetNewsServer();
	bool fresh = probed.insert(server->GetId()).second;
	bool probedServer = false;
	if (fresh)
	{
		{
			Guard guard(g_probeMutex);
			m_connection = connection;
		}

		int remaining = (int)m_samples.size();
		for (size_t index = 0; index < m_samples.size(); index++)
		{
			const Sample& sample = m_samples[index];
			if (IsStopped())
			{
				break;
			}
			remaining--;
			if (m_found[index])
			{
				continue;
			}

			if (!connection->Connect())
			{
				result.Unknown += remaining + 1;
				result.Unreachable = result.Exists + result.Missing == 0;
				break;
			}

			if (server->GetJoinGroup())
			{
				const char* joined = nullptr;
				for (const CString& group : *sample.Groups)
				{
					joined = connection->JoinGroup(group);
					if (joined && !strncmp(joined, "2", 1))
					{
						break;
					}
				}
				if (!joined || strncmp(joined, "2", 1))
				{
					result.Unknown += remaining + 1;
					if (!joined)
					{
						connection->Disconnect();
					}
					break;
				}
			}

			BString<1024> statCmd("STAT %s\r\n", *sample.MessageId);
			const char* response = connection->Request(statCmd);
			if (!response)
			{
				connection->Disconnect();
				result.Unknown += remaining + 1;
				break;
			}

			switch (Classify(response))
			{
				case daExists:
					result.Exists++;
					m_found[index] = 1;
					if (m_foundOn.empty())
					{
						m_foundOn = std::string(server->GetName()) + " (" + *sample.MessageId + ")";
					}
					break;
				case daMissing:
					result.Missing++;
					break;
				case daUnknown:
					result.Unknown++;
					break;
			}

			int foundCount = (int)std::count(m_found.begin(), m_found.end(), 1);
			if (!m_countAll && foundCount >= MinAliveSamples)
			{
				break;
			}
		}
		probedServer = true;
	}

	{
		Guard guard(g_probeMutex);
		m_connection = nullptr;
	}
	if (IsStopped() || connection->GetStatus() == Connection::csCancelled)
	{
		connection->Disconnect();
	}
	g_ServerPool->FreeConnection(connection, true);
	return probedServer;
}

inline DupeProbe::Verdict DupeProbe::Measure(int limitSec)
{
	Verdict verdict;
	Servers* servers = g_ServerPool->GetServers();
	std::vector<uint8_t> srvActive;
	std::vector<int32_t> srvGroups;
	std::vector<int32_t> srvLevels;
	std::vector<int32_t> srvIds;
	srvActive.reserve(servers->size());
	srvGroups.reserve(servers->size());
	srvLevels.reserve(servers->size());
	srvIds.reserve(servers->size());
	for (NewsServer* server : servers)
	{
		srvActive.push_back(server->GetActive() ? 1 : 0);
		srvGroups.push_back(server->GetGroup());
		srvLevels.push_back(server->GetNormLevel());
		srvIds.push_back(server->GetId());
	}
	std::vector<int32_t> order(servers->size());
	size_t orderLen = nzbget_rs_dupe_probe_select_server_order(
		srvActive.data(), srvGroups.data(), srvLevels.data(), srvIds.data(),
		servers->size(), order.data(), order.size());
	order.resize(orderLen);
	verdict.ActiveServers = (int)orderLen;

	std::set<int> probed;
	time_t start = Util::CurrentTime();

	for (int serverId : order)
	{
		if (IsStopped() || Util::CurrentTime() - start > limitSec)
		{
			verdict.Finished = false;
			break;
		}

		ServerResult result;
		bool reached = ProbeServer(serverId, probed, result);
		if (result.Unreachable)
		{
			verdict.ActiveServers--;
			verdict.UnreachableServers++;
		}
		if (!reached)
		{
			continue;
		}
		verdict.ReachedServers++;

		verdict.Existing = (int)std::count(m_found.begin(), m_found.end(), 1);
		verdict.FoundOn = m_foundOn;
		if (m_countAll)
		{
			if (verdict.Existing == (int)m_samples.size())
			{
				break;
			}
			continue;
		}
		if (verdict.Existing >= MinAliveSamples)
		{
			break;
		}
		if (result.Unknown == 0 && result.Exists + result.Missing > 0)
		{
			verdict.MissingServers++;
		}
	}
	if (IsStopped())
	{
		verdict.Finished = false;
	}
	return verdict;
}

inline void DupeProbe::StartRecheck(int nzbId, std::vector<Sample> samples)
{
	DupeProbe* probe = new DupeProbe(nzbId, std::move(samples));
	probe->m_countAll = true;
	probe->m_found.assign(probe->m_samples.size(), 0);
	if (!probe->Register())
	{
		delete probe;
		return;
	}
	probe->SetAutoDestroy(true);
	probe->Thread::Start();
}

inline void DupeProbe::Recheck()
{
	Verdict verdict = Measure(RecheckLimitSec);
	if (IsStopped() || !verdict.Finished)
	{
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() != HistoryInfo::hkNzb || historyInfo->GetNzbInfo()->GetId() != m_nzbId)
		{
			continue;
		}
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		int sampled = (int)m_samples.size();
		const char* duplicateQueuedName = "";
		if (verdict.ReachedServers > 0 && verdict.Existing * 2 >= sampled)
		{
			for (NzbInfo* queued : downloadQueue->GetQueue())
			{
				if (DupeCoordinator::SameNameOrKey(queued->GetName(), queued->GetDupeKey(),
					nzbInfo->GetName(), nzbInfo->GetDupeKey()))
				{
					duplicateQueuedName = queued->GetName();
					break;
				}
			}
		}

		int outcome = 0;
		NzbgetRsBuf msgBuf = nzbget_rs_dupe_probe_eval_recheck_outcome(
			verdict.Existing, sampled, verdict.ReachedServers, duplicateQueuedName, &outcome);
		std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
		nzbget_rs_free(msgBuf);

		nzbInfo->PrintMessage(Message::mkInfo, "%s", message.c_str());
		if (outcome == 1)
		{
			nzbInfo->GetParameters()->SetParameter("*DupeRecheck", nullptr);
			return;
		}
		if (outcome == 2)
		{
			return;
		}
		if (outcome == 3)
		{
			IdList ids = { historyInfo->GetId() };
			downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId, DownloadQueue::eaHistoryRetryFailed, nullptr);
		}
		break;
	}
}

inline void DupeProbe::Run()
{
	using namespace DupeProbeDetail;
	if (m_countAll)
	{
		Recheck();
		Unregister();
		return;
	}

	Verdict verdict = Measure(ProbeLimitSec);
	if (IsStopped())
	{
		Unregister();
		return;
	}

	int sampled = (int)m_samples.size();
	int outcome = 0;
	NzbgetRsBuf msgBuf = nzbget_rs_dupe_probe_eval_run_outcome(
		verdict.Existing,
		sampled,
		verdict.MissingServers,
		verdict.ActiveServers,
		verdict.UnreachableServers,
		verdict.ReachedServers,
		verdict.Finished ? 1 : 0,
		verdict.FoundOn.c_str(),
		&outcome);
	std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
	nzbget_rs_free(msgBuf);

	if (outcome == 1)
	{
		Note(Message::mkInfo, "%s", message.c_str());
		Abandon(verdict.MissingServers, verdict.ActiveServers);
	}
	else if (outcome == 2)
	{
		Note(Message::mkInfo, "%s", message.c_str());
	}
	else if (outcome == 3)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo)
		{
			nzbInfo->SetDeadPickProbed(false);
			nzbInfo->PrintMessage(Message::mkDetail, "%s", message.c_str());
		}
	}
	else if (outcome == 4)
	{
		Note(Message::mkInfo, "%s", message.c_str());
	}

	Unregister();
}

inline void DupeProbe::Note(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, sizeof(text), format, args);
	va_end(args);

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (nzbInfo)
	{
		nzbInfo->PrintMessage(kind, "%s", text);
	}
	else
	{
		detail("Dupe probe of download %i: %s", m_nzbId, text);
	}
}

inline void DupeProbe::Abandon(int missingServers, int activeServers)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	g_QueueCoordinator->FailOverDeadPick(downloadQueue, m_nzbId, (int)m_samples.size(),
		missingServers, activeServers, m_recoveredAtStart);
}

inline bool QueueCoordinator::CoordinatorDownloadQueue::EditEntry(
	int ID, EEditAction action, const char* args)
{
	return m_owner->m_queueEditor.EditEntry(&m_owner->m_downloadQueue, ID, action, args);
}

inline bool QueueCoordinator::CoordinatorDownloadQueue::EditList(
	IdList* idList, NameList* nameList, EMatchMode matchMode, EEditAction action, const char* args)
{
	m_massEdit = true;
	bool ret = m_owner->m_queueEditor.EditList(&m_owner->m_downloadQueue, idList, nameList, matchMode, action, args);
	m_massEdit = false;
	if (m_wantSave)
	{
		Save();
	}
	return ret;
}

inline void QueueCoordinator::CoordinatorDownloadQueue::Save()
{
	if (m_massEdit)
	{
		m_wantSave = true;
		return;
	}

	if (g_Options->GetServerMode())
	{
		g_DiskState->SaveDownloadQueue(this, m_historyChanged);
		m_stateChanged = true;
	}

	for (NzbInfo* nzbInfo : GetQueue())
	{
		nzbInfo->SetChanged(false);
	}

	m_wantSave = false;
	m_historyChanged = false;

	// queue has changed, time to wake up if in standby
	m_owner->WakeUp();
}

inline void QueueCoordinator::CoordinatorDownloadQueue::SaveChanged()
{
	if (g_Options->GetServerMode())
	{
		g_DiskState->SaveDownloadProgress(this);
		m_stateChanged = true;
	}
}

inline QueueCoordinator::QueueCoordinator()
{
	debug("Creating QueueCoordinator");

	CoordinatorDownloadQueue::Init(&m_downloadQueue);
	g_WorkState->Attach(this);
	// a reload creates a new coordinator: probes may run again
	DupeProbe::Reset();
	TwinCheck::Reset();
}

inline QueueCoordinator::~QueueCoordinator()
{
	debug("Destroying QueueCoordinator");

	DupeProbe::WaitAll();
	TwinCheck::WaitAll();

	for (ArticleDownloader* articleDownloader : m_activeDownloads)
	{
		delete articleDownloader;
	}
	m_activeDownloads.clear();

	CoordinatorDownloadQueue::Final();

	debug("QueueCoordinator destroyed");
}

inline void QueueCoordinator::Load()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	bool statLoaded = true;
	bool perfectServerMatch = true;
	bool queueLoaded = false;

	if (g_Options->GetServerMode())
	{
		statLoaded = g_StatMeter->Load(&perfectServerMatch);

		if (g_DiskState->DownloadQueueExists())
		{
			queueLoaded = g_DiskState->LoadDownloadQueue(downloadQueue, g_ServerPool->GetServers());
		}
		else
		{
			g_DiskState->DiscardDownloadQueue();
		}
	}

	if (queueLoaded && statLoaded)
	{
		g_DiskState->CleanupTempDir(downloadQueue);
	}

	if (queueLoaded && g_DiskState->LegacyQueueRead())
	{
		// written again at once in the upstream format: until the next change it would
		// stay in the earlier build's format, which upstream can't read after a downgrade
		info("Saving the queue and history of an earlier build in the upstream format");
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}

	if (queueLoaded && statLoaded && !perfectServerMatch)
	{
		debug("Changes in section <NEWS SERVERS> of config file detected, resaving queue");

		// re-save current server list into diskstate to update server ids
		g_StatMeter->Save();

		// re-save queue into diskstate to update server ids
		downloadQueue->HistoryChanged();
		downloadQueue->Save();

		// re-save file states into diskstate to update server ids
		if (g_Options->GetServerMode())
		{
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				if (g_Options->GetContinuePartial())
				{
					for (FileInfo* fileInfo : nzbInfo->GetFileList())
					{
						if (!fileInfo->GetArticles()->empty())
						{
							g_DiskState->SaveFileState(fileInfo, false);
						}
					}
				}

				for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
				{
					if ((completedFile.GetStatus() == CompletedFile::cfPartial ||
						completedFile.GetStatus() == CompletedFile::cfFailure) &&
						completedFile.GetId() > 0)
					{
						FileInfo fileInfo(completedFile.GetId());
						if (g_DiskState->LoadFileState(&fileInfo, g_ServerPool->GetServers(), true))
						{
							g_DiskState->SaveFileState(&fileInfo, true);
						}
					}
				}
			}
		}
	}

	CoordinatorDownloadQueue::Loaded();
}

inline void QueueCoordinator::Run()
{
	debug("Entering QueueCoordinator-loop");

	Load();
	AdjustDownloadsLimit();
	bool wasStandBy = true;
	bool articeDownloadsRunning = false;
	time_t lastReset = 0;
	g_StatMeter->IntervalCheck();
	int waitInterval = 100;
	while (!IsStopped())
	{
		bool downloadsChecked = false;
		bool downloadStarted = false;
		FileInfo* fileInfo;
		ArticleInfo* articleInfo;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

			bool hasMoreArticles = GetNextArticle(downloadQueue, fileInfo, articleInfo);
			articeDownloadsRunning = !m_activeDownloads.empty();
			m_hasMoreJobs = hasMoreArticles || articeDownloadsRunning;
			int effectiveLimit = g_MemoryMonitor
				? g_MemoryMonitor->GetEffectiveDownloadsLimit(m_downloadsLimit)
				: m_downloadsLimit;
			bool canProceed = !IsStopped() && (int)m_activeDownloads.size() < effectiveLimit;

			if (hasMoreArticles && canProceed &&
				(!g_WorkState->GetDownloadHeld() || (fileInfo && fileInfo->GetExtraPriority())))
			{
				NntpConnection* connection = nullptr;
			
				bool hasDesiredServer = fileInfo->GetNzbInfo()->HasDesiredServer();
				NewsServer* desiredServer = hasDesiredServer ?
					g_ServerPool->GetServerById(fileInfo->GetNzbInfo()->GetDesiredServerId()) : nullptr;
				bool useSpecificServer = hasDesiredServer && desiredServer && desiredServer->GetActive();
				int connLevel = useSpecificServer ? desiredServer->GetNormLevel() : 0;
				connection = g_ServerPool->GetConnection(
					connLevel, useSpecificServer ? desiredServer : nullptr, nullptr);
				 
				if (connection)
				{
					downloadsChecked = true;
					articeDownloadsRunning = true;
					downloadStarted = true;
					StartDeadPickProbe(downloadQueue, fileInfo->GetNzbInfo());
					StartArticleDownload(fileInfo, articleInfo, connection);
				}
				else if (fileInfo->GetNzbInfo()->HasDesiredServer())
				{
					debug("Could not start download for %s: desired server %i has no available connections",
						fileInfo->GetNzbInfo()->GetName(), fileInfo->GetNzbInfo()->GetDesiredServerId());
				}
			}
		}

		if (!downloadsChecked)
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			articeDownloadsRunning = !m_activeDownloads.empty();
		}

		bool standBy = !articeDownloadsRunning;
		if (standBy != wasStandBy)
		{
			g_StatMeter->EnterLeaveStandBy(standBy);
			g_WorkState->SetDownloading(!standBy);
			wasStandBy = standBy;
			if (standBy)
			{
				SaveAllPartialState();
			}
		}

		// sleep longer in StandBy
		if (standBy)
		{
			// sleeping max. 2 seconds; can't sleep much longer because we can't rely on
			// notifications from 'WorkState' and we also have periodical work to do here
			waitInterval = std::min(waitInterval * 2, 2000);

			// woken by WakeUp (new work), not by m_hasMoreJobs: queued articles that
			// can't start (downloads held while post-processing pauses the queue, no
			// server connection) kept it true, and the loop spun without sleeping
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait_for(lk, std::chrono::milliseconds(waitInterval),
								[&] { return m_wakeUp || IsStopped(); });
			m_wakeUp = false;
		}
		else
		{
			int sleepInterval = downloadStarted ? 0 : 5;
			Util::Sleep(sleepInterval);
			g_StatMeter->AddSpeedReading(0);
			waitInterval = 100;
		}

		if (lastReset != Util::CurrentTime())
		{
			// this code should not be called too often, once per second is OK
			g_ServerPool->CloseUnusedConnections();
			ResetHangingDownloads();
			if (!standBy)
			{
				SaveAllPartialState();
			}
			g_StatMeter->IntervalCheck();
			g_Log->IntervalCheck();
			AdjustDownloadsLimit();
			Util::SetStandByMode(standBy);
			lastReset = Util::CurrentTime();
		}
	}

	WaitJobs();
	SaveAllPartialState();
	SaveQueueIfChanged();
	SaveAllFileState();

	debug("Exiting QueueCoordinator-loop");
}

inline void QueueCoordinator::WakeUp()
{
	debug("Waking up QueueCoordinator");
	// Resume Run()
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_hasMoreJobs = true;
	m_wakeUp = true;
	m_waitCond.notify_all();
}

inline void QueueCoordinator::WaitJobs()
{
	// waiting for downloads
	debug("QueueCoordinator: waiting for Downloads to complete");

	while (true)
	{
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			if (m_activeDownloads.empty())
			{
				break;
			}
		}
		Util::Sleep(100);
		ResetHangingDownloads();
	}

	debug("QueueCoordinator: Downloads are completed");
}

/*
 * Compute maximum number of allowed download threads
**/
inline void QueueCoordinator::AdjustDownloadsLimit()
{
	if (m_serverConfigGeneration == g_ServerPool->GetGeneration())
	{
		return;
	}

	std::vector<int32_t> normLevels;
	std::vector<uint8_t> actives;
	std::vector<int32_t> maxConns;
	for (NewsServer* newsServer : g_ServerPool->GetServers())
	{
		normLevels.push_back(newsServer->GetNormLevel());
		actives.push_back(newsServer->GetActive() ? 1 : 0);
		maxConns.push_back(newsServer->GetMaxConnections());
	}
	m_downloadsLimit = nzbget_rs_qhp_adjust_downloads_limit(
		normLevels.empty() ? nullptr : normLevels.data(),
		actives.empty() ? nullptr : actives.data(),
		maxConns.empty() ? nullptr : maxConns.data(),
		normLevels.size());
}

inline NzbInfo* QueueCoordinator::AddNzbFileToQueue(std::unique_ptr<NzbInfo> nzbInfo, NzbInfo* urlInfo, bool addFirst)
{
	debug("Adding NZBFile to queue");

	NzbInfo* addedNzb = nzbInfo.get();

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	DownloadQueue::Aspect foundAspect = { DownloadQueue::eaNzbFound, downloadQueue, nzbInfo.get(), nullptr };
	downloadQueue->Notify(&foundAspect);

	NzbInfo::EDeleteStatus deleteStatus = nzbInfo->GetDeleteStatus();

	if (deleteStatus != NzbInfo::dsNone)
	{
		bool allPaused = !nzbInfo->GetFileList()->empty();
		for (FileInfo* fileInfo: nzbInfo->GetFileList())
		{
			allPaused &= fileInfo->GetPaused();
			if (g_Options->GetServerMode())
			{
				g_DiskState->DiscardFile(fileInfo->GetId(), true, false, false);
			}
		}
		nzbInfo->SetDeletePaused(allPaused);
	}

	if (deleteStatus == NzbInfo::dsNone)
	{
		if (g_Options->GetDupeCheck() && nzbInfo->GetDupeMode() != dmForce)
		{
			CheckDupeFileInfos(nzbInfo.get());
		}

		if (urlInfo)
		{
			// insert at the URL position
			downloadQueue->GetQueue()->insert(downloadQueue->GetQueue()->Find(urlInfo), std::move(nzbInfo));
		}
		else
		{
			downloadQueue->GetQueue()->Add(std::move(nzbInfo), addFirst);
		}
	}
	else
	{
		// temporary adding to queue in order for listeners to see it
		downloadQueue->GetQueue()->Add(std::move(nzbInfo), true);
	}

	if (urlInfo)
	{
		addedNzb->SetId(urlInfo->GetId());
		downloadQueue->GetQueue()->Remove(urlInfo);
	}

	if (deleteStatus == NzbInfo::dsNone)
	{
		addedNzb->PrintMessage(Message::mkInfo, "Collection %s added to queue", addedNzb->GetName());
	}

	if (deleteStatus != NzbInfo::dsManual)
	{
		DownloadQueue::Aspect addedAspect = { DownloadQueue::eaNzbAdded, downloadQueue, addedNzb, nullptr };
		downloadQueue->Notify(&addedAspect);
	}

	if (deleteStatus != NzbInfo::dsNone)
	{
		// in a case if none of listeners did already delete the temporary object - we do it ourselves
		downloadQueue->GetQueue()->Remove(addedNzb);
		if (!downloadQueue->GetHistory()->Find(addedNzb->GetId()))
		{
			addedNzb = nullptr;
		}
	}

	downloadQueue->Save();

	return addedNzb;
}

inline void QueueCoordinator::CheckDupeFileInfos(NzbInfo* nzbInfo)
{
	debug("CheckDupeFileInfos");

	std::vector<FileInfo*> files;
	std::vector<const char*> filenames;
	std::vector<int64_t> sizes;
	files.reserve(nzbInfo->GetFileList()->size());
	filenames.reserve(nzbInfo->GetFileList()->size());
	sizes.reserve(nzbInfo->GetFileList()->size());
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		files.push_back(fileInfo);
		filenames.push_back(fileInfo->GetFilename());
		sizes.push_back(fileInfo->GetSize());
	}

	std::vector<size_t> dupeIndices(files.size(), 0);
	size_t dupeCount = nzbget_rs_queue_coord_find_dupe_file_indices(
		filenames.data(), sizes.data(), files.size(),
		dupeIndices.data(), dupeIndices.size());

	for (size_t i = 0; i < dupeCount; i++)
	{
		FileInfo* fileInfo = files[dupeIndices[i]];
		warn("File \"%s\" appears twice in collection, adding only the biggest file", fileInfo->GetFilename());
		int fileId = fileInfo->GetId();
		nzbInfo->UpdateDeletedStats(fileInfo);
		nzbInfo->GetFileList()->Remove(fileInfo);
		if (g_Options->GetServerMode())
		{
			g_DiskState->DiscardFile(fileId, true, false, false);
		}
	}
}

inline void QueueCoordinator::Stop()
{
	Thread::Stop();

	DupeProbe::StopAll();
	TwinCheck::StopAll();

	debug("Stopping ArticleDownloads");
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		for (ArticleDownloader* articleDownloader : m_activeDownloads)
		{
			articleDownloader->Stop();
		}
	}
	debug("ArticleDownloads are notified");

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

/*
 * Returns next article for download.
 */
inline bool QueueCoordinator::GetNextArticle(DownloadQueue* downloadQueue, FileInfo* &fileInfo, ArticleInfo* &articleInfo)
{
	// find an unpaused file with the highest priority, then take the next article from the file.
	// if the file doesn't have any articles left for download, we store that fact and search again,
	// ignoring all files which were previously marked as not having any articles.

	// special case: if the file has ExtraPriority-flag set, it has the highest priority.

	RawFileList checkedFiles;
	time_t curDate = Util::CurrentTime();

	for (;;)
	{
		fileInfo = nullptr;
		FileInfo* propagationWaitFile = nullptr;

		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			bool nzbHigherPriority = !fileInfo ||
				(nzbInfo->HasExtraPriority() == fileInfo->GetNzbInfo()->HasExtraPriority()
					? nzbInfo->GetPriority() > fileInfo->GetNzbInfo()->GetPriority()
					: nzbInfo->HasExtraPriority() > fileInfo->GetNzbInfo()->HasExtraPriority());

			bool nzbPaused = (int)nzbInfo->GetFileList()->size() == nzbInfo->GetPausedFileCount();

			if ((!fileInfo || nzbHigherPriority) && !nzbPaused &&
				!HasActiveDupe(downloadQueue, nzbInfo) &&
				((!g_WorkState->GetPauseDownload() && !g_WorkState->GetQuotaReached()) || nzbInfo->GetForcePriority()))
			{
				std::vector<FileInfo*> ptrs;
				std::vector<NzbgetRsCandidateFile> cands;
				ptrs.reserve(nzbInfo->GetFileList()->size() + 2);
				cands.reserve(nzbInfo->GetFileList()->size() + 2);
				if (fileInfo)
				{
					ptrs.push_back(fileInfo);
					cands.push_back(NzbgetRsCandidateFile{
						fileInfo->GetNzbInfo()->GetPriority(),
						fileInfo->GetExtraPriority() ? 1 : 0,
						0, 0, 0,
						(int64_t)fileInfo->GetTime()
					});
				}
				if (propagationWaitFile)
				{
					ptrs.push_back(propagationWaitFile);
					cands.push_back(NzbgetRsCandidateFile{
						propagationWaitFile->GetNzbInfo()->GetPriority(),
						propagationWaitFile->GetExtraPriority() ? 1 : 0,
						0, 0, 0,
						(int64_t)propagationWaitFile->GetTime()
					});
				}
				for (FileInfo* fileInfo1 : nzbInfo->GetFileList())
				{
					bool alreadyChecked = !checkedFiles.empty() &&
						std::find(checkedFiles.begin(), checkedFiles.end(), fileInfo1) != checkedFiles.end();
					ptrs.push_back(fileInfo1);
					cands.push_back(NzbgetRsCandidateFile{
						fileInfo1->GetNzbInfo()->GetPriority(),
						fileInfo1->GetExtraPriority() ? 1 : 0,
						fileInfo1->GetPaused() ? 1 : 0,
						fileInfo1->GetDeleted() ? 1 : 0,
						alreadyChecked ? 1 : 0,
						(int64_t)fileInfo1->GetTime()
					});
				}
				int bestIdx = -1;
				int propIdx = -1;
				nzbget_rs_qhp_select_next_file(
					cands.empty() ? nullptr : cands.data(),
					cands.size(),
					g_Options->GetPropagationDelay(),
					(int64_t)curDate,
					&bestIdx,
					&propIdx);
				if (bestIdx >= 0 && (size_t)bestIdx < ptrs.size())
				{
					fileInfo = ptrs[bestIdx];
				}
				if (propIdx >= 0 && (size_t)propIdx < ptrs.size())
				{
					propagationWaitFile = ptrs[propIdx];
				}
			}
		}

		if (!fileInfo)
		{
			// there are no more files for download
			if (propagationWaitFile && propagationWaitFile->GetId() != m_propagationWaitLoggedId)
			{
				// log once per idle period, otherwise the queue looks stuck without any explanation
				m_propagationWaitLoggedId = propagationWaitFile->GetId();
				int postedMin = 0, startMin = 0;
				nzbget_rs_queue_coord_calc_propagation_wait_log_mins(
					(int64)propagationWaitFile->GetTime(),
					g_Options->GetPropagationDelay(),
					(int64)curDate,
					&postedMin,
					&startMin);
				propagationWaitFile->GetNzbInfo()->PrintMessage(Message::mkDetail,
					"Holding %s%c%s due to PropagationDelay: posted %i min ago, download starts in %i min",
					propagationWaitFile->GetNzbInfo()->GetName(), PATH_SEPARATOR, propagationWaitFile->GetFilename(),
					postedMin, startMin);
			}
			break;
		}

		if (g_Options->GetDirectRename() &&
			fileInfo->GetNzbInfo()->GetDirectRenameStatus() <= NzbInfo::tsRunning &&
			!fileInfo->GetNzbInfo()->GetAllFirst() &&
			GetNextFirstArticle(fileInfo->GetNzbInfo(), fileInfo, articleInfo))
		{
			m_propagationWaitLoggedId = 0;
			return true;
		}

		if (fileInfo->GetArticles()->empty() && g_Options->GetServerMode())
		{
			g_DiskState->LoadArticles(fileInfo);
			LoadPartialState(fileInfo);
		}

		// check if the file has any articles left for download
		for (ArticleInfo* article : fileInfo->GetArticles())
		{
			if (article->GetStatus() == ArticleInfo::aiUndefined)
			{
				articleInfo = article;
				// once cut over, lead with the duplicate instead of spending a
				// full server sweep failing on the primary first
				if (fileInfo->GetDupeCutover() && article->GetDupeFallbackRound() == 0)
				{
					// marked before the try: the try counts only articles that failed
					article->SetDupeProactive(true);
					article->SetDupeProactive(
						m_dupeArticleFallback.TryFallback(downloadQueue, fileInfo, article));
				}
				m_propagationWaitLoggedId = 0;
				return true;
			}
		}

		// the file doesn't have any articles left for download
		checkedFiles.reserve(100);
		checkedFiles.push_back(fileInfo);
	}

	return false;
}

inline bool QueueCoordinator::IsPropagationWait(time_t postTime, int propagationDelay, time_t curDate)
{
	return nzbget_rs_qhp_is_propagation_wait((int64_t)postTime, propagationDelay, (int64_t)curDate) != 0;
}

inline bool QueueCoordinator::GetNextFirstArticle(NzbInfo* nzbInfo, FileInfo* &fileInfo, ArticleInfo* &articleInfo)
{
	// find a file not renamed yet
	for (FileInfo* fileInfo1 : nzbInfo->GetFileList())
	{
		if (!fileInfo1->GetFilenameConfirmed())
		{
			if (fileInfo1->GetArticles()->empty() && g_Options->GetServerMode())
			{
				g_DiskState->LoadArticles(fileInfo1);
				LoadPartialState(fileInfo1);
			}
			if (!fileInfo1->GetArticles()->empty() &&
				fileInfo1->GetArticles()->at(0)->GetStatus() == ArticleInfo::aiUndefined)
			{
				ArticleInfo* article = fileInfo1->GetArticles()->at(0).get();
				fileInfo = fileInfo1;
				articleInfo = article;
				nzbInfo->SetDirectRenameStatus(NzbInfo::tsRunning);
				return true;
			}
		}
	}

	// no more files for renaming remained
	nzbInfo->SetAllFirst(true);

	return false;
}

inline void QueueCoordinator::StartArticleDownload(FileInfo* fileInfo, ArticleInfo* articleInfo, NntpConnection* connection)
{
	debug("Starting new ArticleDownloader");

	ArticleDownloader* articleDownloader = new ArticleDownloader();
	articleDownloader->SetAutoDestroy(true);
	articleDownloader->Attach(this);
	articleDownloader->SetFileInfo(fileInfo);
	articleDownloader->SetArticleInfo(articleInfo);
	articleDownloader->SetConnection(connection);

	// the first article is analyzed for direct-rename, and for duplicate recovery
	// (which never writes into par2 data, see DupeArticleFallback::TryFallback)
	if (articleInfo->GetPartNumber() == 1 && !g_Options->GetRawArticle() &&
		(g_Options->GetDirectRename() || g_Options->GetDupeArticleFallback() != Options::dafNone))
	{
		articleDownloader->SetContentAnalyzer(m_directRenamer.MakeArticleContentAnalyzer());
	}

	articleDownloader->SetInfoName(BString<1024>("%s%c%s [%i/%i]",
		fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, fileInfo->GetFilename(),
		articleInfo->GetPartNumber(), (int)fileInfo->GetArticles()->size()));

	articleInfo->SetStatus(ArticleInfo::aiRunning);
	fileInfo->SetActiveDownloads(fileInfo->GetActiveDownloads() + 1);
	fileInfo->GetNzbInfo()->SetActiveDownloads(fileInfo->GetNzbInfo()->GetActiveDownloads() + 1);

	m_activeDownloads.push_back(articleDownloader);
	articleDownloader->Start();
}

inline void QueueCoordinator::Update(Subject* caller, void* aspect)
{
	if (caller == g_WorkState)
	{
		debug("Notification from WorkState received");
		WakeUp();
		return;
	}

	debug("Notification from ArticleDownloader received");

	ArticleDownloader* articleDownloader = static_cast<ArticleDownloader*>(caller);
	if (articleDownloader->GetStatus() == ArticleDownloader::adFinished ||
		articleDownloader->GetStatus() == ArticleDownloader::adFailed ||
		articleDownloader->GetStatus() == ArticleDownloader::adRetry ||
		articleDownloader->GetStatus() == ArticleDownloader::adFatalError)
	{
		ArticleCompleted(articleDownloader);
	}
}

inline void QueueCoordinator::ArticleCompleted(ArticleDownloader* articleDownloader)
{
	debug("Article downloaded");

	FileInfo* fileInfo = articleDownloader->GetFileInfo();
	bool completeFileParts = false;

	{
		NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
		ArticleInfo* articleInfo = articleDownloader->GetArticleInfo();
		bool retry = false;
		bool fileCompleted = false;

		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		// a substituted (donor) article may only count as success if its decoded
		// byte range exactly occupies the slot of the target article, i.e. abuts
		// the decoded boundaries of its already-finished neighbour articles. A
		// mis-placed article would leave a zero-filled gap and/or overwrite a
		// neighbour's bytes, so it is treated as a failed attempt instead
		// (advancing to the next duplicate source, if any)
		bool checkAligned = articleDownloader->GetStatus() == ArticleDownloader::adFinished &&
			articleInfo->GetDupeFallbackRound() > 0 &&
			articleDownloader->GetDecodedFileSize() > 0;
		bool misplaced = checkAligned && !DupeArticleFallback::SegmentAligned(fileInfo, articleInfo);
		if (misplaced)
		{
			DiscardArticleSegment(fileInfo, articleInfo);
			nzbInfo->PrintMessage(Message::mkDetail,
				"Discarding misaligned article %s [%i/%i] from duplicate",
				fileInfo->GetFilename(), articleInfo->GetPartNumber(),
				(int)fileInfo->GetArticles()->size());
		}

		if (articleDownloader->GetStatus() == ArticleDownloader::adFinished && !misplaced)
		{
			articleInfo->SetStatus(ArticleInfo::aiFinished);
			// one of its own arrived: the run of failures ends (a borrowed one says
			// nothing about its posting)
			if (articleInfo->GetDupeFallbackRound() == 0 ||
				(articleInfo->GetDupeOriginalMessageId() &&
					strcmp(articleInfo->GetMessageId(), articleInfo->GetDupeOriginalMessageId()) == 0))
			{
				nzbInfo->SetDupeFailedRun(0);
			}
			{
				int64 nextFileSuccessSize = fileInfo->GetSuccessSize();
				int64 nextNzbCurSuccessSize = nzbInfo->GetCurrentSuccessSize();
				int64 nextNzbParCurSuccessSize = nzbInfo->GetParCurrentSuccessSize();
				int nextFileSuccessArticles = fileInfo->GetSuccessArticles();
				int nextNzbCurSuccessArticles = nzbInfo->GetCurrentSuccessArticles();
				nzbget_rs_queue_coord_apply_success_article_counters(
					articleInfo->GetSize(),
					fileInfo->GetParFile() ? 1 : 0,
					nextFileSuccessSize,
					nextNzbCurSuccessSize,
					nextNzbParCurSuccessSize,
					nextFileSuccessArticles,
					nextNzbCurSuccessArticles,
					&nextFileSuccessSize,
					&nextNzbCurSuccessSize,
					&nextNzbParCurSuccessSize,
					&nextFileSuccessArticles,
					&nextNzbCurSuccessArticles);
				fileInfo->SetSuccessSize(nextFileSuccessSize);
				nzbInfo->SetCurrentSuccessSize(nextNzbCurSuccessSize);
				nzbInfo->SetParCurrentSuccessSize(nextNzbParCurSuccessSize);
				fileInfo->SetSuccessArticles(nextFileSuccessArticles);
				nzbInfo->SetCurrentSuccessArticles(nextNzbCurSuccessArticles);
			}
			int64 previousFileSize = fileInfo->GetDecodedFileSize();
			if (DupeArticleFallback::MergeDecodedFileSize(fileInfo, articleDownloader->GetDecodedFileSize()))
			{
				nzbInfo->PrintMessage(Message::mkDetail,
					"Articles of %s declare different file sizes (%" PRIi64 " vs %" PRIi64
					"); ignoring the declared size", fileInfo->GetFilename(), previousFileSize,
					articleDownloader->GetDecodedFileSize());
			}
			// the duplicate whose article filled this one, checked against the
			// file's par2 checksums once the file is complete (B40)
			{
				std::vector<int>& sourceDonors = *articleInfo->GetDupeSourceDonors();
				articleInfo->SetDupeDonorId(nzbget_rs_queue_coord_select_dupe_donor_id(
					articleInfo->GetDupeFallbackRound(),
					sourceDonors.empty() ? nullptr : sourceDonors.data(),
					sourceDonors.size(),
					articleInfo->GetMessageId(),
					articleInfo->GetDupeOriginalMessageId()));
			}
			// count only proven duplicate recoveries: the article was
			// substituted after genuinely failing on the primary (reactive)
			// and the source that succeeded was not the primary (revert)
			// message-id. Proactive (cutover) fetches never count - the
			// duplicate is tried FIRST then, so a donor success proves
			// nothing about the primary
			int nextFileRec = fileInfo->GetDupeRecoveredArticles();
			int nextNzbRec = nzbInfo->GetDupeRecoveredArticles();
			int triggerCutover = 0;
			if (nzbget_rs_queue_coord_eval_dupe_recovery_and_cutover(
				articleInfo->GetDupeFallbackRound(),
				articleInfo->GetDupeProactive() ? 1 : 0,
				articleInfo->GetMessageId(),
				articleInfo->GetDupeOriginalMessageId(),
				nextFileRec,
				nextNzbRec,
				fileInfo->GetDupeCutover() ? 1 : 0,
				DupeArticleFallback::CutoverThreshold,
				&nextFileRec,
				&nextNzbRec,
				&triggerCutover) != 0)
			{
				fileInfo->SetDupeRecoveredArticles(nextFileRec);
				nzbInfo->SetDupeRecoveredArticles(nextNzbRec);
				if (triggerCutover)
				{
					fileInfo->SetDupeCutover(true);
					nzbInfo->PrintMessage(Message::mkInfo,
						"Leading with duplicate collections for %s (primary is missing many articles)",
						fileInfo->GetFilename());
				}
			}

			// a successful fetch from the lead duplicate ends its miss streak
			// (no-op unless this was a lead-round fetch of the current lead)
			DupeArticleFallback::RegisterLeadSuccess(fileInfo, articleInfo);

			// this article pins decoded boundaries which may not have been known
			// when an adjacent donor-substituted article was accepted (e.g. it
			// finished before this one): re-validate such neighbours and demote
			// any that provably do not tile with the file's decoded geometry
			if (fileInfo->GetDupeRecoveredArticles() > 0 && articleDownloader->GetDecodedFileSize() > 0)
			{
				DemoteMisalignedDupeNeighbors(fileInfo, articleInfo);
			}

			if (!fileInfo->GetParFile() &&
				articleInfo->GetSegmentOffset() >= 0 &&
				articleInfo->GetSegmentSize() > 0 &&
				nzbget_rs_streaming_par_has_session(nzbInfo->GetId()) != 0)
			{
				std::vector<uint8_t> segSnapshot;
				if (g_ArticleCache)
				{
					Guard contentGuard = g_ArticleCache->GuardContent();
					if (!fileInfo->GetFlushLocked() && articleInfo->GetSegmentContent())
					{
						const auto* ptr = reinterpret_cast<const uint8_t*>(articleInfo->GetSegmentContent());
						size_t len = static_cast<size_t>(articleInfo->GetSegmentSize());
						segSnapshot.assign(ptr, ptr + len);
					}
				}
				nzbget_rs_streaming_par_on_article_finished_by_name(
					nzbInfo->GetId(),
					fileInfo->GetFilename(),
					static_cast<uint64_t>(articleInfo->GetSegmentOffset()),
					static_cast<uint64_t>(articleInfo->GetSegmentSize()),
					articleInfo->GetCrc(),
					segSnapshot.empty() ? nullptr : segSnapshot.data(),
					segSnapshot.size());
				uint64_t unpauseIds[32]{};
				while (size_t unpauseCount = nzbget_rs_streaming_par_take_unpause_ids(
					nzbInfo->GetId(), unpauseIds, 32))
				{
					for (size_t ui = 0; ui < unpauseCount; ++ui)
					{
						for (FileInfo* qFile : nzbInfo->GetFileList())
						{
							if (qFile && static_cast<uint64_t>(qFile->GetId()) == unpauseIds[ui] &&
								qFile->GetPaused() && !qFile->GetDeleted())
							{
								qFile->SetPaused(false);
								qFile->SetExtraPriority(true);
								break;
							}
						}
					}
				}
			}
		}
		else if ((articleDownloader->GetStatus() == ArticleDownloader::adFailed || misplaced) &&
			m_dupeArticleFallback.TryFallback(downloadQueue, fileInfo, articleInfo))
		{
			// the article is requeued with a message-id borrowed from a duplicate
			articleInfo->SetStatus(ArticleInfo::aiUndefined);
			retry = true;
			if (articleInfo->GetPartNumber() == 1)
			{
				nzbInfo->SetAllFirst(false);
			}
		}
		else if (articleDownloader->GetStatus() == ArticleDownloader::adFailed ||
			articleDownloader->GetStatus() == ArticleDownloader::adFatalError || misplaced)
		{
			articleInfo->SetStatus(ArticleInfo::aiFailed);
			// par2 volumes aside: a lost one says nothing about the data
			int nextFailedRun = nzbInfo->GetDupeFailedRun();
			int64 nextFileFailedSize = fileInfo->GetFailedSize();
			int64 nextNzbCurFailedSize = nzbInfo->GetCurrentFailedSize();
			int64 nextNzbParCurFailedSize = nzbInfo->GetParCurrentFailedSize();
			int nextFileFailedArticles = fileInfo->GetFailedArticles();
			int nextNzbCurFailedArticles = nzbInfo->GetCurrentFailedArticles();
			nzbget_rs_queue_coord_apply_failed_article_counters(
				articleInfo->GetPartNumber(),
				fileInfo->GetParFile() ? 1 : 0,
				articleInfo->GetSize(),
				nextFailedRun,
				nextFileFailedSize,
				nextNzbCurFailedSize,
				nextNzbParCurFailedSize,
				nextFileFailedArticles,
				nextNzbCurFailedArticles,
				&nextFailedRun,
				&nextFileFailedSize,
				&nextNzbCurFailedSize,
				&nextNzbParCurFailedSize,
				&nextFileFailedArticles,
				&nextNzbCurFailedArticles);
			nzbInfo->SetDupeFailedRun(nextFailedRun);
			fileInfo->SetFailedSize(nextFileFailedSize);
			nzbInfo->SetCurrentFailedSize(nextNzbCurFailedSize);
			nzbInfo->SetParCurrentFailedSize(nextNzbParCurFailedSize);
			fileInfo->SetFailedArticles(nextFileFailedArticles);
			nzbInfo->SetCurrentFailedArticles(nextNzbCurFailedArticles);

			if (!fileInfo->GetParFile() &&
				nzbget_rs_streaming_par_has_session(nzbInfo->GetId()) != 0 &&
				articleInfo->GetSegmentSize() > 0 &&
				articleInfo->GetSegmentOffset() >= 0)
			{
				uint64_t failOffset = static_cast<uint64_t>(articleInfo->GetSegmentOffset());
				uint64_t failSize = static_cast<uint64_t>(articleInfo->GetSegmentSize());
				nzbget_rs_streaming_par_on_article_failed_permanently_by_name(
					nzbInfo->GetId(),
					fileInfo->GetFilename(),
					failOffset,
					failSize);
				uint64_t unpauseIds[32]{};
				while (size_t unpauseCount = nzbget_rs_streaming_par_take_unpause_ids(
					nzbInfo->GetId(), unpauseIds, 32))
				{
					for (size_t ui = 0; ui < unpauseCount; ++ui)
					{
						for (FileInfo* qFile : nzbInfo->GetFileList())
						{
							if (qFile && static_cast<uint64_t>(qFile->GetId()) == unpauseIds[ui] &&
								qFile->GetPaused() && !qFile->GetDeleted())
							{
								qFile->SetPaused(false);
								qFile->SetExtraPriority(true);
								break;
							}
						}
					}
				}
			}
		}
		else if (articleDownloader->GetStatus() == ArticleDownloader::adRetry)
		{
			articleInfo->SetStatus(ArticleInfo::aiUndefined);
			retry = true;
			if (articleInfo->GetPartNumber() == 1)
			{
				nzbInfo->SetAllFirst(false);
			}
		}

		// Account every completed source attempt, including attempts that are
		// followed by a duplicate fallback retry.  The downloader owns one
		// per-attempt ServerStatList, so this is exactly-once for this attempt.
		// An interrupted attempt (stop, pause, server reconfiguration) is
		// downloaded again from scratch and accounted then, as before.
		if (articleDownloader->GetStatus() != ArticleDownloader::adRetry)
		{
			fileInfo->GetServerStats()->ListOp(articleDownloader->GetServerStats(), ServerStatList::soAdd);
			nzbInfo->GetCurrentServerStats()->ListOp(articleDownloader->GetServerStats(), ServerStatList::soAdd);
		}

		if (!retry)
		{
			int64 remFileSize = fileInfo->GetRemainingSize();
			int64 remNzbSize = nzbInfo->GetRemainingSize();
			int64 pausedSize = nzbInfo->GetPausedSize();
			int completedArticles = fileInfo->GetCompletedArticles();
			fileCompleted = nzbget_rs_queue_coord_advance_article_completion_counters(
				remFileSize,
				remNzbSize,
				pausedSize,
				articleInfo->GetSize(),
				fileInfo->GetPaused() ? 1 : 0,
				completedArticles,
				(int)fileInfo->GetArticles()->size(),
				&remFileSize,
				&remNzbSize,
				&pausedSize,
				&completedArticles) != 0;
			fileInfo->SetRemainingSize(remFileSize);
			nzbInfo->SetRemainingSize(remNzbSize);
			if (fileInfo->GetPaused())
			{
				nzbInfo->SetPausedSize(pausedSize);
			}
			fileInfo->SetCompletedArticles(completedArticles);
			fileInfo->SetPartialChanged(true);
		}

		if (!fileInfo->GetFilenameConfirmed() && !misplaced &&
			articleDownloader->GetStatus() == ArticleDownloader::adFinished &&
			articleDownloader->GetArticleFilename())
		{
			// in "FileNaming=auto"-mode prefer filename from nzb-file to filename read from article
			// if the name from article seems to be obfuscated
			bool useFilenameFromArticle = g_Options->GetFileNaming() == Options::nfArticle ||
				(g_Options->GetFileNaming() == Options::nfAuto &&
					nzbget_rs_is_excessively_obfuscated(
						articleDownloader->GetArticleFilename(),
						strlen(articleDownloader->GetArticleFilename())) == 0 &&
					!nzbInfo->GetManyDupeFiles());
			if (useFilenameFromArticle)
			{
				fileInfo->SetFilename(articleDownloader->GetArticleFilename());
				fileInfo->MakeValidFilename();
			}
			fileInfo->SetFilenameConfirmed(true);
			bool dupeFileExists = false;
			if (g_Options->GetDupeCheck() &&
				nzbInfo->GetDupeMode() != dmForce &&
				!nzbInfo->GetManyDupeFiles())
			{
				NzbgetRsBuf destPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					nzbInfo->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
				dupeFileExists = FileSystem::FileExists(destPathBuf.data ? destPathBuf.data : "");
				nzbget_rs_free(destPathBuf);
			}
			if (dupeFileExists)
			{
				warn("File \"%s\" seems to be duplicate, cancelling download and deleting file from queue", fileInfo->GetFilename());
				fileCompleted = false;
				fileInfo->SetDupeDeleted(true);
				DeleteQueueEntry(downloadQueue, fileInfo);
			}
		}

		if (articleDownloader->GetContentAnalyzer() && !misplaced &&
			articleDownloader->GetStatus() == ArticleDownloader::adFinished)
		{
			fileInfo->SetFirstContent(DirectRenamer::AnalyzedParFile(articleDownloader->GetContentAnalyzer()) ?
				FileInfo::fcPar2 : FileInfo::fcOther);
			if (g_Options->GetDirectRename())
			{
				m_directRenamer.ArticleDownloaded(downloadQueue, fileInfo, articleInfo, articleDownloader->GetContentAnalyzer());
			}
		}

		nzbInfo->SetDownloadedSize(nzbInfo->GetDownloadedSize() + articleDownloader->GetDownloadedSize());

		// the article that completes the file may be the one whose failure drops
		// health below critical: the cancel then deletes a file that is finished
		// but for its holes, which is assembled like any other (and its holes
		// queued for stream repair), not dropped unassembled
		bool finishedBeforeCancel = fileCompleted && !fileInfo->GetDeleted();

		CheckHealth(downloadQueue, fileInfo);
		CheckDeadDownload(downloadQueue, nzbInfo);

		int fcOut = fileCompleted ? 1 : 0;
		completeFileParts = nzbget_rs_queue_coord_eval_file_completion_and_assembly(
			fcOut,
			nzbInfo->GetParking() ? 1 : 0,
			fileInfo->GetActiveDownloads(),
			fileInfo->GetDupeDeleted() ? 1 : 0,
			fileInfo->GetDeleted() ? 1 : 0,
			finishedBeforeCancel ? 1 : 0,
			&fcOut) != 0;
		fileCompleted = fcOut != 0;

		// last line of defence before a would-be cfSuccess file is assembled and
		// classified: reject mis-placed donor bytes that the round-gated backstops
		// above cannot see after a restart (round/recovered state is not persisted)
		if (fileCompleted)
		{
			ValidateCompletedFileTiling(fileInfo);
		}


		if (!completeFileParts)
		{
			DeleteDownloader(&m_downloadQueue, articleDownloader, false);
		}
	}

	if (completeFileParts)
	{
		// all jobs done
		if (!articleDownloader->CompleteFileParts())
		{
			// Assembly/commit failures are local I/O failures. Classify this as
			// a completed failed file under the queue lock; a user deletion would
			// remove its damage from aggregate health and can yield false SUCCESS.
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
			int successArticles = fileInfo->GetSuccessArticles();
			int64 successSize = fileInfo->GetSuccessSize();
			int fileFailedArticles = fileInfo->GetFailedArticles();
			int64 fileFailedSize = fileInfo->GetFailedSize();
			int nzbCurFailedArticles = nzbInfo->GetCurrentFailedArticles();
			int nzbCurSuccessArticles = nzbInfo->GetCurrentSuccessArticles();
			int64 nzbCurFailedSize = nzbInfo->GetCurrentFailedSize();
			int64 nzbCurSuccessSize = nzbInfo->GetCurrentSuccessSize();
			int64 nzbParCurFailedSize = nzbInfo->GetParCurrentFailedSize();
			int64 nzbParCurSuccessSize = nzbInfo->GetParCurrentSuccessSize();
			nzbget_rs_queue_coord_apply_assembly_failure_counters(
				successArticles,
				successSize,
				fileInfo->GetParFile() ? 1 : 0,
				&fileFailedArticles,
				&fileFailedSize,
				&nzbCurFailedArticles,
				&nzbCurSuccessArticles,
				&nzbCurFailedSize,
				&nzbCurSuccessSize,
				&nzbParCurFailedSize,
				&nzbParCurSuccessSize);
			fileInfo->SetFailedArticles(fileFailedArticles);
			fileInfo->SetSuccessArticles(0);
			fileInfo->SetFailedSize(fileFailedSize);
			fileInfo->SetSuccessSize(0);
			nzbInfo->SetCurrentFailedArticles(nzbCurFailedArticles);
			nzbInfo->SetCurrentSuccessArticles(nzbCurSuccessArticles);
			nzbInfo->SetCurrentFailedSize(nzbCurFailedSize);
			nzbInfo->SetCurrentSuccessSize(nzbCurSuccessSize);
			if (fileInfo->GetParFile())
			{
				nzbInfo->SetParCurrentFailedSize(nzbParCurFailedSize);
				nzbInfo->SetParCurrentSuccessSize(nzbParCurSuccessSize);
			}
			for (ArticleInfo* article : fileInfo->GetArticles())
			{
				if (article->GetStatus() == ArticleInfo::aiFinished)
				{
					article->SetStatus(ArticleInfo::aiFailed);
				}
			}
			DiscardTempFiles(fileInfo);
			DeleteDownloader(downloadQueue, articleDownloader, true);
			return;
		}
		fileInfo->SetPartialChanged(false);

		// the borrowed articles must hold the right bytes: checked against the par2
		// block checksums of the file (B40), outside the lock - it reads the disk.
		// The names are copied under the lock first: a rename (DirectRename, an
		// edit of the nzb) replaces them meanwhile
		std::string path, destDir, filename;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			path = fileInfo->GetOutputFilename();
			destDir = fileInfo->GetNzbInfo()->GetDestDir();
			filename = fileInfo->GetFilename() ? fileInfo->GetFilename() : "";
		}
		if (path.empty() || !FileSystem::FileExists(path.c_str()))
		{
			NzbgetRsBuf destPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				destDir.c_str(), PATH_SEPARATOR, filename.c_str());
			path = destPathBuf.data ? destPathBuf.data : "";
			nzbget_rs_free(destPathBuf);
		}
		std::vector<ArticleInfo*> wrong = g_Options->GetDupeArticleFallback() != Options::dafNone ?
			DupeArticleFallback::BorrowedPar2Mismatches(fileInfo, path.c_str(), destDir.c_str(), filename.c_str()) :
			std::vector<ArticleInfo*>();

		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (!wrong.empty())
		{
			RejectBorrowedArticles(fileInfo, wrong);
		}
		DeleteDownloader(downloadQueue, articleDownloader, true);
	}
}

/*
 * Borrowed articles of a completed file that failed its par2 checksums (B40): they
 * count as failed (so par-check repairs them - with no failed article it might not
 * even look), and their duplicates lend this download no more articles: a duplicate
 * that passed the structural pairing but holds other bytes is a wrong pairing.
 */
inline void QueueCoordinator::RejectBorrowedArticles(FileInfo* fileInfo, const std::vector<ArticleInfo*>& articles)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	std::vector<int32_t> rawDonorIds;
	rawDonorIds.reserve(articles.size());
	for (ArticleInfo* article : articles)
	{
		rawDonorIds.push_back(article->GetDupeDonorId());
		if (article->GetStatus() == ArticleInfo::aiFinished)
		{
			// (not recovered any more: own articles are counted right again, review item 4)
			CountAsFailed(fileInfo, article);
		}
	}
	std::vector<int32_t> uniqueDonors(rawDonorIds.size(), 0);
	size_t uniqueLen = 0;
	NzbgetRsBuf namesBuf = nzbget_rs_queue_coord_format_donor_id_list(
		rawDonorIds.empty() ? nullptr : rawDonorIds.data(),
		rawDonorIds.size(),
		uniqueDonors.empty() ? nullptr : uniqueDonors.data(),
		&uniqueLen);
	for (size_t i = 0; i < uniqueLen; ++i)
	{
		nzbInfo->GetDupeBlockedDonors()->insert(uniqueDonors[i]);
	}
	nzbInfo->PrintMessage(Message::mkWarning,
		"%i article(s) of %s borrowed from duplicates don't match its par2 checksums: counted as "
		"failed, and duplicate(s) %s lend this download no more articles",
		(int)articles.size(), fileInfo->GetFilename(), namesBuf.data ? namesBuf.data : "");
	nzbget_rs_free(namesBuf);
}

inline void QueueCoordinator::DeleteDownloader(DownloadQueue* downloadQueue,
	ArticleDownloader* articleDownloader, bool fileCompleted)
{
	FileInfo* fileInfo = articleDownloader->GetFileInfo();
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	bool deleteFileObj = fileCompleted || (fileInfo->GetDeleted() && fileInfo->GetActiveDownloads() == 1);

	// remove downloader from downloader list
	m_activeDownloads.erase(std::find(m_activeDownloads.begin(), m_activeDownloads.end(), articleDownloader));

	fileInfo->SetActiveDownloads(fileInfo->GetActiveDownloads() - 1);
	nzbInfo->SetActiveDownloads(nzbInfo->GetActiveDownloads() - 1);

	if (deleteFileObj)
	{
		int nzbId = nzbInfo->GetId();
		DeleteFileInfo(downloadQueue, fileInfo, fileCompleted);
		if (NzbInfo* nzb = downloadQueue->GetQueue()->Find(nzbId))
		{
			nzb->SetChanged(true);
			downloadQueue->SaveChanged();
		}
	}
	else if (g_PrePostProcessor && nzbInfo->GetActiveDownloads() == 0)
	{
		// A paused PAR volume can finish its last in-flight request without
		// finishing the file. It may be the collection's last active request.
		g_PrePostProcessor->CheckDownloadCompleted(downloadQueue, nzbInfo);
	}
}

inline void QueueCoordinator::DeleteFileInfo(DownloadQueue* downloadQueue, FileInfo* fileInfo, bool completed)
{
	while (g_ArticleCache->FileBusy(fileInfo))
	{
		Util::Sleep(5);
	}

	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	bool parking = fileInfo->GetNzbInfo()->GetParking();
	bool fileDeleted = fileInfo->GetDeleted();

	fileInfo->SetDeleted(true);

	if (completed && fileInfo->GetDupeAttemptedArticles() > 0)
	{
		// honest per-file summary: recovered vs how many were tried from
		// duplicates (0 recovered means the duplicates lacked those articles too)
		nzbInfo->PrintMessage(Message::mkInfo,
			"Recovered %i of %i missing article(s) of %s from duplicate collections",
			fileInfo->GetDupeRecoveredArticles(), fileInfo->GetDupeAttemptedArticles(), fileInfo->GetFilename());
	}

	if (completed || nzbInfo->GetDeleting())
	{
		nzbInfo->UpdateCompletedStats(fileInfo);
	}
	else
	{
		nzbInfo->UpdateDeletedStats(fileInfo);
	}

	CompletedFile::EStatus fileStatus = (CompletedFile::EStatus)nzbget_rs_qhp_classify_completed_file_status(
		fileInfo->GetTotalArticles(), fileInfo->GetSuccessArticles(),
		fileInfo->GetMissedArticles(), fileInfo->GetFailedArticles());

	if (g_Options->GetServerMode())
	{
		int deleteData = 0, savePartial = 0;
		nzbget_rs_queue_coord_eval_delete_file_diskstate_flags(
			(int)fileStatus,
			(int)CompletedFile::cfSuccess,
			(int)CompletedFile::cfPartial,
			fileDeleted ? 1 : 0,
			parking ? 1 : 0,
			completed ? 1 : 0,
			&deleteData,
			&savePartial);
		g_DiskState->DiscardFile(fileInfo->GetId(), deleteData != 0, true, false);
		if (savePartial != 0)
		{
			g_DiskState->SaveFileState(fileInfo, true);
		}
	}

	if (!completed)
	{
		DiscardTempFiles(fileInfo);
	}

	if (completed || parking)
	{
		const std::string& outputFilename = fileInfo->GetOutputFilename();
		bool outputFileExists = (completed && fileStatus == CompletedFile::cfFailure && !Util::EmptyStr(fileInfo->GetFilename()))
			? FileSystem::FileExists(outputFilename.c_str())
			: true;
		NzbgetRsBuf fnBuf = nzbget_rs_qhp_resolve_completed_filename(
			completed ? 1 : 0, (int)fileStatus, outputFilename.c_str(),
			fileInfo->GetFilename() ? fileInfo->GetFilename() : "",
			fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR,
			outputFileExists ? 1 : 0);
		std::string filename(fnBuf.data ? fnBuf.data : "");
		nzbget_rs_free(fnBuf);

		// the par2 blocks its failed articles spoil (CheckParDamage)
		if (completed)
		{
			ParDamage::FileCompleted(nzbInfo, fileInfo, filename);
		}

		// capture the missing byte ranges of an incomplete file for
		// post-processing stream repair while the article list still exists
		// (see option <DupeArticleFallback> value "stream"). A file none of
		// whose articles arrived is captured whole: a duplicate proven on the
		// collection's other files may still carry it
		bool isIncomplete = completed &&
			(fileStatus == CompletedFile::cfPartial || fileStatus == CompletedFile::cfFailure);
		bool isSide = isIncomplete && SideFile(filename.c_str());
		if (isSide)
		{
			// an .nfo or .sfv is worth no repair from a duplicate: par2 rarely covers
			// it, and repairing it held up a download whose data was whole (B53)
			nzbInfo->PrintMessage(Message::mkDetail, "Not repairing side file %s from duplicates", filename.c_str());
		}
		if (isIncomplete && !isSide &&
			DupeStreamRepair::BuildRepairJob(fileInfo, filename.c_str()))
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				fileStatus == CompletedFile::cfFailure
					? "Queueing stream repair of %s (no article available) from duplicate collections"
					: "Queueing stream repair of %s from duplicate collections",
				filename.c_str());

			// live mode: repair this file's holes now, while the rest of the
			// collection still downloads. Only when other files remain queued -
			// for the collection's last file the post-processing stage starts
			// moments later anyway and remains the accounting authority either
			// way. One live pass per collection at a time; jobs captured while
			// one runs are picked up by the next dispatch.
			bool moreFilesQueued = false;
			for (FileInfo* queuedFile : nzbInfo->GetFileList())
			{
				if (queuedFile != fileInfo && !queuedFile->GetDeleted())
				{
					moreFilesQueued = true;
					break;
				}
			}
			if (g_Options->GetDupeArticleFallback() == Options::dafLive &&
				moreFilesQueued && !nzbInfo->GetLiveRepairThread())
			{
				StreamRepairController::StartLive(nzbInfo);
			}
		}

		uint32 fileCrc = fileStatus == CompletedFile::cfSuccess ? fileInfo->GetCrc() : 0;
		fileInfo->GetNzbInfo()->GetCompletedFiles()->emplace_back(
			fileInfo->GetId(),
			std::move(filename),
			fileInfo->GetOrigname() ? fileInfo->GetOrigname() : "", 
			fileStatus,
			fileCrc,
			fileInfo->GetParFile(), 
			fileInfo->GetHash16k() ? fileInfo->GetHash16k() : "", 
			fileInfo->GetParSetId() ? fileInfo->GetParSetId() : ""
		);
	}

	if (g_Options->GetDirectRename())
	{
		m_directRenamer.FileDownloaded(downloadQueue, fileInfo);
	}

	if (nzbInfo->GetDirectRenameStatus() == NzbInfo::tsRunning &&
		!nzbInfo->GetDeleting() && nzbInfo->IsDownloadCompleted(true))
	{
		DiscardDirectRename(downloadQueue, nzbInfo);
	}

	std::unique_ptr<FileInfo> srcFileInfo = nzbInfo->GetFileList()->Remove(fileInfo);

	DownloadQueue::EAspectAction removedAction = (completed && !fileDeleted)
		? DownloadQueue::eaFileCompleted
		: DownloadQueue::eaFileDeleted;
	DownloadQueue::Aspect aspect = { removedAction, downloadQueue, nzbInfo, fileInfo };
	downloadQueue->Notify(&aspect);

	// now can destroy FileInfo
	srcFileInfo.reset();
}

/*
 * A donor-substituted article can finish before its neighbour articles; its
 * decoded boundaries are then not fully verifiable and it is accepted
 * provisionally. Each newly finished article pins more of the file's decoded
 * geometry: re-validate the adjacent donor-substituted articles against it and
 * demote any that provably do not tile (their bytes would leave a zero-filled
 * gap and/or overwrite a neighbour's decoded bytes). The demoted article
 * counts as failed, so the file cannot complete as an intact download around
 * mis-placed donor bytes - par2 or stream repair take over instead.
 * Must be called within DownloadQueue-lock.
 */
inline void QueueCoordinator::DemoteMisalignedDupeNeighbors(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	ArticleList* articles = fileInfo->GetArticles();

	int index = -1;
	for (size_t i = 0; i < articles->size(); i++)
	{
		if ((*articles)[i].get() == articleInfo)
		{
			index = (int)i;
			break;
		}
	}
	if (index < 0)
	{
		return;
	}

	for (int neighborIndex : {index - 1, index + 1})
	{
		if (neighborIndex < 0 || neighborIndex >= (int)articles->size())
		{
			continue;
		}

		ArticleInfo* neighbor = (*articles)[neighborIndex].get();
		if (neighbor->GetStatus() != ArticleInfo::aiFinished ||
			neighbor->GetDupeFallbackRound() <= 0 ||
			neighbor->GetSegmentSize() <= 0 ||
			DupeArticleFallback::SegmentAligned(fileInfo, neighbor))
		{
			continue;
		}

		DemoteFinishedArticle(fileInfo, neighbor);

		fileInfo->GetNzbInfo()->PrintMessage(Message::mkDetail,
			"Discarding misaligned article %s [%i/%i] from duplicate",
			fileInfo->GetFilename(), neighbor->GetPartNumber(), (int)articles->size());
	}
}

/*
 * Final whole-file guard, run when a file finishes with every article marked
 * successful and would therefore be classified cfSuccess. Independent of the
 * (non-persisted) fallback round and recovered-count, so it also catches a
 * provisionally-accepted drifted donor article that survived a daemon restart:
 * the restart reloads it as a plain finished article (round reset to 0), which
 * the per-article and neighbour backstops no longer recognise, but its
 * persisted decoded offset/size still betray the gap/overlap here. A healthy
 * file's yEnc parts tile [0, DecodedFileSize) exactly by construction, so this
 * only fires on genuinely mis-placed bytes; the offending article is demoted
 * to failed so the file classifies cfPartial and par2/stream repair take over.
 * Must be called within DownloadQueue-lock.
 */
inline void QueueCoordinator::ValidateCompletedFileTiling(FileInfo* fileInfo)
{
	// only a would-be cfSuccess file is at risk of silently standing on
	// mis-placed bytes; a file with any failed/missed article already falls to
	// par2/stream repair
	if (fileInfo->GetSuccessArticles() != fileInfo->GetTotalArticles())
	{
		return;
	}

	// only donor bytes can be mis-placed: a file assembled purely from its own
	// postings is written exactly as before this feature existed, even when the
	// poster's yEnc geometry is unusual (e.g. obfuscated "=ybegin size=" values)
	if (!DupeArticleFallback::HasDonorArticles(fileInfo))
	{
		return;
	}

	ArticleInfo* untiled = DupeArticleFallback::FirstUntiledArticle(fileInfo);
	if (!untiled)
	{
		return;
	}

	DemoteFinishedArticle(fileInfo, untiled);

	fileInfo->GetNzbInfo()->PrintMessage(Message::mkWarning,
		"Discarding misaligned article %s [%i/%i]: decoded bytes do not tile the "
		"file (leaving to par-repair)",
		fileInfo->GetFilename(), untiled->GetPartNumber(), (int)fileInfo->GetArticles()->size());
}

/*
 * Demotes an article that was counted successful back to failed, moving all
 * per-file and per-nzb success/failed counters (incl. the par-set variants)
 * and undoing its duplicate-recovered credit if it held one. The decoded bytes
 * it contributed are dropped; the file can then no longer classify cfSuccess.
 * Must be called within DownloadQueue-lock.
 */
/*
 * A finished article turns out wrong: it counts as failed, and no longer as
 * recovered from a duplicate if it was one - symmetric with the increment in
 * ArticleCompleted, which excludes proactive (cutover) fetches: without the
 * same exclusion here a demoted proactive article would decrement a count it
 * never incremented. (The recovered part is a no-op after a restart, where the
 * round/original-id state is not persisted and has already reset.)
 * Must be called within DownloadQueue-lock.
 */
inline void QueueCoordinator::CountAsFailed(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	articleInfo->SetStatus(ArticleInfo::aiFailed);

	int64 fileSuccessSize = fileInfo->GetSuccessSize();
	int64 fileFailedSize = fileInfo->GetFailedSize();
	int fileSuccessArticles = fileInfo->GetSuccessArticles();
	int fileFailedArticles = fileInfo->GetFailedArticles();
	int64 nzbCurSuccessSize = nzbInfo->GetCurrentSuccessSize();
	int64 nzbCurFailedSize = nzbInfo->GetCurrentFailedSize();
	int64 nzbParCurSuccessSize = nzbInfo->GetParCurrentSuccessSize();
	int64 nzbParCurFailedSize = nzbInfo->GetParCurrentFailedSize();
	int nzbCurSuccessArticles = nzbInfo->GetCurrentSuccessArticles();
	int nzbCurFailedArticles = nzbInfo->GetCurrentFailedArticles();
	int fileDupeRecovered = fileInfo->GetDupeRecoveredArticles();
	int nzbDupeRecovered = nzbInfo->GetDupeRecoveredArticles();
	nzbget_rs_queue_coord_demote_article_counters(
		articleInfo->GetSize(),
		fileInfo->GetParFile() ? 1 : 0,
		articleInfo->GetDupeProactive() ? 1 : 0,
		articleInfo->GetMessageId(),
		articleInfo->GetDupeOriginalMessageId(),
		&fileSuccessSize,
		&fileFailedSize,
		&fileSuccessArticles,
		&fileFailedArticles,
		&nzbCurSuccessSize,
		&nzbCurFailedSize,
		&nzbParCurSuccessSize,
		&nzbParCurFailedSize,
		&nzbCurSuccessArticles,
		&nzbCurFailedArticles,
		&fileDupeRecovered,
		&nzbDupeRecovered);
	fileInfo->SetSuccessSize(fileSuccessSize);
	fileInfo->SetFailedSize(fileFailedSize);
	fileInfo->SetSuccessArticles(fileSuccessArticles);
	fileInfo->SetFailedArticles(fileFailedArticles);
	nzbInfo->SetCurrentSuccessSize(nzbCurSuccessSize);
	nzbInfo->SetCurrentFailedSize(nzbCurFailedSize);
	nzbInfo->SetParCurrentSuccessSize(nzbParCurSuccessSize);
	nzbInfo->SetParCurrentFailedSize(nzbParCurFailedSize);
	nzbInfo->SetCurrentSuccessArticles(nzbCurSuccessArticles);
	nzbInfo->SetCurrentFailedArticles(nzbCurFailedArticles);
	fileInfo->SetDupeRecoveredArticles(fileDupeRecovered);
	nzbInfo->SetDupeRecoveredArticles(nzbDupeRecovered);
}

inline void QueueCoordinator::DemoteFinishedArticle(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();

	DiscardArticleSegment(fileInfo, articleInfo);
	CountAsFailed(fileInfo, articleInfo);

	// a lead-donor article accepted provisionally (its neighbours were still
	// in flight) reset the lead-miss streak as a success; demoting it proves
	// that fetch was in truth a lead miss, so charge it as one - otherwise a
	// systematically drifted lead could keep resetting its own streak and
	// never be rotated away (round-gated: a no-op for non-lead articles)
	if (DupeArticleFallback::RegisterLeadFailure(fileInfo, articleInfo))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Switching lead duplicate collection for %s (the lead duplicate is missing many articles)",
			fileInfo->GetFilename());
	}

	fileInfo->SetPartialChanged(true);
}

/*
 * Drops the cached decoded bytes of a rejected article. If the article cache
 * is currently flushing this file's segments the flusher owns the segment
 * lifetime (see ArticleWriter::FlushCache) and the segment is left to it.
 * Must be called within DownloadQueue-lock.
 */
inline void QueueCoordinator::DiscardArticleSegment(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	Guard contentGuard = g_ArticleCache->GuardContent();
	if (articleInfo->GetDupeFallbackRound() > 0 && articleInfo->GetResultFilename())
	{
		FileSystem::DeleteFile(articleInfo->GetResultFilename());
	}
	if (!articleInfo->GetSegmentContent() || fileInfo->GetFlushLocked())
	{
		return;
	}
	articleInfo->DiscardSegment();
	fileInfo->SetCachedArticles(fileInfo->GetCachedArticles() - 1);
}

inline void QueueCoordinator::DiscardTempFiles(FileInfo* fileInfo)
{
	for (ArticleInfo* pa : fileInfo->GetArticles())
	{
		if (pa->GetResultFilename() &&
			((!g_Options->GetDirectWrite() && !fileInfo->GetForceDirectWrite()) ||
			pa->GetDupeFallbackRound() > 0))
		{
			FileSystem::DeleteFile(pa->GetResultFilename());
		}
	}

	const std::string& outputFilename = fileInfo->GetOutputFilename();
	if (g_Options->GetDirectWrite() && !outputFilename.empty() && !fileInfo->GetForceDirectWrite())
	{
		FileSystem::DeleteFile(outputFilename.c_str());
		if (fileInfo->IsHardLinked())
		{
			FileSystem::DeleteFile(fileInfo->GetHardLinkPath().c_str());
		}
	}
}

inline void QueueCoordinator::SaveQueueIfChanged()
{
	if (!g_Options->GetServerMode())
	{
		return;
	}

	bool hasChanges = false;
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		hasChanges |= nzbInfo->GetChanged();
	}

	if (hasChanges)
	{
		downloadQueue->Save();
	}
}

inline void QueueCoordinator::SaveAllPartialState()
{
	if (!g_Options->GetServerMode() || !g_Options->GetContinuePartial())
	{
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			SavePartialState(fileInfo);
		}
	}

	downloadQueue->SaveChanged();
}

inline void QueueCoordinator::SavePartialState(FileInfo* fileInfo)
{
	if (fileInfo->GetPartialChanged())
	{
		debug("Saving partial state for %s", fileInfo->GetFilename());
		if (fileInfo->GetPartialState() == FileInfo::psCompleted)
		{
			g_DiskState->DiscardFile(fileInfo->GetId(), false, false, true);
		}
		g_DiskState->SaveFileState(fileInfo, false);
		fileInfo->SetPartialChanged(false);
		fileInfo->SetPartialState(FileInfo::psPartial);
	}
}

inline void QueueCoordinator::LoadPartialState(FileInfo* fileInfo)
{
	if (fileInfo->GetPartialState() == FileInfo::psPartial)
	{
		g_DiskState->LoadFileState(fileInfo, g_ServerPool->GetServers(), false);
		if ((g_Options->GetDirectWrite() || fileInfo->GetForceDirectWrite()) &&
			!fileInfo->GetOutputInitialized())
		{
			NzbgetRsBuf outTmpBuf = nzbget_rs_dqh_diskstate_format_out_tmp_path(
				fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetId());
			BString<1024> outTmpFilename = outTmpBuf.data ? outTmpBuf.data : "";
			nzbget_rs_free(outTmpBuf);
			if (FileSystem::FileExists(outTmpFilename))
			{
				fileInfo->SetOutputFilename(outTmpFilename);
				fileInfo->SetOutputInitialized(true);
			}
			else if (fileInfo->GetForceDirectWrite())
			{
				NzbgetRsBuf outBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
				BString<1024> outputFilename = outBuf.data ? outBuf.data : "";
				nzbget_rs_free(outBuf);
				if (FileSystem::FileExists(outputFilename))
				{
					fileInfo->SetOutputFilename(outputFilename);
					fileInfo->SetOutputInitialized(true);
				}
			}
		}
	}
	else if (fileInfo->GetPartialState() == FileInfo::psCompleted)
	{
		g_DiskState->LoadFileState(fileInfo, g_ServerPool->GetServers(), true);

		NzbgetRsBuf outBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
		fileInfo->SetOutputFilename(outBuf.data ? outBuf.data : "");
		nzbget_rs_free(outBuf);

		fileInfo->SetOutputInitialized(true);
		fileInfo->SetForceDirectWrite(true);
		fileInfo->SetFilenameConfirmed(true);
	}
}

inline void QueueCoordinator::SaveAllFileState()
{
	if (g_Options->GetServerMode() && m_downloadQueue.m_stateChanged)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		g_DiskState->SaveAllFileInfos(downloadQueue);
	}
}

inline void QueueCoordinator::CheckHealth(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	int health = fileInfo->GetNzbInfo()->CalcHealth();
	int critHealth = (g_Options->GetHealthCheck() == Options::hcNone ||
		fileInfo->GetNzbInfo()->GetHealthPaused() ||
		fileInfo->GetNzbInfo()->GetDeleteStatus() == NzbInfo::dsHealth)
		? 0
		: DownloadCriticalHealth(downloadQueue, fileInfo->GetNzbInfo());
	if (nzbget_rs_queue_coord_should_skip_health_check(
		(int)g_Options->GetHealthCheck(), (int)Options::hcNone, (int)Options::hcPark,
		(int)g_Options->GetParScan(), (int)Options::psDupe,
		fileInfo->GetNzbInfo()->GetHealthPaused() ? 1 : 0,
		(int)fileInfo->GetNzbInfo()->GetDeleteStatus(), (int)NzbInfo::dsHealth,
		health, critHealth,
		fileInfo->GetNzbInfo()->GetSuccessArticles(),
		fileInfo->GetNzbInfo()->GetTotalArticles()) != 0)
	{
		return;
	}

	if (g_Options->GetHealthCheck() == Options::hcPause)
	{
		warn("Pausing %s due to health %.1f%% below critical %.1f%%", fileInfo->GetNzbInfo()->GetName(),
			fileInfo->GetNzbInfo()->CalcHealth() / 10.0,
			fileInfo->GetNzbInfo()->CalcCriticalHealth(true) / 10.0);
		fileInfo->GetNzbInfo()->SetHealthPaused(true);
		downloadQueue->EditEntry(fileInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupPause, nullptr);
	}
	else if (g_Options->GetHealthCheck() == Options::hcDelete ||
		g_Options->GetHealthCheck() == Options::hcPark)
	{
		fileInfo->GetNzbInfo()->PrintMessage(Message::mkWarning,
			"Cancelling download and deleting %s due to health %.1f%% below critical %.1f%%",
			fileInfo->GetNzbInfo()->GetName(),
			fileInfo->GetNzbInfo()->CalcHealth() / 10.0,
			fileInfo->GetNzbInfo()->CalcCriticalHealth(true) / 10.0);
		fileInfo->GetNzbInfo()->SetDeleteStatus(NzbInfo::dsHealth);
		downloadQueue->EditEntry(fileInfo->GetNzbInfo()->GetId(),
			g_Options->GetHealthCheck() == Options::hcPark
				? DownloadQueue::eaGroupParkDelete
				: DownloadQueue::eaGroupDelete,
			nullptr);
	}
	else if (g_Options->GetHealthCheck() == Options::hcDupe)
	{
		CheckDupeFailover(downloadQueue, fileInfo->GetNzbInfo());
	}
}

/*
 * The critical health of the download-time checks under HealthCheck=dupe. Seeing
 * no par2-files, nzbget assumes 85% (the files may be renamed par2-files). A
 * download that certainly has none - every file named as data - and no duplicate
 * holding a file of its size that stream repair could fill the holes from can't
 * lose an article: 100% (Velvet Underground 7913, a single .mp4, 52% of its
 * articles failing, kept downloading at health 97.5%; TEPES 7909 failed over
 * only at 84.9%, after 2.4 GB and 3,886 failed articles).
 */
inline int QueueCoordinator::DownloadCriticalHealth(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	int critical = nzbInfo->CalcCriticalHealth(true);
	int criticalExact = nzbInfo->CalcCriticalHealth(false);
	if (g_Options->GetHealthCheck() == Options::hcDupe && criticalExact == 1000 &&
		CertainlyParless(nzbInfo) && !HasSizeTwin(downloadQueue, nzbInfo))
	{
		return 1000;
	}
	return critical;
}

inline bool QueueCoordinator::CertainlyParless(NzbInfo* nzbInfo)
{
	// a renamed par2-file loses its extension: only names that are data count
	auto dataName = [](const char* filename)
		{
			return nzbget_rs_qhp_is_data_filename(filename) != 0;
		};

	if (DupeArticleFallback::HasPar2(nzbInfo))
	{
		return false;
	}
	int files = 0;
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (!dataName(fileInfo->GetFilename()))
		{
			return false;
		}
		files++;
	}
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (!dataName(completedFile.GetFilename()))
		{
			return false;
		}
		files++;
	}
	return files > 0;
}

inline bool QueueCoordinator::HasSizeTwin(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	int64 largest = 0;
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		largest = std::max(largest, fileInfo->GetSize());
	}
	if (largest <= 0)
	{
		return false;
	}

	// a duplicate with a file of that size, or of the same size in all (a
	// download in history has no file list): within 0.3%, nzb sizes are encoded.
	// Its par2-files don't count: the same archives reposted with other par2
	// volumes are still a twin of this par-less download
	auto twin = [nzbInfo, largest](NzbInfo* other)
		{
			bool sameNameOrKey = other != nzbInfo &&
				DupeCoordinator::SameNameOrKey(other->GetName(), other->GetDupeKey(),
					nzbInfo->GetName(), nzbInfo->GetDupeKey());
			std::vector<int64_t> otherFileSizes;
			if (sameNameOrKey)
			{
				otherFileSizes.reserve(other->GetFileList()->size());
				for (FileInfo* fileInfo : other->GetFileList())
				{
					otherFileSizes.push_back(fileInfo->GetSize());
				}
			}
			return nzbget_rs_queue_coord_is_size_twin(
				other == nzbInfo ? 1 : 0,
				sameNameOrKey ? 1 : 0,
				nzbInfo->GetSize(),
				other->GetSize(),
				other->GetParSize(),
				largest,
				otherFileSizes.empty() ? nullptr : otherFileSizes.data(),
				otherFileSizes.size()) != 0;
		};
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (twin(queued))
		{
			return true;
		}
	}
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb && twin(historyInfo->GetNzbInfo()))
		{
			return true;
		}
	}
	return false;
}

/*
 * Checks whether another job in the queue sharing the same DupeKey is currently
 * active (actively downloading, in post-processing, or direct unpacking).
 * Called under GuardedDownloadQueue lock.
 */
inline bool QueueCoordinator::HasActiveDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) const
{
	const char* dupeKey = nzbInfo->GetDupeKey();
	if (Util::EmptyStr(dupeKey))
	{
		return false;
	}

	// An active post-processing/repair job (or extra-priority par fetch) already owns
	// the active slot and must not be blocked from downloading its own repair volumes.
	PostInfo* selfPi = nzbInfo->GetPostInfo();
	if (nzbInfo->HasExtraPriority() ||
		(selfPi && (selfPi->GetWorking() || selfPi->GetStage() != PostInfo::ptQueued)))
	{
		return false;
	}

	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued != nzbInfo &&
			!Util::EmptyStr(queued->GetDupeKey()) &&
			nzbget_rs_dqh_bytes_eq_ignore_case(queued->GetDupeKey(), dupeKey) != 0)
		{
			PostInfo* qpi = queued->GetPostInfo();
			if (nzbget_rs_queue_coord_is_queued_dupe_active(
				queued->GetDeleting() ? 1 : 0,
				(int)queued->GetDeleteStatus(),
				(int)NzbInfo::dsNone,
				queued->GetActiveDownloads(),
				qpi != nullptr ? 1 : 0,
				(qpi && qpi->GetWorking()) ? 1 : 0,
				qpi ? (int)qpi->GetStage() : 0,
				(int)PostInfo::ptQueued,
				queued->HasExtraPriority() ? 1 : 0,
				nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
				(int)queued->GetDirectUnpackStatus(),
				(int)NzbInfo::nsRunning) != 0)
			{
				return true;
			}
		}
	}

	return false;
}

/*
 * Option <HealthCheck> value "dupe": a download whose health fell below
 * critical is abandoned (parked, so its files stay available as a stream
 * repair donor) as soon as a better duplicate waits in history, instead of
 * failing every remaining article first and only then fetching that
 * duplicate through DupeCoordinator::NzbCompleted. The download-time
 * recovery (option <DupeArticleFallback>) gets its say first: with it
 * enabled the failover waits until the duplicates were asked for a sample of
 * articles and could supply less than half of them.
 *
 * Without a better duplicate the download continues - unless it is hopeless:
 * at least a tenth of it was tried and fewer than one in ten of the tried
 * articles existed (the ratio the park-action uses under ParScan=dupe), so
 * neither par2 nor the byte-level repair from duplicates, which needs the
 * collection's own files to verify against, has anything to work with. Such
 * a download is parked too, instead of failing every remaining article;
 * whatever history holds is still tried through DupeCoordinator::NzbCompleted.
 * The tenth-of-the-download floor matters: a posting that merely starts
 * with a dead stretch is below critical health with nothing downloaded yet.
 */
inline bool QueueCoordinator::DownloadHopeless(NzbInfo* nzbInfo)
{
	return nzbget_rs_qhp_download_hopeless(
		nzbInfo->GetTotalArticles(),
		nzbInfo->GetCurrentSuccessArticles(),
		nzbInfo->GetCurrentFailedArticles()
	) != 0;
}

inline void QueueCoordinator::CheckDupeFailover(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (nzbInfo->GetDeleting() || nzbInfo->GetParking())
	{
		return;
	}

	// borrowing from duplicates gets its say first - but only where it applies:
	// with RawArticle, DupeMode force or failures only in par2 files it never
	// counts an attempt, and waiting for its sample would block the failover
	// (and the hopeless park) for good
	int attempted = nzbInfo->GetDupeAttemptedArticles() + nzbInfo->GetDupeUnsourcedArticles();
	int recovered = nzbInfo->GetDupeRecoveredArticles();
	int failed = nzbInfo->GetCurrentFailedArticles();
	bool fallbackEnabled = g_Options->GetDupeArticleFallback() != Options::dafNone;
	bool parCannotCover = (fallbackEnabled && attempted == 0 &&
		nzbInfo->GetDupeParDeferState() == NzbInfo::dpDeferred) ?
		DupeArticleFallback::ParCannotCover(nzbInfo) : false;
	int updateChecked = 0;
	if (nzbget_rs_queue_coord_should_defer_dupe_failover(
		fallbackEnabled ? 1 : 0,
		attempted,
		recovered,
		DupeFailoverSample,
		(int)nzbInfo->GetDupeParDeferState(),
		(int)NzbInfo::dpDeferred,
		parCannotCover ? 1 : 0,
		failed,
		nzbInfo->GetDupeFailoverChecked(),
		&updateChecked) != 0)
	{
		return;
	}
	if (updateChecked)
	{
		nzbInfo->SetDupeFailoverChecked(failed);
	}

	// failing over needs duplicate handling (a backup in history is returned
	// by DupeCoordinator::NzbCompleted only then); parking a hopeless
	// download does not
	HistoryInfo* backup = DupeCoordinator::FailsOver(nzbInfo) ?
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()) :
		nullptr;
	// one that can't lose an article (DownloadCriticalHealth raised the critical
	// health to 100%) has lost one: it fails, whatever its health, like a dead one
	int dlCrit = DownloadCriticalHealth(downloadQueue, nzbInfo);
	int effectiveFailoverHealth = dlCrit > nzbInfo->CalcCriticalHealth(true) ? 0 : nzbInfo->CalcHealth();
	if (backup && DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(),
		effectiveFailoverHealth, backup->GetNzbInfo()->GetDupeScore()))
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Failing over %s to duplicate %s: health %.1f%% below critical %.1f%%, "
			"%i of %i missing article(s) recovered from duplicates",
			nzbInfo->GetName(), backup->GetNzbInfo()->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			DownloadCriticalHealth(downloadQueue, nzbInfo) / 10.0,
			recovered, attempted);
	}
	else if (DownloadHopeless(nzbInfo))
	{
		int tried = nzbInfo->GetCurrentSuccessArticles() + nzbInfo->GetCurrentFailedArticles();
		nzbInfo->PrintMessage(Message::mkWarning,
			"Parking %s: health %.1f%% below critical %.1f%%, %i of %i missing article(s) "
			"recovered from duplicates, %i of %i tried article(s) exist and no better duplicate in history (%s)",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			DownloadCriticalHealth(downloadQueue, nzbInfo) / 10.0,
			recovered, attempted, nzbInfo->GetCurrentSuccessArticles(), tried,
			g_DupeCoordinator->NoBackupReason(downloadQueue, nzbInfo).c_str());
	}
	else
	{
		return;
	}
	// the parked failure is processed like a health-deletion: on its way to
	// history DupeCoordinator::NzbCompleted returns the backup to the queue
	nzbInfo->SetDeleteStatus(NzbInfo::dsHealth);
	downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupParkDelete, nullptr);
}

/*
 * A download (almost) none of whose own articles arrived, after DeadDownloadFailures failed
 * ones on top of the first article of each file (direct-rename asks every file's
 * first article first: a posting that lacks only those is repairable), is dead
 * whatever the probe said - one stray article answering a STAT, or servers that
 * didn't answer it, left it running until the health fell below critical, which
 * takes thousands of failures in a large posting (B45). With HealthCheck=dupe it
 * fails over to the best duplicate in history, as after a dead verdict of the probe.
 */
inline void QueueCoordinator::CheckDeadDownload(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetHealthCheck() != Options::hcDupe || !g_Options->GetDupeCheck() ||
		!DupeCoordinator::FailsOver(nzbInfo) || nzbInfo->GetKind() != NzbInfo::nkNzb ||
		nzbInfo->GetDeleting() || nzbInfo->GetParking() ||
		nzbInfo->GetDeleteStatus() != NzbInfo::dsNone)
	{
		return;
	}

	// its own articles: those borrowed from duplicates say nothing about its posting
	int critical = DownloadCriticalHealth(downloadQueue, nzbInfo);
	int probing = DupeProbe::ProbingFor(nzbInfo->GetId());
	NzbgetRsDeadDownloadEval rsDead{};
	nzbget_rs_qhp_eval_dead_download(
		nzbInfo->GetCurrentSuccessArticles(),
		nzbInfo->GetDupeRecoveredArticles(),
		nzbInfo->GetCurrentFailedArticles(),
		nzbInfo->GetFileCount(),
		nzbInfo->GetDupeFailedRun(),
		DupeArticleFallback::ParCannotCover(nzbInfo, false) ? 1 : 0,
		critical,
		DupeArticleFallback::ParEdgeMargin,
		FilesTried(nzbInfo),
		probing,
		&rsDead
	);
	int own = rsDead.own;
	int failed = nzbInfo->GetCurrentFailedArticles();
	int tried = rsDead.tried;
	bool deadRate = rsDead.dead_rate != 0;
	bool dead = rsDead.dead != 0;
	int projected = rsDead.projected;
	bool doomed = rsDead.doomed != 0;
	bool hopeless = rsDead.hopeless != 0;
	if (!dead && !doomed && CheckParDamage(downloadQueue, nzbInfo))
	{
		return;
	}
	if ((!dead && !doomed) || (probing >= 0 && !hopeless))
	{
		return;
	}

	HistoryInfo* backup = g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo,
		nzbInfo->GetName(), nzbInfo->GetDupeKey());
	if (!backup)
	{
		// nothing to fail over to (B69: Ted Lasso S03E01 4253, 20% arriving, ran through
		// 11,160 articles): one projected under half its critical health that borrowing
		// doesn't fill is parked now, so a client can move on; one projected below
		// critical at all once ParkBelowCriticalSample articles were tried, as the
		// projection is surer then (B74: Las Azules S02E05 4417, 79.3% against 80%, ran
		// through 10 GB). Borrowing gets its say: while it waits for par-check, or
		// recovers most of what it tries, no park.
		int attempted = nzbInfo->GetDupeAttemptedArticles() + nzbInfo->GetDupeUnsourcedArticles();
		bool farBelow = rsDead.far_below != 0;
		bool below = rsDead.below != 0;
		if (nzbget_rs_queue_coord_should_park_dead_without_backup(
			g_Options->GetDupeArticleFallback() != Options::dafNone ? 1 : 0,
			(int)nzbInfo->GetDupeParDeferState(),
			(int)NzbInfo::dpDeferred,
			attempted,
			nzbInfo->GetDupeRecoveredArticles(),
			tried,
			ProjectedFailureSample,
			tried >= ProjectedFailureSample ? FilesTried(nzbInfo) : 0,
			farBelow ? 1 : 0,
			below ? 1 : 0) != 0)
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Parking %s: %i of %i tried article(s) of its own arrived, projected health %.1f%% "
				"%s critical %.1f%%, and no duplicate in history to fail over to (%s)",
				nzbInfo->GetName(), own, tried,
				projected / 10.0,
				farBelow ? "far below" : "below",
				critical / 10.0,
				g_DupeCoordinator->NoBackupReason(downloadQueue, nzbInfo).c_str());
			nzbInfo->SetDeleteStatus(NzbInfo::dsHealth);
			downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupParkDelete, nullptr);
		}
		return;
	}

	if (dead)
	{
		if (!DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(), 0, backup->GetNzbInfo()->GetDupeScore()))
		{
			return;
		}
		if (deadRate)
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Failing over %s to duplicate %s: %i of its own articles downloaded, %i failed",
				nzbInfo->GetName(), backup->GetNzbInfo()->GetName(), own, failed);
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Failing over %s to duplicate %s: %i of its articles failed in a row (%i of %i tried arrived)",
				nzbInfo->GetName(), backup->GetNzbInfo()->GetName(), nzbInfo->GetDupeFailedRun(), own, tried);
		}
	}
	else
	{
		// a partly alive download is only swapped for a backup known to be clearly
		// wholer than its projection (by a dupe tool's or a duplicate search's health
		// check): BackupLead more
		int backupAlive = -1;
		for (const char* name : {"DupeAlive", "DupeHealth"})
		{
			backupAlive = std::max(backupAlive, DupeCoordinator::AlivePermille(backup->GetNzbInfo(), name));
		}
		if (backupAlive < projected + BackupLead || backupAlive < critical ||
			!DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(), projected,
				backup->GetNzbInfo()->GetDupeScore()))
		{
			return;
		}
		nzbInfo->PrintMessage(Message::mkWarning,
			"Failing over %s to duplicate %s: %i of %i tried article(s) of its own arrived, projected health "
			"%.1f%% %s critical %.1f%%; the duplicate is %.0f%% alive",
			nzbInfo->GetName(), backup->GetNzbInfo()->GetName(), own, tried,
			projected / 10.0,
			projected < critical ? "below" : "near",
			critical / 10.0,
			backupAlive / 10.0);
	}
	nzbInfo->SetDeleteStatus(NzbInfo::dsHealth);
	downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupParkDelete, nullptr);
}

/*
 * Health counts articles, par2 repairs blocks: with par2 blocks of 15-25 MB each
 * lost article of 0.7 MB spoils a block, and a download at health 98% can have
 * most of its blocks damaged (FUZEER Joker 7826: 6.8% of articles lost, 87% of
 * blocks; House of the Dragon 7896: 1.5% lost, 153 blocks short with 97 to
 * recover). It downloaded in full and failed par-repair; a duplicate then
 * succeeded. Once the blocks spoiled in the files finished so far exceed all the
 * recovery blocks its par2-files hold, or at that rate will exceed them by a
 * quarter with a fifth of it done, it fails over to the duplicate (for the
 * projection only one known to be whole, 99.9% or more). Without a duplicate
 * it's left to finish: stream repair may still fill the holes.
 */
inline bool QueueCoordinator::CheckParDamage(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	ParDamage::Verdict verdict = ParDamage::Judge(nzbInfo);
	bool certain = verdict.Certain();
	if (!certain && !verdict.Projected())
	{
		return false;
	}

	// borrowing the missing articles from duplicates gets its turn first (B36):
	// the damage ends its wait for par-check (ParCannotCover), and the failover
	// comes when it tried DupeFailoverSample articles and recovered under half
	bool fallbackEnabled = g_Options->GetDupeArticleFallback() != Options::dafNone;
	bool hasPar2 = (fallbackEnabled && !g_Options->GetRawArticle() &&
		nzbInfo->GetDupeMode() != dmForce) ? DupeArticleFallback::HasPar2(nzbInfo) : false;
	if (nzbget_rs_queue_coord_should_wait_for_borrowing_on_par_damage(
		fallbackEnabled ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce,
		hasPar2 ? 1 : 0,
		(int)nzbInfo->GetDupeParDeferState(),
		(int)NzbInfo::dpDeferred,
		nzbInfo->GetDupeAttemptedArticles() + nzbInfo->GetDupeUnsourcedArticles(),
		DupeFailoverSample,
		nzbInfo->GetDupeRecoveredArticles()) != 0)
	{
		return false;
	}

	// a duplicate of it queued and not paused: a dupe tool is swapping it for a
	// backup just now, and failing over too would fetch a second one (paused
	// duplicates are borrowing donors, no reason to wait)
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued != nzbInfo &&
			queued->GetPausedSize() < queued->GetRemainingSize() &&
			DupeCoordinator::SameNameOrKey(queued->GetName(), queued->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			return false;
		}
	}

	HistoryInfo* backup = g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo,
		nzbInfo->GetName(), nzbInfo->GetDupeKey());
	NzbgetRsBuf damageBuf = nzbget_rs_queue_coord_format_par_damage_summary(
		certain ? 1 : 0,
		verdict.damagedBlocks,
		verdict.projectedBlocks,
		verdict.blockSize,
		verdict.recoveryBlocks);
	BString<1024> damage = damageBuf.data ? damageBuf.data : "";
	nzbget_rs_free(damageBuf);
	if (!backup)
	{
		if (ParDamage::FirstReport(nzbInfo->GetId()))
		{
			nzbInfo->PrintMessage(Message::mkInfo, "%s: %s; no duplicate in history to fail over to",
				nzbInfo->GetName(), *damage);
		}
		return false;
	}

	int backupAlive = -1;
	for (const char* name : {"DupeAlive", "DupeHealth"})
	{
		backupAlive = std::max(backupAlive, DupeCoordinator::AlivePermille(backup->GetNzbInfo(), name));
	}
	if ((!certain && backupAlive < 999) ||
		!DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(), 0, backup->GetNzbInfo()->GetDupeScore()))
	{
		return false;
	}

	nzbInfo->PrintMessage(Message::mkWarning, "Failing over %s to duplicate %s: %s, par-repair can't fix it",
		nzbInfo->GetName(), backup->GetNzbInfo()->GetName(), *damage);
	ParDamage::Forget(nzbInfo->GetId());
	nzbInfo->SetDeleteStatus(NzbInfo::dsHealth);
	downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupParkDelete, nullptr);
	return true;
}

/*
 * A file nobody plays and nothing unpacks: release info, checksums, images, links,
 * and whatever option <ExtCleanupDisk> lists for deletion anyway.
 */
inline bool QueueCoordinator::SideFile(const char* filename)
{
	return nzbget_rs_qhp_is_side_file(filename, g_Options->GetExtCleanupDisk()) != 0;
}

inline int QueueCoordinator::FilesTried(NzbInfo* nzbInfo)
{
	int files = (int)nzbInfo->GetCompletedFiles()->size();
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (fileInfo->GetSuccessArticles() + fileInfo->GetFailedArticles() > 0)
		{
			files++;
		}
	}
	return files;
}

inline void QueueCoordinator::StartDeadPickProbe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetHealthCheck() != Options::hcDupe || !g_Options->GetDupeCheck() ||
		nzbInfo->GetDeadPickProbed() || !DupeCoordinator::FailsOver(nzbInfo) ||
		nzbInfo->GetKind() != NzbInfo::nkNzb || nzbInfo->GetDeleting() ||
		nzbInfo->GetParking() || nzbInfo->GetDeleteStatus() != NzbInfo::dsNone)
	{
		return;
	}
	nzbInfo->SetDeadPickProbed(true);
	if (nzbInfo->GetCurrentSuccessArticles() > 0)
	{
		return;
	}

	// a dead posting is only worth abandoning when a duplicate waits
	if (!g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()))
	{
		return;
	}

	// the articles to check, spread over the whole posting: the unpaused files
	// (data files before par2 files) are treated as one run of articles
	std::vector<FileInfo*> files;
	for (int pass = 0; pass < 2 && files.empty(); pass++)
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (!fileInfo->GetDeleted() && !fileInfo->GetPaused() &&
				fileInfo->GetTotalArticles() > 0 &&
				(pass == 1 || !fileInfo->GetParFile()))
			{
				files.push_back(fileInfo);
			}
		}
	}
	size_t total = 0;
	for (FileInfo* fileInfo : files)
	{
		total += fileInfo->GetTotalArticles();
	}
	if (total < (size_t)DupeProbe::MinArticles)
	{
		return;
	}

	// the article list of a file that hasn't started yet is on disk
	std::vector<DupeProbe::Sample> samples = DupeProbe::SamplesOf(files, [this](FileInfo* fileInfo)
		{
			if (g_Options->GetServerMode())
			{
				g_DiskState->LoadArticles(fileInfo);
				LoadPartialState(fileInfo);
			}
		});
	if (samples.size() < (size_t)DupeProbe::MinArticles)
	{
		return;
	}

	DupeProbe::Start(nzbInfo->GetId(), std::move(samples), nzbInfo->GetDupeRecoveredArticles());
}

inline void QueueCoordinator::FailOverDeadPick(DownloadQueue* downloadQueue, int nzbId, int samples,
	int missingServers, int activeServers, int recoveredAtStart)
{
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(nzbId);
	if (!nzbInfo || nzbInfo->GetDeleting() || nzbInfo->GetParking() ||
		nzbInfo->GetDeleteStatus() != NzbInfo::dsNone)
	{
		return;
	}
	// something of its own arrived meanwhile: a partly alive posting is never
	// abandoned here (the regular health check handles partial damage). Articles
	// borrowed from duplicates since it started don't count: they say nothing
	// about its own posting (B39); nor does a single stray one (B45)
	int ownSuccess = nzbInfo->GetCurrentSuccessArticles() -
		std::max(0, nzbInfo->GetDupeRecoveredArticles() - recoveredAtStart);
	if (ownSuccess >= DupeProbe::MinAliveSamples)
	{
		return;
	}

	HistoryInfo* backup = DupeCoordinator::FailsOver(nzbInfo) ?
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()) :
		nullptr;
	// the posting is dead, whatever score it was queued with: health 0
	if (!backup || !DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(), 0,
		backup->GetNzbInfo()->GetDupeScore()))
	{
		return;
	}

	nzbInfo->PrintMessage(Message::mkWarning,
		"Failing over %s to duplicate %s: none of %i sampled articles exists on any server "
		"(%i of %i servers answered definitively)",
		nzbInfo->GetName(), backup->GetNzbInfo()->GetName(), samples, missingServers, activeServers);

	// processed like a health-deletion: on its way to history
	// DupeCoordinator::NzbCompleted returns the backup to the queue
	nzbInfo->SetDeleteStatus(NzbInfo::dsHealth);
	downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupParkDelete, nullptr);
}

inline void QueueCoordinator::LogDebugInfo()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	info("   ---------- Queue");
	int64 remaining, remainingForced;
	downloadQueue->CalcRemainingSize(&remaining, &remainingForced);
	info("     Remaining: %.1f MB, Forced: %.1f MB",
		nzbget_rs_dqh_stream_repair_bytes_to_mb(remaining),
		nzbget_rs_dqh_stream_repair_bytes_to_mb(remainingForced));
	info("     Download: %s, Post-process: %s, Scan: %s",
		 g_WorkState->GetPauseDownload() ? "paused" : (g_WorkState->GetTempPauseDownload() ? "temp-paused" : "active"),
		 g_WorkState->GetPausePostProcess() ? "paused" : "active",
		 g_WorkState->GetPauseScan() ? "paused" : "active");

	info("   ---------- QueueCoordinator");
	info("    Active Downloads: %i, Limit: %i", (int)m_activeDownloads.size(), m_downloadsLimit);
	for (ArticleDownloader* articleDownloader : m_activeDownloads)
	{
		articleDownloader->LogDebugInfo();
	}
}

inline void QueueCoordinator::ResetHangingDownloads()
{
	if (g_Options->GetArticleTimeout() == 0)
	{
		return;
	}

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	time_t tm = Util::CurrentTime();

	for (ArticleDownloader* articleDownloader : m_activeDownloads)
	{
		if (nzbget_rs_qhp_is_hanging_download(
			(int64_t)tm,
			(int64_t)articleDownloader->GetLastUpdateTime(),
			g_Options->GetArticleTimeout(),
			articleDownloader->GetStatus() == ArticleDownloader::adRunning ? 1 : 0
		) != 0)
		{
			error("Cancelling hanging download %s @ %s", articleDownloader->GetInfoName(),
				articleDownloader->GetConnectionName());
			articleDownloader->Stop();
		}
	}
}

/*
 * Returns True if Entry was deleted from Queue or False if it was scheduled for Deletion.
 * NOTE: "False" does not mean unsuccess; the entry is (or will be) deleted in any case.
 */
inline bool QueueCoordinator::DeleteQueueEntry(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	fileInfo->SetDeleted(true);
	bool downloading = false;
	for (ArticleDownloader* articleDownloader : m_activeDownloads)
	{
		if (articleDownloader->GetFileInfo() == fileInfo)
		{
			downloading = true;
			articleDownloader->Stop();
		}
	}

	if (!downloading)
	{
		DeleteFileInfo(downloadQueue, fileInfo, false);
	}
	return downloading;
}

inline bool QueueCoordinator::SetQueueEntryCategory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* category)
{
	if (nzbInfo->GetPostInfo())
	{
		error("Could not change category for %s. File in post-process-stage", nzbInfo->GetName());
		return false;
	}

	BString<1024> oldDestDir = nzbInfo->GetDestDir();

	nzbInfo->SetCategory(category);
	nzbInfo->BuildDestDirName();

	bool dirUnchanged = nzbget_rs_dqh_bytes_equal(nzbInfo->GetDestDir(), oldDestDir) != 0;
	bool ok = dirUnchanged || ArticleWriter::MoveCompletedFiles(nzbInfo, oldDestDir);

	return ok;
}

inline bool QueueCoordinator::SetQueueEntryName(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* name)
{
	if (nzbInfo->GetPostInfo())
	{
		error("Could not rename %s. File in post-process-stage", nzbInfo->GetName());
		return false;
	}

	if (Util::EmptyStr(name))
	{
		error("Could not rename %s. The new name cannot be empty", nzbInfo->GetName());
		return false;
	}

	nzbInfo->SetName(NzbInfo::MakeNiceNzbName(name, false));

	if (nzbInfo->GetKind() == NzbInfo::nkUrl)
	{
		nzbInfo->SetFilename(BString<1024>("%s.nzb", nzbInfo->GetName()));
		return true;
	}

	BString<1024> oldDestDir = nzbInfo->GetDestDir();

	nzbInfo->BuildDestDirName();

	bool dirUnchanged = nzbget_rs_dqh_bytes_equal(nzbInfo->GetDestDir(), oldDestDir) != 0;
	bool ok = dirUnchanged || ArticleWriter::MoveCompletedFiles(nzbInfo, oldDestDir);

	return ok;
}

static inline NzbgetRsNzbMergeSplitCounters ReadNzbMergeSplitCounters(NzbInfo* nzbInfo)
{
	NzbgetRsNzbMergeSplitCounters c{};
	c.file_count = nzbInfo->GetFileCount();
	c.active_downloads = nzbInfo->GetActiveDownloads();
	c.full_content_hash = nzbInfo->GetFullContentHash();
	c.filtered_content_hash = nzbInfo->GetFilteredContentHash();
	c.size = nzbInfo->GetSize();
	c.remaining_size = nzbInfo->GetRemainingSize();
	c.paused_file_count = nzbInfo->GetPausedFileCount();
	c.paused_size = nzbInfo->GetPausedSize();
	c.success_size = nzbInfo->GetSuccessSize();
	c.current_success_size = nzbInfo->GetCurrentSuccessSize();
	c.failed_size = nzbInfo->GetFailedSize();
	c.current_failed_size = nzbInfo->GetCurrentFailedSize();
	c.par_size = nzbInfo->GetParSize();
	c.par_success_size = nzbInfo->GetParSuccessSize();
	c.par_current_success_size = nzbInfo->GetParCurrentSuccessSize();
	c.par_failed_size = nzbInfo->GetParFailedSize();
	c.par_current_failed_size = nzbInfo->GetParCurrentFailedSize();
	c.remaining_par_count = nzbInfo->GetRemainingParCount();
	c.total_articles = nzbInfo->GetTotalArticles();
	c.success_articles = nzbInfo->GetSuccessArticles();
	c.failed_articles = nzbInfo->GetFailedArticles();
	c.current_success_articles = nzbInfo->GetCurrentSuccessArticles();
	c.current_failed_articles = nzbInfo->GetCurrentFailedArticles();
	c.min_time = (int64)nzbInfo->GetMinTime();
	c.max_time = (int64)nzbInfo->GetMaxTime();
	c.downloaded_size = nzbInfo->GetDownloadedSize();
	c.download_sec = nzbInfo->GetDownloadSec();
	c.download_start_time = (int64)nzbInfo->GetDownloadStartTime();
	return c;
}

static inline void WriteNzbMergeSplitCounters(NzbInfo* nzbInfo, const NzbgetRsNzbMergeSplitCounters& c)
{
	nzbInfo->SetFileCount(c.file_count);
	nzbInfo->SetActiveDownloads(c.active_downloads);
	nzbInfo->SetFullContentHash(c.full_content_hash);
	nzbInfo->SetFilteredContentHash(c.filtered_content_hash);
	nzbInfo->SetSize(c.size);
	nzbInfo->SetRemainingSize(c.remaining_size);
	nzbInfo->SetPausedFileCount(c.paused_file_count);
	nzbInfo->SetPausedSize(c.paused_size);
	nzbInfo->SetSuccessSize(c.success_size);
	nzbInfo->SetCurrentSuccessSize(c.current_success_size);
	nzbInfo->SetFailedSize(c.failed_size);
	nzbInfo->SetCurrentFailedSize(c.current_failed_size);
	nzbInfo->SetParSize(c.par_size);
	nzbInfo->SetParSuccessSize(c.par_success_size);
	nzbInfo->SetParCurrentSuccessSize(c.par_current_success_size);
	nzbInfo->SetParFailedSize(c.par_failed_size);
	nzbInfo->SetParCurrentFailedSize(c.par_current_failed_size);
	nzbInfo->SetRemainingParCount(c.remaining_par_count);
	nzbInfo->SetTotalArticles(c.total_articles);
	nzbInfo->SetSuccessArticles(c.success_articles);
	nzbInfo->SetFailedArticles(c.failed_articles);
	nzbInfo->SetCurrentSuccessArticles(c.current_success_articles);
	nzbInfo->SetCurrentFailedArticles(c.current_failed_articles);
	nzbInfo->SetMinTime((time_t)c.min_time);
	nzbInfo->SetMaxTime((time_t)c.max_time);
	nzbInfo->SetDownloadedSize(c.downloaded_size);
	nzbInfo->SetDownloadSec(c.download_sec);
	nzbInfo->SetDownloadStartTime((time_t)c.download_start_time);
}

inline bool QueueCoordinator::MergeQueueEntries(DownloadQueue* downloadQueue, NzbInfo* destNzbInfo, NzbInfo* srcNzbInfo)
{
	if (destNzbInfo->GetPostInfo() || srcNzbInfo->GetPostInfo())
	{
		error("Could not merge %s and %s. File in post-process-stage", destNzbInfo->GetName(), srcNzbInfo->GetName());
		return false;
	}

	if (destNzbInfo->GetKind() == NzbInfo::nkUrl || srcNzbInfo->GetKind() == NzbInfo::nkUrl)
	{
		error("Could not merge %s and %s. URLs cannot be merged", destNzbInfo->GetName(), srcNzbInfo->GetName());
		return false;
	}

	// the files downloaded so far move to the destination's directory: a category
	// change rebuilt the source's own directory, so they stayed where the merged
	// group never looks (par-check and unpack missed them)
	BString<1024> oldDestDir = srcNzbInfo->GetDestDir();
	srcNzbInfo->SetCategory(destNzbInfo->GetCategory());
	srcNzbInfo->SetDestDir(destNzbInfo->GetDestDir());
	if (nzbget_rs_dqh_bytes_equal(oldDestDir, srcNzbInfo->GetDestDir()) == 0)
	{
		ArticleWriter::MoveCompletedFiles(srcNzbInfo, oldDestDir);
	}

	// reattach file items to new NZBInfo-object
	for (std::unique_ptr<FileInfo>& fileInfo : *srcNzbInfo->GetFileList())
	{
		fileInfo->SetNzbInfo(destNzbInfo);
		destNzbInfo->GetFileList()->Add(std::move(fileInfo));
	}
	srcNzbInfo->GetFileList()->clear();

	NzbgetRsNzbMergeSplitCounters destCounters = ReadNzbMergeSplitCounters(destNzbInfo);
	NzbgetRsNzbMergeSplitCounters srcCounters = ReadNzbMergeSplitCounters(srcNzbInfo);
	nzbget_rs_queue_coord_merge_counters(&destCounters, &srcCounters);
	WriteNzbMergeSplitCounters(destNzbInfo, destCounters);
	destNzbInfo->GetServerStats()->ListOp(srcNzbInfo->GetServerStats(), ServerStatList::soAdd);
	destNzbInfo->GetCurrentServerStats()->ListOp(srcNzbInfo->GetCurrentServerStats(), ServerStatList::soAdd);

	// reattach completed file items to new NZBInfo-object
	for (CompletedFile& completedFile : srcNzbInfo->GetCompletedFiles())
	{
		destNzbInfo->GetCompletedFiles()->push_back(std::move(completedFile));
	}
	srcNzbInfo->GetCompletedFiles()->clear();

	// concatenate QueuedFilenames using character '|' as separator
	destNzbInfo->SetQueuedFilename(CString::FormatStr("%s|%s",
		destNzbInfo->GetQueuedFilename(), srcNzbInfo->GetQueuedFilename()));
	// the merged download is other content: its twin fingerprint and labels go
	// (TwinCheck leaves merged downloads alone: their nzb-file is two)
	for (const char* name : { TwinCheck::FilesParam, TwinCheck::KindParam, TwinCheck::SampledParam,
		TwinCheck::SampledOfParam })
	{
		destNzbInfo->GetParameters()->SetParameter(name, nullptr);
	}

	g_DiskState->DiscardFiles(srcNzbInfo);
	ParDamage::Forget(srcNzbInfo->GetId());
	downloadQueue->GetQueue()->Remove(srcNzbInfo);

	return true;
}

/*
 * Creates new nzb-item out of existing files from other nzb-items.
 * If any of file-items is being downloaded the command fail.
 * For each file-item an event "eaFileDeleted" is fired.
 */
inline bool QueueCoordinator::SplitQueueEntries(DownloadQueue* downloadQueue, RawFileList* fileList, const char* name, NzbInfo** newNzbInfo)
{
	NzbInfo* srcNzbInfo = nullptr;

	for (FileInfo* fileInfo : fileList)
	{
		int splitErr = nzbget_rs_queue_coord_validate_split_file_state(
			fileInfo->GetActiveDownloads(),
			fileInfo->GetCompletedArticles(),
			fileInfo->GetNzbInfo()->GetPostInfo() ? 1 : 0,
			(!srcNzbInfo || fileInfo->GetNzbInfo() == srcNzbInfo) ? 1 : 0);
		if (splitErr == 1)
		{
			error("Could not split %s. File is already (partially) downloaded", fileInfo->GetFilename());
			return false;
		}
		else if (splitErr == 2)
		{
			error("Could not split %s. File in post-process-stage", fileInfo->GetFilename());
			return false;
		}
		else if (splitErr == 3)
		{
			error("Could not split %s. The files belong to different collections", fileInfo->GetFilename());
			return false;
		}
		if (!srcNzbInfo)
		{
			srcNzbInfo = fileInfo->GetNzbInfo();
		}
	}

	if (!srcNzbInfo)
	{
		return false;
	}

	std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();

	nzbInfo->SetFilename(srcNzbInfo->GetFilename());
	nzbInfo->SetName(name);
	nzbInfo->SetCategory(srcNzbInfo->GetCategory());
	nzbInfo->SetFullContentHash(0);
	nzbInfo->SetFilteredContentHash(0);
	nzbInfo->SetPriority(srcNzbInfo->GetPriority());
	nzbInfo->BuildDestDirName();
	nzbInfo->SetQueuedFilename(srcNzbInfo->GetQueuedFilename());
	nzbInfo->GetParameters()->CopyFrom(srcNzbInfo->GetParameters());

	srcNzbInfo->SetFullContentHash(0);
	srcNzbInfo->SetFilteredContentHash(0);

	NzbgetRsNzbMergeSplitCounters srcCounters = ReadNzbMergeSplitCounters(srcNzbInfo);
	NzbgetRsNzbMergeSplitCounters dstCounters = ReadNzbMergeSplitCounters(nzbInfo.get());

	for (FileInfo* fileInfo : fileList)
	{
		DownloadQueue::Aspect aspect = { DownloadQueue::eaFileDeleted, downloadQueue, fileInfo->GetNzbInfo(), fileInfo };
		downloadQueue->Notify(&aspect);

		nzbInfo->GetFileList()->Add(srcNzbInfo->GetFileList()->Remove(fileInfo));
		fileInfo->SetNzbInfo(nzbInfo.get());

		NzbgetRsSplitFileCounters fc{};
		fc.size = fileInfo->GetSize();
		fc.remaining_size = fileInfo->GetRemainingSize();
		fc.success_size = fileInfo->GetSuccessSize();
		fc.failed_size = fileInfo->GetFailedSize();
		fc.missed_size = fileInfo->GetMissedSize();
		fc.total_articles = fileInfo->GetTotalArticles();
		fc.missed_articles = fileInfo->GetMissedArticles();
		fc.success_articles = fileInfo->GetSuccessArticles();
		fc.failed_articles = fileInfo->GetFailedArticles();
		fc.par_file = fileInfo->GetParFile() ? 1 : 0;
		fc.paused = fileInfo->GetPaused() ? 1 : 0;

		nzbget_rs_queue_coord_split_transfer_file(&srcCounters, &dstCounters, &fc);
		srcNzbInfo->GetCurrentServerStats()->ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
		nzbInfo->GetCurrentServerStats()->ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
	}

	WriteNzbMergeSplitCounters(srcNzbInfo, srcCounters);
	WriteNzbMergeSplitCounters(nzbInfo.get(), dstCounters);

	nzbInfo->UpdateMinMaxTime();
	if (srcNzbInfo->GetCompletedFiles()->empty())
	{
		srcNzbInfo->UpdateMinMaxTime();
	}

	if (srcNzbInfo->GetFileList()->empty())
	{
		g_DiskState->DiscardFiles(srcNzbInfo);
		ParDamage::Forget(srcNzbInfo->GetId());
		downloadQueue->GetQueue()->Remove(srcNzbInfo);
	}

	*newNzbInfo = nzbInfo.get();
	downloadQueue->GetQueue()->Add(std::move(nzbInfo));

	return true;
}

inline void QueueCoordinator::DirectRenameCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (g_Options->GetServerMode() && !fileInfo->GetArticles()->empty())
		{
			// save new file name into disk state file
			g_DiskState->SaveFile(fileInfo);
		}
	}

	DiscardDirectRename(downloadQueue, nzbInfo);

	nzbInfo->SetDirectRenameStatus(NzbInfo::tsSuccess);

	if (g_Options->GetParCheck() != Options::pcForce)
	{
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupResume, nullptr);
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupPauseAllPars, nullptr);
	}

	if (g_Options->GetReorderFiles())
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Reordering files for %s", nzbInfo->GetName());
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupSortFiles, nullptr);
	}

	nzbInfo->SetChanged(true);
	downloadQueue->SaveChanged();

	DownloadQueue::Aspect namedAspect = { DownloadQueue::eaNzbNamed, downloadQueue, nzbInfo, nullptr };
	downloadQueue->Notify(&namedAspect);
}

inline void QueueCoordinator::DiscardDirectRename(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	int64 discardedSize = 0;
	int discardedCount = 0;

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		int discardInfos = 0;
		bool discardPar = nzbget_rs_queue_coord_eval_discard_direct_rename_file(
			fileInfo->GetParFile() ? 1 : 0,
			fileInfo->GetCompletedArticles(),
			fileInfo->GetActiveDownloads(),
			g_Options->GetServerMode() ? 1 : 0,
			!fileInfo->GetArticles()->empty() ? 1 : 0,
			g_Options->GetContinuePartial() ? 1 : 0,
			fileInfo->GetCachedArticles(),
			&discardInfos) != 0;
		if (discardPar)
		{
			bool locked = false;
			{
				Guard contentGuard = g_ArticleCache->GuardContent();
				locked = fileInfo->GetFlushLocked();
				if (!locked)
				{
					fileInfo->SetFlushLocked(true);
				}
			}

			if (!locked)
			{
				// discard downloaded articles from partially downloaded par-files
				discardedSize += fileInfo->GetSuccessSize();
				discardedCount++;

				DiscardDownloadedArticles(nzbInfo, fileInfo);
			}

			if (!locked)
			{
				Guard contentGuard = g_ArticleCache->GuardContent();
				fileInfo->SetFlushLocked(false);
			}
		}

		if (discardInfos && !fileInfo->GetArticles()->empty())
		{
			// discard article infos to free up memory if possible
			debug("Discarding article infos for %s/%s", nzbInfo->GetName(), fileInfo->GetFilename());
			fileInfo->SetPartialChanged(true);
			SavePartialState(fileInfo);
			fileInfo->GetArticles()->clear();
		}
	}

	if (discardedSize > 0)
	{
		nzbInfo->PrintMessage(Message::mkDetail, "Discarded %s from %i files used for direct renaming",
			*Util::FormatSize(discardedSize), discardedCount);
	}
}

inline void QueueCoordinator::DiscardDownloadedArticles(NzbInfo* nzbInfo, FileInfo* fileInfo)
{
	int64 nextNzbRemSize = nzbInfo->GetRemainingSize();
	int64 nextNzbPausedSize = nzbInfo->GetPausedSize();
	int nextNzbCurSuccArt = nzbInfo->GetCurrentSuccessArticles();
	int64 nextNzbCurSuccSize = nzbInfo->GetCurrentSuccessSize();
	int64 nextNzbParCurSuccSize = nzbInfo->GetParCurrentSuccessSize();
	int nextNzbCurFailArt = nzbInfo->GetCurrentFailedArticles();
	int64 nextNzbCurFailSize = nzbInfo->GetCurrentFailedSize();
	int64 nextNzbParCurFailSize = nzbInfo->GetParCurrentFailedSize();
	int64 nextFileRemSize = fileInfo->GetRemainingSize();
	nzbget_rs_queue_coord_discard_downloaded_articles_counters(
		nextNzbRemSize,
		nextNzbPausedSize,
		nextNzbCurSuccArt,
		nextNzbCurSuccSize,
		nextNzbParCurSuccSize,
		nextNzbCurFailArt,
		nextNzbCurFailSize,
		nextNzbParCurFailSize,
		fileInfo->GetPaused() ? 1 : 0,
		fileInfo->GetSuccessArticles(),
		fileInfo->GetSuccessSize(),
		fileInfo->GetFailedArticles(),
		fileInfo->GetFailedSize(),
		fileInfo->GetSize(),
		fileInfo->GetMissedSize(),
		&nextNzbRemSize,
		&nextNzbPausedSize,
		&nextNzbCurSuccArt,
		&nextNzbCurSuccSize,
		&nextNzbParCurSuccSize,
		&nextNzbCurFailArt,
		&nextNzbCurFailSize,
		&nextNzbParCurFailSize,
		&nextFileRemSize);
	nzbInfo->SetRemainingSize(nextNzbRemSize);
	nzbInfo->SetPausedSize(nextNzbPausedSize);
	nzbInfo->GetCurrentServerStats()->ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
	fileInfo->GetServerStats()->clear();
	nzbInfo->SetCurrentSuccessArticles(nextNzbCurSuccArt);
	nzbInfo->SetCurrentSuccessSize(nextNzbCurSuccSize);
	nzbInfo->SetParCurrentSuccessSize(nextNzbParCurSuccSize);
	fileInfo->SetSuccessSize(0);
	fileInfo->SetSuccessArticles(0);
	nzbInfo->SetCurrentFailedArticles(nextNzbCurFailArt);
	nzbInfo->SetCurrentFailedSize(nextNzbCurFailSize);
	nzbInfo->SetParCurrentFailedSize(nextNzbParCurFailSize);
	fileInfo->SetFailedSize(0);
	fileInfo->SetFailedArticles(0);
	fileInfo->SetCompletedArticles(0);
	fileInfo->SetRemainingSize(nextFileRemSize);

	// discard temporary files
	DiscardTempFiles(fileInfo);
	g_DiskState->DiscardFile(fileInfo->GetId(), false, true, false);

	fileInfo->SetOutputFilename(nullptr);
	fileInfo->SetOutputInitialized(false);
	fileInfo->SetCachedArticles(0);
	fileInfo->SetPartialChanged(false);
	fileInfo->SetPartialState(FileInfo::psNone);

	if (g_Options->GetServerMode())
	{
		// free up memory used by articles if possible
		fileInfo->GetArticles()->clear();
	}
	else
	{
		// reset article states if discarding isn't possible
		for (ArticleInfo* articleInfo : fileInfo->GetArticles())
		{
			articleInfo->SetStatus(ArticleInfo::aiUndefined);
			articleInfo->SetResultFilename(nullptr);
			articleInfo->DiscardSegment();
		}
	}
}

#endif
#endif
