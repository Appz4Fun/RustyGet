/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "Log.h"
#include "DownloadInfo.h"
#include "Util.h"
#include "FeedFilter.h"

#include <climits>
#include <type_traits>
#include "nzbget_rs.h"

// the feed item and this filter's regular expressions, for the callbacks
struct FeedFilter::Context
{
	FeedFilter* filter;
	FeedItemInfo* item;
};

FeedFilter::FeedFilter(const char* filter) :
	m_filter(nzbget_rs_feed_filter_new(filter))
{
}

FeedFilter::~FeedFilter()
{
	nzbget_rs_feed_filter_free(m_filter);
}

// Term::GetFieldData's values, by field number (see nzbget_rs.h)
void FeedFilter::Field(void* user, int field, const char* attr, const char** str, long long* num) noexcept
{
	FeedItemInfo* item = static_cast<Context*>(user)->item;
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
		{
			// Subtract at time_t's width, preserving legacy wraparound for
			// extreme timestamps without overflowing a signed time_t.
			using UnsignedTime = std::make_unsigned_t<time_t>;
			*num = static_cast<time_t>(static_cast<UnsignedTime>(Util::CurrentTime()) -
				static_cast<UnsignedTime>(item->GetTime()));
			break;
		}
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

const char* FeedFilter::SeasonEpisode(void* user, int episode) noexcept
{
	FeedItemInfo* item = static_cast<Context*>(user)->item;
	// GetSeasonNum/GetEpisodeNum parse the title
	if (episode)
	{
		item->GetEpisodeNum();
		return item->GetEpisode();
	}
	item->GetSeasonNum();
	return item->GetSeason();
}

size_t FeedFilter::RegexNew(void* user, const char* pattern, int bufSize) noexcept
{
	FeedFilter* filter = static_cast<Context*>(user)->filter;
	filter->m_regExes.push_back(std::make_unique<RegEx>(pattern, bufSize));
	return filter->m_regExes.size() - 1;
}

int FeedFilter::RegexMatch(void* user, size_t regex, const char* text, int (*groups)[2], int capacity) noexcept
{
	RegEx* regEx = static_cast<Context*>(user)->filter->m_regExes[regex].get();
	if (!regEx->Match(text))
	{
		return -1;
	}
	int count = regEx->GetMatchCount();
	for (int i = 0; i < count && i < capacity; i++)
	{
		groups[i][0] = regEx->GetMatchStart(i);
		groups[i][1] = regEx->GetMatchLen(i);
	}
	return count;
}

// ApplyOptions
void FeedFilter::Apply(void* user, const NzbgetRsFeedOptions* o) noexcept
{
	FeedItemInfo* item = static_cast<Context*>(user)->item;
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
		// Preserve the legacy wraparound without signed-overflow UB.
		item->SetPriority(static_cast<int>(static_cast<unsigned int>(item->GetPriority()) +
			static_cast<unsigned int>(o->addPriority)));
	}
	if (o->hasDupeScore)
	{
		item->SetDupeScore(o->dupeScore);
	}
	if (o->hasAddDupeScore)
	{
		item->SetDupeScore(static_cast<int>(static_cast<unsigned int>(item->GetDupeScore()) +
			static_cast<unsigned int>(o->addDupeScore)));
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

void FeedFilter::SetMatch(void* user, int status, int rule) noexcept
{
	FeedItemInfo* item = static_cast<Context*>(user)->item;
	item->SetMatchStatus(status == 1 ? FeedItemInfo::msAccepted :
		status == 2 ? FeedItemInfo::msRejected : FeedItemInfo::msIgnored);
	item->SetMatchRule(rule);
}

namespace
{
	// tolower as WildMask's C++ code did: on a char, signed or not
	int FeedFilterFold(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		return tolower(ch);
#else
		return ch < 0 ? ch : tolower(ch);
#endif
	}
}

void FeedFilter::Match(FeedItemInfo& feedItemInfo)
{
	Context context{this, &feedItemInfo};
	NzbgetRsFeedItem item{};
	item.user = &context;
	item.field = Field;
	item.seasonEpisode = SeasonEpisode;
	item.regexNew = RegexNew;
	item.regexMatch = RegexMatch;
	item.apply = Apply;
	item.setMatch = SetMatch;
#ifdef __GLIBC__
	item.lowerTable = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
	item.lowerTable = nullptr;
#endif
	item.charSigned = CHAR_MIN < 0;
	item.fold = FeedFilterFold;
	nzbget_rs_feed_filter_match(m_filter, &item);
}

