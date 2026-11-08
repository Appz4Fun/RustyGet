/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2012-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef WEBSERVER_H
#define WEBSERVER_H


inline constexpr int TOKEN_SIZE = 48 + 1;

class WebProcessor
{
public:
	enum EHttpMethod
	{
		hmPost,
		hmGet,
		hmOptions
	};

	static void Init();
	void Execute();
	void SetConnection(Connection* connection) { m_connection = connection; }
	void SetUrl(const char* url) { m_url = url; }
	void SetHttpMethod(EHttpMethod httpMethod) { m_httpMethod = httpMethod; }
	bool GetKeepAlive() { return m_keepAlive; }

private:
	enum EUserAccess
	{
		uaControl,
		uaRestricted,
		uaAdd
	};

	Connection* m_connection = nullptr;
	CString m_request;
	CString m_url;
	EHttpMethod m_httpMethod;
	EUserAccess m_userAccess;
	bool m_rpcRequest;
	bool m_authorized;
	bool m_gzip;
	CString m_origin;
	int m_contentLen;
	char m_authInfo[256+1];
	char m_authToken[TOKEN_SIZE];
	inline static char m_serverAuthToken[3][TOKEN_SIZE]{};
	CString m_forwardedFor;
	CString m_oldETag;
	bool m_keepAlive = false;
	std::hash<std::string_view> m_hasher;

	void Dispatch();
	void SendAuthResponse();
	void SendOptionsResponse();
	void SendErrorResponse(const char* errCode, bool printWarning);
	void SendSingleFileResponse();
	void SendMultiFileResponse();
	void SendBodyResponse(const char* body, int bodyLen, const char* contentType, bool cachable);
	void SendRedirectResponse(const char* url);
	const char* DetectContentType(const char* filename);
	void ParseHeaders();
	bool ParseUrl();
	bool CheckCredentials();
};

#include <climits>
#include "nzbget_rs.h"
#include "Util.h"

class XmlCommand;

class XmlRpcProcessor
{
public:
	enum ERpcProtocol
	{
		rpUndefined,
		rpXmlRpc,
		rpJsonRpc,
		rpJsonPRpc
	};

	enum EHttpMethod
	{
		hmPost,
		hmGet
	};

	enum EUserAccess
	{
		uaControl,
		uaRestricted,
		uaAdd
	};

	void Execute();
	void SetHttpMethod(EHttpMethod httpMethod) { m_httpMethod = httpMethod; }
	void SetUserAccess(EUserAccess userAccess) { m_userAccess = userAccess; }
	// whether JSON-P requests are answered (option JsonP); off unless set
	void SetJsonP(bool jsonP) { m_jsonP = jsonP; }
	void SetUrl(const char* url);
	void SetRequest(char* request) { m_request = request; }
	const char* GetResponse() { return m_response; }
	const char* GetContentType() { return m_contentType; }
	static bool IsRpcRequest(const char* url);
	bool IsSafeMethod() { return m_safeMethod; }

private:
	char* m_request = nullptr;
	const char* m_contentType = nullptr;
	ERpcProtocol m_protocol;
	EHttpMethod m_httpMethod;
	EUserAccess m_userAccess;
	CString m_url;
	StringBuilder m_response;
	bool m_safeMethod = false;
	bool m_jsonP = false;

	// what Dispatch does with a command: run it, or answer with an error, inside the
	// JSON-P callback or plain
	enum EDispatch
	{
		dpExecute,
		dpRefuseInCallback,
		dpRefuse
	};

	static EUserAccess Access(ERpcProtocol protocol, EUserAccess userAccess);
	static EDispatch Decide(ERpcProtocol protocol, EHttpMethod httpMethod, bool safeMethod, bool error,
		bool jsonP, int* errCode, const char** errText);
	void Dispatch();
	std::unique_ptr<XmlCommand> CreateCommand(const char* methodName);
	void MutliCall();
	void BuildResponse(const char* response, const char* callbackFunc, bool fault, const char* requestId);
	void BuildErrorResponse(int errCode, const char* errText);
	void BuildJsonPRefusal(XmlCommand* command, int errCode, const char* errText, const char* requestId);
};

class XmlCommand
{
public:
	XmlCommand();
	virtual ~XmlCommand();
	virtual void Execute() = 0;
	void PrepareParams();
	void SetRequest(char* request) { m_request = request; m_requestPtr = m_request; }
	void SetProtocol(XmlRpcProcessor::ERpcProtocol protocol) { m_protocol = protocol; }
	void SetHttpMethod(XmlRpcProcessor::EHttpMethod httpMethod) { m_httpMethod = httpMethod; }
	void SetUserAccess(XmlRpcProcessor::EUserAccess userAccess) { m_userAccess = userAccess; }
	const char* GetResponse() { return m_response; }
	const char* GetCallbackFunc() { return m_callbackFunc; }
	bool GetFault() { return m_fault; }
	virtual bool IsSafeMethod() { return false; };
	virtual bool IsError() { return false; };

protected:
	char* m_request = nullptr;
	char* m_requestPtr = nullptr;
	char* m_callbackFunc = nullptr;
	StringBuilder m_response;
	bool m_fault = false;
	XmlRpcProcessor::ERpcProtocol m_protocol = XmlRpcProcessor::rpUndefined;
	XmlRpcProcessor::EHttpMethod m_httpMethod;
	XmlRpcProcessor::EUserAccess m_userAccess;

	void BuildErrorResponse(int errCode, const char* errText, ...);
	void BuildBoolResponse(bool ok);
	void BuildIntResponse(int value);
	void AppendResponse(const char* part);
	void AppendFmtResponse(const char* format, ...);
	void AppendCondResponse(const char* part, bool cond);
	bool IsJson();
	bool NextParamAsInt(int* value);
	bool NextParamAsBool(bool* value);
	bool NextParamAsStr(char** valueBuf);
	CString EncodeStr(const char* str);
	void DecodeStr(char* str);
};

#include "nzbget_rs.h"
#include "QueueCoordinator.h"
#include "Util.h"
#include "Util.h"
#include "QueueCoordinator.h"
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
	int urlTimeout = g_Options->GetUrlTimeout();
	if (urlTimeout == 0)
	{
		return;
	}

	std::lock_guard<std::mutex> guard(m_downloadsMutex);
	time_t tm = Util::CurrentTime();

	for (FeedDownloader* feedDownloader: m_activeDownloads)
	{
		if (tm - feedDownloader->GetLastUpdateTime() > urlTimeout + 10 &&
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
#include "Util.h"
#include <cerrno>
#include <climits>
#include "Util.h"
#include "DiskState.h"
#include "QueueCoordinator.h"
namespace
{
	// Pauses downloads for the duration of a speed test and restores the previous
	// state. Tests at the same time share it: the first saves the state, the last
	// restores it (a second test restored "paused", saved while the first ran).
	class PauseGuard
	{
	public:
		PauseGuard()
		{
			std::lock_guard<std::mutex> lock(Mutex());
			if (Active()++ == 0)
			{
				WasPaused() = g_WorkState->GetPauseDownload();
				g_WorkState->SetPauseDownload(true);
			}
		}
		PauseGuard(const PauseGuard&) = delete;
		PauseGuard& operator=(const PauseGuard&) = delete;
		~PauseGuard()
		{
			std::lock_guard<std::mutex> lock(Mutex());
			if (--Active() == 0)
			{
				g_WorkState->SetPauseDownload(WasPaused());
			}
		}

	private:
		static std::mutex& Mutex() { static std::mutex mutex; return mutex; }
		static int& Active() { static int active = 0; return active; }
		static bool& WasPaused() { static bool wasPaused = false; return wasPaused; }
	};

	double MiBPerSec(uint64_t bytes, double ms)
	{
		return nzbget_rs_ntw_xmlrpc_mib_per_sec(bytes, ms);
	}

	void AppendRsBuf(StringBuilder& response, NzbgetRsBuf buf)
	{
		if (buf.data && buf.len > 0)
		{
			response.Append(buf.data, static_cast<int>(buf.len));
		}
		nzbget_rs_free(buf);
	}
}

extern void ExitProc();
extern void Reload();

class SafeXmlCommand : public XmlCommand
{
public:
	~SafeXmlCommand() override;
	bool IsSafeMethod() override { return true; }
};

class ErrorXmlCommand final : public XmlCommand
{
public:
	ErrorXmlCommand(int errCode, const char* errText) :
		m_errCode(errCode), m_errText(errText) {}
	void Execute() override;
	bool IsError() override { return true; }

private:
	int m_errCode;
	const char* m_errText;
};

class PauseUnpauseXmlCommand final : public XmlCommand
{
public:
	enum EPauseAction
	{
		paDownload,
		paPostProcess,
		paScan
	};

	PauseUnpauseXmlCommand(bool pause, EPauseAction pauseAction) :
		m_pause(pause), m_pauseAction(pauseAction) {}
	void Execute() override;

private:
	bool m_pause;
	EPauseAction m_pauseAction;
};

class ScheduleResumeXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ShutdownXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ReloadXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class VersionXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class DumpDebugXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class SetDownloadRateXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class StatusXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class SysInfoXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class SystemHealthXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class LogXmlCommand: public SafeXmlCommand
{
public:
	~LogXmlCommand() override;
	void Execute() override;

protected:
	int m_idFrom;
	int m_nrEntries;
	virtual GuardedMessageList GuardMessages();
};

class NzbInfoXmlCommand: public SafeXmlCommand
{
public:
	~NzbInfoXmlCommand() override;

protected:
	void AppendNzbInfoFields(NzbInfo* nzbInfo);
	void AppendPostInfoFields(PostInfo* postInfo, int logEntries, bool postQueue);
};

class ListFilesXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ListGroupsXmlCommand final : public NzbInfoXmlCommand
{
public:
	void Execute() override;
private:
	const char* DetectStatus(NzbInfo* nzbInfo);
};

class EditQueueXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class DownloadXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class AppendFleetXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class PostQueueXmlCommand final : public NzbInfoXmlCommand
{
public:
	void Execute() override;
};

class WriteLogXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ClearLogXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ScanXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class HistoryXmlCommand final : public NzbInfoXmlCommand
{
public:
	void Execute() override;
private:
	const char* DetectStatus(HistoryInfo* historyInfo);
};

class UrlQueueXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ConfigXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class LoadConfigXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class LoadExtensionsXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class DownloadExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class UpdateExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class DeleteExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class TestExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class SaveConfigXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ConfigTemplatesXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ViewFeedXmlCommand final : public XmlCommand
{
public:
	ViewFeedXmlCommand(bool preview) : m_preview(preview) {}
	void Execute() override;
private:
	bool m_preview;
};

class FetchFeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class EditServerXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ReadUrlXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class CheckUpdatesXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class StartUpdateXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class LogUpdateXmlCommand final : public LogXmlCommand
{
protected:
	GuardedMessageList GuardMessages() override;
};

class ServerVolumesXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ResetServerVolumeXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class LoadLogXmlCommand final : public LogXmlCommand
{
protected:
	void Execute() override;
	GuardedMessageList GuardMessages() override;
private:
	int m_nzbId;
	NzbInfo* m_nzbInfo;
	MessageList m_messages;
	std::unique_ptr<GuardedDownloadQueue> m_downloadQueue;
};

class TestServerXmlCommand final : public XmlCommand
{
public:
	void Execute() override;

	struct ServerParams
	{
		std::string host;
		std::string username;
		std::string password;
		std::string cipher;
		int port;
		int timeout;
		int certVerifLevel;
		bool encryption;
	};

private:
	CString m_errText;

	class TestConnection final : public NntpConnection
	{
	public:
		TestConnection(NewsServer* newsServer, TestServerXmlCommand* owner) :
			NntpConnection(newsServer), m_owner(owner) {
		}
	protected:
		TestServerXmlCommand* m_owner;
		void PrintError(const char* errMsg) override { m_owner->PrintError(errMsg); }
	};

	void PrintError(const char* errMsg);
};

class TlsCipherXmlCommand final : public XmlCommand
{
public:
	explicit TlsCipherXmlCommand(int operation) : m_operation(operation) {}
	void Execute() override
	{
		const NzbgetRsJsonLimits limits{0x7ffffffeu, 0x7fffffffu / 24u, 0x3fffffffu};
		int fault = 0;
		NzbgetRsBuf buf = nzbget_rs_xmlrpc_execute_tls_cipher(
			m_operation,
			(int)m_userAccess, (int)XmlRpcProcessor::uaControl,
			(int)m_protocol, (int)XmlRpcProcessor::rpJsonRpc,
			(int)m_httpMethod, (int)XmlRpcProcessor::hmPost,
			IsJson() ? 1 : 0,
			m_request,
			limits,
			nullptr,
			[](void*, int opKind, const char* jobId) -> NzbgetRsBuf
			{
				std::string resp = opKind == 0 ? TlsCipherBackend::Start() :
					opKind == 1 ? TlsCipherBackend::Status(jobId) :
					TlsCipherBackend::Cancel(jobId ? jobId : "");
				return nzbget_rs_alloc_buf(resp.data(), resp.size());
			},
			&fault);
		m_fault = fault != 0;
		AppendRsBuf(m_response, buf);
	}
private:
	int m_operation;
};

class TestServerSpeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class TestDiskSpeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class TestNetworkSpeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class StartScriptXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class LogScriptXmlCommand final : public LogXmlCommand
{
protected:
	virtual GuardedMessageList GuardMessages();
};


//*****************************************************************
// XmlRpcProcessor

inline void XmlRpcProcessor::SetUrl(const char* url)
{
	NzbgetRsBuf urlBuf = nzbget_rs_xmlrpc_normalize_url(url);
	m_url.Set(urlBuf.data ? urlBuf.data : "", (int)urlBuf.len);
	nzbget_rs_free(urlBuf);
}

inline bool XmlRpcProcessor::IsRpcRequest(const char* url)
{
	return nzbget_rs_rco_rpc_is_rpc_request(url) != 0;
}

inline void XmlRpcProcessor::Execute()
{
	m_protocol = (ERpcProtocol)nzbget_rs_rpc_protocol(m_url);
	if (m_protocol == rpUndefined)
	{
		error("internal error: invalid rpc-request: %s", *m_url);
		return;
	}

	Dispatch();
}

inline void XmlRpcProcessor::Dispatch()
{
	char* request = m_request;

	BString<100> methodName;
	// echoed as sent: cut to 99 characters it made the response invalid JSON (an id
	// past 4 KB is left out rather than cut)
	CString requestId;

	const char* params = nullptr;
	const char* requestIdPtr = nullptr;
	int requestIdLen = 0;
	nzbget_rs_rpc_route(m_url, m_request, m_httpMethod == hmGet, m_protocol, methodName, &params,
		&requestIdPtr, &requestIdLen);
	request = const_cast<char*>(params);
	if (requestIdPtr)
	{
		requestId.Set(requestIdPtr, requestIdLen);
	}

	debug("MethodName=%s", *methodName);

	m_userAccess = Access(m_protocol, m_userAccess);

	if (nzbget_rs_dqh_xmlrpc_is_multicall_post(
		methodName, m_protocol == rpXmlRpc ? 1 : 0, m_httpMethod == hmPost ? 1 : 0) != 0)
	{
		MutliCall();
	}
	else
	{
		std::unique_ptr<XmlCommand> command = CreateCommand(methodName);
		command->SetRequest(request);
		command->SetProtocol(m_protocol);
		command->SetHttpMethod(m_httpMethod);
		command->SetUserAccess(m_userAccess);
		command->PrepareParams();
		m_safeMethod = command->IsSafeMethod();
		int errCode = 0;
		const char* errText = nullptr;
		switch (Decide(m_protocol, m_httpMethod, m_safeMethod, command->IsError(), m_jsonP, &errCode, &errText))
		{
			case dpExecute:
				command->Execute();
				BuildResponse(command->GetResponse(), command->GetCallbackFunc(), command->GetFault(), requestId);
				break;

			case dpRefuseInCallback:
				BuildJsonPRefusal(command.get(), errCode, errText, requestId);
				break;

			case dpRefuse:
				BuildErrorResponse(errCode, errText);
				break;
		}
	}
}

inline XmlRpcProcessor::EUserAccess XmlRpcProcessor::Access(ERpcProtocol protocol, EUserAccess userAccess)
{
	// rust/src/rpcroute.rs
	return (EUserAccess)nzbget_rs_rpc_access(protocol, userAccess);
}

inline XmlRpcProcessor::EDispatch XmlRpcProcessor::Decide(ERpcProtocol protocol, EHttpMethod httpMethod,
	bool safeMethod, bool error, bool jsonP, int* errCode, const char** errText)
{
	// rust/src/rpcroute.rs
	return (EDispatch)nzbget_rs_rpc_decide(protocol, httpMethod, safeMethod, error, jsonP, errCode, errText);
}

inline void XmlRpcProcessor::MutliCall()
{
	bool error = false;
	StringBuilder response;

	response.Append("<array><data>");

	char* requestPtr = m_request;
	size_t structOff = 0;
	while (nzbget_rs_rol_xmlrpc_multicall_find_struct_end(requestPtr, &structOff))
	{
		char* callEnd = requestPtr + structOff;
		*callEnd = '\0';
		debug("MutliCall, request=%s", requestPtr);
		size_t nameOff = 0;
		size_t paramsOff = 0;
		if (!nzbget_rs_rol_xmlrpc_multicall_locate_parts(requestPtr, structOff, &nameOff, &paramsOff))
		{
			error = true;
			break;
		}
		char* nameEnd = requestPtr + nameOff;

		char methodName[100];
		methodName[0] = '\0';
		WebUtil::XmlParseTagValue(nameEnd, "string", methodName, sizeof(methodName), nullptr);
		debug("MutliCall, MethodName=%s", methodName);

		std::unique_ptr<XmlCommand> command = CreateCommand(methodName);
		// a call of the multicall runs as the caller, with the caller's access: it ran
		// with full control access (a restricted user read the passwords through it);
		// its parameters are its "params" member, not the start of the call
		command->SetProtocol(m_protocol);
		command->SetHttpMethod(m_httpMethod);
		command->SetUserAccess(m_userAccess);
		command->SetRequest(requestPtr + paramsOff);
		m_safeMethod |= command->IsSafeMethod();
		command->Execute();

		debug("MutliCall, Response=%s", command->GetResponse());

		bool wrapArray = nzbget_rs_roc_xmlrpc_multicall_needs_array_wrap(command->GetResponse()) != 0;
		if (wrapArray)
		{
			response.Append("<array><data>");
		}
		response.Append("<value>");
		response.Append(command->GetResponse());
		response.Append("</value>");
		if (wrapArray)
		{
			response.Append("</data></array>");
		}

		requestPtr = callEnd + 9; //strlen("</struct>")
	}

	if (error)
	{
		BuildErrorResponse(4, "Parse error");
	}
	else
	{
		response.Append("</data></array>");
		BuildResponse(response, "", false, nullptr);
	}
}

inline void XmlRpcProcessor::BuildResponse(const char* response, const char* callbackFunc,
	bool fault, const char* requestId)
{
	debug("Response=%s", response);

	bool xmlRpc = m_protocol == rpXmlRpc;

	NzbgetRsBuf head, tail;
	nzbget_rs_rpc_envelope(m_protocol, fault, callbackFunc, requestId, &head, &tail);
	m_response.Append(head.data);
	m_response.Append(response);
	m_response.Append(tail.data);
	nzbget_rs_free(head);
	nzbget_rs_free(tail);

	m_contentType = nzbget_rs_dqh_xmlrpc_response_content_type(xmlRpc ? 1 : 0);
}

inline void XmlRpcProcessor::BuildErrorResponse(int errCode, const char* errText)
{
	ErrorXmlCommand command(errCode, errText);
	command.SetProtocol(m_protocol);
	command.Execute();
	BuildResponse(command.GetResponse(), "", command.GetFault(), nullptr);
}

// a JSON-P refusal goes through the caller's callback, so the page that loaded it
// sees it
inline void XmlRpcProcessor::BuildJsonPRefusal(XmlCommand* command, int errCode, const char* errText,
	const char* requestId)
{
	ErrorXmlCommand refusal(errCode, errText);
	refusal.SetProtocol(m_protocol);
	refusal.Execute();
	BuildResponse(refusal.GetResponse(), command->GetCallbackFunc(), refusal.GetFault(), requestId);
}

inline std::unique_ptr<XmlCommand> XmlRpcProcessor::CreateCommand(const char* methodName)
{
	int accessDenied = nzbget_rs_ntw_xmlrpc_check_user_access_denied(
		(int)m_userAccess, (int)uaAdd, (int)uaRestricted, methodName);
	if (accessDenied == 1)
	{
		warn("Received request \"%s\" from add-user, access denied", methodName);
		return std::make_unique<ErrorXmlCommand>(401, "Access denied");
	}
	if (accessDenied == 2)
	{
		warn("Received request \"%s\" from restricted user, access denied", methodName);
		return std::make_unique<ErrorXmlCommand>(401, "Access denied");
	}

	switch (nzbget_rs_xmlrpc_match_command(methodName))
	{
		case 1: return std::make_unique<PauseUnpauseXmlCommand>(true, PauseUnpauseXmlCommand::paDownload);
		case 2: return std::make_unique<PauseUnpauseXmlCommand>(false, PauseUnpauseXmlCommand::paDownload);
		case 3: return std::make_unique<ShutdownXmlCommand>();
		case 4: return std::make_unique<ReloadXmlCommand>();
		case 5: return std::make_unique<VersionXmlCommand>();
		case 6: return std::make_unique<DumpDebugXmlCommand>();
		case 7: return std::make_unique<SetDownloadRateXmlCommand>();
		case 8: return std::make_unique<StatusXmlCommand>();
		case 9: return std::make_unique<SysInfoXmlCommand>();
		case 10: return std::make_unique<SystemHealthXmlCommand>();
		case 11: return std::make_unique<LogXmlCommand>();
		case 12: return std::make_unique<ListFilesXmlCommand>();
		case 13: return std::make_unique<ListGroupsXmlCommand>();
		case 14: return std::make_unique<EditQueueXmlCommand>();
		case 15: return std::make_unique<DownloadXmlCommand>();
		case 16: return std::make_unique<AppendFleetXmlCommand>();
		case 17: return std::make_unique<PostQueueXmlCommand>();
		case 18: return std::make_unique<WriteLogXmlCommand>();
		case 19: return std::make_unique<ClearLogXmlCommand>();
		case 20: return std::make_unique<LoadLogXmlCommand>();
		case 21: return std::make_unique<ScanXmlCommand>();
		case 22: return std::make_unique<PauseUnpauseXmlCommand>(true, PauseUnpauseXmlCommand::paPostProcess);
		case 23: return std::make_unique<PauseUnpauseXmlCommand>(false, PauseUnpauseXmlCommand::paPostProcess);
		case 24: return std::make_unique<PauseUnpauseXmlCommand>(true, PauseUnpauseXmlCommand::paScan);
		case 25: return std::make_unique<PauseUnpauseXmlCommand>(false, PauseUnpauseXmlCommand::paScan);
		case 26: return std::make_unique<ScheduleResumeXmlCommand>();
		case 27: return std::make_unique<HistoryXmlCommand>();
		case 28: return std::make_unique<UrlQueueXmlCommand>();
		case 29: return std::make_unique<ConfigXmlCommand>();
		case 30: return std::make_unique<LoadConfigXmlCommand>();
		case 31: return std::make_unique<LoadExtensionsXmlCommand>();
		case 32: return std::make_unique<DownloadExtensionXmlCommand>();
		case 33: return std::make_unique<UpdateExtensionXmlCommand>();
		case 34: return std::make_unique<DeleteExtensionXmlCommand>();
		case 35: return std::make_unique<TestExtensionXmlCommand>();
		case 36: return std::make_unique<SaveConfigXmlCommand>();
		case 37: return std::make_unique<ConfigTemplatesXmlCommand>();
		case 38: return std::make_unique<ViewFeedXmlCommand>(false);
		case 39: return std::make_unique<ViewFeedXmlCommand>(true);
		case 40: return std::make_unique<FetchFeedXmlCommand>();
		case 41: return std::make_unique<EditServerXmlCommand>();
		case 42: return std::make_unique<ReadUrlXmlCommand>();
		case 43: return std::make_unique<CheckUpdatesXmlCommand>();
		case 44: return std::make_unique<StartUpdateXmlCommand>();
		case 45: return std::make_unique<LogUpdateXmlCommand>();
		case 46: return std::make_unique<ServerVolumesXmlCommand>();
		case 47: return std::make_unique<ResetServerVolumeXmlCommand>();
		case 48: return std::make_unique<TlsCipherXmlCommand>(0);
		case 49: return std::make_unique<TlsCipherXmlCommand>(1);
		case 50: return std::make_unique<TlsCipherXmlCommand>(2);
		case 51: return std::make_unique<TestServerXmlCommand>();
		case 52: return std::make_unique<TestServerSpeedXmlCommand>();
		case 53: return std::make_unique<TestDiskSpeedXmlCommand>();
		case 54: return std::make_unique<TestNetworkSpeedXmlCommand>();
		case 55: return std::make_unique<StartScriptXmlCommand>();
		case 56: return std::make_unique<LogScriptXmlCommand>();
		default: return std::make_unique<ErrorXmlCommand>(1, "Invalid procedure");
	}
}


//*****************************************************************
// Base command

inline XmlCommand::XmlCommand()
	: m_protocol(XmlRpcProcessor::rpUndefined)
	, m_httpMethod(XmlRpcProcessor::hmPost)
	, m_userAccess(XmlRpcProcessor::uaControl)
{
	m_response.Reserve(1024 * 10 - 1);
}

inline XmlCommand::~XmlCommand() = default;
inline SafeXmlCommand::~SafeXmlCommand() = default;
inline LogXmlCommand::~LogXmlCommand() = default;
inline NzbInfoXmlCommand::~NzbInfoXmlCommand() = default;

inline bool XmlCommand::IsJson()
{
	return nzbget_rs_dqh_xmlrpc_is_json_protocol(
		(int)m_protocol,
		(int)XmlRpcProcessor::rpJsonRpc,
		(int)XmlRpcProcessor::rpJsonPRpc) != 0;
}

inline void XmlCommand::AppendResponse(const char* part)
{
	m_response.Append(part);
}

inline void XmlCommand::AppendFmtResponse(const char* format, ...)
{
	va_list args;
	va_start(args, format);
	m_response.AppendFmtV(format, args);
	va_end(args);
}

inline void XmlCommand::AppendCondResponse(const char* part, bool cond)
{
	if (cond)
	{
		m_response.Append(part);
	}
}

inline void XmlCommand::BuildErrorResponse(int errCode, const char* errText, ...)
{
	char fullText[1024];

	va_list ap;
	va_start(ap, errText);
	vsnprintf(fullText, 1024, errText, ap);
	fullText[1024-1] = '\0';
	va_end(ap);

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_error(IsJson() ? 1 : 0, errCode, fullText));

	m_fault = true;
}

inline void XmlCommand::BuildBoolResponse(bool ok)
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_bool(IsJson() ? 1 : 0, ok ? 1 : 0));
}

inline void XmlCommand::BuildIntResponse(int value)
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_int(IsJson() ? 1 : 0, value));
}

inline void XmlCommand::PrepareParams()
{
	if (nzbget_rs_dqh_xmlrpc_should_skip_to_json_post_params(
		IsJson() ? 1 : 0, (int)m_httpMethod, (int)XmlRpcProcessor::hmPost) != 0)
	{
		// without "params" the request is emptied: no parameters follow
		m_requestPtr = nzbget_rs_rpc_skip_to_params(m_requestPtr);
	}

	if (m_protocol == XmlRpcProcessor::rpJsonPRpc)
	{
		NextParamAsStr(&m_callbackFunc);
	}
}

inline bool XmlCommand::NextParamAsInt(int* value)
{
	return nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 0, value, nullptr);
}

inline bool XmlCommand::NextParamAsBool(bool* value)
{
	int intValue;
	if (!nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 1, &intValue, nullptr))
	{
		return false;
	}
	*value = intValue != 0;
	return true;
}

inline bool XmlCommand::NextParamAsStr(char** value)
{
	return nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 2, nullptr, value);
}

inline CString XmlCommand::EncodeStr(const char* str)
{
	if (!str)
	{
		return "";
	}

	CString result;
	if (IsJson())
	{
		result = WebUtil::JsonEncode(str);
	}
	else
	{
		result = WebUtil::XmlEncode(str);
	}
	return result;
}

inline void XmlCommand::DecodeStr(char* str)
{
	if (IsJson())
	{
		WebUtil::JsonDecode(str);
	}
	else
	{
		WebUtil::XmlDecode(str);
	}
}

//*****************************************************************
// Commands

inline void ErrorXmlCommand::Execute()
{
	BuildErrorResponse(m_errCode, m_errText);
}

inline void PauseUnpauseXmlCommand::Execute()
{
	bool ok = true;

	g_WorkState->SetResumeTime(0);

	switch (m_pauseAction)
	{
		case paDownload:
			g_WorkState->SetPauseDownload(m_pause);
			break;

		case paPostProcess:
			g_WorkState->SetPausePostProcess(m_pause);
			break;

		case paScan:
			g_WorkState->SetPauseScan(m_pause);
			break;

		default:
			ok = false;
	}

	BuildBoolResponse(ok);
}

// bool scheduleresume(int Seconds)
inline void ScheduleResumeXmlCommand::Execute()
{
	int seconds = 0;
	bool hasParam = NextParamAsInt(&seconds);
	int64_t resumeTime = 0;
	if (!nzbget_rs_dqh_xmlrpc_calc_schedule_resume_time(
		hasParam ? 1 : 0, seconds, (int64_t)Util::CurrentTime(), &resumeTime))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	g_WorkState->SetResumeTime((time_t)resumeTime);

	BuildBoolResponse(true);
}

inline void ShutdownXmlCommand::Execute()
{
	BuildBoolResponse(true);
	ExitProc();
}

inline void ReloadXmlCommand::Execute()
{
	BuildBoolResponse(true);
	Reload();
}

inline void VersionXmlCommand::Execute()
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(IsJson() ? 1 : 0, Util::VersionRevision(), 0, 1023));
}

inline void DumpDebugXmlCommand::Execute()
{
	g_Log->LogDebugInfo();
	BuildBoolResponse(true);
}

inline void SetDownloadRateXmlCommand::Execute()
{
	int rate = 0;
	// in KB/s: a rate past INT_MAX / 1024 wrapped around to a tiny limit
	int bytesPerSec = 0;
	if (!NextParamAsInt(&rate) ||
		!nzbget_rs_rol_xmlrpc_validate_download_rate(rate, &bytesPerSec))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	g_WorkState->SetSpeedLimit(bytesPerSec);
	BuildBoolResponse(true);
}

inline void StatusXmlCommand::Execute()
{
	int postJobCount = 0;
	int urlCount = 0;
	int64 remainingSize, forcedSize;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbget_rs_dqh_xmlrpc_accum_queue_job_counts(
				nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
				(int)nzbInfo->GetKind(),
				(int)NzbInfo::nkUrl,
				&postJobCount,
				&urlCount);
		}
		downloadQueue->CalcRemainingSize(&remainingSize, &forcedSize);
	}

	int64 articleCache = g_ArticleCache->GetAllocated();
	const char* memoryPressure = g_MemoryMonitor ? g_MemoryMonitor->GetTierName() : "NORMAL";
	int effectiveArticleCacheMB = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveArticleCacheMB() : g_Options->GetArticleCache();
	int effectiveParBufferMB = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveParBufferMB(g_Options->GetParBuffer()) : g_Options->GetParBuffer();

	int upTimeSec, downloadTimeSec;
	int64 allBytes;
	bool serverStandBy;
	g_StatMeter->CalcTotalStat(&upTimeSec, &downloadTimeSec, &allBytes, &serverStandBy);

	int64 monthBytes, dayBytes;
	g_StatMeter->CalcQuotaUsage(monthBytes, dayBytes);

	int64 freeDiskSpace = 0;
	int64 totalDiskSpace = 0;
	int64 freeInterDiskSpace = 0;
	int64 totalInterDiskSpace = 0;

	if (!Util::EmptyStr(g_Options->GetDestDir()))
	{
		auto res = FileSystem::GetDiskState(g_Options->GetDestDir());
		if (res.has_value())
		{
			const auto& value = res.value();
			freeDiskSpace = value.available;
			totalDiskSpace = value.total;
		}
	}

	bool interDirEmpty = Util::EmptyStr(g_Options->GetInterDir());
	if (!interDirEmpty)
	{
		auto res = FileSystem::GetDiskState(g_Options->GetInterDir());
		if (res.has_value())
		{
			const auto& value = res.value();
			freeInterDiskSpace = value.available;
			totalInterDiskSpace = value.total;
		}
	}

	NzbgetRsRpcStatusInput statusFields{};
	statusFields.remaining_size = remainingSize;
	statusFields.forced_size = forcedSize;
	statusFields.article_cache = articleCache;
	statusFields.all_bytes = allBytes;
	statusFields.month_bytes = monthBytes;
	statusFields.day_bytes = dayBytes;
	statusFields.memory_pressure = memoryPressure;
	statusFields.effective_article_cache_mb = effectiveArticleCacheMB;
	statusFields.effective_par_buffer_mb = effectiveParBufferMB;
	statusFields.download_rate = g_StatMeter->CalcCurrentDownloadSpeed();
	statusFields.download_limit = (int)(g_WorkState->GetSpeedLimit());
	statusFields.thread_count = Thread::GetThreadCount() - 1; // not counting itself
	statusFields.post_job_count = postJobCount;
	statusFields.url_count = urlCount;
	statusFields.up_time_sec = upTimeSec;
	statusFields.download_time_sec = downloadTimeSec;
	statusFields.download_paused = g_WorkState->GetPauseDownload() ? 1 : 0;
	statusFields.server_stand_by = serverStandBy ? 1 : 0;
	statusFields.post_paused = g_WorkState->GetPausePostProcess() ? 1 : 0;
	statusFields.scan_paused = g_WorkState->GetPauseScan() ? 1 : 0;
	statusFields.quota_reached = g_WorkState->GetQuotaReached() ? 1 : 0;
	statusFields.free_disk_space = freeDiskSpace;
	statusFields.total_disk_space = totalDiskSpace;
	statusFields.inter_dir_empty = interDirEmpty ? 1 : 0;
	statusFields.free_inter_disk_space = freeInterDiskSpace;
	statusFields.total_inter_disk_space = totalInterDiskSpace;
	statusFields.server_time = (int)Util::CurrentTime();
	statusFields.resume_time = (int)g_WorkState->GetResumeTime();
	statusFields.feed_active = g_FeedCoordinator->HasActiveDownloads() ? 1 : 0;
	statusFields.queued_scripts = g_QueueScriptCoordinator->GetQueueSize();

	std::vector<NzbgetRsRpcServerStatus> servers;
	servers.reserve(g_ServerPool->GetServers()->size());
	for (NewsServer* server : g_ServerPool->GetServers())
	{
		servers.push_back(NzbgetRsRpcServerStatus{ server->GetId(), server->GetActive() ? 1 : 0 });
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_status(
		IsJson() ? 1 : 0, &statusFields, servers.data(), servers.size()));
}

inline void SysInfoXmlCommand::Execute()
{
	std::string response = IsJson()
		? System::ToJsonStr(*g_SystemInfo)
		: System::ToXmlStr(*g_SystemInfo);

	AppendResponse(response.c_str());
}

inline void SystemHealthXmlCommand::Execute()
{
	if (g_Options->GetSystemHealthCheck())
	{
		AppendRsBuf(m_response, g_SystemHealth->DiagnoseSerialized(IsJson()));
	}
	else
	{
		BuildErrorResponse(1, "System health check is disabled in configuration");
	}
}

// struct[] log(idfrom, entries)
inline void LogXmlCommand::Execute()
{
	m_idFrom = 0;
	m_nrEntries = 0;
	if (!NextParamAsInt(&m_idFrom) || !NextParamAsInt(&m_nrEntries) ||
		!nzbget_rs_roc_xmlrpc_log_params_valid(m_idFrom, m_nrEntries))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	debug("iIDFrom=%i", m_idFrom);
	debug("iNrEntries=%i", m_nrEntries);

	GuardedMessageList messages = GuardMessages();

	int start = messages->size();
	nzbget_rs_rco_rpc_log_slice_range(
		(int)messages->size(),
		messages->empty() ? 0 : (int)messages->front().GetId(),
		m_idFrom, m_nrEntries, &start, &m_nrEntries);

	std::vector<NzbgetRsRpcLogEntry> entries;
	if ((uint32)start < messages->size())
	{
		entries.reserve(messages->size() - (uint32)start);
		for (uint32 i = (uint32)start; i < messages->size(); i++)
		{
			Message& message = messages->at(i);
			entries.push_back(NzbgetRsRpcLogEntry{
				(int)message.GetId(),
				(int)message.GetKind(),
				(int)message.GetTime(),
				message.GetText()
			});
		}
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_log_list(
		IsJson() ? 1 : 0, entries.data(), entries.size()));
}

inline GuardedMessageList LogXmlCommand::GuardMessages()
{
	return g_Log->GuardMessages();
}

// struct[] listfiles(int IDFrom, int IDTo, int NZBID)
// For backward compatibility with 0.8 parameter "NZBID" is optional
inline void ListFilesXmlCommand::Execute()
{
	int idStart = 0;
	int idEnd = 0;
	bool hasIdStart = NextParamAsInt(&idStart);
	bool hasIdEnd = hasIdStart ? NextParamAsInt(&idEnd) : false;
	if (!nzbget_rs_roc_xmlrpc_list_files_params_valid(hasIdStart ? 1 : 0, hasIdEnd ? 1 : 0, idStart, idEnd, 0))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	// For backward compatibility with 0.8 parameter "NZBID" is optional (error checking omitted)
	int nzbId = 0;
	NextParamAsInt(&nzbId);

	if (!nzbget_rs_roc_xmlrpc_list_files_params_valid(hasIdStart ? 1 : 0, hasIdEnd ? 1 : 0, idStart, idEnd, nzbId))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	debug("iIDStart=%i", idStart);
	debug("iIDEnd=%i", idEnd);

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			int progress = 0;
			if (nzbget_rs_rco_rpc_list_files_match_and_progress(
				nzbId, fileInfo->GetNzbInfo()->GetId(), fileInfo->GetId(),
				idStart, idEnd, fileInfo->GetSize(), fileInfo->GetRemainingSize(),
				fileInfo->GetMissedSize(), fileInfo->GetFailedSize(),
				fileInfo->GetSuccessSize(), &progress))
			{
				NzbgetRsRpcFileItem item{};
				item.id = fileInfo->GetId();
				item.file_size = fileInfo->GetSize();
				item.remaining_size = fileInfo->GetRemainingSize();
				item.post_time = (int)fileInfo->GetTime();
				item.filename_confirmed = fileInfo->GetFilenameConfirmed() ? 1 : 0;
				item.paused = fileInfo->GetPaused() ? 1 : 0;
				item.nzb_id = fileInfo->GetNzbInfo()->GetId();
				item.nzb_name = fileInfo->GetNzbInfo()->GetName();
				item.nzb_filename = fileInfo->GetNzbInfo()->GetFilename();
				item.subject = fileInfo->GetSubject();
				item.filename = fileInfo->GetFilename();
				item.dest_dir = fileInfo->GetNzbInfo()->GetDestDir();
				item.category = fileInfo->GetNzbInfo()->GetCategory();
				item.priority = fileInfo->GetNzbInfo()->GetPriority();
				item.active_downloads = fileInfo->GetActiveDownloads();
				item.progress = progress;

				AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_file_item(
					IsJson() ? 1 : 0, index++ > 0 ? 1 : 0, &item));
			}
		}
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline void NzbInfoXmlCommand::AppendNzbInfoFields(NzbInfo* nzbInfo)
{
	time_t postStartTime = nzbInfo->GetPostInfo() ? nzbInfo->GetPostInfo()->GetStartTime() : 0;
	time_t curTime = postStartTime ? Util::CurrentTime() : 0;

	NzbgetRsRpcNzbInfoInput fields{};
	fields.id = nzbInfo->GetId();
	fields.name = nzbInfo->GetName();
	fields.kind = (int)nzbInfo->GetKind();
	fields.url = nzbInfo->GetUrl();
	fields.filename = nzbInfo->GetFilename();
	fields.dest_dir = nzbInfo->GetDestDir();
	fields.final_dir = nzbInfo->GetFinalDir();
	fields.category = nzbInfo->GetCategory();
	fields.par_status = (int)nzbInfo->GetParStatus();
	fields.extra_par_blocks = nzbInfo->GetExtraParBlocks();
	fields.unpack_status = (int)nzbInfo->GetUnpackStatus();
	fields.move_status = (int)nzbInfo->GetMoveStatus();
	fields.script_status = (int)nzbInfo->GetScriptStatuses()->CalcTotalStatus();
	fields.delete_status = (int)nzbInfo->GetDeleteStatus();
	fields.mark_status = (int)nzbInfo->GetMarkStatus();
	fields.url_status = (int)nzbInfo->GetUrlStatus();
	fields.size = nzbInfo->GetSize();
	fields.file_count = nzbInfo->GetFileCount();
	fields.min_time = (int)nzbInfo->GetMinTime();
	fields.max_time = (int)nzbInfo->GetMaxTime();
	fields.total_articles = nzbInfo->GetTotalArticles();
	fields.current_success_articles = nzbInfo->GetCurrentSuccessArticles();
	fields.current_failed_articles = nzbInfo->GetCurrentFailedArticles();
	fields.dupe_recovered_articles = nzbInfo->GetDupeRecoveredArticles();
	fields.dupe_recovered_bytes = nzbInfo->GetDupeRecoveredBytes();
	fields.dupe_recovered_holes = nzbInfo->GetDupeRecoveredHoles();
	fields.live_repairing = nzbInfo->GetLiveRepairThread() != nullptr ? 1 : 0;
	fields.health = nzbInfo->CalcHealth();
	fields.critical_health = nzbInfo->CalcCriticalHealth(false);
	fields.dupe_key = nzbInfo->GetDupeKey();
	fields.dupe_score = nzbInfo->GetDupeScore();
	fields.dupe_mode = (int)nzbInfo->GetDupeMode();
	fields.downloaded_size = nzbInfo->GetDownloadedSize();
	fields.download_sec = nzbInfo->GetDownloadSec();
	fields.post_total_sec = nzbInfo->GetPostTotalSec();
	fields.has_post_start_time = postStartTime != 0 ? 1 : 0;
	fields.cur_time = (int64)curTime;
	fields.post_start_time = (int64)postStartTime;
	fields.par_sec = nzbInfo->GetParSec();
	fields.repair_sec = nzbInfo->GetRepairSec();
	fields.unpack_sec = nzbInfo->GetUnpackSec();
	fields.message_count = nzbInfo->GetMessageCount();
	fields.cached_message_count = nzbInfo->GetCachedMessageCount();

	std::vector<NzbgetRsRpcParamItem> params;
	params.reserve(nzbInfo->GetParameters()->size());
	for (NzbParameter& parameter : nzbInfo->GetParameters())
	{
		params.push_back(NzbgetRsRpcParamItem{ parameter.GetName(), parameter.GetValue() });
	}

	std::vector<NzbgetRsRpcScriptItem> scripts;
	scripts.reserve(nzbInfo->GetScriptStatuses()->size());
	for (ScriptStatus& scriptStatus : nzbInfo->GetScriptStatuses())
	{
		scripts.push_back(NzbgetRsRpcScriptItem{ scriptStatus.GetName(), (int)scriptStatus.GetStatus() });
	}

	std::vector<NzbgetRsRpcServerStatItem> stats;
	stats.reserve(nzbInfo->GetCurrentServerStats()->size());
	for (ServerStat& serverStat : nzbInfo->GetCurrentServerStats())
	{
		stats.push_back(NzbgetRsRpcServerStatItem{
			serverStat.GetServerId(),
			serverStat.GetSuccessArticles(),
			serverStat.GetFailedArticles()
		});
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_nzb_info_fields(
		IsJson() ? 1 : 0, &fields,
		params.data(), params.size(),
		scripts.data(), scripts.size(),
		stats.data(), stats.size()));
}

inline void NzbInfoXmlCommand::AppendPostInfoFields(PostInfo* postInfo, int logEntries, bool postQueue)
{
	time_t curTime = postInfo ? Util::CurrentTime() : 0;

	std::optional<GuardedMessageList> messages;
	std::vector<NzbgetRsRpcLogEntry> entries;
	if (logEntries > 0 && postInfo)
	{
		messages.emplace(postInfo->GetNzbInfo()->GuardCachedMessages());
		int start = 0;
		if (nzbget_rs_rol_xmlrpc_post_info_log_slice(
			logEntries, (int)(*messages)->size(), &start, &logEntries))
		{
			entries.reserve((*messages)->size() - (uint32)start);
			for (uint32 i = (uint32)start; i < (*messages)->size(); i++)
			{
				Message& message = (*messages)->at(i);
				entries.push_back(NzbgetRsRpcLogEntry{
					(int)message.GetId(),
					(int)message.GetKind(),
					(int)message.GetTime(),
					message.GetText()
				});
			}
		}
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_post_info_fields(
		IsJson() ? 1 : 0,
		postQueue ? 1 : 0,
		postInfo ? 1 : 0,
		postInfo ? postInfo->GetProgressLabel() : nullptr,
		postInfo ? postInfo->GetStageProgress() : 0,
		(int64)curTime,
		postInfo ? (int64)postInfo->GetStageTime() : 0,
		postInfo ? (int64)postInfo->GetStartTime() : 0,
		entries.data(),
		entries.size()));
}

// struct[] listgroups(int NumberOfLogEntries)
inline void ListGroupsXmlCommand::Execute()
{
	int nrEntries = 0;
	NextParamAsInt(&nrEntries);

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		const char* status = DetectStatus(nzbInfo);

		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_group_item_start(
			IsJson() ? 1 : 0,
			index++ > 0 ? 1 : 0,
			nzbInfo->GetId(),
			nzbInfo->GetRemainingSize(),
			nzbInfo->GetPausedSize(),
			(int)nzbInfo->GetFileList()->size(),
			nzbInfo->GetRemainingParCount(),
			nzbInfo->GetPriority(),
			nzbInfo->GetActiveDownloads(),
			status));

		AppendNzbInfoFields(nzbInfo);
		AppendCondResponse(",\n", IsJson());
		AppendPostInfoFields(nzbInfo->GetPostInfo(), nrEntries, false);

		AppendResponse(IsJson() ? "}" : "</struct></value>\n");
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline const char* ListGroupsXmlCommand::DetectStatus(NzbInfo* nzbInfo)
{
	bool hasPostInfo = nzbInfo->GetPostInfo() != nullptr;
	int postStage = hasPostInfo ? (int)nzbInfo->GetPostInfo()->GetStage() : 0;
	bool queueScriptActive = false;
	bool hasQueueScriptJob = nzbget_rs_dqh_xmlrpc_should_check_queue_script_job(
		hasPostInfo ? 1 : 0, postStage, (int)PostInfo::ptQueued) != 0 &&
		g_QueueScriptCoordinator->HasJob(nzbInfo->GetId(), &queueScriptActive);
	return nzbget_rs_rco_rpc_detect_group_status(
		hasPostInfo ? 1 : 0,
		postStage,
		hasQueueScriptJob ? 1 : 0,
		queueScriptActive ? 1 : 0,
		(int)nzbInfo->GetDirectUnpackStatus(),
		nzbInfo->GetActiveDownloads(),
		(int)nzbInfo->GetKind(),
		nzbInfo->GetPausedSize(),
		nzbInfo->GetRemainingSize());
}

// v18:
//    bool editqueue(string Command, string Args, int[] IDs)
// v17:
//    bool editqueue(string Command, int Offset, string Args, int[] IDs)
inline void EditQueueXmlCommand::Execute()
{
	int action = -1;
	NzbgetRsBuf argsOrErr{nullptr, 0};
	int errCode = 0;
	IdList idList;
	int okParse = nzbget_rs_xmlrpc_parse_edit_queue(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		(int)DownloadQueue::eaFileMoveOffset,
		(int)DownloadQueue::eaGroupMoveOffset,
		&action,
		&argsOrErr,
		&errCode,
		&idList,
		[](void* ctx, int id)
		{
			static_cast<IdList*>(ctx)->push_back(id);
		});
	if (!okParse)
	{
		BuildErrorResponse(errCode, "%s", argsOrErr.data ? argsOrErr.data : "Invalid parameter");
		nzbget_rs_free(argsOrErr);
		return;
	}

	bool ok = DownloadQueue::Guard()->EditList(&idList, nullptr, DownloadQueue::mmId,
		(DownloadQueue::EEditAction)action, argsOrErr.data ? argsOrErr.data : "");
	nzbget_rs_free(argsOrErr);

	BuildBoolResponse(ok);
}
// v26.0: Added archived NZBs support.
//   bool append(string filename, string Category, int Priority, bool AddToTop, string Content, bool AddPaused, string DupeKey, int DupeScore, string DupeMode, bool autoCategory)
// v25.2: Added new parameter "autoCategory".
//   bool append(string NZBFilename, string Category, int Priority, bool AddToTop, string Content, bool AddPaused, string DupeKey, int DupeScore, string DupeMode, bool autoCategory)
// v16:
//   int append(string NZBFilename, string NZBContent, string Category, int Priority, bool AddToTop, bool AddPaused, string DupeKey, int DupeScore, string DupeMode, struct[] Parameters)
// v13 (new param order and new result type):
//   int append(string NZBFilename, string NZBContent, string Category, int Priority, bool AddToTop, bool AddPaused, string DupeKey, int DupeScore, string DupeMode)
// v12 (backward compatible, some params are optional):
//   bool append(string NZBFilename, string Category, int Priority, bool AddToTop, string Content, bool AddPaused, string DupeKey, int DupeScore, string DupeMode)
// appendfleet(DupeKey, Category, Priority, Timeout, Name1, Content1, Name2, Content2, ...):
// a content is the nzb-file in base64 or its url; see Fleet
inline void AppendFleetXmlCommand::Execute()
{
	if (!IsJson())
	{
		BuildErrorResponse(2, "appendfleet needs JSON-RPC");
		return;
	}
	Fleet::Request request;
	char* dupeKey = nullptr;
	char* category = nullptr;
	// an empty key is derived from the first member's name (Fleet)
	if (!NextParamAsStr(&dupeKey))
	{
		BuildErrorResponse(2, "Invalid parameter (DupeKey)");
		return;
	}
	if (!NextParamAsStr(&category))
	{
		BuildErrorResponse(2, "Invalid parameter (Category)");
		return;
	}
	if (!NextParamAsInt(&request.priority))
	{
		BuildErrorResponse(2, "Invalid parameter (Priority)");
		return;
	}
	if (!NextParamAsInt(&request.timeoutSec))
	{
		BuildErrorResponse(2, "Invalid parameter (Timeout)");
		return;
	}
	DecodeStr(dupeKey);
	DecodeStr(category);
	request.dupeKey = dupeKey;
	request.category = category;
	char* name = nullptr;
	while (NextParamAsStr(&name))
	{
		char* content = nullptr;
		if (!NextParamAsStr(&content))
		{
			BuildErrorResponse(2, "Invalid parameter (Content of %s)", name);
			return;
		}
		if (nzbget_rs_dqh_xmlrpc_is_fleet_member_limit_reached(
			request.members.size(), Fleet::MaxMembers) != 0)
		{
			BuildErrorResponse(2, "Too many members (at most %i)", Fleet::MaxMembers);
			return;
		}
		DecodeStr(name);
		DecodeStr(content);
		Fleet::Member member;
		member.name = name;
		if (nzbget_rs_ntw_xmlrpc_is_http_url(content) != 0)
		{
			member.url = content;
		}
		else
		{
			int len = WebUtil::DecodeBase64(content, 0, content);
			member.data.assign(content, len);
		}
		request.members.push_back(std::move(member));
	}

	Fleet::Result result = Fleet::Append(std::move(request));

	std::vector<NzbgetRsRpcFleetMember> entries;
	entries.reserve(result.members.size());
	for (const Fleet::Entry& entry : result.members)
	{
		entries.push_back(NzbgetRsRpcFleetMember{
			entry.nzbId,
			entry.name.c_str(),
			entry.rank,
			entry.alive,
			entry.status.c_str(),
			entry.sameAs,
			entry.sameAsRank,
			entry.reason.c_str()
		});
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_fleet_result(
		result.chosen, result.complete ? 1 : 0, result.reason.c_str(),
		entries.data(), entries.size()));
}

inline void DownloadXmlCommand::Execute()
{
	bool v13 = true;

	char* filename;
	if (!NextParamAsStr(&filename))
	{
		BuildErrorResponse(2, "Invalid parameter (Filename)");
		return;
	}

	char* content;
	if (!NextParamAsStr(&content))
	{
		BuildErrorResponse(2, "Invalid parameter (Content)");
		return;
	}

	char* category;
	if (!NextParamAsStr(&category))
	{
		v13 = false;
		category = content;
	}

	DecodeStr(filename);
	DecodeStr(category);

	debug("FileName=%s", filename);

	// For backward compatibility with 0.8 parameter "Priority" is optional (error checking omitted)
	int priority = 0;
	NextParamAsInt(&priority);

	bool addTop;
	if (!NextParamAsBool(&addTop))
	{
		BuildErrorResponse(2, "Invalid parameter (AddTop)");
		return;
	}

	if (!v13 && !NextParamAsStr(&content))
	{
		BuildErrorResponse(2, "Invalid parameter (FileContent)");
		return;
	}

	DecodeStr(content);

	bool addPaused = false;
	char* dupeKey = nullptr;
	int dupeScore = 0;
	bool autoCategory = false;
	EDupeMode dupeMode = dmScore;
	if (NextParamAsBool(&addPaused))
	{
		if (!NextParamAsStr(&dupeKey))
		{
			BuildErrorResponse(2, "Invalid parameter (DupeKey)");
			return;
		}
		DecodeStr(dupeKey);
		if (!NextParamAsInt(&dupeScore))
		{
			BuildErrorResponse(2, "Invalid parameter (DupeScore)");
			return;
		}
		char* dupeModeStr = nullptr;
		int parsedDupeMode = NextParamAsStr(&dupeModeStr)
			? nzbget_rs_ntw_xmlrpc_parse_dupe_mode(
				dupeModeStr, (int)dmScore, (int)dmAll, (int)dmForce)
			: -1;
		if (parsedDupeMode < 0)
		{
			BuildErrorResponse(2, "Invalid parameter (DupeMode)");
			return;
		}
		dupeMode = (EDupeMode)parsedDupeMode;

		NextParamAsBool(&autoCategory);
	}
	else if (v13)
	{
		BuildErrorResponse(2, "Invalid parameter (AddPaused)");
		return;
	}

	NzbParameterList Params;
	if (v13)
	{
		char* paramName = nullptr;
		char* paramValue = nullptr;
		while (NextParamAsStr(&paramName))
		{
			if (!NextParamAsStr(&paramValue))
			{
				BuildErrorResponse(2, "Invalid parameter (Parameters)");
				return;
			}
			// decoded like every other string: a password with a quote or a
			// backslash was stored escaped, and unpacking failed
			DecodeStr(paramName);
			DecodeStr(paramValue);
			Params.SetParameter(paramName, paramValue);
		}
	}

	if (nzbget_rs_ntw_xmlrpc_is_http_url(content) != 0)
	{
		// add url
		std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
		nzbInfo->SetKind(NzbInfo::nkUrl);
		nzbInfo->SetUrl(content);
		nzbInfo->SetFilename(filename);
		nzbInfo->SetCategory(category);
		nzbInfo->SetAutoCategory(autoCategory);
		nzbInfo->SetPriority(priority);
		nzbInfo->SetAddUrlPaused(addPaused);
		nzbInfo->SetDupeKey(dupeKey ? dupeKey : "");
		nzbInfo->SetDupeScore(dupeScore);
		nzbInfo->SetDupeMode(dupeMode);
		nzbInfo->GetParameters()->CopyFrom(&Params);
		int nzbId = nzbInfo->GetId();

		info("Queue %s", *nzbInfo->MakeNiceUrlName(content, filename));

		g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), addTop);

		if (v13)
		{
			BuildIntResponse(nzbId);
		}
		else
		{
			BuildBoolResponse(true);
		}
	}
	else
	{
		int len = WebUtil::DecodeBase64(content, 0, content);
		content[len] = '\0';
		if (nzbget_rs_unpack_can_extract_with_zipfast(filename) != 0)
		{
			const auto status = g_Scanner->AddArchive(
				filename,
				category,
				autoCategory,
				priority,
				dupeKey,
				dupeScore,
				dupeMode,
				Params.empty() ? nullptr : &Params,
				addTop,
				addPaused,
				nullptr,
				content,
				len
			);
			BuildBoolResponse(status == Scanner::asSuccess);
		}
		else
		{
			int nzbId = -1;
			g_Scanner->AddExternalFile(
				filename,
				category,
				autoCategory,
				priority,
				dupeKey,
				dupeScore,
				dupeMode,
				Params.empty() ? nullptr : &Params,
				addTop,
				addPaused,
				nullptr,
				nullptr,
				content,
				len,
				&nzbId
			);

			int appendMode = nzbget_rs_dqh_xmlrpc_classify_append_result_mode(
				v13 ? 1 : 0, nzbId);
			if (appendMode == 1)
			{
				BuildIntResponse(nzbId);
			}
			else
			{
				BuildBoolResponse(appendMode == 2);
			}
		}
	}
}

// deprecated
inline void PostQueueXmlCommand::Execute()
{
	int nrEntries = 0;
	NextParamAsInt(&nrEntries);

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		PostInfo* postInfo = nzbInfo->GetPostInfo();
		if (!postInfo)
		{
			continue;
		}

		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_postqueue_item_start(
			IsJson() ? 1 : 0,
			index++ > 0 ? 1 : 0,
			nzbInfo->GetId(),
			postInfo->GetNzbInfo()->GetName(),
			(int)postInfo->GetStage(),
			postInfo->GetFileProgress()));

		AppendNzbInfoFields(postInfo->GetNzbInfo());
		AppendCondResponse(",\n", IsJson());
		AppendPostInfoFields(postInfo, nrEntries, true);

		AppendResponse(IsJson() ? "}" : "</struct></value>\n");
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline void WriteLogXmlCommand::Execute()
{
	char* kind;
	char* text;
	if (!NextParamAsStr(&kind) || !NextParamAsStr(&text))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	DecodeStr(text);

	debug("Kind=%s, Text=%s", kind, text);

	switch (nzbget_rs_rco_rpc_parse_write_log_kind(kind))
	{
		case 0: info("%s", text); break;
		case 1: warn("%s", text); break;
		case 2: error("%s", text); break;
		case 4: detail("%s", text); break;
		case 3: debug("%s", text); break;
		default:
			BuildErrorResponse(3, "Invalid Kind");
			return;
	}

	BuildBoolResponse(true);
}

inline void ClearLogXmlCommand::Execute()
{
	g_Log->Clear();

	BuildBoolResponse(true);
}

inline void ScanXmlCommand::Execute()
{
	bool syncMode = false;
	// optional parameter "SyncMode"
	NextParamAsBool(&syncMode);

	g_Scanner->ScanNzbDir(syncMode);
	BuildBoolResponse(true);
}

// struct[] history(bool hidden)
// Parameter "hidden" is optional (new in v12)
inline void HistoryXmlCommand::Execute()
{
	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	bool dup = false;
	NextParamAsBool(&dup);

	int index = 0;

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	for (HistoryInfo* historyInfo : guard->GetHistory())
	{
		if (nzbget_rs_roc_xmlrpc_history_should_skip_dup(
			(int)historyInfo->GetKind(), (int)HistoryInfo::hkDup, dup ? 1 : 0))
		{
			continue;
		}

		NzbInfo* nzbInfo = nullptr;
		const char* status = DetectStatus(historyInfo);
		int hasComma = index++ > 0 ? 1 : 0;
		int entryKind = nzbget_rs_dqh_xmlrpc_classify_history_entry_kind(
			(int)historyInfo->GetKind(),
			(int)HistoryInfo::hkNzb,
			(int)HistoryInfo::hkUrl,
			(int)HistoryInfo::hkDup);

		if (entryKind == 1)
		{
			nzbInfo = historyInfo->GetNzbInfo();

			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_history_nzb_start(
				IsJson() ? 1 : 0,
				hasComma,
				historyInfo->GetId(),
				historyInfo->GetName(),
				nzbInfo->GetParkedFileCount(),
				!nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
				(int)historyInfo->GetTime(),
				status));
		}
		else if (entryKind == 2)
		{
			DupInfo* dupInfo = historyInfo->GetDupInfo();

			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_history_dup_item(
				IsJson() ? 1 : 0,
				hasComma,
				historyInfo->GetId(),
				historyInfo->GetName(),
				(int)historyInfo->GetTime(),
				dupInfo->GetSize(),
				dupInfo->GetDupeKey(),
				dupInfo->GetDupeScore(),
				(int)dupInfo->GetDupeMode(),
				(int)dupInfo->GetStatus(),
				status));
		}

		if (nzbInfo)
		{
			AppendNzbInfoFields(nzbInfo);
		}

		AppendResponse(IsJson() ? "}" : "</struct></value>");
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline const char* HistoryXmlCommand::DetectStatus(HistoryInfo* historyInfo)
{
	const char* status = "FAILURE/INTERNAL_ERROR";
	int entryKind = nzbget_rs_dqh_xmlrpc_classify_history_entry_kind(
		(int)historyInfo->GetKind(),
		(int)HistoryInfo::hkNzb,
		(int)HistoryInfo::hkUrl,
		(int)HistoryInfo::hkDup);

	if (entryKind == 1)
	{
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		status = nzbInfo->MakeTextStatus(false);
	}
	else if (entryKind == 2)
	{
		DupInfo* dupInfo = historyInfo->GetDupInfo();
		status = nzbget_rs_rco_rpc_detect_history_dup_status((int)dupInfo->GetStatus());
	}

	return status;
}

// Deprecated in v13
inline void UrlQueueXmlCommand::Execute()
{
	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetKind() == NzbInfo::nkUrl)
		{
			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_urlqueue_item(
				IsJson() ? 1 : 0,
				index++ > 0 ? 1 : 0,
				nzbInfo->GetId(),
				nzbInfo->GetFilename(),
				nzbInfo->GetUrl(),
				nzbInfo->GetName(),
				nzbInfo->GetCategory(),
				nzbInfo->GetPriority()));
		}
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

// struct[] config()
inline void ConfigXmlCommand::Execute()
{
	Options::GuardedOptEntries optEntries = g_Options->GuardOptEntries();
	std::vector<NzbgetRsRpcConfigItem> entries;
	entries.reserve(optEntries->size());
	for (Options::OptEntry& optEntry : optEntries)
	{
		entries.push_back(NzbgetRsRpcConfigItem{
			optEntry.GetName(),
			optEntry.GetValue(),
			optEntry.Restricted() ? 1 : 0
		});
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_config_list(
		IsJson() ? 1 : 0,
		(int)m_userAccess,
		(int)XmlRpcProcessor::uaRestricted,
		entries.data(),
		entries.size()));
}

// struct[] loadconfig()
inline void LoadConfigXmlCommand::Execute()
{
	int fault = 0;
	NzbgetRsBuf buf = nzbget_rs_xmlrpc_execute_load_config(
		IsJson() ? 1 : 0,
		(int)m_userAccess,
		(int)XmlRpcProcessor::uaRestricted,
		g_Options ? g_Options->GetConfigFilename() : nullptr,
		&fault);
	m_fault = fault != 0;
	AppendRsBuf(m_response, buf);
}

inline void LoadExtensionsXmlCommand::Execute()
{
	bool loadFromDisk = false;
	bool isJson = IsJson();

	if (!NextParamAsBool(&loadFromDisk))
	{
		BuildErrorResponse(2, "Invalid parameter (Load from disk)");
		return;
	}

	if (loadFromDisk)
	{
		const auto& error = g_ExtensionManager->LoadExtensions();
		if (error)
		{
			BuildErrorResponse(3, "%s", error.value().c_str());
			return;
		}
	}

	AppendResponse(isJson ? "[\n" : "<array><data>\n");

	int index = 0;

	g_ExtensionManager->ForEach([&](auto extension)
		{
			const std::string response = isJson
				? Extension::ToJsonStr(*extension)
				: Extension::ToXmlStr(*extension);

			AppendCondResponse(",\n", nzbget_rs_dqh_xmlrpc_should_emit_json_array_comma(
				isJson ? 1 : 0, index++) != 0);
			AppendResponse(response.c_str());
		}
	);

	AppendResponse(isJson ? "\n]" : "</data></array>\n");
}

inline void DownloadExtensionXmlCommand::Execute()
{
	char* url;
	if (!NextParamAsStr(&url))
	{
		BuildErrorResponse(2, "Invalid parameter (URL)");
		return;
	}
	DecodeStr(url);

	char* extName;
	if (!NextParamAsStr(&extName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension name)");
		return;
	}
	DecodeStr(extName);

	if (Util::EmptyStr(g_Options->GetTempDir()))
	{
		BuildErrorResponse(3, "\"TempDir\" is not specified");
		return;
	}

	const auto result = g_ExtensionManager->DownloadExtension(url, extName);
	const auto& filename = std::get<1>(result);
	// the downloaded archive stayed in TempDir whenever the install didn't
	// happen (a successful install removes it)
	auto removeDownload = [&filename]()
		{
			fs::error_code ec;
			fs::remove(filename, ec);
		};
	bool ok = std::get<0>(result) == WebDownloader::adFinished;
	if (!ok)
	{
		removeDownload();
		BuildErrorResponse(3, "Failed to read URL");
		return;
	}

	if (g_Options->GetScriptDirPaths().empty())
	{
		removeDownload();
		BuildErrorResponse(3, "\"ScriptDir\" is not specified");
		return;
	}

	const auto error = g_ExtensionManager->InstallExtension(filename, g_Options->GetScriptDirPaths().front());
	if (error)
	{
		removeDownload();
		BuildErrorResponse(3, "%s", error.value().c_str());
		return;
	}

	BuildBoolResponse(true);
}

inline void UpdateExtensionXmlCommand::Execute()
{
	char* url;
	if (!NextParamAsStr(&url))
	{
		BuildErrorResponse(2, "Invalid parameter (URL)");
		return;
	}
	DecodeStr(url);

	char* extName;
	if (!NextParamAsStr(&extName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension name)");
		return;
	}
	DecodeStr(extName);

	const auto result = g_ExtensionManager->DownloadExtension(url, extName);
	bool ok = std::get<0>(result) == WebDownloader::adFinished;
	if (!ok)
	{
		// a partly downloaded archive stayed in TempDir
		fs::error_code ec;
		fs::remove(std::get<1>(result), ec);
		BuildErrorResponse(3, "Failed to read URL");
		return;
	}

	const auto filename = std::get<1>(result);
	const auto error = g_ExtensionManager->UpdateExtension(filename, extName);
	if (error)
	{
		BuildErrorResponse(3, "%s", error.value().c_str());
		return;
	}

	BuildBoolResponse(true);
}

inline void DeleteExtensionXmlCommand::Execute()
{
	char* extName;
	if (!NextParamAsStr(&extName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension name)");
		return;
	}
	DecodeStr(extName);

	const auto error = g_ExtensionManager->DeleteExtension(extName);
	if (error)
	{
		BuildErrorResponse(2, "%s", error.value().c_str());
		return;
	}

	BuildBoolResponse(true);
}

inline void TestExtensionXmlCommand::Execute()
{
	char* extEntryFileName;
	if (!NextParamAsStr(&extEntryFileName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension entry file name)");
		return;
	}
	DecodeStr(extEntryFileName);

	const auto found = Util::FindExecutorProgram(extEntryFileName, g_Options->GetShellOverride());
	if (!found)
	{
		BuildErrorResponse(2, "Failed to find the corresponding executor");
		return;
	}

	BuildBoolResponse(true);
}

// bool saveconfig(struct[] data)
inline void SaveConfigXmlCommand::Execute()
{
	int fault = 0;
	NzbgetRsBuf buf = nzbget_rs_xmlrpc_execute_save_config(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		g_Options ? g_Options->GetConfigFilename() : nullptr,
		&fault);
	m_fault = fault != 0;
	AppendRsBuf(m_response, buf);
}

// struct[] configtemplates(bool loadFromDisk)
// parameter "loadFromDisk" is optional (new in v14)
inline void ConfigTemplatesXmlCommand::Execute()
{
	bool loadFromDisk = false;
	NextParamAsBool(&loadFromDisk);

	ScriptConfig::ConfigTemplates* configTemplates = g_ScriptConfig->GetConfigTemplates();

	ScriptConfig::ConfigTemplates loadedConfigTemplates;
	if (loadFromDisk)
	{
		if (!g_ScriptConfig->LoadConfigTemplates(&loadedConfigTemplates))
		{
			BuildErrorResponse(3, "Could not read configuration templates");
			return;
		}
		configTemplates = &loadedConfigTemplates;
	}

	std::vector<const char*> templates;
	templates.reserve(configTemplates->size());
	for (ScriptConfig::ConfigTemplate& configTemplate : configTemplates)
	{
		templates.push_back(configTemplate.GetTemplate());
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_config_templates(
		IsJson() ? 1 : 0, templates.data(), templates.size()));
}

// struct[] viewfeed(int id)
// v12:
// struct[] previewfeed(string name, string url, string filter, bool pauseNzb, string category,
//		int priority, bool includeNonMatching, int cacheTimeSec, string cacheId)
// v16:
// struct[] previewfeed(int id, string name, string url, string filter, bool backlog, bool pauseNzb, string category,
//		int priority, int interval, string feedfilter, bool includeNonMatching, int cacheTimeSec, string cacheId)
inline void ViewFeedXmlCommand::Execute()
{
	bool includeNonMatching = false;
	std::shared_ptr<FeedItemList> feedItems;

	if (m_preview)
	{
		int id = 0;
		char* name;
		char* url;
		char* filter;
		bool backlog = true;
		bool pauseNzb;
		char* category;
		char* categorySourceStr = nullptr;	// optional: unset when not sent
		auto categorySource = FeedInfo::CategorySource::NZBFile;
		int interval = 0;
		int priority;
		char* feedFilter = nullptr;
		char* cacheId;
		int cacheTimeSec;

		// if the first parameter is int then it's the v16 signature
		bool v16 = NextParamAsInt(&id);

		if (!NextParamAsStr(&name) || !NextParamAsStr(&url) || !NextParamAsStr(&filter) ||
			(v16 && !NextParamAsBool(&backlog)) || !NextParamAsBool(&pauseNzb) ||
			!NextParamAsStr(&category) || !NextParamAsInt(&priority) ||
			(v16 && (!NextParamAsInt(&interval) || !NextParamAsStr(&feedFilter))) ||
			!NextParamAsBool(&includeNonMatching) || !NextParamAsInt(&cacheTimeSec) ||
			!NextParamAsStr(&cacheId))
		{
			BuildErrorResponse(2, "Invalid parameter");
			return;
		}

		NextParamAsStr(&categorySourceStr);
		if (categorySourceStr)
		{
			categorySource = (FeedInfo::CategorySource)nzbget_rs_ntw_xmlrpc_parse_category_source(
				categorySourceStr,
				(int)FeedInfo::CategorySource::NZBFile,
				(int)FeedInfo::CategorySource::FeedFile,
				(int)FeedInfo::CategorySource::Auto);
		}

		DecodeStr(name);
		DecodeStr(url);
		DecodeStr(filter);
		DecodeStr(cacheId);
		DecodeStr(category);

		debug("Url=%s", url);
		debug("Filter=%s", filter);

		feedItems = g_FeedCoordinator->PreviewFeed(
			id,
			name,
			url,
			filter,
			backlog,
			pauseNzb,
			category,
			categorySource,
			priority,
			interval,
			feedFilter,
			cacheTimeSec,
			cacheId
		);
	}
	else
	{
		int id = 0;
		if (!NextParamAsInt(&id) || !NextParamAsBool(&includeNonMatching))
		{
			BuildErrorResponse(2, "Invalid parameter");
			return;
		}

		debug("ID=%i", id);

		feedItems = g_FeedCoordinator->ViewFeed(id);
	}

	if (!feedItems)
	{
		BuildErrorResponse(3, "Could not read feed");
		return;
	}

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");
	int index = 0;

	for (FeedItemInfo& feedItemInfo : feedItems.get())
	{
		int sizeMB = 0;
		if (nzbget_rs_rol_xmlrpc_view_feed_should_include(
			includeNonMatching ? 1 : 0, (int)feedItemInfo.GetMatchStatus(),
			(int)FeedItemInfo::msAccepted, feedItemInfo.GetSize(), &sizeMB))
		{
			NzbgetRsRpcFeedItem item{};
			item.title = feedItemInfo.GetTitle();
			item.filename = feedItemInfo.GetFilename();
			item.url = feedItemInfo.GetUrl();
			item.size = feedItemInfo.GetSize();
			item.size_mb = sizeMB;
			item.category = feedItemInfo.GetCategory();
			item.add_category = feedItemInfo.GetAddCategory().c_str();
			item.pause_nzb = feedItemInfo.GetPauseNzb() ? 1 : 0;
			item.priority = feedItemInfo.GetPriority();
			item.time = (int)feedItemInfo.GetTime();
			item.match_status = (int)feedItemInfo.GetMatchStatus();
			item.match_rule = feedItemInfo.GetMatchRule();
			item.dupe_key = feedItemInfo.GetDupeKey();
			item.dupe_score = feedItemInfo.GetDupeScore();
			item.dupe_mode = (int)feedItemInfo.GetDupeMode();
			item.status = (int)feedItemInfo.GetStatus();

			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_feed_item(
				IsJson() ? 1 : 0, index++ > 0 ? 1 : 0, &item));
		}
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

// bool fetchfeed(int ID)
inline void FetchFeedXmlCommand::Execute()
{
	int id;
	if (!NextParamAsInt(&id))
	{
		BuildErrorResponse(2, "Invalid parameter (ID)");
		return;
	}

	g_FeedCoordinator->FetchFeed(id);

	BuildBoolResponse(true);
}

// bool editserver(int ID, bool Active)
inline void EditServerXmlCommand::Execute()
{
	bool ok = false;
	int first = true;

	int id;
	while (NextParamAsInt(&id))
	{
		first = false;

		bool active;
		if (!NextParamAsBool(&active))
		{
			BuildErrorResponse(2, "Invalid parameter");
			return;
		}

		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (nzbget_rs_dqh_xmlrpc_matches_edit_server_id(server->GetId(), id) != 0)
			{
				server->SetActive(active);
				ok = true;
			}
		}
	}

	if (first)
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	if (ok)
	{
		g_ServerPool->Changed();
	}

	BuildBoolResponse(ok);
}

// string readurl(string url, string infoname)
inline void ReadUrlXmlCommand::Execute()
{
	char* url;
	if (!NextParamAsStr(&url))
	{
		BuildErrorResponse(2, "Invalid parameter (URL)");
		return;
	}
	DecodeStr(url);

	char* infoName;
	if (!NextParamAsStr(&infoName))
	{
		BuildErrorResponse(2, "Invalid parameter (InfoName)");
		return;
	}
	DecodeStr(infoName);

	// generate temp file name
	BString<1024> tempFileName;
	int num = 1;
	while (num == 1 || FileSystem::FileExists(tempFileName))
	{
		NzbgetRsBuf tmpBuf = nzbget_rs_tpr_xmlrpc_format_readurl_temp_filename(
			g_Options->GetTempDir(), PATH_SEPARATOR, num);
		tempFileName.Set(tmpBuf.data ? tmpBuf.data : "", (int)tmpBuf.len);
		nzbget_rs_free(tmpBuf);
		num++;
	}

	std::unique_ptr<WebDownloader> downloader = std::make_unique<WebDownloader>();
	downloader->SetUrl(url);
	downloader->SetForce(true);
	downloader->SetRetry(false);
	downloader->SetOutputFilename(tempFileName);
	downloader->SetInfoName(infoName);

	// do sync download
	WebDownloader::EStatus status = downloader->DownloadWithRedirects(5);
	bool ok = status == WebDownloader::adFinished;

	downloader.reset();

	if (ok)
	{
		CharBuffer fileContent;
		FileSystem::LoadFileIntoBuffer(tempFileName, fileContent, true);
		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(IsJson() ? 1 : 0, fileContent, 1, 0));
	}
	else
	{
		BuildErrorResponse(3, "Could not read url");
	}

	FileSystem::DeleteFile(tempFileName);
}

// string checkupdates()
inline void CheckUpdatesXmlCommand::Execute()
{
	CString updateInfo;
	bool ok = g_Maintenance->CheckUpdates(updateInfo);

	if (ok)
	{
		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(IsJson() ? 1 : 0, updateInfo, 1, 0));
	}
	else
	{
		BuildErrorResponse(3, "Could not read update info from update-info-script");
	}
}

// bool startupdate(string branch)
inline void StartUpdateXmlCommand::Execute()
{
	char* branchName;
	if (!NextParamAsStr(&branchName))
	{
		BuildErrorResponse(2, "Invalid parameter (Branch)");
		return;
	}
	DecodeStr(branchName);

	Maintenance::EBranch branch;
	int branchCode = nzbget_rs_rco_rpc_parse_update_branch(branchName);
	if (branchCode < 0)
	{
		BuildErrorResponse(2, "Invalid parameter (Branch)");
		return;
	}
	branch = (Maintenance::EBranch)branchCode;

	bool ok = g_Maintenance->StartUpdate(branch);

	BuildBoolResponse(ok);
}

// struct[] logupdate(idfrom, entries)
inline GuardedMessageList LogUpdateXmlCommand::GuardMessages()
{
	return g_Maintenance->GuardMessages();
}

// struct[] servervolumes(bool BytesPerSeconds, bool BytesPerMinutes, bool BytesPerHours, bool BytesPerDays, bool ArticlesPerDays)
inline void ServerVolumesXmlCommand::Execute()
{
	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	bool BytesPer[] = {true, true, true, true};
	NextParamAsBool(&BytesPer[0])
		&& NextParamAsBool(&BytesPer[1])
		&& NextParamAsBool(&BytesPer[2])
		&& NextParamAsBool(&BytesPer[3]);

	bool articlesPerDays = true;
	NextParamAsBool(&articlesPerDays);

	int index = 0;

	for (ServerVolume& serverVolume : g_StatMeter->GuardServerVolumes())
	{
		const auto& articles = serverVolume.GetArticlesPerDays();
		std::vector<NzbgetRsRpcArticlePair> articleEntries;
		if (articlesPerDays)
		{
			articleEntries.reserve(articles.size());
			for (const auto& art : articles)
			{
				articleEntries.push_back(NzbgetRsRpcArticlePair{ art.failed, art.success });
			}
		}

		const ServerVolume::VolumeArray* bps = serverVolume.BytesPerSeconds();
		const ServerVolume::VolumeArray* bpm = serverVolume.BytesPerMinutes();
		const ServerVolume::VolumeArray* bph = serverVolume.BytesPerHours();
		const ServerVolume::VolumeArray* bpd = serverVolume.BytesPerDays();

		NzbgetRsRpcServerVolumeItem volFields{};
		volFields.server_id = index;
		volFields.data_time = static_cast<int>(serverVolume.GetDataTime());
		volFields.first_day = serverVolume.GetFirstDay();
		volFields.total_bytes = serverVolume.GetTotalBytes();
		volFields.custom_bytes = serverVolume.GetCustomBytes();
		volFields.custom_time = static_cast<int>(serverVolume.GetCustomTime());
		volFields.counters_reset_time = static_cast<int>(serverVolume.GetCountersResetTime());
		volFields.sec_slot = serverVolume.GetSecSlot();
		volFields.min_slot = serverVolume.GetMinSlot();
		volFields.hour_slot = serverVolume.GetHourSlot();
		volFields.day_slot = serverVolume.GetDaySlot();
		volFields.include_seconds = BytesPer[0] ? 1 : 0;
		volFields.bytes_per_seconds = bps->data();
		volFields.bytes_per_seconds_len = bps->size();
		volFields.include_minutes = BytesPer[1] ? 1 : 0;
		volFields.bytes_per_minutes = bpm->data();
		volFields.bytes_per_minutes_len = bpm->size();
		volFields.include_hours = BytesPer[2] ? 1 : 0;
		volFields.bytes_per_hours = bph->data();
		volFields.bytes_per_hours_len = bph->size();
		volFields.include_days = BytesPer[3] ? 1 : 0;
		volFields.bytes_per_days = bpd->data();
		volFields.bytes_per_days_len = bpd->size();
		volFields.include_articles = articlesPerDays ? 1 : 0;
		volFields.articles_per_days = articleEntries.data();
		volFields.articles_per_days_len = articleEntries.size();

		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_server_volume_item(
			IsJson() ? 1 : 0,
			index,
			&volFields));

		index++;
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

// bool resetservervolume(int serverid, string counter);
inline void ResetServerVolumeXmlCommand::Execute()
{
	int serverId;
	char* counter;
	if (!NextParamAsInt(&serverId) || !NextParamAsStr(&counter))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	int counterKind = nzbget_rs_ntw_xmlrpc_validate_reset_counter(counter);
	if (counterKind == 0)
	{
		BuildErrorResponse(3, "Invalid Counter");
		return;
	}
	bool reset = counterKind == 1;

	bool ok = false;
	int index = 0;
	for (ServerVolume& serverVolume : g_StatMeter->GuardServerVolumes())
	{
		if (nzbget_rs_dqh_xmlrpc_matches_reset_server_volume(index, serverId) != 0)
		{
			if (reset)
				serverVolume.Reset();
			else
				serverVolume.ResetCustom();
			ok = true;
		}
		index++;
	}

	// saved like a download's counts: a restart before the next download
	// brought the old counters back
	if (ok)
	{
		g_StatMeter->MarkChanged();
	}

	BuildBoolResponse(ok);
}

// struct[] loadlog(nzbid, logidfrom, logentries)
inline void LoadLogXmlCommand::Execute()
{
	m_nzbInfo = nullptr;
	m_nzbId = 0;
	if (!NextParamAsInt(&m_nzbId))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	LogXmlCommand::Execute();
	m_downloadQueue.reset();
}

inline GuardedMessageList LoadLogXmlCommand::GuardMessages()
{
	// TODO: optimize for m_iIDFrom and m_iNrEntries
	g_DiskState->LoadNzbMessages(m_nzbId, &m_messages);

	if (m_messages.empty())
	{
		// When we returing cached messages from m_nzbInfo we have to make sure that
		// m_nzbInfo isn't destroyed during we are working with CachedMessages.
		// For that purpose we set a lock on DownloadQueue,
		// which will be released later in LoadLogXmlCommand::Execute().

		m_downloadQueue = std::make_unique<GuardedDownloadQueue>(DownloadQueue::Guard());
		m_nzbInfo = (*m_downloadQueue)->GetQueue()->Find(m_nzbId);
		if (m_nzbInfo)
		{
			return m_nzbInfo->GuardCachedMessages();
		}
		else
		{
			// No nzb-object available, the lock on DownloadQueue can be released right now.
			m_downloadQueue.reset();
		}
	}

	return GuardedMessageList(&m_messages, nullptr);
}

// string testserver(string host, int port, string username, string password, bool encryption, string cipher, int timeout, int certVerifLevel);
inline void TestServerXmlCommand::Execute()
{
	TestServerXmlCommand::ServerParams params;
	char* host;
	int port;
	char* username;
	char* password;
	bool encryption;
	char* cipher;
	int timeout;
	int certVerifLevel;

	// a JSON body only with POST: over GET (JSON-P) m_request is the query string
	if (nzbget_rs_dqh_xmlrpc_should_parse_json_post_test_server(
		IsJson() ? 1 : 0, m_request != nullptr ? 1 : 0,
		(int)m_httpMethod, (int)XmlRpcProcessor::hmPost) != 0)
	{
		// rust/src/rpcparams.rs; a std::exception while copying the strings
		// gives "Invalid parameters", as in ParseRequestParams
		struct Context
		{
			ServerParams* params;
			bool failed;
		} context{&params, false};
		const NzbgetRsJsonLimits limits{0x7ffffffeu, 0x7fffffffu / 24u, 0x3fffffffu};
		int parsed = nzbget_rs_test_server_params(m_request, limits, &context,
			[](void* ctx, const NzbgetRsTestServerParams* from) noexcept
			{
				Context& context = *static_cast<Context*>(ctx);
				try
				{
					ServerParams p;
					p.host.assign(from->host.data, from->host.len);
					p.port = from->port;
					p.username.assign(from->username.data, from->username.len);
					p.password.assign(from->password.data, from->password.len);
					p.encryption = from->encryption != 0;
					p.cipher.assign(from->cipher.data, from->cipher.len);
					p.timeout = from->timeout;
					p.certVerifLevel = from->certVerifLevel;
					*context.params = std::move(p);
				}
				catch (const std::exception&)
				{
					context.failed = true;
				}
			});
		if (parsed == 0)
		{
			BuildErrorResponse(2, "Invalid JSON");
			return;
		}
		if (parsed < 0 || context.failed)
		{
			BuildErrorResponse(2, "Invalid parameters");
			return;
		}
	}
	else if (!NextParamAsStr(&host) || !NextParamAsInt(&port) || !NextParamAsStr(&username) ||
		!NextParamAsStr(&password) || !NextParamAsBool(&encryption) ||
		!NextParamAsStr(&cipher) || !NextParamAsInt(&timeout) ||
		!NextParamAsInt(&certVerifLevel))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}
	else
	{
		DecodeStr(host);
		DecodeStr(username);
		DecodeStr(password);
		DecodeStr(cipher);
		params.host = host;
		params.port = port;
		params.username = username;
		params.password = password;
		params.encryption = encryption;
		params.cipher = cipher;
		params.timeout = timeout;
		params.certVerifLevel = certVerifLevel;
	}

	if (!nzbget_rs_dqh_xmlrpc_is_valid_cert_verif_level(
		params.certVerifLevel, (int)Options::ECertVerifLevel::Count))
	{
		BuildErrorResponse(2, "Invalid parameter (Certificate Verification Level).");
		return;
	}

	NewsServer server(
		0,
		true,
		"test server",
		params.host.c_str(),
		params.port,
		0,
		params.username.c_str(),
		params.password.c_str(),
		false,
		params.encryption,
		params.cipher.c_str(),
		1,
		0,
		0,
		0,
		false,
		params.certVerifLevel
	);
	TestConnection connection(&server, this);
	connection.SetTimeout(nzbget_rs_rol_xmlrpc_test_server_effective_timeout(
		params.timeout, g_Options->GetArticleTimeout()));
	connection.SetSuppressErrors(false);

	bool ok = connection.Connect();
	if (ok)
	{
		// generate a unique non-existent message-id since we don't want a real article to be returned
		BString<1024> id;
		while (id.Length() < 30)
		{
			NzbgetRsBuf rBuf = nzbget_rs_dqh_conn_format_port_str(rand());
			id.Append(rBuf.data ? rBuf.data : "", (int)rBuf.len);
			nzbget_rs_free(rBuf);
		}
		NzbgetRsBuf cmdBuf = nzbget_rs_tpr_xmlrpc_format_testserver_article_cmd(*id);
		const char* response = connection.Request(cmdBuf.data ? cmdBuf.data : "");
		nzbget_rs_free(cmdBuf);
		ok = nzbget_rs_rol_xmlrpc_test_server_response_ok(response) != 0;
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(
		IsJson() ? 1 : 0,
		nzbget_rs_dqh_xmlrpc_select_test_server_err_text(ok ? 1 : 0, *m_errText),
		0,
		1023));
}

inline void TestServerXmlCommand::PrintError(const char* errMsg)
{
	if (m_errText.Empty())
	{
		m_errText = EncodeStr(errMsg);
	}
}

inline void TestServerSpeedXmlCommand::Execute()
{
	char* nzbFileUrl;
	if (!NextParamAsStr(&nzbFileUrl))
	{
		BuildErrorResponse(2, "Invalid parameter: NZB file url");
		return;
	}
	DecodeStr(nzbFileUrl);

	int serverId;
	if (!NextParamAsInt(&serverId))
	{
		BuildErrorResponse(2, "Invalid parameter: Server ID");
		return;
	}

	NewsServer* server = g_ServerPool->GetServerById(serverId);
	int serverErr = nzbget_rs_roc_xmlrpc_test_server_speed_check_server(
		server != nullptr ? 1 : 0,
		server && server->GetActive() ? 1 : 0,
		server ? server->GetMaxConnections() : 0);
	if (serverErr != 0)
	{
		BuildErrorResponse(2, nzbget_rs_dqh_xmlrpc_test_server_speed_err_msg(serverErr));
		return;
	}

	std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
	nzbInfo->SetKind(NzbInfo::nkUrl);
	nzbInfo->SetUrl(nzbFileUrl);
	nzbInfo->SetFilename(nzbFileUrl);
	nzbInfo->SetPriority(NzbInfo::FORCE_PRIORITY);
	nzbInfo->SetExtraPriority(NzbInfo::FORCE_PRIORITY);
	nzbInfo->SetAddUrlPaused(false);
	nzbInfo->SetDupeMode(EDupeMode::dmForce);
	nzbInfo->SetDesiredServerId(serverId);
	nzbInfo->SetSkipScriptProcessing(true);
	nzbInfo->SetSkipDiskWrite(true);

	g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), true);

	BuildBoolResponse(true);
}

inline void TestDiskSpeedXmlCommand::Execute()
{
	NzbgetRsBuf dirPathBuf{nullptr, 0};
	size_t bufferSizeBytes = 0;
	uint64_t maxFileSizeBytes = 0;
	int timeoutSec = 0;
	const char* errMsg = nullptr;
	if (!nzbget_rs_xmlrpc_parse_test_disk_speed(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		&dirPathBuf,
		&bufferSizeBytes,
		&maxFileSizeBytes,
		&timeoutSec,
		&errMsg))
	{
		nzbget_rs_free(dirPathBuf);
		BuildErrorResponse(2, "%s", errMsg ? errMsg : "Invalid parameter");
		return;
	}
	CString dirPath(dirPathBuf.data ? dirPathBuf.data : "");
	nzbget_rs_free(dirPathBuf);

	PauseGuard pauseGuard;

	std::string dir(*dirPath ? *dirPath : "");
	fs::error_code ec;
	const fs::path fsDirPath = fs::u8path(dir);
	fs::space_info space = fs::space(fsDirPath, ec);
	std::string spaceErr = ec ? ec.message() : std::string();

	uint64_t writeBytes = 0;
	double writeMs = 0.0;
	uint64_t readBytes = 0;
	double readMs = 0.0;
	NzbgetRsBuf errBuf{nullptr, 0};
	int rc = nzbget_rs_benchmark_run(
		dir.data(), dir.size(),
		bufferSizeBytes,
		maxFileSizeBytes,
		timeoutSec,
		ec ? 0 : 1,
		space.available,
		spaceErr.data(), spaceErr.size(),
		&writeBytes, &writeMs, &readBytes, &readMs,
		&errBuf);
	if (rc != 0)
	{
		std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
		nzbget_rs_free(errBuf);
		warn("Disk speed test for %s failed: %s", *dirPath, err.c_str());
		BuildErrorResponse(2, "%s", err.c_str());
		return;
	}
	nzbget_rs_free(errBuf);

	info("Disk speed test for %s: write %.1f MiB/s, read %.1f MiB/s",
		*dirPath, MiBPerSec(writeBytes, writeMs), MiBPerSec(readBytes, readMs));

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_disk_speed(
		IsJson() ? 1 : 0, writeBytes, writeMs, readBytes, readMs));
}

inline void TestNetworkSpeedXmlCommand::Execute()
{
	// a pause that was on before the test stays on
	PauseGuard pauseGuard;
	auto executeIterCb = [](
		void*,
		const char* reqPtr,
		size_t reqLen,
		int64_t timeoutSec,
		uint32_t* outDownloaded,
		double* outElapsed) -> int
	{
		using namespace std::chrono;
#ifdef DISABLE_TLS
		Connection connection("speed.cloudflare.com", 80, false);
#else
		Connection connection("speed.cloudflare.com", 443, true);
#endif
		connection.SetTimeout(1);
		if (!connection.Connect() || !connection.Send(reqPtr, reqLen))
		{
			if (outDownloaded) *outDownloaded = 0;
			if (outElapsed) *outElapsed = 0.0;
			return 0;
		}
		uint32_t totalReceived = 0;
		char buffer[8 * 1024];
		auto start = steady_clock::now();
		while (int received = connection.TryRecv(buffer, sizeof(buffer)))
		{
			if (nzbget_rs_dqh_speed_test_should_stop_recv_loop(
				received,
				duration<double>(steady_clock::now() - start).count(),
				static_cast<double>(timeoutSec)) != 0)
			{
				break;
			}
			totalReceived += static_cast<uint32_t>(received);
		}
		auto finish = steady_clock::now();
		if (outDownloaded) *outDownloaded = totalReceived;
		if (outElapsed) *outElapsed = duration<double>(finish - start).count();
		return 1;
	};
	auto logCb = [](void*, int kind, const char* msg)
	{
		if (!msg) return;
		if (kind == 1)
		{
			error("%s", msg);
		}
		else
		{
			info("%s", msg);
		}
	};
	double speedMbps = 0.0;
	NzbgetRsBuf errBuf{nullptr, 0};
	int ok = nzbget_rs_network_speed_test_run(nullptr, executeIterCb, logCb, &speedMbps, &errBuf);
	if (!ok)
	{
		std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
		nzbget_rs_free(errBuf);
		BuildErrorResponse(2, "%s", err.c_str());
		return;
	}
	nzbget_rs_free(errBuf);
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_network_speed(
		IsJson() ? 1 : 0, speedMbps));
}

// bool startscript(string script, string command, string context, struct[] options);
inline void StartScriptXmlCommand::Execute()
{
	NzbgetRsBuf scriptBuf{nullptr, 0};
	NzbgetRsBuf commandBuf{nullptr, 0};
	NzbgetRsBuf contextBuf{nullptr, 0};
	std::unique_ptr<Options::OptEntries> optEntries = std::make_unique<Options::OptEntries>();
	if (!nzbget_rs_xmlrpc_parse_start_script(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		&scriptBuf,
		&commandBuf,
		&contextBuf,
		optEntries.get(),
		[](void* ctx, const char* name, const char* val)
		{
			static_cast<Options::OptEntries*>(ctx)->emplace_back(name, val);
		}))
	{
		nzbget_rs_free(scriptBuf);
		nzbget_rs_free(commandBuf);
		nzbget_rs_free(contextBuf);
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}
	nzbget_rs_free(contextBuf);

	bool ok = CommandScriptController::StartScript(
		scriptBuf.data ? scriptBuf.data : "",
		commandBuf.data ? commandBuf.data : "",
		std::move(optEntries));
	nzbget_rs_free(scriptBuf);
	nzbget_rs_free(commandBuf);

	BuildBoolResponse(ok);
}

// struct[] logscript(idfrom, entries)
inline GuardedMessageList LogScriptXmlCommand::GuardMessages()
{
	return g_CommandScriptLog->GuardMessages();
}


#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "Util.h"

#ifndef DISABLE_TLS
#include <openssl/rand.h>
#endif

namespace WebServerDetail
{
	inline constexpr const char* ERR_HTTP_BAD_REQUEST = "400 Bad Request";
	inline constexpr const char* ERR_HTTP_NOT_FOUND = "404 Not Found";
	inline constexpr const char* ERR_HTTP_SERVICE_UNAVAILABLE = "503 Service Unavailable";
	inline constexpr int MAX_UNCOMPRESSED_SIZE = 500;

	inline void GenerateSecureToken(char* token)
	{
		// 1 byte = 2 hex characters.
		// For a token string of size TOKEN_SIZE (including \0), we need half that many random bytes.
		constexpr size_t BYTES_NEEDED = (TOKEN_SIZE - 1) / 2;

#ifndef DISABLE_TLS
		unsigned char randBytes[BYTES_NEEDED];
		if (RAND_bytes(randBytes, static_cast<int>(BYTES_NEEDED)) == 1)
		{
			nzbget_rs_dqh_webserver_encode_hex_token(
				randBytes, BYTES_NEEDED, token, TOKEN_SIZE);
			return;
		}
#endif

		try
		{
			std::random_device rd;
			std::uniform_int_distribution<unsigned int> dist(0, 0xFF);
			uint8_t fallbackBytes[BYTES_NEEDED];
			for (size_t i = 0; i < BYTES_NEEDED; ++i)
			{
				fallbackBytes[i] = static_cast<uint8_t>(dist(rd));
			}
			nzbget_rs_dqh_webserver_encode_hex_token(
				fallbackBytes, BYTES_NEEDED, token, TOKEN_SIZE);
		}
		catch (const std::exception& ex)
		{
			error("Could not generate secure authentication token: %s", ex.what());
			exit(1);
		}
	}

	// tolower as WildMask's C++ code did: on a char, signed or not
	inline int WebFold(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		return tolower(ch);
#else
		return ch < 0 ? ch : tolower(ch);
#endif
	}
}

inline void WebProcessor::Init()
{
	if (m_serverAuthToken[0][0] != 0)
	{
		// already initialized
		return;
	}

	for (size_t j = uaControl; j <= uaAdd; ++j)
	{
		WebServerDetail::GenerateSecureToken(m_serverAuthToken[j]);
	}
}

inline void WebProcessor::Execute()
{
	m_gzip =false;
	m_userAccess = uaControl;
	m_authInfo[0] = '\0';
	m_authToken[0] = '\0';

	ParseHeaders();

	int preParseMethod = nzbget_rs_dqh_webserver_classify_pre_parse_method(
		(int)m_httpMethod, (int)hmPost, (int)hmOptions, m_contentLen);
	if (preParseMethod == 1)
	{
		// answered and closed: without an answer a kept-alive client waited until
		// its timeout, and a body (chunked) left unread would be the next request
		error("Invalid-request: content length is 0");
		m_keepAlive = false;
		SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, false);
		return;
	}

	if (preParseMethod == 2)
	{
		SendOptionsResponse();
		return;
	}

	if (!ParseUrl())
	{
		return;
	}

	m_rpcRequest = XmlRpcProcessor::IsRpcRequest(m_url);
	m_authorized = CheckCredentials();

	if (nzbget_rs_sfw_web_proc_should_send_auth_response(
		g_Options->GetFormAuth() ? 1 : 0, m_rpcRequest ? 1 : 0, m_authorized ? 1 : 0) != 0)
	{
		SendAuthResponse();
		return;
	}

	if (m_httpMethod == hmPost)
	{
		// a body of a size no request has: the buffer for it couldn't be allocated
		// (its size + 1 overflowed at INT_MAX) and was written through anyway. A
		// request without credentials (the login form) is small
		if (!nzbget_rs_ntw_web_proc_is_content_len_valid(m_contentLen, m_authorized ? 1 : 0))
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, true);
			return;
		}

		// reading http body (request content)
		m_request.Reserve(m_contentLen);
		if (!m_request)
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_SERVICE_UNAVAILABLE, true);
			return;
		}
		m_request[m_contentLen] = '\0';

		if (!m_connection->Recv(m_request, m_contentLen))
		{
			error("Invalid-request: could not read data");
			m_keepAlive = false;
			return;
		}
		debug("Request=%s", *m_request);
	}

	debug("request received from %s", m_connection->GetRemoteAddr());

	Dispatch();
}

inline void WebProcessor::ParseHeaders()
{
	// Keep the entire initialized buffer: in unusual locales strncasecmp can
	// match at an early NUL, but C++ still reads values at fixed offsets.
	char buffer[1024]{};
	m_contentLen = 0;
	while (char* p = m_connection->ReadLine(buffer, sizeof(buffer), nullptr))
	{
		size_t crPos = 0;
		if (nzbget_rs_dqh_scanner_find_last_char_pos(p, '\r', &crPos) != 0)
		{
			p[crPos] = '\0';
		}
		debug("header=%s", p);

		size_t start = 0, len = 0;
		int number = 0;
		int kind = nzbget_rs_web_header(p, sizeof(buffer), Util::EmptyStr(m_authInfo), &start, &len, &number);
		std::string value(p + start, len);
		switch (kind)
		{
			case 1:
				m_contentLen = number;
				break;
			case 2:
				m_authInfo[WebUtil::DecodeBase64(p + start, 0, m_authInfo)] = '\0';
				break;
			case 3:
				error("Invalid-request: auth-info too big");
				return;
			case 4:
				m_gzip = number != 0;
				break;
			case 5:
				m_origin = value.c_str();
				break;
			case 6:
				strncpy(m_authToken, value.c_str(), sizeof(m_authToken) - 1);
				m_authToken[sizeof(m_authToken) - 1] = '\0';
				break;
			case 7:
				m_forwardedFor = value.c_str();
				break;
			case 8:
				m_oldETag = value.c_str();
				break;
			case 9:
				m_keepAlive = true;
				break;
			case 10:
				debug("URL=%s", *m_url);
				return;
		}
	}

	debug("URL=%s", *m_url);
}

// false when it answered the request itself (a redirect)
inline bool WebProcessor::ParseUrl()
{
	// rust/src/webserver.rs
	int redirect = 0;
	NzbgetRsBuf auth{};
	NzbgetRsBuf url = nzbget_rs_web_parse_url(m_url, strlen(m_url), &redirect, &auth);
	// Both buffers belong to Rust, including if constructing target throws.
	auto release = [](NzbgetRsBuf* buf) { nzbget_rs_free(*buf); };
	std::unique_ptr<NzbgetRsBuf, decltype(release)> urlOwner(&url, release);
	std::unique_ptr<NzbgetRsBuf, decltype(release)> authOwner(&auth, release);
	std::string target(url.data, url.len);
	if (redirect)
	{
		SendRedirectResponse(target.c_str());
		return false;
	}
	if (auth.data)
	{
		size_t len = std::min(auth.len, sizeof(m_authInfo) - 1);
		memcpy(m_authInfo, auth.data, len);
		m_authInfo[len] = '\0';
	}
	m_url = target.c_str();

	debug("Final URL=%s", *m_url);
	return true;
}

inline bool WebProcessor::CheckCredentials()
{
	// rust/src/webserver.rs
	NzbgetRsWebCredentials input{};
	input.users[0] = g_Options->GetControlUsername();
	input.users[1] = g_Options->GetControlPassword();
	input.users[2] = g_Options->GetRestrictedUsername();
	input.users[3] = g_Options->GetRestrictedPassword();
	input.users[4] = g_Options->GetAddUsername();
	input.users[5] = g_Options->GetAddPassword();
	input.authorizedIp = g_Options->GetAuthorizedIp();
	input.remoteAddr = m_connection->GetRemoteAddr();
	input.authInfo = m_authInfo;
	input.authToken = m_authToken;
	for (int j = uaControl; j <= uaAdd; j++)
	{
		input.serverTokens[j] = m_serverAuthToken[j];
	}
#ifdef __GLIBC__
	input.lowerTable = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#endif
	input.charSigned = CHAR_MIN < 0;
	input.fold = WebServerDetail::WebFold;

	NzbgetRsWebCheck check{};
	nzbget_rs_web_check_credentials(&input, &check);
	if (check.authCut >= 0)
	{
		m_authInfo[check.authCut] = '\0';
	}
	if (check.access >= 0)
	{
		m_userAccess = (EUserAccess)check.access;
	}
	if (check.warn)
	{
		NzbgetRsBuf fwdBuf = nzbget_rs_dqh_webserver_format_forwarded_for_suffix(*m_forwardedFor);
		warn("Request received on port %i from %s%s, but username (%s) or password invalid",
			g_Options->GetControlPort(), m_connection->GetRemoteAddr(),
			fwdBuf.data ? fwdBuf.data : "",
			m_authInfo);
		nzbget_rs_free(fwdBuf);
	}
	return check.authorized != 0;
}

inline void WebProcessor::Dispatch()
{
	if (m_url[0] != '/')
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, true);
		return;
	}

	if (m_rpcRequest)
	{
		XmlRpcProcessor processor;
		processor.SetRequest(m_request);
		processor.SetHttpMethod(m_httpMethod == hmGet ? XmlRpcProcessor::hmGet : XmlRpcProcessor::hmPost);
		processor.SetUserAccess((XmlRpcProcessor::EUserAccess)m_userAccess);
		processor.SetJsonP(g_Options->GetJsonP());
		processor.SetUrl(m_url);
		processor.Execute();
		SendBodyResponse(processor.GetResponse(), strlen(processor.GetResponse()), processor.GetContentType(), processor.IsSafeMethod());
		return;
	}

	if (m_httpMethod != hmGet)
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, true);
		return;
	}

	int getRoute = nzbget_rs_sus_web_classify_get_dispatch(m_url);
	if (getRoute == 0)
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, true);
		return;
	}
	if (nzbget_rs_dqh_webserver_classify_ext_route(getRoute) != 0)
	{
		if (!m_authorized)
		{
			SendAuthResponse();
			return;
		}

		const auto& scriptDirs = g_Options->GetScriptDirPaths();
		if (scriptDirs.empty())
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_SERVICE_UNAVAILABLE, true);
			return;
		}

		if (getRoute == 1)
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, false);
			return;
		}

		std::string_view extUrl = m_url + 12;
		bool found = false;
		for (const auto& scriptDir : scriptDirs)
		{
			fs::path filePath = scriptDir / extUrl;
			fs::error_code ec;
			auto resolvedDir = fs::canonical(scriptDir, ec);
			if (ec) continue;

			auto resolvedPath = fs::canonical(filePath, ec);
			if (ec) continue;

			auto rel = resolvedPath.lexically_relative(resolvedDir);
			if (nzbget_rs_dqh_webserver_is_rel_path_outside_root(
				rel.empty() ? 1 : 0,
				rel.empty() ? '\0' : static_cast<char>(rel.native().front())) != 0) continue;

			CharBuffer body;
			std::string filePathStr = fs::u8string(resolvedPath);
			if (FileSystem::LoadFileIntoBuffer(filePathStr.c_str(), body, true))
			{
				const char* contentType = DetectContentType(filePathStr.c_str());
				int len = body.Size() - 1;
				SendBodyResponse(body, len, contentType, true);
				found = true;
				break;
			}
		}

		if (!found)
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, false);
		}
		return;
	}

	if (Util::EmptyStr(g_Options->GetWebDir()))
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_SERVICE_UNAVAILABLE, true);
		return;
	}

	if (getRoute == 3)
	{
		SendMultiFileResponse();
	}
	else
	{
		SendSingleFileResponse();
	}
}

inline void WebProcessor::SendAuthResponse()
{
	NzbgetRsBuf hdr = nzbget_rs_web_format_auth_response(
		g_Options->GetFormAuth() ? 1 : 0, m_keepAlive ? 1 : 0, Util::VersionRevision());
	debug("ResponseHeader=%s", hdr.data ? hdr.data : "");
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
}

inline void WebProcessor::SendOptionsResponse()
{
	NzbgetRsBuf hdr = nzbget_rs_web_format_options_response(
		m_keepAlive ? 1 : 0, m_origin.Str(), Util::VersionRevision());
	debug("ResponseHeader=%s", hdr.data ? hdr.data : "");
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
}

inline void WebProcessor::SendErrorResponse(const char* errCode, bool printWarning)
{
	if (printWarning)
	{
		warn("Web-Server: %s, Resource: %s", errCode, *m_url);
	}

	NzbgetRsBuf body{};
	NzbgetRsBuf hdr = nzbget_rs_web_format_error_response(
		errCode, m_keepAlive ? 1 : 0, Util::VersionRevision(), &body);
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	if (body.data)
	{
		m_connection->Send(body.data, (int)body.len);
	}
	nzbget_rs_free(hdr);
	nzbget_rs_free(body);
}

inline void WebProcessor::SendRedirectResponse(const char* url)
{
	NzbgetRsBuf hdr = nzbget_rs_web_format_redirect_response(
		url, m_keepAlive ? 1 : 0, Util::VersionRevision());
	debug("ResponseHeader=%s", hdr.data ? hdr.data : "");
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
}

inline void WebProcessor::SendBodyResponse(const char* body, int bodyLen, const char* contentType, bool cachable)
{
	BString<1024> newETag;
	bool unchanged = false;

	if (cachable)
	{
		// over the whole body: as a C string it ended at the first zero byte, and
		// every image (PNG, ICO...) had the same ETag - a changed one was answered
		// "not modified"
		size_t hash = m_hasher(std::string_view(body, bodyLen));
		NzbgetRsBuf etagBuf = nzbget_rs_tpr_web_format_quoted_hex_etag(hash);
		newETag.Set(etagBuf.data ? etagBuf.data : "", (int)etagBuf.len);
		nzbget_rs_free(etagBuf);

		unchanged = nzbget_rs_sfw_web_proc_is_etag_unchanged(m_oldETag, newETag) != 0;
		if (unchanged)
		{
			body = "";
			bodyLen = 0;
		}
	}

#ifndef DISABLE_GZIP
	CharBuffer gbuf;
	bool gzip = nzbget_rs_ntw_web_proc_should_use_gzip(
		m_gzip ? 1 : 0, bodyLen, WebServerDetail::MAX_UNCOMPRESSED_SIZE) != 0;
	if (gzip)
	{
		uint32 outLen = ZLib::GZipLen(bodyLen);
		gbuf.Reserve(outLen);
		int gzippedLen = ZLib::GZip(body, bodyLen, *gbuf, outLen);
		if (nzbget_rs_tpr_web_is_gzip_smaller(gzippedLen, bodyLen) != 0)
		{
			body = gbuf;
			bodyLen = gzippedLen;
		}
		else
		{
			gzip = false;
		}
	}
#else
	bool gzip = false;
#endif

	NzbgetRsBuf hdr = nzbget_rs_web_format_body_response_header(
		unchanged ? 1 : 0,
		m_keepAlive ? 1 : 0,
		m_origin.Str(),
		g_Options->GetFormAuth() ? 1 : 0,
		m_authorized ? m_serverAuthToken[m_userAccess] : "",
		bodyLen,
		contentType,
		gzip ? 1 : 0,
		cachable ? *newETag : nullptr,
		Util::VersionRevision());

	debug("[%s] (%s) %s", *m_url, *m_oldETag, hdr.data ? hdr.data : "");

	// Send the request answer
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
	m_connection->Send(body, bodyLen);
}

inline void WebProcessor::SendSingleFileResponse()
{
	const char* defRes = nzbget_rs_ntw_web_proc_default_resource(m_url);

	BString<1024> filename;
	{
		NzbgetRsBuf pathBuf = nzbget_rs_tpr_web_format_single_file_path(
			g_Options->GetWebDir(), *m_url, defRes);
		filename.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
		nzbget_rs_free(pathBuf);
	}

	debug("serving file: %s", *filename);

	CharBuffer body;
	if (!FileSystem::LoadFileIntoBuffer(filename, body, true))
	{
		// do not print warnings "404 not found" for certain files (by base
		// name: the full path never matched them; and the flag passed on is
		// "print warning", so no missing file was ever warned about)
		const char* baseName = FileSystem::BaseFileName(filename);
		bool ignorable = nzbget_rs_sus_web_is_ignorable_404(baseName) != 0;

		SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, !ignorable);
		return;
	}

	const char* contentType = DetectContentType(filename);
	int len = body.Size() - 1;

#ifdef DEBUG
	if (nzbget_rs_dqh_webserver_is_html_content_type(contentType) != 0)
	{
		Util::ReduceStr(body, "<!-- %if-debug%", "");
		Util::ReduceStr(body, "<!-- %if-not-debug% -->", "<!--");
		Util::ReduceStr(body, "<!-- %end% -->", "-->");
		Util::ReduceStr(body, "%end% -->", "");
		len = strlen(body);
	}
#endif

	SendBodyResponse(body, len, contentType, true);
}

inline void WebProcessor::SendMultiFileResponse()
{
	debug("serving multiple files: %s", *m_url);

	StringBuilder response;
	size_t qPos = 0;
	nzbget_rs_dqh_diskstate_find_char_pos(m_url, '?', &qPos);
	char* filelist = (char*)m_url + qPos;
	*filelist++ = '\0';

	Tokenizer tok(filelist, "+");
	while (const char* filename = tok.Next())
	{
		NzbgetRsBuf diskFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetWebDir(), PATH_SEPARATOR, filename);
		BString<1024> diskFilename;
		diskFilename.Set(diskFnBuf.data ? diskFnBuf.data : "", (int)diskFnBuf.len);
		nzbget_rs_free(diskFnBuf);

		CharBuffer body;
		if (!FileSystem::LoadFileIntoBuffer(diskFilename, body, true))
		{
			warn("Web-Server: %s, Resource: /%s", WebServerDetail::ERR_HTTP_NOT_FOUND, filename);
			SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, false);
			return;
		}

		response.Append(body);
	}

	SendBodyResponse(response, response.Length(), DetectContentType(m_url), true);
}

inline const char* WebProcessor::DetectContentType(const char* filename)
{
	return nzbget_rs_sus_web_detect_content_type(filename);
}

#include <system_error>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef BINRPC_H
#define BINRPC_H

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef FRONTEND_H
#define FRONTEND_H
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@users.sourceforge.net>
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef MESSAGEBASE_H
#define MESSAGEBASE_H

static const int32 NZBMESSAGE_SIGNATURE = 0x6E7A6231; // = "nzb-XX" (protocol version)
static const int NZBREQUESTFILENAMESIZE = 512;
static const int NZBREQUESTPASSWORDSIZE = 32;

/**
* NZBGet communication protocol uses only two basic data types: integer and char.
* Integer values are passed using network byte order (Big-Endian).
* Use function "htonl" and "ntohl" to convert integers to/from machine
* (host) byte order.
* All char-strings ends with nullptr-char.
*
* NOTE:
* NZBGet communication protocol is intended for usage only by NZBGet itself.
* The communication works only if server and client has the same version.
* The compatibility with previous program versions is not provided.
* Third-party programs should use JSON-RPC or XML-RPC to communicate with NZBGet.
*/

// Possible values for field "m_type" of struct "SNZBRequestBase":
enum ERemoteRequest
{
	rrDownload = 1,
	rrPauseUnpause,
	rrList,
	rrSetDownloadRate,
	rrDumpDebug,
	rrEditQueue,
	rrLog,
	rrShutdown,
	rrReload,
	rrVersion,
	rrPostQueue,
	rrWriteLog,
	rrScan,
	rrHistory
};

// Possible values for field "m_action" of struct "SNZBPauseUnpauseRequest":
enum ERemotePauseUnpauseAction
{
	rpDownload = 1, // pause/unpause download queue
	rpPostProcess, // pause/unpause post-processor queue
	rpScan // pause/unpause scan of incoming nzb-directory
};

// Possible values for field "m_matchMode" of struct "SNZBEditQueueRequest":
enum ERemoteMatchMode
{
	rmId = 1, // ID
	rmName, // Name
	rmRegEx // RegEx
};

// The basic SNZBRequestBase struct, used in all requests
struct SNzbRequestBase
{
	int32 m_signature; // Signature must be NZBMESSAGE_SIGNATURE in integer-value
	int32 m_structSize; // Size of the entire struct
	int32 m_type; // Message type, see enum in NZBMessageRequest-namespace
	char m_username[NZBREQUESTPASSWORDSIZE]; // User name
	char m_password[NZBREQUESTPASSWORDSIZE]; // Password
};

// The basic SNZBResposneBase struct, used in all responses
struct SNzbResponseBase
{
	int32 m_signature; // Signature must be NZBMESSAGE_SIGNATURE in integer-value
	int32 m_structSize; // Size of the entire struct
};

// A download request
struct SNzbDownloadRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	char m_nzbFilename[NZBREQUESTFILENAMESIZE]; // Name of nzb-file. For URLs can be empty, then the filename is read from URL download response
	char m_category[NZBREQUESTFILENAMESIZE]; // Category, can be empty
	int32 m_addFirst; // 1 - add file to the top of download queue
	int32 m_addPaused; // 1 - pause added files
	int32 m_priority; // Priority for files (0 - default)
	int32 m_dupeScore; // Duplicate score
	int32 m_dupeMode; // Duplicate mode (EDupeMode)
	char m_dupeKey[NZBREQUESTFILENAMESIZE]; // Duplicate key
	int32 m_trailingDataLength; // Length of nzb-file in bytes
	int32 m_autoCategory;
	//char m_content[m_trailingDataLength]; // variable sized
};

// A download response
struct SNzbDownloadResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// A list and status request
struct SNzbListRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_fileList; // 1 - return file list
	int32 m_serverState; // 1 - return server state
	int32 m_matchMode; // File/Group match mode, see enum eRemoteMatchMode (only values eRemoteMatchModeID (no filter) and eRemoteMatchModeRegEx are allowed)
	int32 m_matchGroup; // 0 - match files; 1 - match nzbs (when m_iMatchMode == eRemoteMatchModeRegEx)
	char m_pattern[NZBREQUESTFILENAMESIZE]; // RegEx Pattern (when m_iMatchMode == eRemoteMatchModeRegEx)
};

// A list response
struct SNzbListResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBListResponseEntry-struct
	int32 m_remainingSizeLo; // Remaining size in bytes, Low 32-bits of 64-bit value
	int32 m_remainingSizeHi; // Remaining size in bytes, High 32-bits of 64-bit value
	int32 m_downloadRate; // Current download speed, in Bytes pro Second
	int32 m_downloadLimit; // Current download limit, in Bytes pro Second
	int32 m_downloadPaused; // 1 - download queue is currently in paused-state
	int32 m_download2Paused; // 1 - download queue is currently in paused-state (second pause-register)
	int32 m_downloadStandBy; // 0 - there are currently downloads running, 1 - no downloads in progress (download queue paused or all download jobs completed)
	int32 m_postPaused; // 1 - post-processor queue is currently in paused-state
	int32 m_scanPaused; // 1 - scaning of incoming directory is currently in paused-state
	int32 m_threadCount; // Number of threads running
	int32 m_postJobCount; // Number of jobs in post-processor queue (including current job)
	int32 m_upTimeSec; // Server up time in seconds
	int32 m_downloadTimeSec; // Server download time in seconds (up_time - standby_time)
	int32 m_downloadedBytesLo; // Amount of data downloaded since server start, Low 32-bits of 64-bit value
	int32 m_downloadedBytesHi; // Amount of data downloaded since server start, High 32-bits of 64-bit value
	int32 m_regExValid; // 0 - error in RegEx-pattern, 1 - RegEx-pattern is valid (only when Request has eRemoteMatchModeRegEx)
	int32 m_nrTrailingNzbEntries; // Number of List-NZB-entries, following to this structure
	int32 m_nrTrailingPPPEntries; // Number of List-PPP-entries, following to this structure
	int32 m_nrTrailingFileEntries; // Number of List-File-entries, following to this structure
	int32 m_trailingDataLength; // Length of all List-entries, following to this structure
	// SNzbListResponseEntry m_nzbEntries[m_nrTrailingNZBEntries] // variable sized
	// SNzbListResponseEntry m_pppEntries[m_nrTrailingPPPEntries] // variable sized
	// SNzbListResponseEntry m_fileEntries[m_nrTrailingFileEntries] // variable sized
};

// A list response nzb entry
struct SNzbListResponseNzbEntry
{
	int32 m_id; // NZB-ID
	int32 m_kind; // Item Kind (see NZBInfo::Kind)
	int32 m_sizeLo; // Size of all files in bytes, Low 32-bits of 64-bit value
	int32 m_sizeHi; // Size of all files in bytes, High 32-bits of 64-bit value
	int32 m_remainingSizeLo; // Size of remaining (unpaused) files in bytes, Low 32-bits of 64-bit value
	int32 m_remainingSizeHi; // Size of remaining (unpaused) files in bytes, High 32-bits of 64-bit value
	int32 m_pausedSizeLo; // Size of npaused files in bytes, Low 32-bits of 64-bit value
	int32 m_pausedSizeHi; // Size of paused files in bytes, High 32-bits of 64-bit value
	int32 m_pausedCount; // Number of paused files
	int32 m_remainingParCount; // Number of remaining par-files
	int32 m_priority; // Download priority
	int32 m_match; // 1 - group matches the pattern (only when Request has eRemoteMatchModeRegEx)
	int32 m_filenameLen; // Length of Filename-string (m_szFilename), following to this record
	int32 m_nameLen; // Length of Name-string (m_szName), following to this record
	int32 m_destDirLen; // Length of DestDir-string (m_szDestDir), following to this record
	int32 m_categoryLen; // Length of Category-string (m_szCategory), following to this record
	int32 m_queuedFilenameLen; // Length of queued file name (m_szQueuedFilename), following to this record
	//char m_filename[m_filenameLen]; // variable sized
	//char m_name[m_nameLen]; // variable sized
	//char m_destDir[m_destDirLen]; // variable sized
	//char m_dategory[m_categoryLen]; // variable sized
	//char m_queuedFilename[m_queuedFilenameLen]; // variable sized
};

// A list response pp-parameter entry
struct SNzbListResponsePPPEntry
{
	int32 m_nzbIndex; // Index of NZB-Entry in m_NZBEntries-list
	int32 m_nameLen; // Length of Name-string (m_szName), following to this record
	int32 m_valueLen; // Length of Value-string (m_szValue), following to this record
	//char m_name[m_nameLen]; // variable sized
	//char m_value[m_valueLen]; // variable sized
};

// A list response file entry
struct SNzbListResponseFileEntry
{
	int32 m_id; // Entry-ID
	int32 m_nzbIndex; // Index of NZB-Entry in m_NZBEntries-list
	int32 m_fileSizeLo; // Filesize in bytes, Low 32-bits of 64-bit value
	int32 m_fileSizeHi; // Filesize in bytes, High 32-bits of 64-bit value
	int32 m_remainingSizeLo; // Remaining size in bytes, Low 32-bits of 64-bit value
	int32 m_remainingSizeHi; // Remaining size in bytes, High 32-bits of 64-bit value
	int32 m_paused; // 1 - file is paused
	int32 m_filenameConfirmed; // 1 - Filename confirmed (read from article body), 0 - Filename parsed from subject (can be changed after reading of article)
	int32 m_activeDownloads; // Number of active downloads for this file
	int32 m_match; // 1 - file matches the pattern (only when Request has eRemoteMatchModeRegEx)
	int32 m_subjectLen; // Length of Subject-string (m_szSubject), following to this record
	int32 m_filenameLen; // Length of Filename-string (m_szFilename), following to this record
	//char m_subject[m_subjectLen]; // variable sized
	//char m_filename[m_filenameLen]; // variable sized
};

// A log request
struct SNzbLogRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_idFrom; // Only one of these two parameters
	int32 m_lines; // can be set. The another one must be set to "0".
};

// A log response
struct SNzbLogResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBLogResponseEntry-struct
	int32 m_nrTrailingEntries; // Number of Log-entries, following to this structure
	int32 m_trailingDataLength; // Length of all Log-entries, following to this structure
	// SNZBLogResponseEntry m_entries[m_trailingEntries] // variable sized
};

// A log response entry
struct SNzbLogResponseEntry
{
	int32 m_id; // ID of Log-entry
	int32 m_kind; // see Message::Kind in "Log.h"
	int32 m_time; // time since the Epoch (00:00:00 UTC, January 1, 1970), measured in seconds.
	int32 m_textLen; // Length of Text-string (m_szText), following to this record
	//char m_text[m_textLen]; // variable sized
};

// A Pause/Unpause request
struct SNzbPauseUnpauseRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_pause; // 1 - server must be paused, 0 - server must be unpaused
	int32 m_action; // Action to be executed, see enum ERemotePauseUnpauseAction
};

// A Pause/Unpause response
struct SNzbPauseUnpauseResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Request setting the download rate
struct SNzbSetDownloadRateRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_downloadRate; // Speed limit, in Bytes pro Second
};

// A setting download rate response
struct SNzbSetDownloadRateResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
								//char m_text[m_trailingDataLength]; // variable sized
};

// edit queue request
struct SNzbEditQueueRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_action; // Action to be executed, see enum DownloadQueue::EEditAction
	int32 m_offset; // Offset to move (for m_iAction = 0)
	int32 m_matchMode; // File/Group match mode, see enum eRemoteMatchMode
	int32 m_nrTrailingIdEntries; // Number of ID-entries, following to this structure
	int32 m_nrTrailingNameEntries; // Number of Name-entries, following to this structure
	int32 m_trailingNameEntriesLen; // Length of all Name-entries, following to this structure
	int32 m_textLen; // Length of Text-string (m_szText), following to this record
	int32 m_trailingDataLength; // Length of Text-string and all ID-entries, following to this structure
	//char m_text[m_textLen]; // variable sized
	//int32 m_ids[m_nrTrailingIdEntries]; // variable sized array of IDs. For File-Actions - ID of file, for Group-Actions - ID of any file belonging to group
	//char* m_names[m_nrTrailingNameEntries]; // variable sized array of strings. For File-Actions - name of file incl. nzb-name as path, for Group-Actions - name of group
};

// An edit queue response
struct SNzbEditQueueResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Request dumping of debug info
struct SNzbDumpDebugRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Dumping of debug response
struct SNzbDumpDebugResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Shutdown server request
struct SNzbShutdownRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Shutdown server response
struct SNzbShutdownResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Reload server request
struct SNzbReloadRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Reload server response
struct SNzbReloadResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Server version request
struct SNzbVersionRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Server version response
struct SNzbVersionResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// PostQueue request
struct SNzbPostQueueRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// A PostQueue response
struct SNzbPostQueueResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBPostQueueResponseEntry-struct
	int32 m_nrTrailingEntries; // Number of PostQueue-entries, following to this structure
	int32 m_trailingDataLength; // Length of all PostQueue-entries, following to this structure
	// SNZBPostQueueResponseEntry m_entries[m_nrTrailingEntries] // variable sized
};

// A PostQueue response entry
struct SNzbPostQueueResponseEntry
{
	int32 m_id; // ID of Post-entry
	int32 m_stage; // See PrePostProcessor::EPostJobStage
	int32 m_stageProgress; // Progress of current stage, value in range 0..1000
	int32 m_fileProgress; // Progress of current file, value in range 0..1000
	int32 m_totalTimeSec; // Number of seconds this post-job is beeing processed (after it first changed the state from QUEUED).
	int32 m_stageTimeSec; // Number of seconds the current stage is beeing processed.
	int32 m_nzbFilenameLen; // Length of NZBFileName-string (m_szNZBFilename), following to this record
	int32 m_infoNameLen; // Length of Filename-string (m_szFilename), following to this record
	int32 m_destDirLen; // Length of DestDir-string (m_szDestDir), following to this record
	int32 m_progressLabelLen; // Length of ProgressLabel-string (m_szProgressLabel), following to this record
	//char m_nzbFilename[m_nzbFilenameLen]; // variable sized, may contain full path (local path on client) or only filename
	//char m_infoName[m_infoNameLen]; // variable sized
	//char m_destDir[m_destDirLen]; // variable sized
	//char m_progressLabel[m_progressLabelLen]; // variable sized
};

// Write log request
struct SNzbWriteLogRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_kind; // see Message::Kind in "Log.h"
	int32 m_trailingDataLength; // Length of nzb-file in bytes
	//char m_text[m_trailingDataLength]; // variable sized
};

// Write log response
struct SNzbWriteLogResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Scan nzb directory request
struct SNzbScanRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_syncMode; // 0 - asynchronous Scan (the command returns immediately), 1 - synchronous Scan (the command returns when the scan is completed)
};

// Scan nzb directory response
struct SNzbScanResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// A history request
struct SNzbHistoryRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_hidden; // 0 - only return visible records, 1 - also return hidden records
};

// history response
struct SNzbHistoryResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBHistoryResponseEntry-struct
	int32 m_nrTrailingEntries; // Number of History-entries, following to this structure
	int32 m_trailingDataLength; // Length of all History-entries, following to this structure
	// SNZBHistoryResponseEntry m_entries[m_nrTrailingEntries] // variable sized
};

// history entry
struct SNzbHistoryResponseEntry
{
	int32 m_id; // History-ID
	int32 m_kind; // Kind of Item: 1 - Collection (NZB), 2 - URL, 3 - DUP (hidden record)
	int32 m_time; // When the item was added to history. time since the Epoch (00:00:00 UTC, January 1, 1970), measured in seconds.
	int32 m_nicenameLen; // Length of Nicename-string (m_szNicename), following to this record
	// for Collection and Dup items (m_iKind = 1 or 2)
	int32 m_sizeLo; // Size of all files in bytes, Low 32-bits of 64-bit value
	int32 m_sizeHi; // Size of all files in bytes, High 32-bits of 64-bit value
	// for Collection items (m_iKind = 1)
	int32 m_fileCount; // Initial number of files included in NZB-file
	int32 m_parStatus; // See NZBInfo::EParStatus
	int32 m_scriptStatus; // See NZBInfo::EScriptStatus
	// for URL items (m_iKind = 2)
	int32 m_urlStatus; // See NZBInfo::EUrlStatus
	//char m_nicename[m_nicenameLen]; // variable sized
};

#endif

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef REMOTECLIENT_H
#define REMOTECLIENT_H


class RemoteClient
{
public:
	void SetVerbose(bool verbose) { m_verbose = verbose; };
	bool RequestServerDownload(const char* nzbFilename, const char* nzbContent,
		const char* category, bool autoCategory,
		bool addFirst, bool addPaused, int priority,
		const char* dupeKey, int dupeMode, int dupeScore);
	bool RequestServerList(bool files, bool groups, const char* pattern);
	bool RequestServerPauseUnpause(bool pause, ERemotePauseUnpauseAction action);
	bool RequestServerSetDownloadRate(int rate);
	bool RequestServerDumpDebug();
	bool RequestServerEditQueue(DownloadQueue::EEditAction action, int offset, const char* text,
		IdList* idList, NameList* nameList, ERemoteMatchMode matchMode);
	bool RequestServerLog(int lines);
	bool RequestServerShutdown();
	bool RequestServerReload();
	bool RequestServerVersion();
	bool RequestPostQueue();
	bool RequestWriteLog(int kind, const char* text);
	bool RequestScan(bool syncMode);
	bool RequestHistory(bool withHidden);
	void BuildFileList(SNzbListResponse* listResponse, const char* trailingData, DownloadQueue* downloadQueue);

private:
	class MatchedNzbInfo: public NzbInfo
	{
	public:
		bool m_match;
	};

	class MatchedFileInfo: public FileInfo
	{
	public:
		bool m_match;
	};

	std::unique_ptr<Connection> m_connection;
	bool m_verbose = true;

	bool InitConnection();
	void InitMessageBase(SNzbRequestBase* messageBase, int request, int size);
	bool ReceiveBoolResponse();
	void printf(const char* msg, ...);
	void perror(const char* msg);
};

#include "nzbget_rs.h"

// One entry of a list response, read by rust/src/binrpc.rs: its ints (host
// order) and strings; Next is false at an entry that doesn't fit the data,
// which ends the listing
template <size_t Ints, size_t Strings>
struct RsEntry
{
	uint32 ints[Ints];
	const char* strings[Strings];

	bool Next(const char* data, size_t len, size_t& at, const size_t (&fields)[Strings])
	{
		size_t starts[Strings];
		if (!nzbget_rs_binrpc_read_entry(data, len, &at, ints, Ints, fields, starts, Strings))
		{
			return false;
		}
		for (size_t k = 0; k < Strings; k++)
		{
			strings[k] = data + starts[k];
		}
		return true;
	}
};

// A listing from rust/src/binrpc.rs: the printed lines of a response's entries as one string, freed after
#define RS_PRINT_LISTING(buf) do { NzbgetRsBuf rsListing = (buf); printf("%s", rsListing.data); nzbget_rs_free(rsListing); } while (0)

inline void RemoteClient::printf(const char * msg,...)
{
	if (m_verbose)
	{
		va_list ap;
		va_start(ap, msg);
		::vprintf(msg, ap);
		va_end(ap);
	}
}

inline void RemoteClient::perror(const char * msg)
{
	if (m_verbose)
	{
		::perror(msg);
	}
}

inline bool RemoteClient::InitConnection()
{
	const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());

	// Create a connection to the server (retry briefly if server is mid-reload)
	bool OK = false;
	for (int attempt = 0; attempt < 10; ++attempt)
	{
		m_connection = std::make_unique<Connection>(controlIp, g_Options->GetControlPort(), false);
		OK = m_connection->Connect();
		if (OK)
		{
			break;
		}
		Util::Sleep(50);
	}
	if (!OK)
	{
		printf("Unable to send request to RustyGet server at %s (port %i)\n", controlIp, g_Options->GetControlPort());
	}
	return OK;
}

inline void RemoteClient::InitMessageBase(SNzbRequestBase* messageBase, int request, int size)
{
	nzbget_rs_binrpc_init_message_base(
		(uint8_t*)messageBase,
		request,
		size,
		g_Options->GetControlUsername(),
		g_Options->GetControlPassword());
}

inline bool RemoteClient::ReceiveBoolResponse()
{
	printf("Request sent\n");

	// all bool-responses have the same format of structure, we use SNZBDownloadResponse here
	SNzbDownloadResponse BoolResponse;
	memset(&BoolResponse, 0, sizeof(BoolResponse));

	bool read = m_connection->Recv((char*)&BoolResponse, sizeof(BoolResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(BoolResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(BoolResponse.m_messageBase.m_structSize),
		(uint32)sizeof(BoolResponse)))
	{
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	int textLen = ntohl(BoolResponse.m_trailingDataLength);
	CString buf;
	buf.Reserve(textLen);
	read = m_connection->Recv(buf, textLen);
	if (!read)
	{
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	printf("server returned: %s\n", *buf);
	return ntohl(BoolResponse.m_success);
}

/*
 * Sends a message to the running nzbget process.
 */
inline bool RemoteClient::RequestServerDownload(const char* nzbFilename, const char* nzbContent,
	const char* category, bool autoCategory, bool addFirst, bool addPaused, int priority,
	const char* dupeKey, int dupeMode, int dupeScore)
{
	// Read the file into the buffer
	CharBuffer buffer;
	int length = 0;
	bool isUrl = nzbget_rs_ccm_remote_client_is_http_url(nzbContent) != 0;
	if (isUrl)
	{
		length = strlen(nzbContent) + 1;
	}
	else
	{
		if (!FileSystem::LoadFileIntoBuffer(nzbContent, buffer, false))
		{
			printf("Could not load file %s\n", nzbContent);
			return false;
		}
		length = buffer.Size();
	}

	bool OK = InitConnection();
	if (OK)
	{
		SNzbDownloadRequest DownloadRequest;
		nzbget_rs_binrpc_init_download_request(
			(uint8_t*)&DownloadRequest,
			g_Options->GetControlUsername(),
			g_Options->GetControlPassword(),
			nzbFilename,
			nzbContent,
			isUrl ? 1 : 0,
			category,
			addFirst ? 1 : 0,
			addPaused ? 1 : 0,
			priority,
			dupeScore,
			dupeMode,
			dupeKey,
			length,
			autoCategory ? 1 : 0);

		if (!m_connection->Send((char*)(&DownloadRequest), sizeof(DownloadRequest)))
		{
			perror("m_pConnection->Send");
			OK = false;
		}
		else
		{
			m_connection->Send(isUrl ? nzbContent : buffer, length);
			OK = ReceiveBoolResponse();
			m_connection->Disconnect();
		}
	}

	return OK;
}

inline void RemoteClient::BuildFileList(SNzbListResponse* listResponse, const char* trailingData, DownloadQueue* downloadQueue)
{
	size_t len = ntohl(listResponse->m_trailingDataLength);
	if (len == 0)
	{
		return;
	}
	size_t at = 0;

	// read nzb entries
	static const size_t nzbStrings[] = {12, 13, 14, 15, 16};
	for (uint32 i = 0; i < ntohl(listResponse->m_nrTrailingNzbEntries); i++)
	{
		RsEntry<17, 5> e;
		if (!e.Next(trailingData, len, at, nzbStrings))
		{
			return;
		}
		std::unique_ptr<MatchedNzbInfo> nzbInfo = std::make_unique<MatchedNzbInfo>();
		nzbInfo->SetId(e.ints[0]);
		nzbInfo->SetKind((NzbInfo::EKind)e.ints[1]);
		nzbInfo->SetSize(Util::JoinInt64(e.ints[3], e.ints[2]));
		nzbInfo->SetRemainingSize(Util::JoinInt64(e.ints[5], e.ints[4]));
		nzbInfo->SetPausedSize(Util::JoinInt64(e.ints[7], e.ints[6]));
		nzbInfo->SetPausedFileCount(e.ints[8]);
		nzbInfo->SetRemainingParCount(e.ints[9]);
		nzbInfo->SetFilename(e.strings[0]);
		nzbInfo->SetName(e.strings[1]);
		nzbInfo->SetDestDir(e.strings[2]);
		nzbInfo->SetCategory(e.strings[3]);
		nzbInfo->SetQueuedFilename(e.strings[4]);
		nzbInfo->SetPriority(e.ints[10]);
		nzbInfo->m_match = e.ints[11];

		downloadQueue->GetQueue()->Add(std::move(nzbInfo));
	}

	//read ppp entries
	static const size_t pppStrings[] = {1, 2};
	for (uint32 i = 0; i < ntohl(listResponse->m_nrTrailingPPPEntries); i++)
	{
		RsEntry<3, 2> e;
		if (!e.Next(trailingData, len, at, pppStrings))
		{
			return;
		}
		std::unique_ptr<NzbInfo>& nzbInfo = downloadQueue->GetQueue()->at(e.ints[0] - 1);
		nzbInfo->GetParameters()->SetParameter(e.strings[0], e.strings[1]);
	}

	//read file entries
	static const size_t fileStrings[] = {10, 11};
	for (uint32 i = 0; i < ntohl(listResponse->m_nrTrailingFileEntries); i++)
	{
		RsEntry<12, 2> e;
		if (!e.Next(trailingData, len, at, fileStrings))
		{
			return;
		}
		std::unique_ptr<MatchedFileInfo> fileInfo = std::make_unique<MatchedFileInfo>();
		fileInfo->SetId(e.ints[0]);
		fileInfo->SetSize(Util::JoinInt64(e.ints[3], e.ints[2]));
		fileInfo->SetRemainingSize(Util::JoinInt64(e.ints[5], e.ints[4]));
		fileInfo->SetPaused(e.ints[6]);
		fileInfo->SetSubject(e.strings[0]);
		fileInfo->SetFilename(e.strings[1]);
		fileInfo->SetFilenameConfirmed(e.ints[7]);
		fileInfo->SetActiveDownloads(e.ints[8]);
		fileInfo->m_match = e.ints[9];

		std::unique_ptr<NzbInfo>& nzbInfo = downloadQueue->GetQueue()->at(e.ints[1] - 1);
		// the group's count is its files' (the answer has none per group): it
		// stayed 0, and -L G never showed the threads of a downloading group
		nzbInfo->SetActiveDownloads(nzbInfo->GetActiveDownloads() + fileInfo->GetActiveDownloads());
		fileInfo->SetNzbInfo(nzbInfo.get());
		nzbInfo->GetFileList()->Add(std::move(fileInfo));
	}
}

inline bool RemoteClient::RequestServerList(bool files, bool groups, const char* pattern)
{
	if (!InitConnection()) return false;

	SNzbListRequest ListRequest = {};
	InitMessageBase(&ListRequest.m_messageBase, rrList, sizeof(ListRequest));
	ListRequest.m_fileList = htonl(true);
	ListRequest.m_serverState = htonl(true);
	ListRequest.m_matchMode = htonl(pattern ? rmRegEx : rmId);
	ListRequest.m_matchGroup = htonl(groups);
	if (pattern)
	{
		strncpy(ListRequest.m_pattern, pattern, NZBREQUESTFILENAMESIZE - 1);
		ListRequest.m_pattern[NZBREQUESTFILENAMESIZE-1] = '\0';
	}

	if (!m_connection->Send((char*)(&ListRequest), sizeof(ListRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned list
	SNzbListResponse ListResponse;
	bool read = m_connection->Recv((char*) &ListResponse, sizeof(ListResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(ListResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(ListResponse.m_messageBase.m_structSize),
		(uint32)sizeof(ListResponse)))
	{
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(ListResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(ListResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (pattern && !ListResponse.m_regExValid)
	{
		printf("Error in regular expression\n");
		return false;
	}

	if (files)
	{
		if (ntohl(ListResponse.m_trailingDataLength) == 0)
		{
			printf("Server has no files queued for download\n");
		}
		else
		{
			NzbgetRsBuf formatted = nzbget_rs_binrpc_file_listing_from_wire(
				buf,
				buf.Size(),
				ntohl(ListResponse.m_nrTrailingNzbEntries),
				ntohl(ListResponse.m_nrTrailingPPPEntries),
				ntohl(ListResponse.m_nrTrailingFileEntries),
				pattern ? 1 : 0);
			if (formatted.data)
			{
				fwrite(formatted.data, 1, formatted.len, stdout);
			}
			nzbget_rs_free(formatted);
		}
	}

	if (groups)
	{
		if (ntohl(ListResponse.m_trailingDataLength) == 0)
		{
			printf("Server has no files queued for download\n");
		}
		else
		{
			NzbgetRsBuf formatted = nzbget_rs_binrpc_group_listing_from_wire(
				buf,
				buf.Size(),
				ntohl(ListResponse.m_nrTrailingNzbEntries),
				ntohl(ListResponse.m_nrTrailingPPPEntries),
				ntohl(ListResponse.m_nrTrailingFileEntries),
				pattern ? 1 : 0);
			if (formatted.data)
			{
				fwrite(formatted.data, 1, formatted.len, stdout);
			}
			nzbget_rs_free(formatted);
		}
	}

	NzbgetRsBinRpcStatusSummarySpec statusSpec{};
	statusSpec.include_remaining_size = (!files && !groups) ? 1 : 0;
	statusSpec.remaining_size = Util::JoinInt64(ntohl(ListResponse.m_remainingSizeHi), ntohl(ListResponse.m_remainingSizeLo));
	statusSpec.download_rate = ntohl(ListResponse.m_downloadRate);
	statusSpec.download_paused = ntohl(ListResponse.m_downloadPaused);
	statusSpec.download2_paused = ntohl(ListResponse.m_download2Paused);
	statusSpec.download_stand_by = ntohl(ListResponse.m_downloadStandBy);
	statusSpec.downloaded_bytes = Util::JoinInt64(ntohl(ListResponse.m_downloadedBytesHi), ntohl(ListResponse.m_downloadedBytesLo));
	statusSpec.download_time_sec = ntohl(ListResponse.m_downloadTimeSec);
	statusSpec.download_limit = ntohl(ListResponse.m_downloadLimit);
	statusSpec.up_time_sec = ntohl(ListResponse.m_upTimeSec);
	statusSpec.thread_count = ntohl(ListResponse.m_threadCount);
	statusSpec.post_job_count = ntohl(ListResponse.m_postJobCount);
	statusSpec.scan_paused = ntohl(ListResponse.m_scanPaused);
	statusSpec.post_paused = ntohl(ListResponse.m_postPaused);

	NzbgetRsBuf statusBuf = nzbget_rs_binrpc_status_summary(&statusSpec);
	if (statusBuf.data)
	{
		fwrite(statusBuf.data, 1, statusBuf.len, stdout);
	}
	nzbget_rs_free(statusBuf);

	return true;
}

inline bool RemoteClient::RequestServerLog(int lines)
{
	if (!InitConnection()) return false;

	SNzbLogRequest LogRequest = {};
	InitMessageBase(&LogRequest.m_messageBase, rrLog, sizeof(LogRequest));
	LogRequest.m_lines = htonl(lines);
	LogRequest.m_idFrom = 0;

	if (!m_connection->Send((char*)(&LogRequest), sizeof(LogRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned log
	SNzbLogResponse LogResponse;
	bool read = m_connection->Recv((char*) &LogResponse, sizeof(LogResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(LogResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(LogResponse.m_messageBase.m_structSize),
		(uint32)sizeof(LogResponse)))
	{
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(LogResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(LogResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (LogResponse.m_trailingDataLength == 0)
	{
		printf("Log is empty\n");
	}
	else
	{
		printf("Log (last %i entries)\n", ntohl(LogResponse.m_nrTrailingEntries));
		printf("-----------------------------------\n");

		// rust/src/binrpc.rs: the lines of all the entries that fit the data
		RS_PRINT_LISTING(nzbget_rs_binrpc_log_listing(buf, buf.Size(), ntohl(LogResponse.m_nrTrailingEntries)));

		printf("-----------------------------------\n");
	}

	return true;
}

inline bool RemoteClient::RequestServerPauseUnpause(bool pause, ERemotePauseUnpauseAction action)
{
	if (!InitConnection()) return false;

	SNzbPauseUnpauseRequest PauseUnpauseRequest = {};
	InitMessageBase(&PauseUnpauseRequest.m_messageBase, rrPauseUnpause, sizeof(PauseUnpauseRequest));
	PauseUnpauseRequest.m_pause = htonl(pause);
	PauseUnpauseRequest.m_action = htonl(action);

	if (!m_connection->Send((char*)(&PauseUnpauseRequest), sizeof(PauseUnpauseRequest)))
	{
		perror("m_pConnection->Send");
		m_connection->Disconnect();
		return false;
	}

	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();

	return OK;
}

inline bool RemoteClient::RequestServerSetDownloadRate(int rate)
{
	if (!InitConnection()) return false;

	SNzbSetDownloadRateRequest SetDownloadRateRequest = {};
	InitMessageBase(&SetDownloadRateRequest.m_messageBase, rrSetDownloadRate, sizeof(SetDownloadRateRequest));
	SetDownloadRateRequest.m_downloadRate = htonl(rate);

	if (!m_connection->Send((char*)(&SetDownloadRateRequest), sizeof(SetDownloadRateRequest)))
	{
		perror("m_pConnection->Send");
		m_connection->Disconnect();
		return false;
	}

	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();

	return OK;
}

inline bool RemoteClient::RequestServerDumpDebug()
{
	if (!InitConnection()) return false;

	SNzbDumpDebugRequest DumpDebugInfo = {};
	InitMessageBase(&DumpDebugInfo.m_messageBase, rrDumpDebug, sizeof(DumpDebugInfo));

	if (!m_connection->Send((char*)(&DumpDebugInfo), sizeof(DumpDebugInfo)))
	{
		perror("m_pConnection->Send");
		m_connection->Disconnect();
		return false;
	}

	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();

	return OK;
}

inline bool RemoteClient::RequestServerEditQueue(DownloadQueue::EEditAction action, int offset, const char* text,
	IdList* idList, NameList* nameList, ERemoteMatchMode matchMode)
{
	if (nzbget_rs_dqh_remote_client_is_empty_edit_target(
		idList ? idList->size() : 0, nameList ? nameList->size() : 0) != 0)
	{
		printf("File(s) not specified\n");
		return false;
	}

	if (!InitConnection()) return false;

	std::vector<const char*> namePtrs;
	if (nameList && !nameList->empty())
	{
		namePtrs.reserve(nameList->size());
		for (CString& name : nameList)
		{
			namePtrs.push_back(*name);
		}
	}

	int textLen = 0;
	int nameLength = 0;
	NzbgetRsBuf trailingBuf = nzbget_rs_binrpc_encode_edit_queue_trailing(
		text,
		idList && !idList->empty() ? idList->data() : nullptr,
		idList ? idList->size() : 0,
		namePtrs.empty() ? nullptr : namePtrs.data(),
		namePtrs.size(),
		&textLen,
		&nameLength);
	int length = (int)trailingBuf.len;

	SNzbEditQueueRequest EditQueueRequest = {};
	InitMessageBase(&EditQueueRequest.m_messageBase, rrEditQueue, sizeof(EditQueueRequest));
	EditQueueRequest.m_action = htonl(action);
	EditQueueRequest.m_matchMode = htonl(matchMode);
	EditQueueRequest.m_offset = htonl((int)offset);
	EditQueueRequest.m_textLen = htonl(textLen);
	EditQueueRequest.m_nrTrailingIdEntries = htonl(idList ? idList->size() : 0);
	EditQueueRequest.m_nrTrailingNameEntries = htonl((int)namePtrs.size());
	EditQueueRequest.m_trailingNameEntriesLen = htonl(nameLength);
	EditQueueRequest.m_trailingDataLength = htonl(length);

	bool OK = false;
	if (!m_connection->Send((char*)(&EditQueueRequest), sizeof(EditQueueRequest)))
	{
		perror("m_pConnection->Send");
	}
	else
	{
		if (length > 0 && trailingBuf.data)
		{
			m_connection->Send(trailingBuf.data, length);
		}
		OK = ReceiveBoolResponse();
		m_connection->Disconnect();
	}

	nzbget_rs_free(trailingBuf);
	m_connection->Disconnect();
	return OK;
}

inline bool RemoteClient::RequestServerShutdown()
{
	if (!InitConnection()) return false;

	SNzbShutdownRequest ShutdownRequest = {};
	InitMessageBase(&ShutdownRequest.m_messageBase, rrShutdown, sizeof(ShutdownRequest));

	bool OK = m_connection->Send((char*)(&ShutdownRequest), sizeof(ShutdownRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

inline bool RemoteClient::RequestServerReload()
{
	if (!InitConnection()) return false;

	SNzbReloadRequest ReloadRequest = {};
	InitMessageBase(&ReloadRequest.m_messageBase, rrReload, sizeof(ReloadRequest));

	bool OK = m_connection->Send((char*)(&ReloadRequest), sizeof(ReloadRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

inline bool RemoteClient::RequestServerVersion()
{
	if (!InitConnection()) return false;

	SNzbVersionRequest VersionRequest = {};
	InitMessageBase(&VersionRequest.m_messageBase, rrVersion, sizeof(VersionRequest));

	bool OK = m_connection->Send((char*)(&VersionRequest), sizeof(VersionRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

inline bool RemoteClient::RequestPostQueue()
{
	if (!InitConnection()) return false;

	SNzbPostQueueRequest PostQueueRequest = {};
	InitMessageBase(&PostQueueRequest.m_messageBase, rrPostQueue, sizeof(PostQueueRequest));

	if (!m_connection->Send((char*)(&PostQueueRequest), sizeof(PostQueueRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned list
	SNzbPostQueueResponse PostQueueResponse;
	bool read = m_connection->Recv((char*) &PostQueueResponse, sizeof(PostQueueResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(PostQueueResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(PostQueueResponse.m_messageBase.m_structSize),
		(uint32)sizeof(PostQueueResponse)))
	{
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(PostQueueResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(PostQueueResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (ntohl(PostQueueResponse.m_trailingDataLength) == 0)
	{
		printf("Server has no jobs queued for post-processing\n");
	}
	else
	{
		printf("Post-Processing List\n");
		printf("-----------------------------------\n");

		// rust/src/binrpc.rs: the lines of all the entries that fit the data
		RS_PRINT_LISTING(nzbget_rs_binrpc_post_queue_listing(buf, buf.Size(), ntohl(PostQueueResponse.m_nrTrailingEntries)));

		printf("-----------------------------------\n");
	}

	return true;
}

inline bool RemoteClient::RequestWriteLog(int kind, const char* text)
{
	if (!InitConnection()) return false;

	SNzbWriteLogRequest WriteLogRequest = {};
	InitMessageBase(&WriteLogRequest.m_messageBase, rrWriteLog, sizeof(WriteLogRequest));
	WriteLogRequest.m_kind = htonl(kind);
	int length = strlen(text) + 1;
	WriteLogRequest.m_trailingDataLength = htonl(length);

	if (!m_connection->Send((char*)(&WriteLogRequest), sizeof(WriteLogRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	m_connection->Send(text, length);
	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();
	return OK;
}

inline bool RemoteClient::RequestScan(bool syncMode)
{
	if (!InitConnection()) return false;

	SNzbScanRequest ScanRequest = {};
	InitMessageBase(&ScanRequest.m_messageBase, rrScan, sizeof(ScanRequest));

	ScanRequest.m_syncMode = htonl(syncMode);

	bool OK = m_connection->Send((char*)(&ScanRequest), sizeof(ScanRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

inline bool RemoteClient::RequestHistory(bool withHidden)
{
	if (!InitConnection()) return false;

	SNzbHistoryRequest HistoryRequest = {};
	InitMessageBase(&HistoryRequest.m_messageBase, rrHistory, sizeof(HistoryRequest));
	HistoryRequest.m_hidden = htonl(withHidden);

	if (!m_connection->Send((char*)(&HistoryRequest), sizeof(HistoryRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned list
	SNzbHistoryResponse HistoryResponse;
	bool read = m_connection->Recv((char*) &HistoryResponse, sizeof(HistoryResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(HistoryResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(HistoryResponse.m_messageBase.m_structSize),
		(uint32)sizeof(HistoryResponse)))
	{
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(HistoryResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(HistoryResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (ntohl(HistoryResponse.m_trailingDataLength) == 0)
	{
		printf("Server has no files in history\n");
	}
	else
	{
		printf("History (most recent first)\n");
		printf("-----------------------------------\n");

		// rust/src/binrpc.rs: the lines of all the entries that fit the data
		RS_PRINT_LISTING(nzbget_rs_binrpc_history_listing(buf, buf.Size(), ntohl(HistoryResponse.m_nrTrailingEntries)));

		printf("-----------------------------------\n");
		printf("Items: %i\n", ntohl(HistoryResponse.m_nrTrailingEntries));
	}

	return true;
}

#endif

#include "nzbget_rs.h"

class Frontend : public Thread
{
public:
	Frontend()
	{
		debug("Creating Frontend");

		m_workStateObserver.m_owner = this;
		g_WorkState->Attach(&m_workStateObserver);

		m_updateInterval = g_Options->GetUpdateInterval();
	}

protected:
	bool m_summary = false;
	bool m_fileList = false;
	uint32 m_neededLogEntries = 0;
	uint32 m_neededLogFirstId = 0;
	int m_updateInterval;

	// summary
	int64 m_currentDownloadSpeed = 0;
	int64 m_remainingSize = 0;
	bool m_pauseDownload = false;
	int m_downloadLimit = 0;
	int m_threadCount = 0;
	int m_postJobCount = 0;
	int m_upTimeSec = 0;
	int m_dnTimeSec = 0;
	int64 m_allBytes = 0;
	bool m_standBy = false;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;

	virtual void Stop()
	{
		Thread::Stop();

		m_waitCond.notify_all();
	}

	bool PrepareData()
	{
		if (IsRemoteMode())
		{
			if (IsStopped())
			{
				return false;
			}
			if (!RequestMessages() || (nzbget_rs_dqh_frontend_should_request_file_list(
				m_summary ? 1 : 0, m_fileList ? 1 : 0) != 0 && !RequestFileList()))
			{
				const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());
				printf("\nUnable to send request to RustyGet server at %s (port %i)    \n", controlIp, g_Options->GetControlPort());
				Stop();
				return false;
			}
		}
		else
		{
			if (m_summary)
			{
				m_currentDownloadSpeed = g_StatMeter->CalcCurrentDownloadSpeed();
				m_pauseDownload = g_WorkState->GetPauseDownload();
				m_downloadLimit = g_WorkState->GetSpeedLimit();
				m_threadCount = Thread::GetThreadCount();
				g_StatMeter->CalcTotalStat(&m_upTimeSec, &m_dnTimeSec, &m_allBytes, &m_standBy);

				GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
				m_postJobCount = 0;
				for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
				{
					m_postJobCount += nzbInfo->GetPostInfo() ? 1 : 0;
				}
				downloadQueue->CalcRemainingSize(&m_remainingSize, nullptr);
			}
		}
		return true;
	}

	void FreeData()
	{
		if (IsRemoteMode())
		{
			m_remoteMessages.clear();
			DownloadQueue::Guard()->GetQueue()->clear();
		}
	}

	GuardedMessageList GuardMessages()
	{
		if (IsRemoteMode())
		{
			return GuardedMessageList(&m_remoteMessages, nullptr);
		}
		else
		{
			return g_Log->GuardMessages();
		}
	}

	bool IsRemoteMode()
	{
		return g_Options->GetRemoteClientMode();
	}

	void InitMessageBase(SNzbRequestBase* messageBase, int request, int size)
	{
		nzbget_rs_binrpc_init_message_base(
			(uint8_t*)messageBase,
			request,
			size,
			g_Options->GetControlUsername(),
			g_Options->GetControlPassword());
	}

	void ServerPauseUnpause(bool pause)
	{
		if (IsRemoteMode())
		{
			RequestPauseUnpause(pause);
		}
		else
		{
			g_WorkState->SetResumeTime(0);
			g_WorkState->SetPauseDownload(pause);
		}
	}

	bool RequestPauseUnpause(bool pause)
	{
		RemoteClient client;
		client.SetVerbose(false);
		return client.RequestServerPauseUnpause(pause, rpDownload);
	}

	void ServerSetDownloadRate(int rate)
	{
		if (IsRemoteMode())
		{
			RequestSetDownloadRate(rate);
		}
		else
		{
			g_WorkState->SetSpeedLimit(rate);
		}
	}

	bool RequestSetDownloadRate(int rate)
	{
		RemoteClient client;
		client.SetVerbose(false);
		return client.RequestServerSetDownloadRate(rate);
	}

	bool ServerEditQueue(DownloadQueue::EEditAction action, int offset, int id)
	{
		if (IsRemoteMode())
		{
			return RequestEditQueue(action, offset, id);
		}
		else
		{
			NzbgetRsBuf offBuf = nzbget_rs_dqh_conn_format_port_str(offset);
			bool ok = DownloadQueue::Guard()->EditEntry(id, action, offBuf.data ? offBuf.data : "0");
			nzbget_rs_free(offBuf);
			return ok;
		}
	}

	bool RequestEditQueue(DownloadQueue::EEditAction action, int offset, int id)
	{
		RemoteClient client;
		client.SetVerbose(false);
		IdList ids = { id };
		return client.RequestServerEditQueue(action, offset, nullptr, &ids, nullptr, rmId);
	}

	void Wait(int milliseconds)
	{
		if (g_WorkState->GetPauseFrontend())
		{
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait_for(lk, std::chrono::milliseconds(2000));
		}
		else
		{
			Util::Sleep(milliseconds);
		}
	}

private:
	class WorkStateObserver : public Observer
	{
	public:
		Frontend* m_owner;
		virtual void Update(Subject* caller, void* aspect) { m_owner->WorkStateUpdate(caller, aspect); }
	};

	MessageList m_remoteMessages;
	WorkStateObserver m_workStateObserver;

	bool RequestMessages()
	{
		const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());
		Connection connection(controlIp, g_Options->GetControlPort(), false);

		bool OK = connection.Connect();
		if (!OK)
		{
			return false;
		}

		SNzbLogRequest LogRequest = {};
		InitMessageBase(&LogRequest.m_messageBase, rrLog, sizeof(LogRequest));
		LogRequest.m_lines = htonl(m_neededLogEntries);
		LogRequest.m_idFrom = htonl(nzbget_rs_sfw_frontend_resolve_log_id_from(
			m_neededLogEntries, m_neededLogFirstId));

		if (!connection.Send((char*)(&LogRequest), sizeof(LogRequest)))
		{
			return false;
		}

		// Now listen for the returned log
		SNzbLogResponse LogResponse;
		bool read = connection.Recv((char*) &LogResponse, sizeof(LogResponse));
		if (!nzbget_rs_sfw_rpc_is_response_header_valid(
			read ? 1 : 0,
			ntohl(LogResponse.m_messageBase.m_signature),
			(uint32_t)NZBMESSAGE_SIGNATURE,
			ntohl(LogResponse.m_messageBase.m_structSize),
			(uint32_t)sizeof(LogResponse)))
		{
			return false;
		}

		CharBuffer buf;
		if (ntohl(LogResponse.m_trailingDataLength) > 0)
		{
			buf.Reserve(ntohl(LogResponse.m_trailingDataLength));
			if (!connection.Recv(buf, buf.Size()))
			{
				return false;
			}
		}

		connection.Disconnect();

		if (ntohl(LogResponse.m_trailingDataLength) > 0)
		{
			nzbget_rs_binrpc_decode_log_response_entries(
				(const uint8_t*)(const char*)buf,
				(size_t)buf.Size(),
				ntohl(LogResponse.m_nrTrailingEntries),
				&m_remoteMessages,
				[](void* rawCtx, uint32_t id, uint32_t kind, uint32_t time, const char* text) {
					static_cast<MessageList*>(rawCtx)->emplace_back(
						id, (Message::EKind)kind, (time_t)time, text ? text : "");
				});
		}

		return true;
	}

	bool RequestFileList()
	{
		const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());
		Connection connection(controlIp, g_Options->GetControlPort(), false);

		bool OK = connection.Connect();
		if (!OK)
		{
			return false;
		}

		SNzbListRequest ListRequest = {};
		InitMessageBase(&ListRequest.m_messageBase, rrList, sizeof(ListRequest));
		ListRequest.m_fileList = htonl(m_fileList);
		ListRequest.m_serverState = htonl(m_summary);

		if (!connection.Send((char*)(&ListRequest), sizeof(ListRequest)))
		{
			return false;
		}

		// Now listen for the returned list
		SNzbListResponse ListResponse;
		bool read = connection.Recv((char*) &ListResponse, sizeof(ListResponse));
		if (!nzbget_rs_sfw_rpc_is_response_header_valid(
			read ? 1 : 0,
			ntohl(ListResponse.m_messageBase.m_signature),
			(uint32_t)NZBMESSAGE_SIGNATURE,
			ntohl(ListResponse.m_messageBase.m_structSize),
			(uint32_t)sizeof(ListResponse)))
		{
			return false;
		}

		CharBuffer buf;
		if (ntohl(ListResponse.m_trailingDataLength) > 0)
		{
			buf.Reserve(ntohl(ListResponse.m_trailingDataLength));
			if (!connection.Recv(buf, buf.Size()))
			{
				return false;
			}
		}

		connection.Disconnect();

		if (m_summary)
		{
			m_pauseDownload = ntohl(ListResponse.m_downloadPaused);
			m_remainingSize = Util::JoinInt64(ntohl(ListResponse.m_remainingSizeHi), ntohl(ListResponse.m_remainingSizeLo));
			m_currentDownloadSpeed = ntohl(ListResponse.m_downloadRate);
			m_downloadLimit = ntohl(ListResponse.m_downloadLimit);
			m_threadCount = ntohl(ListResponse.m_threadCount);
			m_postJobCount = ntohl(ListResponse.m_postJobCount);
			m_upTimeSec = ntohl(ListResponse.m_upTimeSec);
			m_dnTimeSec = ntohl(ListResponse.m_downloadTimeSec);
			m_standBy = ntohl(ListResponse.m_downloadStandBy);
			m_allBytes = Util::JoinInt64(ntohl(ListResponse.m_downloadedBytesHi), ntohl(ListResponse.m_downloadedBytesLo));
		}

		if (nzbget_rs_dqh_frontend_should_build_remote_file_list(
			m_fileList ? 1 : 0, ntohl(ListResponse.m_trailingDataLength)) != 0)
		{
			RemoteClient client;
			client.SetVerbose(false);

			client.BuildFileList(&ListResponse, buf, DownloadQueue::Guard());
		}

		return true;
	}

	void WorkStateUpdate(Subject*, void*)
	{
		m_waitCond.notify_all();
	}
};

class LoggableFrontend : public Frontend
{
protected:
	virtual void Run()
	{
		debug("Entering LoggableFrontend-loop");

		while (!IsStopped())
		{
			Update();
			Wait(m_updateInterval);
		}
		Update();

		BeforeExit();

		debug("Exiting LoggableFrontend-loop");
	}

	virtual void Update()
	{
		if (!PrepareData())
		{
			FreeData();
			return;
		}

		BeforePrint();

		{
			GuardedMessageList messages = GuardMessages();
			if (!messages->empty())
			{
				Message& firstMessage = messages->front();
				int skip = 0;
				int start = nzbget_rs_feh_frontend_calc_log_start_index(
					m_neededLogFirstId, firstMessage.GetId(), &skip);
				if (skip)
				{
					PrintSkip();
				}
				for (uint32 i = (uint32)start; i < messages->size(); i++)
				{
					PrintMessage(messages->at(i));
					m_neededLogFirstId = messages->at(i).GetId();
				}
			}
		}

		PrintStatus();

		FreeData();

		fflush(stdout);
	}

	virtual void BeforePrint() {}
	virtual void BeforeExit() {}

	virtual void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			0, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	virtual void PrintStatus() {}

	virtual void PrintSkip()
	{
		printf(".....\n");
	}
};

class ColoredFrontend : public LoggableFrontend
{
public:
	ColoredFrontend()
	{
		m_summary = true;
	}

protected:
	virtual void BeforePrint()
	{
		if (m_needGoBack)
		{
			printf("\r\033[1A");
			m_needGoBack = false;
		}
	}

	virtual void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			1, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	virtual void PrintStatus()
	{
		printf("\033[s");
		NzbgetRsBuf status = nzbget_rs_colored_frontend_format_status_bar(
			m_standBy ? 1 : 0,
			m_pauseDownload ? 1 : 0,
			m_currentDownloadSpeed,
			m_remainingSize,
			m_downloadLimit,
			m_postJobCount,
			m_threadCount,
			"\033[K");
		if (status.data)
		{
			fwrite(status.data, 1, status.len, stdout);
		}
		nzbget_rs_free(status);
		m_needGoBack = true;
	}

	virtual void PrintSkip()
	{
		printf(".....\033[K\n");
	}

	virtual void BeforeExit()
	{
		if (IsRemoteMode())
		{
			printf("\n");
		}
	}

private:
	bool m_needGoBack = false;
};


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef NCURSESFRONTEND_H
#define NCURSESFRONTEND_H

#ifndef DISABLE_CURSES

#include <string>

class NCursesFrontend : public Frontend
{
public:
	NCursesFrontend();
	virtual ~NCursesFrontend();

protected:
	virtual void Run();

private:
	enum EInputMode
	{
		normal,
		editQueue,
		downloadRate
	};

	bool m_useColor = true;
	int m_dataUpdatePos = 0;
	bool m_updateNextTime = false;
	int m_screenHeight = 0;
	int m_screenWidth = 0;
	int m_queueWinTop = 0;
	int m_queueWinHeight = 0;
	int m_queueWinClientHeight = 0;
	int m_messagesWinTop = 0;
	int m_messagesWinHeight = 0;
	int m_messagesWinClientHeight = 0;
	int m_selectedQueueEntry = 0;
	int m_lastEditEntry = -1;
	bool m_lastPausePars = false;
	int m_queueScrollOffset = 0;
	std::string m_hint;
	time_t m_startHint;
	int m_colWidthFiles;
	int m_colWidthTotal;
	int m_colWidthLeft;

	// Inputting numbers
	int m_inputNumberIndex = 0;
	int m_inputValue;

	void* m_window; // WINDOW*

	EInputMode m_inputMode = normal;
	bool m_showNzbname;
	bool m_showTimestamp;
	bool m_groupFiles;
	int m_queueWindowPercentage = 50;

	void PlotText(const char*  string, int row, int pos, int colorPair, bool blink);
	void PlotLine(const char*  string, int row, int pos, int colorPair);
	void PrintMessages();
	void PrintQueue();
	void PrintFileQueue();
	void PrintFilename(FileInfo* fileInfo, int row, bool selected);
	void PrintGroupQueue();
	void ResetColWidths();
	void PrintGroupname(NzbInfo* nzbInfo, int row, bool selected, bool calcColWidth);
	void PrintTopHeader(char* header, int lineNr, bool upTime);
	int PrintMessage(Message& msg, int row, int maxLines);
	void PrintKeyInputBar();
	void PrintStatus();
	void UpdateInput(int initialKey);
	void Update(int key);
	void SetCurrentQueueEntry(int entry);
	void CalcWindowSizes();
	void RefreshScreen();
	int ReadConsoleKey();
	int CalcQueueSize();
	void NeedUpdateData();
	bool EditQueue(DownloadQueue::EEditAction action, int offset);
	void SetHint(const char* hint);
};

#endif

#ifndef DISABLE_CURSES

#include "nzbget_rs.h"

inline void curses_clear()
{
	nzbget_rs_term_clear();
}

extern void ExitProc();

static const int NCURSES_COLORPAIR_TEXT		= 1;
static const int NCURSES_COLORPAIR_INFO		= 2;
static const int NCURSES_COLORPAIR_WARNING		= 3;
static const int NCURSES_COLORPAIR_ERROR		= 4;
static const int NCURSES_COLORPAIR_DEBUG		= 5;
static const int NCURSES_COLORPAIR_DETAIL		= 6;
static const int NCURSES_COLORPAIR_STATUS		= 7;
static const int NCURSES_COLORPAIR_KEYBAR		= 8;
static const int NCURSES_COLORPAIR_INFOLINE	= 9;
static const int NCURSES_COLORPAIR_TEXTHIGHL 	= 10;
static const int NCURSES_COLORPAIR_CURSOR		= 11;
static const int NCURSES_COLORPAIR_HINT		= 12;

static const int MAX_SCREEN_WIDTH				= 512;

static const int READKEY_EMPTY = -1;
static const int KEY_DOWN      = 258;
static const int KEY_UP        = 259;
static const int KEY_HOME      = 262;
static const int KEY_BACKSPACE = 263;
static const int KEY_NPAGE     = 338;
static const int KEY_PPAGE     = 339;
static const int KEY_END       = 360;

inline NCursesFrontend::NCursesFrontend()
	: m_startHint(0)
	, m_colWidthFiles(0)
	, m_colWidthTotal(0)
	, m_colWidthLeft(0)
	, m_inputValue(0)
{
	m_summary = true;
	m_fileList = true;

	m_showNzbname = g_Options->GetCursesNzbName();
	m_showTimestamp = g_Options->GetCursesTime();
	m_groupFiles = g_Options->GetCursesGroup();

	int colorSupport = nzbget_rs_term_init();
	m_window = reinterpret_cast<void*>(1);
	m_useColor = colorSupport > 0;
}

inline NCursesFrontend::~NCursesFrontend()
{
	nzbget_rs_term_shutdown();
	printf("\n");
	SetHint(nullptr);
}

inline void NCursesFrontend::Run()
{
	debug("Entering NCursesFrontend-loop");

	m_dataUpdatePos = 0;

	while (!IsStopped())
	{
		// The data (queue and log) is updated each m_iUpdateInterval msec,
		// but the window is updated more often for better reaction on user's input

		bool updateNow = false;
		int key = ReadConsoleKey();

		int updateNext = m_updateNextTime ? 1 : 0;
		updateNow = nzbget_rs_sfw_ncurses_decide_loop_update(
			key != READKEY_EMPTY ? 1 : 0, &updateNext, m_dataUpdatePos) != 0;
		m_updateNextTime = updateNext != 0;

		if (updateNow)
		{
			Update(key);
		}

		if (m_dataUpdatePos <= 0)
		{
			m_dataUpdatePos = m_updateInterval;
		}

		// update more often (sleep shorter) if need faster reaction on user input
		int sleepInterval = nzbget_rs_sfw_ncurses_select_sleep_interval(m_inputMode == normal ? 1 : 0);
		Wait(sleepInterval);
		m_dataUpdatePos -= sleepInterval;
	}

	FreeData();

	debug("Exiting NCursesFrontend-loop");
}

inline void NCursesFrontend::NeedUpdateData()
{
	m_dataUpdatePos = 10;
	m_updateNextTime = true;
}

inline void NCursesFrontend::Update(int key)
{
	// Figure out how big the screen is
	CalcWindowSizes();

	if (m_dataUpdatePos <= 0)
	{
		FreeData();
		m_neededLogEntries = m_messagesWinClientHeight;
		if (!PrepareData())
		{
			return;
		}

		// recalculate frame sizes
		CalcWindowSizes();
	}

	if (m_inputMode == editQueue)
	{
		int queueSize = CalcQueueSize();
		if (nzbget_rs_dqh_ncurses_should_reset_empty_edit_queue(1, queueSize) != 0)
		{
			m_selectedQueueEntry = 0;
			m_inputMode = normal;
		}
	}

	//------------------------------------------
	// Print Current NZBInfoList
	//------------------------------------------
	if (m_queueWinHeight > 0)
	{
		PrintQueue();
	}

	//------------------------------------------
	// Print Messages
	//------------------------------------------
	if (m_messagesWinHeight > 0)
	{
		PrintMessages();
	}

	PrintStatus();

	PrintKeyInputBar();

	UpdateInput(key);

	RefreshScreen();
}

inline void NCursesFrontend::CalcWindowSizes()
{
	int nrRows = 24, nrColumns = 80;
	nzbget_rs_term_get_size(&nrRows, &nrColumns);
	// the line buffers hold MAX_SCREEN_WIDTH columns (a wider terminal wrote past
	// them), and messages wrap at the width less 8 (a divisor of zero at 8)
	int queueSize = CalcQueueSize();
	int qh = 0, qch = 0, mtop = 0, mh = 0, mch = 0;
	nzbget_rs_feh_ncurses_calc_window_sizes(
		nrRows, nrColumns, MAX_SCREEN_WIDTH, m_queueWindowPercentage, queueSize,
		&nrColumns, &qh, &qch, &mtop, &mh, &mch);
	if (nrRows != m_screenHeight || nrColumns != m_screenWidth)
	{
		curses_clear();
		m_screenHeight = nrRows;
		m_screenWidth = nrColumns;
	}
	m_queueWinTop = 0;
	m_queueWinHeight = qh;
	m_queueWinClientHeight = qch;
	m_messagesWinTop = mtop;
	m_messagesWinHeight = mh;
	m_messagesWinClientHeight = mch;
}

inline int NCursesFrontend::CalcQueueSize()
{
	int queueSize = 0;
	if (m_groupFiles)
	{
		queueSize = DownloadQueue::Guard()->GetQueue()->size();
	}
	else
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			queueSize += nzbInfo->GetFileList()->size();
		}
	}
	return queueSize;
}

inline void NCursesFrontend::PlotLine(const char* str, int row, int pos, int colorPair)
{
	NzbgetRsBuf lineBuf = nzbget_rs_dqh_ncurses_format_plot_line(
		str, m_screenWidth, pos, MAX_SCREEN_WIDTH);
	PlotText(lineBuf.data ? lineBuf.data : "", row, pos, colorPair, false);
	nzbget_rs_free(lineBuf);
}

inline void NCursesFrontend::PlotText(const char * string, int row, int pos, int colorPair, bool blink)
{
	nzbget_rs_term_plot_text(string, row, pos, m_useColor ? 1 : 0, colorPair, blink ? 1 : 0);
}

inline void NCursesFrontend::RefreshScreen()
{
	nzbget_rs_term_refresh(m_screenHeight, m_screenWidth);
}

inline void NCursesFrontend::PrintMessages()
{
	int lineNr = m_messagesWinTop;

	PlotLine(
		nzbget_rs_tpr_ncurses_format_messages_header(m_useColor ? 1 : 0),
		lineNr++, 0, NCURSES_COLORPAIR_INFOLINE);

	int line = lineNr + m_messagesWinClientHeight - 1;
	int linesToPrint = m_messagesWinClientHeight;

	GuardedMessageList messages = GuardMessages();

	// print messages from bottom
	for (int i = (int)messages->size() - 1; i >= 0 && linesToPrint > 0; i--)
	{
		int printedLines = PrintMessage(messages->at(i), line, linesToPrint);
		line -= printedLines;
		linesToPrint -= printedLines;
	}

	if (linesToPrint > 0)
	{
		// too few messages, print them again from top
		line = lineNr + m_messagesWinClientHeight - 1;
		while (linesToPrint-- > 0)
		{
			PlotLine("", line--, 0, NCURSES_COLORPAIR_TEXT);
		}
		int linesToPrint2 = m_messagesWinClientHeight;
		for (int i = (int)messages->size() - 1; i >= 0 && linesToPrint2 > 0; i--)
		{
			int printedLines = PrintMessage(messages->at(i), line, linesToPrint2);
			line -= printedLines;
			linesToPrint2 -= printedLines;
		}
	}
}

inline int NCursesFrontend::PrintMessage(Message& msg, int row, int maxLines)
{
	CString text;

	if (m_showTimestamp)
	{
		time_t rawtime = msg.GetTime() + g_Options->GetTimeCorrection();
		NzbgetRsBuf tsBuf = nzbget_rs_tpr_ncurses_format_timestamp_message(
			*Util::FormatTime(rawtime), msg.GetText());
		text = tsBuf.data ? tsBuf.data : "";
		nzbget_rs_free(tsBuf);
	}
	else
	{
		text = msg.GetText();
	}

	// replace some special characters with spaces
	int len = (int)strlen(text);
	nzbget_rs_ccm_ncurses_sanitize_message_chars((char*)text, (size_t)len);
	int winWidth = 0, msgLines = 0;
	nzbget_rs_ccm_ncurses_calc_msg_lines(len, m_screenWidth, &winWidth, &msgLines);

	int lines = 0;
	for (int i = msgLines - 1; i >= 0 && lines < maxLines; i--)
	{
		int r = row - msgLines + i + 1;
		PlotLine(text + winWidth * i, r, 8, NCURSES_COLORPAIR_TEXT);
		PlotText(
			nzbget_rs_dqh_ncurses_message_type_label((int)msg.GetKind(), i == 0 ? 1 : 0),
			r, 0,
			nzbget_rs_dqh_ncurses_message_color_pair((int)msg.GetKind()),
			false);
		lines++;
	}

	return lines;
}

inline void NCursesFrontend::PrintStatus()
{
	int statusRow = m_screenHeight - 2;

	NzbgetRsNcursesStatusSpec spec{};
	spec.stand_by = m_standBy ? 1 : 0;
	spec.pause_download = m_pauseDownload ? 1 : 0;
	spec.current_download_speed = m_currentDownloadSpeed;
	spec.remaining_size = m_remainingSize;
	spec.download_limit = m_downloadLimit;
	spec.post_job_count = m_postJobCount;
	spec.dn_time_sec = m_dnTimeSec;
	spec.all_bytes = m_allBytes;
	spec.thread_count = m_threadCount;

	NzbgetRsBuf buf = nzbget_rs_ncurses_format_status_bar(&spec);
	if (buf.data)
	{
		PlotLine(buf.data, statusRow, 0, NCURSES_COLORPAIR_STATUS);
	}
	nzbget_rs_free(buf);
}

inline void NCursesFrontend::PrintKeyInputBar()
{
	int queueSize = CalcQueueSize();
	int inputBarRow = m_screenHeight - 1;

	if (!m_hint.empty())
	{
		time_t time = Util::CurrentTime();
		if (nzbget_rs_sfw_ncurses_is_hint_active((int64_t)time, (int64_t)m_startHint) != 0)
		{
			PlotLine(m_hint.c_str(), inputBarRow, 0, NCURSES_COLORPAIR_HINT);
			return;
		}
		else
		{
			SetHint(nullptr);
		}
	}

	switch (m_inputMode)
	{
	case normal:
		PlotLine(
			nzbget_rs_sfw_ncurses_select_normal_keybar(m_groupFiles ? 1 : 0),
			inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		break;
	case editQueue:
	{
		const char* status = nzbget_rs_ccm_ncurses_select_edit_queue_keybar(
			m_selectedQueueEntry, queueSize);

		PlotLine(status, inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		break;
	}
	case downloadRate:
	{
		NzbgetRsBuf hintBuf = nzbget_rs_tpr_ncurses_format_download_rate_hint(m_inputValue);
		PlotLine(hintBuf.data ? hintBuf.data : "", inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		nzbget_rs_free(hintBuf);

		// Print the cursor
		PlotText(" ", inputBarRow, 15 + m_inputNumberIndex, NCURSES_COLORPAIR_CURSOR, true);
		break;
	}
	}
}

inline void NCursesFrontend::SetHint(const char* hint)
{
	if (hint) m_hint = hint;
	if (!m_hint.empty()) m_startHint = Util::CurrentTime();
}

inline void NCursesFrontend::PrintQueue()
{
	if (m_groupFiles)
	{
		PrintGroupQueue();
	}
	else
	{
		PrintFileQueue();
	}
}

inline void NCursesFrontend::PrintFileQueue()
{
	int lineNr = m_queueWinTop + 1;
	int64 remaining = 0;
	int64 paused = 0;
	int pausedFiles = 0;
	int fileNum = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (nzbget_rs_dqh_ncurses_is_row_in_scroll_window(
				fileNum, m_queueScrollOffset, m_queueWinHeight) != 0)
			{
				PrintFilename(fileInfo, lineNr++, fileNum == m_selectedQueueEntry);
			}
			fileNum++;

			nzbget_rs_dqh_ncurses_accum_file_queue_stats(
				fileInfo->GetPaused() ? 1 : 0,
				fileInfo->GetRemainingSize(),
				&remaining,
				&paused,
				&pausedFiles);
		}
	}

	NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_file_queue_header(
		m_useColor ? 1 : 0, fileNum, pausedFiles, remaining, paused);
	if (fileNum > 0)
	{
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, m_queueWinTop, true);
		}
		nzbget_rs_free(headerBuf);
	}
	else
	{
		lineNr--;
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, lineNr++, true);
		}
		nzbget_rs_free(headerBuf);
		PlotLine("Ready to receive nzb-job", lineNr++, 0, NCURSES_COLORPAIR_TEXT);
	}
}

inline void NCursesFrontend::PrintFilename(FileInfo * fileInfo, int row, bool selected)
{
	NzbgetRsNcursesFileRowSpec spec{};
	spec.edit_queue_mode = m_inputMode == editQueue ? 1 : 0;
	spec.selected = selected ? 1 : 0;
	spec.use_color = m_useColor ? 1 : 0;
	spec.color_highl = NCURSES_COLORPAIR_TEXTHIGHL;
	spec.color_text = NCURSES_COLORPAIR_TEXT;
	spec.id = fileInfo->GetId();
	spec.priority = fileInfo->GetNzbInfo()->GetPriority();
	spec.active_downloads = fileInfo->GetActiveDownloads();
	spec.paused = fileInfo->GetPaused() ? 1 : 0;
	spec.show_nzbname = m_showNzbname ? 1 : 0;
	spec.path_sep = (uint8_t)PATH_SEPARATOR;
	spec.nzb_name = fileInfo->GetNzbInfo()->GetName();
	spec.filename = fileInfo->GetFilename();
	spec.size = fileInfo->GetSize();
	spec.remaining_size = fileInfo->GetRemainingSize();

	int color = NCURSES_COLORPAIR_TEXT;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_file_row(&spec, &color);
	if (buf.data)
	{
		PlotLine(buf.data, row, 0, color);
	}
	nzbget_rs_free(buf);
}

inline void NCursesFrontend::PrintTopHeader(char* header, int lineNr, bool upTime)
{
	int time = upTime ? m_upTimeSec : m_dnTimeSec;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_top_header(
		header, m_screenWidth, upTime ? 1 : 0, time);
	if (buf.data)
	{
		PlotLine(buf.data, lineNr, 0, NCURSES_COLORPAIR_INFOLINE);
	}
	nzbget_rs_free(buf);
}

inline void NCursesFrontend::PrintGroupQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	int lineNr = m_queueWinTop;

	if (downloadQueue->GetQueue()->empty())
	{
		NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_group_queue_header(
			m_useColor ? 1 : 0, 0, 0, 0);
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, lineNr++, false);
		}
		nzbget_rs_free(headerBuf);
		PlotLine("Ready to receive nzb-job", lineNr++, 0, NCURSES_COLORPAIR_TEXT);
	}
	else
	{
		lineNr++;

		ResetColWidths();
		int calcLineNr = lineNr;
		int i = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dqh_ncurses_is_row_in_scroll_window(
				i, m_queueScrollOffset, m_queueWinHeight) != 0)
			{
				PrintGroupname(nzbInfo, calcLineNr++, false, true);
			}
			i++;
		}

		int64 remaining = 0;
		int64 paused = 0;
		i = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dqh_ncurses_is_row_in_scroll_window(
				i, m_queueScrollOffset, m_queueWinHeight) != 0)
			{
				PrintGroupname(nzbInfo, lineNr++, i == m_selectedQueueEntry, false);
			}
			i++;
			nzbget_rs_dqh_ncurses_accum_group_queue_stats(
				nzbInfo->GetRemainingSize(),
				nzbInfo->GetPausedSize(),
				&remaining,
				&paused);
		}

		NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_group_queue_header(
			m_useColor ? 1 : 0, (int)downloadQueue->GetQueue()->size(), remaining, paused);
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, m_queueWinTop, false);
		}
		nzbget_rs_free(headerBuf);
	}
}

inline void NCursesFrontend::ResetColWidths()
{
	m_colWidthFiles = 0;
	m_colWidthTotal = 0;
	m_colWidthLeft = 0;
}

inline void NCursesFrontend::PrintGroupname(NzbInfo* nzbInfo, int row, bool selected, bool calcColWidth)
{
	NzbgetRsNcursesGroupRowSpec spec{};
	spec.edit_queue_mode = m_inputMode == editQueue ? 1 : 0;
	spec.selected = selected ? 1 : 0;
	spec.use_color = m_useColor ? 1 : 0;
	spec.color_highl = NCURSES_COLORPAIR_TEXTHIGHL;
	spec.color_text = NCURSES_COLORPAIR_TEXT;
	spec.screen_width = m_screenWidth;
	spec.calc_col_width = calcColWidth ? 1 : 0;
	spec.id = nzbInfo->GetId();
	spec.priority = nzbInfo->GetPriority();
	spec.active_downloads = nzbInfo->GetActiveDownloads();
	spec.name = nzbInfo->GetName();
	spec.file_count = (int)nzbInfo->GetFileList()->size();
	spec.paused_file_count = nzbInfo->GetPausedFileCount();
	spec.size = nzbInfo->GetSize();
	spec.remaining_size = nzbInfo->GetRemainingSize();
	spec.paused_size = nzbInfo->GetPausedSize();
	spec.stand_by = m_standBy ? 1 : 0;
	spec.pause_download = m_pauseDownload ? 1 : 0;
	spec.current_download_speed = m_currentDownloadSpeed;

	int color = NCURSES_COLORPAIR_TEXT;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_group_row(
		&spec, &m_colWidthFiles, &m_colWidthTotal, &m_colWidthLeft, &color);
	if (!calcColWidth && buf.data)
	{
		PlotLine(buf.data, row, 0, color);
	}
	nzbget_rs_free(buf);
}

inline bool NCursesFrontend::EditQueue(DownloadQueue::EEditAction action, int offset)
{
	int ID = 0;

	if (m_groupFiles)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (m_selectedQueueEntry >= 0 && m_selectedQueueEntry < (int)downloadQueue->GetQueue()->size())
		{
			std::unique_ptr<NzbInfo>& nzbInfo = downloadQueue->GetQueue()->at(m_selectedQueueEntry);
			ID = nzbInfo->GetId();
			int newLastPausePars = m_lastPausePars ? 1 : 0;
			action = (DownloadQueue::EEditAction)nzbget_rs_feh_ncurses_resolve_group_pause_action(
				(int)action, nzbInfo->GetRemainingSize(), nzbInfo->GetPausedSize(),
				nzbInfo->GetRemainingParCount(), m_lastPausePars ? 1 : 0,
				m_lastEditEntry, m_selectedQueueEntry, &newLastPausePars);
			m_lastPausePars = newLastPausePars != 0;
		}

		// map file-edit-actions to group-edit-actions
		action = (DownloadQueue::EEditAction)nzbget_rs_tpr_ncurses_map_file_to_group_action((int)action);
	}
	else
	{
		int fileNum = 0;
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (m_selectedQueueEntry == fileNum)
				{
					ID = fileInfo->GetId();
					action = (DownloadQueue::EEditAction)nzbget_rs_tpr_ncurses_resolve_file_pause_action(
						(int)action, (int)DownloadQueue::eaFilePause,
						(int)DownloadQueue::eaFileResume, fileInfo->GetPaused() ? 1 : 0);
				}
				fileNum++;
			}
		}
	}

	m_lastEditEntry = m_selectedQueueEntry;

	NeedUpdateData();

	if (ID != 0)
	{
		return ServerEditQueue(action, offset, ID);
	}
	else
	{
		return false;
	}
}

inline void NCursesFrontend::SetCurrentQueueEntry(int entry)
{
	int queueSize = CalcQueueSize();

	nzbget_rs_feh_ncurses_calc_scroll_and_selection(
		entry, queueSize, m_queueScrollOffset, m_queueWinClientHeight,
		&m_selectedQueueEntry, &m_queueScrollOffset);
}


/*
 * Process keystrokes starting with the initialKey, which must not be
 * READKEY_EMPTY but has alread been set via ReadConsoleKey.
 */
inline void NCursesFrontend::UpdateInput(int initialKey)
{
	int key = initialKey;
	while (key != READKEY_EMPTY)
	{
		int queueSize = CalcQueueSize();

		// Normal or edit queue mode
		if (m_inputMode == normal || m_inputMode == editQueue)
		{
			switch (key)
			{
			case 'q':
				// Key 'q' for quit
				ExitProc();
				break;
			case 'z':
				// show/hide NZBFilename
				m_showNzbname = !m_showNzbname;
				break;
			case 'w':
				// swicth window sizes
				m_queueWindowPercentage = nzbget_rs_ccm_ncurses_next_queue_win_pct(
					m_queueWindowPercentage, m_inputMode == editQueue ? 1 : 0);
				CalcWindowSizes();
				SetCurrentQueueEntry(m_selectedQueueEntry);
				break;
			case 'g':
				// group/ungroup files
				m_groupFiles = !m_groupFiles;
				SetCurrentQueueEntry(m_selectedQueueEntry);
				NeedUpdateData();
				break;
			}
		}

		// Normal mode
		if (m_inputMode == normal)
		{
			switch (key)
			{
			case 'p':
				// Key 'p' for pause
				if (!IsRemoteMode())
				{
					info("%s", nzbget_rs_tpr_ncurses_pause_log_msg(m_pauseDownload ? 1 : 0));
				}
				ServerPauseUnpause(!m_pauseDownload);
				break;
			case 'e':
			case 10: // return
			case 13: // enter
			{
				int nextWinPct = m_queueWindowPercentage;
				if (nzbget_rs_dqh_ncurses_enter_edit_queue_win_pct(
					queueSize, m_queueWindowPercentage, &nextWinPct) != 0)
				{
					m_inputMode = editQueue;
					m_queueWindowPercentage = nextWinPct;
					return;
				}
				break;
			}
			case 'r':
				// Download rate
				m_inputMode = downloadRate;
				m_inputNumberIndex = 0;
				m_inputValue = 0;
				return;
			case 't':
				// show/hide Timestamps
				m_showTimestamp = !m_showTimestamp;
				break;
			}
		}

		// Edit Queue mode
		if (m_inputMode == editQueue)
		{
			switch (key)
			{
			case 'e':
			case 10: // return
			case 13: // enter
				m_inputMode = normal;
				return;
			case KEY_DOWN:
				if (m_selectedQueueEntry < queueSize - 1)
				{
					SetCurrentQueueEntry(m_selectedQueueEntry + 1);
				}
				break;
			case KEY_UP:
				if (m_selectedQueueEntry > 0)
				{
					SetCurrentQueueEntry(m_selectedQueueEntry - 1);
				}
				break;
			case KEY_PPAGE:
			case KEY_NPAGE:
			{
				int newScroll = m_queueScrollOffset;
				int newEntry = m_selectedQueueEntry;
				if (nzbget_rs_tpr_ncurses_calc_page_nav(
					key == KEY_NPAGE ? 1 : 0, m_selectedQueueEntry, queueSize,
					m_queueScrollOffset, m_queueWinClientHeight, &newScroll, &newEntry))
				{
					m_queueScrollOffset = newScroll;
					SetCurrentQueueEntry(newEntry);
				}
				break;
			}
			case KEY_HOME:
				SetCurrentQueueEntry(0);
				break;
			case KEY_END:
				SetCurrentQueueEntry(nzbget_rs_dqh_ncurses_last_queue_index(queueSize));
				break;
			case 'p':
				// Key 'p' for pause
				EditQueue(DownloadQueue::eaFilePause, 0);
				break;
			case 'd':
				SetHint(" Use Uppercase \"D\" for delete");
				break;
			case 'D':
				// Delete entry
				if (EditQueue(DownloadQueue::eaFileDelete, 0))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry);
				}
				break;
			case 'u':
				if (EditQueue(DownloadQueue::eaFileMoveOffset, -1))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry - 1);
				}
				break;
			case 'n':
				if (EditQueue(DownloadQueue::eaFileMoveOffset, +1))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry + 1);
				}
				break;
			case 't':
				if (EditQueue(DownloadQueue::eaFileMoveTop, 0))
				{
					SetCurrentQueueEntry(0);
				}
				break;
			case 'b':
				if (EditQueue(DownloadQueue::eaFileMoveBottom, 0))
				{
					SetCurrentQueueEntry(nzbget_rs_dqh_ncurses_last_queue_index(queueSize));
				}
				break;
			}
		}

		// Edit download rate input mode
		if (m_inputMode == downloadRate)
		{
			int rateStep = nzbget_rs_feh_ncurses_step_rate_input(
				key, KEY_BACKSPACE, &m_inputNumberIndex, &m_inputValue);
			if (rateStep == 1)
			{
				ServerSetDownloadRate(nzbget_rs_tpr_webdl_kb_to_bytes(m_inputValue));
				m_inputMode = normal;
				return;
			}
			else if (rateStep == 2)
			{
				m_inputMode = normal;
				return;
			}
		}

		key = ReadConsoleKey();
	}
}

inline int NCursesFrontend::ReadConsoleKey()
{
	return nzbget_rs_term_read_key();
}

#endif

#endif

#endif

class BinRpcProcessor
{
public:
	BinRpcProcessor();
	void Execute();
	void SetConnection(Connection* connection) { m_connection = connection; }

private:
	SNzbRequestBase m_messageBase;
	Connection* m_connection;

	void Dispatch();
};

#include "Util.h"
#include "Util.h"
#include "QueueCoordinator.h"
#include "Util.h"
#include "Util.h"
#include "DiskState.h"
#include "QueueCoordinator.h"
#include "QueueCoordinator.h"
#include "nzbget_rs.h"

extern void ExitProc();
extern void Reload();

inline const uint32 g_MessageRequestSizes[] =
	{ 0,
		sizeof(SNzbDownloadRequest),
		sizeof(SNzbPauseUnpauseRequest),
		sizeof(SNzbListRequest),
		sizeof(SNzbSetDownloadRateRequest),
		sizeof(SNzbDumpDebugRequest),
		sizeof(SNzbEditQueueRequest),
		sizeof(SNzbLogRequest),
		sizeof(SNzbShutdownRequest),
		sizeof(SNzbReloadRequest),
		sizeof(SNzbVersionRequest),
		sizeof(SNzbPostQueueRequest),
		sizeof(SNzbWriteLogRequest),
		sizeof(SNzbScanRequest),
		sizeof(SNzbHistoryRequest)
	};

static_assert(sizeof(g_MessageRequestSizes) / sizeof(g_MessageRequestSizes[0]) == rrHistory + 1);


class BinCommand
{
public:
	virtual ~BinCommand() {}
	virtual void Execute() = 0;
	void SetConnection(Connection* connection) { m_connection = connection; }
	void SetMessageBase(SNzbRequestBase* messageBase) { m_messageBase = messageBase; }

protected:
	Connection* m_connection;
	SNzbRequestBase* m_messageBase;

	bool ReceiveRequest(void* buffer, int size);
	bool ReceiveTrailing(CharBuffer& buffer, int64 len, int64 maxLen);
	void SendBoolResponse(bool success, const char* text);
};

// a fixed-size string field of a request ends within its field
template <size_t N> inline void TerminateField(char (&field)[N]) { field[N - 1] = '\0'; }

class DownloadBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ListBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class LogBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class PauseUnpauseBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class EditQueueBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class SetDownloadRateBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class DumpDebugBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ShutdownBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ReloadBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class VersionBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class PostQueueBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class WriteLogBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ScanBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class HistoryBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

//*****************************************************************
// BinProcessor

inline BinRpcProcessor::BinRpcProcessor()
{
	m_messageBase.m_signature = (int)NZBMESSAGE_SIGNATURE;
}

inline void BinRpcProcessor::Execute()
{
	// Read the first package which needs to be a request
	if (!m_connection->Recv(((char*)&m_messageBase) + sizeof(m_messageBase.m_signature), sizeof(m_messageBase) - sizeof(m_messageBase.m_signature)))
	{
		warn("Non-RustyGet request received on port %i from %s", g_Options->GetControlPort(), m_connection->GetRemoteAddr());
		return;
	}

	// rust/src/binrpc.rs: the credentials and the request type
	uint32 type = 0;
	if (!nzbget_rs_binrpc_check(reinterpret_cast<const unsigned char*>(&m_messageBase), sizeof(m_messageBase),
		g_Options->GetControlUsername(), g_Options->GetControlPassword(), &type))
	{
		warn("RustyGet request received on port %i from %s, but username or password invalid", g_Options->GetControlPort(), m_connection->GetRemoteAddr());
		return;
	}

	debug("%s request received from %s", nzbget_rs_binrpc_name(type), m_connection->GetRemoteAddr());

	Dispatch();
}


inline void BinRpcProcessor::Dispatch()
{
	// rust/src/binrpc.rs
	uint32 expectedSize = 0, receivedSize = 0;
	if (!nzbget_rs_binrpc_size_ok(reinterpret_cast<const unsigned char*>(&m_messageBase), sizeof(m_messageBase),
		g_MessageRequestSizes, sizeof(g_MessageRequestSizes) / sizeof(g_MessageRequestSizes[0]), &expectedSize, &receivedSize))
	{
		error("Invalid size of request: expected %i Bytes, but received %i Bytes", expectedSize, receivedSize);
		return;
	}

	std::unique_ptr<BinCommand> command;

	switch (ntohl(m_messageBase.m_type))
	{
		case rrDownload:
			command = std::make_unique<DownloadBinCommand>();
			break;

		case rrList:
			command = std::make_unique<ListBinCommand>();
			break;

		case rrLog:
			command = std::make_unique<LogBinCommand>();
			break;

		case rrPauseUnpause:
			command = std::make_unique<PauseUnpauseBinCommand>();
			break;

		case rrEditQueue:
			command = std::make_unique<EditQueueBinCommand>();
			break;

		case rrSetDownloadRate:
			command = std::make_unique<SetDownloadRateBinCommand>();
			break;

		case rrDumpDebug:
			command = std::make_unique<DumpDebugBinCommand>();
			break;

		case rrShutdown:
			command = std::make_unique<ShutdownBinCommand>();
			break;

		case rrReload:
			command = std::make_unique<ReloadBinCommand>();
			break;

		case rrVersion:
			command = std::make_unique<VersionBinCommand>();
			break;

		case rrPostQueue:
			command = std::make_unique<PostQueueBinCommand>();
			break;

		case rrWriteLog:
			command = std::make_unique<WriteLogBinCommand>();
			break;

		case rrScan:
			command = std::make_unique<ScanBinCommand>();
			break;

		case rrHistory:
			command = std::make_unique<HistoryBinCommand>();
			break;

		default:
			error("Received unsupported request %i", ntohl(m_messageBase.m_type));
			break;
	}

	if (command)
	{
		command->SetConnection(m_connection);
		command->SetMessageBase(&m_messageBase);
		command->Execute();
	}
}

// The entries of one kind of a list response, collected under the locks as
// plain structs (the first Inline of them on the stack, the strings pointing
// into the queue or log): rust/src/binrpc.rs encodes them all in one call
template <class T, size_t Inline>
class RsBatch
{
public:
	RsBatch() : m_data(m_inline), m_capacity(Inline) {}
	RsBatch(const RsBatch&) = delete;

	// room for a count of entries known beforehand
	void Reserve(size_t count)
	{
		if (count > m_capacity)
		{
			Grow(count);
		}
	}

	void Add(const T& entry)
	{
		if (m_size == m_capacity)
		{
			Grow(m_capacity * 2);
		}
		m_data[m_size++] = entry;
	}

	const T* Data() const { return m_data; }
	size_t Size() const { return m_size; }

private:
	T m_inline[Inline]; // not initialized: the entries are plain structs
	std::unique_ptr<T[]> m_heap;
	T* m_data;
	size_t m_capacity;
	size_t m_size = 0;

	void Grow(size_t capacity)
	{
		std::unique_ptr<T[]> heap(new T[capacity]);
		memcpy(static_cast<void*>(heap.get()), m_data, m_size * sizeof(T));
		m_heap = std::move(heap);
		m_data = m_heap.get();
		m_capacity = capacity;
	}
};

// Match Rust's repr(C) layout, including pointer alignment on 32-bit targets.
#define RS_LAYOUT(type, first, ints, strings) \
    static_assert(offsetof(type, first) == ((4 * (ints) + alignof(NzbgetRsBinText) - 1) & \
        ~(alignof(NzbgetRsBinText) - 1)) && \
        sizeof(type) == offsetof(type, first) + (strings) * sizeof(NzbgetRsBinText), "BinRpc entry layout")
RS_LAYOUT(NzbgetRsBinNzb, filename, 12, 5);
RS_LAYOUT(NzbgetRsBinParam, name, 1, 2);
RS_LAYOUT(NzbgetRsBinFile, subject, 10, 2);
RS_LAYOUT(NzbgetRsBinLog, text, 3, 1);
RS_LAYOUT(NzbgetRsBinPost, filename, 6, 4);
RS_LAYOUT(NzbgetRsBinHistory, name, 9, 1);
#undef RS_LAYOUT

static inline NzbgetRsBinText RsText(const char* text)
{
	return {text, text ? strlen(text) : 0};
}

// The trailing data of a list response, encoded by rust/src/binrpc.rs
class RsBytes
{
public:
	RsBytes() : m_bytes{} {}
	char* Scratch() { return m_scratch; }
	size_t Capacity() const { return sizeof(m_scratch); }
	RsBytes(const RsBytes&) = delete;
	~RsBytes() { Free(); }

	RsBytes& operator=(NzbgetRsBinResponse bytes)
	{
		Free();
		m_bytes = bytes;
		return *this;
	}

	int Size() { return (int)m_bytes.len; }

	// the bytes, sent after the response
	void Send(Connection* connection)
	{
		if (m_bytes.len > 0)
		{
			connection->Send(m_bytes.data, (int)m_bytes.len);
		}
	}

private:
	NzbgetRsBinResponse m_bytes;
    // A complete small response and its final NUL, alive until Send returns.
    char m_scratch[16384];
    void Free()
    {
        if (m_bytes.ownedCapacity != 0)
        {
            nzbget_rs_free({m_bytes.data, m_bytes.len, m_bytes.ownedCapacity});
        }
    }
};

//*****************************************************************
// Commands

inline void BinCommand::SendBoolResponse(bool success, const char* text)
{
	// rust/src/binrpc.rs: the response, then the text and its NUL
	NzbgetRsBuf response = nzbget_rs_binrpc_bool_response(success, text);
	m_connection->Send(response.data, sizeof(SNzbDownloadResponse));
	m_connection->Send(response.data + sizeof(SNzbDownloadResponse), (int)(response.len - sizeof(SNzbDownloadResponse)));
	nzbget_rs_free(response);
}


inline bool BinCommand::ReceiveRequest(void* buffer, int size)
{
	memcpy(buffer, m_messageBase, sizeof(SNzbRequestBase));
	size -= sizeof(SNzbRequestBase);
	if (size > 0)
	{
		if (!m_connection->Recv(((char*)buffer) + sizeof(SNzbRequestBase), size))
		{
			error("invalid request");
			return false;
		}
	}
	return true;
}

// The data following a request: its length is the client's and is checked (no
// buffer was allocated for 0 or a negative one, and the data was used all the
// same); the buffer gets a terminator after it, as its strings are read with
// the C string functions
inline bool BinCommand::ReceiveTrailing(CharBuffer& buffer, int64 len, int64 maxLen)
{
	if (!nzbget_rs_binrpc_trailing_ok(len, maxLen))
	{
		error("invalid request: data length %" PRIi64, len);
		return false;
	}
	buffer.Reserve((int)len + 1);
	if (!buffer)
	{
		error("invalid request: no memory for %" PRIi64 " bytes", len);
		return false;
	}
	if (len > 0 && !m_connection->Recv(buffer, (int)len))
	{
		error("invalid request");
		return false;
	}
	buffer[(int)len] = '\0';
	return true;
}

inline void PauseUnpauseBinCommand::Execute()
{
	SNzbPauseUnpauseRequest PauseUnpauseRequest;
	if (!ReceiveRequest(&PauseUnpauseRequest, sizeof(PauseUnpauseRequest)))
	{
		return;
	}

	g_WorkState->SetResumeTime(0);

	switch (ntohl(PauseUnpauseRequest.m_action))
	{
		case rpDownload:
			g_WorkState->SetPauseDownload(ntohl(PauseUnpauseRequest.m_pause));
			break;

		case rpPostProcess:
			g_WorkState->SetPausePostProcess(ntohl(PauseUnpauseRequest.m_pause));
			break;

		case rpScan:
			g_WorkState->SetPauseScan(ntohl(PauseUnpauseRequest.m_pause));
			break;
	}

	SendBoolResponse(true, "Pause-/Unpause-Command completed successfully");
}

inline void SetDownloadRateBinCommand::Execute()
{
	SNzbSetDownloadRateRequest SetDownloadRequest;
	if (!ReceiveRequest(&SetDownloadRequest, sizeof(SetDownloadRequest)))
	{
		return;
	}

	g_WorkState->SetSpeedLimit(ntohl(SetDownloadRequest.m_downloadRate));
	SendBoolResponse(true, "Rate-Command completed successfully");
}

inline void DumpDebugBinCommand::Execute()
{
	SNzbDumpDebugRequest DumpDebugRequest;
	if (!ReceiveRequest(&DumpDebugRequest, sizeof(DumpDebugRequest)))
	{
		return;
	}

	g_Log->LogDebugInfo();
	SendBoolResponse(true, "Debug-Command completed successfully");
}

inline void ShutdownBinCommand::Execute()
{
	SNzbShutdownRequest ShutdownRequest;
	if (!ReceiveRequest(&ShutdownRequest, sizeof(ShutdownRequest)))
	{
		return;
	}

	SendBoolResponse(true, "Stopping server");
	ExitProc();
}

inline void ReloadBinCommand::Execute()
{
	SNzbReloadRequest ReloadRequest;
	if (!ReceiveRequest(&ReloadRequest, sizeof(ReloadRequest)))
	{
		return;
	}

	SendBoolResponse(true, "Reloading server");
	Reload();
}

inline void VersionBinCommand::Execute()
{
	SNzbVersionRequest VersionRequest;
	if (!ReceiveRequest(&VersionRequest, sizeof(VersionRequest)))
	{
		return;
	}

	SendBoolResponse(true, Util::VersionRevision());
}

inline void DownloadBinCommand::Execute()
{
	SNzbDownloadRequest DownloadRequest;
	if (!ReceiveRequest(&DownloadRequest, sizeof(DownloadRequest)))
	{
		return;
	}

	TerminateField(DownloadRequest.m_nzbFilename);
	TerminateField(DownloadRequest.m_category);
	TerminateField(DownloadRequest.m_dupeKey);

	int bufLen = (int)ntohl(DownloadRequest.m_trailingDataLength);
	CharBuffer nzbContent;
	if (bufLen <= 0 || !ReceiveTrailing(nzbContent, bufLen, 768 * 1024 * 1024))
	{
		error("invalid request");
		return;
	}

	int priority = ntohl(DownloadRequest.m_priority);
	bool addPaused = ntohl(DownloadRequest.m_addPaused);
	bool addTop = ntohl(DownloadRequest.m_addFirst);
	bool autoCategory = ntohl(DownloadRequest.m_autoCategory);
	int dupeMode = ntohl(DownloadRequest.m_dupeMode);
	int dupeScore = ntohl(DownloadRequest.m_dupeScore);

	bool ok = false;

	if (nzbget_rs_binrpc_is_url(nzbContent))
	{
		// add url
		std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
		nzbInfo->SetKind(NzbInfo::nkUrl);
		nzbInfo->SetUrl(nzbContent);
		nzbInfo->SetFilename(DownloadRequest.m_nzbFilename);
		nzbInfo->SetCategory(DownloadRequest.m_category);
		nzbInfo->SetAutoCategory(autoCategory);
		nzbInfo->SetPriority(priority);
		nzbInfo->SetAddUrlPaused(addPaused);
		nzbInfo->SetDupeKey(DownloadRequest.m_dupeKey);
		nzbInfo->SetDupeScore(dupeScore);
		nzbInfo->SetDupeMode((EDupeMode)dupeMode);

		g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), addTop);

		ok = true;
	}
	else
	{
		ok = g_Scanner->AddExternalFile(
			DownloadRequest.m_nzbFilename,
			DownloadRequest.m_category,
			autoCategory,
			priority,
			DownloadRequest.m_dupeKey,
			dupeScore,
			static_cast<EDupeMode>(dupeMode),
			nullptr,
			addTop,
			addPaused,
			nullptr,
			nullptr,
			nzbContent,
			bufLen,
			nullptr
		) != Scanner::asFailed;
	}

	{
		NzbgetRsBuf msgBuf = nzbget_rs_tpr_binrpc_format_download_response_msg(
			ok ? 1 : 0, FileSystem::BaseFileName(DownloadRequest.m_nzbFilename));
		SendBoolResponse(ok, msgBuf.data ? msgBuf.data : "");
		nzbget_rs_free(msgBuf);
	}
}

inline void ListBinCommand::Execute()
{
	SNzbListRequest ListRequest;
	if (!ReceiveRequest(&ListRequest, sizeof(ListRequest)))
	{
		return;
	}

	SNzbListResponse ListResponse;
	memset(&ListResponse, 0, sizeof(ListResponse));
	ListResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	ListResponse.m_messageBase.m_structSize = htonl(sizeof(ListResponse));
	ListResponse.m_entrySize = htonl(sizeof(SNzbListResponseFileEntry));
	ListResponse.m_regExValid = 0;

	RsBytes entries;

	if (ntohl(ListRequest.m_fileList))
	{
		ERemoteMatchMode matchMode = (ERemoteMatchMode)ntohl(ListRequest.m_matchMode);
		bool matchGroup = ntohl(ListRequest.m_matchGroup);
		TerminateField(ListRequest.m_pattern);
		const char* pattern = ListRequest.m_pattern;

		std::unique_ptr<RegEx> regEx;
		if (matchMode == rmRegEx)
		{
			regEx = std::make_unique<RegEx>(pattern);
			ListResponse.m_regExValid = regEx->IsValid();
		}

		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int nrNzbEntries = downloadQueue->GetQueue()->size();
		int nrPPPEntries = 0;
		int nrFileEntries = 0;

		// the entries, collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinNzb, 16> nzbEntries;
		RsBatch<NzbgetRsBinParam, 32> pppEntries;
		RsBatch<NzbgetRsBinFile, 32> fileEntries;
		size_t pppCount = 0, fileCount = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			pppCount += nzbInfo->GetParameters()->size();
			fileCount += nzbInfo->GetFileList()->size();
		}
		nzbEntries.Reserve(nrNzbEntries);
		pppEntries.Reserve(pppCount);
		fileEntries.Reserve(fileCount);

		// nzb entries
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			uint32 sizeHi, sizeLo, remainingSizeHi, remainingSizeLo, pausedSizeHi, pausedSizeLo;
			Util::SplitInt64(nzbInfo->GetSize(), &sizeHi, &sizeLo);
			Util::SplitInt64(nzbInfo->GetRemainingSize(), &remainingSizeHi, &remainingSizeLo);
			Util::SplitInt64(nzbInfo->GetPausedSize(), &pausedSizeHi, &pausedSizeLo);
			nzbEntries.Add({(uint32)nzbInfo->GetId(), (uint32)nzbInfo->GetKind(), sizeLo, sizeHi, remainingSizeLo,
				remainingSizeHi, pausedSizeLo, pausedSizeHi, (uint32)nzbInfo->GetPausedFileCount(),
				(uint32)nzbInfo->GetRemainingParCount(), (uint32)nzbInfo->GetPriority(),
				nzbget_rs_tpr_binrpc_eval_nzb_group_match(
					matchGroup ? 1 : 0, regEx ? 1 : 0,
					(matchGroup && regEx && regEx->Match(nzbInfo->GetName())) ? 1 : 0),
				RsText(nzbInfo->GetFilename()), RsText(nzbInfo->GetName()), RsText(nzbInfo->GetDestDir()),
				RsText(nzbInfo->GetCategory()), RsText(nzbInfo->GetQueuedFilename())});
		}

		// ppp entries
		int nzbIndex = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbIndex++;
			for (NzbParameter& nzbParameter : nzbInfo->GetParameters())
			{
				pppEntries.Add({(uint32)nzbIndex, RsText(nzbParameter.GetName()), RsText(nzbParameter.GetValue())});
				nrPPPEntries++;
			}
		}

		// file entries: the usual owner is already known from this walk.
		int ownerIndex = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			ownerIndex++;
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				nrFileEntries++;
				int nzbIndex = ownerIndex;
				if (nzbget_rs_dqh_binrpc_should_lookup_unusual_file_owner(
					fileInfo->GetNzbInfo() == nzbInfo ? 1 : 0) != 0)
				{
					// Retain the first-match behavior for an unusual parent.
					nzbIndex = 0;
					for (uint32 i = 0; i < downloadQueue->GetQueue()->size(); i++)
					{
						nzbIndex++;
						if (downloadQueue->GetQueue()->at(i).get() == fileInfo->GetNzbInfo())
						{
							break;
						}
					}
				}

				bool match = false;
				if (nzbget_rs_dqh_binrpc_should_match_file_regex(
					regEx ? 1 : 0, matchGroup ? 1 : 0) != 0)
				{
					NzbgetRsBuf matchNameBuf = nzbget_rs_tpr_binrpc_format_file_match_name(
						fileInfo->GetNzbInfo()->GetName(), FileSystem::BaseFileName(fileInfo->GetFilename()));
					match = regEx->Match(matchNameBuf.data ? matchNameBuf.data : "");
					nzbget_rs_free(matchNameBuf);
				}

				uint32 fileSizeHi, fileSizeLo, remainingSizeHi, remainingSizeLo;
				Util::SplitInt64(fileInfo->GetSize(), &fileSizeHi, &fileSizeLo);
				Util::SplitInt64(fileInfo->GetRemainingSize(), &remainingSizeHi, &remainingSizeLo);
				fileEntries.Add({(uint32)fileInfo->GetId(), (uint32)nzbIndex, fileSizeLo, fileSizeHi, remainingSizeLo,
					remainingSizeHi, (uint32)fileInfo->GetPaused(), (uint32)fileInfo->GetFilenameConfirmed(),
					(uint32)fileInfo->GetActiveDownloads(), (uint32)match,
					RsText(fileInfo->GetSubject()), RsText(fileInfo->GetFilename())});
			}
		}

		entries = nzbget_rs_binrpc_encode_list_into(nzbEntries.Data(), nzbEntries.Size(), pppEntries.Data(), pppEntries.Size(),
			fileEntries.Data(), fileEntries.Size(), entries.Scratch(), entries.Capacity());

		ListResponse.m_nrTrailingNzbEntries = htonl(nrNzbEntries);
		ListResponse.m_nrTrailingPPPEntries = htonl(nrPPPEntries);
		ListResponse.m_nrTrailingFileEntries = htonl(nrFileEntries);
		ListResponse.m_trailingDataLength = htonl(entries.Size());
	}

	if (htonl(ListRequest.m_serverState))
	{
		int postJobCount = 0;
		int64 remainingSize;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				postJobCount += nzbInfo->GetPostInfo() ? 1 : 0;
			}
			downloadQueue->CalcRemainingSize(&remainingSize, nullptr);
		}

		uint32 sizeHi, sizeLo;
		uint32 downloadRate = Util::SafeIntCast<int64, uint32>(g_StatMeter->CalcCurrentDownloadSpeed());
		ListResponse.m_downloadRate = htonl(downloadRate);
		Util::SplitInt64(remainingSize, &sizeHi, &sizeLo);
		ListResponse.m_remainingSizeHi = htonl(sizeHi);
		ListResponse.m_remainingSizeLo = htonl(sizeLo);
		ListResponse.m_downloadLimit = htonl(g_WorkState->GetSpeedLimit());
		ListResponse.m_downloadPaused = htonl(g_WorkState->GetPauseDownload());
		ListResponse.m_postPaused = htonl(g_WorkState->GetPausePostProcess());
		ListResponse.m_scanPaused = htonl(g_WorkState->GetPauseScan());
		ListResponse.m_threadCount = htonl(Thread::GetThreadCount() - 1); // not counting itself
		ListResponse.m_postJobCount = htonl(postJobCount);

		int upTimeSec, dnTimeSec;
		int64 allBytes;
		bool standBy;
		g_StatMeter->CalcTotalStat(&upTimeSec, &dnTimeSec, &allBytes, &standBy);
		ListResponse.m_upTimeSec = htonl(upTimeSec);
		ListResponse.m_downloadTimeSec = htonl(dnTimeSec);
		ListResponse.m_downloadStandBy = htonl(standBy);
		Util::SplitInt64(allBytes, &sizeHi, &sizeLo);
		ListResponse.m_downloadedBytesHi = htonl(sizeHi);
		ListResponse.m_downloadedBytesLo = htonl(sizeLo);
	}

	// Send the request answer
	m_connection->Send((char*) &ListResponse, sizeof(ListResponse));

	// Send the data
	entries.Send(m_connection);
}


inline void LogBinCommand::Execute()
{
	SNzbLogRequest LogRequest;
	if (!ReceiveRequest(&LogRequest, sizeof(LogRequest)))
	{
		return;
	}

	RsBytes entries;
	int nrEntries;
	int bufsize;

	{
		GuardedMessageList messages = g_Log->GuardMessages();

		int start = 0;
		nzbget_rs_binrpc_log_slice_bounds(
			ntohl(LogRequest.m_lines),
			ntohl(LogRequest.m_idFrom),
			messages->size(),
			messages->empty() ? 0 : (uint32)messages->front().GetId(),
			&start, &nrEntries);

		// the messages from start on, collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinLog, 32> log;
		if ((size_t)start < messages->size())
		{
			log.Reserve(messages->size() - (size_t)start);
		}
		for (uint32 i = (uint32)start; i < messages->size(); i++)
		{
			Message& message = messages->at(i);
			log.Add({(uint32)message.GetId(), (uint32)message.GetKind(), (uint32)(int)message.GetTime(),
				RsText(message.GetText())});
		}
		entries = nzbget_rs_binrpc_encode_log_into(log.Data(), log.Size(), entries.Scratch(), entries.Capacity());
		bufsize = entries.Size();
	}

	SNzbLogResponse LogResponse;
	LogResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	LogResponse.m_messageBase.m_structSize = htonl(sizeof(LogResponse));
	LogResponse.m_entrySize = htonl(sizeof(SNzbLogResponseEntry));
	LogResponse.m_nrTrailingEntries = htonl(nrEntries);
	LogResponse.m_trailingDataLength = htonl(bufsize);

	// Send the request answer
	m_connection->Send((char*) &LogResponse, sizeof(LogResponse));

	// Send the data
	if (bufsize > 0)
	{
		entries.Send(m_connection);
	}
}


inline void EditQueueBinCommand::Execute()
{
	SNzbEditQueueRequest EditQueueRequest;
	if (!ReceiveRequest(&EditQueueRequest, sizeof(EditQueueRequest)))
	{
		return;
	}

	int nrIdEntries = ntohl(EditQueueRequest.m_nrTrailingIdEntries);
	int nrNameEntries = ntohl(EditQueueRequest.m_nrTrailingNameEntries);
	int nameEntriesLen = ntohl(EditQueueRequest.m_trailingNameEntriesLen);
	int action = ntohl(EditQueueRequest.m_action);
	int matchMode = ntohl(EditQueueRequest.m_matchMode);
	int offset = ntohl(EditQueueRequest.m_offset);
	int textLen = ntohl(EditQueueRequest.m_textLen);
	uint32 bufLength = ntohl(EditQueueRequest.m_trailingDataLength);

	// the parts' sizes from the client, checked against each other in 64 bits
	// (the unsigned sum wrapped for negative counts) and against the data
	if (!nzbget_rs_binrpc_edit_sizes(nrIdEntries, nrNameEntries, nameEntriesLen, textLen, bufLength))
	{
		error("Invalid struct size");
		return;
	}

	CharBuffer buf;
	if (!ReceiveTrailing(buf, bufLength, 64 * 1024 * 1024))
	{
		return;
	}
	if (textLen > 0)
	{
		buf[textLen - 1] = '\0';
	}

	if (nrIdEntries <= 0 && nrNameEntries <= 0)
	{
		SendBoolResponse(false, "Edit-Command failed: no IDs/Names specified");
		return;
	}

	// no text is an empty one: the edits read it as a string (a priority, a name)
	const char* text = textLen > 0 ? *buf : "";

	IdList cIdList;
	NameList cNameList;

	static_assert(sizeof(IdList::value_type) == sizeof(uint32_t), "ids are 32 bits");
	cIdList.resize(nrIdEntries);
	if (!nzbget_rs_binrpc_edit_split(buf, bufLength, textLen, nrIdEntries, nrNameEntries, (uint32_t*)cIdList.data()))
	{
		error("Invalid struct size");
		return;
	}
	// each name ends within the data: checked above
	cNameList.reserve(nrNameEntries);
	const char* names = buf + textLen + nrIdEntries * sizeof(int32);
	for (int i = 0; i < nrNameEntries; i++)
	{
		cNameList.push_back(names);
		names += strlen(names) + 1;
	}

	bool useOffsetStr = (action == DownloadQueue::eaFileMoveOffset || action == DownloadQueue::eaGroupMoveOffset);
	NzbgetRsBuf offBuf = useOffsetStr ? nzbget_rs_dqh_conn_format_port_str(offset) : NzbgetRsBuf{};
	bool ok = DownloadQueue::Guard()->EditList(
		nrIdEntries > 0 ? &cIdList : nullptr,
		nrNameEntries > 0 ? &cNameList : nullptr,
		(DownloadQueue::EMatchMode)matchMode, (DownloadQueue::EEditAction)action,
		useOffsetStr ? (offBuf.data ? offBuf.data : "0") : text);
	if (useOffsetStr)
	{
		nzbget_rs_free(offBuf);
	}

	if (ok)
	{
		SendBoolResponse(true, "Edit-Command completed successfully");
	}
	else
	{
#ifndef HAVE_REGEX_H
		if ((DownloadQueue::EMatchMode)matchMode == DownloadQueue::mmRegEx)
		{
			SendBoolResponse(false, "Edit-Command failed: the program was compiled without RegEx-support");
			return;
		}
#endif
		SendBoolResponse(false, "Edit-Command failed");
	}
}

inline void PostQueueBinCommand::Execute()
{
	SNzbPostQueueRequest PostQueueRequest;
	if (!ReceiveRequest(&PostQueueRequest, sizeof(PostQueueRequest)))
	{
		return;
	}

	SNzbPostQueueResponse PostQueueResponse;
	memset(&PostQueueResponse, 0, sizeof(PostQueueResponse));
	PostQueueResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	PostQueueResponse.m_messageBase.m_structSize = htonl(sizeof(PostQueueResponse));
	PostQueueResponse.m_entrySize = htonl(sizeof(SNzbPostQueueResponseEntry));

	RsBytes entries;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		// Make a data structure and copy all the elements of the list into it
		NzbList* nzbList = downloadQueue->GetQueue();

		int NrEntries = 0;
		time_t curTime = Util::CurrentTime();
		// collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinPost, 16> jobs;
		jobs.Reserve(nzbList->size());

		for (NzbInfo* nzbInfo : nzbList)
		{
			PostInfo* postInfo = nzbInfo->GetPostInfo();
			if (!postInfo)
			{
				continue;
			}

			NrEntries++;
			jobs.Add({(uint32)nzbInfo->GetId(), (uint32)postInfo->GetStage(), (uint32)postInfo->GetStageProgress(),
				(uint32)postInfo->GetFileProgress(),
				nzbget_rs_tpr_binrpc_calc_elapsed_sec((int64_t)curTime, (int64_t)postInfo->GetStartTime()),
				nzbget_rs_tpr_binrpc_calc_elapsed_sec((int64_t)curTime, (int64_t)postInfo->GetStageTime()),
				RsText(postInfo->GetNzbInfo()->GetFilename()), RsText(postInfo->GetNzbInfo()->GetName()),
				RsText(postInfo->GetNzbInfo()->GetDestDir()), RsText(postInfo->GetProgressLabel())});
		}
		entries = nzbget_rs_binrpc_encode_post_queue_into(jobs.Data(), jobs.Size(), entries.Scratch(), entries.Capacity());

		PostQueueResponse.m_nrTrailingEntries = htonl(NrEntries);
		PostQueueResponse.m_trailingDataLength = htonl(entries.Size());
	}

	// Send the request answer
	m_connection->Send((char*) &PostQueueResponse, sizeof(PostQueueResponse));

	// Send the data
	entries.Send(m_connection);
}


inline void WriteLogBinCommand::Execute()
{
	SNzbWriteLogRequest WriteLogRequest;
	if (!ReceiveRequest(&WriteLogRequest, sizeof(WriteLogRequest)))
	{
		return;
	}

	CharBuffer recvBuffer;
	if (!ReceiveTrailing(recvBuffer, (int)ntohl(WriteLogRequest.m_trailingDataLength), 1024 * 1024))
	{
		return;
	}

	bool OK = true;
	switch ((Message::EKind)ntohl(WriteLogRequest.m_kind))
	{
		case Message::mkDetail:
			detail("%s", *recvBuffer);
			break;
		case Message::mkInfo:
			info("%s", *recvBuffer);
			break;
		case Message::mkWarning:
			warn("%s", *recvBuffer);
			break;
		case Message::mkError:
			error("%s", *recvBuffer);
			break;
		case Message::mkDebug:
			debug("%s", *recvBuffer);
			break;
		default:
			OK = false;
	}
	SendBoolResponse(OK, nzbget_rs_dqh_binrpc_write_log_response_msg(OK ? 1 : 0));
}

inline void ScanBinCommand::Execute()
{
	SNzbScanRequest ScanRequest;
	if (!ReceiveRequest(&ScanRequest, sizeof(ScanRequest)))
	{
		return;
	}

	bool syncMode = ntohl(ScanRequest.m_syncMode);

	g_Scanner->ScanNzbDir(syncMode);
	SendBoolResponse(true, nzbget_rs_dqh_binrpc_scan_response_msg(syncMode ? 1 : 0));
}

inline void HistoryBinCommand::Execute()
{
	SNzbHistoryRequest HistoryRequest;
	if (!ReceiveRequest(&HistoryRequest, sizeof(HistoryRequest)))
	{
		return;
	}

	bool showHidden = ntohl(HistoryRequest.m_hidden);

	SNzbHistoryResponse HistoryResponse;
	memset(&HistoryResponse, 0, sizeof(HistoryResponse));
	HistoryResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	HistoryResponse.m_messageBase.m_structSize = htonl(sizeof(HistoryResponse));
	HistoryResponse.m_entrySize = htonl(sizeof(SNzbHistoryResponseEntry));

	RsBytes entries;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int nrEntries = 0;
		// collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinHistory, 32> history;
		history.Reserve(downloadQueue->GetHistory()->size());
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (nzbget_rs_tpr_binrpc_should_include_history_entry(
				(int)historyInfo->GetKind(), (int)HistoryInfo::hkDup, showHidden ? 1 : 0))
			{
				nrEntries++;
				// fields a kind doesn't set are 0
				uint32 sizeHi = 0, sizeLo = 0, fileCount = 0, parStatus = 0, scriptStatus = 0, urlStatus = 0;
				int histFieldKind = nzbget_rs_dqh_binrpc_classify_history_entry_fields(
					(int)historyInfo->GetKind(),
					(int)HistoryInfo::hkNzb,
					(int)HistoryInfo::hkDup,
					(int)HistoryInfo::hkUrl,
					showHidden ? 1 : 0);
				if (histFieldKind == 1)
				{
					NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
					Util::SplitInt64(nzbInfo->GetSize(), &sizeHi, &sizeLo);
					fileCount = nzbInfo->GetFileCount();
					parStatus = nzbInfo->GetParStatus();
					scriptStatus = nzbInfo->GetScriptStatuses()->CalcTotalStatus();
				}
				else if (histFieldKind == 2)
				{
					DupInfo* dupInfo = historyInfo->GetDupInfo();
					Util::SplitInt64(dupInfo->GetSize(), &sizeHi, &sizeLo);
				}
				else if (histFieldKind == 3)
				{
					NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
					urlStatus = nzbInfo->GetUrlStatus();
				}
				history.Add({(uint32)historyInfo->GetId(), (uint32)historyInfo->GetKind(), (uint32)(int)historyInfo->GetTime(),
					sizeLo, sizeHi, fileCount, parStatus, scriptStatus, urlStatus, RsText(historyInfo->GetName())});
			}
		}
		entries = nzbget_rs_binrpc_encode_history_into(history.Data(), history.Size(), entries.Scratch(), entries.Capacity());

		HistoryResponse.m_nrTrailingEntries = htonl(nrEntries);
		HistoryResponse.m_trailingDataLength = htonl(entries.Size());
	}

	// Send the request answer
	m_connection->Send((char*) &HistoryResponse, sizeof(HistoryResponse));

	// Send the data
	entries.Send(m_connection);
}

#endif


class RequestProcessor : public Thread, public Subject
{
public:
	~RequestProcessor()
	{
		m_connection->Disconnect();

#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}

	void Run() override
	{
		Execute();
		Notify(nullptr);
	}

	void Stop() override
	{
		Thread::Stop();
		m_connection->Cancel();
	}

	void SetTls(bool tls) { m_tls = tls; }
	void SetConnection(std::unique_ptr<Connection>&& connection) { m_connection = std::move(connection); }

private:
	bool m_tls;
	std::unique_ptr<Connection> m_connection;

	bool ServWebRequest(const char* signature)
	{
		char buffer[1024];
		if (!m_connection->ReadLine(buffer, sizeof(buffer), nullptr))
		{
			return false;
		}

		WebProcessor::EHttpMethod httpMethod = WebProcessor::hmGet;
		char* url = buffer;
		int rsMethod = 0;
		size_t urlOffset = 0;
		size_t urlLen = 0;
		if (!nzbget_rs_rco_remote_parse_web_request_line(
			signature, buffer, &rsMethod, &urlOffset, &urlLen))
		{
			return false;
		}
		httpMethod = (WebProcessor::EHttpMethod)rsMethod;
		url = buffer + urlOffset;
		url[urlLen] = '\0';

		debug("url: %s", url);

		WebProcessor processor;
		processor.SetConnection(m_connection.get());
		processor.SetUrl(url);
		processor.SetHttpMethod(httpMethod);
		processor.Execute();

		return processor.GetKeepAlive();
	}

	void Execute()
	{
		bool ok = false;
		m_connection->SetSuppressErrors(true);

#ifndef DISABLE_TLS
		if (m_tls && !m_connection->StartTls(false, g_Options->GetSecureCert(), g_Options->GetSecureKey()))
		{
			debug("Could not establish secure connection to web-client: Start TLS failed");
			return;
		}
#endif

		uint32 signature = 0;
		if (!m_connection->Recv((char*)&signature, 4))
		{
			debug("Could not read request signature");
			return;
		}

		int sigKind = nzbget_rs_rco_remote_classify_signature(
			(const char*)&signature, (uint32)NZBMESSAGE_SIGNATURE);
		if (sigKind == 1)
		{
			ok = true;
			BinRpcProcessor processor;
			processor.SetConnection(m_connection.get());
			processor.Execute();
		}
		else if (sigKind == 2)
		{
			ok = true;
			while (ServWebRequest((char*)&signature))
			{
				if (!m_connection->Recv((char*)&signature, 4))
				{
					debug("Could not read request signature");
					break;
				}
			}

			m_connection->SetGracefull(true);
			m_connection->Disconnect();
		}

		if (!ok)
		{
			int warnPort = nzbget_rs_ntw_remote_server_select_port(
				m_tls ? 1 : 0, g_Options->GetSecurePort(), g_Options->GetControlPort());
			warn("Non-RustyGet request received on port %i from %s", warnPort, m_connection->GetRemoteAddr());
		}
	}
};

class RemoteServer : public Thread, public Observer
{
public:
	RemoteServer(bool tls) : m_tls(tls) {}

	~RemoteServer()
	{
#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}

	void Run() override
	{
		debug("Entering RemoteServer-loop");

#ifndef DISABLE_TLS
		if (m_tls)
		{
			if (nzbget_rs_dqh_remote_server_is_cert_or_key_missing(
				g_Options->GetSecureCert(),
				FileSystem::FileExists(g_Options->GetSecureCert()) ? 1 : 0) != 0)
			{
				error("Could not initialize TLS, secure certificate is not configured or the cert-file was not found. Check option <SecureCert>");
				return;
			}

			if (nzbget_rs_dqh_remote_server_is_cert_or_key_missing(
				g_Options->GetSecureKey(),
				FileSystem::FileExists(g_Options->GetSecureKey()) ? 1 : 0) != 0)
			{
				error("Could not initialize TLS, secure key is not configured or the key-file was not found. Check option <SecureKey>");
				return;
			}
		}
#endif

		while (!IsStopped())
		{
			bool bind = true;

			if (!m_connection)
			{
				int listenPort = nzbget_rs_ntw_remote_server_select_port(
					m_tls ? 1 : 0, g_Options->GetSecurePort(), g_Options->GetControlPort());
				m_connection = std::make_unique<Connection>(g_Options->GetControlIp(),
					listenPort,
					m_tls);
				m_connection->SetTimeout(g_Options->GetRemoteTimeout());
				m_connection->SetSuppressErrors(false);
				bind = m_connection->Bind();
				if (IsStopped())
				{
					break;
				}
			}

			std::unique_ptr<Connection> acceptedConnection;
			if (bind)
			{
				acceptedConnection = m_connection->Accept();
			}
			if (bind && !acceptedConnection && m_connection && m_connection->GetAcceptBusy() && !IsStopped())
			{
				Util::Sleep(200);
				continue;
			}
			if (nzbget_rs_dqh_remote_server_should_reset_listener(
				bind ? 1 : 0, acceptedConnection != nullptr ? 1 : 0) != 0)
			{
				if (IsStopped())
				{
					break;
				}
				m_connection.reset();
				Util::Sleep(500);
				continue;
			}

			if (!IsStopped())
			{
				Guard guard(m_processorsMutex);
				time_t now = Util::CurrentTime();
				int shouldWarn = 0;
				if (nzbget_rs_ntw_remote_server_should_warn_cap(
					(int)m_activeProcessors.size(), MaxProcessors,
					(int64_t)now, (int64_t)m_lastCapWarning, &shouldWarn) != 0)
				{
					if (shouldWarn)
					{
						m_lastCapWarning = now;
						warn("Too many connections to the web interface/API (%i); refusing new ones", MaxProcessors);
					}
					continue;
				}
				RequestProcessor* commandThread = new RequestProcessor();
				commandThread->SetAutoDestroy(true);
				commandThread->SetConnection(std::move(acceptedConnection));
#ifndef DISABLE_TLS
				commandThread->SetTls(m_tls);
#endif
				m_activeProcessors.push_back(commandThread);
				commandThread->Attach(this);
				try
				{
					commandThread->Start();
				}
				catch (const std::system_error& e)
				{
					m_activeProcessors.pop_back();
					commandThread->Detach(this);
					delete commandThread;
					warn("Could not start a thread for a web interface/API connection: %s", e.what());
					Util::Sleep(200);
				}
			}
		}

		if (m_connection)
		{
			m_connection->Disconnect();
		}

		debug("RemoteServer: waiting for request processor to complete");
		bool completed = false;
		while (!completed)
		{
			{
				Guard guard(m_processorsMutex);
				completed = m_activeProcessors.size() == 0;
			}
			Util::Sleep(100);
		}
		debug("RemoteServer: request processor are completed");

		debug("Exiting RemoteServer-loop");
	}

	void Stop() override
	{
		Thread::Stop();
		if (m_connection)
		{
			m_connection->SetSuppressErrors(true);
			m_connection->SetForceClose(true);
			m_connection->Cancel();
		}

		debug("Stopping RequestProcessors");
		{
			Guard guard(m_processorsMutex);
			for (RequestProcessor* requestProcessor : m_activeProcessors)
			{
				requestProcessor->Stop();
			}
		}
		debug("RequestProcessors are notified");
		debug("RemoteServer stop end");
	}

	void ForceStop()
	{
		debug("Killing RequestProcessors");
		Guard guard(m_processorsMutex);
		for (RequestProcessor* requestProcessor : m_activeProcessors)
		{
			requestProcessor->Kill();
		}
		m_activeProcessors.clear();
		debug("RequestProcessors are killed");
	}

	void Update(Subject* caller, void* aspect) override
	{
		(void)aspect;
		debug("Notification from RequestProcessor received");

		RequestProcessor* requestProcessor = (RequestProcessor*)caller;
		Guard guard(m_processorsMutex);
		auto it = std::find(m_activeProcessors.begin(), m_activeProcessors.end(), requestProcessor);
		if (it != m_activeProcessors.end())
		{
			m_activeProcessors.erase(it);
		}
	}

private:
	typedef std::deque<RequestProcessor*> RequestProcessors;

	bool m_tls;
	std::unique_ptr<Connection> m_connection;
	RequestProcessors m_activeProcessors;
	Mutex m_processorsMutex;
	time_t m_lastCapWarning = 0;
	static const int MaxProcessors = 256;
};

#endif
