/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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


#ifndef NZBFILE_H
#define NZBFILE_H

#include <set>
#include "NString.h"
#include "DownloadInfo.h"

class NzbFile
{
public:
	NzbFile(const char* fileName, const char* category);
	bool Parse();
	const char* GetFileName() const { return m_fileName.c_str(); }
	const std::string& GetCategoryFromFile() const { return m_category; }
	std::unique_ptr<NzbInfo> DetachNzbInfo() { return std::move(m_nzbInfo); }
	const std::string& GetPassword() const { return m_password; }
	const std::string& GetMetaName() const { return m_metaName; }
	const std::string& GetMetaTitle() const { return m_metaTitle; }

	void LogDebugInfo();
	// part count declared at the end of a subject ("... yEnc (1/N)"), or 0
	static int DeclaredParts(const char* subject);
	// the files whose nzb stops short of the parts their subject declares
	const std::set<FileInfo*>& GetCutShortFiles() const { return m_cutShortFiles; }

private:
	std::unique_ptr<NzbInfo> m_nzbInfo;
	std::string m_fileName;
	std::string m_category;
	std::string m_password;
	std::string m_metaName;
	std::string m_metaTitle;
	std::set<FileInfo*> m_cutShortFiles;
	struct NzbgetRsNzbFileParser* m_parser = nullptr;

	static void SAX_StartElement(NzbFile* file, const char *name, const char **atts);
	static void SAX_EndElement(NzbFile* file, const char *name);
	static void SAX_characters(NzbFile* file, const char *  xmlstr, int len);
	static void* SAX_getEntity(NzbFile* file, const char *  name);
	static void SAX_error(NzbFile* file, const char *msg, ...);
};

#endif
