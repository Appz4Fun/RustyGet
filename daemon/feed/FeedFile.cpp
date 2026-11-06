/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "FeedFile.h"
#include "Log.h"
#include "nzbget_rs.h"

FeedFile::FeedFile(const char* filename, const char* infoName)
	: m_feedItems(std::make_unique<FeedItemList>())
	, m_fileName(filename ? filename : "")
	, m_infoName(infoName ? infoName : "")
{
	debug("Creating FeedFile");
}

FeedFile::~FeedFile()
{
	nzbget_rs_feed_file_parser_free(m_parser);
}

void FeedFile::LogDebugInfo()
{
	info(" FeedFile %s", m_fileName.c_str());
}

bool FeedFile::Parse()
{
	nzbget_rs_feed_file_parser_free(m_parser);
	m_parser = nzbget_rs_feed_file_parser_parse_file(m_fileName.c_str());
	if (!m_parser)
	{
		error("Failed to parse rss feed %s", m_infoName.c_str());
		return false;
	}

	size_t count = nzbget_rs_feed_file_parser_finalize(m_parser);
	for (size_t i = 0; i < count; ++i)
	{
		NzbgetRsParsedFeedItemView view{};
		if (nzbget_rs_feed_file_parser_get_item(m_parser, i, &view) == 0)
		{
			continue;
		}
		m_feedItems->emplace_back();
		FeedItemInfo& item = m_feedItems->back();
		item.SetTitle(view.title);
		item.SetFilename(view.filename);
		item.SetUrl(view.url);
		item.SetCategory(view.category);
		item.SetDescription(view.description);
		if (view.has_season != 0)
		{
			item.SetSeason(view.season);
		}
		if (view.has_episode != 0)
		{
			item.SetEpisode(view.episode);
		}
		item.SetTime((time_t)view.time);
		item.SetSize(view.size);
		item.SetImdbId(view.imdb_id);
		item.SetRageId(view.rage_id);
		item.SetTvdbId(view.tvdb_id);
		item.SetTvmazeId(view.tvmaze_id);
		for (size_t k = 0; k < view.attr_count; ++k)
		{
			item.GetAttributes()->emplace_back(view.attr_names[k], view.attr_values[k]);
		}
	}

	nzbget_rs_feed_file_parser_free(m_parser);
	m_parser = nullptr;
	return true;
}
