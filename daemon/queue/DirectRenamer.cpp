/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2017-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include <cstddef>
#include <memory>
#include "DirectRenamer.h"
#include "Options.h"
#include "FileSystem.h"
#include "ParParser.h"
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

void DirectParLoader::StartLoader(DirectRenamer* owner, NzbInfo* nzbInfo)
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

void DirectParLoader::Run()
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

void DirectParLoader::LoadParFile(const char* parFile)
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

std::unique_ptr<ArticleContentAnalyzer> DirectRenamer::MakeArticleContentAnalyzer()
{
	return std::make_unique<RenameContentAnalyzer>();
}

bool DirectRenamer::AnalyzedParFile(ArticleContentAnalyzer* articleContentAnalyzer)
{
	return ((RenameContentAnalyzer*)articleContentAnalyzer)->GetParFile();
}

void DirectRenamer::ArticleDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo,
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

void DirectRenamer::FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	CheckState(downloadQueue, fileInfo->GetNzbInfo());
}

void DirectRenamer::CheckState(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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
void DirectRenamer::UnpausePars(NzbInfo* nzbInfo)
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

void DirectRenamer::CollectPars(NzbInfo* nzbInfo, ParFileList* parFiles)
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

void DirectRenamer::RenameFiles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileHashList* parHashes)
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
int DirectRenamer::RenameFilesInProgress(NzbInfo* nzbInfo, FileHashList* parHashes, bool needRenamePars, int& vol)
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

int DirectRenamer::RenameCompletedFiles(NzbInfo* nzbInfo, FileHashList* parHashes, bool needRenamePars, int& vol)
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

std::string DirectRenamer::BuildNewRegularName(const char* oldName, FileHashList* parHashes, const char* hash16k)
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

std::string DirectRenamer::BuildNewParName(const char* oldName, const char* destDir, const char* setId, int& vol)
{
	NzbgetRsBuf name = nzbget_rs_direct_par_name(oldName, destDir, setId, PATH_SEPARATOR, &vol);
	std::string res(name.data ? name.data : "", name.len);
	nzbget_rs_free(name);
	return res;
}

bool DirectRenamer::NeedRenamePars(NzbInfo* nzbInfo)
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
	return nzbget_rs_direct_need_rename_pars(names.data(), names.size(), ParParser::FilenameFold) != 0;
}

bool DirectRenamer::RenameFile(NzbInfo* nzbInfo, const std::string& oldFullFilename, const std::string& newFullFilename)
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


void RenameContentAnalyzer::Reset()
{
#ifndef DISABLE_PARCHECK
	nzbget_rs_rename_analyzer_reset(m_rs.get());
#endif
}

void RenameContentAnalyzer::Append(const void* buffer, int len)
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
void RenameContentAnalyzer::Finish()
{
#ifndef DISABLE_PARCHECK
	char hash[33];
	nzbget_rs_rename_analyzer_finish(m_rs.get(), hash);
	m_hash16k = hash;
#endif
}
