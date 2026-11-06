/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2015-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "DupeMatcher.h"
#include "Log.h"
#include "Options.h"
#include "nzbget_rs.h"

bool DupeMatcher::Prepare()
{
	int comp = m_compressed ? 1 : 0;
	bool sizeOK = nzbget_rs_dupematcher_prepare(
		m_destDir,
		g_Options ? g_Options->GetUnrarCmd() : "",
		m_expectedSize,
		&m_maxSize,
		&comp,
		[](void* rawCtx, const char* msg) {
			static_cast<DupeMatcher*>(rawCtx)->PrintMessage(
				Message::mkDetail, "%s", msg ? msg : "");
		},
		this) != 0;
	m_compressed = comp != 0;
	return sizeOK;
}

bool DupeMatcher::MatchDupeContent(const char* dupeDir)
{
	return nzbget_rs_dupematcher_match_dupe_content(
		dupeDir,
		g_Options ? g_Options->GetUnrarCmd() : "",
		m_maxSize,
		m_compressed ? 1 : 0,
		[](void* rawCtx, const char* msg) {
			static_cast<DupeMatcher*>(rawCtx)->PrintMessage(
				Message::mkDetail, "%s", msg ? msg : "");
		},
		this) != 0;
}
