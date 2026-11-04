/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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
#include "FileSystem.h"
#include "ParParser.h"
#include "nzbget_rs.h"

bool ParParser::FindMainPars(const char* path, ParFileList* fileList)
{
	if (fileList)
	{
		fileList->clear();
	}

	DirBrowser dir(path);
	while (const char* filename = dir.Next())
	{
		int baseLen = 0;
		if (ParseParFilename(filename, true, &baseLen, nullptr))
		{
			if (!fileList)
			{
				return true;
			}

			// check if the base file already added to list
			bool exists = false;
			for (std::string& filename2 : *fileList)
			{
				exists = SameParCollection(filename, filename2.c_str(), true);
				if (exists)
				{
					break;
				}
			}
			if (!exists)
			{
				fileList->emplace_back(filename);
			}
		}
	}
	return fileList && !fileList->empty();
}

int ParParser::FilenameFold(int byte)
{
	// Use the C++ compiler's char signedness, including -funsigned-char.
	int ch = static_cast<char>(byte);
#ifdef __GLIBC__
	// glibc defines tolower for negative signed chars (EOF stays distinct).
	return tolower(ch);
#else
	// Darwin and musl leave negative chars unchanged. Keep them outside
	// the standard ctype API's unsigned-char/EOF domain.
	return ch < 0 ? ch : tolower(ch);
#endif
}

namespace
{
	// glibc's own tolower table: Rust folds by it without a call per byte
	const int* ParFilenameTable()
	{
#ifdef __GLIBC__
		return reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
		return nullptr;
#endif
	}
}

bool ParParser::SameParCollection(const char* filename1, const char* filename2, bool confirmedFilenames)
{
	// rust/src/parparser.rs
	return nzbget_rs_same_par_collection(filename1, filename2, confirmedFilenames, ParFilenameTable(), CHAR_MIN < 0,
		FilenameFold) != 0;
}

bool ParParser::ParseParFilename(const char* parFilename, bool confirmedFilename, int* baseNameLen, int* blocks)
{
	// rust/src/parparser.rs
	return nzbget_rs_parse_par_filename(parFilename, confirmedFilename, baseNameLen, blocks, ParFilenameTable(), CHAR_MIN < 0,
		FilenameFold) != 0;
}
