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


#include "nzbget.h"

#include <algorithm>
#include <chrono>
#include <exception>
#include <set>
#include "StreamRepair.h"
#include "StreamCrypto.h"
#include "DupeStreamRepair.h"
#include "DupeArticleFallback.h"
#include "DupeCoordinator.h"
#include "DupeProbe.h"
#include "NewsServer.h"
#include "ServerPool.h"
#include "StatMeter.h"
#include "DiskState.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "Unpack.h"
#include "ParParser.h"
#include "nzbget_rs.h"

namespace
{
	// a short write to a full disk may leave errno unset
	CString WriteErrorMessage()
	{
		CString message = FileSystem::GetLastErrorMessage();
		if (message.Empty())
		{
			return "incomplete write (disk full?)";
		}
		return message;
	}

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

	struct DonorTransportCtx
	{
		ArticleFetcher* Fetcher;
		FileInfo* DonorFile;
		int* FetchBudget;
		ArticleBatchFetcher* BatchFetcher;
		ArticleFetcher::FetchedArticle CurrentFetched;
		std::exception_ptr Error;
	};

	int DonorFetchPartCb(
		void* rawCtx,
		int32_t partIndex,
		int* outSuccess,
		int64_t* outOffset,
		int64_t* outFileSize,
		const uint8_t** outData,
		size_t* outDataLen) noexcept
	{
		return RepairMapCall<DonorTransportCtx>(rawCtx, 0, [=](DonorTransportCtx* c)
		{
			if (!c->DonorFile || partIndex < 0 ||
				(size_t)partIndex >= c->DonorFile->GetArticles()->size())
			{
				return 0;
			}
			if (c->FetchBudget)
			{
				if (*c->FetchBudget <= 0)
				{
					return 0;
				}
				(*c->FetchBudget)--;
			}
			ArticleInfo* article = (*c->DonorFile->GetArticles())[partIndex].get();
			c->CurrentFetched = c->Fetcher->Fetch(
				article->GetMessageId(), *c->DonorFile->GetGroups());
			*outSuccess = c->CurrentFetched.Success ? 1 : 0;
			*outOffset = c->CurrentFetched.Offset;
			*outFileSize = c->CurrentFetched.FileSize;
			*outData = reinterpret_cast<const uint8_t*>(c->CurrentFetched.Data.data());
			*outDataLen = c->CurrentFetched.Data.size();
			return 1;
		});
	}

	void DonorBeginBatchCb(void* rawCtx, const int32_t* partIndices, size_t count) noexcept
	{
		RepairMapCallVoid<DonorTransportCtx>(rawCtx, [=](DonorTransportCtx* c)
		{
			if (!c->BatchFetcher || !c->DonorFile)
			{
				return;
			}
			auto groups = std::make_shared<std::vector<CString>>();
			for (const CString& group : *c->DonorFile->GetGroups())
			{
				groups->emplace_back(*group);
			}
			std::vector<ArticleBatchFetcher::Request> requests;
			requests.reserve(count);
			for (size_t i = 0; i < count; i++)
			{
				int partIndex = partIndices[i];
				requests.push_back({
					(*c->DonorFile->GetArticles())[partIndex]->GetMessageId(),
					groups
				});
			}
			c->BatchFetcher->Begin(std::move(requests));
		});
	}

	int DonorNextBatchCb(
		void* rawCtx,
		int* outSuccess,
		int64_t* outOffset,
		int64_t* outFileSize,
		const uint8_t** outData,
		size_t* outDataLen) noexcept
	{
		return RepairMapCall<DonorTransportCtx>(rawCtx, 0, [=](DonorTransportCtx* c)
		{
			if (!c->BatchFetcher)
			{
				return 0;
			}
			c->CurrentFetched = ArticleFetcher::FetchedArticle();
			if (!c->BatchFetcher->Next(c->CurrentFetched))
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
	}

	void DonorCancelBatchCb(void* rawCtx) noexcept
	{
		RepairMapCallVoid<DonorTransportCtx>(rawCtx, [](DonorTransportCtx* c)
		{
			if (c->BatchFetcher)
			{
				c->BatchFetcher->CancelRemaining();
			}
		});
	}
}

ContentSource* DiskSourceSet::GetSource(int memberIndex)
{
	if (memberIndex < 0 || (size_t)memberIndex >= m_entries.size())
	{
		return nullptr;
	}
	Entry& entry = m_entries[memberIndex];
	if (!entry.Tried)
	{
		entry.Tried = true;
		NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			*m_destDir, PATH_SEPARATOR, m_members[memberIndex].Name.c_str());
		BString<1024> path;
		path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
		nzbget_rs_free(pathBuf);
		// a member's size may exceed the file on disk: a target joined from
		// article files ends with its last downloaded article, and the ranges
		// past it are declared holes (HoledSourceSet), never read from here
		int64 size = std::max(FileSystem::FileSize(path), m_members[memberIndex].Size);
		if (size > 0 && entry.File.Open(path, DiskFile::omRead))
		{
			entry.Source = std::make_unique<DiskContentSource>(entry.File, size);
		}
	}
	return entry.Source.get();
}

DonorMemberSource::DonorMemberSource(ArticleFetcher& fetcher, FileInfo* donorFile) :
	m_fetcher(fetcher), m_donorFile(donorFile)
{
	std::vector<int32_t> articleSizes;
	if (donorFile && donorFile->GetArticles())
	{
		articleSizes.reserve(donorFile->GetArticles()->size());
		for (ArticleInfo* article : donorFile->GetArticles())
		{
			articleSizes.push_back(article->GetSize());
		}
	}
	m_inner = nzbget_rs_donor_member_source_new(articleSizes.data(), articleSizes.size());
}

DonorMemberSource::~DonorMemberSource()
{
	nzbget_rs_donor_member_source_free(m_inner);
}

int DonorMemberSource::TakeServedParts()
{
	return nzbget_rs_donor_member_source_take_served_parts(m_inner);
}

int64 DonorMemberSource::Size()
{
	DonorTransportCtx ctx{&m_fetcher, m_donorFile, m_fetchBudget, m_batchFetcher, {}, nullptr};
	int64 size = nzbget_rs_donor_member_source_size(m_inner, &ctx, DonorFetchPartCb);
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return size;
}

bool DonorMemberSource::Read(int64 offset, void* buffer, int64 size)
{
	if (size < 0)
	{
		return false;
	}
	DonorTransportCtx ctx{&m_fetcher, m_donorFile, m_fetchBudget, m_batchFetcher, {}, nullptr};
	int allowPrefetch = (m_batchFetcher && !m_fetchBudget) ? 1 : 0;
	bool ok = nzbget_rs_donor_member_source_read(
		m_inner,
		offset,
		static_cast<uint8_t*>(buffer),
		size,
		allowPrefetch,
		&ctx,
		DonorFetchPartCb,
		DonorBeginBatchCb,
		DonorNextBatchCb,
		DonorCancelBatchCb) != 0;
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return ok;
}

DonorSetSources::DonorSetSources(ArticleFetcher& fetcher, NzbInfo* donorNzb) :
	m_fetcher(fetcher)
{
	for (FileInfo* fileInfo : donorNzb->GetFileList())
	{
		m_files.push_back(fileInfo);
	}
	m_sources.resize(m_files.size());
}

DonorMemberSource* DonorSetSources::GetDonorSource(int memberIndex)
{
	if (memberIndex < 0 || (size_t)memberIndex >= m_files.size())
	{
		return nullptr;
	}
	if (!m_sources[memberIndex])
	{
		m_sources[memberIndex] = std::make_unique<DonorMemberSource>(
			m_fetcher, m_files[memberIndex]);
		m_sources[memberIndex]->SetFetchBudget(m_fetchBudget);
		m_sources[memberIndex]->SetBatchFetcher(m_batchFetcher);
	}
	return m_sources[memberIndex].get();
}

std::vector<SetMember> DonorSetSources::BuildMembers()
{
	std::vector<SetMember> members;
	for (FileInfo* fileInfo : m_files)
	{
		members.push_back({FileSystem::BaseFileName(fileInfo->GetFilename()),
			fileInfo->GetSize()});
	}
	return members;
}

void DonorSetSources::SetFetchBudget(int* budget)
{
	m_fetchBudget = budget;
	for (std::unique_ptr<DonorMemberSource>& source : m_sources)
	{
		if (source)
		{
			source->SetFetchBudget(budget);
		}
	}
}

void DonorSetSources::SetBatchFetcher(ArticleBatchFetcher* batchFetcher)
{
	m_batchFetcher = batchFetcher;
	for (std::unique_ptr<DonorMemberSource>& source : m_sources)
	{
		if (source)
		{
			source->SetBatchFetcher(batchFetcher);
		}
	}
}

int DonorSetSources::TakeServedParts()
{
	int total = 0;
	for (std::unique_ptr<DonorMemberSource>& source : m_sources)
	{
		if (source)
		{
			total += source->TakeServedParts();
		}
	}
	return total;
}

DiskFile* TargetSetFiles::GetFile(int memberIndex)
{
	auto existing = m_files.find(memberIndex);
	if (existing != m_files.end())
	{
		return existing->second && existing->second->Active() ?
			existing->second.get() : nullptr;
	}
	std::unique_ptr<DiskFile> file = std::make_unique<DiskFile>();
	NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		*m_destDir, PATH_SEPARATOR, m_members[memberIndex].Name.c_str());
	BString<1024> path;
	path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
	nzbget_rs_free(pathBuf);
	if (!file->Open(path, DiskFile::omReadWrite))
	{
		file.reset();
	}
	// take the pointer from the map entry that now owns the file, not from the
	// moved-from local (which also reads as a dangling return to static analysis)
	auto inserted = m_files.emplace(memberIndex, std::move(file));
	return inserted.first->second.get();
}

void StreamRepairController::StartJob(PostInfo* postInfo)
{
	StreamRepairController* streamRepairController = new StreamRepairController();
	streamRepairController->m_postInfo = postInfo;
	streamRepairController->SetAutoDestroy(false);

	postInfo->SetPostThread(streamRepairController);

	streamRepairController->Start();
}

void StreamRepairController::StartLive(NzbInfo* nzbInfo)
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

void StreamRepairController::Run()
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
void StreamRepairController::RunLive()
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
void StreamRepairController::RefreshLiveNames(std::vector<RepairTarget>& targets,
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

void StreamRepairController::RefreshLiveTargetName(RepairTarget& target)
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
void StreamRepairController::RepairCompletedLive(std::vector<RepairTarget>& targets)
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

bool StreamRepairController::DonorDead(const DonorSource& donor, NzbInfo* donorNzb)
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

void StreamRepairController::StartWatchdog(const std::vector<RepairTarget>& targets)
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

void StreamRepairController::StopWatchdog(const char* nzbName, const std::vector<RepairTarget>& targets)
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

void StreamRepairController::Stop()
{
	m_batchFetcher.Stop();
	m_fetcher.Stop();
	Thread::Stop();
}

void StreamRepairController::CollectTargets(NzbInfo* nzbInfo, std::vector<RepairTarget>& targets,
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

void StreamRepairController::ComputePositionalRanks(const char* destDir,
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

void StreamRepairController::CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo,
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

void StreamRepairController::ExecRepair(const char* destDir,
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

		int consecutiveFailures = 0;
		bool donorProven = false;
		// donor members already proven identical to a target: one member
		// cannot be another target's twin, so they are never probed again
		std::set<FileInfo*> claimed;

		// archive sets of this release proven byte-identical to an archive set
		// of the duplicate: (VolumeSetKey of ours, VolumeSetKey of theirs).
		// A whole volume is recreated only from a set proven this way - an
		// identical sample or .nfo proves nothing about the archive volumes
		// of two different packings
		std::set<std::pair<std::string, std::string>> provenSets;
		// pairs an intact-volume proof already failed for: every further
		// missing volume of the set would only repeat the same probe fetches
		std::set<std::pair<std::string, std::string>> disprovenSets;

		// whole-file targets wait for the end of the donor's pass: only the
		// other targets can prove the donor is a repost of this release
		for (int round = 0; round < 2; round++)
		{
			for (RepairTarget& target : targets)
			{
				if (IsStopped())
				{
					break;
				}
				bool wholeFile = target.DecodedFileSize <= 0;
				if (!target.PatchEligible || !target.NeedsRepair() || (wholeFile != (round == 1)))
				{
					continue;
				}
				bool setProven = false;
				if (wholeFile)
				{
					// each archive set of the duplicate in turn: the missing
					// volume pairs only within one set, and only a set proven
					// against the volume's own set may stand in for it
					std::string ourSet = DupeStreamRepair::VolumeSetKey(target.Filename);
					std::set<std::string> donorSets;
					for (FileInfo* donorFile : donorNzb->GetFileList())
					{
						std::string key = DupeStreamRepair::VolumeSetKey(donorFile->GetFilename());
						if (!key.empty())
						{
							donorSets.insert(key);
						}
					}
					for (const std::string& theirSet : donorSets)
					{
						if (ourSet.empty() || IsStopped())
						{
							break;
						}
						if (!DupeStreamRepair::SelectWholeFileDonor(FileSystem::BaseFileName(target.Filename),
							target.EncodedSize, target.ArticleCount, target.StepsHash, donorNzb.get(),
							&claimed, theirSet))
						{
							continue;
						}
						if (disprovenSets.count({ourSet, theirSet}))
						{
							continue;
						}
						if (provenSets.count({ourSet, theirSet}) ||
							ProveDonorOnIntactFile(destDir, target, targets, memberNames,
								donorNzb.get(), donor.InfoName, claimed, theirSet))
						{
							provenSets.insert({ourSet, theirSet});
							target.DonorSet = theirSet;
							setProven = true;
							break;
						}
						disprovenSets.insert({ourSet, theirSet});
					}
					if (!setProven)
					{
						continue;
					}
				}
				std::set<FileInfo*> claimedBefore = claimed;
				bool verified = false;
				ERepairOutcome outcome = RepairFile(destDir, target, donorNzb.get(), donor.InfoName,
					claimed, verified, setProven);
				// a byte-identical twin proves the donor is a repost of this
				// release even when it misses the same articles as the target
				donorProven |= verified;
				if (verified)
				{
					std::string ourSet = DupeStreamRepair::VolumeSetKey(target.Filename);
					for (FileInfo* member : claimed)
					{
						std::string theirSet = DupeStreamRepair::VolumeSetKey(member->GetFilename());
						if (!claimedBefore.count(member) && !ourSet.empty() && !theirSet.empty())
						{
							provenSets.insert({ourSet, theirSet});
						}
					}
				}
				if (nzbget_rs_stream_eval_donor_outcome_bail(
					(int)outcome, (int)roProductive, (int)roUnproductive,
					donorProven ? 1 : 0, consecutiveFailures,
					DonorFailureBail, UnprovenDonorBail, &consecutiveFailures) != 0)
				{
					PrintMessage(Message::mkInfo,
						"Skipping remaining files for duplicate %s (%i consecutive files without a byte-identical match)",
						*donor.InfoName, consecutiveFailures);
					round = 2;
					break;
				}
			}
		}
	}
}

StreamRepairController::ERepairOutcome StreamRepairController::RepairFile(const char* destDir,
	RepairTarget& target, NzbInfo* donorNzb, const char* donorName, std::set<FileInfo*>& claimed,
	bool& verified, bool donorProven)
{
	if (target.DecodedFileSize <= 0)
	{
		return RepairWholeFile(destDir, target, donorNzb, donorName, claimed, donorProven);
	}

	if (m_liveMode)
	{
		// direct rename can rename the completed file while this pass runs;
		// opening a stale name would consume the job's one live attempt with
		// zero work, so re-resolve it right before the open
		RefreshLiveTargetName(target);
	}

	NzbgetRsBuf filePathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		destDir, PATH_SEPARATOR, *target.Filename);
	BString<1024> filePath;
	filePath.Set(filePathBuf.data ? filePathBuf.data : "", (int)filePathBuf.len);
	nzbget_rs_free(filePathBuf);

	DiskFile file;
	if (!file.Open(filePath, DiskFile::omReadWrite))
	{
		PrintMessage(Message::mkWarning, "Could not open %s for stream repair: %s",
			*filePath, *FileSystem::GetLastErrorMessage());
		return roNoCost;
	}

	bool patched = false;
	bool spentFetches = false;
	int probed = 0;

	for (FileInfo* donorFile : FindDonorFiles(target, donorNzb, claimed))
	{
		if (IsStopped() || target.Holes.empty())
		{
			break;
		}
		// the wide sibling search is only for finding the target's twin; once
		// one member verified, further members get the usual small budget
		if (verified && probed >= DupeStreamRepair::MaxDonorCandidates)
		{
			break;
		}

		StreamRangeList donorRanges = DupeStreamRepair::EstimateDonorRanges(
			donorFile, target.DecodedFileSize);
		if (donorRanges.empty())
		{
			continue;
		}
		spentFetches = true;
		probed++;

		if (!VerifyDonor(file, target, donorFile, donorRanges))
		{
			// mkInfo on purpose: a rejected donor is the safety net firing -
			// the user (and the e2e decoy test) should see it in the log; the
			// sibling search past the usual budget would flood it, so detail
			PrintMessage(
				probed <= DupeStreamRepair::MaxDonorCandidates ? Message::mkInfo : Message::mkDetail,
				"Skipping file %s of duplicate %s for %s: content identity not confirmed",
				donorFile->GetFilename(), donorName, *target.Filename);
			continue;
		}

		verified = true;
		claimed.insert(donorFile);
		patched |= PatchFromDonor(file, target, donorFile, donorRanges, donorName) > 0;
	}

	file.Close();
	return patched ? roProductive : (spentFetches ? roUnproductive : roNoCost);
}

bool StreamRepairController::ProveDonorOnIntactFile(const char* destDir,
	const RepairTarget& wholeTarget, const std::vector<RepairTarget>& targets,
	const std::vector<CString>& memberNames, NzbInfo* donorNzb, const char* donorName,
	std::set<FileInfo*>& claimed, const std::string& donorSet)
{
	// the proof has to come from the same archive set on both sides: a
	// sibling volume of the missing one, and a sibling of the donor member
	// that would stand in for it
	std::string targetSet = DupeStreamRepair::VolumeSetKey(wholeTarget.Filename);
	FileInfo* standIn = DupeStreamRepair::SelectWholeFileDonor(FileSystem::BaseFileName(wholeTarget.Filename),
		wholeTarget.EncodedSize, wholeTarget.ArticleCount, wholeTarget.StepsHash, donorNzb, &claimed, donorSet);
	if (targetSet.empty() || donorSet.empty() || !standIn)
	{
		return false;
	}

	std::vector<const char*> targetFilenames;
	targetFilenames.reserve(targets.size());
	for (const RepairTarget& target : targets)
	{
		targetFilenames.push_back(target.Filename);
	}

	int tried = 0;
	for (const CString& memberName : memberNames)
	{
		if (IsStopped() || tried >= MaxIntactProofFiles)
		{
			break;
		}
		const char* baseName = FileSystem::BaseFileName(memberName);
		if (nzbget_rs_stream_is_intact_proof_member_eligible(
			baseName, memberName, targetSet.c_str(),
			targetFilenames.data(), targetFilenames.size()) == 0)
		{
			continue;
		}

		NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			destDir, PATH_SEPARATOR, *memberName);
		BString<1024> path;
		path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
		nzbget_rs_free(pathBuf);
		int64 size = FileSystem::FileSize(path);
		if (size <= 0)
		{
			continue;
		}
		DiskFile file;
		if (!file.Open(path, DiskFile::omRead))
		{
			continue;
		}
		tried++;

		// an intact file has no holes: every probe compares fully present bytes
		RepairTarget probe;
		probe.FileId = 0;
		probe.Filename = *memberName;
		probe.DecodedFileSize = size;
		// candidates from the donor set only: the member with the intact
		// file's volume number first, then the set's other members of a
		// similar size (a renamed repost may number differently)
		std::vector<FileInfo*> donorFiles;
		std::vector<const char*> donorNames;
		std::vector<int64_t> donorSizes;
		std::vector<uint8_t> donorHasArticles;
		std::vector<uint8_t> donorIsClaimed;
		int standInIndex = -1;
		for (FileInfo* donorFile : donorNzb->GetFileList())
		{
			if (donorFile == standIn)
			{
				standInIndex = (int)donorFiles.size();
			}
			donorFiles.push_back(donorFile);
			donorNames.push_back(donorFile->GetFilename());
			donorSizes.push_back(donorFile->GetSize());
			donorHasArticles.push_back(!donorFile->GetArticles()->empty() ? 1 : 0);
			donorIsClaimed.push_back(claimed.count(donorFile) ? 1 : 0);
		}
		std::vector<int32_t> candIndices(donorFiles.size());
		size_t candCount = nzbget_rs_stream_select_intact_proof_candidates(
			baseName, size, donorSet.c_str(), standInIndex, DupeStreamRepair::MaxDonorCandidates,
			donorNames.data(), donorSizes.data(), donorHasArticles.data(), donorIsClaimed.data(),
			donorFiles.size(), candIndices.data(), candIndices.size());
		std::vector<FileInfo*> candidates;
		candidates.reserve(candCount);
		for (size_t c = 0; c < candCount; c++)
		{
			candidates.push_back(donorFiles[candIndices[c]]);
		}
		for (FileInfo* donorFile : candidates)
		{
			if (IsStopped())
			{
				break;
			}
			StreamRangeList donorRanges = DupeStreamRepair::EstimateDonorRanges(donorFile, size);
			if (!donorRanges.empty() && VerifyDonor(file, probe, donorFile, donorRanges))
			{
				claimed.insert(donorFile);
				PrintMessage(Message::mkInfo,
					"Duplicate %s is byte-identical to this release (verified on intact file %s)",
					donorName, *memberName);
				return true;
			}
		}
	}
	return false;
}

StreamRepairController::ERepairOutcome StreamRepairController::RepairWholeFile(const char* destDir,
	RepairTarget& target, NzbInfo* donorNzb, const char* donorName, std::set<FileInfo*>& claimed,
	bool donorProven)
{
	// nothing of the file was downloaded, so no byte of it can confirm a
	// donor: only a donor already proven byte-identical on another file of
	// this collection, and a member identified by the file's own nzb entry
	// (not by size alone), may stand in for it
	if (!donorProven)
	{
		return roNoCost;
	}
	FileInfo* donorFile = DupeStreamRepair::SelectWholeFileDonor(
		FileSystem::BaseFileName(target.Filename), target.EncodedSize, target.ArticleCount,
		target.StepsHash, donorNzb, &claimed, target.DonorSet);
	if (!donorFile || donorFile->GetArticles()->empty())
	{
		return roNoCost;
	}

	// the decoded size: every yEnc article declares the size of its file, so
	// the first article that arrives sizes it (a repost may miss its start)
	auto donorGroups = std::make_shared<std::vector<CString>>();
	for (const CString& group : *donorFile->GetGroups())
	{
		donorGroups->emplace_back(*group);
	}
	ArticleList* donorArticles = donorFile->GetArticles();
	struct SizeProbeCtx
	{
		StreamRepairController* Controller;
		FileInfo* DonorFile;
		std::shared_ptr<std::vector<CString>> DonorGroups;
		ArticleFetcher::FetchedArticle CurrentFetched;
		std::exception_ptr Error;
	} probeCtx{this, donorFile, std::move(donorGroups), {}, nullptr};

	int64 decodedFileSize = 0;
	bool sized = nzbget_rs_stream_probe_whole_file_donor_size(
		donorArticles->size(),
		target.EncodedSize,
		&probeCtx,
		[](void* rawCtx) -> int
		{
			return RepairMapCall<SizeProbeCtx>(rawCtx, 1, [](SizeProbeCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, const int32_t* partIndices, size_t count)
		{
			RepairMapCallVoid<SizeProbeCtx>(rawCtx, [=](SizeProbeCtx* c)
			{
				std::vector<ArticleBatchFetcher::Request> requests;
				requests.reserve(count);
				for (size_t i = 0; i < count; i++)
				{
					ArticleInfo* article = (*c->DonorFile->GetArticles())[partIndices[i]].get();
					requests.push_back({article->GetMessageId(), c->DonorGroups});
				}
				c->Controller->m_batchFetcher.Begin(std::move(requests));
			});
		},
		[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
			const uint8_t** outData, size_t* outDataLen) -> int
		{
			return RepairMapCall<SizeProbeCtx>(rawCtx, 0, [=](SizeProbeCtx* c)
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
			RepairMapCallVoid<SizeProbeCtx>(rawCtx, [](SizeProbeCtx* c)
			{
				c->Controller->m_batchFetcher.CancelRemaining();
			});
		},
		&decodedFileSize) != 0;
	if (probeCtx.Error)
	{
		std::rethrow_exception(probeCtx.Error);
	}
	if (IsStopped())
	{
		return roNoCost;
	}
	if (!sized)
	{
		PrintMessage(Message::mkDetail,
			"Skipping file %s of duplicate %s for %s: could not learn the file size from it",
			donorFile->GetFilename(), donorName, *target.Filename);
		return roUnproductive;
	}

	NzbgetRsBuf filePathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		destDir, PATH_SEPARATOR, *target.Filename);
	BString<1024> filePath;
	filePath.Set(filePathBuf.data ? filePathBuf.data : "", (int)filePathBuf.len);
	nzbget_rs_free(filePathBuf);
	if (FileSystem::FileExists(filePath))
	{
		// None of the file's own articles arrived, so a file of its name and of the
		// recreated size is an earlier recreation a shutdown cut short (its job state
		// not written back): it is started over. Refused, it stayed a sparse, partly
		// zero file that every later pass reported as "no duplicate could supply"
		int64 existing = FileSystem::FileSize(filePath);
		if (existing != decodedFileSize && existing != 0)
		{
			PrintMessage(Message::mkWarning, "Could not recreate %s: file already exists", *filePath);
			return roNoCost;
		}
		PrintMessage(Message::mkInfo, "Recreating %s again: an earlier recreation was cut short", *filePath);
		FileSystem::DeleteFile(filePath);
	}
	CString errmsg;
	if (!FileSystem::AllocateFile(filePath, decodedFileSize, true, errmsg))
	{
		PrintMessage(Message::mkWarning, "Could not recreate %s: %s", *filePath, *errmsg);
		return roNoCost;
	}
	DiskFile file;
	if (!file.Open(filePath, DiskFile::omReadWrite))
	{
		PrintMessage(Message::mkWarning, "Could not open %s for stream repair: %s",
			*filePath, *FileSystem::GetLastErrorMessage());
		FileSystem::DeleteFile(filePath);
		return roNoCost;
	}

	PrintMessage(Message::mkInfo,
		"Recreating %s (no article of the file was available) from file %s of duplicate %s",
		*target.Filename, donorFile->GetFilename(), donorName);

	target.DecodedFileSize = decodedFileSize;
	target.Holes = { {0, decodedFileSize} };
	claimed.insert(donorFile);

	StreamRangeList donorRanges = DupeStreamRepair::EstimateDonorRanges(donorFile, decodedFileSize);
	int recoveredParts = PatchFromDonor(file, target, donorFile, donorRanges, donorName);
	file.Close();

	if (recoveredParts == 0)
	{
		// nothing arrived: leave no empty file behind, another duplicate may
		// still carry the file
		FileSystem::DeleteFile(filePath);
		target.DecodedFileSize = 0;
		target.Holes.clear();
		return roUnproductive;
	}

	target.Recreated = true;
	return roProductive;
}

void StreamRepairController::MarkRepaired(NzbInfo* nzbInfo, const RepairTarget& target)
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

void StreamRepairController::MarkRecreated(NzbInfo* nzbInfo, const RepairTarget& target)
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

std::vector<FileInfo*> StreamRepairController::FindDonorFiles(const RepairTarget& target,
	NzbInfo* donorNzb, const std::set<FileInfo*>& claimed)
{
	// donor NZBs list flat names, while a par-renamed target may carry a path
	// relative to the destination directory (e.g. "BDMV/STREAM/00000.m2ts")
	return DupeStreamRepair::SelectDonorCandidates(FileSystem::BaseFileName(target.Filename),
		target.DecodedFileSize,
		target.PositionalRank, target.PositionalWindow, donorNzb,
		DupeStreamRepair::MaxSiblingCandidates, target.StepsHash, &claimed);
}

bool StreamRepairController::VerifyDonor(DiskFile& file, const RepairTarget& target,
	FileInfo* donorFile, const StreamRangeList& donorRanges)
{
	auto donorGroups = std::make_shared<std::vector<CString>>();
	for (const CString& group : *donorFile->GetGroups())
	{
		donorGroups->emplace_back(*group);
	}

	struct VerifyFileCtx
	{
		StreamRepairController* Controller;
		DiskFile* File;
		FileInfo* DonorFile;
		std::shared_ptr<std::vector<CString>> DonorGroups;
		ArticleFetcher::FetchedArticle CurrentFetched;
		std::exception_ptr Error;
	} ctx{this, &file, donorFile, std::move(donorGroups), {}, nullptr};

	bool ok = nzbget_rs_stream_verify_donor_file(
		target.DecodedFileSize,
		reinterpret_cast<const NzbgetRsByteRange*>(target.Holes.data()),
		target.Holes.size(),
		reinterpret_cast<const NzbgetRsByteRange*>(donorRanges.data()),
		donorRanges.size(),
		&ctx,
		[](void* rawCtx) -> int
		{
			return RepairMapCall<VerifyFileCtx>(rawCtx, 1, [](VerifyFileCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, const int32_t* partIndices, size_t count)
		{
			RepairMapCallVoid<VerifyFileCtx>(rawCtx, [=](VerifyFileCtx* c)
			{
				std::vector<ArticleBatchFetcher::Request> requests;
				requests.reserve(count);
				for (size_t i = 0; i < count; i++)
				{
					ArticleInfo* article = (*c->DonorFile->GetArticles())[partIndices[i]].get();
					requests.push_back({article->GetMessageId(), c->DonorGroups});
				}
				c->Controller->m_batchFetcher.Begin(std::move(requests));
			});
		},
		[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
			const uint8_t** outData, size_t* outDataLen) -> int
		{
			return RepairMapCall<VerifyFileCtx>(rawCtx, 0, [=](VerifyFileCtx* c)
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
			RepairMapCallVoid<VerifyFileCtx>(rawCtx, [](VerifyFileCtx* c)
			{
				c->Controller->m_batchFetcher.CancelRemaining();
			});
		},
		[](void* rawCtx, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<VerifyFileCtx>(rawCtx, 0, [=](VerifyFileCtx* c)
			{
				c->File->Seek(offset);
				if (c->File->Position() != offset ||
					(size > 0 && c->File->Read(buf, size) != size))
				{
					return 0;
				}
				return 1;
			});
		}) != 0;
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return ok;
}

int StreamRepairController::PatchFromDonor(DiskFile& file, RepairTarget& target,
	FileInfo* donorFile, const StreamRangeList& donorRanges, const char* donorName)
{
	auto donorGroups = std::make_shared<std::vector<CString>>();
	for (const CString& group : *donorFile->GetGroups())
	{
		donorGroups->emplace_back(*group);
	}

	struct PatchFileCtx
	{
		StreamRepairController* Controller;
		DiskFile* File;
		RepairTarget* Target;
		FileInfo* DonorFile;
		std::shared_ptr<std::vector<CString>> DonorGroups;
		ArticleFetcher::FetchedArticle CurrentFetched;
		std::exception_ptr Error;
	} ctx{this, &file, &target, donorFile, std::move(donorGroups), {}, nullptr};

	int64 recoveredBytes = 0;
	int creditRecovery = 0;
	int recoveredParts = nzbget_rs_stream_patch_from_donor_file(
		target.Filename,
		target.DecodedFileSize,
		reinterpret_cast<const NzbgetRsByteRange*>(target.Holes.data()),
		target.Holes.size(),
		reinterpret_cast<const NzbgetRsByteRange*>(donorRanges.data()),
		donorRanges.size(),
		donorName,
		&ctx,
		[](void* rawCtx) -> int
		{
			return RepairMapCall<PatchFileCtx>(rawCtx, 1, [](PatchFileCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, const int32_t* partIndices, size_t count)
		{
			RepairMapCallVoid<PatchFileCtx>(rawCtx, [=](PatchFileCtx* c)
			{
				std::vector<ArticleBatchFetcher::Request> requests;
				requests.reserve(count);
				for (size_t i = 0; i < count; i++)
				{
					ArticleInfo* article = (*c->DonorFile->GetArticles())[partIndices[i]].get();
					requests.push_back({article->GetMessageId(), c->DonorGroups});
				}
				c->Controller->m_batchFetcher.Begin(std::move(requests));
			});
		},
		[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
			const uint8_t** outData, size_t* outDataLen) -> int
		{
			return RepairMapCall<PatchFileCtx>(rawCtx, 0, [=](PatchFileCtx* c)
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
			RepairMapCallVoid<PatchFileCtx>(rawCtx, [](PatchFileCtx* c)
			{
				c->Controller->m_batchFetcher.CancelRemaining();
			});
		},
		[](void* rawCtx, int64_t offset, const uint8_t* data, int64_t size) -> int
		{
			return RepairMapCall<PatchFileCtx>(rawCtx, 0, [=](PatchFileCtx* c)
			{
				c->File->Seek(offset);
				if (c->File->Position() != offset ||
					c->File->Write(data, size) != size ||
					!c->File->Flush())
				{
					c->Controller->PrintMessage(Message::mkError,
						"Could not write to %s during stream repair: %s",
						*c->Target->Filename, *WriteErrorMessage());
					return 0;
				}
				return 1;
			});
		},
		[](void* rawCtx, int64_t bytes)
		{
			RepairMapCallVoid<PatchFileCtx>(rawCtx, [bytes](PatchFileCtx* c)
			{
				c->Controller->m_progressBytes += bytes;
			});
		},
		[](void* rawCtx, int32_t kind, const char* text)
		{
			RepairMapCallVoid<PatchFileCtx>(rawCtx, [=](PatchFileCtx* c)
			{
				Message::EKind msgKind = kind == 2 ? Message::mkError :
					kind == 1 ? Message::mkWarning : Message::mkInfo;
				c->Controller->PrintMessage(msgKind, "%s", text);
			});
		},
		[](void* rawCtx, const NzbgetRsByteRange* holes, size_t holeCount)
		{
			RepairMapCallVoid<PatchFileCtx>(rawCtx, [=](PatchFileCtx* c)
			{
				c->Target->Holes.assign(
					reinterpret_cast<const StreamRange*>(holes),
					reinterpret_cast<const StreamRange*>(holes) + holeCount);
			});
		},
		&recoveredBytes,
		&creditRecovery);
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	if (creditRecovery && recoveredParts > 0)
	{
		m_recoveredBytes += recoveredBytes;
	}
	return recoveredParts;
}

void StreamRepairController::ReportRemainingHoles(std::vector<RepairTarget>& targets)
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

void StreamRepairController::ExecCrossPackRepair(const char* destDir,
	std::vector<RepairTarget>& targets, std::vector<DonorSource>& donors,
	const std::vector<CString>& memberNames)
{
	// the member universe: every completed file, with the CURRENT holes of
	// the ones that are repair targets (holes only shrink after M1). Holes
	// are declared for ALL holed targets - including live-ineligible ones -
	// but only PatchEligible targets get a patch slot: an undeclared hole
	// would be read as valid present bytes (zero-fill parsed as headers,
	// probes comparing donors against zeroes)
	std::vector<SetMember> setMembers;
	std::vector<StreamRangeList> memberHoles(memberNames.size());
	std::vector<int> memberTargets(memberNames.size(), -1);
	for (size_t i = 0; i < memberNames.size(); i++)
	{
		NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			destDir, PATH_SEPARATOR, *memberNames[i]);
		BString<1024> path;
		path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
		nzbget_rs_free(pathBuf);
		int64 size = FileSystem::FileSize(path);
		for (size_t t = 0; t < targets.size(); t++)
		{
			if (nzbget_rs_dqh_bytes_eq_ignore_case(*targets[t].Filename, *memberNames[i]) != 0)
			{
				memberHoles[i] = targets[t].Holes;
				memberTargets[i] = targets[t].PatchEligible ? (int)t : -1;
				// a file joined from article files (DirectWrite=no) ends with
				// its last downloaded article: holes at its end lie past the
				// end of the file on disk, and they are declared holes anyway
				size = std::max(size, targets[t].DecodedFileSize);
				break;
			}
		}
		setMembers.push_back({*memberNames[i], size});
	}

	DiskSourceSet diskSources(destDir, setMembers);
	HoledSourceSet holedSources(diskSources, memberHoles);
	// the target's own password (if any) lets encrypted store-rar target sets
	// map; plaintext sets are unaffected by it
	std::vector<RepairSetData> repairSets =
		ContentMapper::BuildRepairSets(setMembers, memberHoles, holedSources,
			m_targetPassword.Empty() ? nullptr : *m_targetPassword);

	bool anyMapped = false;
	for (RepairSetData& repairSet : repairSets)
	{
		if (repairSet.Map)
		{
			anyMapped = true;
			PrintMessage(Message::mkInfo,
				"Cross-packing repair of %s: %.1f MB missing of inner file %s",
				setMembers[repairSet.Set.Members[0]].Name.c_str(),
				nzbget_rs_dqh_stream_repair_bytes_to_mb(DupeStreamRepair::TotalSize(repairSet.InnerHoles)),
				repairSet.Map->GetInnerName());
		}
		else
		{
			PrintMessage(Message::mkInfo,
				"Cross-packing repair of %s not possible: %s; left to par-repair",
				setMembers[repairSet.Set.Members[0]].Name.c_str(),
				repairSet.SkipReason.c_str());
		}
	}
	if (!anyMapped)
	{
		return;
	}

	TargetSetFiles targetFiles(destDir, setMembers);

	// M4 (option <DupeStreamDecompress>): one decompression attempt per
	// (target set x donor set) for the whole pass - materialize+extract is
	// far too heavy to ever repeat against the same pair
	std::set<std::pair<const RepairSetData*, std::string>> decompressTried;
	std::set<std::string> triedPostings;

	for (DonorSource& donor : donors)
	{
		if (IsStopped())
		{
			break;
		}
		bool anyInnerHoles = false;
		for (RepairSetData& repairSet : repairSets)
		{
			anyInnerHoles |= repairSet.Map && !repairSet.InnerHoles.empty();
		}
		if (!anyInnerHoles)
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

		if (m_postInfo)
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(BString<1024>("Cross-packing repair from duplicate %s", *donor.InfoName));
			m_postInfo->SetStageProgress(0);
		}

		DonorSetSources donorSources(m_fetcher, donorNzb.get());
		// patch reads fetch the donor parts of each range in parallel
		donorSources.SetBatchFetcher(&m_batchFetcher);
		std::vector<SetMember> donorMembers = donorSources.BuildMembers();
		std::vector<MemberSet> donorSets = ContentMapper::GroupSets(donorMembers);

		// a donor set's map depends only on the donor side (members, sources,
		// password) - never on the repair set - so build it at most once per
		// duplicate and reuse it across repair sets. Rebuilds only burned CPU
		// (fetches are cached in DonorMemberSource), but a multi-set release
		// paid the parse R times and logged every skip R times
		std::vector<std::unique_ptr<ContentMap>> setMaps(donorSets.size());
		std::vector<std::string> setSkips(donorSets.size());
		std::vector<bool> setTried(donorSets.size(), false);

		for (RepairSetData& repairSet : repairSets)
		{
			if (!repairSet.Map || repairSet.InnerHoles.empty() || IsStopped())
			{
				continue;
			}

			for (size_t setIndex = 0; setIndex < donorSets.size(); setIndex++)
			{
				const MemberSet& donorSet = donorSets[setIndex];

				if (IsStopped() || repairSet.InnerHoles.empty())
				{
					break;
				}

				if (!setTried[setIndex])
				{
					setTried[setIndex] = true;
					// map building is fetch-budgeted; probing and patching are
					// naturally bounded by the holes
					int mappingBudget = 8 + (int)donorSet.Members.size() * 4;
					donorSources.SetFetchBudget(&mappingBudget);
					setMaps[setIndex] = ContentMapper::BuildMap(donorMembers, donorSet,
						donorSources, setSkips[setIndex]);
					// M3 ladder: a donor that failed ONLY for encryption may map
					// with its own password (already-fetched articles are cached,
					// so the retry mostly re-parses); anything else stays skipped.
					// The reasons are the constants BuildRarMap sets, so a reword
					// there cannot silently disable this retry.
					if (!setMaps[setIndex] && !donor.Password.Empty() &&
						(setSkips[setIndex] == ContentMapper::SkipEncryptedData ||
						 setSkips[setIndex] == ContentMapper::SkipEncryptedHeaders))
					{
						mappingBudget = 8 + (int)donorSet.Members.size() * 4;
						setMaps[setIndex] = ContentMapper::BuildMap(donorMembers, donorSet,
							donorSources, setSkips[setIndex], donor.Password);
						if (!setMaps[setIndex])
						{
							// the safety net firing (e.g. "archive password
							// rejected") must be visible; never log the password
							PrintMessage(Message::mkInfo,
								"Skipping packing %s of duplicate %s: %s",
								donorMembers[donorSet.Members[0]].Name.c_str(),
								*donor.InfoName, setSkips[setIndex].c_str());
						}
					}
					donorSources.SetFetchBudget(nullptr);
				}
				ContentMap* donorMap = setMaps[setIndex].get();

				if (!donorMap ||
					donorMap->GetInnerSize() != repairSet.Map->GetInnerSize())
				{
					// M4 ladder (option <DupeStreamDecompress>): a donor set
					// that cannot map for byte-copy may still donate through
					// extraction if it is a compressed archive. Skipped in a
					// live pass - materializing and extracting whole archives
					// beside an active download is too disk/CPU-heavy; those
					// holes wait for the post-processing pass
					bool canDecompress = !m_liveMode && g_Options->GetDupeStreamDecompress() &&
						ContentMapper::IsCompressibleArchive(donorSet);
					if (canDecompress &&
						decompressTried.emplace(&repairSet,
							std::string(*donor.QueuedFilename) + "|" +
							donorMembers[donorSet.Members[0]].Name).second)
					{
						// an encrypted target is a store-mode encrypted rar (a
						// compressed archive never maps, so it is never a repair
						// target): the extracted plaintext is re-encrypted under
						// the target's stream context by the M3 write core
						ExecDecompressRepair(destDir, repairSet, donorNzb.get(),
							donorMembers, donorSet, donor, targetFiles,
							targets, memberTargets, setMembers);
					}
					continue;	// not this target's content (or not mappable)
				}

				if (!VerifyDonorSet(repairSet, *donorMap, donorSources, targetFiles))
				{
					// mkInfo on purpose: a rejected donor is the safety net
					// firing - it must be visible in the log
					PrintMessage(Message::mkInfo,
						"Skipping packing %s of duplicate %s for %s: content identity not confirmed",
						donorMembers[donorSet.Members[0]].Name.c_str(),
						*donor.InfoName, repairSet.Map->GetInnerName());
					continue;
				}
				donorSources.TakeServedParts();	// probe fetches are not "recovered"

				PatchFromDonorSet(repairSet, *donorMap, donorSources,
					targetFiles, targets, memberTargets, setMembers, donor.InfoName);
			}
		}
	}
}

namespace
{
	struct FlatContentMap
	{
		const char* InnerName = "";
		int64 InnerSize = 0;
		std::vector<NzbgetRsContentRun> Runs;
		std::vector<int64_t> CipherOffsets;
		std::vector<int64_t> CipherSizes;
		std::vector<uint8_t> HasCrypto;
		const uint8* Key = nullptr;
		size_t KeyLen = 0;
		const uint8* Iv = nullptr;

		explicit FlatContentMap(ContentMap& map)
			: InnerName(map.GetInnerName())
			, InnerSize(map.GetInnerSize())
		{
			const std::vector<ContentRun>& runs = *map.GetRuns();
			const std::vector<RunCrypto>& cryptos = *map.GetRunCryptos();
			Runs.resize(runs.size());
			CipherOffsets.resize(runs.size(), 0);
			CipherSizes.resize(runs.size(), 0);
			HasCrypto.resize(runs.size(), 0);
			for (size_t i = 0; i < runs.size(); i++)
			{
				Runs[i] = {runs[i].InnerOffset, runs[i].MemberIndex, runs[i].MemberOffset, runs[i].Size};
				if (i < cryptos.size())
				{
					CipherOffsets[i] = cryptos[i].CipherDataOffset;
					CipherSizes[i] = cryptos[i].CipherSize;
					HasCrypto[i] = cryptos[i].Crypto ? 1 : 0;
				}
			}
			const RunCrypto* rc0 = map.GetRunCrypto(0);
			if (rc0 && rc0->Crypto)
			{
				Key = rc0->Crypto->GetKey();
				KeyLen = static_cast<size_t>(rc0->Crypto->GetKeyLength());
				Iv = rc0->Crypto->GetIv();
			}
		}
	};
}

bool StreamRepairController::ReadDonorInner(ContentMap& donorMap,
	DonorSetSources& donorSources, const StreamRange& innerRange, std::vector<char>& buffer)
{
	if (innerRange.Size < 0)
	{
		return false;
	}
	buffer.resize((size_t)innerRange.Size);
	FlatContentMap flatDonor(donorMap);

	struct ReadDonorCtx
	{
		DonorSetSources* DonorSources;
		std::exception_ptr Error;
	} ctx{&donorSources, nullptr};

	bool ok = nzbget_rs_stream_read_donor_inner(
		flatDonor.InnerName, flatDonor.InnerSize,
		flatDonor.Runs.data(), flatDonor.CipherOffsets.data(),
		flatDonor.CipherSizes.data(), flatDonor.HasCrypto.data(), flatDonor.Runs.size(),
		flatDonor.Key, flatDonor.KeyLen, flatDonor.Iv,
		NzbgetRsByteRange{innerRange.Offset, innerRange.Size},
		buffer.empty() ? nullptr : reinterpret_cast<uint8_t*>(buffer.data()),
		&ctx,
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<ReadDonorCtx>(rawCtx, 0, [memberIndex](ReadDonorCtx* c)
			{
				return c->DonorSources->GetDonorSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return RepairMapCall<ReadDonorCtx>(rawCtx, static_cast<int64_t>(0), [memberIndex](ReadDonorCtx* c)
			{
				DonorMemberSource* src = c->DonorSources->GetDonorSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<ReadDonorCtx>(rawCtx, 0, [=](ReadDonorCtx* c)
			{
				DonorMemberSource* src = c->DonorSources->GetDonorSource(memberIndex);
				return (src && src->Read(offset, buf, size)) ? 1 : 0;
			});
		}) != 0;
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return ok;
}

bool StreamRepairController::VerifyDonorSet(RepairSetData& repairSet, ContentMap& donorMap,
	DonorSetSources& donorSources, TargetSetFiles& targetFiles)
{
	FlatContentMap flatTarget(*repairSet.Map);
	FlatContentMap flatDonor(donorMap);

	struct VerifyCtx
	{
		StreamRepairController* Controller;
		DonorSetSources* DonorSources;
		TargetSetFiles* TargetFiles;
		std::exception_ptr Error;
	} ctx{this, &donorSources, &targetFiles, nullptr};

	bool ok = nzbget_rs_stream_verify_donor_set(
		flatTarget.InnerName, flatTarget.InnerSize,
		flatTarget.Runs.data(), flatTarget.CipherOffsets.data(),
		flatTarget.CipherSizes.data(), flatTarget.HasCrypto.data(), flatTarget.Runs.size(),
		flatTarget.Key, flatTarget.KeyLen, flatTarget.Iv,
		reinterpret_cast<const NzbgetRsByteRange*>(repairSet.InnerHoles.data()),
		repairSet.InnerHoles.size(),
		reinterpret_cast<const NzbgetRsByteRange*>(repairSet.OriginalInnerHoles.data()),
		repairSet.OriginalInnerHoles.size(),
		flatDonor.InnerName, flatDonor.InnerSize,
		flatDonor.Runs.data(), flatDonor.CipherOffsets.data(),
		flatDonor.CipherSizes.data(), flatDonor.HasCrypto.data(), flatDonor.Runs.size(),
		flatDonor.Key, flatDonor.KeyLen, flatDonor.Iv,
		&ctx,
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<VerifyCtx>(rawCtx, 0, [memberIndex](VerifyCtx* c)
			{
				return c->DonorSources->GetDonorSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return RepairMapCall<VerifyCtx>(rawCtx, static_cast<int64_t>(0), [memberIndex](VerifyCtx* c)
			{
				DonorMemberSource* src = c->DonorSources->GetDonorSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<VerifyCtx>(rawCtx, 0, [=](VerifyCtx* c)
			{
				DonorMemberSource* src = c->DonorSources->GetDonorSource(memberIndex);
				return (src && src->Read(offset, buf, size)) ? 1 : 0;
			});
		},
		[](void* rawCtx) -> int
		{
			return RepairMapCall<VerifyCtx>(rawCtx, 1, [](VerifyCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<VerifyCtx>(rawCtx, 0, [memberIndex](VerifyCtx* c)
			{
				return c->TargetFiles->GetFile(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<VerifyCtx>(rawCtx, 0, [=](VerifyCtx* c)
			{
				DiskFile* file = c->TargetFiles->GetFile(memberIndex);
				if (!file)
				{
					return 0;
				}
				file->Seek(offset);
				if (file->Position() != offset ||
					(size > 0 && file->Read(buf, size) != size))
				{
					return 0;
				}
				return 1;
			});
		}) != 0;
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return ok;
}

int64 StreamRepairController::PatchFromDonorSet(RepairSetData& repairSet, ContentMap& donorMap,
	DonorSetSources& donorSources, TargetSetFiles& targetFiles,
	std::vector<RepairTarget>& targets, const std::vector<int>& memberTargets,
	const std::vector<SetMember>& setMembers, const char* donorName)
{
	FlatContentMap flatTarget(*repairSet.Map);
	FlatContentMap flatDonor(donorMap);

	std::vector<const char*> memberNames(setMembers.size());
	std::vector<int64_t> memberSizes(setMembers.size());
	for (size_t i = 0; i < setMembers.size(); i++)
	{
		memberNames[i] = setMembers[i].Name.c_str();
		memberSizes[i] = setMembers[i].Size;
	}
	std::vector<size_t> targetHoleCounts(targets.size());
	std::vector<NzbgetRsByteRange> targetHolesFlat;
	for (size_t t = 0; t < targets.size(); t++)
	{
		targetHoleCounts[t] = targets[t].Holes.size();
		for (const StreamRange& h : targets[t].Holes)
		{
			targetHolesFlat.push_back({h.Offset, h.Size});
		}
	}

	struct PatchCtx
	{
		StreamRepairController* Controller;
		RepairSetData* RepairSet;
		DonorSetSources* DonorSources;
		TargetSetFiles* TargetFiles;
		std::vector<RepairTarget>* Targets;
		const std::vector<SetMember>* SetMembers;
		std::exception_ptr Error;
	} ctx{this, &repairSet, &donorSources, &targetFiles, &targets, &setMembers, nullptr};

	int creditRecovery = 0;
	int64 written = nzbget_rs_stream_patch_from_donor_set(
		flatTarget.InnerName, flatTarget.InnerSize,
		flatTarget.Runs.data(), flatTarget.CipherOffsets.data(),
		flatTarget.CipherSizes.data(), flatTarget.HasCrypto.data(), flatTarget.Runs.size(),
		flatTarget.Key, flatTarget.KeyLen, flatTarget.Iv,
		reinterpret_cast<const NzbgetRsByteRange*>(repairSet.InnerHoles.data()),
		repairSet.InnerHoles.size(),
		flatDonor.InnerName, flatDonor.InnerSize,
		flatDonor.Runs.data(), flatDonor.CipherOffsets.data(),
		flatDonor.CipherSizes.data(), flatDonor.HasCrypto.data(), flatDonor.Runs.size(),
		flatDonor.Key, flatDonor.KeyLen, flatDonor.Iv,
		memberNames.data(), memberSizes.data(), setMembers.size(),
		memberTargets.data(),
		targetHoleCounts.data(), targetHolesFlat.data(), targets.size(),
		donorName,
		&ctx,
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<PatchCtx>(rawCtx, 0, [memberIndex](PatchCtx* c)
			{
				return c->DonorSources->GetDonorSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return RepairMapCall<PatchCtx>(rawCtx, static_cast<int64_t>(0), [memberIndex](PatchCtx* c)
			{
				DonorMemberSource* src = c->DonorSources->GetDonorSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<PatchCtx>(rawCtx, 0, [=](PatchCtx* c)
			{
				DonorMemberSource* src = c->DonorSources->GetDonorSource(memberIndex);
				return (src && src->Read(offset, buf, size)) ? 1 : 0;
			});
		},
		[](void* rawCtx) -> int
		{
			return RepairMapCall<PatchCtx>(rawCtx, 1, [](PatchCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<PatchCtx>(rawCtx, 0, [memberIndex](PatchCtx* c)
			{
				return c->TargetFiles->GetFile(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<PatchCtx>(rawCtx, 0, [=](PatchCtx* c)
			{
				DiskFile* file = c->TargetFiles->GetFile(memberIndex);
				if (!file)
				{
					return 0;
				}
				file->Seek(offset);
				if (file->Position() != offset ||
					(size > 0 && file->Read(buf, size) != size))
				{
					return 0;
				}
				return 1;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, const uint8_t* data, int64_t size) -> int
		{
			return RepairMapCall<PatchCtx>(rawCtx, 0, [=](PatchCtx* c)
			{
				DiskFile* file = c->TargetFiles->GetFile(memberIndex);
				if (!file)
				{
					return 0;
				}
				file->Seek(offset);
				if (file->Position() != offset ||
					file->Write(data, size) != size ||
					!file->Flush())
				{
					c->Controller->PrintMessage(Message::mkError,
						"Could not write to %s during stream repair: %s",
						(*c->SetMembers)[memberIndex].Name.c_str(),
						*WriteErrorMessage());
					return 0;
				}
				return 1;
			});
		},
		[](void* rawCtx, int32_t kind, const char* text)
		{
			RepairMapCallVoid<PatchCtx>(rawCtx, [=](PatchCtx* c)
			{
				Message::EKind msgKind = kind == 2 ? Message::mkError :
					kind == 1 ? Message::mkWarning : Message::mkInfo;
				c->Controller->PrintMessage(msgKind, "%s", text);
			});
		},
		[](void* rawCtx, const NzbgetRsByteRange* innerHoles, size_t innerHoleCount,
			const size_t* updatedCounts, const NzbgetRsByteRange* updatedFlat, size_t)
		{
			RepairMapCallVoid<PatchCtx>(rawCtx, [=](PatchCtx* c)
			{
				c->RepairSet->InnerHoles.assign(
					reinterpret_cast<const StreamRange*>(innerHoles),
					reinterpret_cast<const StreamRange*>(innerHoles) + innerHoleCount);
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
		&creditRecovery);
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}

	if (creditRecovery && written > 0)
	{
		int recoveredParts = donorSources.TakeServedParts();
		m_recoveredBytes += written;
		m_progressBytes += written;
		PrintMessage(Message::mkInfo,
			"Recovered %.1f MB (%i donor article(s)) of %s from duplicate %s (cross-packing)",
			nzbget_rs_dqh_stream_repair_bytes_to_mb(written), recoveredParts,
			repairSet.Map->GetInnerName(), donorName);
	}
	return written;
}

/* the M4 rung (option <DupeStreamDecompress>): materialize the donor set's
 * member files into a scratch dir under DestDir, run the configured extractor
 * (unrar/7z) on the main volume, pick the extracted file matching the target's
 * inner size, and let it donate through the plain inner-space verify/patch
 * path. Every degradation is a logged skip - never an error, never a write;
 * the scratch tree is deleted on EVERY exit path (skip, success, exception,
 * stop). Runs UNLOCKED except for the progress-label set. */
void StreamRepairController::ExecDecompressRepair(const char* destDir,
	RepairSetData& repairSet, NzbInfo* donorNzb,
	const std::vector<SetMember>& donorMembers, const MemberSet& donorSet,
	const DonorSource& donor, TargetSetFiles& targetFiles,
	std::vector<RepairTarget>& targets, const std::vector<int>& memberTargets,
	const std::vector<SetMember>& setMembers)
{
	const char* setName = donorMembers[donorSet.Members[0]].Name.c_str();

	// the main volume is the member the extractor accepts as an entry point
	// (rar: .rar/.part01.rar = data-order first; 7z: .7z/.7z.001 = first;
	// spanned zip: the final .zip = data-order LAST); the other volumes are
	// found next to it in the scratch dir
	const char* mainVolume = nullptr;
	for (int memberIndex : donorSet.Members)
	{
		const std::string& memberName = donorMembers[memberIndex].Name;
		if (Unpack::IsArchive(fs::path(memberName)))
		{
			mainVolume = memberName.c_str();
			break;
		}
	}
	if (!mainVolume)
	{
		PrintMessage(Message::mkInfo,
			"Skipping decompression of %s of duplicate %s: no extractable main volume",
			setName, *donor.InfoName);
		return;
	}

	// probe the extractor tool BEFORE materializing up to MaxDecompressBytes:
	// a missing/unconfigured unrar/7z is a static config state, so without it
	// the whole donor download would be wasted for every gated pair. Mirror
	// the validation MakeExtractor itself does (dispatch order: 7z, then
	// unrar); the real MakeExtractor call below stays the authoritative gate.
	fs::path mainVolumePath(mainVolume);
	if (!nzbget_rs_unpack_is_zipfast_supported_extension(mainVolume))
	{
		const fs::path& toolPath = Unpack::SevenZip::IsSupported(mainVolumePath) ?
			g_Options->GetSevenZipPath() : g_Options->GetUnrarPath();
		fs::error_code toolEc;
		if (toolPath.empty() || !fs::exists(toolPath, toolEc) || toolEc)
		{
			PrintMessage(Message::mkInfo,
				"Skipping decompression of %s of duplicate %s: no extraction tool configured",
				setName, *donor.InfoName);
			return;
		}
	}

	// a unique scratch dir (a stale leftover from a crashed run is never reused)
	BString<1024> tempDir;
	for (int suffix = 0; ; suffix++)
	{
		tempDir.Format("%s%c.stream-decompress.%i", destDir, PATH_SEPARATOR, suffix);
		if (FileSystem::CreateDirectoryExclusive(tempDir))
		{
			break;
		}
		if (errno != EEXIST)
		{
			PrintMessage(Message::mkInfo,
				"Skipping decompression of %s of duplicate %s: could not create %s: %s",
				setName, *donor.InfoName, *tempDir, *FileSystem::GetLastErrorMessage());
			return;
		}
	}

	// removes the whole scratch tree on every exit from this scope; declared
	// before any DiskFile over its content so files close before the delete
	struct TempDirGuard
	{
		CString Dir;
		~TempDirGuard()
		{
			CString errmsg;
			FileSystem::DeleteDirectoryWithContent(Dir, errmsg);
		}
	} tempDirGuard{CString(*tempDir)};

	if (m_postInfo)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		m_postInfo->SetProgressLabel(BString<1024>("Downloading duplicate %s", *donor.InfoName));
		m_postInfo->SetStageProgress(0);
	}

	// downloading and extracting the donor patches no byte for minutes: without
	// this the stall timeout stopped the rung before it could work (the run caps
	// still apply)
	struct MaterializingGuard
	{
		std::atomic<int>& counter;
		explicit MaterializingGuard(std::atomic<int>& c) : counter(c) { counter++; }
		~MaterializingGuard() { counter--; }
	} materializingGuard{m_materializing};

	int64 totalBytes = 0;
	if (!MaterializeDonorSet(donorNzb, donorMembers, donorSet, tempDir,
		donor.InfoName, totalBytes))
	{
		PrintMessage(Message::mkInfo,
			"Skipping decompression of %s of duplicate %s: donor not materializable (stopped, incomplete, over the size cap or disk error)",
			setName, *donor.InfoName);
		return;
	}

	NzbgetRsBuf archPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		*tempDir, PATH_SEPARATOR, mainVolume);
	BString<1024> archivePath;
	archivePath.Set(archPathBuf.data ? archPathBuf.data : "", (int)archPathBuf.len);
	nzbget_rs_free(archPathBuf);
	BString<1024> extractDir("%s%cextracted", *tempDir, PATH_SEPARATOR);
	if (IsStopped())
	{
		return;
	}
	if (m_postInfo)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		m_postInfo->SetProgressLabel(BString<1024>("Decompressing duplicate %s", *donor.InfoName));
		m_postInfo->SetStageProgress(0);
	}

	// the donor's own password unlocks its encrypted archive: passed to the
	// extractor, NEVER logged. MakeExtractor validates the configured tool
	Unpack::ExtractorPtr extractor = Unpack::MakeExtractor(fs::path(*archivePath),
		fs::path(*extractDir),
		donor.Password.Empty() ? std::string() : std::string(*donor.Password),
		Unpack::OverwriteMode::Skip);
	if (!extractor || !extractor->Extract())
	{
		PrintMessage(Message::mkInfo,
			"Skipping decompression of %s of duplicate %s: %s",
			setName, *donor.InfoName,
			extractor ? "extraction failed" : "no extraction tool configured");
		return;
	}

	std::string innerPath = DupeStreamRepair::SelectExtractedInner(extractDir,
		repairSet.Map->GetInnerSize(), repairSet.Map->GetInnerName());
	if (innerPath.empty())
	{
		PrintMessage(Message::mkInfo,
			"Skipping decompression of %s of duplicate %s: no extracted file matches inner file %s",
			setName, *donor.InfoName, repairSet.Map->GetInnerName());
		return;
	}

	DiskFile innerFile;
	if (!innerFile.Open(innerPath.c_str(), DiskFile::omRead))
	{
		PrintMessage(Message::mkInfo,
			"Skipping decompression of %s of duplicate %s: could not open the extracted file",
			setName, *donor.InfoName);
		return;
	}

	int64 innerSize = FileSystem::FileSize(innerPath.c_str());

	if (!VerifyDonorInnerFile(repairSet, innerFile, innerSize, targetFiles))
	{
		// mkInfo on purpose: a rejected donor is the safety net firing -
		// it must be visible in the log
		PrintMessage(Message::mkInfo,
			"Skipping decompression of %s of duplicate %s for %s: content identity not confirmed",
			setName, *donor.InfoName, repairSet.Map->GetInnerName());
		innerFile.Close();
		return;
	}

	PatchFromDonorInnerFile(repairSet, innerFile, innerSize, targetFiles,
		targets, memberTargets, setMembers, donor.InfoName);
	innerFile.Close();
}

/* fetches every article of every member of `set` and writes the decoded bytes
 * at their declared offsets into tempDir/<member basename>. Extraction needs
 * complete donor members, so the first unavailable or invalid article aborts
 * this donor instead of spending hours walking the rest of a missing posting.
 * false = do not extract:
 * stopped, an article-less member, over MaxDecompressBytes, or a disk error.
 * Runs UNLOCKED - fetches and disk I/O never hold the queue lock. */
bool StreamRepairController::MaterializeDonorSet(NzbInfo* donorNzb,
	const std::vector<SetMember>& donorMembers, const MemberSet& set,
	const char* tempDir, const char* donorName, int64& totalBytes)
{
	std::vector<FileInfo*> files;
	std::vector<const char*> fileNames;
	std::vector<int32_t> fileArticleCounts;
	for (FileInfo* fileInfo : donorNzb->GetFileList())
	{
		size_t idx = files.size();
		files.push_back(fileInfo);
		fileNames.push_back(idx < donorMembers.size() ? donorMembers[idx].Name.c_str() : "");
		fileArticleCounts.push_back((int32_t)fileInfo->GetArticles()->size());
	}

	struct MaterializeCtx
	{
		StreamRepairController* Controller;
		const std::vector<FileInfo*>* Files;
		const std::vector<SetMember>* DonorMembers;
		const char* TempDir;
		DiskFile CurrentFile;
		BString<1024> CurrentPath;
		ArticleFetcher::FetchedArticle CurrentFetched;
		std::exception_ptr Error;
	} ctx{this, &files, &donorMembers, tempDir, {}, {}, {}, nullptr};

	bool ok = nzbget_rs_stream_materialize_donor_set(
		set.Members.data(),
		set.Members.size(),
		fileNames.data(),
		fileArticleCounts.data(),
		files.size(),
		donorName,
		&totalBytes,
		&ctx,
		[](void* rawCtx) -> int
		{
			return RepairMapCall<MaterializeCtx>(rawCtx, 1, [](MaterializeCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<MaterializeCtx>(rawCtx, 0, [memberIndex](MaterializeCtx* c)
			{
				NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					c->TempDir, PATH_SEPARATOR, (*c->DonorMembers)[memberIndex].Name.c_str());
				c->CurrentPath.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
				nzbget_rs_free(pathBuf);
				if (!c->CurrentFile.Open(c->CurrentPath, DiskFile::omWrite))
				{
					c->Controller->PrintMessage(Message::mkWarning,
						"Could not create %s for donor materialization: %s",
						*c->CurrentPath, *FileSystem::GetLastErrorMessage());
					return 0;
				}
				return 1;
			});
		},
		[](void* rawCtx, int64_t offset, const uint8_t* data, int64_t size) -> int
		{
			return RepairMapCall<MaterializeCtx>(rawCtx, 0, [=](MaterializeCtx* c)
			{
				c->CurrentFile.Seek(offset);
				if (c->CurrentFile.Position() != offset ||
					c->CurrentFile.Write(data, size) != size ||
					!c->CurrentFile.Flush())
				{
					c->Controller->PrintMessage(Message::mkWarning,
						"Could not write to %s during donor materialization: %s",
						*c->CurrentPath, *WriteErrorMessage());
					return 0;
				}
				return 1;
			});
		},
		[](void* rawCtx)
		{
			RepairMapCallVoid<MaterializeCtx>(rawCtx, [](MaterializeCtx* c)
			{
				c->CurrentFile.Close();
			});
		},
		[](void* rawCtx, int32_t memberIndex)
		{
			RepairMapCallVoid<MaterializeCtx>(rawCtx, [memberIndex](MaterializeCtx* c)
			{
				FileInfo* donorFile = (*c->Files)[memberIndex];
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
		[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
			const uint8_t** outData, size_t* outDataLen) -> int
		{
			return RepairMapCall<MaterializeCtx>(rawCtx, 0, [=](MaterializeCtx* c)
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
			RepairMapCallVoid<MaterializeCtx>(rawCtx, [](MaterializeCtx* c)
			{
				c->Controller->m_batchFetcher.CancelRemaining();
			});
		},
		[](void* rawCtx, const char* label, int32_t stageProgress)
		{
			RepairMapCallVoid<MaterializeCtx>(rawCtx, [=](MaterializeCtx* c)
			{
				if (c->Controller->m_postInfo)
				{
					GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
					c->Controller->m_postInfo->SetProgressLabel(label ? label : "");
					c->Controller->m_postInfo->SetStageProgress(stageProgress);
				}
			});
		},
		[](void* rawCtx, int32_t kind, const char* text)
		{
			RepairMapCallVoid<MaterializeCtx>(rawCtx, [=](MaterializeCtx* c)
			{
				Message::EKind msgKind = kind == 2 ? Message::mkError :
					kind == 1 ? Message::mkWarning : Message::mkInfo;
				c->Controller->PrintMessage(msgKind, "%s", text);
			});
		}) != 0;
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return ok;
}

bool StreamRepairController::VerifyDonorInnerFile(RepairSetData& repairSet,
	DiskFile& donorInner, int64 donorInnerSize, TargetSetFiles& targetFiles)
{
	FlatContentMap flatTarget(*repairSet.Map);
	DiskContentSource donorSource(donorInner, donorInnerSize);

	struct VerifyInnerCtx
	{
		StreamRepairController* Controller;
		DiskContentSource* DonorSource;
		TargetSetFiles* TargetFiles;
		std::exception_ptr Error;
	} ctx{this, &donorSource, &targetFiles, nullptr};

	bool ok = nzbget_rs_stream_verify_donor_inner_file(
		flatTarget.InnerName, flatTarget.InnerSize,
		flatTarget.Runs.data(), flatTarget.CipherOffsets.data(),
		flatTarget.CipherSizes.data(), flatTarget.HasCrypto.data(), flatTarget.Runs.size(),
		flatTarget.Key, flatTarget.KeyLen, flatTarget.Iv,
		reinterpret_cast<const NzbgetRsByteRange*>(repairSet.InnerHoles.data()),
		repairSet.InnerHoles.size(),
		reinterpret_cast<const NzbgetRsByteRange*>(repairSet.OriginalInnerHoles.data()),
		repairSet.OriginalInnerHoles.size(),
		donorInnerSize,
		&ctx,
		[](void* rawCtx, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<VerifyInnerCtx>(rawCtx, 0, [=](VerifyInnerCtx* c)
			{
				return c->DonorSource->Read(offset, buf, size) ? 1 : 0;
			});
		},
		[](void* rawCtx) -> int
		{
			return RepairMapCall<VerifyInnerCtx>(rawCtx, 1, [](VerifyInnerCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<VerifyInnerCtx>(rawCtx, 0, [memberIndex](VerifyInnerCtx* c)
			{
				return c->TargetFiles->GetFile(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<VerifyInnerCtx>(rawCtx, 0, [=](VerifyInnerCtx* c)
			{
				DiskFile* file = c->TargetFiles->GetFile(memberIndex);
				if (!file)
				{
					return 0;
				}
				file->Seek(offset);
				if (file->Position() != offset ||
					(size > 0 && file->Read(buf, size) != size))
				{
					return 0;
				}
				return 1;
			});
		}) != 0;
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return ok;
}

int64 StreamRepairController::PatchFromDonorInnerFile(RepairSetData& repairSet,
	DiskFile& donorInner, int64 donorInnerSize, TargetSetFiles& targetFiles,
	std::vector<RepairTarget>& targets, const std::vector<int>& memberTargets,
	const std::vector<SetMember>& setMembers, const char* donorName)
{
	FlatContentMap flatTarget(*repairSet.Map);
	DiskContentSource donorSource(donorInner, donorInnerSize);

	std::vector<const char*> memberNames(setMembers.size());
	std::vector<int64_t> memberSizes(setMembers.size());
	for (size_t i = 0; i < setMembers.size(); i++)
	{
		memberNames[i] = setMembers[i].Name.c_str();
		memberSizes[i] = setMembers[i].Size;
	}
	std::vector<size_t> targetHoleCounts(targets.size());
	std::vector<NzbgetRsByteRange> targetHolesFlat;
	for (size_t t = 0; t < targets.size(); t++)
	{
		targetHoleCounts[t] = targets[t].Holes.size();
		for (const StreamRange& h : targets[t].Holes)
		{
			targetHolesFlat.push_back({h.Offset, h.Size});
		}
	}

	struct PatchInnerCtx
	{
		StreamRepairController* Controller;
		RepairSetData* RepairSet;
		DiskContentSource* DonorSource;
		TargetSetFiles* TargetFiles;
		std::vector<RepairTarget>* Targets;
		const std::vector<SetMember>* SetMembers;
		std::exception_ptr Error;
	} ctx{this, &repairSet, &donorSource, &targetFiles, &targets, &setMembers, nullptr};

	int holesFilled = 0;
	int creditRecovery = 0;
	int64 written = nzbget_rs_stream_patch_from_donor_inner_file(
		flatTarget.InnerName, flatTarget.InnerSize,
		flatTarget.Runs.data(), flatTarget.CipherOffsets.data(),
		flatTarget.CipherSizes.data(), flatTarget.HasCrypto.data(), flatTarget.Runs.size(),
		flatTarget.Key, flatTarget.KeyLen, flatTarget.Iv,
		reinterpret_cast<const NzbgetRsByteRange*>(repairSet.InnerHoles.data()),
		repairSet.InnerHoles.size(),
		donorInnerSize,
		memberNames.data(), memberSizes.data(), setMembers.size(),
		memberTargets.data(),
		targetHoleCounts.data(), targetHolesFlat.data(), targets.size(),
		donorName,
		&ctx,
		[](void* rawCtx, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<PatchInnerCtx>(rawCtx, 0, [=](PatchInnerCtx* c)
			{
				return c->DonorSource->Read(offset, buf, size) ? 1 : 0;
			});
		},
		[](void* rawCtx) -> int
		{
			return RepairMapCall<PatchInnerCtx>(rawCtx, 1, [](PatchInnerCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return RepairMapCall<PatchInnerCtx>(rawCtx, 0, [memberIndex](PatchInnerCtx* c)
			{
				return c->TargetFiles->GetFile(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return RepairMapCall<PatchInnerCtx>(rawCtx, 0, [=](PatchInnerCtx* c)
			{
				DiskFile* file = c->TargetFiles->GetFile(memberIndex);
				if (!file)
				{
					return 0;
				}
				file->Seek(offset);
				if (file->Position() != offset ||
					(size > 0 && file->Read(buf, size) != size))
				{
					return 0;
				}
				return 1;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, const uint8_t* data, int64_t size) -> int
		{
			return RepairMapCall<PatchInnerCtx>(rawCtx, 0, [=](PatchInnerCtx* c)
			{
				DiskFile* file = c->TargetFiles->GetFile(memberIndex);
				if (!file)
				{
					return 0;
				}
				file->Seek(offset);
				if (file->Position() != offset ||
					file->Write(data, size) != size ||
					!file->Flush())
				{
					c->Controller->PrintMessage(Message::mkError,
						"Could not write to %s during stream repair: %s",
						(*c->SetMembers)[memberIndex].Name.c_str(),
						*WriteErrorMessage());
					return 0;
				}
				return 1;
			});
		},
		[](void* rawCtx, int32_t kind, const char* text)
		{
			RepairMapCallVoid<PatchInnerCtx>(rawCtx, [=](PatchInnerCtx* c)
			{
				Message::EKind msgKind = kind == 2 ? Message::mkError :
					kind == 1 ? Message::mkWarning : Message::mkInfo;
				c->Controller->PrintMessage(msgKind, "%s", text);
			});
		},
		[](void* rawCtx, const NzbgetRsByteRange* innerHoles, size_t innerHoleCount,
			const size_t* updatedCounts, const NzbgetRsByteRange* updatedFlat, size_t)
		{
			RepairMapCallVoid<PatchInnerCtx>(rawCtx, [=](PatchInnerCtx* c)
			{
				c->RepairSet->InnerHoles.assign(
					reinterpret_cast<const StreamRange*>(innerHoles),
					reinterpret_cast<const StreamRange*>(innerHoles) + innerHoleCount);
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
		&holesFilled,
		&creditRecovery);
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}

	if (creditRecovery && written > 0)
	{
		m_recoveredHoles += holesFilled;
		m_recoveredBytes += written;
		m_progressBytes += written;
		PrintMessage(Message::mkInfo,
			"Recovered %.1f MB of %s from duplicate %s (decompressed)",
			nzbget_rs_dqh_stream_repair_bytes_to_mb(written), repairSet.Map->GetInnerName(), donorName);
	}
	return written;
}

void StreamRepairController::RepairCompleted()
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

void StreamRepairController::AddMessage(Message::EKind kind, const char* text)
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
