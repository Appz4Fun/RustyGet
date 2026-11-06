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

#include "CPU.h"
#include "nzbget_rs.h"

namespace System
{
	CPU::CPU()
	{
		Init();
	}

	const std::string& CPU::GetModel() const
	{
		return m_model;
	}

	const std::string& CPU::GetArch() const
	{
		return m_arch;
	}

	void CPU::Init()
	{
		struct utsname uts{};
		const char* rawMachine = "";
		if (uname(&uts) == 0 && uts.machine[0] != '\0')
		{
			rawMachine = uts.machine;
		}
		NzbgetRsBuf modelBuf{};
		NzbgetRsBuf archBuf{};
		nzbget_rs_cpu_detect_info(rawMachine, strlen(rawMachine), &modelBuf, &archBuf);
		m_model.assign(modelBuf.data ? modelBuf.data : "", modelBuf.len);
		m_arch.assign(archBuf.data ? archBuf.data : "", archBuf.len);
		nzbget_rs_free(modelBuf);
		nzbget_rs_free(archBuf);
	}
}
