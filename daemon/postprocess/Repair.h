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


#ifndef REPAIR_H
#define REPAIR_H

#include "DownloadInfo.h"
#include "Thread.h"
#include "ScriptController.h"

#ifndef DISABLE_PARCHECK
#include "ParChecker.h"
#include "TwinCheck.h"
#include "ArticleFetcher.h"
#include "DupeMatcher.h"
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
		/* removes the twin's par2-files fetched for the par-check */
		void DeleteTwinFiles();
	private:
		std::vector<TwinCheck::NzbEntry> m_twinEntries;
		std::string m_twinName;
		std::string m_twinIndex;	// the entry fetched first (its filename)
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

	class PostDupeMatcher: public DupeMatcher
	{
	public:
		PostDupeMatcher(PostInfo* postInfo):
			DupeMatcher(postInfo->GetNzbInfo()->GetDestDir(),
				postInfo->GetNzbInfo()->GetSize() - postInfo->GetNzbInfo()->GetParSize()),
			m_postInfo(postInfo) {}
	protected:
		virtual void PrintMessage(Message::EKind kind, const char* format, ...) PRINTF_SYNTAX(3);
	private:
		PostInfo* m_postInfo;
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

#include "DupeCoordinator.h"
#include "ParParser.h"
#include "Options.h"
#include "WorkState.h"
#include "DiskState.h"
#include "Log.h"
#include "FileSystem.h"
#include <fstream>
#include <iterator>
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
	// read under the queue lock and copied out: the par-check runs while other par
	// volumes complete (appending to the list) or get renamed; the file state is
	// loaded from disk after the lock is released
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
	// a copy (per thread, valid until its next call): the list's string can be
	// replaced by a rename while the caller reads it
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
		PostDupeMatcher dupeMatcher(m_postInfo);
		PrintMessage(Message::mkInfo, "Checking %s for dupe scan usability", m_postInfo->GetNzbInfo()->GetName());
		bool sizeComparisonPossible = dupeMatcher.Prepare();
		for (NzbInfo* dupeNzbInfo : dupeList)
		{
			if (sizeComparisonPossible)
			{
				PrintMessage(Message::mkInfo, "Checking %s for dupe scan usability", FileSystem::BaseFileName(dupeNzbInfo->GetDestDir()));
			}
			bool useDupe = !sizeComparisonPossible ||
				dupeMatcher.MatchDupeContent(dupeNzbInfo->GetDestDir());
			if (useDupe)
			{
				PrintMessage(Message::mkInfo, "Adding %s to dupe scan sources", FileSystem::BaseFileName(dupeNzbInfo->GetDestDir()));
				dupeSourceList->emplace_back(dupeNzbInfo->GetId(), dupeNzbInfo->GetDestDir());
			}
		}
		if (dupeSourceList->empty())
		{
			PrintMessage(Message::mkInfo, "No usable dupe scan sources found");
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
	// a name of its own: the twin's par2-files may be named like ours
	NzbgetRsBuf nameBuf = nzbget_rs_repair_build_twin_filename(
		entry.filename.c_str(), m_twinFiles.size());
	std::string name(nameBuf.data ? nameBuf.data : "", nameBuf.len);
	nzbget_rs_free(nameBuf);
	NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		GetDestDir(), PATH_SEPARATOR, name.c_str());
	BString<1024> path;
	path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
	nzbget_rs_free(pathBuf);
	{
		std::ofstream out(fs::u8path(*path), std::ios::binary | std::ios::trunc);
		out.write(data.data(), (std::streamsize)data.size());
		if (!out)
		{
			return "";
		}
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
	// the files of our set, by the par2-file the check runs on
	std::vector<TwinCheck::FileSig> own;
	{
		std::ifstream in(fs::u8path(GetParFilename()), std::ios::binary);
		std::vector<char> data((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
		own = TwinCheck::ParsePar2(data.data(), data.size());
	}
	std::string ownPrint = TwinCheck::Fingerprint(own);
	if (ownPrint.empty())
	{
		return "";
	}

	// duplicates of its key whose fingerprint is ours, then those not fingerprinted yet
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
			// another encode, or the same files under other names (par2 would rename
			// ours to them): not used
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
	// the smallest volumes that hold enough blocks: by their names, or by their sizes
	// (a recovery packet is a block and 68 bytes; nzb sizes are encoded)
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

	// the fewest bytes: the smallest volume that covers what's left, else the
	// biggest one and go on
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


inline void RepairController::PostDupeMatcher::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, 1024, format, args);
	va_end(args);
	text[1024-1] = '\0';

	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
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
	// stream repair rewrites file contents without updating the download-time
	// CRCs and segment states ParQuick judges files by: once it has written
	// into the collection (live or in post-processing, before or after a
	// restart), only a full verification sees the repaired bytes
	m_parChecker.SetParQuick(nzbget_rs_repair_should_use_par_quick(
		g_Options->GetParQuick() ? 1 : 0,
		m_postInfo->GetForceParFull() ? 1 : 0,
		dupeRepaired ? 1 : 0) != 0);
	m_parChecker.SetForceRepair(m_postInfo->GetForceRepair());

	m_parChecker.PrintMessage(Message::mkInfo, "Checking pars for %s", *nzbName);

	m_parChecker.Execute();
}

/**
 * DownloadQueue must be locked prior to call of this function.
 */
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

	// Update ParStatus (accumulate result)
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

/**
* Unpause par2-files
* returns true, if the files with required number of blocks were unpaused,
* or false if there are no more files in queue for this collection or not enough blocks.
* special case: returns true if there are any unpaused par2-files in the queue regardless
* of the amount of blocks; this is to keep par-checker wait for download completion.
*/
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

		// unpause files with blocks
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

	// extract base name from m_szParFilename (trim .par2-extension and possible .vol-part)
	char* baseParFilename = FileSystem::BaseFileName(parFilename);
	int mainBaseLen = 0;
	if (!ParParser::ParseParFilename(baseParFilename, true, &mainBaseLen, nullptr))
	{
		// should not happen
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

			// if it is a par2-file with blocks and it was from the same NZB-request
			// and it belongs to the same file collection (same base name),
			// then OK, we can use it
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

		// wait until Post-processor is unpaused
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

#endif
