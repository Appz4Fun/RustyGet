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

#include <boost/test/unit_test.hpp>
#include "NzbReader.h"
#include "DupeIdentity.h"
#include "Util.h"
#include "NzbFetcher.h"
#include <stdexcept>

namespace
{

Newznab::Result Listing(const char* link, long long size, int grabs, time_t date, const char* indexer = "idx")
{
	Newznab::Result result;
	result.title = "t";
	result.link = link;
	result.size = size;
	result.grabs = grabs;
	result.date = date;
	result.indexer = indexer;
	return result;
}

}

BOOST_AUTO_TEST_SUITE(DupeSearchTest)

BOOST_AUTO_TEST_CASE(NativeIdentityFetchExceptionTest)
{
	NzbSummary info;
	info.dataBytes = 200;
	info.heads = { { "first.mkv", "a@x", 100 }, { "second.mkv", "b@x", 100 } };
	int attempts = 0;
	auto found = DupeIdentity::Read(info, Util::CurrentTime() + 10,
		[&](const std::string&, const std::vector<CString>&) -> ArticleFetcher::FetchedArticle {
			if (++attempts == 1) throw std::runtime_error("fake transport exception");
			ArticleFetcher::FetchedArticle result;
			result.Success = true;
			result.FileSize = 1000;
			result.Data = { 'v' };
			return result;
		});
	BOOST_CHECK_EQUAL(attempts, 2);
	BOOST_CHECK((bool)found);
	BOOST_CHECK_EQUAL(found.bytes, 1000);
}

BOOST_AUTO_TEST_CASE(NativeFetchBudgetChargesRetriesTest)
{
	NzbFetcher::Budget budget(1);
	int calls = 0;
	NzbFetcher fetcher([&](const std::string&, const std::string&, size_t, long long) {
		calls++;
		HttpGet::Reply reply;
		reply.ok = true;
		reply.status = 200;
		reply.body = "<html>indexer error</html>";
		return reply;
	});
	auto listing = Listing("http://fake.invalid/one", 1, 0, 1);
	fetcher.Fetch(listing, Util::CurrentTime() + 20, 3, &budget);
	BOOST_CHECK_EQUAL(calls, 3);
	BOOST_CHECK(budget.Spent());
}

BOOST_AUTO_TEST_CASE(NativeFetchBudgetCooldownIsFreeTest)
{
	NzbFetcher::Budget budget(1);
	int calls = 0;
	NzbFetcher fetcher([&](const std::string&, const std::string&, size_t, long long) {
		calls++;
		HttpGet::Reply reply;
		reply.status = 403;
		return reply;
	});
	auto listing = Listing("http://fake.invalid/two", 1, 0, 1, "limited");
	fetcher.Fetch(listing, Util::CurrentTime() + 10, 0, &budget);
	BOOST_CHECK_EQUAL(fetcher.Fetch(listing, Util::CurrentTime() + 10, 0, &budget).reason, NzbFetcher::frRefused);
	BOOST_CHECK_EQUAL(calls, 1);
	BOOST_CHECK(budget.Take());
	BOOST_CHECK(budget.Take());
	BOOST_CHECK(!budget.Take());
}

BOOST_AUTO_TEST_SUITE_END()
