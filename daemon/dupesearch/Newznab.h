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


#ifndef NEWZNAB_H
#define NEWZNAB_H

#include <algorithm>
#include <atomic>
#include <ctime>
#include <map>
#include <set>
#include <string>
#include <thread>
#include <utility>
#include <vector>
#include "HttpGet.h"
#include "DonorHealth.h"
#include "ReleaseName.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

/*
 * The Newznab API of an indexer (NZBHydra2 speaks it): searching for another
 * posting of a release.
 */
class Newznab
{
public:
	struct Result
	{
		std::string title;
		std::string link;
		long long size = 0;
		int grabs = 0;
		time_t date = 0;
		std::string indexer;
	};

	struct Page
	{
		std::vector<Result> items;
		bool error = false;
		int errorCode = 0;
		std::string errorText;
	};

	using Params = std::vector<std::pair<std::string, std::string>>;

	static constexpr int PageSize = 100;
	static constexpr int MaxPages = 5;
	static constexpr int Parallel = 4;

	static bool ParseResponse(const std::string& xml, Page& page)
	{
		page = Page();
		return nzbget_rs_newznab_parse_response(
			reinterpret_cast<const uint8_t*>(xml.data()),
			xml.size(),
			&page,
			[](void* ctx, int errorCode, const uint8_t* errorTextPtr, size_t errorTextLen) noexcept
			{
				auto* out = static_cast<Page*>(ctx);
				out->error = true;
				out->errorCode = errorCode;
				out->errorText.assign(reinterpret_cast<const char*>(errorTextPtr), errorTextLen);
			},
			[](void* ctx, const uint8_t* titlePtr, size_t titleLen,
			   const uint8_t* linkPtr, size_t linkLen, int64_t size, int32_t grabs,
			   int64_t date, const uint8_t* indexerPtr, size_t indexerLen) noexcept
			{
				auto* out = static_cast<Page*>(ctx);
				Result r;
				r.title.assign(reinterpret_cast<const char*>(titlePtr), titleLen);
				r.link.assign(reinterpret_cast<const char*>(linkPtr), linkLen);
				r.size = (long long)size;
				r.grabs = grabs;
				r.date = (time_t)date;
				r.indexer.assign(reinterpret_cast<const char*>(indexerPtr), indexerLen);
				out->items.push_back(std::move(r));
			}) != 0;
	}

	static time_t ParseDate(const std::string& text)
	{
		return (time_t)nzbget_rs_df_parse_newznab_date(text.data(), text.size());
	}

	static std::vector<Params> BuildQueries(const std::string& title, const std::string& imdb,
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

	static std::string BuildUrl(const std::string& base, const Params& params, const std::string& apiKey)
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

	static std::string Mask(const std::string& text)
	{
		NzbgetRsBuf buf = nzbget_rs_df_mask_newznab_text(text.data(), text.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	struct SearchStats
	{
		int queries = 0;
		int failed = 0;
		int pages = 0;
	};

	static std::vector<Result> Search(const std::string& base, const std::string& apiKey,
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
				std::vector<const char*> lKeys;
				std::vector<const char*> lVals;
				lKeys.reserve(queries[index].size());
				lVals.reserve(queries[index].size());
				for (const auto& param : queries[index])
				{
					lKeys.push_back(param.first.c_str());
					lVals.push_back(param.second.c_str());
				}
				NzbgetRsBuf lblBuf = nzbget_rs_dqh_newznab_format_query_label(
					lKeys.data(), lVals.data(), lKeys.size());
				std::string label(lblBuf.data ? lblBuf.data : "", lblBuf.len);
				nzbget_rs_free(lblBuf);

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
					long long pageDeadlineMs = std::max(1LL, DonorHealth::NowMs() + (long long)(deadline - Util::CurrentTime()) * 1000LL);
					HttpGet::Reply reply = HttpGet::Fetch(BuildUrl(base, params, apiKey), infoName, 16 * 1024 * 1024,
						pageDeadlineMs);
					if (!reply.ok && reply.status == 0 && !HttpGet::Stopped() && Util::CurrentTime() < deadline)
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
					if ((int)result.items.size() < PageSize)
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
};

#endif
