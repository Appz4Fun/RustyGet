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
	m_parser = nzbget_rs_feed_file_parser_new();

	xmlSAXHandler SAX_handler{};
	SAX_handler.startElement = reinterpret_cast<startElementSAXFunc>(SAX_StartElement);
	SAX_handler.endElement = reinterpret_cast<endElementSAXFunc>(SAX_EndElement);
	SAX_handler.error = reinterpret_cast<errorSAXFunc>(SAX_error);
	SAX_handler.getEntity = reinterpret_cast<getEntitySAXFunc>(SAX_getEntity);
	SAX_handler.characters = reinterpret_cast<charactersSAXFunc>(SAX_textHandler);
	SAX_handler.cdataBlock = reinterpret_cast<cdataBlockSAXFunc>(SAX_textHandler);

	m_ignoreNextError = false;

	int ret = xmlSAXUserParseFile(&SAX_handler, this, m_fileName.c_str());
	if (ret != 0)
	{
		nzbget_rs_feed_file_parser_free(m_parser);
		m_parser = nullptr;
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

void FeedFile::SAX_StartElement(FeedFile* file, const char *name, const char **atts)
{
	nzbget_rs_feed_file_parser_start_element(file->m_parser, name, atts);
}

void FeedFile::SAX_EndElement(FeedFile* file, const char *name)
{
	nzbget_rs_feed_file_parser_end_element(file->m_parser, name);
}

void FeedFile::SAX_textHandler(FeedFile* file, const char* xmlstr, int len)
{
	nzbget_rs_feed_file_parser_text(file->m_parser, xmlstr, len);
}

xmlEntityPtr FeedFile::SAX_getEntity(FeedFile* file, const xmlChar* name)
{
	xmlEntityPtr e = xmlGetPredefinedEntity(name);
	if (!e)
	{
		warn("entity not found");
		file->m_ignoreNextError = true;
	}
	return e;
}

void FeedFile::SAX_error(FeedFile* file, const char *msg, ...)
{
	if (file->m_ignoreNextError)
	{
		file->m_ignoreNextError = false;
		return;
	}

	va_list argp;
	va_start(argp, msg);
	char errMsg[1024];
	vsnprintf(errMsg, sizeof(errMsg), msg, argp);
	errMsg[sizeof(errMsg) - 1] = '\0';
	va_end(argp);

	errMsg[nzbget_rs_feed_file_trim_sax_error(errMsg)] = '\0';
	error("Error parsing rss feed %s: %s", file->m_infoName.c_str(), errMsg);
}
