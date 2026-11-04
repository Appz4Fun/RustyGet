/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef FEEDFILTER_H
#define FEEDFILTER_H

#include "NString.h"
#include "DownloadInfo.h"
#include "FeedInfo.h"
#include "Util.h"

struct NzbgetRsFeedFilter;

// rust/src/feedfilter.rs
class FeedFilter
{
public:
	FeedFilter(const char* filter);
	~FeedFilter();
	FeedFilter(const FeedFilter&) = delete;
	FeedFilter& operator=(const FeedFilter&) = delete;
	void Match(FeedItemInfo& feedItemInfo);

private:
	NzbgetRsFeedFilter* m_filter;
	std::vector<std::unique_ptr<RegEx>> m_regExes;

	struct Context;
	// Allocation failures and helper exceptions must not unwind through Rust.
	static void Field(void* user, int field, const char* attr, const char** str, long long* num) noexcept;
	static const char* SeasonEpisode(void* user, int episode) noexcept;
	static size_t RegexNew(void* user, const char* pattern, int bufSize) noexcept;
	static int RegexMatch(void* user, size_t regex, const char* text, int (*groups)[2], int capacity) noexcept;
	static void Apply(void* user, const struct NzbgetRsFeedOptions* options) noexcept;
	static void SetMatch(void* user, int status, int rule) noexcept;
};

#endif
