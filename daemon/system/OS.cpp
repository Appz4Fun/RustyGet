/*
 *  This file is part of nzbget. See <https://nzbget.com>.
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include <sys/utsname.h>

#include "OS.h"
#include "nzbget_rs.h"

namespace System
{
	OS::OS()
	{
		Init();
	}

	const std::string& OS::GetName() const
	{
		return m_name;
	}

	const std::string& OS::GetVersion() const
	{
		return m_version;
	}

	void OS::Init()
	{
		struct utsname uts{};
		const char* sysname = "";
		const char* release = "";
		if (uname(&uts) == 0)
		{
			sysname = uts.sysname;
			release = uts.release;
		}
		NzbgetRsBuf nameBuf{};
		NzbgetRsBuf verBuf{};
		nzbget_rs_os_detect_info(
			sysname, strlen(sysname),
			release, strlen(release),
			&nameBuf, &verBuf);
		m_name.assign(nameBuf.data ? nameBuf.data : "", nameBuf.len);
		m_version.assign(verBuf.data ? verBuf.data : "", verBuf.len);
		nzbget_rs_free(nameBuf);
		nzbget_rs_free(verBuf);
	}
}
