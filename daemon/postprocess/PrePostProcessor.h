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
#include "DownloadInfo.h"

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
#include "ScriptController.h"
#include "ServerPool.h"

#ifndef DISABLE_PARCHECK
#include "ParChecker.h"
#include "DupeArticleFallback.h"
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
#include "Options.h"
#include "Log.h"
#include "HistoryCoordinator.h"
#include "QueueCoordinator.h"
#include "DupeCoordinator.h"
#include "ExtensionManager.h"
#include "Util.h"
#include "FileSystem.h"
#include "UnpackController.h"
#include "DiskState.h"
#include "StatMeter.h"
#include "DupeArticleFallback.h"
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
