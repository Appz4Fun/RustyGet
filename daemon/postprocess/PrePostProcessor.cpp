/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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


#include "nzbget.h"
#include "PrePostProcessor.h"
#include "Options.h"
#include "WorkState.h"
#include "Log.h"
#include "HistoryCoordinator.h"
#include "DupeCoordinator.h"
#include "DupeStreamRepair.h"
#include "PostScript.h"
#include "Util.h"
#include "FileSystem.h"
#include "UnpackController.h"
#include "Cleanup.h"
#include "Rename.h"
#include "Repair.h"
#include "StreamRepair.h"
#include "NzbFile.h"
#include "QueueScript.h"
#include "ParParser.h"
#include "ParRenamer.h"
#include "DirectUnpack.h"
#include "PostUnpackRenamer.h"
#include "PostDownloadRenamer.h"
#include "ParDamage.h"
#include <mutex>
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

PrePostProcessor::PrePostProcessor()
{
	debug("Creating PrePostProcessor");

	DownloadQueue::Guard()->Attach(this);
}

void PrePostProcessor::Run()
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

void PrePostProcessor::WaitJobs()
{
	debug("PrePostProcessor: waiting for jobs to complete");

	// wait 5 seconds until all post-processing jobs gracefully finish
	time_t waitStart = Util::CurrentTime();
	while (Util::CurrentTime() < waitStart + 5)
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
	while (Util::CurrentTime() < waitStart + 5)
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

void PrePostProcessor::Stop()
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

bool PrePostProcessor::HasActiveUnpack(bool includeWaitingVolume)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	return HasActiveUnpack(downloadQueue, includeWaitingVolume);
}

bool PrePostProcessor::HasActiveUnpack(DownloadQueue* downloadQueue, bool includeWaitingVolume)
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
			if (includeWaitingVolume || (directUnpack->IsUnpacking() && !directUnpack->IsWaitingVolume()))
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
void PrePostProcessor::SanitisePostQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsRunning && !nzbInfo->GetUnpackThread())
		{
			nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
		}
		PostInfo* postInfo = nzbInfo->GetPostInfo();
#ifdef NZBGET_USE_RUST
		bool canCheckComplete = !postInfo && nzbInfo->GetKind() == NzbInfo::nkNzb &&
			!nzbInfo->GetCompletedFiles()->empty() && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone;
		if (nzbget_rs_dad_prepost_should_complete_interrupted_download(
			postInfo ? 1 : 0,
			(int)nzbInfo->GetKind(),
			(int)NzbInfo::nkNzb,
			nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
			(int)nzbInfo->GetDeleteStatus(),
			(int)NzbInfo::dsNone,
			(canCheckComplete && nzbInfo->IsDownloadCompleted(true)) ? 1 : 0) != 0)
#else
		if (!postInfo && nzbInfo->GetKind() == NzbInfo::nkNzb && !nzbInfo->GetCompletedFiles()->empty() &&
			nzbInfo->GetDeleteStatus() == NzbInfo::dsNone && nzbInfo->IsDownloadCompleted(true))
#endif
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
#ifdef NZBGET_USE_RUST
			int nextStage = 0;
			int reqParCheck = 0;
			nzbget_rs_dsp_sanitise_post_stage(
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
#else
			if (postInfo->GetStage() == PostInfo::ptExecutingScript ||
				!FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
			{
				postInfo->SetStage(PostInfo::ptFinished);
			}
			else
			{
				postInfo->SetStage(PostInfo::ptQueued);
				if (nzbInfo->GetUnpackTried() && nzbInfo->GetParStatus() <= NzbInfo::psSkipped)
				{
					postInfo->SetRequestParCheck(true);
				}
			}
#endif
			postInfo->SetWorking(false);
		}
	}
}

void PrePostProcessor::DownloadQueueUpdate(void* aspect)
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
	else if ((queueAspect->action == DownloadQueue::eaFileCompleted ||
		queueAspect->action == DownloadQueue::eaFileDeleted))
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
				if (postJob && queueAspect->fileInfo->GetNzbInfo() == postJob &&
					postJob->GetPostInfo() && postJob->GetPostInfo()->GetPostThread() &&
					postJob->GetPostInfo()->GetStage() >= PostInfo::ptLoadingPars &&
					postJob->GetPostInfo()->GetStage() <= PostInfo::ptVerifyingRepaired &&
					((RepairController*)postJob->GetPostInfo()->GetPostThread())->AddPar(
						queueAspect->fileInfo, queueAspect->action == DownloadQueue::eaFileDeleted))
				{
					return;
				}
			}
		}
#endif

#ifdef NZBGET_USE_RUST
		int eventAction = nzbget_rs_dsp_classify_file_event(
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
#else
		if ((queueAspect->action == DownloadQueue::eaFileCompleted ||
			 queueAspect->fileInfo->GetDupeDeleted()) &&
			queueAspect->fileInfo->GetNzbInfo()->GetDeleteStatus() != NzbInfo::dsHealth &&
			!queueAspect->nzbInfo->GetPostInfo() &&
			queueAspect->nzbInfo->IsDownloadCompleted(true))
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s completely downloaded", queueAspect->nzbInfo->GetName());
			g_QueueScriptCoordinator->EnqueueScript(queueAspect->nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
			NzbDownloaded(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
		else if ((queueAspect->action == DownloadQueue::eaFileDeleted ||
			(queueAspect->action == DownloadQueue::eaFileCompleted &&
			 queueAspect->fileInfo->GetNzbInfo()->GetDeleteStatus() > NzbInfo::dsNone)) &&
			!queueAspect->nzbInfo->GetPostInfo() &&
			queueAspect->nzbInfo->IsDownloadCompleted(false))
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s deleted from queue", queueAspect->nzbInfo->GetName());
			NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
#endif
	}
}

void PrePostProcessor::NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetDupeCheck() && nzbInfo->GetDupeMode() != dmForce)
	{
		g_DupeCoordinator->NzbFound(downloadQueue, nzbInfo);
	}
}

void PrePostProcessor::NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_dad_prepost_is_skipped_delete_status(
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsDupe,
		(int)NzbInfo::dsCopy,
		(int)NzbInfo::dsGood,
		(int)NzbInfo::dsScan) != 0)
#else
	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsDupe ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsCopy ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsGood ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsScan)
#endif
	{
		NzbCompleted(downloadQueue, nzbInfo, false);
	}
	else
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbAdded);
	}
}

void PrePostProcessor::CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (IsStopped())
	{
		return;
	}
	// Direct rename can pause the last PAR volumes after the data files
	// finished. Their in-flight articles may still be draining; the last
	// such request does not complete a file, so check both transitions.
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_fuq_prepost_should_complete_after_direct_rename(
		0,
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsSuccess,
		nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		!nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
		nzbInfo->IsDownloadCompleted(true) ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0) != 0)
#else
	if (nzbInfo->GetDirectRenameStatus() == NzbInfo::tsSuccess &&
		!nzbInfo->GetPostInfo() && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		!nzbInfo->GetCompletedFiles()->empty() && nzbInfo->IsDownloadCompleted(true) &&
		!g_Options->GetRawArticle() && !g_Options->GetSkipWrite() && !nzbInfo->GetSkipDiskWrite())
#endif
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s completely downloaded", nzbInfo->GetName());
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
		NzbDownloaded(downloadQueue, nzbInfo);
	}
}

void PrePostProcessor::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (!nzbInfo)
	{
		return;
	}

#ifdef NZBGET_USE_RUST
	int enqueueDeleted = 0;
	int setParSkipped = 0;
	int dlRoute = nzbget_rs_hsr_prepost_classify_nzb_downloaded_entry(
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
#else
	if (nzbInfo->GetSkipScriptProcessing() && (nzbInfo->GetSkipDiskWrite() || g_Options->GetSkipWrite()))
	{
		if (nzbInfo->GetLiveRepairThread())
		{
			// no post-processing job will wait for the pass and the cleanup
			// below deletes the very files it writes: detach and stop it
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}
		// cleanup first: without history NzbCompleted frees the nzb
		nzbInfo->SetCleanupDisk(true);
		DeleteCleanup(nzbInfo);
		NzbCompleted(downloadQueue, nzbInfo, true);
		return;
	}

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsHealth ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsBad)
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}

	if (!nzbInfo->GetPostInfo() && !g_Options->GetRawArticle() && !g_Options->GetSkipWrite() && !nzbInfo->GetSkipDiskWrite())
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Queueing %s for post-processing", nzbInfo->GetName());

		nzbInfo->EnterPostProcess();

		if (nzbInfo->GetParStatus() == NzbInfo::psNone &&
			g_Options->GetParCheck() != Options::pcAlways &&
			g_Options->GetParCheck() != Options::pcForce)
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

		// a live stream-repair pass may still be writing into completed files:
		// the post-processing job (whose stream stage works on the same files)
		// waits for it; the pass clears the working flag when it detaches
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

		// We have more jobs in the queue, notify Run()
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
			// no post-processing job will wait for the pass: detach and stop
			// it before the collection moves to history
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}

		NzbCompleted(downloadQueue, nzbInfo, true);
	}
#endif
}

void PrePostProcessor::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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

#ifdef NZBGET_USE_RUST
	int newDelStatus = (int)nzbInfo->GetDeleteStatus();
	bool callDownloaded = nzbget_rs_hsr_prepost_resolve_nzb_deleted_route(
		newDelStatus,
		(int)NzbInfo::dsNone,
		(int)NzbInfo::dsManual,
		(int)NzbInfo::dsHealth,
		(int)NzbInfo::dsBad,
		&newDelStatus) != 0;
	nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)newDelStatus);
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
#else
	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
	}
	nzbInfo->SetDeleting(false);

	DeleteCleanup(nzbInfo);

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsHealth ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsBad)
	{
		NzbDownloaded(downloadQueue, nzbInfo);
	}
	else
	{
		NzbCompleted(downloadQueue, nzbInfo, true);
	}
#endif
}

void PrePostProcessor::NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool saveQueue)
{
#ifdef NZBGET_USE_RUST
	int addToHistoryInt = 0, callDupeCompletedInt = 0, enqueueDeletedScriptInt = 0, needSaveInt = 0;
	nzbget_rs_dsp_eval_nzb_completed(
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
#else
	bool downloadDupe = nzbInfo->GetDupeHint() == NzbInfo::dhRedownloadAuto;
	bool addToHistory = g_Options->GetKeepHistory() > 0 && !nzbInfo->GetAvoidHistory();
	if (addToHistory)
	{
		g_HistoryCoordinator->AddToHistory(downloadQueue, nzbInfo);
	}
	nzbInfo->SetAvoidHistory(false);

	bool needSave = addToHistory;

	if (g_Options->GetDupeCheck() && (nzbInfo->GetDupeMode() != dmForce || DupeCoordinator::FailsOver(nzbInfo)) &&
		(nzbInfo->GetDeleteStatus() == NzbInfo::dsNone ||
		 nzbInfo->GetDeleteStatus() == NzbInfo::dsHealth ||
		 nzbInfo->GetDeleteStatus() == NzbInfo::dsBad ||
		 nzbInfo->GetDeleteStatus() == NzbInfo::dsScan ||
		 (nzbInfo->GetDeleteStatus() == NzbInfo::dsCopy && downloadDupe)))
	{
		g_DupeCoordinator->NzbCompleted(downloadQueue, nzbInfo);
		needSave = true;
	}

	if (nzbInfo->GetDeleteStatus() > NzbInfo::dsNone &&
		nzbInfo->GetDeleteStatus() != NzbInfo::dsHealth &&
		nzbInfo->GetDeleteStatus() != NzbInfo::dsBad)
		// nzbs deleted by health check or marked as bad are processed as downloaded with failure status
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}
#endif

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

void PrePostProcessor::DeleteCleanup(NzbInfo* nzbInfo)
{
#ifdef NZBGET_USE_RUST
	int cleanupMode = nzbget_rs_hsr_prepost_classify_delete_cleanup_mode(
		nzbInfo->GetCleanupDisk() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsDupe,
		nzbInfo->GetParking() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0);
	if (cleanupMode != 0)
	{
		if (cleanupMode == 1)
#else
	if (nzbInfo->GetCleanupDisk() ||
		(nzbInfo->GetDeleteStatus() == NzbInfo::dsDupe && !nzbInfo->GetParking()))
	{
		if (nzbInfo->GetSkipDiskWrite() || g_Options->GetSkipWrite())
#endif
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
			BString<1024> fullFileName("%s%c%s", nzbInfo->GetDestDir(), PATH_SEPARATOR, completedFile.GetFilename());
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
#ifdef NZBGET_USE_RUST
			if (nzbget_rs_pup_history_coord_is_temporary_output_name(filename) != 0)
#else
			int len = strlen(filename);
			if (len > 8 && !strcmp(filename + len - 8, ".out.tmp"))
#endif
			{
				BString<1024> fullFilename("%s%c%s", nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
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

void PrePostProcessor::CheckRequestPar(DownloadQueue* downloadQueue)
{
#ifndef DISABLE_PARCHECK
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	for (NzbInfo* postJob : m_activeJobs)
	{
		PostInfo* postInfo = postJob->GetPostInfo();

#ifdef NZBGET_USE_RUST
		int newForceParFull = postInfo->GetForceParFull() ? 1 : 0;
		int reqAction = nzbget_rs_hsr_prepost_classify_request_par_action(
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
#else
		if (postInfo->GetRequestParCheck() &&
			(postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped ||
			(postInfo->GetForceRepair() && !postInfo->GetNzbInfo()->GetParFull())) &&
			g_Options->GetParCheck() != Options::pcManual)
		{
			postInfo->SetForceParFull(postInfo->GetForceParFull() ||
				postInfo->GetNzbInfo()->GetParStatus() > NzbInfo::psSkipped);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psNone);
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
			// the job starts over (par-check, then the scripts again): a script
			// asking for it (exit 92) had finished the job, which then went to
			// the history without the par-check
			postInfo->SetStage(PostInfo::ptQueued);
			postInfo->SetWorking(false);
		}
		else if (postInfo->GetRequestParCheck() &&
			postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped &&
			g_Options->GetParCheck() == Options::pcManual)
		{
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psManual);

			if (!postInfo->GetNzbInfo()->GetFileList()->empty())
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
#endif
	}
#endif
}

void PrePostProcessor::CleanupJobs(DownloadQueue* downloadQueue)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	m_activeJobs.erase(std::remove_if(m_activeJobs.begin(), m_activeJobs.end(),
		[processor = this, downloadQueue](NzbInfo* postJob)
		{
			PostInfo* postInfo = postJob->GetPostInfo();
			if (!postInfo->GetWorking() &&
				!(postInfo->GetPostThread() && postInfo->GetPostThread()->IsRunning()))
			{
				delete postInfo->GetPostThread();
				postInfo->SetPostThread(nullptr);

				postInfo->SetStageTime(0);
				postInfo->SetStageProgress(0);
				postInfo->SetFileProgress(0);
				postInfo->SetProgressLabel("");

				if (postInfo->GetStartTime() > 0)
				{
					postJob->SetPostTotalSec(postJob->GetPostTotalSec() +
						(int)(Util::CurrentTime() - postInfo->GetStartTime()));
					postInfo->SetStartTime(0);
				}

				if (postInfo->GetStage() == PostInfo::ptFinished || postInfo->GetDeleted())
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

bool PrePostProcessor::CanRunMoreJobs(bool* allowPar)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
#ifdef NZBGET_USE_RUST
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	int allowParInt = 0;
	int canRun = nzbget_rs_dsp_can_run_more_jobs(
		stages.data(),
		stages.size(),
		(int)PostInfo::ptLoadingPars,
		(int)PostInfo::ptVerifyingRepaired,
		(int)PostInfo::ptRepairing,
		(int)g_Options->GetPostStrategy(),
		&allowParInt);
	*allowPar = allowParInt != 0;
	return canRun != 0;
#else
	int totalJobs = (int)m_activeJobs.size();
	int parJobs = 0;
	int otherJobs = 0;
	bool repairJobs = false;

	for (NzbInfo* postJob : m_activeJobs)
	{
		bool parJob = postJob->GetPostInfo()->GetStage() >= PostInfo::ptLoadingPars &&
			postJob->GetPostInfo()->GetStage() <= PostInfo::ptVerifyingRepaired;
		repairJobs |= postJob->GetPostInfo()->GetStage() == PostInfo::ptRepairing;
		parJobs += parJob ? 1 : 0;
		otherJobs += parJob ? 0 : 1;
	}

	switch (g_Options->GetPostStrategy())
	{
		case Options::ppSequential:
			*allowPar = true;
			return totalJobs == 0;

		case Options::ppBalanced:
			*allowPar = parJobs == 0;
			return otherJobs == 0 && (parJobs == 0 || repairJobs);

		case Options::ppAggressive:
			*allowPar = parJobs < 1;
			return totalJobs < 3;

		case Options::ppRocket:
			*allowPar = parJobs < 2;
			return totalJobs < 6;
	}

	return false;
#endif
}

NzbInfo* PrePostProcessor::PickNextJob(DownloadQueue* downloadQueue, bool allowPar)
{
	NzbInfo* nzbInfo = nullptr;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);

	bool hasActiveUnpack = HasActiveUnpack(downloadQueue);

#ifdef NZBGET_USE_RUST
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
	int picked = nzbget_rs_dsp_pick_next_post_job(
		candidates.data(),
		candidates.size(),
		allowPar ? 1 : 0,
		hasActiveUnpack ? 1 : 0,
		g_WorkState->GetPausePostProcess() ? 1 : 0);
	if (picked >= 0 && (size_t)picked < items.size())
	{
		nzbInfo = items[(size_t)picked];
	}
#else
	for (NzbInfo* nzbInfo1: downloadQueue->GetQueue())
	{
		if (nzbInfo1->GetPostInfo() && !nzbInfo1->GetPostInfo()->GetWorking() &&
			!g_QueueScriptCoordinator->HasJob(nzbInfo1->GetId(), nullptr) &&
			nzbInfo1->GetDirectUnpackStatus() != NzbInfo::nsRunning &&
			// never start post-processing while a live stream-repair pass still
			// writes into the collection's completed files (the working flag
			// alone is not enough: it is shared with direct unpack, whose exit
			// must not release a job the live pass is still holding)
			!nzbInfo1->GetLiveRepairThread() &&
			(!nzbInfo || nzbInfo1->GetPriority() > nzbInfo->GetPriority()) &&
			(!g_WorkState->GetPausePostProcess() || nzbInfo1->GetForcePriority()) &&
			(allowPar || !nzbInfo1->GetPostInfo()->GetNeedParCheck()) &&
			(!hasActiveUnpack || !nzbInfo1->GetPostInfo()->GetNeedUnpack()) &&
			(std::find(m_activeJobs.begin(), m_activeJobs.end(), nzbInfo1) == m_activeJobs.end()) &&
			nzbInfo1->IsDownloadCompleted(true))
		{
			nzbInfo = nzbInfo1;
		}
	}
#endif

	return nzbInfo;
}

void PrePostProcessor::CheckPostQueue()
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
static bool ParCannotCover(NzbInfo* nzbInfo)
{
	int health = nzbInfo->CalcHealth();
	int criticalHealth = nzbInfo->CalcCriticalHealth(false);
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_hsr_prepost_should_skip_par_cannot_cover_check(health, criticalHealth) != 0)
#else
	if (health > 0 && (criticalHealth >= 1000 || health >= criticalHealth))
#endif
	{
		return false;
	}

	ParParser::ParFileList mainPars;
	if (!ParParser::FindMainPars(nzbInfo->GetDestDir(), &mainPars) || mainPars.size() != 1)
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
			BString<1024> path("%s%c%s", nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
			int64 size = FileSystem::FileSize(path);
			if (size > 0 && (parFileSize < 0 || size < parFileSize))
			{
				parPath = *path;
				parFileSize = size;
			}
		}
	}

	std::vector<std::string> parredFiles;
	if (parFileSize < 0 || !ParRenamer::ListParredFiles(parPath.c_str(), parredFiles))
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
			[baseName](const std::string& parredFile) { return !strcasecmp(parredFile.c_str(), baseName); }))
		{
			// a whole-file job has no decoded geometry yet: its encoded size
			// is the closest measure of its damage
#ifdef NZBGET_USE_RUST
			int64 holesTotal = job.GetDecodedFileSize() > 0 ?
				DupeStreamRepair::TotalSize(*job.GetHoles()) : 0;
			protectedDamage += nzbget_rs_hsr_prepost_calc_job_protected_damage(
				job.GetDecodedFileSize(), holesTotal, job.GetFailedSize(), job.GetMissedSize());
#else
			protectedDamage += job.GetDecodedFileSize() > 0 ?
				DupeStreamRepair::TotalSize(*job.GetHoles()) : job.GetFailedSize() + job.GetMissedSize();
#endif
		}
	}

#ifdef NZBGET_USE_RUST
	return nzbget_rs_hsr_prepost_is_damage_beyond_par(
		protectedDamage, nzbInfo->GetParSize(), nzbInfo->GetParCurrentFailedSize()) != 0;
#else
	return protectedDamage > nzbInfo->GetParSize() - nzbInfo->GetParCurrentFailedSize();
#endif
}

// ParScan=dupe can still find missing blocks in the files of a duplicate
// already downloaded to disk, which the health estimate does not know about
static bool HasDupeScanSources(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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

void PrePostProcessor::StartJob(DownloadQueue* downloadQueue, PostInfo* postInfo, bool allowPar)
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
	if (streamRepair && !postInfo->GetStreamRepairDone() && g_Options->GetHealthCheck() == Options::hcDupe &&
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
		nzbInfo->GetParStatus() <= NzbInfo::psSkipped &&
		g_Options->GetParCheck() == Options::pcAuto &&
		ParCannotCover(nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Par2 files of %s cannot cover its damage (health %.1f%%, critical %.1f%%), "
			"recovering from duplicates before par-check",
			nzbInfo->GetName(), nzbInfo->CalcHealth() / 10.0, nzbInfo->CalcCriticalHealth(false) / 10.0);
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
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_hsr_prepost_should_check_unrecovered_stream_skip(
		postInfo->GetParCannotCover() ? 1 : 0,
		postInfo->GetStreamRepairDone() ? 1 : 0,
		postInfo->GetStreamRepairRecovered() ? 1 : 0,
		nzbInfo->GetDupeRecoveredBytes(),
		(int)g_Options->GetParCheck(),
		(int)Options::pcAuto,
		(int)nzbInfo->GetParStatus(),
		(int)NzbInfo::psNone,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone) != 0 &&
		!HasDupeScanSources(downloadQueue, nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
#else
	if (postInfo->GetParCannotCover() && postInfo->GetStreamRepairDone() &&
		!postInfo->GetStreamRepairRecovered() && nzbInfo->GetDupeRecoveredBytes() == 0 &&
		g_Options->GetParCheck() == Options::pcAuto &&
		nzbInfo->GetParStatus() == NzbInfo::psNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		!HasDupeScanSources(downloadQueue, nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
#endif
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping par-check for %s: its par2 files cannot cover the damage "
			"(health %.1f%%, critical %.1f%%) and duplicates recovered nothing",
			nzbInfo->GetName(), nzbInfo->CalcHealth() / 10.0, nzbInfo->CalcCriticalHealth(false) / 10.0);
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
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone && nzbInfo->CalcHealth() < 1000 &&
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
	
#ifdef NZBGET_USE_RUST
	int skippedParAction = nzbget_rs_hsr_prepost_classify_skipped_par_health(
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
				nzbInfo->CalcHealth() / 10.0, nzbInfo->CalcCriticalHealth(false) / 10.0);
		}
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}
	if (skippedParAction == 3 &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s with health %.1f%% needs par-check",
			nzbInfo->GetName(), nzbInfo->CalcHealth() / 10.0);
		postInfo->SetRequestParCheck(true);
		return;
	}
#else
	if (nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		((g_Options->GetParScan() != Options::psDupe &&
		  nzbInfo->CalcHealth() < nzbInfo->CalcCriticalHealth(false) &&
		  nzbInfo->CalcCriticalHealth(false) < 1000) ||
		  nzbInfo->CalcHealth() == 0) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		if (nzbInfo->CalcHealth() == 0)
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health 0%%", nzbInfo->GetName());
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health %.1f%% below critical %.1f%%",
				nzbInfo->GetName(),
				nzbInfo->CalcHealth() / 10.0, nzbInfo->CalcCriticalHealth(false) / 10.0);
		}
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}
	
	if (nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		nzbInfo->GetFailedSize() - nzbInfo->GetParFailedSize() > 0 &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s with health %.1f%% needs par-check",
			nzbInfo->GetName(), nzbInfo->CalcHealth() / 10.0);
		postInfo->SetRequestParCheck(true);
		return;
	}
#endif
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
#ifdef NZBGET_USE_RUST
	int wantUnpackInt = 0;
	int unpackInt = 0;
	bool startRarRename = nzbget_rs_hsr_prepost_eval_want_unpack_and_rar_rename(
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
#else
	bool wantUnpack = !(unpackParameter && !strcasecmp(unpackParameter->GetValue(), "no"));
	bool unpack = wantUnpack && nzbInfo->GetUnpackStatus() == NzbInfo::usNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone;

	if (nzbInfo->GetRarRenameStatus() == NzbInfo::rsNone &&
		unpack && g_Options->GetRarRename())
#endif
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

#ifdef NZBGET_USE_RUST
	bool inInterDir = !Util::EmptyStr(g_Options->GetInterDir()) &&
		!strncmp(nzbInfo->GetDestDir(), g_Options->GetInterDir(), strlen(g_Options->GetInterDir())) &&
		nzbInfo->GetDestDir()[strlen(g_Options->GetInterDir())] == PATH_SEPARATOR;
	NzbgetRsPostStageDecision stageDec{};
	nzbget_rs_dsp_eval_post_stages(
		wantUnpack ? 1 : 0,
		(int)nzbInfo->GetUnpackStatus(),
		(int)nzbInfo->GetDeleteStatus(),
		(int)nzbInfo->GetParStatus(),
		(int)nzbInfo->GetCleanupStatus(),
		!Util::EmptyStr(g_Options->GetExtCleanupDisk()) ? 1 : 0,
		nzbInfo->CalcHealth(),
		(int)nzbInfo->GetMoveStatus(),
		inInterDir ? 1 : 0,
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
#else
	bool parFailed = nzbInfo->GetParStatus() == NzbInfo::psFailure ||
		nzbInfo->GetParStatus() == NzbInfo::psRepairPossible ||
		nzbInfo->GetParStatus() == NzbInfo::psManual;

	bool cleanup = !unpack && wantUnpack &&
		nzbInfo->GetCleanupStatus() == NzbInfo::csNone &&
		!Util::EmptyStr(g_Options->GetExtCleanupDisk()) &&
		((nzbInfo->GetParStatus() == NzbInfo::psSuccess &&
		  nzbInfo->GetUnpackStatus() != NzbInfo::usFailure &&
		  nzbInfo->GetUnpackStatus() != NzbInfo::usSpace &&
		  nzbInfo->GetUnpackStatus() != NzbInfo::usPassword) ||
		 (nzbInfo->GetUnpackStatus() == NzbInfo::usSuccess &&
		  nzbInfo->GetParStatus() != NzbInfo::psFailure) ||
		 ((nzbInfo->GetUnpackStatus() == NzbInfo::usNone ||
		   nzbInfo->GetUnpackStatus() == NzbInfo::usSkipped) &&
		  (nzbInfo->GetParStatus() == NzbInfo::psNone ||
		   nzbInfo->GetParStatus() == NzbInfo::psSkipped) &&
		  nzbInfo->CalcHealth() == 1000));

	bool moveInter = !unpack &&
		nzbInfo->GetMoveStatus() == NzbInfo::msNone &&
		nzbInfo->GetUnpackStatus() != NzbInfo::usFailure &&
		nzbInfo->GetUnpackStatus() != NzbInfo::usSpace &&
		nzbInfo->GetUnpackStatus() != NzbInfo::usPassword &&
		nzbInfo->GetParStatus() != NzbInfo::psFailure &&
		nzbInfo->GetParStatus() != NzbInfo::psManual &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		!(((nzbInfo->GetUnpackStatus() == NzbInfo::usNone ||
		   nzbInfo->GetUnpackStatus() == NzbInfo::usSkipped) &&
		  (nzbInfo->GetParStatus() == NzbInfo::psNone ||
		   nzbInfo->GetParStatus() == NzbInfo::psSkipped) &&
		  nzbInfo->CalcHealth() < 1000)) &&
		!Util::EmptyStr(g_Options->GetInterDir()) &&
		!strncmp(nzbInfo->GetDestDir(), g_Options->GetInterDir(), strlen(g_Options->GetInterDir())) &&
		nzbInfo->GetDestDir()[strlen(g_Options->GetInterDir())] == PATH_SEPARATOR;

	if (unpack && parFailed && nzbInfo->GetDirectUnpackStatus() != NzbInfo::nsSuccess)
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping unpack for %s due to %s", nzbInfo->GetName(),
			nzbInfo->GetParStatus() == NzbInfo::psManual ? "required par-repair" : "par-failure");
		nzbInfo->SetUnpackStatus(NzbInfo::usSkipped);
		unpack = false;
	}

	bool postDownloadRenaming = g_Options->GetPostDownloadRename() &&
		nzbInfo->GetPostDownloadRenamingStatus() == NzbInfo::PostDownloadRenamingStatus::None &&
		!Util::EmptyStr(nzbInfo->GetDestDir()) &&
		!Util::EmptyStr(nzbInfo->GetName()) &&
		(nzbInfo->GetUnpackStatus() == NzbInfo::usNone || nzbInfo->GetUnpackStatus() == NzbInfo::usSkipped) &&
		nzbInfo->GetUnpackStatus() != NzbInfo::usFailure &&
		nzbInfo->GetUnpackStatus() != NzbInfo::usSpace &&
		nzbInfo->GetUnpackStatus() != NzbInfo::usPassword &&
		nzbInfo->GetParStatus() != NzbInfo::psFailure &&
		nzbInfo->GetParStatus() != NzbInfo::psManual &&
		!moveInter &&
		(nzbInfo->GetMoveStatus() == NzbInfo::msSuccess || nzbInfo->GetMoveStatus() == NzbInfo::msNone);

	bool postUnpackRenaming = g_Options->GetRenameAfterUnpack() &&
		nzbInfo->GetPostUnpackRenamingStatus() == NzbInfo::PostUnpackRenamingStatus::None &&
		!Util::EmptyStr(nzbInfo->GetDestDir()) &&
		!Util::EmptyStr(nzbInfo->GetName()) &&
		nzbInfo->GetUnpackStatus() == NzbInfo::usSuccess &&
		nzbInfo->GetParStatus() != NzbInfo::psFailure &&
		nzbInfo->GetParStatus() != NzbInfo::psManual &&
		!moveInter &&
		(nzbInfo->GetMoveStatus() == NzbInfo::msSuccess || nzbInfo->GetMoveStatus() == NzbInfo::msNone);
#endif

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
		PostDownloadRenamer::Controller::StartJob(postInfo);
	}
	else if (postUnpackRenaming)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptPostUnpackRenaming);
		PostUnpackRenamer::Controller::StartJob(postInfo);
	}
	else
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptExecutingScript);
		PostScriptController::StartJob(postInfo);
	}
}

void PrePostProcessor::EnterStage(DownloadQueue* downloadQueue, PostInfo* postInfo, PostInfo::EStage stage)
{
	postInfo->SetWorking(true);
	postInfo->SetStage(stage);
}

void PrePostProcessor::JobCompleted(DownloadQueue* downloadQueue, PostInfo* postInfo)
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

void PrePostProcessor::UpdatePauseState()
{
	bool needPause = false;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
#ifdef NZBGET_USE_RUST
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	needPause = nzbget_rs_dsp_calc_post_pause_state(
		stages.data(),
		stages.size(),
		g_Options->GetParPauseQueue() ? 1 : 0,
		g_Options->GetUnpackPauseQueue() ? 1 : 0,
		g_Options->GetScriptPauseQueue() ? 1 : 0) != 0;
#else
	for (NzbInfo* postJob : m_activeJobs)
	{
		switch (postJob->GetPostInfo()->GetStage())
		{
			case PostInfo::ptLoadingPars:
			case PostInfo::ptVerifyingSources:
			case PostInfo::ptRepairing:
			case PostInfo::ptVerifyingRepaired:
			case PostInfo::ptParRenaming:
			case PostInfo::ptStreamRepairing:
				needPause |= g_Options->GetParPauseQueue();
				break;

			case PostInfo::ptRarRenaming:
			case PostInfo::ptUnpacking:
			case PostInfo::ptCleaningUp:
			case PostInfo::ptMoving:
			case PostInfo::ptPostUnpackRenaming:
			case PostInfo::ptPostDownloadRenaming:
				needPause |= g_Options->GetUnpackPauseQueue();
				break;

			case PostInfo::ptExecutingScript:
				needPause |= g_Options->GetScriptPauseQueue();
				break;

			case PostInfo::ptQueued:
			case PostInfo::ptFinished:
				break;
		}
	}
#endif

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

bool PrePostProcessor::EditList(DownloadQueue* downloadQueue, IdList* idList,
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

bool PrePostProcessor::PostQueueDelete(DownloadQueue* downloadQueue, IdList* idList)
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

void PrePostProcessor::FileDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileInfo* fileInfo)
{
	if (!nzbInfo->GetPostInfo())
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeFileDownloaded);
	}

#ifdef NZBGET_USE_RUST
	int duAction = nzbget_rs_hsr_prepost_classify_file_downloaded_direct_unpack(
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
			bool wantUnpack = !(unpackParameter && !strcasecmp(unpackParameter->GetValue(), "no"));
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
#else
	if (g_Options->GetDirectUnpack() && !g_Options->GetRawArticle() && !g_Options->GetSkipWrite() && !nzbInfo->GetSkipDiskWrite())
	{
		bool allowPar;
		if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsNone &&
			nzbInfo->GetDirectRenameStatus() != NzbInfo::tsRunning &&
			DirectUnpack::IsArchiveFilename(fileInfo->GetFilename()) &&
			CanRunMoreJobs(&allowPar) &&
			!HasActiveUnpack(downloadQueue))
		{
			NzbParameter* unpackParameter = nzbInfo->GetParameters()->Find("*Unpack:");
			bool wantUnpack = !(unpackParameter && !strcasecmp(unpackParameter->GetValue(), "no"));
			if (wantUnpack && nzbInfo->GetFailedArticles() == 0)
			{
				DirectUnpack::StartJob(nzbInfo);
			}
		}
		else if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsRunning && nzbInfo->GetUnpackThread())
		{
			((DirectUnpack*)nzbInfo->GetUnpackThread())->FileDownloaded(downloadQueue, fileInfo);
		}
	}
#endif
}
