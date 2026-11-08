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
#include "NString.h"
#include "Log.h"
#include "Thread.h"
#include "WebDownloader.h"
#include "DownloadInfo.h"
#include "FeedInfo.h"
#include "Observer.h"
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

#include "Options.h"
#include "WorkState.h"
#include "FileSystem.h"
#include "ExtensionManager.h"
#include "DiskState.h"
#include "DupeCoordinator.h"
#include "UrlCoordinator.h"
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

#endif
