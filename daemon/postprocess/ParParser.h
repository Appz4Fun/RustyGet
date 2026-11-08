/*
 *  This file is part of RustyGet.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef PARPARSER_H
#define PARPARSER_H

#include <string>
#include "Container.h"
#include "nzbget_rs.h"

class ParParser
{
public:
	typedef std::vector<std::string> ParFileList;

	static bool FindMainPars(const char* path, ParFileList* fileList)
	{
		if (fileList)
		{
			fileList->clear();
		}
		return nzbget_rs_find_main_pars(
			path,
			fileList != nullptr ? 1 : 0,
			fileList ? +[](void* ctx, const char* name, size_t len)
			{
				static_cast<ParFileList*>(ctx)->emplace_back(name, len);
			} : nullptr,
			fileList) != 0;
	}

	static bool ParseParFilename(const char* parFilename, bool confirmedFilename, int* baseNameLen, int* blocks)
	{
		return nzbget_rs_parse_par_filename(parFilename, confirmedFilename, baseNameLen, blocks) != 0;
	}

	static bool SameParCollection(const char* filename1, const char* filename2, bool confirmedFilenames)
	{
		return nzbget_rs_same_par_collection(filename1, filename2, confirmedFilenames) != 0;
	}

	static constexpr int64 MaxIndexParSize = 16 * 1024 * 1024;

	static bool ListParredFiles(const char* parFilename, std::vector<std::string>& filenames)
	{
		return nzbget_rs_par_renamer_list_parred_files(
			parFilename,
			MaxIndexParSize,
			&filenames,
			+[](void* rawCtx, const char* name) noexcept
			{
				if (name)
				{
					static_cast<std::vector<std::string>*>(rawCtx)->emplace_back(name);
				}
			}) != 0;
	}
};

#endif
