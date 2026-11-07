/*
 *  This file is part of RustyGet.
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


#ifndef DUPEMATCHER_H
#define DUPEMATCHER_H

#include "NString.h"
#include "Log.h"
#include "Options.h"
#include "nzbget_rs.h"

class DupeMatcher
{
public:
	DupeMatcher(const char* destDir, int64 expectedSize) :
		m_destDir(destDir), m_expectedSize(expectedSize) {}

	bool Prepare()
	{
		int comp = m_compressed ? 1 : 0;
		bool sizeOK = nzbget_rs_dupematcher_prepare(
			m_destDir,
			g_Options ? g_Options->GetUnrarCmd() : "",
			m_expectedSize,
			&m_maxSize,
			&comp,
			+[](void* rawCtx, const char* msg) {
				static_cast<DupeMatcher*>(rawCtx)->PrintMessage(
					Message::mkDetail, "%s", msg ? msg : "");
			},
			this) != 0;
		m_compressed = comp != 0;
		return sizeOK;
	}

	bool MatchDupeContent(const char* dupeDir)
	{
		return nzbget_rs_dupematcher_match_dupe_content(
			dupeDir,
			g_Options ? g_Options->GetUnrarCmd() : "",
			m_maxSize,
			m_compressed ? 1 : 0,
			+[](void* rawCtx, const char* msg) {
				static_cast<DupeMatcher*>(rawCtx)->PrintMessage(
					Message::mkDetail, "%s", msg ? msg : "");
			},
			this) != 0;
	}

protected:
	virtual void PrintMessage([[maybe_unused]] Message::EKind kind,
		[[maybe_unused]] const char* format, ...) PRINTF_SYNTAX(3) {}

private:
	CString m_destDir;
	int64 m_expectedSize;
	int64 m_maxSize = -1;
	bool m_compressed = false;
};

#endif
