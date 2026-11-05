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
	, m_ignoreNextError(false)
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

ArticleInfo* NzbFile::AddArticle(FileInfo* fileInfo, std::unique_ptr<ArticleInfo> articleInfo)
{
	// kept in file order and put in part-number order when the file ends: a
	// list indexed by the part number took the memory a huge number asks for
	fileInfo->GetArticles()->push_back(std::move(articleInfo));
	return fileInfo->GetArticles()->back().get();
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

void NzbFile::AddFileInfo(std::unique_ptr<FileInfo> fileInfo)
{
	// rust/src/nzbfile.rs: the segments in part-number order (of two with one
	// number the later counts), the parts the subject declares past the last
	// one when the nzb plainly cut the file short, and the file's sizes
	ArticleList* articles = fileInfo->GetArticles();
	std::vector<int> parts;
	std::vector<int64> sizes;
	parts.reserve(articles->size());
	sizes.reserve(articles->size());
	for (std::unique_ptr<ArticleInfo>& article : *articles)
	{
		parts.push_back(article->GetPartNumber());
		sizes.push_back(article->GetSize());
	}
	std::vector<size_t> order(articles->size());
	NzbgetRsCtype ct = RsCtype();
	NzbgetRsFileTotals totals;
	if (!nzbget_rs_nzb_file_totals(fileInfo->GetSubject(), parts.data(), sizes.data(), parts.size(), &ct,
		order.data(), &totals))
	{
		return;
	}

	ArticleList kept;
	kept.reserve(totals.kept);
	for (size_t k = 0; k < totals.kept; k++)
	{
		kept.push_back(std::move((*articles)[order[k]]));
	}
	*articles = std::move(kept);

	if (totals.cutShort)
	{
		m_cutShortFiles.insert(fileInfo.get());
	}
	fileInfo->SetNzbInfo(m_nzbInfo.get());
	fileInfo->SetSize(totals.size);
	fileInfo->SetRemainingSize(totals.remainingSize);
	fileInfo->SetMissedSize(totals.missedSize);
	fileInfo->SetTotalArticles(totals.totalArticles);
	fileInfo->SetMissedArticles(totals.missedArticles);
	m_nzbInfo->GetFileList()->Add(std::move(fileInfo));
}

void NzbFile::ParseSubject(FileInfo* fileInfo, bool TryQuotes)
{
	if (!fileInfo) return;

	if (!fileInfo->GetSubject())
	{
		// Malformed file element without subject. We generate subject using internal element id.
		NzbgetRsBuf subjBuf = nzbget_rs_dqh_conn_format_port_str(fileInfo->GetId());
		fileInfo->SetSubject(subjBuf.data ? subjBuf.data : "");
		nzbget_rs_free(subjBuf);
		return;
	}

	detail("Extracting a filename from Subject %s", fileInfo->GetSubject());

	std::string filename = Deobfuscation::Deobfuscate(fileInfo->GetSubject());

	detail("Extracted Filename: %s", filename.c_str());

	fileInfo->SetFilename(std::move(filename));
}

bool NzbFile::HasDuplicateFilenames()
{
	// rust/src/nzbfile.rs: one pass, counting each name's and each name and
	// subject's later files
	std::vector<const char*> names;
	std::vector<const char*> subjects;
	names.reserve(m_nzbInfo->GetFileList()->size());
	subjects.reserve(m_nzbInfo->GetFileList()->size());
	for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
	{
		names.push_back(fileInfo->GetFilename());
		subjects.push_back(fileInfo->GetSubject());
	}
	return nzbget_rs_nzb_duplicate_filenames(names.data(), subjects.data(), names.size()) != 0;
}

/**
 * Generate filenames from subjects and check if the parsing of subject was correct
 */
void NzbFile::BuildFilenames()
{
	for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
	{
		ParseSubject(fileInfo, true);
	}

	if (HasDuplicateFilenames())
	{
		for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
		{
			ParseSubject(fileInfo, false);
		}
	}

	if (HasDuplicateFilenames())
	{
		m_nzbInfo->SetManyDupeFiles(true);
		for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
		{
			fileInfo->SetFilename(fileInfo->GetSubject());
		}
	}
}

void NzbFile::CalcHashes()
{
	RawFileList sortedFiles;

	for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
	{
		sortedFiles.push_back(fileInfo);
	}

	std::sort(sortedFiles.begin(), sortedFiles.end(),
		[](FileInfo* first, FileInfo* second)
		{
			return nzbget_rs_dqh_bytes_cmp(first->GetFilename(), second->GetFilename()) > 0;
		});

	size_t fileCount = sortedFiles.size();
	std::vector<uint8_t> skipFlags;
	std::vector<size_t> articleCounts;
	std::vector<const char*> messageIds;
	skipFlags.reserve(fileCount);
	articleCounts.reserve(fileCount);

	for (FileInfo* fileInfo : sortedFiles)
	{
		bool skip = !fileInfo->GetParFile() &&
			Util::MatchFileExt(fileInfo->GetFilename(), g_Options->GetParIgnoreExt(), ",;");
		skipFlags.push_back(skip ? 1 : 0);
		articleCounts.push_back(fileInfo->GetArticles()->size());
		for (ArticleInfo* article : fileInfo->GetArticles())
		{
			messageIds.push_back(article->GetMessageId());
		}
	}

	uint32 fullContentHash = 0;
	uint32 filteredContentHash = 0;
	nzbget_rs_sus_calc_nzb_hashes(
		skipFlags.data(),
		articleCounts.data(),
		fileCount,
		messageIds.data(),
		messageIds.size(),
		&fullContentHash,
		&filteredContentHash);

	m_nzbInfo->SetFullContentHash(fullContentHash);
	m_nzbInfo->SetFilteredContentHash(filteredContentHash);
}

void NzbFile::ProcessFiles()
{
	BuildFilenames();

	for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
	{
		fileInfo->MakeValidFilename();

		int fileCount = m_nzbInfo->GetFileCount();
		int totalArticles = m_nzbInfo->GetTotalArticles();
		int failedArticles = m_nzbInfo->GetFailedArticles();
		int curFailedArticles = m_nzbInfo->GetCurrentFailedArticles();
		int64 size = m_nzbInfo->GetSize();
		int64 remSize = m_nzbInfo->GetRemainingSize();
		int64 failedSize = m_nzbInfo->GetFailedSize();
		int64 curFailedSize = m_nzbInfo->GetCurrentFailedSize();
		int64 parSize = m_nzbInfo->GetParSize();
		int64 parFailedSize = m_nzbInfo->GetParFailedSize();
		int64 parCurFailedSize = m_nzbInfo->GetParCurrentFailedSize();
		int remParCount = m_nzbInfo->GetRemainingParCount();
		bool parFile = nzbget_rs_pnd_nzbfile_accumulate_file_stats(
			fileInfo->GetFilename(),
			fileInfo->GetTotalArticles(),
			fileInfo->GetMissedArticles(),
			fileInfo->GetSize(),
			fileInfo->GetRemainingSize(),
			fileInfo->GetMissedSize(),
			&fileCount, &totalArticles, &failedArticles, &curFailedArticles,
			&size, &remSize, &failedSize, &curFailedSize,
			&parSize, &parFailedSize, &parCurFailedSize, &remParCount) != 0;
		m_nzbInfo->SetFileCount(fileCount);
		m_nzbInfo->SetTotalArticles(totalArticles);
		m_nzbInfo->SetFailedArticles(failedArticles);
		m_nzbInfo->SetCurrentFailedArticles(curFailedArticles);
		m_nzbInfo->SetSize(size);
		m_nzbInfo->SetRemainingSize(remSize);
		m_nzbInfo->SetFailedSize(failedSize);
		m_nzbInfo->SetCurrentFailedSize(curFailedSize);
		fileInfo->SetParFile(parFile);
		m_nzbInfo->SetParSize(parSize);
		m_nzbInfo->SetParFailedSize(parFailedSize);
		m_nzbInfo->SetParCurrentFailedSize(parCurFailedSize);
		m_nzbInfo->SetRemainingParCount(remParCount);
	}

	m_nzbInfo->UpdateMinMaxTime();

	CalcHashes();

	if (g_Options->GetServerMode())
	{
		for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
		{
			g_DiskState->SaveFile(fileInfo);
			fileInfo->GetArticles()->clear();
		}
	}

	if (m_password.empty())
	{
		ReadPasswordFromFilename();
	}

	m_metaName = FileSystem::SanitizePathSegment(m_metaName);
	m_metaTitle = FileSystem::SanitizePathSegment(m_metaTitle);

	if (nzbget_rs_dcf_nzbfile_should_fallback_meta_name_to_title(
		m_metaName.empty() ? 1 : 0, m_metaTitle.empty() ? 1 : 0) != 0)
	{
		m_metaName = m_metaTitle;
	}

	// Sanitize control characters (\r, \n, \t, etc.) to prevent
	// line desynchronization in the line-based DiskState file format.
	Util::SanitizeLine(m_category);

	if (!m_metaName.empty() && m_nzbInfo)
	{
		m_nzbInfo->SetMetaName(m_metaName);
	}
}
/*
* Attempt to Read the Password from the Filename encoded in {{ Bracets }}
*/
void NzbFile::ReadPasswordFromFilename()
{
	size_t start = 0, len = 0;
	if (nzbget_rs_sus_extract_password_from_filename(m_fileName.c_str(), &start, &len))
	{
		m_password = m_fileName.substr(start, len);
	}
}

bool NzbFile::Parse()
{
	xmlSAXHandler SAX_handler = {0};
	SAX_handler.startElement = reinterpret_cast<startElementSAXFunc>(SAX_StartElement);
	SAX_handler.endElement = reinterpret_cast<endElementSAXFunc>(SAX_EndElement);
	SAX_handler.characters = reinterpret_cast<charactersSAXFunc>(SAX_characters);
	SAX_handler.error = reinterpret_cast<errorSAXFunc>(SAX_error);
	SAX_handler.getEntity = reinterpret_cast<getEntitySAXFunc>(SAX_getEntity);

	m_ignoreNextError = false;

	int ret = xmlSAXUserParseFile(&SAX_handler, this, m_fileName.c_str());

	if (ret != 0)
	{
		NzbgetRsBuf errBuf = nzbget_rs_tpr_nzbfile_format_parse_error_msg(
			FileSystem::BaseFileName(m_fileName.c_str()), 0);
		m_nzbInfo->AddMessage(Message::mkError, errBuf.data ? errBuf.data : "");
		nzbget_rs_free(errBuf);
		return false;
	}

	if (m_nzbInfo->GetFileList()->empty())
	{
		NzbgetRsBuf errBuf = nzbget_rs_tpr_nzbfile_format_parse_error_msg(
			FileSystem::BaseFileName(m_fileName.c_str()), 1);
		m_nzbInfo->AddMessage(Message::mkError, errBuf.data ? errBuf.data : "");
		nzbget_rs_free(errBuf);
		return false;
	}

	ProcessFiles();

	return true;
}

void NzbFile::Parse_StartElement(const char *name, const char **atts)
{
	m_currentElement = name;
	m_tagContent.Clear();

	int elemKind = nzbget_rs_dqh_nzbfile_classify_start_element(name);
	if (elemKind == 1)
	{
		m_fileInfo = std::make_unique<FileInfo>();
		m_fileInfo->SetFilename(m_fileName.c_str());

		if (!atts)
		{
			NzbgetRsBuf msgBuf = nzbget_rs_dqh_nzbfile_format_missing_attrs_msg(name);
			m_nzbInfo->AddMessage(Message::mkWarning, msgBuf.data ? msgBuf.data : "");
			nzbget_rs_free(msgBuf);
			return;
		}

		for (int i = 0; atts[i]; i += 2)
		{
			const char* attrname = atts[i];
			const char* attrvalue = atts[i + 1];
			switch (nzbget_rs_dqh_nzbfile_classify_file_attr(attrname))
			{
				case 1:
					m_fileInfo->SetSubject(attrvalue);
					break;
				case 2:
					m_fileInfo->SetTime(nzbget_rs_dqh_parse_c_atoi(attrvalue));
					break;
			}
		}
	}
	else if (elemKind == 2)
	{
		if (!m_fileInfo)
		{
			m_nzbInfo->AddMessage(Message::mkWarning, "Malformed nzb-file, tag <segment> without tag <file>");
			return;
		}

		if (!atts)
		{
			NzbgetRsBuf msgBuf = nzbget_rs_dqh_nzbfile_format_missing_attrs_msg(name);
			m_nzbInfo->AddMessage(Message::mkWarning, msgBuf.data ? msgBuf.data : "");
			nzbget_rs_free(msgBuf);
			return;
		}

		int64 lsize = -1;
		int partNumber = -1;

		for (int i = 0; atts[i]; i += 2)
		{
			const char* attrname = atts[i];
			const char* attrvalue = atts[i + 1];
			switch (nzbget_rs_dqh_nzbfile_classify_segment_attr(attrname))
			{
				case 1:
					lsize = nzbget_rs_dqh_parse_c_atoll(attrvalue);
					break;
				case 2:
					partNumber = nzbget_rs_dqh_parse_c_atoi(attrvalue);
					break;
			}
		}

		if (partNumber > 0)
		{
			// new segment, add it!
			std::unique_ptr<ArticleInfo> article = std::make_unique<ArticleInfo>();
			article->SetPartNumber(partNumber);
			article->SetSize(lsize);
			m_article = AddArticle(m_fileInfo.get(), std::move(article));
		}
	}
	else if (elemKind == 3)
	{
		m_hasPassword = false;
		m_hasCategory = false;
		m_hasName = false;
		m_hasTitle = false;

		if (!atts)
		{
			NzbgetRsBuf msgBuf = nzbget_rs_dqh_nzbfile_format_missing_attrs_msg(name);
			m_nzbInfo->AddMessage(Message::mkWarning, msgBuf.data ? msgBuf.data : "");
			nzbget_rs_free(msgBuf);
			return;
		}

		for (int i = 0; atts[i] && atts[i + 1]; i += 2)
		{
			switch (nzbget_rs_rrd_nzbfile_classify_meta_type(atts[i], atts[i + 1]))
			{
				case 1: m_hasPassword = true; break;
				case 2: m_hasCategory = true; break;
				case 3: m_hasName = true; break;
				case 4: m_hasTitle = true; break;
			}
		}
	}
}

void NzbFile::Parse_EndElement(const char *name)
{
	int elemKind = nzbget_rs_dqh_nzbfile_classify_end_element(name);
	if (elemKind == 1)
	{
		// Close the file element, add the new file to file-list
		AddFileInfo(std::move(m_fileInfo));
		m_article = nullptr;
	}
	else if (elemKind == 2)
	{
		if (!m_fileInfo)
		{
			// error: bad nzb-file
			return;
		}

		m_fileInfo->GetGroups()->push_back(*m_tagContent);
		m_tagContent.Clear();
	}
	else if (elemKind == 3)
	{
		if (!m_fileInfo || !m_article)
		{
			// error: bad nzb-file
			return;
		}

		// Get the #text part; it goes into NNTP commands as is, so a line break
		// in it would send a command of the nzb's choosing on our connection.
		// Spaces and control characters can't be in a message-id: replaced, the
		// article is simply not found
		NzbgetRsBuf idBuf = nzbget_rs_sus_sanitize_segment_message_id(*m_tagContent);
		m_article->SetMessageId(idBuf.data ? idBuf.data : "");
		nzbget_rs_free(idBuf);
		m_article = nullptr;
	}
	else if (elemKind == 4 && m_hasPassword)
	{
		m_password = m_tagContent;
	}
	else if (elemKind == 4 && m_hasCategory)
	{
		m_category = m_tagContent;
	}
	else if (elemKind == 4 && m_hasName)
	{
		m_metaName = m_tagContent;
	}
	else if (elemKind == 4 && m_hasTitle)
	{
		m_metaTitle = m_tagContent;
	}

	m_currentElement.clear();
}

void NzbFile::Parse_Content(const char *buf, int len)
{
	m_tagContent.Append(buf, len);
}

void NzbFile::SAX_StartElement(NzbFile* file, const char *name, const char **atts)
{
	file->Parse_StartElement(name, atts);
}

void NzbFile::SAX_EndElement(NzbFile* file, const char *name)
{
	file->Parse_EndElement(name);
}

void NzbFile::SAX_characters(NzbFile* file, const char * xmlstr, int len)
{
	if (len <= 0)
		return;

	std::string str(xmlstr, len);
	if (file->m_currentElement == "meta" && file->m_hasCategory)
	{
		// Do not break existing users' filters that rely on this normalization
		Util::Trim(str);
	}

	if (!str.empty())
	{
		file->Parse_Content(str.data(), str.length());
	}
}

void* NzbFile::SAX_getEntity(NzbFile* file, const char * name)
{
	xmlEntityPtr e = xmlGetPredefinedEntity(reinterpret_cast<const xmlChar*>(name));

	if (!e)
	{
		file->m_nzbInfo->AddMessage(Message::mkWarning, "entity not found");
		file->m_ignoreNextError = true;
	}

	return e;
}

void NzbFile::SAX_error(NzbFile* file, const char *msg, ...)
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
	nzbget_rs_tpr_nzbfile_trim_sax_error_in_place(errMsg);

	NzbgetRsBuf errBuf = nzbget_rs_tpr_nzbfile_format_sax_error_msg(errMsg);
	file->m_nzbInfo->AddMessage(Message::mkError, errBuf.data ? errBuf.data : "");
	nzbget_rs_free(errBuf);
}
