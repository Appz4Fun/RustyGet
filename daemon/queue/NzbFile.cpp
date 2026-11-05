/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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
#include "NzbFile.h"
#include "Log.h"
#include "DownloadInfo.h"
#include "Options.h"
#include "DiskState.h"
#include "Util.h"
#include "FileSystem.h"
#include "Deobfuscation.h"
#include "nzbget_rs.h"

NzbFile::NzbFile(const char* fileName, const char* category) 
	: m_fileName{ fileName ? fileName : "" }
{
	debug("Creating NZBFile");

	m_nzbInfo = std::make_unique<NzbInfo>();
	m_nzbInfo->SetFilename(fileName);
	m_nzbInfo->SetCategory(category);
	m_nzbInfo->BuildDestDirName();
}

void NzbFile::LogDebugInfo()
{
	info(" NZBFile %s", m_fileName.c_str());
}

// The C library's character classes and lower case under the daemon's
// locale, as the C++ took them (isspace, isdigit, tolower)
static NzbgetRsCtype RsCtype()
{
#ifdef __GLIBC__
	return {*__ctype_b_loc(), (unsigned short)_ISspace, (unsigned short)_ISdigit,
		reinterpret_cast<const int*>(*__ctype_tolower_loc()), CHAR_MIN < 0};
#else
	static thread_local unsigned short classes[256];
	static thread_local int lower[384];
	for (int c = 0; c < 256; c++)
	{
		classes[c] = (isspace(c) ? 1 : 0) | (isdigit(c) ? 2 : 0);
	}
	for (int c = -128; c < 256; c++)
	{
		// negative chars are outside the ctype API's unsigned char/EOF domain:
		// left unchanged, as Darwin and musl leave them
		lower[c + 128] = c < 0 ? c : tolower(c);
	}
	return {classes, 1, 2, lower + 128, CHAR_MIN < 0};
#endif
}

int NzbFile::DeclaredParts(const char* subject)
{
	// rust/src/nzbfile.rs
	NzbgetRsCtype ct = RsCtype();
	return nzbget_rs_nzb_declared_parts(subject, &ct);
}

bool NzbFile::Parse()
{
	xmlSAXHandler SAX_handler = {0};
	SAX_handler.startElement = reinterpret_cast<startElementSAXFunc>(SAX_StartElement);
	SAX_handler.endElement = reinterpret_cast<endElementSAXFunc>(SAX_EndElement);
	SAX_handler.characters = reinterpret_cast<charactersSAXFunc>(SAX_characters);
	SAX_handler.error = reinterpret_cast<errorSAXFunc>(SAX_error);
	SAX_handler.getEntity = reinterpret_cast<getEntitySAXFunc>(SAX_getEntity);

	std::unique_ptr<NzbgetRsNzbFileParser, void (*)(NzbgetRsNzbFileParser*)> parser(
		nzbget_rs_nzbfile_parser_new(m_fileName.c_str()),
		nzbget_rs_nzbfile_parser_free);
	m_parser = parser.get();

	int ret = xmlSAXUserParseFile(&SAX_handler, this, m_fileName.c_str());

	NzbgetRsCtype ct = RsCtype();
	NzbgetRsNzbFileSummary summary{};
	bool ok = nzbget_rs_nzbfile_parser_finalize(
		m_parser,
		ret,
		g_Options ? g_Options->GetParIgnoreExt() : "",
		&ct,
		[](const char* subject, const char* filename)
		{
			detail("Extracting a filename from Subject %s", subject);
			detail("Extracted Filename: %s", filename);
		},
		[](const char* const* filenames, size_t* indices, size_t count)
		{
			std::sort(indices, indices + count,
				[filenames](size_t a, size_t b)
				{
					return strcmp(filenames[a], filenames[b]) > 0;
				});
		},
		&summary) != 0;

	for (size_t i = 0; i < summary.messages_count; i++)
	{
		int kind = 0;
		const char* text = nullptr;
		if (nzbget_rs_nzbfile_parser_get_message(m_parser, i, &kind, &text) && text)
		{
			m_nzbInfo->AddMessage(
				kind == 2 ? Message::mkError : Message::mkWarning,
				text);
		}
	}

	if (!ok)
	{
		m_parser = nullptr;
		return false;
	}

	m_nzbInfo->SetManyDupeFiles(summary.many_dupe_files != 0);
	m_nzbInfo->SetFileCount(summary.file_count);
	m_nzbInfo->SetTotalArticles(summary.total_articles);
	m_nzbInfo->SetFailedArticles(summary.failed_articles);
	m_nzbInfo->SetCurrentFailedArticles(summary.cur_failed_articles);
	m_nzbInfo->SetSize(summary.size);
	m_nzbInfo->SetRemainingSize(summary.remaining_size);
	m_nzbInfo->SetFailedSize(summary.failed_size);
	m_nzbInfo->SetCurrentFailedSize(summary.cur_failed_size);
	m_nzbInfo->SetParSize(summary.par_size);
	m_nzbInfo->SetParFailedSize(summary.par_failed_size);
	m_nzbInfo->SetParCurrentFailedSize(summary.par_cur_failed_size);
	m_nzbInfo->SetRemainingParCount(summary.remaining_par_count);
	m_nzbInfo->SetFullContentHash(summary.full_content_hash);
	m_nzbInfo->SetFilteredContentHash(summary.filtered_content_hash);

	for (size_t i = 0; i < summary.kept_files_count; i++)
	{
		NzbgetRsNzbKeptFileView view{};
		if (!nzbget_rs_nzbfile_parser_get_file(m_parser, i, &view))
		{
			continue;
		}
		std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>(view.id);
		fileInfo->SetNzbInfo(m_nzbInfo.get());
		fileInfo->SetSubject(view.subject ? view.subject : "");
		fileInfo->SetFilename(view.filename ? view.filename : "");
		fileInfo->SetTime(view.time);
		fileInfo->SetSize(view.size);
		fileInfo->SetRemainingSize(view.remaining_size);
		fileInfo->SetMissedSize(view.missed_size);
		fileInfo->SetTotalArticles(view.total_articles);
		fileInfo->SetMissedArticles(view.missed_articles);
		fileInfo->SetParFile(view.par_file != 0);

		for (size_t g = 0; g < view.groups_count; g++)
		{
			fileInfo->GetGroups()->push_back(view.groups_ptrs[g] ? view.groups_ptrs[g] : "");
		}

		ArticleList* articles = fileInfo->GetArticles();
		articles->reserve(view.articles_count);
		for (size_t a = 0; a < view.articles_count; a++)
		{
			std::unique_ptr<ArticleInfo> article = std::make_unique<ArticleInfo>();
			article->SetPartNumber(view.article_parts[a]);
			article->SetSize(view.article_sizes[a]);
			article->SetMessageId(view.article_msg_ids[a] ? view.article_msg_ids[a] : "");
			articles->push_back(std::move(article));
		}

		if (view.cut_short)
		{
			m_cutShortFiles.insert(fileInfo.get());
		}

		m_nzbInfo->GetFileList()->Add(std::move(fileInfo));
	}

	m_nzbInfo->UpdateMinMaxTime();

	if (g_Options && g_Options->GetServerMode())
	{
		for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
		{
			g_DiskState->SaveFile(fileInfo);
			fileInfo->GetArticles()->clear();
		}
	}

	m_password = summary.password ? summary.password : "";
	m_metaName = summary.meta_name ? summary.meta_name : "";
	m_metaTitle = summary.meta_title ? summary.meta_title : "";
	m_category = summary.category ? summary.category : "";

	if (!m_metaName.empty() && m_nzbInfo)
	{
		m_nzbInfo->SetMetaName(m_metaName);
	}

	m_parser = nullptr;
	return true;
}

void NzbFile::SAX_StartElement(NzbFile* file, const char *name, const char **atts)
{
	nzbget_rs_nzbfile_parser_start_element(
		file->m_parser,
		name,
		atts,
		nullptr,
		[](void*) -> int
		{
			FileInfo tmp;
			return tmp.GetId();
		});
}

void NzbFile::SAX_EndElement(NzbFile* file, const char *name)
{
	nzbget_rs_nzbfile_parser_end_element(file->m_parser, name);
}

void NzbFile::SAX_characters(NzbFile* file, const char * xmlstr, int len)
{
	nzbget_rs_nzbfile_parser_characters(file->m_parser, xmlstr, len);
}

void* NzbFile::SAX_getEntity(NzbFile* file, const char * name)
{
	xmlEntityPtr e = xmlGetPredefinedEntity(reinterpret_cast<const xmlChar*>(name));
	if (!e)
	{
		nzbget_rs_nzbfile_parser_on_missing_entity(file->m_parser);
	}
	return e;
}

void NzbFile::SAX_error(NzbFile* file, const char *msg, ...)
{
	va_list argp;
	va_start(argp, msg);
	char errMsg[1024];
	vsnprintf(errMsg, sizeof(errMsg), msg, argp);
	errMsg[1024-1] = '\0';
	va_end(argp);

	nzbget_rs_nzbfile_parser_on_sax_error(file->m_parser, errMsg);
}
