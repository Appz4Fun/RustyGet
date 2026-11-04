/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
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

#include <algorithm>
#include <regex>

#include "Unpack.h"
#include "FileTypes.h"
#include "Util.h"
#include "nzbget_rs.h"

using namespace Unpack;

/**
 * <Commands>
 * 		x : Extract files with full path
 * <Switches>
 *		-y : Assume Yes on all queries
 *		-ai : Ignore file attributes
 *		-o{+|-|r} : Set the overwrite mode
 *		-p{Password} : set Password
 */
ScriptController::ArgList Unrar::MakeArgs() const
{
	ScriptController::ArgList args;

	args.push_back(fs::u8string(m_tool).c_str());
	args.push_back("x");
	args.push_back("-y");
	args.push_back("-ai");

	args.push_back(nzbget_rs_euh_unrar_overwrite_switch(static_cast<int>(m_mode)));

	args.push_back(MakePassword().c_str());
	args.push_back(fs::u8string(m_archive).c_str());
	args.push_back(fs::u8string(m_outputDir).c_str());

	return args;
}

bool Unrar::IsSupported(const fs::path& path)
{
	if (!path.has_filename()) return false;

	auto filename = fs::u8string(path.filename());
	auto ext = fs::u8string(path.extension());

	if (!FileTypes::IsRarExt(ext))
	{
		return false;
	}

	return nzbget_rs_euh_unrar_is_part1_or_unsplit_rar(filename.data(), filename.size()) != 0;
}

bool Unrar::DecodeExitCode(int ec) const
{
	const char* msg = nullptr;
	if (nzbget_rs_euh_unrar_decode_exit_code(ec, &msg) != 0)
	{
		return true;
	}
	if (msg && *msg)
	{
		error("%s", msg);
	}
	else
	{
		error("Unknown error %d", ec);
	}
	return false;
}
