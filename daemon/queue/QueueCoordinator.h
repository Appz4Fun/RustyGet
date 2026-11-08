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
#include "Log.h"
#include "DiskState.h"
#include "DownloadInfo.h"
#include "ServerPool.h"
#include "Options.h"
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
#include "Options.h"
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

#include "DupeArticleFallback.h"

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

#include <algorithm>
#include <cstdarg>
#include "Options.h"
#include "ServerPool.h"
#include "ServerPool.h"
#include "DiskState.h"
#include "Util.h"
#include "Util.h"
#include "ServerPool.h"
#include "HistoryCoordinator.h"
#include "DupeArticleFallback.h"
#include "PrePostProcessor.h"
#include "HistoryCoordinator.h"
#include "Scanner.h"
#include "DupeSearch.h"
#include "nzbget_rs.h"

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
