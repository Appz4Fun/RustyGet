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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef STREAMREPAIR_H
#define STREAMREPAIR_H

#include <atomic>
#include <condition_variable>
#include <deque>
#include <map>
#include <mutex>
#include <set>
#include <thread>
#include <vector>
#include "NString.h"
#include "DownloadInfo.h"
#include "ScriptController.h"
#include "ArticleFetcher.h"

class DiskFile;

/*
 * Post-processing stage for option <DupeArticleFallback> value "stream":
 * fills the missing byte ranges of completed media files (captured as
 * StreamRepairJobs at download completion) with bytes fetched from duplicate
 * postings of the same content, regardless of how the duplicate was split
 * into articles. Content identity is verified by byte-comparing fetched
 * donor articles against already-downloaded regions before anything is
 * written; par-check remains the backstop for whatever stays missing.
 *
 * With option value "live" the same controller additionally runs in LIVE
 * mode, download-concurrent (StartLive, lifecycle mirroring DirectUnpack):
 * dispatched when a file completes with holes while the collection still
 * downloads, it repairs the holes of already-completed files immediately
 * instead of waiting for post-processing. A live pass only writes bytes and
 * shrinks the jobs' hole lists (each job gets one live attempt); it never
 * credits health, never drains the job list and skips the decompression
 * rung - the post-processing stage remains the sole accounting authority
 * and the final repair word. The live target files are COMPLETED files, so
 * there is no concurrent writer on them; the continuing download only
 * writes other files of the collection.
 */
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
		CString Filename;		// current on-disk base name (in DestDir)
		// 0 for a file none of whose articles arrived: nothing is on disk,
		// the whole file is the hole and its size is learned from the first
		// donor article (RepairWholeFile); Holes is empty until then
		int64 DecodedFileSize;
		StreamRangeList Holes;
		// true while the target still needs bytes: declared holes, or a
		// whole-file target that was not sized yet
		bool NeedsRepair() const { return !Holes.empty() || DecodedFileSize <= 0; }
		// the file's ENCODED size and article count from its own nzb-file
		// (whole-file donor pairing has nothing else to go by)
		int64 EncodedSize = 0;
		int ArticleCount = 0;
		// a whole-file target that was allocated on disk in this pass
		bool Recreated = false;
		// the file's ENCODED failed size and par2 flag, captured from the
		// StreamRepairJob (the source of truth). When this target is FULLY
		// repaired its FailedSize is credited back to health, exactly reversing
		// the file's contribution to m_currentFailedSize (and, for a par2 file,
		// m_parCurrentFailedSize)
		int64 FailedSize = 0;
		int64 MissedSize = 0;
		int FailedArticles = 0;
		bool IsParFile = false;
		// false in a live pass for jobs that already had their one live
		// attempt: their holes stay DECLARED (cross-pack member universe and
		// probe selection must know them - treating them as present bytes
		// would parse zero-fill as headers and fail donor verification) but
		// no further patches are attempted on them until post-processing
		bool PatchEligible = true;
		// rank among same-size members (by name) and that window's size,
		// for positional donor pairing of obfuscated reposts; -1/0 = none
		int PositionalRank = -1;
		int PositionalWindow = 0;
		// article-size step fingerprint of the target's posting, 0 = none
		uint64 StepsHash = 0;
	};

	struct DonorSource
	{
		CString QueuedFilename;
		CString InfoName;
		// the donor's own archive password (its *Unpack:Password parameter,
		// captured from the LIVE donor NzbInfo: re-parsing the .nzb would lose
		// passwords set at intake or via the API). A secret - never logged.
		CString Password;
	};

	// null in live mode: progress labels are skipped and the NzbInfo is
	// re-found by id under the DownloadQueue lock at every locked touchpoint
	PostInfo* m_postInfo = nullptr;
	bool m_liveMode = false;
	int m_nzbId = 0;
	// option <DupeStreamTimeout>: a watchdog stops the pass once it recovered
	// nothing for that long; unlike a stop for shutdown, the pass then
	// finishes as a normal one (what is still missing goes to par-repair,
	// nothing is kept to resume). m_progressBytes is what it watches. It also
	// stops a repair that recovers so slowly that the rest would take more
	// than SlowRepairFactor times as long as downloading it at nzbget's
	// average download speed (checked once the pass ran half the timeout):
	// another release is quicker then. Whatever its progress, a pass stops
	// once it ran MaxRunFactor times the timeout (5 minutes at the default 60).
	static constexpr int SlowRepairFactor = 3;
	static constexpr int MaxRunFactor = 5;
	// the most a pass waits for news-server connections in all (B76): beyond the
	// MaxRunFactor cap of its working time, it ends then
	static constexpr int MaxConnectionWaitSec = 1800;
	std::thread m_watchdog;
	std::mutex m_watchdogMutex;
	std::condition_variable m_watchdogCond;
	bool m_watchdogDone = false;
	std::atomic<bool> m_timedOut{false};
	std::atomic<int64> m_progressBytes{0};
	int64 m_missingBytes = 0;
	CString m_slowReason;
	// a duplicate none of whose sampled articles exists on any server is
	// skipped (checked once per pass, within DonorCheckSec)
	static constexpr int DonorCheckSec = 20;
	std::map<std::string, bool> m_donorDead;
	// donor checks running: their connection waits don't count against the watchdog
	std::atomic<int> m_checkingDonor{0};
	// fetching and extracting a compressed duplicate (the decompression rung):
	// work without patched bytes, not a stall
	std::atomic<int> m_materializing{0};
	// this repair's fetches waiting for a free connection (B76: not a lack of progress)
	std::atomic<int> m_waitingForConnection{0};
	bool DonorDead(const DonorSource& donor, NzbInfo* donorNzb);
	void StartWatchdog(const std::vector<RepairTarget>& targets);
	void StopWatchdog(const char* nzbName, const std::vector<RepairTarget>& targets);
	/* stopped for a reload or shutdown (the pass is to run again), not by the timeout */
	bool Interrupted() { return IsStopped() && !m_timedOut; }
	ArticleFetcher m_fetcher;
	ArticleBatchFetcher m_batchFetcher;
	// the target's own archive password (*Unpack:Password); never logged
	CString m_targetPassword;
	int m_recoveredArticles = 0;
	int64 m_recoveredBytes = 0;
	int m_recoveredHoles = 0;
	// health is byte-based in ENCODED (bytes=) units, so it is credited per
	// FULLY-repaired file by that file's encoded FailedSize (not by decoded
	// recovered bytes): m_recoveredFailedSize is the total encoded failed size
	// of fully-repaired targets, m_recoveredFailedParSize its par2 subset
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
	/* Marks the completed-file record of a recreated whole-file target as
	 * partially downloaded, so later stages read it from disk. */
	static void MarkRecreated(NzbInfo* nzbInfo, const RepairTarget& target);
	/* Marks the completed-file record of a target whose every hole is
	 * filled as complete, the moment its failed size is credited back to the
	 * collection's health: a later "retry failed articles" must neither
	 * download it again nor subtract that failed size a second time. */
	static void MarkRepaired(NzbInfo* nzbInfo, const RepairTarget& target);

	// cross-packing (M2/M3) and decompression-assisted donor extraction (M4):
	// repairs holes through Rust inner-content maps after the per-member
	// same-bytes pass (M1) exhausted all donors
	void ExecCrossPackRepair(const char* destDir, std::vector<RepairTarget>& targets,
		std::vector<DonorSource>& donors, const std::vector<CString>& memberNames);

	void ReportRemainingHoles(std::vector<RepairTarget>& targets);

	void RepairCompleted();
};

#include <algorithm>
#include <chrono>
#include <exception>
#include <set>
#include "DupeArticleFallback.h"
#include "DupeCoordinator.h"
#include "DupeProbe.h"
#include "ServerPool.h"
#include "StatMeter.h"
#include "DiskState.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

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
	// Parity may already cover every hole. Download-time recovery cannot
	// establish that it is insufficient; defer to the normal PAR stage.
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

/*
 * Download-concurrent repair pass (option <DupeArticleFallback> value
 * "live"): repairs the holes of the collection's already-completed files
 * while the remaining files still download. Every locked touchpoint re-finds
 * the NzbInfo by id and verifies this thread is still attached, so a deleted
 * collection (detached via PrePostProcessor::NzbDeleted) aborts the pass
 * without touching freed state. See the class comment for what a live pass
 * deliberately does NOT do.
 */
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

		// each job gets exactly ONE live attempt: donors are static, so
		// re-fetching for holes an earlier live pass could not fill would only
		// re-pay the fetch traffic; whatever remains is retried once more by
		// the post-processing pass with the complete member universe. Already
		// attempted jobs stay in the target list with their holes DECLARED
		// (PatchEligible=false): the cross-pack member universe must know
		// their holes, or their zero-filled ranges would be treated as valid
		// present bytes and poison map building and donor verification
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
			// completed files may have been renamed since the pass started
			// (e.g. direct rename after its trigger par2 arrived): re-resolve
			// the on-disk names so the member universe is built against them
			RefreshLiveNames(targets, memberNames);
			ExecCrossPackRepair(destDir, targets, donors, memberNames);
		}
		StopWatchdog(nzbName, targets);
	}

	RepairCompletedLive(targets);
}

/*
 * Re-resolves the targets' and member universe's on-disk names from the
 * live CompletedFiles records (live mode only): direct rename can rename
 * completed files while the pass runs, and repairing under a stale name
 * would consume the job's one live attempt with zero work.
 */
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

/*
 * Live-pass epilogue: shrink the jobs' hole lists to what is still missing
 * and add the recovered bytes/holes statistics, then detach. Deliberately
 * NO health credit, NO job draining, NO par-check request - the
 * post-processing stage stays the sole accounting authority (its per-file
 * credit gate then simply finds the holes already empty). Must not print
 * messages while holding the lock (AddMessage re-acquires it in live mode).
 */
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
		// detached mid-pass (shutdown): no job state may be written back, but
		// the bytes already patched on disk must still be recorded, or a later
		// par-check would judge those files by their download-time state
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
			// declared-only target: its job's holes were never touched
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
		// the download completed while this pass ran and the post-processing
		// job is waiting for it (see PrePostProcessor::NzbDownloaded). The
		// working flag is shared with direct unpack: when that thread is
		// still attached it stays the flag's owner and clears it on its own
		// exit (which symmetrically leaves the flag alone while this pass is
		// attached - see DirectUnpack's epilogue)
		nzbInfo->GetPostInfo()->SetWorking(false);
	}

	// jobs (and their shrunk holes) are persisted; keep disk-state honest
	downloadQueue->Save();
}

inline bool StreamRepairController::DonorDead(const DonorSource& donor, NzbInfo* donorNzb)
{
	auto it = m_donorDead.find(*donor.QueuedFilename);
	if (it != m_donorDead.end())
	{
		return it->second;
	}

	// data files before par2 files, as the dead-pick probe samples a posting
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
		// the check waits for connections too: not a lack of progress (B76)
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

	// what the pass has to recover: the holes, and the whole files not sized yet
	m_missingBytes = 0;
	for (const RepairTarget& target : targets)
	{
		m_missingBytes += target.DecodedFileSize > 0
			? DupeStreamRepair::TotalSize(target.Holes)
			: target.EncodedSize;
	}

	// nzbget's average download speed
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
			auto begun = start;	// not shifted by waits: bounds them
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
					// waiting for a connection the queue's downloads hold is neither a
					// lack of progress nor running time (B76)
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
	// the target's own archive password enables encrypted store-rar target
	// sets to map (M3); an empty parameter means plain M2 behavior
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
		// Older queue states may contain PAR2 jobs captured before duplicate
		// recovery excluded them. Never use another release to patch parity.
		if (nzbget_rs_stream_is_par2_job_or_file(
			job.GetParFile() ? 1 : 0, job.GetFilename()) != 0)
		{
			continue;
		}
		RepairTarget target;
		target.FileId = job.GetFileId();
		target.Filename = job.GetFilename();
		target.DecodedFileSize = job.GetDecodedFileSize();
		// encoded failed size and par2 flag come from the job (source of truth,
		// captured while the FileInfo was still alive at job creation)
		target.FailedSize = job.GetFailedSize();
		target.MissedSize = job.GetMissedSize();
		target.FailedArticles = job.GetFailedArticles();
		target.IsParFile = job.GetParFile();
		target.Holes = *job.GetHoles();
		target.StepsHash = job.GetStepsHash();
		target.EncodedSize = job.GetFailedSize() + job.GetMissedSize();
		target.ArticleCount = job.GetFailedArticles();

		// par-rename may have renamed the file since capture; the
		// completed-file record tracks the current on-disk name
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
	// a target's rank = how many same-size members sort before it by name;
	// the window size (incl. the target itself) must match the donor side's
	// for positional pairing to fire (the set-signature check). Complete
	// members' sizes come from disk (DirectWrite pre-allocates full size; a
	// join-mode short file drops out of the window, degrading to later tiers)
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
		// an exact copy of the same posting shares the message-ids and cannot
		// supply articles that already failed everywhere
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
			// the donor's password must come from the live NzbInfo (queue or
			// history - both persist parameters): the later ParseDonorNzb
			// re-parse of the raw .nzb cannot see intake- or API-set passwords
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

		// parse lazily: a donor nzb is only read once a previous donor left
		// holes unfilled (file I/O and XML parsing happen unlocked)
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
			// the article state kept for an incomplete file is no longer needed
			// (and DiskState::DiscardFiles skips complete files); its download-
			// time CRC is unknown, so a quick par-check falls back to verifying
			// the file in full
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
		// a reload or shutdown interrupted the pass, which says nothing about
		// what the duplicates carry: the pass runs again after the restart (for
		// the unfinished files only - the finished ones are marked repaired and
		// their jobs dropped, so they're credited below, now or never)
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
		// exactly the targets ReportRemainingHoles credited back to health
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

	// restore byte-based health in ENCODED (bytes=) units - the units
	// m_currentFailedSize is kept in. For each FULLY-repaired file we credit
	// its exact encoded FailedSize (accumulated in ReportRemainingHoles),
	// which precisely reverses that file's contribution to m_currentFailedSize
	// (DownloadInfo.cpp:675) and, for a par2 file, m_parCurrentFailedSize
	// (:687). So a fully-repaired par-less release reaches m_currentFailedSize
	// == 0 -> CalcHealth() 1000 -> completes as SUCCESS; a partially-repaired
	// file credits nothing, leaving m_currentFailedSize > 0 -> health < 1000,
	// so the release still parks (never a false SUCCESS - crediting per
	// fully-repaired file, not per decoded byte, is the correctness and safety
	// argument). Clamped at 0. When par2 exists SetRequestParCheck below stays
	// authoritative.
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

	// Persist unfinished holes only when the controller is stopping so a
	// restart can resume. During a normal run, unfinished targets are handed to
	// par-repair and must not re-arm this stage indefinitely.
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
			// A job without a matching target is retained conservatively.
			++it;
		}
	}

	// bytes written by THIS pass: m_recoveredArticles also credits targets an
	// earlier live pass already completed. A par-check that judged those has
	// verified them in full (ParQuick is off once stream repair wrote bytes)
	bool recovered = m_recoveredBytes > 0;
	m_postInfo->SetStreamRepairDone(true);
	m_postInfo->SetStreamRepairRecovered(recovered);

	// whatever was written - and whatever is still missing - goes through
	// par-check as final verification (a no-op when no par2 files exist)
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

	// Persist repaired counters and any remaining holes before leaving the
	// stage, so a graceful stop cannot lose the current repair state.
	downloadQueue->Save();

	m_postInfo->SetWorking(false);
}

inline void StreamRepairController::AddMessage(Message::EKind kind, const char* text)
{
	if (m_liveMode)
	{
		// no PostInfo in a live pass: find the collection by id (it may have
		// been deleted mid-pass, then the message is dropped). Callers must
		// not hold the DownloadQueue lock when printing in live mode.
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

#endif
