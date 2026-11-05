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
#include "ParParser.h"
#include "nzbget_rs.h"

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

	void AddParFile(void* ctx, const char* name, size_t len)
	{
		static_cast<ParParser::ParFileList*>(ctx)->emplace_back(name, len);
	}
}

bool ParParser::FindMainPars(const char* path, ParFileList* fileList)
{
	if (fileList)
	{
		fileList->clear();
	}
	// rust/src/parparser.rs
	return nzbget_rs_find_main_pars(
		path,
		fileList != nullptr ? 1 : 0,
		ParFilenameTable(),
		CHAR_MIN < 0,
		FilenameFold,
		fileList ? AddParFile : nullptr,
		fileList) != 0;
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
