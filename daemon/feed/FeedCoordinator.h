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


#ifndef FEEDCOORDINATOR_H
#define FEEDCOORDINATOR_H

#include <atomic>
#include "Util.h"
#include "Connection.h"
#include "DiskState.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

struct NzbgetRsFeedFileParser;

// rust/src/feed.rs
class FeedFile
{
public:
	FeedFile(const char* filename, const char* infoName)
		: m_feedItems(std::make_unique<FeedItemList>())
		, m_fileName(filename ? filename : "")
		, m_infoName(infoName ? infoName : "")
	{
		debug("Creating FeedFile");
	}

	~FeedFile()
	{
		nzbget_rs_feed_file_parser_free(m_parser);
	}

	bool Parse()
	{
		nzbget_rs_feed_file_parser_free(m_parser);
		m_parser = nzbget_rs_feed_file_parser_parse_file(m_fileName.c_str());
		if (!m_parser)
		{
			error("Failed to parse rss feed %s", m_infoName.c_str());
			return false;
		}

		size_t count = nzbget_rs_feed_file_parser_finalize(m_parser);
		for (size_t i = 0; i < count; ++i)
		{
			NzbgetRsParsedFeedItemView view{};
			if (nzbget_rs_feed_file_parser_get_item(m_parser, i, &view) == 0)
			{
				continue;
			}
			m_feedItems->emplace_back();
			FeedItemInfo& item = m_feedItems->back();
			item.SetTitle(view.title);
			item.SetFilename(view.filename);
			item.SetUrl(view.url);
			item.SetCategory(view.category);
			item.SetDescription(view.description);
			if (view.has_season != 0)
			{
				item.SetSeason(view.season);
			}
			if (view.has_episode != 0)
			{
				item.SetEpisode(view.episode);
			}
			item.SetTime((time_t)view.time);
			item.SetSize(view.size);
			item.SetImdbId(view.imdb_id);
			item.SetRageId(view.rage_id);
			item.SetTvdbId(view.tvdb_id);
			item.SetTvmazeId(view.tvmaze_id);
			for (size_t k = 0; k < view.attr_count; ++k)
			{
				item.GetAttributes()->emplace_back(view.attr_names[k], view.attr_values[k]);
			}
		}

		nzbget_rs_feed_file_parser_free(m_parser);
		m_parser = nullptr;
		return true;
	}

	std::unique_ptr<FeedItemList> DetachFeedItems() { return std::move(m_feedItems); }

	void LogDebugInfo()
	{
		info(" FeedFile %s", m_fileName.c_str());
	}

private:
	std::unique_ptr<FeedItemList> m_feedItems;
	std::string m_fileName;
	std::string m_infoName;
	NzbgetRsFeedFileParser* m_parser = nullptr;
};

// rust/src/feedfilter.rs
class FeedFilter
{
public:
	FeedFilter(const char* filter) :
		m_filter(nzbget_rs_feed_filter_new(filter))
	{
	}

	~FeedFilter()
	{
		nzbget_rs_feed_filter_free(m_filter);
	}

	FeedFilter(const FeedFilter&) = delete;
	FeedFilter& operator=(const FeedFilter&) = delete;

	void Match(FeedItemInfo& feedItemInfo)
	{
		NzbgetRsFeedItem item{};
		item.user = &feedItemInfo;
		item.field = Field;
		item.seasonEpisode = SeasonEpisode;
		item.apply = Apply;
		item.setMatch = SetMatch;
		nzbget_rs_feed_filter_match(m_filter, &item);
	}

private:
	NzbgetRsFeedFilter* m_filter;

	// Allocation failures and helper exceptions must not unwind through Rust.
	static void Field(void* user, int field, const char* attr, const char** str, long long* num) noexcept
	{
		FeedItemInfo* item = static_cast<FeedItemInfo*>(user);
		*str = nullptr;
		*num = 0;
		switch (field)
		{
			case 0: *str = item->GetTitle(); break;
			case 1: *str = item->GetFilename(); break;
			case 2: *str = item->GetCategory(); break;
			case 3: *str = item->GetUrl(); break;
			case 4: *num = item->GetSize(); break;
			case 5:
				*num = static_cast<time_t>(
					static_cast<uint64_t>(Util::CurrentTime()) - static_cast<uint64_t>(item->GetTime()));
				break;
			case 6: *num = item->GetImdbId(); break;
			case 7: *num = item->GetRageId(); break;
			case 8: *num = item->GetTvdbId(); break;
			case 9: *num = item->GetTvmazeId(); break;
			case 10: *str = item->GetDescription(); break;
			case 11: *num = item->GetSeasonNum(); break;
			case 12: *num = item->GetEpisodeNum(); break;
			case 13: *num = item->GetPriority(); break;
			case 14: *str = item->GetDupeKey(); break;
			case 15: *num = item->GetDupeScore(); break;
			case 16: *str = item->GetDupeStatus(); break;
			case 17:
			{
				FeedItemInfo::Attr* a = item->GetAttributes()->Find(attr);
				*str = a ? a->GetValue() : nullptr;
				break;
			}
		}
	}

	static const char* SeasonEpisode(void* user, int episode) noexcept
	{
		FeedItemInfo* item = static_cast<FeedItemInfo*>(user);
		// GetSeasonNum/GetEpisodeNum parse the title
		if (episode)
		{
			item->GetEpisodeNum();
			return item->GetEpisode();
		}
		item->GetSeasonNum();
		return item->GetSeason();
	}

	static void Apply(void* user, const struct NzbgetRsFeedOptions* o) noexcept
	{
		FeedItemInfo* item = static_cast<FeedItemInfo*>(user);
		if (o->hasPause)
		{
			item->SetPauseNzb(o->pause != 0);
		}
		if (o->hasCategory)
		{
			item->SetAddCategory(o->category);
		}
		if (o->hasPriority)
		{
			item->SetPriority(o->priority);
		}
		if (o->hasAddPriority)
		{
			item->SetPriority(static_cast<int>(
				static_cast<uint32_t>(item->GetPriority()) + static_cast<uint32_t>(o->addPriority)));
		}
		if (o->hasDupeScore)
		{
			item->SetDupeScore(o->dupeScore);
		}
		if (o->hasAddDupeScore)
		{
			item->SetDupeScore(static_cast<int>(
				static_cast<uint32_t>(item->GetDupeScore()) + static_cast<uint32_t>(o->addDupeScore)));
		}
		if (o->hasBuildDupeKey)
		{
			item->BuildDupeKey(o->ids[0], o->ids[1], o->ids[2], o->ids[3]);
		}
		if (o->hasDupeKey)
		{
			item->SetDupeKey(o->dupeKey);
		}
		if (o->hasAddDupeKey)
		{
			item->AppendDupeKey(o->addDupeKey);
		}
		if (o->hasDupeMode)
		{
			item->SetDupeMode(static_cast<EDupeMode>(o->dupeMode));
		}
	}

	static void SetMatch(void* user, int status, int rule) noexcept
	{
		FeedItemInfo* item = static_cast<FeedItemInfo*>(user);
		item->SetMatchStatus(
			status == 1 ? FeedItemInfo::msAccepted :
			status == 2 ? FeedItemInfo::msRejected :
			FeedItemInfo::msIgnored);
		item->SetMatchRule(rule);
	}
};


class FeedDownloader;

class NzbInfoCreator final
{
public:
	NzbInfoCreator() = default;
	~NzbInfoCreator() = default;

	std::unique_ptr<NzbInfo> Create(const FeedInfo& feedInfo, const FeedItemInfo& feedItemInfo) const;
};

class FeedCoordinator : public Thread, public Observer, public Subject, public Debuggable
{
public:
	FeedCoordinator();
	~FeedCoordinator() override;
	void Run() override;
	void Stop() override;
	void Update(Subject* caller, void* aspect) override;
	void AddFeed(std::unique_ptr<FeedInfo> feedInfo) { m_feeds.push_back(std::move(feedInfo)); }

	/* may return empty pointer on error */
	std::shared_ptr<FeedItemList> PreviewFeed(int id, const char* name, const char* url,
		const char* filter, bool backlog, bool pauseNzb, const char* category, FeedInfo::CategorySource categorySource, int priority,
		int interval, const char* feedScript, int cacheTimeSec, const char* cacheId);

	/* may return empty pointer on error */
	std::shared_ptr<FeedItemList> ViewFeed(int id);

	void FetchFeed(int id);
	bool HasActiveDownloads();
	Feeds* GetFeeds() { return &m_feeds; }

	/**
	 * @brief Resolves effective category for a feed item.
	 *
	 * Priority:
	 *  1. Feed.Category (if non-empty)
	 *  2. CategorySource fallback when Feed.Category is empty:
	 *     - NZBFile / Auto -> item's own category
	 *     - FeedFile -> empty
	 *
	 * @note Filter rules applied later (in FeedFilter::Match) may override
	 *       AddCategory with their own category.
	 */
	static std::string ResolveCategory(const FeedInfo& feedInfo, const FeedItemInfo& feedItemInfo);

protected:
	void LogDebugInfo() override;

private:
	class DownloadQueueObserver: public Observer
	{
	public:
		FeedCoordinator* m_owner;
		virtual void Update(Subject* caller, void* aspect) { m_owner->DownloadQueueUpdate(caller, aspect); }
	};

	class WorkStateObserver: public Observer
	{
	public:
		FeedCoordinator* m_owner;
		virtual void Update(Subject* caller, void* aspect) { m_owner->WorkStateUpdate(caller, aspect); }
	};

	class FeedCacheItem
	{
	public:
		FeedCacheItem(const char* url, int cacheTimeSec,const char* cacheId,
				time_t lastUsage, std::shared_ptr<FeedItemList> feedItems) :
			m_url(url), m_cacheTimeSec(cacheTimeSec), m_cacheId(cacheId),
			m_lastUsage(lastUsage), m_feedItems(feedItems) {}
		const char* GetUrl() { return m_url; }
		int GetCacheTimeSec() { return m_cacheTimeSec; }
		const char* GetCacheId() { return m_cacheId; }
		time_t GetLastUsage() { return m_lastUsage; }
		void SetLastUsage(time_t lastUsage) { m_lastUsage = lastUsage; }
		std::shared_ptr<FeedItemList> GetFeedItems() { return m_feedItems; }

	private:
		CString m_url;
		int m_cacheTimeSec;
		CString m_cacheId;
		time_t m_lastUsage;
		std::shared_ptr<FeedItemList> m_feedItems;
	};

	class FilterHelper : public FeedFilterHelper
	{
	public:
		virtual void CalcDupeStatus(const char* title, const char* dupeKey, char* statusBuf, int bufLen);
	};

	typedef std::list<FeedCacheItem> FeedCache;
	typedef std::deque<FeedDownloader*> ActiveDownloads;

	Feeds m_feeds;
	ActiveDownloads m_activeDownloads;
	FeedHistory m_feedHistory;
	std::mutex m_downloadsMutex;
	DownloadQueueObserver m_downloadQueueObserver;
	WorkStateObserver m_workStateObserver;
	NzbInfoCreator m_nzbInfoCreator;
	FeedCache m_feedCache;
	std::condition_variable m_waitCond;
	std::atomic<bool> m_force{false};
	bool m_wokenUp = false;
	bool m_save = false;

	void StartFeedDownload(FeedInfo* feedInfo, bool force);
	void FeedCompleted(FeedDownloader* feedDownloader);
	void FilterFeed(FeedInfo* feedInfo, FeedItemList* feedItems);
	std::vector<std::unique_ptr<NzbInfo>> ProcessFeed(FeedInfo* feedInfo, FeedItemList* feedItems);
	void ResetHangingDownloads();
	void DownloadQueueUpdate(Subject* caller, void* aspect);
	void CleanupHistory();
	void CleanupCache();
	void CheckSaveFeeds();
	std::unique_ptr<FeedFile> parseFeed(FeedInfo* feedInfo);
	void SchedulerNextUpdate(FeedInfo* feedInfo, bool success);
	void WorkStateUpdate(Subject* caller, void* aspect);
};

extern FeedCoordinator* g_FeedCoordinator;

class FeedDownloader : public WebDownloader
{
public:
	void SetFeedInfo(FeedInfo* feedInfo) { m_feedInfo = feedInfo; }
	FeedInfo* GetFeedInfo() { return m_feedInfo; }

private:
	FeedInfo* m_feedInfo;
};

#include "Util.h"
#include "Util.h"
#include "PrePostProcessor.h"
#include "DiskState.h"
#include "QueueCoordinator.h"
#include "QueueCoordinator.h"
#include "nzbget_rs.h"

class FeedScriptController : public NzbScriptController
{
public:
	static constexpr int FEED_SUCCESS = 93;

	static void ExecuteScripts(const char* feedScript, const char* feedFile, int feedId, bool* success)
	{
		FeedScriptController scriptController;
		scriptController.m_feedFile = feedFile;
		scriptController.m_feedId = feedId;

		scriptController.ExecuteScriptList(feedScript);

		if (success)
		{
			*success = scriptController.m_success;
		}
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		if (!script->GetFeedScript())
		{
			return;
		}

		PrintMessage(Message::mkInfo, "Executing feed-script %s for Feed%i", script->GetName(), m_feedId);

		SetArgs({script->GetEntry()});

		BString<1024> infoName("feed-script %s for Feed%i", script->GetName(), m_feedId);
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		PrepareParams(script->GetName());

		int exitCode = Execute();

		if (exitCode != FEED_SUCCESS)
		{
			infoName[0] = 'F';
			PrintMessage(Message::mkError, "%s failed", *infoName);
			m_success = false;
		}

		SetLogPrefix(nullptr);
	}

private:
	const char* m_feedFile;
	int m_feedId;
	bool m_success = true;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		SetEnvVar("NZBFP_FILENAME", m_feedFile);
		SetIntEnvVar("NZBFP_FEEDID", m_feedId);

		PrepareEnvScript(nullptr, scriptName);
	}
};

inline std::unique_ptr<NzbInfo> NzbInfoCreator::Create(const FeedInfo& feedInfo, const FeedItemInfo& feedItemInfo) const
{
	NzbgetRsBuf fnBuf{};
	NzbgetRsBuf catBuf{};
	nzbget_rs_feed_create_nzb_info(
		feedInfo.GetId(),
		feedInfo.GetCategory(),
		static_cast<int>(feedInfo.GetCategorySource()),
		feedItemInfo.GetUrl(),
		feedItemInfo.GetFilename(),
		feedItemInfo.GetCategory(),
		feedItemInfo.GetAddCategory().c_str(),
		feedItemInfo.GetSize(),
		(int64_t)feedItemInfo.GetTime(),
		feedItemInfo.GetPriority(),
		feedItemInfo.GetPauseNzb() ? 1 : 0,
		feedItemInfo.GetDupeKey(),
		feedItemInfo.GetDupeScore(),
		(int)feedItemInfo.GetDupeMode(),
		&fnBuf,
		&catBuf);

	std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
	nzbInfo->SetKind(NzbInfo::nkUrl);
	nzbInfo->SetFeedId(feedInfo.GetId());
	nzbInfo->SetUrl(feedItemInfo.GetUrl());
	if (fnBuf.len > 0 && fnBuf.data)
	{
		nzbInfo->SetFilename(fnBuf.data);
	}
	nzbInfo->SetCategory(catBuf.data ? catBuf.data : "");
	nzbget_rs_free(fnBuf);
	nzbget_rs_free(catBuf);

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

inline void FeedCoordinator::FilterHelper::CalcDupeStatus(const char* title, const char* dupeKey, char* statusBuf, int bufLen)
{
	DupeCoordinator::EDupeStatus dupeStatus = g_DupeCoordinator->GetDupeStatus(DownloadQueue::Guard(), title, dupeKey);

	NzbgetRsBuf buf = nzbget_rs_feed_format_dupe_status_mask((int)dupeStatus);
	strncpy(statusBuf, buf.data ? buf.data : "", bufLen);
	nzbget_rs_free(buf);
}

inline FeedCoordinator::FeedCoordinator()
{
	debug("Creating FeedCoordinator");

	m_downloadQueueObserver.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);

	m_workStateObserver.m_owner = this;
	g_WorkState->Attach(&m_workStateObserver);
}

inline FeedCoordinator::~FeedCoordinator()
{
	debug("Destroying FeedCoordinator");

	for (FeedDownloader* feedDownloader : m_activeDownloads)
	{
		delete feedDownloader;
	}
	m_activeDownloads.clear();
}

inline void FeedCoordinator::Run()
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
		if (!g_WorkState->GetPauseDownload() || m_force || g_Options->GetUrlForce())
		{
			std::lock_guard<std::mutex> guard(m_downloadsMutex);

			time_t current = Util::CurrentTime();
			if ((int)m_activeDownloads.size() < g_Options->GetUrlConnections())
			{
				m_force = false;
				for (FeedInfo* feedInfo : &m_feeds)
				{
					int setForce = 0;
					if (nzbget_rs_feed_should_start(
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

		time_t now = Util::CurrentTime();
		if (std::abs(now - lastCleanup) >= 60)
		{
			CleanupHistory();
			CleanupCache();
			CheckSaveFeeds();
			lastCleanup = Util::CurrentTime();
		}

		std::unique_lock<std::mutex> lk(m_downloadsMutex);
		if (m_force)
		{
			m_waitCond.wait_for(lk, std::chrono::milliseconds(1000), [&]{ return IsStopped(); });
		}
		else
		{
			int waitInterval = (g_Options->GetUrlForce() || m_feeds.empty()) ? 60000 : 5000;
			m_waitCond.wait_for(lk, std::chrono::milliseconds(waitInterval), [&]{ return m_force || IsStopped(); });
		}
	}

	debug("FeedCoordinator: waiting for Downloads to complete");
	bool completed = false;
	while (!completed)
	{
		{
			std::lock_guard<std::mutex> guard(m_downloadsMutex);
			completed = m_activeDownloads.empty();
		}
		CheckSaveFeeds();
		Util::Sleep(100);
		ResetHangingDownloads();
	}
	debug("FeedCoordinator: Downloads are completed");

	debug("Exiting FeedCoordinator-loop");
}

inline void FeedCoordinator::Stop()
{
	Thread::Stop();

	debug("Stopping UrlDownloads");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	for (FeedDownloader* feedDownloader : m_activeDownloads)
	{
		feedDownloader->Stop();
	}
	debug("UrlDownloads are notified");

	m_waitCond.notify_all();
}

inline void FeedCoordinator::WorkStateUpdate(Subject*, void*)
{
	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	m_force = true;
	m_waitCond.notify_all();
}

inline void FeedCoordinator::ResetHangingDownloads()
{
	if (g_Options->GetUrlTimeout() == 0)
	{
		return;
	}

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	time_t tm = Util::CurrentTime();

	for (FeedDownloader* feedDownloader: m_activeDownloads)
	{
		if (tm - feedDownloader->GetLastUpdateTime() > g_Options->GetUrlTimeout() + 10 &&
			feedDownloader->GetStatus() == FeedDownloader::adRunning)
		{
			error("Cancelling hanging feed download %s", feedDownloader->GetInfoName());
			feedDownloader->Stop();
		}
	}
}

inline void FeedCoordinator::LogDebugInfo()
{
	info("   ---------- FeedCoordinator");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	info("    Active Downloads: %i", (int)m_activeDownloads.size());
	for (FeedDownloader* feedDownloader : m_activeDownloads)
	{
		feedDownloader->LogDebugInfo();
	}
}

inline void FeedCoordinator::StartFeedDownload(FeedInfo* feedInfo, bool force)
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

	static std::atomic<int> previewNum{0};
	bool useFeedId = feedInfo->GetId() > 0 && !feedInfo->GetPreview();
	NzbgetRsBuf outFilenameBuf = nzbget_rs_feed_format_temp_filename(
		g_Options->GetTempDir(), PATH_SEPARATOR, feedInfo->GetId(),
		useFeedId ? 0 : 1,
		useFeedId ? 0 : (int)Util::CurrentTime(),
		useFeedId ? 0 : rand(),
		useFeedId ? 0 : ++previewNum);
	BString<1024> outFilename("%s", outFilenameBuf.data ? outFilenameBuf.data : "");
	nzbget_rs_free(outFilenameBuf);
	feedDownloader->SetOutputFilename(outFilename);

	feedInfo->SetStatus(FeedInfo::fsRunning);
	feedInfo->SetForce(force);
	feedInfo->SetFetch(false);

	m_activeDownloads.push_back(feedDownloader);
	feedDownloader->Start();
}

inline void FeedCoordinator::Update(Subject* caller, void*)
{
	debug("Notification from FeedDownloader received");

	FeedDownloader* feedDownloader = (FeedDownloader*) caller;
	WebDownloader::EStatus st = feedDownloader->GetStatus();
	if (st == WebDownloader::adFinished || st == WebDownloader::adFailed || st == WebDownloader::adRetry)
	{
		FeedCompleted(feedDownloader);
	}
}

inline void FeedCoordinator::FeedCompleted(FeedDownloader* feedDownloader)
{
	debug("Feed downloaded");

	FeedInfo* feedInfo = feedDownloader->GetFeedInfo();
	bool statusOK = feedDownloader->GetStatus() == WebDownloader::adFinished;
	if (statusOK)
	{
		feedInfo->SetOutputFilename(feedDownloader->GetOutputFilename());
	}

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
			const char* extensions = !Util::EmptyStr(feedInfo->GetExtensions())
				? feedInfo->GetExtensions()
				: g_Options->GetExtensions();
			FeedScriptController::ExecuteScripts(
				extensions, feedInfo->GetOutputFilename(), feedInfo->GetId(), &scriptSuccess);
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

inline void FeedCoordinator::SchedulerNextUpdate(FeedInfo* feedInfo, bool success)
{
	time_t current = Util::CurrentTime();
	int newLastInterval = 0;
	int interval = nzbget_rs_feed_calc_next_interval(
		success ? 1 : 0, feedInfo->GetInterval(), feedInfo->GetLastInterval(), &newLastInterval);
	feedInfo->SetLastInterval(newLastInterval);

	detail("Scheduling update for feed %s in %i minute(s)", feedInfo->GetName(), interval / 60);
	feedInfo->SetNextUpdate(current + interval);
}

inline std::string FeedCoordinator::ResolveCategory(const FeedInfo& feedInfo, const FeedItemInfo& feedItemInfo)
{
	return nzbget_rs_feed_resolve_category(
		feedInfo.GetCategory(),
		(int)feedInfo.GetCategorySource(),
		feedItemInfo.GetCategory());
}

inline void FeedCoordinator::FilterFeed(FeedInfo* feedInfo, FeedItemList* feedItems)
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

inline std::vector<std::unique_ptr<NzbInfo>> FeedCoordinator::ProcessFeed(FeedInfo* feedInfo, FeedItemList* feedItems)
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
			FeedHistoryInfo::EStatus status = (FeedHistoryInfo::EStatus)nzbget_rs_feed_classify_item_history(
				firstFetch ? 1 : 0, feedInfo->GetBacklog() ? 1 : 0, feedHistoryInfo != nullptr ? 1 : 0,
				&shouldAdd);
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

inline std::shared_ptr<FeedItemList> FeedCoordinator::ViewFeed(int id)
{
	if (id < 1 || id > static_cast<int>(m_feeds.size()))
	{
		return nullptr;
	}

	std::unique_ptr<FeedInfo>& feedInfo = m_feeds[static_cast<size_t>(id - 1)];

	return PreviewFeed(feedInfo->GetId(), feedInfo->GetName(), feedInfo->GetUrl(), feedInfo->GetFilter(),
		feedInfo->GetBacklog(), feedInfo->GetPauseNzb(), feedInfo->GetCategory(), feedInfo->GetCategorySource(),
		feedInfo->GetPriority(), feedInfo->GetInterval(), feedInfo->GetExtensions(), 0, nullptr);
}

inline std::shared_ptr<FeedItemList> FeedCoordinator::PreviewFeed(int id,
	const char* name, const char* url, const char* filter, bool backlog, bool pauseNzb,
	const char* category, FeedInfo::CategorySource categorySource, int priority, int interval, const char* feedScript,
	int cacheTimeSec, const char* cacheId)
{
	debug("Preview feed %s", name);

	unsigned int certVerifLevel = Options::ECertVerifLevel::cvStrict;
	if (id >= 1 && id <= static_cast<int>(m_feeds.size()))
	{
		certVerifLevel = m_feeds[static_cast<size_t>(id - 1)]->GetCertVerificationLevel();
	}

	std::unique_ptr<FeedInfo> feedInfo = std::make_unique<FeedInfo>(id, name, url, backlog, interval,
		filter, pauseNzb, category, categorySource, priority, feedScript, certVerifLevel);
	feedInfo->SetPreview(true);

	std::shared_ptr<FeedItemList> feedItems;
	bool hasCache = false;
	if (cacheTimeSec > 0 && !Util::EmptyStr(cacheId))
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
				if (!strcmp(feedInfo2->GetUrl(), feedInfo->GetUrl()) &&
					!strcmp(feedInfo2->GetFilter(), feedInfo->GetFilter()) &&
					feedInfo2->GetLastUpdate() > 0)
				{
					firstFetch = false;
					break;
				}
			}

			StartFeedDownload(feedInfo.get(), true);

			m_force = true;
			m_waitCond.notify_all();
		}

		while (feedInfo->GetStatus() == FeedInfo::fsRunning)
		{
			Util::Sleep(100);
		}

		if (feedInfo->GetStatus() != FeedInfo::fsFinished)
		{
			return nullptr;
		}

		const char* extensions = !Util::EmptyStr(feedInfo->GetExtensions())
			? feedInfo->GetExtensions()
			: g_Options->GetExtensions();
		FeedScriptController::ExecuteScripts(
			extensions, feedInfo->GetOutputFilename(), feedInfo->GetId(), nullptr);

		std::unique_ptr<FeedFile> feedFile = parseFeed(feedInfo.get());
		if (!feedFile)
		{
			return nullptr;
		}

		feedItems = feedFile->DetachFeedItems();
		feedFile.reset();

		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		for (FeedItemInfo& feedItemInfo : feedItems.get())
		{
			FeedHistoryInfo* feedHistoryInfo = m_feedHistory.Find(feedItemInfo.GetUrl());
			feedItemInfo.SetStatus((FeedItemInfo::EStatus)nzbget_rs_feed_resolve_preview_item_status(
				feedHistoryInfo != nullptr ? 1 : 0,
				feedHistoryInfo ? (int)feedHistoryInfo->GetStatus() : 0,
				firstFetch ? 1 : 0,
				feedInfo->GetBacklog() ? 1 : 0));
		}
	}

	FilterFeed(feedInfo.get(), feedItems.get());
	feedInfo.reset();

	if (!hasCache && cacheTimeSec > 0 && !Util::EmptyStr(cacheId))
	{
		std::lock_guard<std::mutex> guard(m_downloadsMutex);
		m_feedCache.emplace_back(url, cacheTimeSec, cacheId, Util::CurrentTime(), feedItems);
	}

	return feedItems;
}

inline void FeedCoordinator::FetchFeed(int id)
{
	debug("FetchFeeds");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	for (FeedInfo* feedInfo : &m_feeds)
	{
		if (feedInfo->GetId() == id || id == 0)
		{
			feedInfo->SetFetch(true);
			m_force = true;
		}
	}

	m_waitCond.notify_all();
}

inline std::unique_ptr<FeedFile> FeedCoordinator::parseFeed(FeedInfo* feedInfo)
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

inline void FeedCoordinator::DownloadQueueUpdate(Subject*, void* aspect)
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

inline bool FeedCoordinator::HasActiveDownloads()
{
	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	return !m_activeDownloads.empty();
}

inline void FeedCoordinator::CheckSaveFeeds()
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

inline void FeedCoordinator::CleanupHistory()
{
	debug("CleanupHistory");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);

	std::vector<int64_t> lastUpdates;
	lastUpdates.reserve(m_feeds.size());
	for (FeedInfo* feedInfo : &m_feeds)
	{
		lastUpdates.push_back((int64_t)feedInfo->GetLastUpdate());
	}

	time_t borderDate = (time_t)nzbget_rs_feed_calc_history_border_date(
		(int64_t)Util::CurrentTime(),
		lastUpdates.data(),
		lastUpdates.size(),
		g_Options->GetFeedHistory());

	m_feedHistory.erase(std::remove_if(m_feedHistory.begin(), m_feedHistory.end(),
		[borderDate, this](FeedHistoryInfo& feedHistoryInfo)
		{
			size_t urlLenWithoutQuery = 0;
			if (nzbget_rs_feed_should_delete_history_entry(
				(int64_t)feedHistoryInfo.GetLastSeen(),
				(int64_t)borderDate,
				feedHistoryInfo.GetUrl(),
				&urlLenWithoutQuery) != 0)
			{
				detail("Deleting %.*s from feed history",
					(int)urlLenWithoutQuery, feedHistoryInfo.GetUrl());
				m_save = true;
				return true;
			}
			return false;
		}),
		m_feedHistory.end());
}

inline void FeedCoordinator::CleanupCache()
{
	debug("CleanupCache");

	std::lock_guard<std::mutex> guard(m_downloadsMutex);

	time_t curTime = Util::CurrentTime();

	m_feedCache.remove_if(
		[curTime](FeedCacheItem& feedCacheItem)
		{
			if (nzbget_rs_feed_is_cache_expired(
				(int64_t)feedCacheItem.GetLastUsage(), feedCacheItem.GetCacheTimeSec(), (int64_t)curTime))
			{
				debug("Deleting %s from feed cache", feedCacheItem.GetUrl());
				return true;
			}
			return false;
		});
}

#
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2008-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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


#ifndef SCHEDULER_H
#define SCHEDULER_H
#include "Util.h"
#include "Util.h"
#include "ServerPool.h"
#include "PrePostProcessor.h"
#include "nzbget_rs.h"

class SchedulerScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run()
	{
		if (m_externalProcess)
		{
			ExecuteExternalProcess();
		}
		else
		{
			ExecuteScriptList(m_script);
		}
	}

	static void StartScript(const char* param, bool externalProcess, int taskId)
	{
		std::vector<CString> argv;
		if (externalProcess && (argv = Util::SplitCommandLine(param)).empty())
		{
			error("Could not execute scheduled process-script, failed to parse command line: %s", param);
			return;
		}

		SchedulerScriptController* scriptController = new SchedulerScriptController();

		scriptController->m_externalProcess = externalProcess;
		scriptController->m_script = param;
		scriptController->m_taskId = taskId;

		if (externalProcess)
		{
			scriptController->SetArgs(std::move(argv));
		}

		scriptController->SetAutoDestroy(true);

		scriptController->Start();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		if (!script->GetSchedulerScript())
		{
			return;
		}

		BString<1024> taskName;
		if (m_taskId > 0)
		{
			taskName.Format(" for Task%i", m_taskId);
		}
		else
		{
			taskName = "";
		}

		PrintMessage(Message::mkInfo, "Executing scheduler-script %s%s", script->GetName(), *taskName);

		SetArgs({script->GetEntry()});

		BString<1024> infoName("scheduler-script %s%s", script->GetName(), *taskName);
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		PrepareParams(script->GetName());

		Execute();

		SetLogPrefix(nullptr);
	}

private:
	CString m_script;
	bool m_externalProcess;
	int m_taskId;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		SetIntEnvVar("NZBSP_TASKID", m_taskId);

		PrepareEnvScript(nullptr, scriptName);
	}

	void ExecuteExternalProcess()
	{
		info("Executing scheduled process-script %s for Task%i", FileSystem::BaseFileName(GetScript()), m_taskId);

		BString<1024> infoName("scheduled process-script %s for Task%i", FileSystem::BaseFileName(GetScript()), m_taskId);
		SetInfoName(infoName);

		BString<1024> logPrefix = FileSystem::BaseFileName(GetScript());
		if (char* ext = strrchr(logPrefix, '.')) *ext = '\0';
		SetLogPrefix(logPrefix);

		Execute();
	}
};

class Scheduler : public Service
{
public:
	enum ECommand
	{
		scPauseDownload,
		scUnpauseDownload,
		scPausePostProcess,
		scUnpausePostProcess,
		scDownloadRate,
		scExtensions,
		scProcess,
		scPauseScan,
		scUnpauseScan,
		scActivateServer,
		scDeactivateServer,
		scFetchFeed
	};

	class Task
	{
	public:
		Task(int id, int hours, int minutes, int weekDaysBits, ECommand command, const char* param)
			: m_id(id),
			  m_hours(hours),
			  m_minutes(minutes),
			  m_weekDaysBits(weekDaysBits),
			  m_command(command),
			  m_param(param ? param : "")
		{
		}
		friend class Scheduler;
		static const int STARTUP_TASK = -1;
		int GetId() const { return m_id; }
		int GetHours() const { return m_hours; }
		int GetMinutes() const { return m_minutes; }
		int GetWeeDaysBits() const { return m_weekDaysBits; }
		ECommand GetCommand() const { return m_command; }
		const std::string& GetParam() const { return m_param; }

	private:
		const int m_id;
		const int m_hours;
		const int m_minutes;
		const int m_weekDaysBits;
		const ECommand m_command;
		const std::string m_param;
		time_t m_lastExecuted = 0;
	};

	using TaskList = std::vector<std::unique_ptr<Task>>;

	const TaskList& GetTasks() const { return m_taskList; }
	void AddTask(std::unique_ptr<Task> task)
	{
		Guard guard(m_taskListMutex);
		m_taskList.push_back(std::move(task));
	}

protected:
	int ServiceInterval() override { return m_serviceInterval; }
	void ServiceWork() override
	{
		debug("Scheduler service work");

		if (!DownloadQueue::IsLoaded())
		{
			return;
		}

		debug("Scheduler service work: doing work");

		if (!m_firstChecked)
		{
			FirstCheck();
			m_firstChecked = true;
			return;
		}

		m_executeProcess = true;
		CheckTasks();
		CheckScheduledResume();
		ScheduleNextWork();
	}

private:
	using ServerStatusList = std::vector<bool>;

	TaskList m_taskList;
	Mutex m_taskListMutex;
	time_t m_lastCheck = 0;
	bool m_downloadRateChanged;
	bool m_executeProcess;
	bool m_pauseDownloadChanged;
	bool m_pausePostProcessChanged;
	bool m_pauseScanChanged;
	bool m_serverChanged;
	ServerStatusList m_serverStatusList;
	bool m_firstChecked = false;
	int m_serviceInterval = 1;

	void ExecuteTask(Task* task)
	{
#ifdef DEBUG
		debug("Executing scheduled command: %s",
			nzbget_rs_dqh_scheduler_command_name((int)task->m_command));
#endif

		bool executeProcess = nzbget_rs_sfw_scheduler_should_execute_process(
			m_executeProcess ? 1 : 0, task->m_hours, Task::STARTUP_TASK) != 0;

		switch (task->m_command)
		{
			case scDownloadRate:
				if (!task->m_param.empty())
				{
					g_WorkState->SetSpeedLimit(
						nzbget_rs_sfw_scheduler_calc_speed_limit_bytes(
							nzbget_rs_dqh_parse_c_atoi(task->m_param.c_str())));
					m_downloadRateChanged = true;
				}
				break;

			case scPauseDownload:
			case scUnpauseDownload:
				g_WorkState->SetPauseDownload(task->m_command == scPauseDownload);
				m_pauseDownloadChanged = true;
				break;

			case scPausePostProcess:
			case scUnpausePostProcess:
				g_WorkState->SetPausePostProcess(task->m_command == scPausePostProcess);
				m_pausePostProcessChanged = true;
				break;

			case scPauseScan:
			case scUnpauseScan:
				g_WorkState->SetPauseScan(task->m_command == scPauseScan);
				m_pauseScanChanged = true;
				break;

			case scExtensions:
			case scProcess:
				if (executeProcess)
				{
					SchedulerScriptController::StartScript(task->m_param.c_str(), task->m_command == scProcess, task->m_id);
				}
				break;

			case scActivateServer:
			case scDeactivateServer:
				EditServer(task->m_command == scActivateServer, task->m_param.c_str());
				break;

			case scFetchFeed:
				if (executeProcess)
				{
					FetchFeed(task->m_param.c_str());
					break;
				}
		}
	}

	void CheckTasks()
	{
		PrepareLog();

		{
			Guard guard(m_taskListMutex);

			time_t current = Util::CurrentTime();

			std::vector<NzbgetRsSchedTask> tasks;
			tasks.reserve(m_taskList.size());
			for (Task* task : &m_taskList)
			{
				tasks.push_back({task->m_hours, task->m_minutes, task->m_weekDaysBits,
					static_cast<long long>(task->m_lastExecuted)});
			}
			std::vector<size_t> due(m_taskList.size() * 9);
			long long lastCheck = m_lastCheck;
			int reset = 0;
			// StatMeter can change the atomic offset between these two readings.
			// Preserve the original readings and reuse them if the buffer grows.
			int currentOffset = tasks.empty() ? 0 : g_WorkState->GetLocalTimeOffset();
			int lastCheckOffset = tasks.empty() ? 0 : g_WorkState->GetLocalTimeOffset();
			auto calendar = [](long long value, NzbgetRsSchedTm* result)
			{
				time_t time = static_cast<time_t>(value);
				tm fields{};
				gmtime_r(&time, &fields);
				*result = {static_cast<long long>(fields.tm_year) + 1900, fields.tm_mon,
					fields.tm_mday, fields.tm_hour, fields.tm_min, fields.tm_sec, fields.tm_wday};
			};
			size_t dueCount;
			while (true)
			{
				dueCount = nzbget_rs_scheduler_check(tasks.data(), tasks.size(), &lastCheck, current,
					currentOffset, lastCheckOffset, due.data(), due.size(), &reset, calendar);
				if (dueCount <= due.size())
				{
					break;
				}
				// Leap corrections in libc's calendar can exceed the usual count * 9.
				// An undersized output leaves all input state unchanged for this retry.
				due.resize(dueCount);
			}

			if (reset)
			{
				debug("Reset scheduled tasks (detected clock change greater than 90 minutes or negative)");
				m_executeProcess = false;
			}
			for (size_t i = 0; i < tasks.size(); i++)
			{
				m_taskList[i]->m_lastExecuted = static_cast<time_t>(tasks[i].lastExecuted);
			}
			for (size_t i = 0; i < dueCount; i++)
			{
				ExecuteTask(m_taskList[due[i]].get());
			}
			m_lastCheck = static_cast<time_t>(lastCheck);
		}

		PrintLog();
	}

	void PrepareLog()
	{
		m_downloadRateChanged = false;
		m_pauseDownloadChanged = false;
		m_pausePostProcessChanged = false;
		m_pauseScanChanged = false;
		m_serverChanged = false;
	}

	void PrintLog()
	{
		if (m_downloadRateChanged)
		{
			info("Scheduler: setting download rate to %i KB/s",
				nzbget_rs_dqh_scheduler_bytes_to_kb(g_WorkState->GetSpeedLimit()));
		}
		if (m_pauseDownloadChanged)
		{
			info("Scheduler: %s download",
				nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPauseDownload() ? 1 : 0));
		}
		if (m_pausePostProcessChanged)
		{
			info("Scheduler: %s post-processing",
				nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPausePostProcess() ? 1 : 0));
		}
		if (m_pauseScanChanged)
		{
			info("Scheduler: %s scan",
				nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPauseScan() ? 1 : 0));
		}
		if (m_serverChanged)
		{
			int index = 0;
			for (NewsServer* server : g_ServerPool->GetServers())
			{
				if (server->GetActive() != m_serverStatusList[index])
				{
					info("Scheduler: %s %s",
						nzbget_rs_dqh_scheduler_server_action_str(server->GetActive() ? 1 : 0),
						server->GetName());
				}
				index++;
			}
			g_ServerPool->Changed();
		}
	}

	void EditServer(bool active, const char* serverList)
	{
		Tokenizer tok(serverList, ",;");
		while (const char* serverRef = tok.Next())
		{
			int id = nzbget_rs_dqh_parse_c_atoi(serverRef);
			for (NewsServer* server : g_ServerPool->GetServers())
			{
				if (nzbget_rs_sfw_scheduler_matches_server_ref(
					id, server->GetId(), server->GetName(), serverRef) != 0)
				{
					if (!m_serverChanged)
					{
						// store old server status for logging
						m_serverStatusList.clear();
						m_serverStatusList.reserve(g_ServerPool->GetServers()->size());
						for (NewsServer* server2 : g_ServerPool->GetServers())
						{
							m_serverStatusList.push_back(server2->GetActive());
						}
					}
					m_serverChanged = true;
					server->SetActive(active);
					break;
				}
			}
		}
	}

	void FetchFeed(const char* feedList)
	{
		Tokenizer tok(feedList, ",;");
		while (const char* feedRef = tok.Next())
		{
			int id = nzbget_rs_dqh_parse_c_atoi(feedRef);
			for (FeedInfo* feed : g_FeedCoordinator->GetFeeds())
			{
				int targetFeedId = 0;
				if (nzbget_rs_sfw_scheduler_matches_feed_ref(
					id, feed->GetId(), feed->GetName(), feedRef, &targetFeedId) != 0)
				{
					g_FeedCoordinator->FetchFeed(targetFeedId);
					break;
				}
			}
		}
	}

	void CheckScheduledResume()
	{
		time_t resumeTime = g_WorkState->GetResumeTime();
		time_t currentTime = Util::CurrentTime();
		if (nzbget_rs_sfw_scheduler_should_autoresume((int64_t)resumeTime, (int64_t)currentTime) != 0)
		{
			info("Autoresume");
			g_WorkState->SetResumeTime(0);
			g_WorkState->SetPauseDownload(false);
			g_WorkState->SetPausePostProcess(false);
			g_WorkState->SetPauseScan(false);
		}
	}

	void FirstCheck()
	{
		{
			Guard guard(m_taskListMutex);

			std::sort(m_taskList.begin(), m_taskList.end(),
				[](const std::unique_ptr<Task>& task1, const std::unique_ptr<Task>& task2)
			{
				return nzbget_rs_sfw_scheduler_task_less(
					task1->m_hours, task1->m_minutes, task2->m_hours, task2->m_minutes) != 0;
			});
		}

		// check all tasks for the last week
		CheckTasks();
	}

	void ScheduleNextWork()
	{
		m_serviceInterval = nzbget_rs_scheduler_calc_next_work_interval(
			static_cast<int64_t>(Util::CurrentTime()));
	}
};

#ifndef SYSTEM_HEALTH_H
#define SYSTEM_HEALTH_H

#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace SystemHealth
{
class Service
{
public:
	Service(const Options& options, const Servers& servers, const ::Feeds& feeds,
			const ::Scheduler::TaskList& tasks, const ::Log& log)
		: m_options(options), m_servers(servers), m_feeds(feeds), m_tasks(tasks), m_log(log)
	{
	}

	void DiagnoseAndLog() const
	{
		NzbgetRsBuf buf = RunDiagnose(0);
		nzbget_rs_free(buf);
	}

	NzbgetRsBuf DiagnoseSerialized(bool isJson) const
	{
		return RunDiagnose(isJson ? 1 : 2);
	}

private:
	NzbgetRsBuf RunDiagnose(int formatMode) const
	{
		std::string mainDir = m_options.GetMainDirPath().string();
		std::string destDir = m_options.GetDestDirPath().string();
		std::string interDir = m_options.GetInterDirPath().string();
		std::string nzbDir = m_options.GetNzbDirPath().string();
		std::string queueDir = m_options.GetQueueDirPath().string();
		std::string webDir = m_options.GetWebDirPath().string();
		std::string tempDir = m_options.GetTempDirPath().string();
		std::string configTemplate = m_options.GetConfigTemplatePath().string();
		std::string configFile = m_options.GetConfigFilePath().string();
		std::string logFile = m_log.GetLogFilePath().string();
		std::string optionsLogFile = m_options.GetLogFilePath().string();
		std::string_view certStore = m_options.GetCertStore();
		std::string certStorePath = m_options.GetCertStorePath().string();
		std::string lockFile = m_options.GetLockFilePath().string();
		std::string unrarPath = m_options.GetUnrarPath().string();
		std::string sevenZipPath = m_options.GetSevenZipPath().string();

		const auto& scriptDirPaths = m_options.GetScriptDirPaths();
		std::vector<std::string> scriptDirStrs;
		scriptDirStrs.reserve(scriptDirPaths.size());
		std::vector<const char*> scriptDirPtrs;
		std::vector<size_t> scriptDirLens;
		scriptDirPtrs.reserve(scriptDirPaths.size());
		scriptDirLens.reserve(scriptDirPaths.size());
		for (const auto& dir : scriptDirPaths)
		{
			scriptDirStrs.push_back(dir.string());
			scriptDirPtrs.push_back(scriptDirStrs.back().data());
			scriptDirLens.push_back(scriptDirStrs.back().size());
		}

		std::vector<NzbgetRsHealthServerInput> servers;
		servers.reserve(m_servers.size());
		for (const auto& server : m_servers)
		{
			std::string_view name = server->GetName();
			std::string_view host = server->GetHost();
			std::string_view user = server->GetUser();
			std::string_view pass = server->GetPassword();
			std::string_view cipher = server->GetCipher();
			servers.push_back(NzbgetRsHealthServerInput{
				server->GetId(),
				server->GetActive() ? 1 : 0,
				name.data(), name.size(),
				server->GetLevel(),
				server->GetOptional() ? 1 : 0,
				server->GetGroup(),
				host.data(), host.size(),
				server->GetPort(),
				user.data(), user.size(),
				pass.data(), pass.size(),
				server->GetTls() ? 1 : 0,
				cipher.data(), cipher.size(),
				server->GetMaxConnections(),
				server->GetRetention(),
				static_cast<uint32_t>(server->GetCertVerificationLevel()),
				server->GetIpVersion()
			});
		}

		std::vector<NzbgetRsHealthTaskInput> tasks;
		tasks.reserve(m_tasks.size());
		for (const auto& task : m_tasks)
		{
			tasks.push_back(NzbgetRsHealthTaskInput{
				task->GetId(),
				task->GetHours(),
				task->GetMinutes(),
				task->GetWeeDaysBits()
			});
		}

		std::vector<NzbgetRsHealthCategoryInput> categories;
		if (const auto* cats = m_options.GetCategories())
		{
			categories.reserve(cats->size());
			for (const auto& cat : *cats)
			{
				std::string_view name = cat.GetName();
				categories.push_back(NzbgetRsHealthCategoryInput{
					name.data(), name.size(),
					cat.GetUnpack() ? 1 : 0
				});
			}
		}

		std::vector<NzbgetRsHealthFeedInput> feeds;
		feeds.reserve(m_feeds.size());
		for (const auto& feed : m_feeds)
		{
			std::string_view name = feed->GetName();
			std::string_view url = feed->GetUrl();
			feeds.push_back(NzbgetRsHealthFeedInput{
				feed->GetId(),
				name.data(), name.size(),
				url.data(), url.size(),
				feed->GetInterval(),
				static_cast<uint32_t>(feed->GetCertVerificationLevel())
			});
		}

		std::string_view controlIp = m_options.GetControlIp();
		std::string_view controlUser = m_options.GetControlUsername();
		std::string_view controlPass = m_options.GetControlPassword();
		std::string_view restrictedUser = m_options.GetRestrictedUsername();
		std::string_view restrictedPass = m_options.GetRestrictedPassword();
		std::string_view addUser = m_options.GetAddUsername();
		std::string_view addPass = m_options.GetAddPassword();
		std::string_view secureCert = m_options.GetSecureCert();
		std::string_view secureKey = m_options.GetSecureKey();
		std::string_view daemonUser = m_options.GetDaemonUsername();
		std::string_view extCleanup = m_options.GetExtCleanupDisk();
		std::string_view unpackPassFile = m_options.GetUnpackPassFile();
		std::string_view unpackIgnoreExt = m_options.GetUnpackIgnoreExt();

		NzbgetRsSystemHealthConfig cfg{};
		cfg.main_dir = mainDir.data();
		cfg.main_dir_len = mainDir.size();
		cfg.dest_dir = destDir.data();
		cfg.dest_dir_len = destDir.size();
		cfg.inter_dir = interDir.data();
		cfg.inter_dir_len = interDir.size();
		cfg.nzb_dir = nzbDir.data();
		cfg.nzb_dir_len = nzbDir.size();
		cfg.queue_dir = queueDir.data();
		cfg.queue_dir_len = queueDir.size();
		cfg.web_dir = webDir.data();
		cfg.web_dir_len = webDir.size();
		cfg.temp_dir = tempDir.data();
		cfg.temp_dir_len = tempDir.size();
		cfg.script_dir_ptrs = scriptDirPtrs.data();
		cfg.script_dir_lens = scriptDirLens.data();
		cfg.script_dir_count = scriptDirPaths.size();
		cfg.config_template = configTemplate.data();
		cfg.config_template_len = configTemplate.size();
		cfg.log_file = logFile.data();
		cfg.log_file_len = logFile.size();
		cfg.options_log_file = optionsLogFile.data();
		cfg.options_log_file_len = optionsLogFile.size();
		cfg.cert_store = certStore.data();
		cfg.cert_store_len = certStore.size();
		cfg.cert_store_path = certStorePath.data();
		cfg.cert_store_path_len = certStorePath.size();
		cfg.config_file = configFile.data();
		cfg.config_file_len = configFile.size();
		cfg.lock_file = lockFile.data();
		cfg.lock_file_len = lockFile.size();
		cfg.write_log = static_cast<int>(m_options.GetWriteLog());
		cfg.cert_check = m_options.GetCertCheck() ? 1 : 0;
		cfg.daemon_mode = m_options.GetDaemonMode() ? 1 : 0;
		cfg.servers = servers.data();
		cfg.server_count = servers.size();
		cfg.output_mode = static_cast<int>(m_options.GetOutputMode());
		cfg.update_interval = m_options.GetUpdateInterval();
		cfg.curses_nzb_name = m_options.GetCursesNzbName() ? 1 : 0;
		cfg.curses_group = m_options.GetCursesGroup() ? 1 : 0;
		cfg.curses_time = m_options.GetCursesTime() ? 1 : 0;
		cfg.rotate_log = m_options.GetRotateLog();
		cfg.log_buffer = m_options.GetLogBuffer();
		cfg.crash_dump = m_options.GetCrashDump() ? 1 : 0;
		cfg.time_correction = m_options.GetTimeCorrection();
		cfg.article_retries = m_options.GetArticleRetries();
		cfg.hc_dupe = m_options.GetHealthCheck() == Options::hcDupe ? 1 : 0;
		cfg.dupe_search = m_options.GetDupeSearch() ? 1 : 0;
		cfg.article_interval = m_options.GetArticleInterval();
		cfg.article_timeout = m_options.GetArticleTimeout();
		cfg.article_read_chunk_size = m_options.GetArticleReadChunkSize();
		cfg.url_retries = m_options.GetUrlRetries();
		cfg.url_interval = m_options.GetUrlInterval();
		cfg.url_timeout = m_options.GetUrlTimeout();
		cfg.url_connections = m_options.GetUrlConnections();
		cfg.remote_timeout = m_options.GetRemoteTimeout();
		cfg.download_rate = m_options.GetDownloadRate();
		cfg.monthly_quota = m_options.GetMonthlyQuota();
		cfg.quota_start_day = m_options.GetQuotaStartDay();
		cfg.daily_quota = m_options.GetDailyQuota();
		cfg.flush_queue = m_options.GetFlushQueue() ? 1 : 0;
		cfg.continue_partial = m_options.GetContinuePartial() ? 1 : 0;
		cfg.propagation_delay = m_options.GetPropagationDelay();
		cfg.article_cache = m_options.GetArticleCache();
		cfg.direct_write = m_options.GetDirectWrite() ? 1 : 0;
		cfg.write_buffer = m_options.GetWriteBuffer();
		cfg.file_naming = static_cast<int>(m_options.GetFileNaming());
		cfg.post_strategy = static_cast<int>(m_options.GetPostStrategy());
		cfg.disk_space = m_options.GetDiskSpace();
		cfg.nzb_cleanup_disk = m_options.GetNzbCleanupDisk() ? 1 : 0;
		cfg.keep_history = m_options.GetKeepHistory();
		cfg.feed_history = m_options.GetFeedHistory();
		cfg.skip_write = m_options.GetSkipWrite() ? 1 : 0;
		cfg.raw_article = m_options.GetRawArticle() ? 1 : 0;
		cfg.control_ip = controlIp.data();
		cfg.control_ip_len = controlIp.size();
		cfg.control_port = m_options.GetControlPort();
		cfg.control_username = controlUser.data();
		cfg.control_username_len = controlUser.size();
		cfg.control_password = controlPass.data();
		cfg.control_password_len = controlPass.size();
		cfg.restricted_username = restrictedUser.data();
		cfg.restricted_username_len = restrictedUser.size();
		cfg.restricted_password = restrictedPass.data();
		cfg.restricted_password_len = restrictedPass.size();
		cfg.add_username = addUser.data();
		cfg.add_username_len = addUser.size();
		cfg.add_password = addPass.data();
		cfg.add_password_len = addPass.size();
		cfg.form_auth = m_options.GetFormAuth() ? 1 : 0;
		cfg.secure_control = m_options.GetSecureControl() ? 1 : 0;
		cfg.secure_cert = secureCert.data();
		cfg.secure_cert_len = secureCert.size();
		cfg.secure_key = secureKey.data();
		cfg.secure_key_len = secureKey.size();
		cfg.secure_port = m_options.GetSecurePort();
		cfg.cert_store_path_empty = m_options.GetCertStorePath().empty() ? 1 : 0;
		cfg.daemon_username = daemonUser.data();
		cfg.daemon_username_len = daemonUser.size();
		cfg.umask = m_options.GetUMask();
		cfg.append_category_dir = m_options.GetAppendCategoryDir() ? 1 : 0;
		cfg.nzb_dir_interval = m_options.GetNzbDirInterval();
		cfg.nzb_dir_file_age = m_options.GetNzbDirFileAge();
		cfg.dupe_check = m_options.GetDupeCheck() ? 1 : 0;
		cfg.health_check = static_cast<int>(m_options.GetHealthCheck());
		cfg.crc_check = m_options.GetCrcCheck() ? 1 : 0;
		cfg.par_check = static_cast<int>(m_options.GetParCheck());
		cfg.par_repair = m_options.GetParRepair() ? 1 : 0;
		cfg.par_quick = m_options.GetParQuick() ? 1 : 0;
		cfg.par_buffer = m_options.GetParBuffer();
		cfg.par_threads = m_options.GetParThreads();
		cfg.hw_threads = std::thread::hardware_concurrency();
		cfg.par_rename = m_options.GetParRename() ? 1 : 0;
		cfg.rar_rename = m_options.GetRarRename() ? 1 : 0;
		cfg.direct_rename = m_options.GetDirectRename() ? 1 : 0;
		cfg.hard_linking = m_options.GetHardLinking() ? 1 : 0;
		cfg.par_time_limit = m_options.GetParTimeLimit();
		cfg.unpack = m_options.GetUnpack() ? 1 : 0;
		cfg.direct_unpack = m_options.GetDirectUnpack() ? 1 : 0;
		cfg.unpack_pause_queue = m_options.GetUnpackPauseQueue() ? 1 : 0;
		cfg.unrar_path = unrarPath.data();
		cfg.unrar_path_len = unrarPath.size();
		cfg.seven_zip_path = sevenZipPath.data();
		cfg.seven_zip_path_len = sevenZipPath.size();
		cfg.ext_cleanup_disk = extCleanup.data();
		cfg.ext_cleanup_disk_len = extCleanup.size();
		cfg.unpack_pass_file = unpackPassFile.data();
		cfg.unpack_pass_file_len = unpackPassFile.size();
		cfg.unpack_ignore_ext = unpackIgnoreExt.data();
		cfg.unpack_ignore_ext_len = unpackIgnoreExt.size();
		cfg.tasks = tasks.data();
		cfg.task_count = tasks.size();
		cfg.categories = categories.data();
		cfg.category_count = categories.size();
		cfg.feeds = feeds.data();
		cfg.feed_count = feeds.size();

		auto logCb = [](void*, int sev, const char* msg)
		{
			if (!msg) return;
			if (sev == 2)
			{
				error("%s", msg);
			}
			else if (sev == 1)
			{
				warn("%s", msg);
			}
		};

		return nzbget_rs_system_health_diagnose(&cfg, formatMode, nullptr, logCb);
	}

	const Options& m_options;
	const Servers& m_servers;
	const ::Feeds& m_feeds;
	const ::Scheduler::TaskList& m_tasks;
	const ::Log& m_log;
};
}  // namespace SystemHealth

extern SystemHealth::Service* g_SystemHealth;

#endif

#ifndef MAINTENANCE_H
#define MAINTENANCE_H

#include "PrePostProcessor.h"

extern void ExitProc();
extern int g_ArgumentCount;
extern char* (*g_Arguments)[];

class UpdateScriptController;

class UpdateInfoScriptController : public ScriptController
{
public:
	static void ExecuteScript(const char* script, CString& updateInfo)
	{
		detail("Executing update-info-script %s", FileSystem::BaseFileName(script));

		UpdateInfoScriptController scriptController;
		scriptController.SetArgs({script});

		const char* baseScript = FileSystem::BaseFileName(script);
		{
			NzbgetRsBuf infoBuf = nzbget_rs_dqh_maintenance_format_script_info_name(1, baseScript);
			scriptController.SetInfoName(infoBuf.data ? infoBuf.data : "");
			nzbget_rs_free(infoBuf);
		}

		size_t prefixLen = 0;
		{
			NzbgetRsBuf lpBuf = nzbget_rs_dqh_maintenance_strip_ext_and_calc_prefix_len(
				baseScript, 1023, &prefixLen);
			scriptController.SetLogPrefix(lpBuf.data ? lpBuf.data : "");
			nzbget_rs_free(lpBuf);
		}
		scriptController.m_prefixLen = (int)prefixLen;

		scriptController.Execute();

		if (!scriptController.m_updateInfo.Empty())
		{
			updateInfo = scriptController.m_updateInfo;
		}
	}

private:
	int m_prefixLen;
	StringBuilder m_updateInfo;

protected:
	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		text = text + m_prefixLen;

		int infoKind = nzbget_rs_feh_maintenance_classify_update_info_message(text);
		if (infoKind != 0)
		{
			debug("Command %s detected", text + 6);
			if (infoKind == 1)
			{
				m_updateInfo.Append(text + 6 + 12);
			}
			else
			{
				error("Invalid command \"%s\" received from %s", text, GetInfoName());
			}
		}
		else
		{
			ScriptController::AddMessage(kind, text);
		}
	}
};

class Maintenance
{
public:
	enum EBranch
	{
		brStable,
		brTesting,
		brDevel
	};

	~Maintenance();

	void AddMessage(Message::EKind kind, time_t time, const char* text)
	{
		if (time == 0)
		{
			time = Util::CurrentTime();
		}

		Guard guard(m_logMutex);
		m_messages.emplace_back(++m_idMessageGen, kind, time, text);
	}

	GuardedMessageList GuardMessages() { return GuardedMessageList(&m_messages, &m_logMutex); }
	bool StartUpdate(EBranch branch);

	void ResetUpdateController()
	{
		Guard guard(m_controllerMutex);
		m_updateScriptController = nullptr;
	}

	bool CheckUpdates(CString& updateInfo)
	{
		CString updateInfoScript;
		if (!ReadPackageInfoStr("update-info-script", updateInfoScript))
		{
			return false;
		}

		UpdateInfoScriptController::ExecuteScript(updateInfoScript, updateInfo);

		return nzbget_rs_dqh_maintenance_has_update_info(updateInfo.Length()) != 0;
	}

	static bool VerifySignature(const char* inFilename, const char* sigFilename, const char* pubKeyFilename)
	{
#ifndef DISABLE_TLS
		uint8_t inHash[32]{};
		uint8_t signature[256]{};
		NzbgetRsBuf pubKeyBuf{};
		if (!nzbget_rs_maintenance_prepare_signature_inputs(
			inFilename, sigFilename, pubKeyFilename, inHash, signature, &pubKeyBuf))
		{
			nzbget_rs_free(pubKeyBuf);
			return false;
		}
		BIO* mem = BIO_new_mem_buf(pubKeyBuf.data, static_cast<int>(pubKeyBuf.len));
		EVP_PKEY* pubKey = PEM_read_bio_PUBKEY(mem, nullptr, nullptr, nullptr);
		BIO_free(mem);
		nzbget_rs_free(pubKeyBuf);
		if (!pubKey)
		{
			return false;
		}
		EVP_PKEY_CTX* pctx = EVP_PKEY_CTX_new(pubKey, nullptr);
		if (!pctx)
		{
			EVP_PKEY_free(pubKey);
			return false;
		}
		bool ok = EVP_PKEY_verify_init(pctx) == 1 &&
			EVP_PKEY_CTX_set_rsa_padding(pctx, RSA_PKCS1_PADDING) > 0 &&
			EVP_PKEY_CTX_set_signature_md(pctx, EVP_sha256()) > 0 &&
			EVP_PKEY_verify(pctx, signature, sizeof(signature), inHash, sizeof(inHash)) == 1;
		EVP_PKEY_CTX_free(pctx);
		EVP_PKEY_free(pubKey);
		return ok;
#else
		(void)inFilename;
		(void)sigFilename;
		(void)pubKeyFilename;
		return false;
#endif
	}

private:
	MessageList m_messages;
	Mutex m_logMutex;
	Mutex m_controllerMutex;
	int m_idMessageGen = 0;
	UpdateScriptController* m_updateScriptController = nullptr;
	CString m_updateScript;

	bool ReadPackageInfoStr(const char* key, CString& value)
	{
		int errKind = 0;
		NzbgetRsBuf fnBuf{};
		NzbgetRsBuf valBuf = nzbget_rs_maintenance_read_package_info_str(
			g_Options->GetWebDir(), PATH_SEPARATOR, key, &errKind, &fnBuf);
		if (errKind != 0)
		{
			const char* fn = fnBuf.data ? fnBuf.data : "";
			if (errKind == 1)
			{
				error("Could not load file %s", fn);
			}
			else
			{
				error("Could not parse file %s", fn);
			}
			nzbget_rs_free(fnBuf);
			nzbget_rs_free(valBuf);
			return false;
		}
		value.Set(valBuf.data ? valBuf.data : "", static_cast<int>(valBuf.len));
		nzbget_rs_free(fnBuf);
		nzbget_rs_free(valBuf);
		return true;
	}
};

extern Maintenance* g_Maintenance;

class UpdateScriptController : public Thread, public ScriptController
{
public:
	virtual void Run()
	{
		UnregisterRunningScript();

		m_prefixLen = 0;
		PrintMessage(Message::mkInfo, "Executing update-script %s", GetScript());

		{
			NzbgetRsBuf infoBuf = nzbget_rs_dqh_maintenance_format_script_info_name(
				0, FileSystem::BaseFileName(GetScript()));
			SetInfoName(infoBuf.data ? infoBuf.data : "");
			nzbget_rs_free(infoBuf);
		}

		SetEnvVar("NZBUP_BRANCH", nzbget_rs_dqh_maintenance_branch_name((int)m_branch));

		SetEnvVar("NZBUP_RUNMODE", nzbget_rs_dqh_maintenance_run_mode_name(g_Options->GetDaemonMode() ? 1 : 0));

		for (int i = 0; i < g_ArgumentCount; i++)
		{
			NzbgetRsBuf envBuf = nzbget_rs_dqh_maintenance_format_cmdline_env_name(i);
			SetEnvVar(envBuf.data ? envBuf.data : "", (*g_Arguments)[i]);
			nzbget_rs_free(envBuf);
		}

		int pid = (int)getpid();

		{
			NzbgetRsBuf pidBuf = nzbget_rs_dqh_conn_format_port_str(pid);
			SetEnvVar("NZBUP_PROCESSID", pidBuf.data ? pidBuf.data : "");
			nzbget_rs_free(pidBuf);
		}

		const char* baseScript = FileSystem::BaseFileName(GetScript());
		size_t prefixLen = 0;
		{
			NzbgetRsBuf lpBuf = nzbget_rs_dqh_maintenance_strip_ext_and_calc_prefix_len(
				baseScript, 99, &prefixLen);
			SetLogPrefix(lpBuf.data ? lpBuf.data : "");
			nzbget_rs_free(lpBuf);
		}
		m_prefixLen = (int)prefixLen;

		Execute();

		g_Maintenance->ResetUpdateController();
	}

	void SetBranch(Maintenance::EBranch branch) { m_branch = branch; }

protected:
	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		text = text + m_prefixLen;

		int msgKind = nzbget_rs_feh_maintenance_classify_update_message(text);
		if (msgKind != 0)
		{
			debug("Command %s detected", text + 6);
			if (msgKind == 1)
			{
				Detach();
				ExitProc();
			}
			else
			{
				error("Invalid command \"%s\" received", text);
			}
		}
		else
		{
			g_Maintenance->AddMessage(kind, Util::CurrentTime(), text);
			ScriptController::AddMessage(kind, text);
		}
	}

private:
	Maintenance::EBranch m_branch;
	int m_prefixLen;
};

inline Maintenance::~Maintenance()
{
	bool waitScript = false;
	{
		Guard guard(m_controllerMutex);
		if (m_updateScriptController)
		{
			m_updateScriptController->Detach();
			waitScript = true;
		}
	}

	if (waitScript)
	{
		while (m_updateScriptController)
		{
			Util::Sleep(20);
		}
	}
}

inline bool Maintenance::StartUpdate(EBranch branch)
{
	Guard guard(m_controllerMutex);

	if (m_updateScriptController)
	{
		error("Could not start update-script: update-script is already running");
		return false;
	}

	if (!ReadPackageInfoStr("install-script", m_updateScript))
	{
		return false;
	}

	{
		NzbgetRsBuf scriptPathBuf = nzbget_rs_dqh_maintenance_resolve_script_path(
			g_Options->GetAppDir(), PATH_SEPARATOR, *m_updateScript);
		m_updateScript.Set(scriptPathBuf.data ? scriptPathBuf.data : "", (int)scriptPathBuf.len);
		nzbget_rs_free(scriptPathBuf);
	}

	{
		Guard logGuard(m_logMutex);
		m_messages.clear();
	}

	m_updateScriptController = new UpdateScriptController();
	m_updateScriptController->SetArgs({*m_updateScript});
	m_updateScriptController->SetBranch(branch);
	m_updateScriptController->SetAutoDestroy(true);

	m_updateScriptController->Start();

	return true;
}

#endif

#endif

#endif
