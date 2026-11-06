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

#include <fstream>
#include "NzbFetcher.h"
#include "DupeUtil.h"
#include "HttpGet.h"
#include "DonorHealth.h"
#include "FileSystem.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

NzbFetcher::Budget::Budget(int maxDonors) : m_left(maxDonors > 0 ? (unsigned long long)maxDonors * 3 : 0), m_unlimited(maxDonors <= 0)
{
    m_rust = nzbget_rs_dupe_budget_new(maxDonors > 0 ? (uint32_t)maxDonors : 0);
}
NzbFetcher::Budget::~Budget()
{
    nzbget_rs_dupe_budget_free(m_rust);
}
bool NzbFetcher::Budget::Take()
{
    return nzbget_rs_dupe_budget_take(m_rust) != 0;
}
bool NzbFetcher::Budget::Spent() const
{
    return nzbget_rs_dupe_budget_spent(m_rust) != 0;
}

void NzbFetcher::Stats::Add(const Stats& other)
{
	ok += other.ok;
	fetch += other.fetch;
	parse += other.parse;
	refused += other.refused;
	relisted += other.relisted;
	listingMismatch += other.listingMismatch;
	budget += other.budget;
}

void NzbFetcher::SetStatePath(const std::string& path)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	m_statePath = path;
}

void NzbFetcher::Load()
{
	std::lock_guard<std::mutex> guard(m_mutex);
	std::ifstream file(fs::u8path(m_statePath));
	std::string line;
	time_t now = Util::CurrentTime();
	while (std::getline(file, line))
	{
		NzbgetRsBuf idxBuf{};
		int64_t when = 0;
		if (nzbget_rs_dupesearch_parse_cooldown_line(
			line.data(), line.size(), (int64_t)now, &idxBuf, &when) != 0)
		{
			m_refusedUntil[std::string(idxBuf.data ? idxBuf.data : "", idxBuf.len)] = (time_t)when;
		}
		nzbget_rs_free(idxBuf);
	}
}

// called with m_mutex held; written to a temporary file and renamed, so a
// crash can't lose a cooldown (or leave half of the file)
void NzbFetcher::Save()
{
	if (m_statePath.empty())
	{
		return;
	}
	std::string text;
	time_t now = Util::CurrentTime();
	for (const auto& entry : m_refusedUntil)
	{
		NzbgetRsBuf lineBuf = nzbget_rs_dupesearch_format_cooldown_line(
			entry.first.data(), entry.first.size(), (int64_t)entry.second, (int64_t)now);
		if (lineBuf.data && lineBuf.len > 0)
		{
			text.append(lineBuf.data, lineBuf.len);
		}
		nzbget_rs_free(lineBuf);
	}
	if (!DupeUtil::WriteAtomic(m_statePath, text))
	{
		warn("Could not save the DupeSearch indexer state to %s", m_statePath.c_str());
	}
}

std::mutex& NzbFetcher::IndexerLock(const std::string& indexer)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	std::unique_ptr<std::mutex>& lock = m_indexerLocks[indexer];
	if (!lock)
	{
		lock = std::make_unique<std::mutex>();
	}
	return *lock;
}

bool NzbFetcher::IsRefused(const std::string& indexer)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	auto it = m_refusedUntil.find(indexer);
	return it != m_refusedUntil.end() && it->second > Util::CurrentTime();
}

void NzbFetcher::Refuse(const std::string& indexer)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	m_refusedUntil[indexer] = Util::CurrentTime() + CooldownSec;
	Save();
}

NzbFetcher::Fetched NzbFetcher::Fetch(const Newznab::Result& listing, time_t deadline, int retries, Budget* budget)
{
	Fetched fetched;
	fetched.listing = listing;
    if (budget && budget->Spent()) { fetched.reason = frBudget; return fetched; }
    if (HttpGet::Stopped() || Util::CurrentTime() >= deadline) return fetched;

	// one download per indexer at a time: its refusal is known before the next grab
	std::lock_guard<std::mutex> indexerGuard(IndexerLock(listing.indexer));
	if (IsRefused(listing.indexer))
	{
		fetched.reason = frRefused;
		return fetched;
	}

	int status = 0;
	for (int attempt = 0; attempt <= retries; attempt++)
	{
        if (HttpGet::Stopped() || Util::CurrentTime() >= deadline) break;
        if (budget && !budget->Take()) { fetched.reason = frBudget; return fetched; }
		std::string why;
		// the deadline ends a download that drips, not only the retries
		long long fetchDeadlineMs = std::max(1LL, DonorHealth::NowMs() + (long long)(deadline - Util::CurrentTime()) * 1000LL);
		HttpGet::Reply reply = m_fetch ? m_fetch(listing.link, "DupeSearch nzb of " + listing.indexer, MaxNzbBytes, fetchDeadlineMs) :
			HttpGet::Fetch(listing.link, "DupeSearch nzb of " + listing.indexer, MaxNzbBytes, fetchDeadlineMs);
		if (reply.ok)
		{
			status = 0;
			NzbSummary info;
			if (NzbReader::Parse(reply.body, info))
			{
				fetched.reason = frOk;
				fetched.data = std::move(reply.body);
				fetched.info = std::move(info);
				return fetched;
			}
			// an error answer served with HTTP 200 (code 300, code 429) lands
			// here too: retried, but no sign of a grab limit by itself
			fetched.reason = frParse;
			NzbgetRsBuf whyBuf = nzbget_rs_dqh_nzb_fetcher_format_parse_err(
				reply.body.data(), reply.body.size());
			why.assign(whyBuf.data ? whyBuf.data : "", whyBuf.len);
			nzbget_rs_free(whyBuf);
		}
		else
		{
			status = reply.status;
			fetched.reason = frFetch;
			NzbgetRsBuf whyBuf = nzbget_rs_dqh_nzb_fetcher_format_http_status_err(reply.status);
			why.assign(whyBuf.data ? whyBuf.data : "", whyBuf.len);
			nzbget_rs_free(whyBuf);
		}
		info("DupeSearch: the nzb-file of %s from %s failed (attempt %i): %s",
			listing.title.c_str(), listing.indexer.c_str(), attempt + 1, why.c_str());

        if (budget && budget->Spent()) break;
		if (attempt < retries)
		{
			// no time for the pause: no more attempts (they went out at once,
			// each one an indexer grab, and a 429 was asked again unpaused)
			if (Util::CurrentTime() + RetryDelaySec >= deadline)
			{
				break;
			}
			for (int waited = 0; waited < RetryDelaySec * 10 && !HttpGet::Stopped(); waited++)
			{
				Util::Sleep(100);
			}
		}
		if (HttpGet::Stopped())
		{
			break;
		}
	}

	if ((status == 403 || status == 429) && !listing.indexer.empty())
	{
		Refuse(listing.indexer);
		info("DupeSearch: indexer %s refused nzb-file downloads (HTTP %i): not asking it for %i minutes",
			listing.indexer.c_str(), status, CooldownSec / 60);
	}
	return fetched;
}

NzbFetcher::Fetched NzbFetcher::FetchPosting(const Posting::Group& listings, time_t deadline, Stats& stats, Budget* budget)
{
	Fetched last;
    if (listings.empty()) { stats.parse++; return last; }
	last.listing = listings.front();
	bool haveOther = false;
	Fetched other;

	for (size_t i = 0; i < listings.size(); i++)
	{
		Fetched fetched = Fetch(listings[i], deadline, 1, budget);
        if (fetched.reason == frBudget) { stats.budget++; last = std::move(fetched); break; }
		if (fetched.reason == frOk && !Posting::ListingMismatch(listings[i], fetched.info.totalBytes))
		{
			stats.ok++;
			// the other listings of the posting were not fetched: the same posting
			stats.relisted += (int)(listings.size() - i - 1);
			return fetched;
		}
		if (fetched.reason == frOk)
		{
			if (!haveOther)
			{
				other = std::move(fetched);
				haveOther = true;
			}
			continue;
		}
		switch (fetched.reason)
		{
			case frFetch: stats.fetch++; break;
			case frParse: stats.parse++; break;
			case frRefused: stats.refused++; break;
			default: break;
		}
		last = std::move(fetched);
	}

	if (haveOther)
	{
		stats.ok++;
		stats.listingMismatch++;
		return other;
	}
	return last;
}
