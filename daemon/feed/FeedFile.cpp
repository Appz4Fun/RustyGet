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
#include "DownloadInfo.h"
#include "Options.h"
#include "Util.h"
#include "nzbget_rs.h"

FeedFile::FeedFile(const char* filename, const char* infoName) 
	: m_fileName(filename ? filename : "")
	, m_infoName(infoName ? infoName : "")
	, m_ignoreNextError(false)
{
	debug("Creating FeedFile");

	m_feedItems = std::make_unique<FeedItemList>();
	m_feedItemInfo = nullptr;
	m_tagContent.clear();
}

void FeedFile::LogDebugInfo()
{
	info(" FeedFile %s", m_fileName.c_str());
}

void FeedFile::ParseSubject(FeedItemInfo& feedItemInfo)
{
	NzbgetRsBuf buf = nzbget_rs_df_parse_feed_subject(feedItemInfo.GetTitle());
	feedItemInfo.SetFilename(buf.data ? buf.data : "");
	nzbget_rs_free(buf);
}


bool FeedFile::Parse()
{
	xmlSAXHandler SAX_handler{};
	SAX_handler.startElement = reinterpret_cast<startElementSAXFunc>(SAX_StartElement);
	SAX_handler.endElement = reinterpret_cast<endElementSAXFunc>(SAX_EndElement);
	SAX_handler.error = reinterpret_cast<errorSAXFunc>(SAX_error);
	SAX_handler.getEntity = reinterpret_cast<getEntitySAXFunc>(SAX_getEntity);

	/*
	 * libxml2 2.14+ sends CDATA via `cdataBlock` instead of `characters`. Earlier
	 * versions deliver CDATA in `characters`; `cdataBlock` is unused. Both
	 * require identical handling, so the same handler is assigned.
	 */
	SAX_handler.characters  = reinterpret_cast<charactersSAXFunc>(SAX_textHandler);
	SAX_handler.cdataBlock  = reinterpret_cast<cdataBlockSAXFunc>(SAX_textHandler);

	m_ignoreNextError = false;

	int ret = xmlSAXUserParseFile(&SAX_handler, this, m_fileName.c_str());

	if (ret != 0)
	{
		error("Failed to parse rss feed %s", m_infoName.c_str());
		return false;
	}

	return true;
}

int64 FeedFile::ExtractSizeFromDescription(std::string_view description)
{
	std::string descStr(description);
	return nzbget_rs_df_extract_size_from_description(descStr.c_str());
}

void FeedFile::Parse_StartElement(const char* name, const char **atts)
{
	if (!name)
		return;

	// libxml2 passes no array for an element without attributes
	static const char* noAtts[] = { nullptr };
	if (!atts)
		atts = noAtts;

	m_currentElement = name;
	ResetTagContent();

	int elemKind = nzbget_rs_dqh_feed_file_classify_start_element(name);
	if (elemKind == 1)
	{
		m_feedItems->emplace_back();
		m_feedItemInfo = &m_feedItems->back();
	}
	else if (elemKind == 2 && m_feedItemInfo)
	{
		// <enclosure url="http://myindexer.com/fetch/9eeb264aecce961a6e0d" length="150263340" type="application/x-nzb" />
		for (; *atts; atts+=2)
		{
			int encKind = nzbget_rs_sfc_feed_file_classify_enclosure_attr(atts[0]);
			if (encKind == 1)
			{
				CString url = atts[1];
				WebUtil::XmlDecode(url);
				m_feedItemInfo->SetUrl(url);
			}
			else if (encKind == 2)
			{
				int64 size = nzbget_rs_dqh_parse_c_atoll(atts[1]);
				m_feedItemInfo->SetSize(size);
			}
			if (m_feedItemInfo->GetSize() == 0)
			{
				int64 size = ExtractSizeFromDescription(m_feedItemInfo->GetDescription());
				m_feedItemInfo->SetSize(size);
			}
		}
	}
	else if (m_feedItemInfo && 
		atts[0] && atts[1] && atts[2] && atts[3] &&
		nzbget_rs_sfc_feed_file_is_newznab_attr_element(name, atts[0], atts[1], atts[2], atts[3]) != 0
	)
	{
		m_feedItemInfo->GetAttributes()->emplace_back(atts[1], atts[3]);

		int attrKind = nzbget_rs_sfc_feed_file_classify_newznab_attr_key(atts[1], m_feedItemInfo->GetSize());
		if (attrKind == 1)
		{
			int64 size = nzbget_rs_dqh_parse_c_atoll(atts[3]);
			m_feedItemInfo->SetSize(size);
		}
		else if (attrKind == 2)
		{
			m_feedItemInfo->SetImdbId(nzbget_rs_dqh_parse_c_atoi(atts[3]));
		}
		else if (attrKind == 3)
		{
			m_feedItemInfo->SetRageId(nzbget_rs_dqh_parse_c_atoi(atts[3]));
		}
		else if (attrKind == 4)
		{
			m_feedItemInfo->SetTvdbId(nzbget_rs_dqh_parse_c_atoi(atts[3]));
		}
		else if (attrKind == 5)
		{
			m_feedItemInfo->SetTvmazeId(nzbget_rs_dqh_parse_c_atoi(atts[3]));
		}
		else if (attrKind == 6)
		{
			m_feedItemInfo->SetEpisode(atts[3]);
		}
		else if (attrKind == 7)
		{
			m_feedItemInfo->SetSeason(atts[3]);
		}
	}
}

void FeedFile::Parse_EndElement(const char* name)
{
	if (!name)
		return;

	// Trim the collected content once after all chunks (text + CDATA) are merged
	Util::Trim(m_tagContent);

	int endKind = m_feedItemInfo
		? nzbget_rs_sfc_feed_file_classify_end_element(name, 1, m_feedItemInfo->GetUrl()[0] == '\0' ? 1 : 0)
		: 0;
	if (endKind == 1)
	{
		m_feedItemInfo->SetTitle(m_tagContent.c_str());
		ResetTagContent();
		ParseSubject(*m_feedItemInfo);
	}
	else if (endKind == 2)
	{
		m_feedItemInfo->SetUrl(m_tagContent.c_str());
		ResetTagContent();
	}
	else if (endKind == 3)
	{
		m_feedItemInfo->SetCategory(m_tagContent.c_str());
		ResetTagContent();
	}
	else if (endKind == 4)
	{
		// cleanup CDATA
		CString description = m_tagContent.c_str();
		WebUtil::XmlStripTags(description);
		WebUtil::XmlDecode(description);
		WebUtil::XmlRemoveEntities(description);
		m_feedItemInfo->SetDescription(description);
		ResetTagContent();
	}
	else if (endKind == 5)
	{
		time_t unixtime = WebUtil::ParseRfc822DateTime(m_tagContent.c_str());
		if (unixtime > 0)
		{
			m_feedItemInfo->SetTime(unixtime);
		}
		ResetTagContent();
	}

	m_currentElement.clear();
}

void FeedFile::Parse_Content(std::string content)
{
	m_tagContent.append(std::move(content));
}

void FeedFile::ResetTagContent()
{
	m_tagContent.clear();
}

void FeedFile::SAX_StartElement(FeedFile* file, const char *name, const char **atts)
{
	file->Parse_StartElement(name, atts);
}

void FeedFile::SAX_EndElement(FeedFile* file, const char *name)
{
	file->Parse_EndElement(name);
}

void FeedFile::SAX_textHandler(FeedFile* file, const char* xmlstr, int len)
{
	if (!xmlstr || len <= 0)
		return;

	std::string str(xmlstr, len);
	if (file->m_currentElement == "category")
	{
		// Do not break existing users' filters that rely on this normalization
		Util::Trim(str);
	}
	file->Parse_Content(std::move(str));
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
	errMsg[1024-1] = '\0';
	va_end(argp);

	// remove trailing CRLF
	errMsg[nzbget_rs_sfc_feed_file_trim_trailing_crlf_space_len(errMsg)] = '\0';
	error("Error parsing rss feed %s: %s", file->m_infoName.c_str(), errMsg);
}
