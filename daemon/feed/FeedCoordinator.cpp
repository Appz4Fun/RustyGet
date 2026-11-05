/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"

#include "FeedCoordinator.h"
#include "Options.h"
#include "WorkState.h"
#include "WebDownloader.h"
#include "Util.h"
#include "FileSystem.h"
#include "FeedFile.h"
#include "FeedFilter.h"
#include "FeedScript.h"
#include "DiskState.h"
#include "DupeCoordinator.h"
#include "UrlCoordinator.h"
#include "nzbget_rs.h"

std::unique_ptr<NzbInfo> NzbInfoCreator::Create(const FeedInfo& feedInfo, const FeedItemInfo& feedItemInfo) const
{
	std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
	nzbInfo->SetKind(NzbInfo::nkUrl);
	nzbInfo->SetFeedId(feedInfo.GetId());
	nzbInfo->SetUrl(feedItemInfo.GetUrl());

	// add .nzb-extension if not present
	BString<1024> nzbName = feedItemInfo.GetFilename();
	NzbgetRsBuf normBuf = nzbget_rs_df_normalize_feed_nzb_name(*nzbName);
	if (normBuf.len > 0 && normBuf.data)
	{
		BString<1024> nzbName2("%s", normBuf.data);
		nzbInfo->SetFilename(FileSystem::MakeValidFilename(nzbName2));
	}
	nzbget_rs_free(normBuf);

	ApplyCategory(*nzbInfo, feedInfo, feedItemInfo);

	nzbInfo->SetPriority(feedItemInfo.GetPriority());
	nzbInfo->SetAddUrlPaused(feedItemInfo.GetPauseNzb());
	nzbInfo->SetDupeKey(feedItemInfo.GetDupeKey());
	nzbInfo->SetDupeScore(feedItemInfo.GetDupeScore());
	nzbInfo->SetDupeMode(feedItemInfo.GetDupeMode());
	nzbInfo->SetSize(feedItemInfo.GetSize());
	nzbInfo->SetMinTime(feedItemInfo.GetTime());
	nzbInfo->SetMaxTime(feedItemInfo.GetTime());

	return nzbInfo;
}

void NzbInfoCreator::ApplyCategory(NzbInfo& nzbInfo, const FeedInfo& feedInfo, const FeedItemInfo& feedItemInfo) const
{
	// Priority 1: AddCategory from filter rules
	const std::string& addCategory = feedItemInfo.GetAddCategory();
	if (!addCategory.empty())
	{
		nzbInfo.SetCategory(addCategory.c_str());
		return;
	}

	// Priority 2: Feed.Category / CategorySource fallback
	nzbInfo.SetCategory(FeedCoordinator::ResolveCategory(feedInfo, feedItemInfo).c_str());
}

std::unique_ptr<RegEx>& FeedCoordinator::FilterHelper::GetRegEx(int id)
{
	m_regExes.resize(id);
	return m_regExes[id - 1];
}

void FeedCoordinator::FilterHelper::CalcDupeStatus(const char* title, const char* dupeKey, char* statusBuf, int bufLen)
{
	DupeCoordinator::EDupeStatus dupeStatus = g_DupeCoordinator->GetDupeStatus(DownloadQueue::Guard(), title, dupeKey);

	NzbgetRsBuf buf = nzbget_rs_df_format_dupe_status_mask((int)dupeStatus);
	strncpy(statusBuf, buf.data ? buf.data : "", bufLen);
	nzbget_rs_free(buf);
}

FeedCoordinator::FeedCoordinator()
{
	debug("Creating FeedCoordinator");

	m_downloadQueueObserver.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);

	m_workStateObserver.m_owner = this;
	g_WorkState->Attach(&m_workStateObserver);
}

FeedCoordinator::~FeedCoordinator()
{
	debug("Destroying FeedCoordinator");

	for (FeedDownloader* feedDownloader : m_activeDownloads)
	{
		delete feedDownloader;
	}
	m_activeDownloads.clear();
}

void FeedCoordinator::Run()
{
	debug("Entering FeedCoordinator-loop");

	while (!DownloadQueue::IsLoaded())
	{
		Util::Sleep(20);
	}

	if (g_Options->GetServerMode())
	{
		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		g_DiskState->LoadFeeds(&m_feeds, &m_feedHistory);
	}

	time_t lastCleanup = 0;
	while (!IsStopped())
	{
		// this code should not be called too often, once per second is OK
		if (nzbget_rs_ucf_feed_coord_can_check_feeds(
			g_WorkState->GetPauseDownload() ? 1 : 0,
			m_force ? 1 : 0,
			g_Options->GetUrlForce() ? 1 : 0) != 0)
		{
			std::lock_guard<std::mutex> guard(m_downloadsMutex);

			time_t current = Util::CurrentTime();
			if ((int)m_activeDownloads.size() < g_Options->GetUrlConnections())
			{
				m_force = false;
				// check feed list and update feeds
				for (FeedInfo* feedInfo : &m_feeds)
				{
					int setForce = 0;
					if (nzbget_rs_df_should_start_feed(
						feedInfo->GetInterval(), (int64_t)feedInfo->GetNextUpdate(), (int64_t)current,
						feedInfo->GetFetch() ? 1 : 0, (int)feedInfo->GetStatus(), (int)FeedInfo::fsRunning,
						&setForce))
					{
						StartFeedDownload(feedInfo, feedInfo->GetFetch());
					}
					else if (setForce)
					{
						m_force = true;
					}
				}
			}
		}

		CheckSaveFeeds();
		ResetHangingDownloads();

		if (nzbget_rs_fuq_feed_coord_should_cleanup_history(
			(int64_t)Util::CurrentTime(), (int64_t)lastCleanup) != 0)
		{
			// clean up feed history once a minute
			CleanupHistory();
			CleanupCache();
			CheckSaveFeeds();
			lastCleanup = Util::CurrentTime();
		}

		std::unique_lock<std::mutex> lk(m_downloadsMutex);
		if (m_force)
		{
			// don't sleep too long if there active feeds scheduled for redownload
			m_waitCond.wait_for(lk, std::chrono::milliseconds(1000), [&]{ return IsStopped(); });
		}
		else
		{
			// no active jobs, we can sleep longer:
			//  - if option "UrlForce" is active or if the feed list is empty we need to wake up
			//    only when a new feed preview is requested. We could wait indefinitely for that
			//    but we need to do some job every now and then and therefore we sleep only 60 seconds.
			//  - if option "UrlForce" is disabled we need also to wake up when state "DownloadPaused"
			//    is changed. We detect this via notification from 'WorkState'. However such
			//    notifications are not 100% reliable due to possible race conditions. Therefore
			//    we sleep for max. 5 seconds.
			int waitInterval = nzbget_rs_fuq_feed_coord_calc_wait_interval(
				g_Options->GetUrlForce() ? 1 : 0, m_feeds.empty() ? 1 : 0);
			m_waitCond.wait_for(lk, std::chrono::milliseconds(waitInterval), [&]{ return m_force || IsStopped(); });
		}
	}

	// waiting for downloads
	debug("FeedCoordinator: waiting for Downloads to complete");
	bool completed = false;
	while (!completed)
	{
		{
			std::lock_guard<std::mutex> guard(m_downloadsMutex);
			completed = m_activeDownloads.size() == 0;
		}
		CheckSaveFeeds();
		Util::Sleep(100);
		ResetHangingDownloads();
	}
	debug("FeedCoordinator: Downloads are completed");

	debug("Exiting FeedCoordinator-loop");
}

void FeedCoordinator::Stop()
{
	Thread::Stop();

	debug("Stopping UrlDownloads");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	for (FeedDownloader* feedDownloader : m_activeDownloads)
	{
		feedDownloader->Stop();
	}
	debug("UrlDownloads are notified");

	// Resume Run() to exit it
	m_waitCond.notify_all();
}

void FeedCoordinator::WorkStateUpdate(Subject*, void*)
{
	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	m_force = true;
	m_waitCond.notify_all();
}

void FeedCoordinator::ResetHangingDownloads()
{
	if (g_Options->GetUrlTimeout() == 0)
	{
		return;
	}

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	time_t tm = Util::CurrentTime();

	for (FeedDownloader* feedDownloader: m_activeDownloads)
	{
		if (nzbget_rs_fuq_feed_coord_is_hanging_download(
			(int64_t)tm, (int64_t)feedDownloader->GetLastUpdateTime(),
			g_Options->GetUrlTimeout(), (int)feedDownloader->GetStatus(),
			(int)FeedDownloader::adRunning) != 0)
		{
			error("Cancelling hanging feed download %s", feedDownloader->GetInfoName());
			feedDownloader->Stop();
		}
	}
}

void FeedCoordinator::LogDebugInfo()
{
	info("   ---------- FeedCoordinator");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	info("    Active Downloads: %i", (int)m_activeDownloads.size());
	for (FeedDownloader* feedDownloader : m_activeDownloads)
	{
		feedDownloader->LogDebugInfo();
	}
}

void FeedCoordinator::StartFeedDownload(FeedInfo* feedInfo, bool force)
{
	debug("Starting new FeedDownloader for %s", feedInfo->GetName());

	FeedDownloader* feedDownloader = new FeedDownloader();
	feedDownloader->SetAutoDestroy(true);
	feedDownloader->Attach(this);
	feedDownloader->SetFeedInfo(feedInfo);
	feedDownloader->SetUrl(feedInfo->GetUrl());
	feedDownloader->SetInfoName(feedInfo->GetName());
	feedDownloader->SetForce(force || g_Options->GetUrlForce());
#ifndef DISABLE_TLS
	feedDownloader->SetCertVerifLevel(feedInfo->GetCertVerificationLevel());
#endif

	// a preview of a configured feed must not share the file of that feed's own fetch
	static std::atomic<int> previewNum{0};
	BString<1024> outFilename;
	if (nzbget_rs_ucf_feed_coord_use_feed_id_temp_file(
		feedInfo->GetId(), feedInfo->GetPreview() ? 1 : 0) != 0)
	{
		outFilename.Format("%s%cfeed-%i.tmp", g_Options->GetTempDir(), PATH_SEPARATOR, feedInfo->GetId());
	}
	else
	{
		outFilename.Format("%s%cfeed-%i-%i-%i.tmp", g_Options->GetTempDir(), PATH_SEPARATOR,
			(int)Util::CurrentTime(), rand(), ++previewNum);
	}
	feedDownloader->SetOutputFilename(outFilename);

	feedInfo->SetStatus(FeedInfo::fsRunning);
	feedInfo->SetForce(force);
	feedInfo->SetFetch(false);

	m_activeDownloads.push_back(feedDownloader);
	feedDownloader->Start();
}

void FeedCoordinator::Update(Subject* caller, void* aspect)
{
	debug("Notification from FeedDownloader received");

	FeedDownloader* feedDownloader = (FeedDownloader*) caller;
	if (nzbget_rs_uqs_feed_coord_is_completed_status(
		(int)feedDownloader->GetStatus(),
		(int)WebDownloader::adFinished,
		(int)WebDownloader::adFailed,
		(int)WebDownloader::adRetry) != 0)
	{
		FeedCompleted(feedDownloader);
	}
}

void FeedCoordinator::FeedCompleted(FeedDownloader* feedDownloader)
{
	debug("Feed downloaded");

	FeedInfo* feedInfo = feedDownloader->GetFeedInfo();
	bool statusOK = feedDownloader->GetStatus() == WebDownloader::adFinished;
	if (statusOK)
	{
		feedInfo->SetOutputFilename(feedDownloader->GetOutputFilename());
	}

	// remove downloader from downloader list
	{
		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		m_activeDownloads.erase(std::find(m_activeDownloads.begin(), m_activeDownloads.end(), feedDownloader));
	}

	SchedulerNextUpdate(feedInfo, statusOK);

	if (statusOK)
	{
		if (!feedInfo->GetPreview())
		{
			bool scriptSuccess = true;
			FeedScriptController::ExecuteScripts(
				!Util::EmptyStr(feedInfo->GetExtensions()) ? feedInfo->GetExtensions(): g_Options->GetExtensions(),
				feedInfo->GetOutputFilename(), feedInfo->GetId(), &scriptSuccess);
			if (!scriptSuccess)
			{
				feedInfo->SetStatus(FeedInfo::fsFailed);
				return;
			}

			std::unique_ptr<FeedFile> feedFile = parseFeed(feedInfo);

			std::vector<std::unique_ptr<NzbInfo>> addedNzbs;

			{
				std::lock_guard<std::mutex> guard(m_downloadsMutex);
				if (feedFile)
				{
					std::unique_ptr<FeedItemList> feedItems = feedFile->DetachFeedItems();
					addedNzbs = ProcessFeed(feedInfo, feedItems.get());
					feedFile.reset();
				}
				feedInfo->SetLastUpdate(Util::CurrentTime());
				feedInfo->SetForce(false);
				m_save = true;
			}

			for (std::unique_ptr<NzbInfo>& nzbInfo : addedNzbs)
			{
				g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), false);
			}
		}
		feedInfo->SetStatus(FeedInfo::fsFinished);
	}
	else
	{
		feedInfo->SetStatus(FeedInfo::fsFailed);
	}
}

void FeedCoordinator::SchedulerNextUpdate(FeedInfo* feedInfo, bool success)
{
	time_t current = Util::CurrentTime();
	int interval;

	int newLastInterval = 0;
	interval = nzbget_rs_df_calc_feed_next_interval(
		success ? 1 : 0, feedInfo->GetInterval(), feedInfo->GetLastInterval(), &newLastInterval);
	feedInfo->SetLastInterval(newLastInterval);

	detail("Scheduling update for feed %s in %i minute(s)", feedInfo->GetName(), interval / 60);
	feedInfo->SetNextUpdate(current + interval);
}

std::string FeedCoordinator::ResolveCategory(const FeedInfo& feedInfo, const FeedItemInfo& feedItemInfo)
{
	return nzbget_rs_fuq_feed_coord_resolve_category(
		feedInfo.GetCategory(),
		(int)feedInfo.GetCategorySource(),
		(int)FeedInfo::CategorySource::NZBFile,
		(int)FeedInfo::CategorySource::Auto,
		feedItemInfo.GetCategory());
}

void FeedCoordinator::FilterFeed(FeedInfo* feedInfo, FeedItemList* feedItems)
{
	debug("Filtering feed %s", feedInfo->GetName());

	FilterHelper filterHelper;
	std::unique_ptr<FeedFilter> feedFilter;

	if (!Util::EmptyStr(feedInfo->GetFilter()))
	{
		feedFilter = std::make_unique<FeedFilter>(feedInfo->GetFilter());
	}

	for (FeedItemInfo& feedItemInfo : feedItems)
	{
		feedItemInfo.SetMatchStatus(FeedItemInfo::msAccepted);
		feedItemInfo.SetMatchRule(0);
		feedItemInfo.SetPauseNzb(feedInfo->GetPauseNzb());
		feedItemInfo.SetPriority(feedInfo->GetPriority());
		feedItemInfo.SetAddCategory(FeedCoordinator::ResolveCategory(*feedInfo, feedItemInfo).c_str());
		feedItemInfo.SetDupeScore(0);
		feedItemInfo.SetDupeMode(dmScore);
		feedItemInfo.SetFeedFilterHelper(&filterHelper);
		feedItemInfo.BuildDupeKey(nullptr, nullptr, nullptr, nullptr);
		if (feedFilter)
		{
			feedFilter->Match(feedItemInfo);
		}
	}
}

std::vector<std::unique_ptr<NzbInfo>> FeedCoordinator::ProcessFeed(FeedInfo* feedInfo, FeedItemList* feedItems)
{
	debug("Process feed %s", feedInfo->GetName());

	FilterFeed(feedInfo, feedItems);

	std::vector<std::unique_ptr<NzbInfo>> addedNzbs;
	bool firstFetch = feedInfo->GetLastUpdate() == 0;
	int added = 0;

	for (FeedItemInfo& feedItemInfo : feedItems)
	{
		if (feedItemInfo.GetMatchStatus() == FeedItemInfo::msAccepted)
		{
			FeedHistoryInfo* feedHistoryInfo = m_feedHistory.Find(feedItemInfo.GetUrl());
			int shouldAdd = 0;
			FeedHistoryInfo::EStatus status = (FeedHistoryInfo::EStatus)nzbget_rs_fuq_feed_coord_classify_item_history(
				firstFetch ? 1 : 0, feedInfo->GetBacklog() ? 1 : 0, feedHistoryInfo != nullptr ? 1 : 0,
				(int)FeedHistoryInfo::hsUnknown, (int)FeedHistoryInfo::hsBacklog,
				(int)FeedHistoryInfo::hsFetched, &shouldAdd);
			if (shouldAdd != 0)
			{
				debug("Download %s from %s", feedItemInfo.GetUrl(), feedInfo->GetName());

				addedNzbs.push_back(m_nzbInfoCreator.Create(*feedInfo, feedItemInfo));
				added++;
			}

			if (feedHistoryInfo)
			{
				feedHistoryInfo->SetLastSeen(Util::CurrentTime());
			}
			else
			{
				m_feedHistory.emplace_back(feedItemInfo.GetUrl(), status, Util::CurrentTime());
			}
		}
	}

	if (added)
	{
		info("%s has %i new item(s)", feedInfo->GetName(), added);
	}
	else
	{
		detail("%s has no new items", feedInfo->GetName());
	}

	return addedNzbs;
}

std::shared_ptr<FeedItemList> FeedCoordinator::ViewFeed(int id)
{
	if (!nzbget_rs_uqs_feed_coord_is_valid_feed_id(id, static_cast<int>(m_feeds.size())))
	{
		return nullptr;
	}

	std::unique_ptr<FeedInfo>& feedInfo = m_feeds[static_cast<size_t>(id - 1)];

	return PreviewFeed(feedInfo->GetId(), feedInfo->GetName(), feedInfo->GetUrl(), feedInfo->GetFilter(),
		feedInfo->GetBacklog(), feedInfo->GetPauseNzb(), feedInfo->GetCategory(), feedInfo->GetCategorySource(),
		feedInfo->GetPriority(), feedInfo->GetInterval(), feedInfo->GetExtensions(), 0, nullptr);
}

std::shared_ptr<FeedItemList> FeedCoordinator::PreviewFeed(int id,
	const char* name, const char* url, const char* filter, bool backlog, bool pauseNzb,
	const char* category, FeedInfo::CategorySource categorySource, int priority, int interval, const char* feedScript,
	int cacheTimeSec, const char* cacheId)
{
	debug("Preview feed %s", name);

	unsigned int certVerifLevel = Options::ECertVerifLevel::cvStrict;
	if (nzbget_rs_uqs_feed_coord_is_valid_feed_id(id, static_cast<int>(m_feeds.size())) != 0)
	{
		certVerifLevel = m_feeds[static_cast<size_t>(id - 1)]->GetCertVerificationLevel();
	}

	std::unique_ptr<FeedInfo> feedInfo = std::make_unique<FeedInfo>(id, name, url, backlog, interval,
		filter, pauseNzb, category, categorySource, priority, feedScript, certVerifLevel);
	feedInfo->SetPreview(true);

	std::shared_ptr<FeedItemList> feedItems;
	bool hasCache = false;
	if (nzbget_rs_ucf_feed_coord_can_use_cache(cacheTimeSec, cacheId) != 0)
	{
		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		for (FeedCacheItem& feedCacheItem : m_feedCache)
		{
			if (!strcmp(feedCacheItem.GetCacheId(), cacheId))
			{
				feedCacheItem.SetLastUsage(Util::CurrentTime());
				feedItems = feedCacheItem.GetFeedItems();
				hasCache = true;
				break;
			}
		}
	}

	if (!hasCache)
	{
		bool firstFetch = true;

		{
			std::lock_guard<std::mutex> guard(m_downloadsMutex);

			for (FeedInfo* feedInfo2 : &m_feeds)
			{
				if (nzbget_rs_ucf_feed_coord_matches_existing_feed(
					feedInfo2->GetUrl(), feedInfo->GetUrl(),
					feedInfo2->GetFilter(), feedInfo->GetFilter(),
					(int64_t)feedInfo2->GetLastUpdate()) != 0)
				{
					firstFetch = false;
					break;
				}
			}

			StartFeedDownload(feedInfo.get(), true);

			m_force = true;
			m_waitCond.notify_all();
		}

		// wait until the download in a separate thread completes
		while (feedInfo->GetStatus() == FeedInfo::fsRunning)
		{
			Util::Sleep(100);
		}

		// now can process the feed

		if (feedInfo->GetStatus() != FeedInfo::fsFinished)
		{
			return nullptr;
		}

		FeedScriptController::ExecuteScripts(
			!Util::EmptyStr(feedInfo->GetExtensions()) ? feedInfo->GetExtensions(): g_Options->GetExtensions(),
			feedInfo->GetOutputFilename(), feedInfo->GetId(), nullptr);

		std::unique_ptr<FeedFile> feedFile = parseFeed(feedInfo.get());
		if (!feedFile)
		{
			return nullptr;
		}

		feedItems = feedFile->DetachFeedItems();
		feedFile.reset();

		// the history grows while feeds are processed
		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		for (FeedItemInfo& feedItemInfo : feedItems.get())
		{
			FeedHistoryInfo* feedHistoryInfo = m_feedHistory.Find(feedItemInfo.GetUrl());
			feedItemInfo.SetStatus((FeedItemInfo::EStatus)nzbget_rs_ucf_feed_coord_resolve_preview_item_status(
				feedHistoryInfo != nullptr ? 1 : 0,
				feedHistoryInfo ? (int)feedHistoryInfo->GetStatus() : 0,
				firstFetch ? 1 : 0,
				feedInfo->GetBacklog() ? 1 : 0,
				(int)FeedItemInfo::isBacklog,
				(int)FeedItemInfo::isNew));
		}
	}

	FilterFeed(feedInfo.get(), feedItems.get());
	feedInfo.reset();

	if (!hasCache && nzbget_rs_ucf_feed_coord_can_use_cache(cacheTimeSec, cacheId) != 0)
	{
		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		m_feedCache.emplace_back(url, cacheTimeSec, cacheId, Util::CurrentTime(), feedItems);
	}

	return feedItems;
}

void FeedCoordinator::FetchFeed(int id)
{
	debug("FetchFeeds");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	for (FeedInfo* feedInfo : &m_feeds)
	{
		if (nzbget_rs_uqs_feed_coord_matches_fetch_feed_id(feedInfo->GetId(), id) != 0)
		{
			feedInfo->SetFetch(true);
			m_force = true;
		}
	}

	m_waitCond.notify_all();
}

std::unique_ptr<FeedFile> FeedCoordinator::parseFeed(FeedInfo* feedInfo)
{
	std::unique_ptr<FeedFile> feedFile = std::make_unique<FeedFile>(feedInfo->GetOutputFilename(), feedInfo->GetName());
	if (feedFile->Parse())
	{
		FileSystem::DeleteFile(feedInfo->GetOutputFilename());
	}
	else
	{
		error("Feed file %s kept for troubleshooting (will be deleted on next successful feed fetch)", feedInfo->GetOutputFilename());
		feedFile.reset();
	}
	return feedFile;
}

void FeedCoordinator::DownloadQueueUpdate(Subject* caller, void* aspect)
{
	debug("Notification from URL-Coordinator received");

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaUrlCompleted)
	{
		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		FeedHistoryInfo* feedHistoryInfo = m_feedHistory.Find(queueAspect->nzbInfo->GetUrl());
		if (feedHistoryInfo)
		{
			feedHistoryInfo->SetStatus(FeedHistoryInfo::hsFetched);
		}
		else
		{
			m_feedHistory.emplace_back(queueAspect->nzbInfo->GetUrl(), FeedHistoryInfo::hsFetched, Util::CurrentTime());
		}
		m_save = true;
	}
}

bool FeedCoordinator::HasActiveDownloads()
{
	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	return !m_activeDownloads.empty();
}

void FeedCoordinator::CheckSaveFeeds()
{
	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	if (m_save)
	{
		debug("CheckSaveFeeds: save");
		if (g_Options->GetServerMode())
		{
			g_DiskState->SaveFeeds(&m_feeds, &m_feedHistory);
		}
		m_save = false;
	}
}

void FeedCoordinator::CleanupHistory()
{
	debug("CleanupHistory");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);

	time_t oldestUpdate = Util::CurrentTime();

	for (FeedInfo* feedInfo : &m_feeds)
	{
		if (feedInfo->GetLastUpdate() < oldestUpdate)
		{
			oldestUpdate = feedInfo->GetLastUpdate();
		}
	}

	time_t borderDate = (time_t)nzbget_rs_fuq_feed_coord_calc_history_border_date(
		(int64_t)oldestUpdate, g_Options->GetFeedHistory());

	m_feedHistory.erase(std::remove_if(m_feedHistory.begin(), m_feedHistory.end(),
		[borderDate, this](FeedHistoryInfo& feedHistoryInfo)
		{
			size_t urlLenWithoutQuery = 0;
			if (nzbget_rs_tpr_feed_coord_should_delete_history_entry(
				(int64_t)feedHistoryInfo.GetLastSeen(),
				(int64_t)borderDate,
				feedHistoryInfo.GetUrl(),
				&urlLenWithoutQuery) != 0)
			{
				// without the query: feed item links carry api keys
				detail("Deleting %.*s from feed history",
					(int)urlLenWithoutQuery, feedHistoryInfo.GetUrl());
				m_save = true;
				return true;
			}
			return false;
		}),
		m_feedHistory.end());
}

void FeedCoordinator::CleanupCache()
{
	debug("CleanupCache");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);

	time_t curTime = Util::CurrentTime();

	m_feedCache.remove_if(
		[curTime](FeedCacheItem& feedCacheItem)
		{
			if (nzbget_rs_df_is_feed_cache_expired(
				(int64_t)feedCacheItem.GetLastUsage(), feedCacheItem.GetCacheTimeSec(), (int64_t)curTime))
			{
				debug("Deleting %s from feed cache", feedCacheItem.GetUrl());
				return true;
			}
			return false;
		});
}
