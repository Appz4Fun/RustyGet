/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef OPTIONS_H

#include <atomic>
#include <cstdint>
#include <string>
#include <string_view>
#include "FileSystem.h"
#include "Util.h"
#include "nzbget_rs.h"

#ifndef OPTIONS_H
#define OPTIONS_H

#ifndef FEEDINFO_H
#define FEEDINFO_H

#ifndef EDUPEMODE_DEFINED
#define EDUPEMODE_DEFINED
enum EDupeMode
{
	dmScore,
	dmAll,
	dmForce
};
#endif

class FeedInfo
{
public:
	enum EStatus
	{
		fsUndefined,
		fsRunning,
		fsFinished,
		fsFailed
	};

	enum class CategorySource
	{
		Auto,
		NZBFile,
		FeedFile
	};

	FeedInfo(
		int id,
		const char* name,
		const char* url,
		bool backlog,
		int interval,
		const char* filter,
		bool pauseNzb,
		const char* category,
		CategorySource categorySource,
		int priority,
		const char* extensions,
		unsigned int certVerifLevel
	)
		: m_id{ id }
		, m_name{ name ? name : "" }
		, m_url{ url ? url : "" }
		, m_filter{ filter ? filter : "" }
		, m_category{ category ? category : "" }
		, m_categorySource{ categorySource }
		, m_extensions{ extensions ? extensions : "" }
		, m_backlog{ backlog }
		, m_interval{ interval }
		, m_pauseNzb{ pauseNzb }
		, m_priority{ priority }
		, m_certVerifLevel{ certVerifLevel }
	{
		NzbgetRsBuf nameBuf{};
		nzbget_rs_feed_info_init(m_id, m_name.c_str(), m_filter.c_str(), &nameBuf, &m_filterHash);
		if (nameBuf.data)
		{
			m_name.assign(nameBuf.data, nameBuf.len);
		}
		nzbget_rs_free(nameBuf);
	}
	int GetId() const { return m_id; }
	const char* GetName() const { return m_name.c_str(); }
	const char* GetUrl() const { return m_url.c_str(); }
	int GetInterval() const { return m_interval; }
	const char* GetFilter() const { return m_filter.c_str(); }
	uint32 GetFilterHash() { return m_filterHash; }
	bool GetPauseNzb() const { return m_pauseNzb; }
	const char* GetCategory() const { return m_category.c_str(); }
	int GetPriority() const { return m_priority; }
	const char* GetExtensions() const { return m_extensions.c_str(); }
	unsigned int GetCertVerificationLevel() const { return m_certVerifLevel; }
	time_t GetLastUpdate() { return m_lastUpdate; }
	void SetLastUpdate(time_t lastUpdate) { m_lastUpdate = lastUpdate; }
	time_t GetNextUpdate() { return m_nextUpdate; }
	void SetNextUpdate(time_t nextUpdate) { m_nextUpdate = nextUpdate; }
	int GetLastInterval() { return m_lastInterval; }
	void SetLastInterval(int lastInterval) { m_lastInterval = lastInterval; }
	bool GetPreview() { return m_preview; }
	void SetPreview(bool preview) { m_preview = preview; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }
	const char* GetOutputFilename() const { return m_outputFilename.c_str(); }
	void SetOutputFilename(const char* outputFilename) { m_outputFilename = outputFilename ? outputFilename : ""; }
	bool GetFetch() { return m_fetch; }
	void SetFetch(bool fetch) { m_fetch = fetch; }
	bool GetForce() { return m_force; }
	void SetForce(bool force) { m_force = force; }
	bool GetBacklog() { return m_backlog; }
	void SetBacklog(bool backlog) { m_backlog = backlog; }
	CategorySource GetCategorySource() const { return m_categorySource; }
	void SetCategorySource(CategorySource categorySource) { m_categorySource = categorySource; }

private:
	int m_id;
	std::string m_name;
	std::string m_url;
	std::string m_category;
	std::string m_extensions;
	std::string m_filter;
	std::string m_outputFilename;
	time_t m_lastUpdate = 0;
	time_t m_nextUpdate = 0;
	uint32 m_filterHash;
	std::atomic<EStatus> m_status{fsUndefined};
	CategorySource m_categorySource = CategorySource::NZBFile;
	int m_interval;
	int m_priority;
	int m_lastInterval = 0;
	bool m_backlog;
	bool m_pauseNzb;
	bool m_preview = false;
	bool m_fetch = false;
	bool m_force = false;
	unsigned int m_certVerifLevel;
};

typedef std::deque<std::unique_ptr<FeedInfo>> Feeds;

class FeedFilterHelper
{
public:
	virtual void CalcDupeStatus(const char* title, const char* dupeKey, char* statusBuf, int bufLen) = 0;
};

class FeedItemInfo
{
public:
	enum EStatus
	{
		isUnknown,
		isBacklog,
		isFetched,
		isNew
	};

	enum EMatchStatus
	{
		msIgnored,
		msAccepted,
		msRejected
	};

	class Attr
	{
	public:
		Attr(const char* name, const char* value)
			: m_name(name ? name : "")
			, m_value(value ? value : "") 
		{ }
		const char* GetName() const { return m_name.c_str(); }
		const char* GetValue() const { return m_value.c_str(); }
	private:
		std::string m_name;
		std::string m_value;
	};

	typedef std::deque<Attr> AttributesBase;

	class Attributes: public AttributesBase
	{
	public:
		Attr* Find(const char* name)
		{
			if (!name)
			{
				return nullptr;
			}
			for (Attr& attr : *this)
			{
				if (!strcasecmp(attr.GetName(), name))
				{
					return &attr;
				}
			}
			return nullptr;
		}
	};

	FeedItemInfo() {}
	FeedItemInfo(FeedItemInfo&&) = delete;
	void SetFeedFilterHelper(FeedFilterHelper* feedFilterHelper) { m_feedFilterHelper = feedFilterHelper; }
	const char* GetTitle() const { return m_title.c_str(); }
	void SetTitle(const char* title) { m_title = title ? title : ""; }
	const char* GetFilename() const { return m_filename.c_str(); }
	void SetFilename(const char* filename) { m_filename = filename ? filename : ""; }
	const char* GetUrl() const { return m_url.c_str(); }
	void SetUrl(const char* url) { m_url = url ? url : ""; }
	int64 GetSize() const { return m_size; }
	void SetSize(int64 size) { m_size = size; }
	const char* GetCategory() const { return m_category.c_str(); }
	void SetCategory(const char* category) { m_category = category ? category : ""; }
	int GetImdbId() { return m_imdbId; }
	void SetImdbId(int imdbId) { m_imdbId = imdbId; }
	int GetRageId() { return m_rageId; }
	void SetRageId(int rageId) { m_rageId = rageId; }
	int GetTvdbId() { return m_tvdbId; }
	void SetTvdbId(int tvdbId) { m_tvdbId = tvdbId; }
	int GetTvmazeId() { return m_tvmazeId; }
	void SetTvmazeId(int tvmazeId) { m_tvmazeId = tvmazeId; }
	const char* GetDescription() const { return m_description.c_str(); }
	void SetDescription(const char* description) { m_description = description ? description: ""; }
	const char* GetSeason() const { return m_season.c_str(); }
	void SetSeason(const char* season)
	{
		m_season = season ? season : "";
		m_seasonNum = season ? ParsePrefixedInt(season) : 0;
	}
	const char* GetEpisode() const { return m_episode.c_str(); }
	void SetEpisode(const char* episode)
	{
		m_episode = episode ? episode : "";
		m_episodeNum = episode ? ParsePrefixedInt(episode) : 0;
	}
	int GetSeasonNum()
	{
		if (m_season.empty() && !m_seasonEpisodeParsed)
		{
			ParseSeasonEpisode();
		}
		return m_seasonNum;
	}
	int GetEpisodeNum()
	{
		if (m_episode.empty() && !m_seasonEpisodeParsed)
		{
			ParseSeasonEpisode();
		}
		return m_episodeNum;
	}
	const std::string& GetAddCategory() const { return m_addCategory; }
	void SetAddCategory(const char* addCategory) { m_addCategory = addCategory ? addCategory : ""; }
	bool GetPauseNzb() const { return m_pauseNzb; }
	void SetPauseNzb(bool pauseNzb) { m_pauseNzb = pauseNzb; }
	int GetPriority() const { return m_priority; }
	void SetPriority(int priority) { m_priority = priority; }
	time_t GetTime() const { return m_time; }
	void SetTime(time_t time) { m_time = time; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus status) { m_status = status; }
	EMatchStatus GetMatchStatus() { return m_matchStatus; }
	void SetMatchStatus(EMatchStatus matchStatus) { m_matchStatus = matchStatus; }
	int GetMatchRule() { return m_matchRule; }
	void SetMatchRule(int matchRule) { m_matchRule = matchRule; }
	const char* GetDupeKey() const { return m_dupeKey.c_str(); }
	void SetDupeKey(const char* dupeKey) { m_dupeKey = dupeKey ? dupeKey : ""; }
	void AppendDupeKey(const char* extraDupeKey)
	{
		if (!m_dupeKey.empty() && !Util::EmptyStr(extraDupeKey))
		{
			m_dupeKey += "-";
			m_dupeKey += extraDupeKey;
		}
	}
	void BuildDupeKey(const char* rageId, const char* tvdbId, const char* tvmazeId, const char* series)
	{
		bool anySeriesId = nzbget_rs_feed_has_any_series_id(
			series, rageId, tvdbId, tvmazeId, m_rageId, m_tvdbId, m_tvmazeId) != 0;
		int seasonNum = anySeriesId ? GetSeasonNum() : 0;
		int episodeNum = (anySeriesId && seasonNum != 0) ? GetEpisodeNum() : 0;
		NzbgetRsBuf buf = nzbget_rs_feed_build_dupe_key(
			rageId, tvdbId, tvmazeId, series,
			m_rageId, m_tvdbId, m_tvmazeId, m_imdbId,
			seasonNum, episodeNum, m_season.c_str(), m_episode.c_str());
		m_dupeKey.assign(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		m_dupeKey.shrink_to_fit();
	}
	int GetDupeScore() const { return m_dupeScore; }
	void SetDupeScore(int dupeScore) { m_dupeScore = dupeScore; }
	EDupeMode GetDupeMode() const { return m_dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { m_dupeMode = dupeMode; }
	const char* GetDupeStatus()
	{
		if (m_dupeStatus.empty())
		{
			BString<1024> statuses;
			m_feedFilterHelper->CalcDupeStatus(m_title.c_str(), m_dupeKey.c_str(), statuses, statuses.Capacity());
			m_dupeStatus = statuses;
		}
		return m_dupeStatus.c_str();
	}
	Attributes* GetAttributes() { return &m_attributes; }

private:
	std::string m_title;
	std::string m_filename;
	std::string m_url;
	std::string m_category;
	std::string m_description;
	std::string m_season;
	std::string m_episode;
	std::string m_addCategory;
	std::string m_dupeKey;
	std::string m_dupeStatus;
	time_t m_time = 0;
	int64 m_size = 0;
	int m_imdbId = 0;
	int m_rageId = 0;
	int m_tvdbId = 0;
	int m_tvmazeId = 0;
	int m_seasonNum = 0;
	int m_episodeNum = 0;
	bool m_seasonEpisodeParsed = false;
	bool m_pauseNzb = false;
	int m_priority = 0;
	EStatus m_status = isUnknown;
	EMatchStatus m_matchStatus = msIgnored;
	int m_matchRule = 0;
	int m_dupeScore = 0;
	EDupeMode m_dupeMode = dmScore;
	FeedFilterHelper* m_feedFilterHelper = nullptr;
	Attributes m_attributes;

	int ParsePrefixedInt(const char *value)
	{
		return nzbget_rs_feed_parse_prefixed_int(value);
	}

	void ParseSeasonEpisode()
	{
		if (!m_feedFilterHelper)
		{
			return;
		}

		m_seasonEpisodeParsed = true;

		NzbgetRsBuf seasonBuf{};
		NzbgetRsBuf episodeBuf{};
		if (nzbget_rs_feed_parse_season_episode(m_title.c_str(), &seasonBuf, &episodeBuf) != 0)
		{
			SetSeason(seasonBuf.data ? seasonBuf.data : "");
			SetEpisode(episodeBuf.data ? episodeBuf.data : "");
		}
		nzbget_rs_free(seasonBuf);
		nzbget_rs_free(episodeBuf);
	}
};

typedef std::deque<FeedItemInfo> FeedItemList;

class FeedHistoryInfo
{
public:
	enum EStatus
	{
		hsUnknown,
		hsBacklog,
		hsFetched
	};

	FeedHistoryInfo(const char* url, EStatus status, time_t lastSeen) 
		: m_url{ url ? url : "" }
		, m_status{ status }
		, m_lastSeen{ lastSeen } {}
	const char* GetUrl() const { return m_url.c_str(); }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }
	time_t GetLastSeen() { return m_lastSeen; }
	void SetLastSeen(time_t lastSeen) { m_lastSeen = lastSeen; }

private:
	std::string m_url;
	EStatus m_status;
	time_t m_lastSeen;
};

typedef std::deque<FeedHistoryInfo> FeedHistoryBase;

class FeedHistory : public FeedHistoryBase
{
public:
	void Remove(const char* url)
	{
		if (!url)
		{
			return;
		}
		for (iterator it = begin(); it != end(); ++it)
		{
			if (!strcmp(it->GetUrl(), url))
			{
				erase(it);
				break;
			}
		}
	}

	FeedHistoryInfo* Find(const char* url)
	{
		if (!url)
		{
			return nullptr;
		}
		for (FeedHistoryInfo& feedHistoryInfo : *this)
		{
			if (!strcmp(feedHistoryInfo.GetUrl(), url))
			{
				return &feedHistoryInfo;
			}
		}
		return nullptr;
	}
};

#endif


class Options
{
public:
 // Program options
	static constexpr std::string_view CONFIGFILE = "ConfigFile";
	static constexpr std::string_view APPBIN = "AppBin";
	static constexpr std::string_view APPDIR = "AppDir";
	static constexpr std::string_view APPVERSION = "Version";
	static constexpr std::string_view MAINDIR = "MainDir";
	static constexpr std::string_view DESTDIR = "DestDir";
	static constexpr std::string_view INTERDIR = "InterDir";
	static constexpr std::string_view TEMPDIR = "TempDir";
	static constexpr std::string_view QUEUEDIR = "QueueDir";
	static constexpr std::string_view NZBDIR = "NzbDir";
	static constexpr std::string_view WEBDIR = "WebDir";
	static constexpr std::string_view CONFIGTEMPLATE = "ConfigTemplate";
	static constexpr std::string_view SCRIPTDIR = "ScriptDir";
	static constexpr std::string_view REQUIREDDIR = "RequiredDir";
	static constexpr std::string_view LOGFILE = "LogFile";
	static constexpr std::string_view WRITELOG = "WriteLog";
	static constexpr std::string_view ROTATELOG = "RotateLog";
	static constexpr std::string_view APPENDCATEGORYDIR = "AppendCategoryDir";
	static constexpr std::string_view LOCKFILE = "LockFile";
	static constexpr std::string_view DAEMONUSERNAME = "DaemonUsername";
	static constexpr std::string_view OUTPUTMODE = "OutputMode";
	static constexpr std::string_view DUPECHECK = "DupeCheck";
	static constexpr std::string_view DUPEARTICLEFALLBACK = "DupeArticleFallback";
	static constexpr std::string_view DUPESTREAMDECOMPRESS = "DupeStreamDecompress";
	static constexpr std::string_view DUPESTREAMTIMEOUT = "DupeStreamTimeout";
	static constexpr std::string_view DUPESEARCH = "DupeSearch";
	static constexpr std::string_view DUPESEARCHDRYRUN = "DupeSearchDryRun";
	static constexpr std::string_view DUPESEARCHURL = "DupeSearchUrl";
	static constexpr std::string_view DUPESEARCHAPIKEY = "DupeSearchApiKey";
	static constexpr std::string_view DUPESEARCHDELAY = "DupeSearchDelay";
	static constexpr std::string_view DUPESEARCHMAXDONORS = "DupeSearchMaxDonors";
	static constexpr std::string_view DUPEHEALTHPERCENT = "DupeHealthPercent";
	static constexpr std::string_view DUPEHEALTHMIN = "DupeHealthMin";
	static constexpr std::string_view DUPEHEALTHMAX = "DupeHealthMax";
	static constexpr std::string_view DUPEBODYCHECKS = "DupeBodyChecks";
	static constexpr std::string_view DUPEHEALTHBUDGET = "DupeHealthBudget";
	static constexpr std::string_view DUPEMINALIVE = "DupeMinAlive";
	static constexpr std::string_view DUPEFASTDONORS = "DupeFastDonors";
	static constexpr std::string_view DOWNLOADRATE = "DownloadRate";
	static constexpr std::string_view CONTROLIP = "ControlIP";
	static constexpr std::string_view CONTROLPORT = "ControlPort";
	static constexpr std::string_view CONTROLUSERNAME = "ControlUsername";
	static constexpr std::string_view CONTROLPASSWORD = "ControlPassword";
	static constexpr std::string_view RESTRICTEDUSERNAME = "RestrictedUsername";
	static constexpr std::string_view RESTRICTEDPASSWORD = "RestrictedPassword";
	static constexpr std::string_view ADDUSERNAME = "AddUsername";
	static constexpr std::string_view ADDPASSWORD = "AddPassword";
	static constexpr std::string_view FORMAUTH = "FormAuth";
	static constexpr std::string_view JSONP = "JsonP";
	static constexpr std::string_view SECURECONTROL = "SecureControl";
	static constexpr std::string_view SECUREPORT = "SecurePort";
	static constexpr std::string_view SECURECERT = "SecureCert";
	static constexpr std::string_view SECUREKEY = "SecureKey";
	static constexpr std::string_view CERTSTORE = "CertStore";
	static constexpr std::string_view CERTCHECK = "CertCheck";
	static constexpr std::string_view AUTHORIZEDIP = "AuthorizedIP";
	static constexpr std::string_view ARTICLETIMEOUT = "ArticleTimeout";
	static constexpr std::string_view ARTICLEREADCHUNKSIZE = "ArticleReadChunkSize";
	static constexpr std::string_view URLTIMEOUT = "UrlTimeout";
	static constexpr std::string_view REMOTETIMEOUT = "RemoteTimeout";
	static constexpr std::string_view FLUSHQUEUE = "FlushQueue";
	static constexpr std::string_view SYSTEMHEALTHCHECK = "SystemHealthCheck";
	static constexpr std::string_view NZBLOG = "NzbLog";
	static constexpr std::string_view RAWARTICLE = "RawArticle";
	static constexpr std::string_view SKIPWRITE = "SkipWrite";
	static constexpr std::string_view ARTICLERETRIES = "ArticleRetries";
	static constexpr std::string_view ARTICLEINTERVAL = "ArticleInterval";
	static constexpr std::string_view URLRETRIES = "UrlRetries";
	static constexpr std::string_view URLINTERVAL = "UrlInterval";
	static constexpr std::string_view CONTINUEPARTIAL = "ContinuePartial";
	static constexpr std::string_view URLCONNECTIONS = "UrlConnections";
	static constexpr std::string_view LOGBUFFER = "LogBuffer";
	static constexpr std::string_view INFOTARGET = "InfoTarget";
	static constexpr std::string_view WARNINGTARGET = "WarningTarget";
	static constexpr std::string_view ERRORTARGET = "ErrorTarget";
	static constexpr std::string_view DEBUGTARGET = "DebugTarget";
	static constexpr std::string_view DETAILTARGET = "DetailTarget";
	static constexpr std::string_view PARCHECK = "ParCheck";
	static constexpr std::string_view PARREPAIR = "ParRepair";
	static constexpr std::string_view PARSCAN = "ParScan";
	static constexpr std::string_view PARQUICK = "ParQuick";
	static constexpr std::string_view POSTSTRATEGY = "PostStrategy";
	static constexpr std::string_view FILENAMING = "FileNaming";
	static constexpr std::string_view POSTDOWNLOADRENAME = "PostDownloadRename";
	static constexpr std::string_view RENAMEAFTERUNPACK = "RenameAfterUnpack";
	static constexpr std::string_view RENAMEIGNOREEXT = "RenameIgnoreExt";
	static constexpr std::string_view PARRENAME = "ParRename";
	static constexpr std::string_view PARBUFFER = "ParBuffer";
	static constexpr std::string_view PARTHREADS = "ParThreads";
	static constexpr std::string_view RARRENAME = "RarRename";
	static constexpr std::string_view HEALTHCHECK = "HealthCheck";
	static constexpr std::string_view DIRECTRENAME = "DirectRename";
	static constexpr std::string_view HARDLINKING = "HardLinking";
	static constexpr std::string_view HARDLINKINGIGNOREEXT = "HardLinkingIgnoreExt";
	static constexpr std::string_view UMASK = "UMask";
	static constexpr std::string_view UPDATEINTERVAL = "UpdateInterval";
	static constexpr std::string_view CURSESNZBNAME = "CursesNzbName";
	static constexpr std::string_view CURSESTIME = "CursesTime";
	static constexpr std::string_view CURSESGROUP = "CursesGroup";
	static constexpr std::string_view CRCCHECK = "CrcCheck";
	static constexpr std::string_view DIRECTWRITE = "DirectWrite";
	static constexpr std::string_view WRITEBUFFER = "WriteBuffer";
	static constexpr std::string_view NZBDIRINTERVAL = "NzbDirInterval";
	static constexpr std::string_view NZBDIRFILEAGE = "NzbDirFileAge";
	static constexpr std::string_view NZBDIRARCHIVESCAN = "NzbDirArchiveScan";
	static constexpr std::string_view NZBDIRARCHIVEACTION = "NzbDirArchiveAction";
	static constexpr std::string_view DISKSPACE = "DiskSpace";
	static constexpr std::string_view CRASHTRACE = "CrashTrace";
	static constexpr std::string_view CRASHDUMP = "CrashDump";
	static constexpr std::string_view PARPAUSEQUEUE = "ParPauseQueue";
	static constexpr std::string_view SCRIPTPAUSEQUEUE = "ScriptPauseQueue";
	static constexpr std::string_view NZBCLEANUPDISK = "NzbCleanupDisk";
	static constexpr std::string_view PARTIMELIMIT = "ParTimeLimit";
	static constexpr std::string_view KEEPHISTORY = "KeepHistory";
	static constexpr std::string_view UNPACK = "Unpack";
	static constexpr std::string_view DIRECTUNPACK = "DirectUnpack";
	static constexpr std::string_view USETEMPUNPACKDIR = "UseTempUnpackDir";
	static constexpr std::string_view UNPACKCLEANUPDISK = "UnpackCleanupDisk";
	static constexpr std::string_view UNRARCMD = "UnrarCmd";
	static constexpr std::string_view SEVENZIPCMD = "SevenZipCmd";
	static constexpr std::string_view UNPACKPASSFILE = "UnpackPassFile";
	static constexpr std::string_view UNPACKPAUSEQUEUE = "UnpackPauseQueue";
	static constexpr std::string_view SCRIPTORDER = "ScriptOrder";
	static constexpr std::string_view EXTENSIONS = "Extensions";
	static constexpr std::string_view EXTCLEANUPDISK = "ExtCleanupDisk";
	static constexpr std::string_view PARIGNOREEXT = "ParIgnoreExt";
	static constexpr std::string_view UNPACKIGNOREEXT = "UnpackIgnoreExt";
	static constexpr std::string_view FEEDHISTORY = "FeedHistory";
	static constexpr std::string_view URLFORCE = "UrlForce";
	static constexpr std::string_view TIMECORRECTION = "TimeCorrection";
	static constexpr std::string_view PROPAGATIONDELAY = "PropagationDelay";
	static constexpr std::string_view ARTICLECACHE = "ArticleCache";
	static constexpr std::string_view EVENTINTERVAL = "EventInterval";
	static constexpr std::string_view SHELLOVERRIDE = "ShellOverride";
	static constexpr std::string_view MONTHLYQUOTA = "MonthlyQuota";
	static constexpr std::string_view QUOTASTARTDAY = "QuotaStartDay";
	static constexpr std::string_view DAILYQUOTA = "DailyQuota";
	static constexpr std::string_view REORDERFILES = "ReorderFiles";
	static constexpr std::string_view UPDATECHECK = "UpdateCheck";
	static constexpr std::string_view MEMORYPRESSUREINTERVAL = "MemoryPressureInterval";
	static constexpr std::string_view MEMORYPRESSUREPATH = "MemoryPressurePath";

// obsolete options
	static constexpr std::string_view POSTLOGKIND = "PostLogKind";
	static constexpr std::string_view NZBLOGKIND = "NZBLogKind";
	static constexpr std::string_view RETRYONCRCERROR = "RetryOnCrcError";
	static constexpr std::string_view ALLOWREPROCESS = "AllowReProcess";
	static constexpr std::string_view POSTPROCESS = "PostProcess";
	static constexpr std::string_view LOADPARS = "LoadPars";
	static constexpr std::string_view THREADLIMIT = "ThreadLimit";
	static constexpr std::string_view PROCESSLOGKIND = "ProcessLogKind";
	static constexpr std::string_view APPENDNZBDIR = "AppendNzbDir";
	static constexpr std::string_view RENAMEBROKEN = "RenameBroken";
	static constexpr std::string_view MERGENZB = "MergeNzb";
	static constexpr std::string_view STRICTPARNAME = "StrictParName";
	static constexpr std::string_view RELOADURLQUEUE = "ReloadUrlQueue";
	static constexpr std::string_view RELOADPOSTQUEUE = "ReloadPostQueue";
	static constexpr std::string_view NZBPROCESS = "NZBProcess";
	static constexpr std::string_view NZBADDEDPROCESS = "NZBAddedProcess";
	static constexpr std::string_view CREATELOG = "CreateLog";
	static constexpr std::string_view RESETLOG = "ResetLog";
	static constexpr std::string_view PARCLEANUPQUEUE = "ParCleanupQueue";
	static constexpr std::string_view DELETECLEANUPDISK = "DeleteCleanupDisk";
	static constexpr std::string_view HISTORYCLEANUPDISK = "HistoryCleanupDisk";
	static constexpr std::string_view SCANSCRIPT = "ScanScript";
	static constexpr std::string_view QUEUESCRIPT = "QueueScript";
	static constexpr std::string_view FEEDSCRIPT = "FeedScript";
	static constexpr std::string_view DECODE = "Decode";
	static constexpr std::string_view SAVEQUEUE = "SaveQueue";
	static constexpr std::string_view RELOADQUEUE = "ReloadQueue";
	static constexpr std::string_view TERMINATETIMEOUT = "TerminateTimeout";
	static constexpr std::string_view ACCURATERATE = "AccurateRate";
	static constexpr std::string_view CREATEBROKENLOG = "CreateBrokenLog";
	static constexpr std::string_view BROKENLOG = "BrokenLog";

	enum EWriteLog
	{
		wlNone,
		wlAppend,
		wlReset,
		wlRotate
	};
	enum EMessageTarget
	{
		mtNone,
		mtScreen,
		mtLog,
		mtBoth
	};
	enum EOutputMode
	{
		omLoggable,
		omColored,
		omNCurses
	};
	enum EParCheck
	{
		pcAuto,
		pcAlways,
		pcForce,
		pcManual
	};
	enum EParScan
	{
		psLimited,
		psExtended,
		psFull,
		psDupe
	};
	enum EDupeArticleFallback
	{
		// ordered by capability: every value includes the capabilities of the
		// values before it, so feature gates compare with >= (dafLive adds
		// download-concurrent repair on top of dafStream's post-processing repair)
		dafNone,
		dafArticle,
		dafStream,
		dafLive
	};
	enum EHealthCheck
	{
		hcPause,
		hcDelete,
		hcPark,
		hcNone,
		hcDupe
	};
	enum ESchedulerCommand
	{
		scPauseDownload,
		scUnpauseDownload,
		scPausePostProcess,
		scUnpausePostProcess,
		scDownloadRate,
		scScript,
		scProcess,
		scPauseScan,
		scUnpauseScan,
		scActivateServer,
		scDeactivateServer,
		scFetchFeed
	};
	enum EPostStrategy
	{
		ppSequential,
		ppBalanced,
		ppAggressive,
		ppRocket
	};
	enum EFileNaming
	{
		nfAuto,
		nfArticle,
		nfNzb
	};
	enum ECertVerifLevel
	{
		cvNone,
		cvMinimal,
		cvStrict,
		Count
	};
	
	enum ENzbDirArchiveAction
	{
		Delete,
		Move
	};

	class OptEntry
	{
	public:
		OptEntry(const char* name, const char* value) :
			m_name(name), m_value(value) {}
		void SetName(const char* name) { m_name = name; }
		const char* GetName() const { return m_name; }
		void SetValue(const char* value);
		const char* GetValue() const { return m_value; }
		const char* GetDefValue() const { return m_defValue; }
		int GetLineNo() const { return m_lineNo; }
		bool Restricted();

	private:
		CString m_name;
		CString m_value;
		CString m_defValue;
		int m_lineNo = 0;

		void SetLineNo(int lineNo) { m_lineNo = lineNo; }

		friend class Options;
	};

	typedef std::deque<OptEntry> OptEntriesBase;

	class OptEntries: public OptEntriesBase
	{
	public:
		OptEntry* FindOption(const char* name);
	};

	typedef GuardedPtr<OptEntries> GuardedOptEntries;

	typedef std::vector<CString> NameList;
	typedef std::vector<const char*> CmdOptList;

	class Category
	{
	public:
		Category(const char* name, const char* destDir, bool unpack, const char* extensions)
			: m_name(name ? name : ""),
			  m_destDir(destDir ? destDir : ""),
			  m_extensions(extensions ? extensions : ""),
			  m_destDirPath(m_destDir),
			  m_unpack(unpack)
		{
		}
		const char* GetName() const { return m_name.c_str(); }
		const char* GetDestDir() const { return m_destDir.c_str(); }
		const fs::path& GetDestDirPath() const { return m_destDirPath; }
		bool GetUnpack() const { return m_unpack; }
		const char* GetExtensions() const { return m_extensions.c_str(); }
		NameList* GetAliases() { return &m_aliases; }
		const NameList* GetAliases() const { return &m_aliases; }

	private:
		const std::string m_name;
		const std::string m_destDir;
		const std::string m_extensions;
		const fs::path m_destDirPath;
		NameList m_aliases;
		const bool m_unpack;
	};

	typedef std::deque<Category> CategoriesBase;

	class Categories: public CategoriesBase
	{
	public:
		Category* FindCategory(const char* name, bool searchAliases);
		const Category* FindCategory(const char* name, bool searchAliases) const;
	};

	class Extender
	{
	public:
		virtual void AddNewsServer(int id, bool active, const char* name, const char* host,
			int port, int ipVersion, const char* user, const char* pass, bool joinGroup,
			bool tls, const char* cipher, int maxConnections, int retention,
			int level, int group, bool optional, unsigned int certVerificationfLevel) = 0;
		virtual ~Extender();
		virtual void AddFeed(
			int id,
			const char* name,
			const char* url,
			int interval,
			const char* filter,
			bool backlog,
			bool pauseNzb,
			const char* category,
			FeedInfo::CategorySource categorySource,
			int priority,
			const char* extensions,
			unsigned int certVeriflevel
		) = 0;
		virtual void AddTask(
			int id,
			int hours,
			int minutes,
			int weekDaysBits,
			ESchedulerCommand command,
			const char* param
		) = 0;
		virtual void SetupFirstStart() {}
	};

	Options(const char* exeName, const char* configFilename, bool noConfig,
		CmdOptList* commandLineOptions, Extender* extender);
	Options(CmdOptList* commandLineOptions, Extender* extender);
	~Options();

	static bool SplitOptionString(const char* option, CString& optName, CString& optValue);
	static void ConvertOldOptions(OptEntries* optEntries);
	bool GetFatalError() { return m_fatalError; }
	GuardedOptEntries GuardOptEntries() { return GuardedOptEntries(&m_optEntries, &m_optEntriesMutex); }
	void CreateSchedulerTask(int id, const char* time, const char* weekDays,
		ESchedulerCommand command, const char* param);

	// Options
	const char* GetConfigFilename() const { return m_configFilename; }
	bool GetConfigErrors() const { return m_configErrors; }
	const char* GetMainDir() const { return m_mainDir; }
	const char* GetAppDir() const { return m_appDir; }
	const char* GetDestDir() const { return m_destDir; }
	const char* GetInterDir() const { return m_interDir; }
	const char* GetTempDir() const { return m_tempDir; }
	const char* GetQueueDir() const { return m_queueDir; }
	const char* GetNzbDir() const { return m_nzbDir; }
	const char* GetWebDir() const { return m_webDir; }
	const char* GetConfigTemplate() const { return m_configTemplate; }
	const char* GetScriptDir() const { return m_scriptDir; }
	const char* GetRequiredDir() const { return m_requiredDir; }
	bool GetSystemHealthCheck() const { return m_systemHealthCheck; }
	bool GetNzbLog() const { return m_nzbLog; }
	EMessageTarget GetInfoTarget() const { return m_infoTarget; }
	EMessageTarget GetWarningTarget() const { return m_warningTarget; }
	EMessageTarget GetErrorTarget() const { return m_errorTarget; }
	EMessageTarget GetDebugTarget() const { return m_debugTarget; }
	EMessageTarget GetDetailTarget() const { return m_detailTarget; }
	int GetArticleTimeout() const { return m_articleTimeout; }
	int GetArticleReadChunkSize() const { return m_articleReadChunkSize; }
	int GetUrlTimeout() const { return m_urlTimeout; }
	int GetRemoteTimeout() const { return m_remoteTimeout; }
	bool GetRawArticle() const { return m_rawArticle; }
	bool GetSkipWrite() const { return m_skipWrite; }
	bool GetAppendCategoryDir() const { return m_appendCategoryDir; }
	bool GetContinuePartial() const { return m_continuePartial; }
	int GetArticleRetries() const { return m_articleRetries; }
	int GetArticleInterval() const { return m_articleInterval; }
	int GetUrlRetries() const { return m_urlRetries; }
	int GetUrlInterval() const { return m_urlInterval; }
	bool GetFlushQueue() const { return m_flushQueue; }
	bool GetDupeCheck() const { return m_dupeCheck; }
	EDupeArticleFallback GetDupeArticleFallback() const { return m_dupeArticleFallback; }
	bool GetDupeStreamDecompress() const { return m_dupeStreamDecompress; }
	int GetDupeStreamTimeout() const { return m_dupeStreamTimeout; }
	bool GetDupeSearch() const { return m_dupeSearch; }
	bool GetDupeSearchDryRun() const { return m_dupeSearchDryRun; }
	const char* GetDupeSearchUrl() const { return m_dupeSearchUrl; }
	const char* GetDupeSearchApiKey() const { return m_dupeSearchApiKey; }
	int GetDupeSearchDelay() const { return m_dupeSearchDelay; }
	int GetDupeSearchMaxDonors() const { return m_dupeSearchMaxDonors; }
	int GetDupeHealthPercent() const { return m_dupeHealthPercent; }
	int GetDupeHealthMin() const { return m_dupeHealthMin; }
	int GetDupeHealthMax() const { return m_dupeHealthMax; }
	int GetDupeBodyChecks() const { return m_dupeBodyChecks; }
	int GetDupeHealthBudget() const { return m_dupeHealthBudget; }
	int GetDupeMinAlive() const { return m_dupeMinAlive; }
	int GetDupeFastDonors() const { return m_dupeFastDonors; }
	const char* GetControlIp() const { return m_controlIp; }
	const char* GetControlUsername() const { return m_controlUsername; }
	const char* GetControlPassword() const { return m_controlPassword; }
	const char* GetRestrictedUsername() const { return m_restrictedUsername; }
	const char* GetRestrictedPassword() const { return m_restrictedPassword; }
	const char* GetAddUsername() const { return m_addUsername; }
	const char* GetAddPassword() const { return m_addPassword; }
	int GetControlPort() const { return m_controlPort; }
	bool GetFormAuth() const { return m_formAuth; }
	bool GetJsonP() const { return m_jsonP; }
	bool GetSecureControl() const { return m_secureControl; }
	int GetSecurePort() const { return m_securePort; }
	const char* GetSecureCert() const { return m_secureCert; }
	const char* GetSecureKey() const { return m_secureKey; }
	const char* GetCertStore() const { return m_certStore; }
	bool GetCertCheck() const { return m_certCheck; }
	const char* GetAuthorizedIp() const { return m_authorizedIp; }
	const char* GetLockFile() const { return m_lockFile; }
	const char* GetDaemonUsername() const { return m_daemonUsername; }
	EOutputMode GetOutputMode() const { return m_outputMode; }
	int GetUrlConnections() const { return m_urlConnections; }
	int GetLogBuffer() const { return m_logBuffer; }
	EWriteLog GetWriteLog() const { return m_writeLog; }
	const char* GetLogFile() const { return m_logFile; }
	int GetRotateLog() const { return m_rotateLog; }
	EParCheck GetParCheck() const { return m_parCheck; }
	bool GetParRepair() const { return m_parRepair; }
	EParScan GetParScan() const { return m_parScan; }
	bool GetParQuick() const { return m_parQuick; }
	EPostStrategy GetPostStrategy() const { return m_postStrategy; }
	bool GetParRename() const { return m_parRename; }
	int GetParBuffer() const { return m_parBuffer; }
	int GetParThreads() const { return m_parThreads; }
	bool GetRarRename() const { return m_rarRename; }
	EHealthCheck GetHealthCheck() const { return m_healthCheck; }
	const char* GetScriptOrder() const { return m_scriptOrder; }
	const char* GetExtensions() const { return m_extensions; }
	int GetUMask() const { return m_umask; }
	int GetUpdateInterval() const { return m_updateInterval; }
	bool GetCursesNzbName() const { return m_cursesNzbName; }
	bool GetCursesTime() const { return m_cursesTime; }
	bool GetCursesGroup() const { return m_cursesGroup; }
	bool GetCrcCheck() const { return m_crcCheck; }
	bool GetDirectWrite() const { return m_directWrite; }
	int GetWriteBuffer() const { return m_writeBuffer; }
	int GetNzbDirInterval() const { return m_nzbDirInterval; }
	int GetNzbDirFileAge() const { return m_nzbDirFileAge; }
	bool GetNzbDirArchiveScan() const { return m_nzbDirArchiveScan; }
	ENzbDirArchiveAction GetNzbDirArchiveAction() const { return m_nzbDirArchiveAction; }
	int GetDiskSpace() const { return m_diskSpace; }
	bool GetTls() const { return m_tls; }
	bool GetCrashTrace() const { return m_crashTrace; }
	bool GetCrashDump() const { return m_crashDump; }
	bool GetParPauseQueue() const { return m_parPauseQueue; }
	bool GetScriptPauseQueue() const { return m_scriptPauseQueue; }
	bool GetNzbCleanupDisk() const { return m_nzbCleanupDisk; }
	int GetParTimeLimit() const { return m_parTimeLimit; }
	int GetKeepHistory() const { return m_keepHistory; }
	bool GetUnpack() const { return m_unpack; }
	bool GetDirectUnpack() const { return m_directUnpack; }
	bool GetUseTempUnpackDir() const { return m_useTempUnpackDir; }
	bool GetUnpackCleanupDisk() const { return m_unpackCleanupDisk; }
	const char* GetUnrarCmd() const { return m_unrarCmd; }
	const char* GetSevenZipCmd() const { return m_sevenZipCmd; }
	const char* GetUnpackPassFile() const { return m_unpackPassFile; }
	bool GetUnpackPauseQueue() const { return m_unpackPauseQueue; }
	bool GetUnpackMutexEnforced() const { return m_unpackMutexEnforced; }
	void SetUnpackMutexEnforced(bool unpackMutexEnforced) { m_unpackMutexEnforced = unpackMutexEnforced; }
	const char* GetExtCleanupDisk() const { return m_extCleanupDisk; }
	const char* GetParIgnoreExt() const { return m_parIgnoreExt; }
	bool GetPostDownloadRename() const { return m_postDownloadRename; }
	bool GetRenameAfterUnpack() const { return m_renameAfterUnpack; }
	const char* GetRenameIgnoreExt() const { return m_renameIgnoreExt; }
	const char* GetUnpackIgnoreExt() const { return m_unpackIgnoreExt; }
	int GetFeedHistory() const { return m_feedHistory; }
	bool GetUrlForce() const { return m_urlForce; }
	int GetTimeCorrection() const { return m_timeCorrection; }
	int GetPropagationDelay() const { return m_propagationDelay; }
	int GetArticleCache() const { return m_articleCache; }
	int GetEventInterval() const { return m_eventInterval; }
	int GetMemoryPressureInterval() const { return m_memoryPressureInterval; }
	const char* GetMemoryPressurePath() const { return m_memoryPressurePath; }
	const std::string& GetShellOverride() const { return m_shellOverride; }
	int GetMonthlyQuota() const { return m_monthlyQuota; }
	int GetQuotaStartDay() const { return m_quotaStartDay; }
	int GetDailyQuota() const { return m_dailyQuota; }
	bool GetDirectRename() const { return m_directRename; }
	bool GetHardLinking() const { return m_hardLinking; }
	const char* GetHardLinkingIgnoreExt() const { return m_hardLinkingIgnoreExt; }
	bool GetReorderFiles() const { return m_reorderFiles; }
	EFileNaming GetFileNaming() const { return m_fileNaming; }
	int GetDownloadRate() const { return m_downloadRate; }

	Categories* GetCategories() { return &m_categories; }
	const Categories* GetCategories() const { return &m_categories; }
	Category* FindCategory(const char* name, bool searchAliases) { return m_categories.FindCategory(name, searchAliases); }
	const char* GetOption(const char* optname);

	// Current state
	void SetServerMode(bool serverMode) { m_serverMode = serverMode; }
	bool GetServerMode() const { return m_serverMode; }
	void SetDaemonMode(bool daemonMode) { m_daemonMode = daemonMode; }
	bool GetDaemonMode() const { return m_daemonMode; }
	void SetRemoteClientMode(bool remoteClientMode) { m_remoteClientMode = remoteClientMode; }
	bool GetRemoteClientMode() const { return m_remoteClientMode; }
	void SetDirectClientMode(bool directClientMode) { m_directClientMode = directClientMode; }
	bool GetDirectClientMode() const { return m_directClientMode; }

	const fs::path& GetAppDirPath() const { return m_appDirPath; }
	const fs::path& GetConfigFilePath() const { return m_configFilePath; }
	const fs::path& GetMainDirPath() const { return m_mainDirPath; }
	const fs::path& GetDestDirPath() const { return m_destDirPath; }
	const fs::path& GetInterDirPath() const { return m_interDirPath; }
	const fs::path& GetTempDirPath() const { return m_tempDirPath; }
	const fs::path& GetQueueDirPath() const { return m_queueDirPath; }
	const fs::path& GetNzbDirPath() const { return m_nzbDirPath; }
	const fs::path& GetWebDirPath() const { return m_webDirPath; }
	const fs::path& GetConfigTemplatePath() const { return m_configTemplatePath; }
	const fs::path& GetSecureCertPath() const { return m_secureCertPath; }
	const fs::path& GetSecureKeyPath() const { return m_secureKeyPath; }
	const fs::path& GetCertStorePath() const { return m_certStorePath; }
	const fs::path& GetLockFilePath() const { return m_lockFilePath; }
	const fs::path& GetLogFilePath() const { return m_logFilePath; }
	const fs::path& GetUnpackPassFilePath() const { return m_unpackPassFilePath; }
	const fs::path& GetUnrarPath() const { return m_unrarPath; }
	const fs::path& GetSevenZipPath() const { return m_sevenZipPath; }
	const std::vector<fs::path>& GetScriptDirPaths() const { return m_scriptDirPaths; }

private:
	void CheckDirs();
	void CheckScriptDir();

	OptEntries m_optEntries;
	Mutex m_optEntriesMutex;
	Categories m_categories;
	bool m_noDiskAccess = false;
	bool m_noConfig = false;
	bool m_fatalError = false;
	Extender* m_extender;

	// Options
	fs::path m_appDirPath;
	fs::path m_configFilePath;
	fs::path m_mainDirPath;
	fs::path m_destDirPath;
	fs::path m_interDirPath;
	fs::path m_tempDirPath;
	fs::path m_queueDirPath;
	fs::path m_nzbDirPath;
	fs::path m_webDirPath;
	fs::path m_configTemplatePath;
	fs::path m_secureCertPath;
	fs::path m_secureKeyPath;
	fs::path m_certStorePath;
	fs::path m_lockFilePath;
	fs::path m_logFilePath;
	fs::path m_unpackPassFilePath;
	fs::path m_unrarPath;
	fs::path m_sevenZipPath;
	std::vector<fs::path> m_scriptDirPaths;

	bool m_configErrors = false;
	int m_configLine = 0;
	CString m_appDir;
	CString m_configFilename;
	CString m_mainDir;
	CString m_destDir;
	CString m_interDir;
	CString m_tempDir;
	CString m_queueDir;
	CString m_nzbDir;
	CString m_webDir;
	CString m_configTemplate;
	CString m_scriptDir;
	CString m_requiredDir;
	EMessageTarget m_infoTarget = mtScreen;
	EMessageTarget m_warningTarget = mtScreen;
	EMessageTarget m_errorTarget = mtScreen;
	EMessageTarget m_debugTarget = mtNone;
	EMessageTarget m_detailTarget = mtScreen;
	bool m_skipWrite = false;
	bool m_rawArticle = false;
	bool m_nzbLog = false;
	bool m_systemHealthCheck = true;
	int m_articleTimeout = 0;
	int m_articleReadChunkSize = 4;
	int m_urlTimeout = 0;
	int m_remoteTimeout = 0;
	bool m_appendCategoryDir = false;
	bool m_continuePartial = false;
	int m_articleRetries = 0;
	int m_articleInterval = 0;
	int m_urlRetries = 0;
	int m_urlInterval = 0;
	bool m_flushQueue = false;
	bool m_dupeCheck = false;
	EDupeArticleFallback m_dupeArticleFallback = dafNone;
	bool m_dupeStreamDecompress = false;
	int m_dupeStreamTimeout = 60;
	bool m_dupeSearch = false;
	bool m_dupeSearchDryRun = false;
	CString m_dupeSearchUrl;
	CString m_dupeSearchApiKey;
	int m_dupeSearchDelay = 20;
	int m_dupeSearchMaxDonors = 0;
	int m_dupeHealthPercent = 5;
	int m_dupeHealthMin = 50;
	int m_dupeHealthMax = 1000;
	int m_dupeBodyChecks = 20;
	int m_dupeHealthBudget = 120;
	int m_dupeMinAlive = 50;
	int m_dupeFastDonors = 5;
	CString m_controlIp;
	CString m_controlUsername;
	CString m_controlPassword;
	CString m_restrictedUsername;
	CString m_restrictedPassword;
	CString m_addUsername;
	CString m_addPassword;
	bool m_formAuth = false;
	bool m_jsonP = false;
	int m_controlPort = 0;
	bool m_secureControl = false;
	int m_securePort = 0;
	CString m_secureCert;
	CString m_secureKey;
	CString m_certStore;
	bool m_certCheck = false;
	CString m_authorizedIp;
	CString m_lockFile;
	CString m_daemonUsername;
	EOutputMode m_outputMode = omLoggable;
	int m_urlConnections = 0;
	int m_logBuffer = 0;
	EWriteLog m_writeLog = wlAppend;
	int m_rotateLog = 0;
	CString m_logFile;
	EParCheck m_parCheck = pcManual;
	bool m_parRepair = false;
	EParScan m_parScan = psLimited;
	bool m_parQuick = true;
	EPostStrategy m_postStrategy = ppSequential;
	bool m_parRename = false;
	int m_parBuffer = 0;
	int m_parThreads = 0;
	bool m_rarRename = false;
	bool m_directRename = false;
	bool m_hardLinking = false;
	CString m_hardLinkingIgnoreExt;
	EHealthCheck m_healthCheck = hcNone;
	CString m_extensions;
	CString m_scriptOrder;
	int m_umask = 0;
	int m_updateInterval = 0;
	bool m_cursesNzbName = false;
	bool m_cursesTime = false;
	bool m_cursesGroup = false;
	bool m_crcCheck = false;
	bool m_directWrite = false;
	int m_writeBuffer = 0;
	int m_nzbDirInterval = 0;
	int m_nzbDirFileAge = 0;
	bool m_nzbDirArchiveScan = true;
	ENzbDirArchiveAction m_nzbDirArchiveAction = ENzbDirArchiveAction::Move;
	int m_diskSpace = 0;
	bool m_tls = false;
	bool m_crashTrace = false;
	bool m_crashDump = false;
	bool m_parPauseQueue = false;
	bool m_scriptPauseQueue = false;
	bool m_nzbCleanupDisk = false;
	int m_parTimeLimit = 0;
	int m_keepHistory = 0;
	bool m_unpack = false;
	bool m_directUnpack = false;
	bool m_useTempUnpackDir = true;
	bool m_unpackCleanupDisk = false;
	CString m_unrarCmd;
	CString m_sevenZipCmd;
	CString m_unpackPassFile;
	bool m_unpackPauseQueue;
	bool m_unpackMutexEnforced = false;
	CString m_extCleanupDisk;
	CString m_parIgnoreExt;
	CString m_unpackIgnoreExt;
	int m_feedHistory = 0;
	bool m_urlForce = false;
	int m_timeCorrection = 0;
	int m_propagationDelay = 0;
	int m_articleCache = 0;
	int m_eventInterval = 0;
	int m_memoryPressureInterval = 500;
	CString m_memoryPressurePath;
	std::string m_shellOverride;
	int m_monthlyQuota = 0;
	int m_quotaStartDay = 0;
	int m_dailyQuota = 0;
	bool m_reorderFiles = false;
	EFileNaming m_fileNaming = nfArticle;
	bool m_postDownloadRename = true;
	bool m_renameAfterUnpack = true;
	CString m_renameIgnoreExt;
	int m_downloadRate = 0;

	// Application mode
	bool m_serverMode = false;
	bool m_daemonMode = false;
	bool m_remoteClientMode = false;
	bool m_directClientMode = false;

	void Init(const char* exeName, const char* configFilename, bool noConfig,
		CmdOptList* commandLineOptions, bool noDiskAccess, Extender* extender);
	void InitDefaults();
	void InitOptions();
	void InitOptFile();
	void InitServers();
	void InitCategories();
	void InitScheduler();
	void InitFeeds();
	void InitCommandLineOptions(CmdOptList* commandLineOptions);
	void CheckOptions();
	NzbgetRsOptionsHost MakeHost();
	NzbgetRsParsedOptions ExportParsedScalars() const;
	void ApplyParsedScalars(const NzbgetRsParsedOptions& s);
	OptEntry* FindOption(const char* optname);
	void SetOption(const char* optname, const char* value);
	void SetToolPathOption(fs::path& pathOpt, std::string_view value);
	bool SetOptionString(const char* option);
	bool ValidateOptionName(const char* optname, const char* optvalue);
	void LoadConfigFile();
	void CheckDir(CString& dir, const char* optionName, const char* parentDir,
		bool allowEmpty, bool create);
	void ConfigError(const char* msg, ...);
	void ConfigWarn(const char* msg, ...);
};

extern Options* g_Options;

class MemoryMonitor final : public Thread
{
public:
	MemoryMonitor();
	~MemoryMonitor() override;

	void InitOptions();
	void Run() override;
	void Stop() override;

	NzbgetRsPressureTier GetTier() const { return (NzbgetRsPressureTier)m_tier.load(std::memory_order_acquire); }
	const char* GetTierName() const
	{
		return nzbget_rs_awc_memory_monitor_tier_name(
			(int)GetTier(), (int)NZBGET_RS_PRESSURE_CONSTRAINED, (int)NZBGET_RS_PRESSURE_CRITICAL);
	}

	size_t GetEffectiveArticleCacheBytes() const
	{
		return nzbget_rs_tpr_article_cache_mb_to_bytes(
			(int)m_effectiveCacheMB.load(std::memory_order_acquire));
	}
	int GetEffectiveArticleCacheMB() const
	{
		return (int)m_effectiveCacheMB.load(std::memory_order_acquire);
	}
	int GetEffectiveParBufferMB(int configuredMB) const
	{
		int tier = m_tier.load(std::memory_order_acquire);
		int mb = 0;
		if (nzbget_rs_awc_memory_monitor_calc_critical_or_normal_par_buffer(
			tier, configuredMB, (int)NZBGET_RS_PRESSURE_CRITICAL, (int)NZBGET_RS_PRESSURE_NORMAL, &mb) != 0)
		{
			return mb;
		}
		return (int)nzbget_rs_memory_pressure_effective_par_buffer((uint32_t)configuredMB);
	}
	int GetEffectiveDownloadsLimit(int configuredLimit) const
	{
		int tier = m_tier.load(std::memory_order_acquire);
		return nzbget_rs_awc_memory_monitor_effective_downloads_limit(
			tier, configuredLimit, (int)NZBGET_RS_PRESSURE_CONSTRAINED, (int)NZBGET_RS_PRESSURE_CRITICAL);
	}

	static void TrimHeap()
	{
		nzbget_rs_memory_pressure_trim_heap();
	}

private:
	std::atomic<int> m_tier{NZBGET_RS_PRESSURE_NORMAL};
	std::atomic<uint32_t> m_effectiveCacheMB{0};
	std::atomic<uint32_t> m_effectiveParBufferMB{0};
	std::atomic<uint32_t> m_effectiveDownloadsLimit{0};

	int m_consecutiveSamples = 0;
	int m_candidateTier = NZBGET_RS_PRESSURE_NORMAL;

	void UpdateState();
	void ApplyTierTransition(NzbgetRsPressureTier oldTier, NzbgetRsPressureTier newTier);
};

inline MemoryMonitor* g_MemoryMonitor = nullptr;

#include "Util.h"
#include "nzbget_rs.h"
#include "Log.h"
#include "FileSystem.h"

inline const char* PossibleConfigLocations[] =
	{
		"~/.rustyget",
		"~/.config/rustyget.conf",
		"~/.config/rustyget/rustyget.conf",
		"/etc/rustyget.conf",
		"/usr/etc/rustyget.conf",
		"/usr/local/etc/rustyget.conf",
		"/opt/etc/rustyget.conf",
		"~/usr/etc/rustyget.conf",
		"~/.nzbget",
		"~/.config/nzbget.conf",
		"~/.config/nzbget/nzbget.conf",
		"/etc/nzbget.conf",
		"/usr/etc/nzbget.conf",
		"/usr/local/etc/nzbget.conf",
		"/opt/etc/nzbget.conf",
		"~/usr/etc/nzbget.conf",
		nullptr
	};

inline void Options::OptEntry::SetValue(const char* value)
{
	m_value = value;
	if (!m_defValue)
	{
		m_defValue = value;
	}
}

inline bool Options::OptEntry::Restricted()
{
	// rust/src/options.rs
	return nzbget_rs_option_restricted(m_name, (char)-1 < 0) != 0;
}

inline Options::OptEntry* Options::OptEntries::FindOption(const char* name)
{
	if (!name)
	{
		return nullptr;
	}

	for (OptEntry& optEntry : *this)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(optEntry.GetName(), name) != 0)
		{
			return &optEntry;
		}
	}

	return nullptr;
}

inline Options::Category* Options::Categories::FindCategory(const char* name, bool searchAliases)
{
	if (!name)
	{
		return nullptr;
	}

	for (Category& category : *this)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(category.GetName(), name) != 0)
		{
			return &category;
		}
	}

	if (searchAliases)
	{
		for (Category& category : *this)
		{
			std::vector<const char*> aliasPtrs;
			aliasPtrs.reserve(category.GetAliases()->size());
			for (const CString& alias : *category.GetAliases())
			{
				aliasPtrs.push_back(alias ? *alias : "");
			}
			if (nzbget_rs_options_category_alias_list_matches(
				aliasPtrs.data(), aliasPtrs.size(), name, (char)-1 < 0) != 0)
			{
				return &category;
			}
		}
	}

	return nullptr;
}

inline Options::Extender::~Extender() = default;

inline Options::Options(const char* exeName, const char* configFilename, bool noConfig,
	CmdOptList* commandLineOptions, Extender* extender)
{
	Init(exeName, configFilename, noConfig, commandLineOptions, false, extender);
}

inline Options::Options(CmdOptList* commandLineOptions, Extender* extender)
{
	Init("rustyget/rustyget", nullptr, true, commandLineOptions, true, extender);
}

inline void Options::Init(const char* exeName, const char* configFilename, bool noConfig,
	CmdOptList* commandLineOptions, bool noDiskAccess, Extender* extender)
{
	g_Options = this;
	m_extender = extender;
	m_noDiskAccess = noDiskAccess;
	m_noConfig = noConfig;
	m_configFilename = configFilename;

	SetOption(CONFIGFILE.data(), "");

	CString filename;
	if (m_noDiskAccess)
	{
		filename = exeName;
	}
	else
	{
		filename = FileSystem::GetExeFileName(exeName);
	}
	FileSystem::NormalizePathSeparators(filename);
	SetOption(APPBIN.data(), filename);
	NzbgetRsBuf appDirBuf = nzbget_rs_dqh_fs_extract_parent_dir_path(filename, PATH_SEPARATOR);
	filename.Set(appDirBuf.data ? appDirBuf.data : "", (int)appDirBuf.len);
	nzbget_rs_free(appDirBuf);
	SetOption(APPDIR.data(), filename);
	m_appDir = *filename;
	m_appDirPath = fs::u8path(*m_appDir);

	SetOption(APPVERSION.data(), Util::VersionRevision());

	InitDefaults();

	InitOptFile();
	if (m_fatalError)
	{
		return;
	}

	if (commandLineOptions)
	{
		InitCommandLineOptions(commandLineOptions);
	}

	if (!m_configFilename && !noConfig)
	{
		printf("No configuration-file found\n");
		printf("Please use option \"-c\" or put configuration-file in one of the following locations:\n");
		int p = 0;
		while (const char* filename = PossibleConfigLocations[p++])
		{
			printf("%s\n", filename);
		}
		m_fatalError = true;
		return;
	}

	ConvertOldOptions(&m_optEntries);

	CheckDirs();
	InitOptions();
	CheckOptions();

	InitServers();
	InitCategories();
	InitScheduler();
	InitFeeds();
}

inline Options::~Options()
{
	g_Options = nullptr;
}

inline NzbgetRsOptionsHost Options::MakeHost()
{
	NzbgetRsOptionsHost host{};
	host.ctx = this;
	host.find_entry = [](void* ctx, const char* name, const char** outVal, const char** outDefVal, int* outLineNo) -> int
	{
		auto* self = static_cast<Options*>(ctx);
		OptEntry* optEntry = self->FindOption(name);
		if (!optEntry)
		{
			return 0;
		}
		if (outVal) *outVal = optEntry->GetValue();
		if (outDefVal) *outDefVal = optEntry->GetDefValue();
		if (outLineNo) *outLineNo = optEntry->GetLineNo();
		return 1;
	};
	host.set_entry_raw_value = [](void* ctx, const char* name, const char* value)
	{
		auto* self = static_cast<Options*>(ctx);
		if (OptEntry* optEntry = self->FindOption(name))
		{
			optEntry->SetValue(value);
		}
	};
	host.set_option = [](void* ctx, const char* name, const char* value)
	{
		static_cast<Options*>(ctx)->SetOption(name, value);
	};
	host.set_config_line = [](void* ctx, int line)
	{
		static_cast<Options*>(ctx)->m_configLine = line;
	};
	host.get_config_line = [](void* ctx) -> int
	{
		return static_cast<Options*>(ctx)->m_configLine;
	};
	host.config_error = [](void* ctx, const char* msg)
	{
		static_cast<Options*>(ctx)->ConfigError("%s", msg);
	};
	host.config_warn = [](void* ctx, const char* msg)
	{
		static_cast<Options*>(ctx)->ConfigWarn("%s", msg);
	};
	host.file_exists = [](void* /*ctx*/, const char* path) -> int
	{
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	host.ensure_directory = [](void* ctx, const char* dir, char* errBuf, size_t errCap) -> int
	{
		auto* self = static_cast<Options*>(ctx);
		CString errmsg;
		if (!FileSystem::ForceDirectories(dir, errmsg))
		{
			if (errBuf && errCap > 0)
			{
				snprintf(errBuf, errCap, "%s", errmsg ? *errmsg : "");
			}
			return 0;
		}
		if (getuid() == 0 || geteuid() == 0)
		{
			struct passwd* pwd = getpwnam(self->GetOption(DAEMONUSERNAME.data()));
			if (pwd != nullptr)
			{
				chown(dir, pwd->pw_uid, pwd->pw_gid);
			}
		}
		return 1;
	};
	host.add_news_server = [](void* ctx, int id, int active, const char* name, const char* host,
		int port, int ipVersion, const char* user, const char* pass, int joinGroup, int tls,
		const char* cipher, int maxConnections, int retention, int level, int group, int optional,
		uint32_t certVerifLevel)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddNewsServer(id, active != 0, name, host, port, ipVersion,
				user, pass, joinGroup != 0, tls != 0, cipher, maxConnections, retention,
				level, group, optional != 0, certVerifLevel);
		}
	};
	host.add_feed = [](void* ctx, int id, const char* name, const char* url, int interval,
		const char* filter, int backlog, int pauseNzb, const char* category, int categorySource,
		int priority, const char* extensions, uint32_t certVerifLevel)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddFeed(id, name, url, interval, filter, backlog != 0, pauseNzb != 0,
				category, static_cast<FeedInfo::CategorySource>(categorySource), priority,
				extensions, certVerifLevel);
		}
	};
	host.add_task = [](void* ctx, int id, int hours, int minutes, int weekDaysBits,
		int command, const char* param)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddTask(id, hours, minutes, weekDaysBits,
				static_cast<ESchedulerCommand>(command), param);
		}
	};
	host.add_category = [](void* ctx, const char* name, const char* destDir, int unpack,
		const char* extensions, const char* const* aliases, size_t aliasesLen)
	{
		auto* self = static_cast<Options*>(ctx);
		self->m_categories.emplace_back(name, destDir, unpack != 0, extensions);
		Category& category = self->m_categories.back();
		for (size_t i = 0; i < aliasesLen; ++i)
		{
			category.GetAliases()->push_back(aliases[i]);
		}
	};
	return host;
}

inline NzbgetRsParsedOptions Options::ExportParsedScalars() const
{
	NzbgetRsParsedOptions s{};
	s.download_rate = m_downloadRate;
	s.article_timeout = m_articleTimeout;
	s.article_read_chunk_size = m_articleReadChunkSize;
	s.url_timeout = m_urlTimeout;
	s.remote_timeout = m_remoteTimeout;
	s.article_retries = m_articleRetries;
	s.article_interval = m_articleInterval;
	s.url_retries = m_urlRetries;
	s.url_interval = m_urlInterval;
	s.control_port = m_controlPort;
	s.secure_port = m_securePort;
	s.url_connections = m_urlConnections;
	s.log_buffer = m_logBuffer;
	s.rotate_log = m_rotateLog;
	s.umask = m_umask;
	s.update_interval = m_updateInterval;
	s.write_buffer = m_writeBuffer;
	s.nzb_dir_interval = m_nzbDirInterval;
	s.nzb_dir_file_age = m_nzbDirFileAge;
	s.nzb_dir_archive_scan = m_nzbDirArchiveScan ? 1 : 0;
	s.nzb_dir_archive_action = (int)m_nzbDirArchiveAction;
	s.disk_space = m_diskSpace;
	s.par_time_limit = m_parTimeLimit;
	s.keep_history = m_keepHistory;
	s.feed_history = m_feedHistory;
	s.time_correction = m_timeCorrection;
	s.propagation_delay = m_propagationDelay;
	s.article_cache = m_articleCache;
	s.memory_pressure_interval = m_memoryPressureInterval;
	s.event_interval = m_eventInterval;
	s.par_buffer = m_parBuffer;
	s.par_threads = m_parThreads;
	s.monthly_quota = m_monthlyQuota;
	s.quota_start_day = m_quotaStartDay;
	s.daily_quota = m_dailyQuota;
	s.system_health_check = m_systemHealthCheck ? 1 : 0;
	s.nzb_log = m_nzbLog ? 1 : 0;
	s.append_category_dir = m_appendCategoryDir ? 1 : 0;
	s.continue_partial = m_continuePartial ? 1 : 0;
	s.flush_queue = m_flushQueue ? 1 : 0;
	s.dupe_check = m_dupeCheck ? 1 : 0;
	s.par_repair = m_parRepair ? 1 : 0;
	s.par_quick = m_parQuick ? 1 : 0;
	s.par_rename = m_parRename ? 1 : 0;
	s.rar_rename = m_rarRename ? 1 : 0;
	s.direct_rename = m_directRename ? 1 : 0;
	s.hard_linking = m_hardLinking ? 1 : 0;
	s.curses_nzb_name = m_cursesNzbName ? 1 : 0;
	s.curses_time = m_cursesTime ? 1 : 0;
	s.curses_group = m_cursesGroup ? 1 : 0;
	s.crc_check = m_crcCheck ? 1 : 0;
	s.direct_write = m_directWrite ? 1 : 0;
	s.raw_article = m_rawArticle ? 1 : 0;
	s.skip_write = m_skipWrite ? 1 : 0;
	s.crash_trace = m_crashTrace ? 1 : 0;
	s.crash_dump = m_crashDump ? 1 : 0;
	s.par_pause_queue = m_parPauseQueue ? 1 : 0;
	s.script_pause_queue = m_scriptPauseQueue ? 1 : 0;
	s.nzb_cleanup_disk = m_nzbCleanupDisk ? 1 : 0;
	s.form_auth = m_formAuth ? 1 : 0;
	s.json_p = m_jsonP ? 1 : 0;
	s.secure_control = m_secureControl ? 1 : 0;
	s.unpack = m_unpack ? 1 : 0;
	s.direct_unpack = m_directUnpack ? 1 : 0;
	s.use_temp_unpack_dir = m_useTempUnpackDir ? 1 : 0;
	s.unpack_cleanup_disk = m_unpackCleanupDisk ? 1 : 0;
	s.unpack_pause_queue = m_unpackPauseQueue ? 1 : 0;
	s.unpack_mutex_enforced = m_unpackMutexEnforced ? 1 : 0;
	s.url_force = m_urlForce ? 1 : 0;
	s.cert_check = m_certCheck ? 1 : 0;
	s.reorder_files = m_reorderFiles ? 1 : 0;
	s.post_download_rename = m_postDownloadRename ? 1 : 0;
	s.rename_after_unpack = m_renameAfterUnpack ? 1 : 0;
	s.output_mode = (int)m_outputMode;
	s.par_check = (int)m_parCheck;
	s.par_scan = (int)m_parScan;
	s.dupe_article_fallback = (int)m_dupeArticleFallback;
	s.dupe_stream_decompress = m_dupeStreamDecompress ? 1 : 0;
	s.dupe_stream_timeout = m_dupeStreamTimeout;
	s.dupe_search = m_dupeSearch ? 1 : 0;
	s.dupe_search_dry_run = m_dupeSearchDryRun ? 1 : 0;
	s.dupe_search_delay = m_dupeSearchDelay;
	s.dupe_search_max_donors = m_dupeSearchMaxDonors;
	s.dupe_health_percent = m_dupeHealthPercent;
	s.dupe_health_min = m_dupeHealthMin;
	s.dupe_health_max = m_dupeHealthMax;
	s.dupe_body_checks = m_dupeBodyChecks;
	s.dupe_health_budget = m_dupeHealthBudget;
	s.dupe_min_alive = m_dupeMinAlive;
	s.dupe_fast_donors = m_dupeFastDonors;
	s.post_strategy = (int)m_postStrategy;
	s.file_naming = (int)m_fileNaming;
	s.health_check = (int)m_healthCheck;
	s.info_target = (int)m_infoTarget;
	s.warning_target = (int)m_warningTarget;
	s.error_target = (int)m_errorTarget;
	s.debug_target = (int)m_debugTarget;
	s.detail_target = (int)m_detailTarget;
	s.write_log = (int)m_writeLog;
	s.tls = m_tls ? 1 : 0;
	return s;
}

inline void Options::ApplyParsedScalars(const NzbgetRsParsedOptions& s)
{
	m_downloadRate = s.download_rate;
	m_articleTimeout = s.article_timeout;
	m_articleReadChunkSize = s.article_read_chunk_size;
	m_urlTimeout = s.url_timeout;
	m_remoteTimeout = s.remote_timeout;
	m_articleRetries = s.article_retries;
	m_articleInterval = s.article_interval;
	m_urlRetries = s.url_retries;
	m_urlInterval = s.url_interval;
	m_controlPort = s.control_port;
	m_securePort = s.secure_port;
	m_urlConnections = s.url_connections;
	m_logBuffer = s.log_buffer;
	m_rotateLog = s.rotate_log;
	m_umask = s.umask;
	m_updateInterval = s.update_interval;
	m_writeBuffer = s.write_buffer;
	m_nzbDirInterval = s.nzb_dir_interval;
	m_nzbDirFileAge = s.nzb_dir_file_age;
	m_nzbDirArchiveScan = s.nzb_dir_archive_scan != 0;
	m_nzbDirArchiveAction = (ENzbDirArchiveAction)s.nzb_dir_archive_action;
	m_diskSpace = s.disk_space;
	m_parTimeLimit = s.par_time_limit;
	m_keepHistory = s.keep_history;
	m_feedHistory = s.feed_history;
	m_timeCorrection = s.time_correction;
	m_propagationDelay = s.propagation_delay;
	m_articleCache = s.article_cache;
	m_memoryPressureInterval = s.memory_pressure_interval;
	m_eventInterval = s.event_interval;
	m_parBuffer = s.par_buffer;
	m_parThreads = s.par_threads;
	m_monthlyQuota = s.monthly_quota;
	m_quotaStartDay = s.quota_start_day;
	m_dailyQuota = s.daily_quota;
	m_systemHealthCheck = s.system_health_check != 0;
	m_nzbLog = s.nzb_log != 0;
	m_appendCategoryDir = s.append_category_dir != 0;
	m_continuePartial = s.continue_partial != 0;
	m_flushQueue = s.flush_queue != 0;
	m_dupeCheck = s.dupe_check != 0;
	m_parRepair = s.par_repair != 0;
	m_parQuick = s.par_quick != 0;
	m_parRename = s.par_rename != 0;
	m_rarRename = s.rar_rename != 0;
	m_directRename = s.direct_rename != 0;
	m_hardLinking = s.hard_linking != 0;
	m_cursesNzbName = s.curses_nzb_name != 0;
	m_cursesTime = s.curses_time != 0;
	m_cursesGroup = s.curses_group != 0;
	m_crcCheck = s.crc_check != 0;
	m_directWrite = s.direct_write != 0;
	m_rawArticle = s.raw_article != 0;
	m_skipWrite = s.skip_write != 0;
	m_crashTrace = s.crash_trace != 0;
	m_crashDump = s.crash_dump != 0;
	m_parPauseQueue = s.par_pause_queue != 0;
	m_scriptPauseQueue = s.script_pause_queue != 0;
	m_nzbCleanupDisk = s.nzb_cleanup_disk != 0;
	m_formAuth = s.form_auth != 0;
	m_jsonP = s.json_p != 0;
	m_secureControl = s.secure_control != 0;
	m_unpack = s.unpack != 0;
	m_directUnpack = s.direct_unpack != 0;
	m_useTempUnpackDir = s.use_temp_unpack_dir != 0;
	m_unpackCleanupDisk = s.unpack_cleanup_disk != 0;
	m_unpackPauseQueue = s.unpack_pause_queue != 0;
	m_unpackMutexEnforced = s.unpack_mutex_enforced != 0;
	m_urlForce = s.url_force != 0;
	m_certCheck = s.cert_check != 0;
	m_reorderFiles = s.reorder_files != 0;
	m_postDownloadRename = s.post_download_rename != 0;
	m_renameAfterUnpack = s.rename_after_unpack != 0;
	m_outputMode = (EOutputMode)s.output_mode;
	m_parCheck = (EParCheck)s.par_check;
	m_parScan = (EParScan)s.par_scan;
	m_dupeArticleFallback = (EDupeArticleFallback)s.dupe_article_fallback;
	m_dupeStreamDecompress = s.dupe_stream_decompress != 0;
	m_dupeStreamTimeout = s.dupe_stream_timeout;
	m_dupeSearch = s.dupe_search != 0;
	m_dupeSearchDryRun = s.dupe_search_dry_run != 0;
	m_dupeSearchDelay = s.dupe_search_delay;
	m_dupeSearchMaxDonors = s.dupe_search_max_donors;
	m_dupeHealthPercent = s.dupe_health_percent;
	m_dupeHealthMin = s.dupe_health_min;
	m_dupeHealthMax = s.dupe_health_max;
	m_dupeBodyChecks = s.dupe_body_checks;
	m_dupeHealthBudget = s.dupe_health_budget;
	m_dupeMinAlive = s.dupe_min_alive;
	m_dupeFastDonors = s.dupe_fast_donors;
	m_postStrategy = (EPostStrategy)s.post_strategy;
	m_fileNaming = (EFileNaming)s.file_naming;
	m_healthCheck = (EHealthCheck)s.health_check;
	m_infoTarget = (EMessageTarget)s.info_target;
	m_warningTarget = (EMessageTarget)s.warning_target;
	m_errorTarget = (EMessageTarget)s.error_target;
	m_debugTarget = (EMessageTarget)s.debug_target;
	m_detailTarget = (EMessageTarget)s.detail_target;
	m_writeLog = (EWriteLog)s.write_log;
	m_tls = s.tls != 0;
}

inline void Options::ConfigError(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	printf("%s(%i): %s\n", m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>", m_configLine, tmp2);
	error("%s(%i): %s", m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>", m_configLine, tmp2);

	m_configErrors = true;
}

inline void Options::ConfigWarn(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	const char* filename = m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>";
	printf("%s(%i): %s\n", filename, m_configLine, tmp2);
	warn("%s(%i): %s", filename, m_configLine, tmp2);
}

inline void Options::InitDefaults()
{
#ifdef DISABLE_CURSES
	int disableCurses = 1;
#else
	int disableCurses = 0;
#endif
	nzbget_rs_options_init_defaults(
		disableCurses,
		[](void* ctx, const char* name, const char* value)
		{
			static_cast<Options*>(ctx)->SetOption(name, value);
		},
		this);
}

inline void Options::InitOptFile()
{
	if (!m_configFilename && !m_noConfig)
	{
		// search for config file in default locations
		// look in the exe-directory first
		BString<1024> filename;
		{
			NzbgetRsBuf cfBuf = nzbget_rs_tpr_options_format_default_conf_path(*m_appDir, '/');
			filename.Set(cfBuf.data ? cfBuf.data : "", (int)cfBuf.len);
			nzbget_rs_free(cfBuf);
		}

		if (FileSystem::FileExists(filename))
		{
			m_configFilename = filename;
		}
		else
		{
			int p = 0;
			while (const char* altfilename = PossibleConfigLocations[p++])
			{
				// substitute HOME-variable
				filename = FileSystem::ExpandHomePath(altfilename);

				if (FileSystem::FileExists(filename))
				{
					m_configFilename = *filename;
					break;
				}
			}
		}
	}

	if (m_configFilename)
	{
		// normalize path in filename
		CString filename = FileSystem::ExpandFileName(m_configFilename);

		// substitute HOME-variable
		filename = FileSystem::ExpandHomePath(filename);

		m_configFilename = *filename;
		m_configFilePath = fs::u8path(*m_configFilename);

		SetOption(CONFIGFILE.data(), m_configFilename);

		LoadConfigFile();
	}
}

inline void Options::CheckDir(CString& dir, const char* optionName,
	const char* parentDir, bool allowEmpty, bool create)
{
	NzbgetRsOptionsHost host = MakeHost();
	int hasDir = 0;
	NzbgetRsBuf buf = nzbget_rs_options_check_dir(
		&host, m_noDiskAccess ? 1 : 0, optionName, parentDir,
		allowEmpty ? 1 : 0, create ? 1 : 0, &hasDir);
	if (hasDir)
	{
		dir = buf.data ? buf.data : "";
	}
	else
	{
		dir = nullptr;
	}
	nzbget_rs_free(buf);
}

// ScriptDir may list several folders: each is made absolute and created on
// its own (as one path, "a;b" created junk folders such as "a;")
inline void Options::CheckScriptDir()
{
	NzbgetRsOptionsHost host = MakeHost();
	int hasDir = 0;
	NzbgetRsBuf buf = nzbget_rs_options_check_script_dir(
		&host, m_noDiskAccess ? 1 : 0, m_mainDir, &hasDir);
	if (hasDir)
	{
		m_scriptDir = buf.data ? buf.data : "";
	}
	else
	{
		m_scriptDir = nullptr;
	}
	nzbget_rs_free(buf);
}

inline void Options::CheckDirs()
{
	m_mainDir = GetOption(MAINDIR.data());

	CheckDir(m_destDir, DESTDIR.data(), m_mainDir, false, true);
	CheckDir(m_interDir, INTERDIR.data(), m_mainDir, true, true);
	CheckDir(m_tempDir, TEMPDIR.data(), m_mainDir, false, true);
	CheckDir(m_queueDir, QUEUEDIR.data(), m_mainDir, false, true);
	CheckDir(m_webDir, WEBDIR.data(), nullptr, true, false);
	CheckScriptDir();
	CheckDir(m_nzbDir, NZBDIR.data(), m_mainDir, false, true);

	m_mainDirPath = fs::u8path(*m_mainDir);
	m_destDirPath = fs::u8path(*m_destDir);
	m_interDirPath = fs::u8path(*m_interDir);
	m_tempDirPath = fs::u8path(*m_tempDir);
	m_queueDirPath = fs::u8path(*m_queueDir);
	m_webDirPath = fs::u8path(*m_webDir);
	m_nzbDirPath = fs::u8path(*m_nzbDir);

	Tokenizer tokDir(g_Options->GetScriptDir(), ",;");
	while (const char* scriptDir = tokDir.Next())
	{
		fs::path path;
		path = fs::u8path(scriptDir);
		m_scriptDirPaths.push_back(std::move(path));
	}
}

inline void Options::InitOptions()
{
	m_requiredDir = GetOption(REQUIREDDIR.data());

	m_configTemplate		= GetOption(CONFIGTEMPLATE.data());
	m_scriptOrder			= GetOption(SCRIPTORDER.data());
	m_extensions			= GetOption(EXTENSIONS.data());
	m_controlIp				= GetOption(CONTROLIP.data());
	m_controlUsername		= GetOption(CONTROLUSERNAME.data());
	m_controlPassword		= GetOption(CONTROLPASSWORD.data());
	m_restrictedUsername	= GetOption(RESTRICTEDUSERNAME.data());
	m_restrictedPassword	= GetOption(RESTRICTEDPASSWORD.data());
	m_addUsername			= GetOption(ADDUSERNAME.data());
	m_addPassword			= GetOption(ADDPASSWORD.data());
	m_secureCert			= GetOption(SECURECERT.data());
	m_secureKey				= GetOption(SECUREKEY.data());
	m_certStore				= GetOption(CERTSTORE.data());
	m_authorizedIp			= GetOption(AUTHORIZEDIP.data());
	m_lockFile				= GetOption(LOCKFILE.data());
	m_daemonUsername		= GetOption(DAEMONUSERNAME.data());
	m_logFile				= GetOption(LOGFILE.data());
	m_unrarCmd				= GetOption(UNRARCMD.data());
	m_sevenZipCmd			= GetOption(SEVENZIPCMD.data());
	m_unpackPassFile		= GetOption(UNPACKPASSFILE.data());
	m_extCleanupDisk		= GetOption(EXTCLEANUPDISK.data());
	m_parIgnoreExt			= GetOption(PARIGNOREEXT.data());
	m_unpackIgnoreExt		= GetOption(UNPACKIGNOREEXT.data());

	m_configTemplatePath = fs::u8path(*m_configTemplate);
	m_secureCertPath = fs::u8path(*m_secureCert);
	m_secureKeyPath = fs::u8path(*m_secureKey);
	m_certStorePath = fs::u8path(*m_certStore);
	m_lockFilePath = fs::u8path(*m_lockFile);
	m_logFilePath = fs::u8path(*m_logFile);
	m_unpackPassFilePath = fs::u8path(*m_unpackPassFile);
	SetToolPathOption(m_unrarPath, *m_unrarCmd);
	SetToolPathOption(m_sevenZipPath, *m_sevenZipCmd);

	const char* shellOverride = GetOption(SHELLOVERRIDE.data());
	m_shellOverride	= shellOverride ? shellOverride : "";

	m_renameIgnoreExt 	    = GetOption(RENAMEIGNOREEXT.data());

	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars{};
	int hasMemPath = 0, hasHlExt = 0, hasDupeUrl = 0, hasDupeKey = 0, warnDupeNoUrl = 0;
	NzbgetRsBuf memPathBuf{}, hlExtBuf{}, dupeUrlBuf{}, dupeKeyBuf{};
	nzbget_rs_options_init_scalars(
		&host, &scalars,
		&hasMemPath, &memPathBuf,
		&hasHlExt, &hlExtBuf,
		&hasDupeUrl, &dupeUrlBuf,
		&hasDupeKey, &dupeKeyBuf,
		&warnDupeNoUrl);
	m_memoryPressurePath = hasMemPath ? (memPathBuf.data ? memPathBuf.data : "") : nullptr;
	m_hardLinkingIgnoreExt = hasHlExt ? (hlExtBuf.data ? hlExtBuf.data : "") : nullptr;
	m_dupeSearchUrl = hasDupeUrl ? (dupeUrlBuf.data ? dupeUrlBuf.data : "") : nullptr;
	m_dupeSearchApiKey = hasDupeKey ? (dupeKeyBuf.data ? dupeKeyBuf.data : "") : nullptr;
	nzbget_rs_free(memPathBuf);
	nzbget_rs_free(hlExtBuf);
	nzbget_rs_free(dupeUrlBuf);
	nzbget_rs_free(dupeKeyBuf);
	ApplyParsedScalars(scalars);

	if (warnDupeNoUrl)
	{
		warn("Option <%s> is enabled but option <%s> is empty: no duplicates will be searched",
			DUPESEARCH.data(), DUPESEARCHURL.data());
	}
}

inline void Options::SetOption(const char* optname, const char* value)
{
	OptEntry* optEntry = FindOption(optname);
	if (!optEntry)
	{
		m_optEntries.emplace_back(optname, nullptr);
		optEntry = &m_optEntries.back();
	}

	CString curvalue;

	if (nzbget_rs_dqh_options_should_expand_home_value(value, m_noDiskAccess ? 1 : 0) != 0)
	{
		curvalue = FileSystem::ExpandHomePath(value);
	}
	else
	{
		curvalue = value;
	}

	optEntry->SetLineNo(m_configLine);

	if (curvalue)
	{
		NzbgetRsBuf expanded = nzbget_rs_options_expand_variables(
			curvalue,
			[](void* ctx, const char* name) -> const char*
			{
				return static_cast<Options*>(ctx)->GetOption(name);
			},
			this);
		curvalue.Set(expanded.data ? expanded.data : "", (int)expanded.len);
		nzbget_rs_free(expanded);
	}

	optEntry->SetValue(curvalue);
}

inline void Options::SetToolPathOption(fs::path& pathOpt, std::string_view value)
{
	if (value.empty()) return;

	NzbgetRsBuf resolved = nzbget_rs_options_resolve_tool_path(value.data(), value.size());
	std::string_view sv(resolved.data ? resolved.data : "", resolved.len);
	fs::path directPath = fs::u8path(sv);
	nzbget_rs_free(resolved);
	pathOpt.swap(directPath);
}

inline Options::OptEntry* Options::FindOption(const char* optname)
{
	OptEntry* optEntry = m_optEntries.FindOption(optname);

	// normalize option name in option list; for example "server1.joingroup" -> "Server1.JoinGroup"
	if (optEntry && nzbget_rs_dqh_bytes_equal(optEntry->GetName(), optname) == 0)
	{
		optEntry->SetName(optname);
	}

	return optEntry;
}

inline const char* Options::GetOption(const char* optname)
{
	OptEntry* optEntry = FindOption(optname);
	if (optEntry)
	{
		m_configLine = nzbget_rs_dqh_options_update_config_line_on_get(
			optEntry->GetLineNo(), m_configLine);
		return optEntry->GetValue();
	}
	return nullptr;
}

inline void Options::InitServers()
{
	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars = ExportParsedScalars();
#ifdef DISABLE_TLS
	int disableTls = 1;
#else
	int disableTls = 0;
#endif
	nzbget_rs_options_init_servers(&host, disableTls, &scalars);
	ApplyParsedScalars(scalars);
}

inline void Options::InitCategories()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_categories(&host, m_noDiskAccess ? 1 : 0, m_destDir);
}

inline void Options::InitFeeds()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_feeds(&host);
}

inline void Options::InitScheduler()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_scheduler(&host);
}

inline void Options::CreateSchedulerTask(int id, const char* time, const char* weekDays,
	ESchedulerCommand command, const char* param)
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_create_scheduler_task(&host, id, time, weekDays, (int)command, param);
}

inline void Options::LoadConfigFile()
{
	SetOption(CONFIGFILE.data(), m_configFilename);

	DiskFile infile;

	if (!infile.Open(m_configFilename, DiskFile::omRead))
	{
		ConfigError("Could not open file %s", *m_configFilename);
		m_fatalError = true;
		return;
	}

	m_configLine = 0;
	int bufLen = (int)FileSystem::FileSize(m_configFilename) + 1;
	CharBuffer buf(bufLen);

	int line = 0;
	while (infile.ReadLine(buf, buf.Size() - 1))
	{
		m_configLine = ++line;

		if (nzbget_rs_rco_options_trim_and_check_config_line(buf))
		{
			continue;
		}

		SetOptionString(buf);
	}

	infile.Close();

	m_configLine = 0;
}

inline void Options::InitCommandLineOptions(CmdOptList* commandLineOptions)
{
	for (const char* option : *commandLineOptions)
	{
		SetOptionString(option);
	}
}

inline bool Options::SetOptionString(const char* option)
{
	CString optname;
	CString optvalue;

	if (!SplitOptionString(option, optname, optvalue))
	{
		ConfigError("Invalid option \"%s\"", option);
		return false;
	}

	bool ok = ValidateOptionName(optname, optvalue);
	if (ok)
	{
		SetOption(optname, optvalue);
	}
	else
	{
		ConfigError("Invalid option \"%s\"", *optname);
	}

	return ok;
}

/*
 * Splits option string into name and value;
 * Converts old names and values if necessary;
 * Returns true if the option string has name and value;
 */
inline bool Options::SplitOptionString(const char* option, CString& optName, CString& optValue)
{
	NzbgetRsBuf nameBuf{}, valBuf{};
	if (!nzbget_rs_options_split_and_convert(option, &nameBuf, &valBuf))
	{
		return false;
	}

	optName.Set(nameBuf.data ? nameBuf.data : "", (int)nameBuf.len);
	optValue.Set(valBuf.data ? valBuf.data : "", (int)valBuf.len);
	nzbget_rs_free(nameBuf);
	nzbget_rs_free(valBuf);

	return true;
}

inline bool Options::ValidateOptionName(const char* optname, const char* optvalue)
{
	// rust/src/options.rs: the name's class; the messages are logged here
	auto predefined = [](void* ctx, const char* name) -> int
	{
		return static_cast<Options*>(ctx)->GetOption(name) != nullptr;
	};
	switch (nzbget_rs_validate_option_name(optname, predefined, this))
	{
		case 1:
			return true;

		case 2:
			ConfigWarn("Option \"%s\" is obsolete, ignored", optname);
			return true;

		case 3:
			if (!Util::EmptyStr(optvalue))
			{
				ConfigError("Option \"%s\" is obsolete, ignored, use \"%s\" and \"%s\" instead",
					optname, SCRIPTDIR.data(), EXTENSIONS.data());
			}
			return true;

		case 4:
			ConfigWarn("Option \"%s\" is obsolete, ignored, use \"%s\" instead", optname, WRITELOG.data());
			return true;

		default:
			return false;
	}
}

inline void Options::CheckOptions()
{
	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars = ExportParsedScalars();
#ifdef DISABLE_PARCHECK
	int disableParcheck = 1;
#else
	int disableParcheck = 0;
#endif
#ifdef DISABLE_CURSES
	int disableCurses = 1;
#else
	int disableCurses = 0;
#endif
#ifdef DISABLE_TLS
	int disableTls = 1;
#else
	int disableTls = 0;
#endif
#ifdef HAVE_X509_CHECK_HOST
	int haveX509CheckHost = 1;
#else
	int haveX509CheckHost = 0;
#endif
	int ctChanged = 0;
	NzbgetRsBuf ctBuf = nzbget_rs_options_check_options(
		&host, &scalars,
		m_noDiskAccess ? 1 : 0,
		sizeof(void*) == 4 ? 1 : 0,
		disableParcheck, disableCurses, disableTls, haveX509CheckHost,
		m_certStore.Empty() ? 1 : 0,
		m_webDir ? *m_webDir : "",
		m_configTemplate ? *m_configTemplate : nullptr,
		m_unpackPassFile ? *m_unpackPassFile : nullptr,
		&ctChanged);
	if (ctChanged)
	{
		m_configTemplate.Set(ctBuf.data ? ctBuf.data : "", (int)ctBuf.len);
	}
	nzbget_rs_free(ctBuf);
	ApplyParsedScalars(scalars);
}

inline void Options::ConvertOldOptions(OptEntries* optEntries)
{
	if (!optEntries) return;
	std::vector<const char*> names;
	std::vector<const char*> values;
	names.reserve(optEntries->size());
	values.reserve(optEntries->size());
	for (const OptEntry& opt : *optEntries)
	{
		names.push_back(opt.GetName() ? opt.GetName() : "");
		values.push_back(opt.GetValue() ? opt.GetValue() : "");
	}
	nzbget_rs_options_convert_old_options(
		names.data(),
		values.data(),
		names.size(),
		optEntries,
		[](void* ctx, size_t idx, const char* name, const char* value)
		{
			auto* entries = static_cast<OptEntries*>(ctx);
			if (idx < entries->size())
			{
				(*entries)[idx].m_value = value;
			}
			else
			{
				entries->emplace_back(name, value);
			}
		});
}

#ifndef DECODER_H
#define DECODER_H

class Decoder
{
public:
	enum EStatus
	{
		dsUnknownError,
		dsFinished,
		dsArticleIncomplete,
		dsCrcError,
		dsInvalidSize,
		dsNoBinaryData
	};

	enum EFormat
	{
		efUnknown,
		efYenc,
		efUx,
	};

	// rust/src/decoder.rs
	Decoder() : m_decoder(nzbget_rs_decoder_new(nullptr, nullptr)) {}
	~Decoder() { nzbget_rs_decoder_free(m_decoder); }
	Decoder(const Decoder&) = delete;
	Decoder& operator=(const Decoder&) = delete;
	EStatus Check() { return static_cast<EStatus>(nzbget_rs_decoder_check(m_decoder)); }
	void Clear() { nzbget_rs_decoder_clear(m_decoder); }
	int DecodeBuffer(char* buffer, int len) { return nzbget_rs_decoder_decode(m_decoder, buffer, len); }
	void SetCrcCheck(bool crcCheck) { nzbget_rs_decoder_set(m_decoder, 0, crcCheck); }
	void SetRawMode(bool rawMode) { nzbget_rs_decoder_set(m_decoder, 1, rawMode); }
	EFormat GetFormat() { return static_cast<EFormat>(nzbget_rs_decoder_get(m_decoder, 0)); }
	int64 GetBeginPos() { return nzbget_rs_decoder_get(m_decoder, 1); }
	int64 GetEndPos() { return nzbget_rs_decoder_get(m_decoder, 2); }
	int64 GetSize() { return nzbget_rs_decoder_get(m_decoder, 3); }
	uint32 GetExpectedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 4)); }
	uint32 GetCalculatedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 5)); }
	bool GetEof() { return nzbget_rs_decoder_get(m_decoder, 6) != 0; }
	const char* GetArticleFilename() { return nzbget_rs_decoder_filename(m_decoder); }

private:
	struct NzbgetRsDecoder* m_decoder;
};

#endif

#ifndef PARPARSER_H
#define PARPARSER_H

class ParParser
{
public:
	typedef std::vector<std::string> ParFileList;

	static bool FindMainPars(const char* path, ParFileList* fileList)
	{
		if (fileList)
		{
			fileList->clear();
		}
		return nzbget_rs_find_main_pars(
			path,
			fileList != nullptr ? 1 : 0,
			fileList ? +[](void* ctx, const char* name, size_t len)
			{
				static_cast<ParFileList*>(ctx)->emplace_back(name, len);
			} : nullptr,
			fileList) != 0;
	}

	static bool ParseParFilename(const char* parFilename, bool confirmedFilename, int* baseNameLen, int* blocks)
	{
		return nzbget_rs_parse_par_filename(parFilename, confirmedFilename, baseNameLen, blocks) != 0;
	}

	static bool SameParCollection(const char* filename1, const char* filename2, bool confirmedFilenames)
	{
		return nzbget_rs_same_par_collection(filename1, filename2, confirmedFilenames) != 0;
	}

	static constexpr int64 MaxIndexParSize = 16 * 1024 * 1024;

	static bool ListParredFiles(const char* parFilename, std::vector<std::string>& filenames)
	{
		return nzbget_rs_par_renamer_list_parred_files(
			parFilename,
			MaxIndexParSize,
			&filenames,
			+[](void* rawCtx, const char* name) noexcept
			{
				if (name)
				{
					static_cast<std::vector<std::string>*>(rawCtx)->emplace_back(name);
				}
			}) != 0;
	}
};

#endif

#ifndef WORKSTATE_H
#define WORKSTATE_H

class WorkState final : public Subject
{
public:
	void SetPauseDownload(bool pauseDownload) { m_pauseDownload = pauseDownload; Changed(); }
	bool GetPauseDownload() const { return m_pauseDownload; }
	void SetPausePostProcess(bool pausePostProcess) { m_pausePostProcess = pausePostProcess; Changed(); }
	bool GetPausePostProcess() const { return m_pausePostProcess; }
	void SetPauseScan(bool pauseScan) { m_pauseScan = pauseScan; Changed(); }
	bool GetPauseScan() const { return m_pauseScan; }
	void SetTempPauseDownload(bool tempPauseDownload) { m_tempPauseDownload = tempPauseDownload; Changed(); }
	bool GetTempPauseDownload() const { return m_tempPauseDownload; }
	void HoldDownloadForFleet(bool hold) { m_fleetHold += hold ? 1 : -1; Changed(); }
	bool GetDownloadHeld() const { return m_tempPauseDownload || m_fleetHold > 0; }
	void SetTempPausePostprocess(bool tempPausePostprocess) { m_tempPausePostprocess = tempPausePostprocess; Changed(); }
	bool GetTempPausePostprocess() const { return m_tempPausePostprocess; }
	void SetPauseFrontend(bool pauseFrontend) { m_pauseFrontend = pauseFrontend; Changed(); }
	bool GetPauseFrontend() const { return m_pauseFrontend; }
	void SetSpeedLimit(int speedLimit) { m_speedLimit = speedLimit; Changed(); }
	int GetSpeedLimit() const { return m_speedLimit; }
	void SetResumeTime(time_t resumeTime) { m_resumeTime = resumeTime; Changed(); }
	time_t GetResumeTime() const { return m_resumeTime; }
	void SetLocalTimeOffset(int localTimeOffset) { m_localTimeOffset = localTimeOffset; Changed(); }
	int GetLocalTimeOffset() { return m_localTimeOffset; }
	void SetQuotaReached(bool quotaReached) { m_quotaReached = quotaReached; Changed(); }
	bool GetQuotaReached() { return m_quotaReached; }
	void SetDownloading(bool downloading) { m_downloading = downloading; Changed(); }
	bool GetDownloading() { return m_downloading; }

private:
	std::atomic<time_t> m_resumeTime{0};
	std::atomic<int> m_localTimeOffset{0};
	std::atomic<int> m_speedLimit{0};
	std::atomic<bool> m_tempPauseDownload{true};
	std::atomic<int> m_fleetHold{0};
	std::atomic<bool> m_tempPausePostprocess{true};
	std::atomic<bool> m_pauseDownload{false};
	std::atomic<bool> m_pausePostProcess{false};
	std::atomic<bool> m_pauseScan{false};
	std::atomic<bool> m_pauseFrontend{false};
	std::atomic<bool> m_downloading{false};
	std::atomic<bool> m_quotaReached{false};

	void Changed() { Notify(nullptr); }
};

extern WorkState* g_WorkState;

#endif

#ifndef NEWSSERVER_H
#define NEWSSERVER_H

class NewsServer
{
public:
	NewsServer(int id, bool active, const char* name, const char* host, int port, int ipVersion,
		const char* user, const char* pass, bool joinGroup,
		bool tls, const char* cipher, int maxConnections, int retention,
		int level, int group, bool optional, unsigned int certVerificationfLevel) :
		m_id(id), m_active(active), m_name(name), m_host(host ? host : ""), m_port(port), m_ipVersion(ipVersion),
		m_user(user ? user : ""), m_password(pass ? pass : ""), m_joinGroup(joinGroup), m_tls(tls),
		m_cipher(cipher ? cipher : ""), m_maxConnections(maxConnections), m_retention(retention),
		m_level(level), m_normLevel(level), m_group(group), m_optional(optional), m_certVerificationfLevel(certVerificationfLevel)
	{
		if (m_name.Empty())
		{
			m_name.Format("Server%i", id);
		}
	}
	int GetId() const { return m_id; }
	int GetStateId() const { return m_stateId; }
	void SetStateId(int stateId) { m_stateId = stateId; }
	bool GetActive() const { return m_active; }
	void SetActive(bool active) { m_active = active; }
	const char* GetName() const { return m_name; }
	int GetGroup() const { return m_group; }
	const char* GetHost() const { return m_host; }
	int GetPort() const { return m_port; }
	int GetIpVersion() const { return m_ipVersion; }
	const char* GetUser() const { return m_user; }
	const char* GetPassword() const { return m_password; }
	int GetMaxConnections() const { return m_maxConnections; }
	int GetLevel() const { return m_level; }
	int GetNormLevel() const { return m_normLevel; }
	void SetNormLevel(int level) { m_normLevel = level; }
	bool GetJoinGroup() const { return m_joinGroup; }
	bool GetTls() const { return m_tls; }
	const char* GetCipher() const { return m_cipher; }
	int GetRetention() const { return m_retention; }
	bool GetOptional() const { return m_optional; }
	time_t GetBlockTime() const { return m_blockTime; }
	void SetBlockTime(time_t blockTime) { m_blockTime = blockTime; }
	unsigned int GetCertVerificationLevel() const { return m_certVerificationfLevel; }

private:
	int m_id;
	int m_stateId = 0;
	bool m_active;
	CString m_name;
	CString m_host;
	int m_port;
	int m_ipVersion;
	CString m_user;
	CString m_password;
	bool m_joinGroup;
	bool m_tls;
	CString m_cipher;
	int m_maxConnections;
	int m_retention;
	int m_level;
	int m_normLevel;
	int m_group;
	bool m_optional = false;
	time_t m_blockTime = 0;
	unsigned int m_certVerificationfLevel;
};

typedef std::vector<std::unique_ptr<NewsServer>> Servers;

#endif

#ifndef SCRIPTCONFIG_H
#define SCRIPTCONFIG_H

#include <functional>

class ScriptConfig
{
public:

	class ConfigTemplate
	{
	public:
		ConfigTemplate(const char* templ)
			: m_template(templ) {}
		const char* GetTemplate() { return m_template; }

	private:
		CString m_template;
	};

	using ConfigTemplates = std::deque<ConfigTemplate>;

	void InitOptions()
	{
		InitConfigTemplates();
	}

	bool LoadConfig(Options::OptEntries* optEntries)
	{
		if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
		{
			return false;
		}
		return nzbget_rs_script_config_load(
			g_Options->GetConfigFilename(),
			optEntries,
			[](void* ctx, const char* name, const char* value)
			{
				try
				{
					static_cast<Options::OptEntries*>(ctx)->emplace_back(name, value);
				}
				catch (...)
				{
				}
			}) != 0;
	}

	bool SaveConfig(Options::OptEntries* optEntries, const char* cipherBaseline = nullptr, bool* conflict = nullptr)
	{
		if (conflict) *conflict = false;
		if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
		{
			return false;
		}
		std::vector<const char*> names;
		std::vector<const char*> values;
		names.reserve(optEntries->size());
		values.reserve(optEntries->size());
		for (const auto& entry : *optEntries)
		{
			if (entry.GetName() && entry.GetValue())
			{
				names.push_back(entry.GetName());
				values.push_back(entry.GetValue());
			}
		}
		int conflictFlag = 0;
		int saved = nzbget_rs_script_config_save(
			g_Options->GetConfigFilename(),
			names.data(),
			values.data(),
			names.size(),
			cipherBaseline,
			&conflictFlag);
		if (conflict)
		{
			*conflict = conflictFlag != 0;
		}
		return saved != 0;
	}

	static std::string CipherBaseline(const Options::OptEntries& optEntries)
	{
		std::vector<const char*> names;
		std::vector<const char*> values;
		names.reserve(optEntries.size());
		values.reserve(optEntries.size());
		for (const auto& entry : optEntries)
		{
			if (!entry.GetName() || !entry.GetValue()) continue;
			names.push_back(entry.GetName());
			values.push_back(entry.GetValue());
		}
		NzbgetRsBuf buf = nzbget_rs_sus_build_cipher_baseline(names.data(), values.data(), names.size());
		std::string token(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return token;
	}

	enum class CipherCommit { Saved, Unchanged, Conflict, Cancelled, Failed };

	bool ReadCipherSnapshot(std::string& bytes)
	{
		if (!g_Options || !g_Options->GetConfigFilename())
		{
			return false;
		}
		NzbgetRsBuf buf{};
		if (!nzbget_rs_script_config_read_cipher_snapshot(g_Options->GetConfigFilename(), &buf))
		{
			return false;
		}
		bytes.assign(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return true;
	}

	CipherCommit CommitCipherPatch(std::string_view expected, std::string_view replacement, const std::function<bool()>& cancel)
	{
		if (!g_Options || !g_Options->GetConfigFilename())
		{
			return CipherCommit::Failed;
		}
		int res = nzbget_rs_script_config_commit_cipher_patch(
			g_Options->GetConfigFilename(),
			expected.data(),
			expected.size(),
			replacement.data(),
			replacement.size(),
			const_cast<std::function<bool()>*>(&cancel),
			[](void* ctx) -> int
			{
				try
				{
					const auto* fn = static_cast<const std::function<bool()>*>(ctx);
					return (*fn)() ? 1 : 0;
				}
				catch (...)
				{
					return 1;
				}
			},
			[](void*)
			{
				try
				{
					warn("TLS cipher settings were replaced, but the configuration directory could not be synchronized");
				}
				catch (...)
				{
				}
			});
		switch (res)
		{
			case 0: return CipherCommit::Saved;
			case 1: return CipherCommit::Unchanged;
			case 2: return CipherCommit::Conflict;
			case 3: return CipherCommit::Cancelled;
			default: return CipherCommit::Failed;
		}
	}

	bool LoadConfigTemplates(ConfigTemplates* configTemplates)
	{
		CharBuffer buffer;
		if (!FileSystem::LoadFileIntoBuffer(g_Options->GetConfigTemplate(), buffer, true))
		{
			return false;
		}
		configTemplates->emplace_back(ConfigTemplate(buffer));

		return true;
	}

	ConfigTemplates* GetConfigTemplates() { return &m_configTemplates; }

private:
	ConfigTemplates m_configTemplates;

	void InitConfigTemplates()
	{
		if (!LoadConfigTemplates(&m_configTemplates))
		{
			error("Could not read configuration templates");
		}
	}
};

extern ScriptConfig* g_ScriptConfig;

#endif

#endif
#endif
