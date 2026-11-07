/*
 *  This file is part of RustyGet.
 *
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef DEOBFUSCATION_H
#define DEOBFUSCATION_H

#include <memory>
#include <string>
#include <string_view>
#include "nzbget_rs.h"

namespace Deobfuscation
{
	inline bool IsExcessivelyObfuscated(std::string_view str)
	{
		return nzbget_rs_is_excessively_obfuscated(str.data(), str.size()) != 0;
	}

	inline std::string Deobfuscate(std::string_view str)
	{
		NzbgetRsBuf buf = nzbget_rs_deobfuscate(str.data(), str.size());
		auto release = [](NzbgetRsBuf* owned) { nzbget_rs_free(*owned); };
		std::unique_ptr<NzbgetRsBuf, decltype(release)> owner(&buf, release);
		return std::string(buf.data, buf.len);
	}
}

#endif
