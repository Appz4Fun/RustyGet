/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@users.sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "QueueCoordinator.h"
#include "Options.h"
#include "WorkState.h"
#include "ServerPool.h"
#include "ArticleDownloader.h"
#include "ArticleWriter.h"
#include "DiskState.h"
#include "Util.h"
#include "FileSystem.h"
#include "Decoder.h"
#include "StatMeter.h"
#include "Deobfuscation.h"
#include "DupeCoordinator.h"
#include "DupeProbe.h"
#include "TwinCheck.h"
#include "DupeStreamRepair.h"
#include "ParDamage.h"
#include "DupeArticleFallback.h"
#include "StreamRepair.h"
#include "PrePostProcessor.h"
#include "nzbget_rs.h"

bool QueueCoordinator::CoordinatorDownloadQueue::EditEntry(
	int ID, EEditAction action, const char* args)
{
	return m_owner->m_queueEditor.EditEntry(&m_owner->m_downloadQueue, ID, action, args);
}

bool QueueCoordinator::CoordinatorDownloadQueue::EditList(
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

void QueueCoordinator::CoordinatorDownloadQueue::Save()
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

void QueueCoordinator::CoordinatorDownloadQueue::SaveChanged()
{
	if (g_Options->GetServerMode())
	{
		g_DiskState->SaveDownloadProgress(this);
		m_stateChanged = true;
	}
}

QueueCoordinator::QueueCoordinator()
{
	debug("Creating QueueCoordinator");

	CoordinatorDownloadQueue::Init(&m_downloadQueue);
	g_WorkState->Attach(this);
	// a reload creates a new coordinator: probes may run again
	DupeProbe::Reset();
	TwinCheck::Reset();
}

QueueCoordinator::~QueueCoordinator()
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

void QueueCoordinator::Load()
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
					if (nzbget_rs_qci_queue_coord_should_resave_completed_file_state(
						(int)completedFile.GetStatus(),
						(int)CompletedFile::cfPartial,
						(int)CompletedFile::cfFailure,
						completedFile.GetId()) != 0)
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

void QueueCoordinator::Run()
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
			m_hasMoreJobs = nzbget_rs_qcf_queue_coord_has_more_jobs(
				hasMoreArticles ? 1 : 0, articeDownloadsRunning ? 1 : 0) != 0;
			int effectiveLimit = g_MemoryMonitor
				? g_MemoryMonitor->GetEffectiveDownloadsLimit(m_downloadsLimit)
				: m_downloadsLimit;
			bool canProceed = nzbget_rs_qcf_queue_coord_can_proceed_downloads(
				IsStopped() ? 1 : 0, m_activeDownloads.size(), effectiveLimit) != 0;

			if (nzbget_rs_fuq_queue_coord_can_start_article(
				hasMoreArticles ? 1 : 0, canProceed ? 1 : 0,
				g_WorkState->GetDownloadHeld() ? 1 : 0,
				(fileInfo && fileInfo->GetExtraPriority()) ? 1 : 0) != 0)
			{
				NntpConnection* connection = nullptr;
			
				bool hasDesiredServer = fileInfo->GetNzbInfo()->HasDesiredServer();
				NewsServer* desiredServer = hasDesiredServer ?
					g_ServerPool->GetServerById(fileInfo->GetNzbInfo()->GetDesiredServerId()) : nullptr;
				int connLevel = 0;
				bool useSpecificServer = nzbget_rs_qci_queue_coord_resolve_desired_server_level(
					hasDesiredServer ? 1 : 0,
					desiredServer ? 1 : 0,
					(desiredServer && desiredServer->GetActive()) ? 1 : 0,
					desiredServer ? desiredServer->GetNormLevel() : -1,
					&connLevel) != 0;
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
			waitInterval = nzbget_rs_fuq_queue_coord_next_standby_wait(waitInterval);

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
			int sleepInterval = nzbget_rs_fuq_queue_coord_active_sleep_interval(downloadStarted ? 1 : 0);
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

void QueueCoordinator::WakeUp()
{
	debug("Waking up QueueCoordinator");
	// Resume Run()
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_hasMoreJobs = true;
	m_wakeUp = true;
	m_waitCond.notify_all();
}

void QueueCoordinator::WaitJobs()
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
void QueueCoordinator::AdjustDownloadsLimit()
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

NzbInfo* QueueCoordinator::AddNzbFileToQueue(std::unique_ptr<NzbInfo> nzbInfo, NzbInfo* urlInfo, bool addFirst)
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
		if (nzbget_rs_qcf_queue_coord_should_run_check_dupe_file_infos(
			(int)deleteStatus,
			(int)NzbInfo::dsNone,
			g_Options->GetDupeCheck() ? 1 : 0,
			(int)nzbInfo->GetDupeMode(),
			(int)dmForce) != 0)
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

	if (nzbget_rs_qcf_queue_coord_should_notify_nzb_added(
		(int)deleteStatus, (int)NzbInfo::dsManual) != 0)
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

void QueueCoordinator::CheckDupeFileInfos(NzbInfo* nzbInfo)
{
	debug("CheckDupeFileInfos");

	// the files of each name, in list order: comparing every pair, and counting the
	// name over the whole list for each, took 24 s for 2,000 files of one name (F15)
	std::unordered_map<std::string, std::vector<std::pair<int, FileInfo*>>> byName;
	int index = 0;
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		byName[fileInfo->GetFilename()].emplace_back(++index, fileInfo);
	}

	RawFileList dupeList;
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		const auto& same = byName[fileInfo->GetFilename()];
		// If more than two files have same filename we don't filter them out since that
		// naming might be intentional and correct filenames must be read from article bodies.
		if (same.size() != 2)
		{
			continue;
		}
		const auto& self = same[0].second == fileInfo ? same[0] : same[1];
		const auto& other = same[0].second == fileInfo ? same[1] : same[0];
		if (nzbget_rs_fuq_queue_coord_is_smaller_dupe_file(
			fileInfo->GetSize(), self.first, other.second->GetSize(), other.first) != 0)
		{
			warn("File \"%s\" appears twice in collection, adding only the biggest file", fileInfo->GetFilename());
			dupeList.push_back(fileInfo);
		}
	}

	for (FileInfo* fileInfo : dupeList)
	{
		int fileId = fileInfo->GetId();
		nzbInfo->UpdateDeletedStats(fileInfo);
		nzbInfo->GetFileList()->Remove(fileInfo);
		if (g_Options->GetServerMode())
		{
			g_DiskState->DiscardFile(fileId, true, false, false);
		}
	}
}

void QueueCoordinator::Stop()
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
bool QueueCoordinator::GetNextArticle(DownloadQueue* downloadQueue, FileInfo* &fileInfo, ArticleInfo* &articleInfo)
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
			bool nzbHigherPriority = nzbget_rs_fuq_queue_coord_is_higher_nzb_priority(
				fileInfo != nullptr ? 1 : 0,
				nzbInfo->HasExtraPriority() ? 1 : 0,
				(fileInfo && fileInfo->GetNzbInfo()->HasExtraPriority()) ? 1 : 0,
				nzbInfo->GetPriority(),
				fileInfo ? fileInfo->GetNzbInfo()->GetPriority() : 0) != 0;

			bool nzbPaused = nzbget_rs_cwp_queue_coord_is_nzb_paused(
				(int)nzbInfo->GetFileList()->size(), nzbInfo->GetPausedFileCount()) != 0;

			if ((!fileInfo || nzbHigherPriority) && !nzbPaused &&
				!HasActiveDupe(downloadQueue, nzbInfo) &&
				(!(g_WorkState->GetPauseDownload() || g_WorkState->GetQuotaReached()) || nzbInfo->GetForcePriority()))
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
				nzbget_rs_qci_queue_coord_calc_propagation_wait_log_mins(
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

		if (nzbget_rs_qci_queue_coord_should_fetch_first_article_for_rename(
			g_Options->GetDirectRename() ? 1 : 0,
			(int)fileInfo->GetNzbInfo()->GetDirectRenameStatus(),
			(int)NzbInfo::tsRunning,
			fileInfo->GetNzbInfo()->GetAllFirst() ? 1 : 0) != 0 &&
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
				if (nzbget_rs_cwp_queue_coord_should_try_proactive_cutover(
					fileInfo->GetDupeCutover() ? 1 : 0, article->GetDupeFallbackRound()) != 0)
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

bool QueueCoordinator::IsPropagationWait(time_t postTime, int propagationDelay, time_t curDate)
{
	return nzbget_rs_qhp_is_propagation_wait((int64_t)postTime, propagationDelay, (int64_t)curDate) != 0;
}

bool QueueCoordinator::GetNextFirstArticle(NzbInfo* nzbInfo, FileInfo* &fileInfo, ArticleInfo* &articleInfo)
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
			if (!fileInfo1->GetArticles()->empty())
			{
				ArticleInfo* article = fileInfo1->GetArticles()->at(0).get();
				if (article->GetStatus() == ArticleInfo::aiUndefined)
				{
					fileInfo = fileInfo1;
					articleInfo = article;
					nzbInfo->SetDirectRenameStatus(NzbInfo::tsRunning);
					return true;
				}
			}
		}
	}

	// no more files for renaming remained
	nzbInfo->SetAllFirst(true);

	return false;
}

void QueueCoordinator::StartArticleDownload(FileInfo* fileInfo, ArticleInfo* articleInfo, NntpConnection* connection)
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
	if (nzbget_rs_fuq_queue_coord_should_analyze_first_article(
		articleInfo->GetPartNumber(), g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetDirectRename() ? 1 : 0,
		g_Options->GetDupeArticleFallback() != Options::dafNone ? 1 : 0) != 0)
	{
		articleDownloader->SetContentAnalyzer(m_directRenamer.MakeArticleContentAnalyzer());
	}

	BString<1024> infoName("%s%c%s [%i/%i]", fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, fileInfo->GetFilename(), articleInfo->GetPartNumber(), (int)fileInfo->GetArticles()->size());
	articleDownloader->SetInfoName(infoName);

	articleInfo->SetStatus(ArticleInfo::aiRunning);
	fileInfo->SetActiveDownloads(fileInfo->GetActiveDownloads() + 1);
	fileInfo->GetNzbInfo()->SetActiveDownloads(fileInfo->GetNzbInfo()->GetActiveDownloads() + 1);

	m_activeDownloads.push_back(articleDownloader);
	articleDownloader->Start();
}

void QueueCoordinator::Update(Subject* caller, void* aspect)
{
	if (caller == g_WorkState)
	{
		debug("Notification from WorkState received");
		WakeUp();
		return;
	}

	debug("Notification from ArticleDownloader received");

	ArticleDownloader* articleDownloader = static_cast<ArticleDownloader*>(caller);
	if (nzbget_rs_fuq_queue_coord_is_article_completed_status(
		(int)articleDownloader->GetStatus(),
		(int)ArticleDownloader::adFinished,
		(int)ArticleDownloader::adFailed,
		(int)ArticleDownloader::adRetry,
		(int)ArticleDownloader::adFatalError) != 0)
	{
		ArticleCompleted(articleDownloader);
	}
}

void QueueCoordinator::ArticleCompleted(ArticleDownloader* articleDownloader)
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
		bool misplaced = nzbget_rs_qci_queue_coord_is_misplaced_donor_article(
			(int)articleDownloader->GetStatus(),
			(int)ArticleDownloader::adFinished,
			articleInfo->GetDupeFallbackRound(),
			articleDownloader->GetDecodedFileSize(),
			(!checkAligned || DupeArticleFallback::SegmentAligned(fileInfo, articleInfo)) ? 1 : 0) != 0;
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
			if (nzbget_rs_qci_queue_coord_should_reset_dupe_failed_run(
				articleInfo->GetDupeFallbackRound(),
				articleInfo->GetMessageId(),
				articleInfo->GetDupeOriginalMessageId()) != 0)
			{
				nzbInfo->SetDupeFailedRun(0);
			}
			{
				int64 nextFileSuccessSize = fileInfo->GetSuccessSize();
				int64 nextNzbCurSuccessSize = nzbInfo->GetCurrentSuccessSize();
				int64 nextNzbParCurSuccessSize = nzbInfo->GetParCurrentSuccessSize();
				int nextFileSuccessArticles = fileInfo->GetSuccessArticles();
				int nextNzbCurSuccessArticles = nzbInfo->GetCurrentSuccessArticles();
				nzbget_rs_qcf_queue_coord_apply_success_article_counters(
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
				articleInfo->SetDupeDonorId(nzbget_rs_cwp_queue_coord_select_dupe_donor_id(
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
			if (nzbget_rs_qci_queue_coord_eval_dupe_recovery_and_cutover(
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
			if (nzbget_rs_qcf_queue_coord_should_revalidate_dupe_neighbors(
				fileInfo->GetDupeRecoveredArticles(),
				articleDownloader->GetDecodedFileSize()) != 0)
			{
				DemoteMisalignedDupeNeighbors(fileInfo, articleInfo);
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
		else if (articleDownloader->GetStatus() == ArticleDownloader::adFailed || misplaced ||
			articleDownloader->GetStatus() == ArticleDownloader::adFatalError)
		{
			articleInfo->SetStatus(ArticleInfo::aiFailed);
			// par2 volumes aside: a lost one says nothing about the data
			int nextFailedRun = nzbInfo->GetDupeFailedRun();
			int64 nextFileFailedSize = fileInfo->GetFailedSize();
			int64 nextNzbCurFailedSize = nzbInfo->GetCurrentFailedSize();
			int64 nextNzbParCurFailedSize = nzbInfo->GetParCurrentFailedSize();
			int nextFileFailedArticles = fileInfo->GetFailedArticles();
			int nextNzbCurFailedArticles = nzbInfo->GetCurrentFailedArticles();
			nzbget_rs_cwp_queue_coord_apply_failed_article_counters(
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
		if (nzbget_rs_qcf_queue_coord_should_account_attempt_server_stats(
			(int)articleDownloader->GetStatus(),
			(int)ArticleDownloader::adRetry) != 0)
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
			fileCompleted = nzbget_rs_qci_queue_coord_advance_article_completion_counters(
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
			bool useFilenameFromArticle = nzbget_rs_fuq_queue_coord_should_use_article_filename(
				(int)g_Options->GetFileNaming(), (int)Options::nfArticle, (int)Options::nfAuto,
				g_Options->GetFileNaming() == Options::nfAuto &&
					Deobfuscation::IsExcessivelyObfuscated(articleDownloader->GetArticleFilename()) ? 1 : 0,
				nzbInfo->GetManyDupeFiles() ? 1 : 0) != 0;
			if (useFilenameFromArticle)
			{
				fileInfo->SetFilename(articleDownloader->GetArticleFilename());
				fileInfo->MakeValidFilename();
			}
			fileInfo->SetFilenameConfirmed(true);
			if (g_Options->GetDupeCheck() &&
				nzbInfo->GetDupeMode() != dmForce &&
				!nzbInfo->GetManyDupeFiles() &&
				FileSystem::FileExists(BString<1024>("%s%c%s", nzbInfo->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename())))
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
		completeFileParts = nzbget_rs_qci_queue_coord_eval_file_completion_and_assembly(
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
			nzbget_rs_qci_queue_coord_apply_assembly_failure_counters(
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
			path = destDir + PATH_SEPARATOR + filename;
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
void QueueCoordinator::RejectBorrowedArticles(FileInfo* fileInfo, const std::vector<ArticleInfo*>& articles)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	std::set<int> donors;
	for (ArticleInfo* article : articles)
	{
		donors.insert(article->GetDupeDonorId());
		if (article->GetStatus() == ArticleInfo::aiFinished)
		{
			// (not recovered any more: own articles are counted right again, review item 4)
			CountAsFailed(fileInfo, article);
		}
	}
	BString<1024> names;
	for (int donorId : donors)
	{
		nzbInfo->GetDupeBlockedDonors()->insert(donorId);
		names.AppendFmt("%s%i", names.Empty() ? "" : ", ", donorId);
	}
	nzbInfo->PrintMessage(Message::mkWarning,
		"%i article(s) of %s borrowed from duplicates don't match its par2 checksums: counted as "
		"failed, and duplicate(s) %s lend this download no more articles",
		(int)articles.size(), fileInfo->GetFilename(), *names);
}

void QueueCoordinator::DeleteDownloader(DownloadQueue* downloadQueue,
	ArticleDownloader* articleDownloader, bool fileCompleted)
{
	FileInfo* fileInfo = articleDownloader->GetFileInfo();
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	bool deleteFileObj = nzbget_rs_fuq_queue_coord_should_delete_file_obj(
		fileCompleted ? 1 : 0, fileInfo->GetDeleted() ? 1 : 0, fileInfo->GetActiveDownloads()) != 0;

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

void QueueCoordinator::DeleteFileInfo(DownloadQueue* downloadQueue, FileInfo* fileInfo, bool completed)
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
		nzbget_rs_cwp_queue_coord_eval_delete_file_diskstate_flags(
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
		bool sideFile = completed &&
			(fileStatus == CompletedFile::cfPartial || fileStatus == CompletedFile::cfFailure) &&
			SideFile(filename.c_str());
		if (sideFile)
		{
			// an .nfo or .sfv is worth no repair from a duplicate: par2 rarely covers
			// it, and repairing it held up a download whose data was whole (B53)
			nzbInfo->PrintMessage(Message::mkDetail, "Not repairing side file %s from duplicates", filename.c_str());
		}
		if (completed && !sideFile &&
			(fileStatus == CompletedFile::cfPartial || fileStatus == CompletedFile::cfFailure) &&
			DupeStreamRepair::BuildRepairJob(fileInfo, filename.c_str()))
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				fileStatus == CompletedFile::cfFailure ?
					"Queueing stream repair of %s (no article available) from duplicate collections" :
					"Queueing stream repair of %s from duplicate collections", filename.c_str());

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
			if (nzbget_rs_cwp_queue_coord_should_start_live_stream_repair(
				(int)g_Options->GetDupeArticleFallback(),
				(int)Options::dafLive,
				moreFilesQueued ? 1 : 0,
				nzbInfo->GetLiveRepairThread() ? 1 : 0) != 0)
			{
				StreamRepairController::StartLive(nzbInfo);
			}
		}

		uint32 fileCrc = nzbget_rs_qcf_queue_coord_completed_file_crc(
			(int)fileStatus, (int)CompletedFile::cfSuccess, fileInfo->GetCrc());
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

	bool renameRunningNotDeleting = nzbInfo->GetDirectRenameStatus() == NzbInfo::tsRunning &&
		!nzbInfo->GetDeleting();
	if (nzbget_rs_qcf_queue_coord_should_discard_direct_rename_on_complete(
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsRunning,
		nzbInfo->GetDeleting() ? 1 : 0,
		(renameRunningNotDeleting && nzbInfo->IsDownloadCompleted(true)) ? 1 : 0) != 0)
	{
		DiscardDirectRename(downloadQueue, nzbInfo);
	}

	std::unique_ptr<FileInfo> srcFileInfo = nzbInfo->GetFileList()->Remove(fileInfo);

	DownloadQueue::EAspectAction removedAction =
		(DownloadQueue::EAspectAction)nzbget_rs_qcf_queue_coord_select_file_removed_event(
			completed ? 1 : 0,
			fileDeleted ? 1 : 0,
			(int)DownloadQueue::eaFileCompleted,
			(int)DownloadQueue::eaFileDeleted);
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
void QueueCoordinator::DemoteMisalignedDupeNeighbors(FileInfo* fileInfo, ArticleInfo* articleInfo)
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
		if (nzbget_rs_cwp_queue_coord_is_neighbor_eligible_for_demotion(
				(int)neighbor->GetStatus(),
				(int)ArticleInfo::aiFinished,
				neighbor->GetDupeFallbackRound(),
				neighbor->GetSegmentSize()) == 0 ||
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
void QueueCoordinator::ValidateCompletedFileTiling(FileInfo* fileInfo)
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
void QueueCoordinator::CountAsFailed(FileInfo* fileInfo, ArticleInfo* articleInfo)
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
	nzbget_rs_qci_queue_coord_demote_article_counters(
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

void QueueCoordinator::DemoteFinishedArticle(FileInfo* fileInfo, ArticleInfo* articleInfo)
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
void QueueCoordinator::DiscardArticleSegment(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	Guard contentGuard = g_ArticleCache->GuardContent();
	int deleteResultFile = 0;
	int discardCachedSeg = 0;
	nzbget_rs_qcf_queue_coord_discard_segment_actions(
		articleInfo->GetDupeFallbackRound(),
		articleInfo->GetResultFilename() ? 1 : 0,
		articleInfo->GetSegmentContent() ? 1 : 0,
		fileInfo->GetFlushLocked() ? 1 : 0,
		&deleteResultFile,
		&discardCachedSeg);
	if (deleteResultFile)
	{
		FileSystem::DeleteFile(articleInfo->GetResultFilename());
	}
	if (!discardCachedSeg)
	{
		return;
	}
	articleInfo->DiscardSegment();
	fileInfo->SetCachedArticles(fileInfo->GetCachedArticles() - 1);
}

void QueueCoordinator::DiscardTempFiles(FileInfo* fileInfo)
{
	for (ArticleInfo* pa : fileInfo->GetArticles())
	{
		if (nzbget_rs_qci_queue_coord_should_delete_temp_article_file(
			pa->GetResultFilename() ? 1 : 0,
			g_Options->GetDirectWrite() ? 1 : 0,
			fileInfo->GetForceDirectWrite() ? 1 : 0,
			pa->GetDupeFallbackRound()) != 0)
		{
			FileSystem::DeleteFile(pa->GetResultFilename());
		}
	}

	const std::string& outputFilename = fileInfo->GetOutputFilename();
	if (nzbget_rs_qcf_queue_coord_should_delete_direct_write_output(
		g_Options->GetDirectWrite() ? 1 : 0,
		outputFilename.empty() ? 1 : 0,
		fileInfo->GetForceDirectWrite() ? 1 : 0) != 0)
	{
		FileSystem::DeleteFile(outputFilename.c_str());
		if (fileInfo->IsHardLinked())
		{
			FileSystem::DeleteFile(fileInfo->GetHardLinkPath().c_str());
		}
	}
}

void QueueCoordinator::SaveQueueIfChanged()
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

void QueueCoordinator::SaveAllPartialState()
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

void QueueCoordinator::SavePartialState(FileInfo* fileInfo)
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

void QueueCoordinator::LoadPartialState(FileInfo* fileInfo)
{
	if (fileInfo->GetPartialState() == FileInfo::psPartial)
	{
		g_DiskState->LoadFileState(fileInfo, g_ServerPool->GetServers(), false);
		if ((g_Options->GetDirectWrite() || fileInfo->GetForceDirectWrite()) && !fileInfo->GetOutputInitialized())
		{
			BString<1024> outTmpFilename("%s%c%i.out.tmp", fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetId());
			if (FileSystem::FileExists(outTmpFilename))
			{
				fileInfo->SetOutputFilename(outTmpFilename);
				fileInfo->SetOutputInitialized(true);
			}
			else if (fileInfo->GetForceDirectWrite())
			{
				BString<1024> outputFilename("%s%c%s", fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
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

		BString<1024> outputFilename("%s%c%s", fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
		fileInfo->SetOutputFilename(outputFilename);

		fileInfo->SetOutputInitialized(true);
		fileInfo->SetForceDirectWrite(true);
		fileInfo->SetFilenameConfirmed(true);
	}
}

void QueueCoordinator::SaveAllFileState()
{
	if (g_Options->GetServerMode() && m_downloadQueue.m_stateChanged)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		g_DiskState->SaveAllFileInfos(downloadQueue);
	}
}

void QueueCoordinator::CheckHealth(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	int health = fileInfo->GetNzbInfo()->CalcHealth();
	int critHealth = (g_Options->GetHealthCheck() == Options::hcNone ||
		fileInfo->GetNzbInfo()->GetHealthPaused() ||
		fileInfo->GetNzbInfo()->GetDeleteStatus() == NzbInfo::dsHealth)
		? 0
		: DownloadCriticalHealth(downloadQueue, fileInfo->GetNzbInfo());
	if (nzbget_rs_fuq_queue_coord_should_skip_health_check(
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
			fileInfo->GetNzbInfo()->CalcHealth() / 10.0, fileInfo->GetNzbInfo()->CalcCriticalHealth(true) / 10.0);
		fileInfo->GetNzbInfo()->SetHealthPaused(true);
		downloadQueue->EditEntry(fileInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupPause, nullptr);
	}
	else if (g_Options->GetHealthCheck() == Options::hcDelete ||
		g_Options->GetHealthCheck() == Options::hcPark)
	{
		fileInfo->GetNzbInfo()->PrintMessage(Message::mkWarning,
			"Cancelling download and deleting %s due to health %.1f%% below critical %.1f%%",
			fileInfo->GetNzbInfo()->GetName(), fileInfo->GetNzbInfo()->CalcHealth() / 10.0,
			fileInfo->GetNzbInfo()->CalcCriticalHealth(true) / 10.0);
		fileInfo->GetNzbInfo()->SetDeleteStatus(NzbInfo::dsHealth);
		downloadQueue->EditEntry(fileInfo->GetNzbInfo()->GetId(),
			g_Options->GetHealthCheck() == Options::hcPark ? DownloadQueue::eaGroupParkDelete : DownloadQueue::eaGroupDelete,
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
int QueueCoordinator::DownloadCriticalHealth(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	int critical = nzbInfo->CalcCriticalHealth(true);
	int criticalExact = nzbInfo->CalcCriticalHealth(false);
	bool parless = (g_Options->GetHealthCheck() == Options::hcDupe && criticalExact == 1000)
		? CertainlyParless(nzbInfo) : false;
	bool sizeTwin = parless ? HasSizeTwin(downloadQueue, nzbInfo) : false;
	return nzbget_rs_fuq_queue_coord_resolve_download_critical_health(
		critical, criticalExact, (int)g_Options->GetHealthCheck(), (int)Options::hcDupe,
		parless ? 1 : 0, sizeTwin ? 1 : 0);
}

bool QueueCoordinator::CertainlyParless(NzbInfo* nzbInfo)
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

bool QueueCoordinator::HasSizeTwin(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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
	// ("sizeNear": "near" and "far" are macros in the Windows headers)
	auto sizeNear = [](int64 size1, int64 size2) { return nzbget_rs_qhp_size_near(size1, size2) != 0; };
	auto twin = [nzbInfo, largest, &sizeNear](NzbInfo* other)
		{
			if (other == nzbInfo || !DupeCoordinator::SameNameOrKey(other->GetName(), other->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
			{
				return false;
			}
			if (sizeNear(nzbInfo->GetSize(), other->GetSize()) ||
				(other->GetParSize() > 0 && sizeNear(nzbInfo->GetSize(), other->GetSize() - other->GetParSize())))
			{
				return true;
			}
			for (FileInfo* fileInfo : other->GetFileList())
			{
				if (sizeNear(largest, fileInfo->GetSize()))
				{
					return true;
				}
			}
			return false;
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
bool QueueCoordinator::HasActiveDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) const
{
	const char* dupeKey = nzbInfo->GetDupeKey();
	if (Util::EmptyStr(dupeKey))
	{
		return false;
	}

	// An active post-processing/repair job (or extra-priority par fetch) already owns
	// the active slot and must not be blocked from downloading its own repair volumes.
	PostInfo* selfPi = nzbInfo->GetPostInfo();
	if (nzbget_rs_qcf_queue_coord_has_active_dupe_self_exempt(
		nzbInfo->HasExtraPriority() ? 1 : 0,
		selfPi != nullptr ? 1 : 0,
		(selfPi && selfPi->GetWorking()) ? 1 : 0,
		selfPi ? (int)selfPi->GetStage() : 0,
		(int)PostInfo::ptQueued) != 0)
	{
		return false;
	}

	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued != nzbInfo &&
			!Util::EmptyStr(queued->GetDupeKey()) &&
			!strcasecmp(queued->GetDupeKey(), dupeKey))
		{
			PostInfo* qpi = queued->GetPostInfo();
			if (nzbget_rs_fuq_queue_coord_is_queued_dupe_active(
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
bool QueueCoordinator::DownloadHopeless(NzbInfo* nzbInfo)
{
	return nzbget_rs_qhp_download_hopeless(
		nzbInfo->GetTotalArticles(),
		nzbInfo->GetCurrentSuccessArticles(),
		nzbInfo->GetCurrentFailedArticles()
	) != 0;
}

void QueueCoordinator::CheckDupeFailover(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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
	if (nzbget_rs_qci_queue_coord_should_defer_dupe_failover(
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
	bool doomed = DownloadCriticalHealth(downloadQueue, nzbInfo) > nzbInfo->CalcCriticalHealth(true);
	if (backup && DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(),
		doomed ? 0 : nzbInfo->CalcHealth(), backup->GetNzbInfo()->GetDupeScore()))
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Failing over %s to duplicate %s: health %.1f%% below critical %.1f%%, "
			"%i of %i missing article(s) recovered from duplicates",
			nzbInfo->GetName(), backup->GetNzbInfo()->GetName(),
			nzbInfo->CalcHealth() / 10.0, DownloadCriticalHealth(downloadQueue, nzbInfo) / 10.0,
			recovered, attempted);
	}
	else if (DownloadHopeless(nzbInfo))
	{
		int tried = nzbInfo->GetCurrentSuccessArticles() + nzbInfo->GetCurrentFailedArticles();
		nzbInfo->PrintMessage(Message::mkWarning,
			"Parking %s: health %.1f%% below critical %.1f%%, %i of %i missing article(s) "
			"recovered from duplicates, %i of %i tried article(s) exist and no better duplicate in history (%s)",
			nzbInfo->GetName(), nzbInfo->CalcHealth() / 10.0, DownloadCriticalHealth(downloadQueue, nzbInfo) / 10.0,
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
void QueueCoordinator::CheckDeadDownload(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetHealthCheck() != Options::hcDupe || !g_Options->GetDupeCheck() ||
		!DupeCoordinator::FailsOver(nzbInfo) || nzbInfo->GetKind() != NzbInfo::nkNzb ||
		nzbInfo->GetDeleting() || nzbInfo->GetParking() || nzbInfo->GetDeleteStatus() != NzbInfo::dsNone)
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
		if (nzbget_rs_qci_queue_coord_should_park_dead_without_backup(
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
				nzbInfo->GetName(), own, tried, projected / 10.0, farBelow ? "under half the" : "below",
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
		int projBelowCrit = 0;
		bool warranted = (backupAlive >= projected + BackupLead && backupAlive >= critical) ?
			DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(), projected,
				backup->GetNzbInfo()->GetDupeScore()) : false;
		if (!nzbget_rs_qci_queue_coord_is_partly_alive_failover_ready(
			backupAlive, projected, BackupLead, critical, warranted ? 1 : 0, &projBelowCrit))
		{
			return;
		}
		nzbInfo->PrintMessage(Message::mkWarning,
			"Failing over %s to duplicate %s: %i of %i tried article(s) of its own arrived, projected health "
			"%.1f%% %s critical %.1f%%; the duplicate is %.0f%% alive",
			nzbInfo->GetName(), backup->GetNzbInfo()->GetName(), own, tried, projected / 10.0,
			projBelowCrit ? "below" : "too close to", critical / 10.0, backupAlive / 10.0);
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
bool QueueCoordinator::CheckParDamage(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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
	if (nzbget_rs_qci_queue_coord_should_wait_for_borrowing_on_par_damage(
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
		if (queued != nzbInfo && queued->GetPausedSize() < queued->GetRemainingSize() &&
			DupeCoordinator::SameNameOrKey(queued->GetName(), queued->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			return false;
		}
	}

	HistoryInfo* backup = g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo,
		nzbInfo->GetName(), nzbInfo->GetDupeKey());
	BString<1024> damage("%i par2 block(s) of %.1f MB damaged%s, its par2-files hold %i recovery block(s)",
		certain ? verdict.damagedBlocks : verdict.projectedBlocks, verdict.blockSize / 1024.0 / 1024.0,
		certain ? "" : " at the rate so far", verdict.recoveryBlocks);
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
	bool warranted = (certain || backupAlive >= 999) ?
		DupeCoordinator::DupeFailoverWarranted(nzbInfo->GetDupeScore(), 0, backup->GetNzbInfo()->GetDupeScore()) : false;
	if (!nzbget_rs_qcf_queue_coord_par_damage_failover_allowed(
		certain ? 1 : 0, backupAlive, warranted ? 1 : 0))
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
bool QueueCoordinator::SideFile(const char* filename)
{
	return nzbget_rs_qhp_is_side_file(filename, g_Options->GetExtCleanupDisk()) != 0;
}

int QueueCoordinator::FilesTried(NzbInfo* nzbInfo)
{
	int files = (int)nzbInfo->GetCompletedFiles()->size();
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		files += nzbget_rs_cwp_queue_coord_is_file_tried(
			fileInfo->GetSuccessArticles(), fileInfo->GetFailedArticles());
	}
	return files;
}

void QueueCoordinator::StartDeadPickProbe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	int markProbed = 0;
	int continueProbe = nzbget_rs_qcf_queue_coord_start_dead_pick_probe_eligible(
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0,
		g_Options->GetDupeCheck() ? 1 : 0,
		nzbInfo->GetDeadPickProbed() ? 1 : 0,
		DupeCoordinator::FailsOver(nzbInfo) ? 1 : 0,
		nzbInfo->GetKind() == NzbInfo::nkNzb ? 1 : 0,
		nzbInfo->GetDeleting() ? 1 : 0,
		nzbInfo->GetParking() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		nzbInfo->GetCurrentSuccessArticles(),
		&markProbed);
	if (markProbed)
	{
		nzbInfo->SetDeadPickProbed(true);
	}
	if (!continueProbe)
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
			if (nzbget_rs_cwp_queue_coord_is_dead_pick_probe_file_eligible(
				fileInfo->GetDeleted() ? 1 : 0,
				fileInfo->GetPaused() ? 1 : 0,
				fileInfo->GetTotalArticles(),
				pass,
				fileInfo->GetParFile() ? 1 : 0) != 0)
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

void QueueCoordinator::FailOverDeadPick(DownloadQueue* downloadQueue, int nzbId, int samples,
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
	if (nzbget_rs_fuq_queue_coord_should_abort_dead_pick_failover(
		nzbInfo->GetCurrentSuccessArticles(), nzbInfo->GetDupeRecoveredArticles(),
		recoveredAtStart, DupeProbe::MinAliveSamples) != 0)
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

void QueueCoordinator::LogDebugInfo()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	info("   ---------- Queue");
	int64 remaining, remainingForced;
	downloadQueue->CalcRemainingSize(&remaining, &remainingForced);
	info("     Remaining: %.1f MB, Forced: %.1f MB", remaining / 1024.0 / 1024.0, remainingForced / 1024.0 / 1024.0);
	info("     Download: %s, Post-process: %s, Scan: %s",
		 (g_WorkState->GetPauseDownload() ? "paused" : g_WorkState->GetTempPauseDownload() ? "temp-paused" : "active"),
		 (g_WorkState->GetPausePostProcess() ? "paused" : "active"),
		 (g_WorkState->GetPauseScan() ? "paused" : "active"));

	info("   ---------- QueueCoordinator");
	info("    Active Downloads: %i, Limit: %i", (int)m_activeDownloads.size(), m_downloadsLimit);
	for (ArticleDownloader* articleDownloader : m_activeDownloads)
	{
		articleDownloader->LogDebugInfo();
	}
}

void QueueCoordinator::ResetHangingDownloads()
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
bool QueueCoordinator::DeleteQueueEntry(DownloadQueue* downloadQueue, FileInfo* fileInfo)
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

bool QueueCoordinator::SetQueueEntryCategory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* category)
{
	if (nzbInfo->GetPostInfo())
	{
		error("Could not change category for %s. File in post-process-stage", nzbInfo->GetName());
		return false;
	}

	BString<1024> oldDestDir = nzbInfo->GetDestDir();

	nzbInfo->SetCategory(category);
	nzbInfo->BuildDestDirName();

	bool dirUnchanged = !strcmp(nzbInfo->GetDestDir(), oldDestDir);
	bool ok = dirUnchanged || ArticleWriter::MoveCompletedFiles(nzbInfo, oldDestDir);

	return ok;
}

bool QueueCoordinator::SetQueueEntryName(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* name)
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

	bool dirUnchanged = !strcmp(nzbInfo->GetDestDir(), oldDestDir);
	bool ok = dirUnchanged || ArticleWriter::MoveCompletedFiles(nzbInfo, oldDestDir);

	return ok;
}

static NzbgetRsNzbMergeSplitCounters ReadNzbMergeSplitCounters(NzbInfo* nzbInfo)
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

static void WriteNzbMergeSplitCounters(NzbInfo* nzbInfo, const NzbgetRsNzbMergeSplitCounters& c)
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

bool QueueCoordinator::MergeQueueEntries(DownloadQueue* downloadQueue, NzbInfo* destNzbInfo, NzbInfo* srcNzbInfo)
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
	if (strcmp(oldDestDir, srcNzbInfo->GetDestDir()))
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
	nzbget_rs_qci_queue_coord_merge_counters(&destCounters, &srcCounters);
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
	CString queuedFilename;
	queuedFilename.Format("%s|%s", destNzbInfo->GetQueuedFilename(), srcNzbInfo->GetQueuedFilename());
	destNzbInfo->SetQueuedFilename(queuedFilename);
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
bool QueueCoordinator::SplitQueueEntries(DownloadQueue* downloadQueue, RawFileList* fileList, const char* name, NzbInfo** newNzbInfo)
{
	NzbInfo* srcNzbInfo = nullptr;

	for (FileInfo* fileInfo : fileList)
	{
		int splitErr = nzbget_rs_qci_queue_coord_validate_split_file_state(
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

		nzbget_rs_qci_queue_coord_split_transfer_file(&srcCounters, &dstCounters, &fc);
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

void QueueCoordinator::DirectRenameCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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

void QueueCoordinator::DiscardDirectRename(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	int64 discardedSize = 0;
	int discardedCount = 0;

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		int discardInfos = 0;
		bool discardPar = nzbget_rs_qci_queue_coord_eval_discard_direct_rename_file(
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

void QueueCoordinator::DiscardDownloadedArticles(NzbInfo* nzbInfo, FileInfo* fileInfo)
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
	nzbget_rs_cwp_queue_coord_discard_downloaded_articles_counters(
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
