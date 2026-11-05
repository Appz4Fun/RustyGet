/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"
#include "FeedInfo.h"
#include "Util.h"
#include "nzbget_rs.h"

FeedInfo::FeedInfo(
	int id,
	const char* name,
	const char* url,
	bool backlog,
	int interval,
	const char* filter,
	bool pauseNzb,
	const char* category,
	FeedInfo::CategorySource categorySource,
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
	, m_backlog{ backlog}
	, m_interval{ interval }
	, m_pauseNzb{ pauseNzb }
	, m_priority{ priority }
	, m_certVerifLevel{ certVerifLevel }
{
	if (m_name.empty())
	{
		NzbgetRsBuf nameBuf = nzbget_rs_tpr_feed_info_default_name(m_id);
		m_name.assign(nameBuf.data ? nameBuf.data : "", nameBuf.len);
		nzbget_rs_free(nameBuf);
	}
	m_filterHash = Util::HashBJ96(m_filter.c_str(), m_filter.size(), 0);
}

FeedItemInfo::Attr* FeedItemInfo::Attributes::Find(const char* name)
{
	for (Attr& attr : this)
	{
		if (nzbget_rs_sfc_feed_info_attr_name_eq_ignore_case(attr.GetName(), name) != 0)
		{
			return &attr;
		}
	}

	return nullptr;
}


void FeedItemInfo::SetSeason(const char* season)
{
	m_season = season ? season : "";
	m_seasonNum = season ? ParsePrefixedInt(season) : 0;
}

void FeedItemInfo::SetEpisode(const char* episode)
{
	m_episode = episode ? episode : "";
	m_episodeNum = episode ? ParsePrefixedInt(episode) : 0;
}

int FeedItemInfo::ParsePrefixedInt(const char *value)
{
	return nzbget_rs_df_parse_prefixed_int(value);
}

void FeedItemInfo::AppendDupeKey(const char* extraDupeKey)
{
	if (nzbget_rs_sfc_feed_info_should_append_dupe_key(m_dupeKey.empty() ? 1 : 0, extraDupeKey) != 0)
	{
		m_dupeKey += "-";
		m_dupeKey += extraDupeKey;
	}
}

void FeedItemInfo::BuildDupeKey(const char* rageId, const char* tvdbId, const char* tvmazeId, const char* series)
{
	bool anySeriesId = nzbget_rs_tpr_feed_item_has_any_series_id(
		series, rageId, tvdbId, tvmazeId, m_rageId, m_tvdbId, m_tvmazeId) != 0;
	int seasonNum = anySeriesId ? GetSeasonNum() : 0;
	int episodeNum = (anySeriesId && seasonNum != 0) ? GetEpisodeNum() : 0;
	NzbgetRsBuf buf = nzbget_rs_df_build_feed_dupe_key(
		rageId, tvdbId, tvmazeId, series,
		m_rageId, m_tvdbId, m_tvmazeId, m_imdbId,
		seasonNum, episodeNum, m_season.c_str(), m_episode.c_str());
	m_dupeKey.assign(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	m_dupeKey.shrink_to_fit();
}

int FeedItemInfo::GetSeasonNum()
{
	if (nzbget_rs_sfc_feed_info_should_parse_season_episode(m_season.empty() ? 1 : 0, m_seasonEpisodeParsed ? 1 : 0) != 0)
	{
		ParseSeasonEpisode();
	}

	return m_seasonNum;
}

int FeedItemInfo::GetEpisodeNum()
{
	if (nzbget_rs_sfc_feed_info_should_parse_season_episode(m_episode.empty() ? 1 : 0, m_seasonEpisodeParsed ? 1 : 0) != 0)
	{
		ParseSeasonEpisode();
	}

	return m_episodeNum;
}

void FeedItemInfo::ParseSeasonEpisode()
{
	if (!m_feedFilterHelper)
	{
		return;
	}

	m_seasonEpisodeParsed = true;

	const char* pattern = "[^[:alnum:]]s?([0-9]+)[ex]([0-9]+(-?e[0-9]+)?)[^[:alnum:]]";

	std::unique_ptr<RegEx>& regEx = m_feedFilterHelper->GetRegEx(1);
	if (!regEx)
	{
		regEx = std::make_unique<RegEx>(pattern, 10);
	}

	if (regEx->Match(m_title.c_str()))
	{
		const char* season = m_title.c_str() + regEx->GetMatchStart(1);
		int seasonNum = Util::StrToNum<int>(season).value_or(0);
		const char* val = m_title.c_str() + regEx->GetMatchStart(2);
		size_t valLen = (size_t)regEx->GetMatchLen(2);
		NzbgetRsBuf seasonBuf{};
		NzbgetRsBuf episodeBuf{};
		nzbget_rs_sfc_feed_info_format_season_episode(seasonNum, val, valLen, &seasonBuf, &episodeBuf);
		SetSeason(seasonBuf.data ? seasonBuf.data : "");
		SetEpisode(episodeBuf.data ? episodeBuf.data : "");
		nzbget_rs_free(seasonBuf);
		nzbget_rs_free(episodeBuf);
	}
}

const char* FeedItemInfo::GetDupeStatus()
{
	if (m_dupeStatus.empty())
	{
		BString<1024> statuses;
		m_feedFilterHelper->CalcDupeStatus(m_title.c_str(), m_dupeKey.c_str(), statuses, statuses.Capacity());
		m_dupeStatus = statuses;
	}

	return m_dupeStatus.c_str();
}


void FeedHistory::Remove(const char* url)
{
	for (iterator it = begin(); it != end(); ++it)
	{
		FeedHistoryInfo& feedHistoryInfo = *it;
		if (!strcmp(feedHistoryInfo.GetUrl(), url))
		{
			erase(it);
			break;
		}
	}
}

FeedHistoryInfo* FeedHistory::Find(const char* url)
{
	for (FeedHistoryInfo& feedHistoryInfo : this)
	{
		if (!strcmp(feedHistoryInfo.GetUrl(), url))
		{
			return &feedHistoryInfo;
		}
	}

	return nullptr;
}
