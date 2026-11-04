/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2023 Denis <denis@nzbget.com>
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

#ifndef EXTENSION_LOADER_H
#define EXTENSION_LOADER_H

#include <vector>
#include "Extension.h"
#include "Util.h"

namespace ExtensionLoader
{
	namespace V1
	{
		bool Load(Extension::Script& script, const char* location, const char* rootDir);
		void BuildDisplayName(Extension::Script& script);
	}

	namespace V2
	{
		bool Load(Extension::Script& script, const char* location, const char* rootDir);
	}

	Extension::Kind GetScriptKind(const std::string& line);
}

#endif
