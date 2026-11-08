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
#include "ServerPool.h"
#include "Util.h"
#include "ServerPool.h"
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

#include "Connection.h"
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
#include "PrePostProcessor.h"

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
#include "ServerPool.h"
#include "Connection.h"
#include "ServerPool.h"
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
#include "ServerPool.h"
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
#include "ServerPool.h"
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
