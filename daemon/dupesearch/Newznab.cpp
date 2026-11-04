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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include <algorithm>
#include <atomic>
#include <map>
#include <regex>
#include <set>
#include <thread>
#include "Newznab.h"
#include "HttpGet.h"
#include "DonorHealth.h"
#include "ReleaseName.h"
#include "XmlReader.h"
#include "Log.h"
#include "Util.h"

#include "DupeUtil.h"
#include "nzbget_rs.h"

using DupeUtil::Lower;
using DupeUtil::Trim;

namespace
{

long long ToInt(const std::string& text)
{
	return atoll(Trim(text).c_str());
}

class PageReader : public XmlReader::Handler
{
public:
	explicit PageReader(Newznab::Page& page) : m_page(page) {}

	void Start(const std::string& name, const XmlReader::Attrs& attrs) override
	{
		m_depth++;
		m_text.clear();
		if (m_depth == 1 && name == "error")
		{
			m_page.error = true;
			m_page.errorCode = atoi(XmlReader::Attr(attrs, "code").c_str());
			m_page.errorText = XmlReader::Attr(attrs, "description");
		}
		else if (name == "item")
		{
			m_inItem = true;
			m_item = Newznab::Result();
			m_attrSize = 0;
			m_elementSize = 0;
		}
		else if (m_inItem && name == "attr")
		{
			std::string key = XmlReader::Attr(attrs, "name");
			std::string value = XmlReader::Attr(attrs, "value");
			if (key == "size")
			{
				m_attrSize = ToInt(value);
			}
			else if (key == "grabs")
			{
				m_item.grabs = (int)ToInt(value);
			}
			else if (key == "usenetdate")
			{
				m_item.date = Newznab::ParseDate(value);
			}
			else if (key == "hydraIndexerName")
			{
				m_item.indexer = value;
			}
		}
	}

	void End(const std::string& name) override
	{
		if (m_inItem)
		{
			if (name == "title")
			{
				m_item.title = Trim(m_text);
			}
			else if (name == "link")
			{
				m_item.link = Trim(m_text);
			}
			else if (name == "size")
			{
				m_elementSize = ToInt(m_text);
			}
			else if (name == "item")
			{
				m_item.size = nzbget_rs_dsf_newznab_resolve_item_size(
					m_attrSize, m_elementSize);
				m_page.items.push_back(m_item);
				m_inItem = false;
			}
		}
		m_depth--;
		m_text.clear();
	}

	void Text(const std::string& text) override
	{
		m_text += text;
	}

private:
	Newznab::Page& m_page;
	Newznab::Result m_item;
	std::string m_text;
	int m_depth = 0;
	bool m_inItem = false;
	long long m_attrSize = 0;
	long long m_elementSize = 0;
};

std::string UrlEncode(const std::string& text)
{
	NzbgetRsBuf buf = nzbget_rs_df_url_encode(text.data(), text.size());
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

std::string DigitsOnly(const std::string& text)
{
	NzbgetRsBuf buf = nzbget_rs_df_digits_only(text.data(), text.size());
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

// days since 1970-01-01 of a civil date
long long DaysFromCivil(long long year, int month, int day)
{
	return nzbget_rs_df_days_from_civil(year, month, day);
}

}

bool Newznab::ParseResponse(const std::string& xml, Page& page)
{
	page = Page();
	PageReader reader(page);
	return XmlReader::Parse(xml, reader);
}

time_t Newznab::ParseDate(const std::string& text)
{
	return (time_t)nzbget_rs_df_parse_newznab_date(text.data(), text.size());
}

std::vector<Newznab::Params> Newznab::BuildQueries(const std::string& title, const std::string& imdb,
	const std::string& tvdb)
{
	std::string normalized = ReleaseName::Normalize(title);
	std::string full = normalized;
	std::replace(full.begin(), full.end(), '.', ' ');
	std::string shortQuery = ReleaseName::ShortQuery(title);
	std::string group = ReleaseName::Parse(title).group;

	std::vector<Params> queries;
	queries.push_back({ { "t", "search" }, { "q", full } });
	if (!shortQuery.empty() && shortQuery != full)
	{
		queries.push_back({ { "t", "search" }, { "q", shortQuery } });
	}
	if (!shortQuery.empty() && !group.empty())
	{
		queries.push_back({ { "t", "search" }, { "q", shortQuery + " " + group } });
	}

	std::string imdbId = DigitsOnly(imdb);
	if (!imdbId.empty())
	{
		queries.push_back({ { "t", "movie" }, { "imdbid", imdbId } });
	}

	std::string tvdbId = DigitsOnly(tvdb);
	std::smatch match;
	static const std::regex episode("(?:^|\\.)s(\\d+)e(\\d+)(?:\\.|$)");
	if (!tvdbId.empty() && std::regex_search(normalized, match, episode))
	{
		queries.push_back({ { "t", "tvsearch" }, { "tvdbid", tvdbId },
			{ "season", std::to_string(atoi(match[1].str().c_str())) },
			{ "ep", std::to_string(atoi(match[2].str().c_str())) } });
	}
	return queries;
}

std::string Newznab::BuildUrl(const std::string& base, const Params& params, const std::string& apiKey)
{
	std::vector<const char*> keys(params.size());
	std::vector<const char*> vals(params.size());
	for (size_t i = 0; i < params.size(); i++)
	{
		keys[i] = params[i].first.c_str();
		vals[i] = params[i].second.c_str();
	}
	NzbgetRsBuf buf = nzbget_rs_df_build_newznab_url(
		base.c_str(), keys.data(), vals.data(), params.size(), apiKey.c_str());
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

std::string Newznab::Mask(const std::string& text)
{
	NzbgetRsBuf buf = nzbget_rs_df_mask_newznab_text(text.data(), text.size());
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

std::vector<Newznab::Result> Newznab::Search(const std::string& base, const std::string& apiKey,
	const std::vector<Params>& queries, time_t deadline, SearchStats* stats)
{
	std::vector<std::vector<Result>> found(queries.size());
	std::atomic<size_t> next{0};
	std::atomic<int> failed{0};
	std::atomic<int> pages{0};

	auto worker = [&]()
	{
		for (size_t index = next++; index < queries.size(); index = next++)
		{
			std::string label;
			for (const auto& param : queries[index])
			{
				label += (label.empty() ? "" : " ") + param.first + "=" + param.second;
			}

			for (int page = 0; page < MaxPages; page++)
			{
				if (Util::CurrentTime() >= deadline)
				{
					detail("DupeSearch: no time left for the search %s", label.c_str());
					break;
				}

				Params params = queries[index];
				params.emplace_back("limit", std::to_string(PageSize));
				params.emplace_back("offset", std::to_string(page * PageSize));
				std::string infoName = "DupeSearch " + label + " page " + std::to_string(page + 1);
				long long pageDeadlineMs = nzbget_rs_dsf_newznab_calc_page_deadline_ms(
					DonorHealth::NowMs(), (int64_t)deadline, (int64_t)Util::CurrentTime());
				HttpGet::Reply reply = HttpGet::Fetch(BuildUrl(base, params, apiKey), infoName, 16 * 1024 * 1024,
					pageDeadlineMs);
				if (nzbget_rs_dsf_newznab_should_retry_dropped_fetch(
					reply.ok ? 1 : 0, reply.status, HttpGet::Stopped() ? 1 : 0,
					(int64_t)Util::CurrentTime(), (int64_t)deadline) != 0)
				{
					reply = HttpGet::Fetch(BuildUrl(base, params, apiKey), infoName, 16 * 1024 * 1024,
						pageDeadlineMs);
				}
				pages++;

				Page result;
				if (!reply.ok)
				{
					warn("DupeSearch: the search %s failed (HTTP %i)", label.c_str(), reply.status);
					failed++;
					break;
				}
				if (!ParseResponse(reply.body, result))
				{
					warn("DupeSearch: the answer to the search %s isn't XML (is option DupeSearchUrl the "
						"indexer's api address, like http://host:5076/api?)", label.c_str());
					failed++;
					break;
				}
				if (result.error)
				{
					warn("DupeSearch: the indexer answered the search %s with error %i: %s",
						label.c_str(), result.errorCode, Mask(result.errorText).c_str());
					failed++;
					break;
				}

				found[index].insert(found[index].end(), result.items.begin(), result.items.end());
				if (nzbget_rs_dsf_newznab_should_stop_pagination(
					result.items.size(), PageSize) != 0)
				{
					break;
				}
			}
		}
	};

	std::vector<std::thread> threads;
	size_t count = std::min(queries.size(), (size_t)Parallel);
	for (size_t i = 0; i < count; i++)
	{
		threads.emplace_back(worker);
	}
	for (std::thread& thread : threads)
	{
		thread.join();
	}

	std::vector<Result> merged;
	std::set<std::string> links;
	for (const std::vector<Result>& results : found)
	{
		for (const Result& result : results)
		{
			if (!result.link.empty() && links.insert(result.link).second)
			{
				merged.push_back(result);
			}
		}
	}

	if (stats)
	{
		stats->queries = (int)queries.size();
		stats->failed = failed;
		stats->pages = pages;
	}
	return merged;
}
