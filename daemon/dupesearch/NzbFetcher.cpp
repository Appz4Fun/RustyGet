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

#include "NzbFetcher.h"
#include "HttpGet.h"
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

NzbFetcher::NzbFetcher() : m_rust(nzbget_rs_nzb_fetcher_new()) {}
NzbFetcher::NzbFetcher(FetchFunc fetch) : m_rust(nzbget_rs_nzb_fetcher_new()), m_fetch(std::move(fetch)) {}
NzbFetcher::~NzbFetcher()
{
	nzbget_rs_nzb_fetcher_free(m_rust);
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
	nzbget_rs_nzb_fetcher_set_state_path(m_rust, path.data(), path.size());
}

void NzbFetcher::Load()
{
	nzbget_rs_nzb_fetcher_load(m_rust, (int64_t)Util::CurrentTime());
}

bool NzbFetcher::IsRefused(const std::string& indexer)
{
	return nzbget_rs_nzb_fetcher_is_refused(
		m_rust, indexer.data(), indexer.size(), (int64_t)Util::CurrentTime()) != 0;
}

NzbFetcher::Fetched NzbFetcher::Fetch(const Newznab::Result& listing, time_t deadline, int retries, Budget* budget)
{
	Fetched fetched;
	fetched.listing = listing;
	struct FetchCtx
	{
		NzbFetcher* self;
		HttpGet::Reply lastReply;
	} ctx{this, {}};
	NzbgetRsBuf dataBuf{};
	RsNzbSummaryScalars scalars{};
	int reason = nzbget_rs_nzb_fetcher_fetch(
		m_rust,
		listing.indexer.data(), listing.indexer.size(),
		listing.title.data(), listing.title.size(),
		listing.link.data(), listing.link.size(),
		listing.size, listing.grabs, (int64_t)listing.date,
		(int64_t)deadline, retries,
		budget ? budget->RustHandle() : nullptr,
		&ctx,
		[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
		[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
		[](void*) { Util::Sleep(100); },
		[](void* rawCtx, const char* url, size_t urlLen, const char* label, size_t labelLen,
			size_t maxBytes, int64_t deadlineMs, int* outStatus,
			const char** outBodyPtr, size_t* outBodyLen) -> int {
			auto* c = static_cast<FetchCtx*>(rawCtx);
			std::string urlStr(url ? url : "", urlLen);
			std::string labelStr(label ? label : "", labelLen);
			c->lastReply = c->self->m_fetch
				? c->self->m_fetch(urlStr, labelStr, maxBytes, deadlineMs)
				: HttpGet::Fetch(urlStr, labelStr, maxBytes, deadlineMs);
			*outStatus = c->lastReply.status;
			*outBodyPtr = c->lastReply.body.empty() ? nullptr : c->lastReply.body.data();
			*outBodyLen = c->lastReply.body.size();
			return c->lastReply.ok ? 1 : 0;
		},
		[](void*, const char* msg) {
			info("%s", msg ? msg : "");
		},
		[](void*, const char* msg) {
			warn("%s", msg ? msg : "");
		},
		&dataBuf,
		&scalars,
		&fetched.info,
		NzbReader::OnStrings,
		NzbReader::OnSlice,
		NzbReader::OnRecoveryBytes);
	fetched.reason = static_cast<EReason>(reason);
	if (fetched.reason == frOk && dataBuf.data)
	{
		fetched.data.assign(dataBuf.data, dataBuf.len);
		NzbReader::ApplyScalars(fetched.info, scalars);
	}
	nzbget_rs_free(dataBuf);
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
