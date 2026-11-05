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
		switch (nzbget_rs_dqh_newznab_classify_start_element(
			m_depth, m_inItem ? 1 : 0, name.c_str()))
		{
			case 1:
				m_page.error = true;
				m_page.errorCode = atoi(XmlReader::Attr(attrs, "code").c_str());
				m_page.errorText = XmlReader::Attr(attrs, "description");
				break;
			case 2:
				m_inItem = true;
				m_item = Newznab::Result();
				m_attrSize = 0;
				m_elementSize = 0;
				break;
			case 3:
			{
				std::string key = XmlReader::Attr(attrs, "name");
				std::string value = XmlReader::Attr(attrs, "value");
				switch (nzbget_rs_dqh_newznab_classify_item_attr_key(key.c_str()))
				{
					case 1:
						m_attrSize = ToInt(value);
						break;
					case 2:
						m_item.grabs = (int)ToInt(value);
						break;
					case 3:
						m_item.date = Newznab::ParseDate(value);
						break;
					case 4:
						m_item.indexer = value;
						break;
				}
				break;
			}
		}
	}

	void End(const std::string& name) override
	{
		switch (nzbget_rs_dqh_newznab_classify_end_element(m_inItem ? 1 : 0, name.c_str()))
		{
			case 1:
				m_item.title = Trim(m_text);
				break;
			case 2:
				m_item.link = Trim(m_text);
				break;
			case 3:
				m_elementSize = ToInt(m_text);
				break;
			case 4:
				m_item.size = nzbget_rs_dsf_newznab_resolve_item_size(
					m_attrSize, m_elementSize);
				m_page.items.push_back(m_item);
				m_inItem = false;
				break;
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
	std::vector<Params> queries;
	nzbget_rs_df_build_newznab_queries(
		title.data(), title.size(),
		imdb.data(), imdb.size(),
		tvdb.data(), tvdb.size(),
		&queries,
		[](void* ctx, const char* const* keys, const size_t* keyLens,
		   const char* const* vals, const size_t* valLens, size_t count) noexcept
		{
			auto* out = static_cast<std::vector<Params>*>(ctx);
			Params p;
			p.reserve(count);
			for (size_t i = 0; i < count; i++)
			{
				p.emplace_back(std::string(keys[i], keyLens[i]), std::string(vals[i], valLens[i]));
			}
			out->push_back(std::move(p));
		});
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
				NzbgetRsBuf limBuf = nzbget_rs_dqh_conn_format_port_str(PageSize);
				params.emplace_back("limit", std::string(limBuf.data ? limBuf.data : "", limBuf.len));
				nzbget_rs_free(limBuf);
				NzbgetRsBuf offBuf = nzbget_rs_dqh_conn_format_port_str(page * PageSize);
				params.emplace_back("offset", std::string(offBuf.data ? offBuf.data : "", offBuf.len));
				nzbget_rs_free(offBuf);
				NzbgetRsBuf infoBuf = nzbget_rs_dqh_newznab_format_search_info_name(label.c_str(), page + 1);
				std::string infoName(infoBuf.data ? infoBuf.data : "", infoBuf.len);
				nzbget_rs_free(infoBuf);
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
