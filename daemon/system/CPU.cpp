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
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace System
{
	static const int BUFFER_SIZE = 256;

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

	std::string CPU::GetCanonicalCPUArch(const std::string& arch) const
	{
		const char* canonical = nzbget_rs_snh_cpu_canonical_arch(arch.c_str(), arch.size());
		return canonical ? std::string(canonical) : arch;
	}

#ifdef __linux__
#include <fstream>
	void CPU::Init()
	{
		auto result = GetCPUArch();
		if (result.has_value())
		{
			m_arch = std::move(result.value());
		}
		else
		{
			debug("Failed to get CPU arch from uname(2)");
		}

		result = GetCPUModelFromCPUInfo();
		if (result.has_value())
		{
			m_model = std::move(result.value());
		}
		else
		{
			debug("Failed to get CPU model from /proc/cpuinfo");
		}
	}

	std::optional<std::string> CPU::GetCPUModelFromCPUInfo() const
	{
		std::ifstream cpuinfo("/proc/cpuinfo");
		if (!cpuinfo.is_open())
		{
			return std::nullopt;
		}

		std::string line;
		while (std::getline(cpuinfo, line))
		{
			size_t valOffset = 0;
			if (nzbget_rs_snh_cpu_parse_cpuinfo_line(line.c_str(), line.size(), &valOffset))
			{
				std::string model = line.substr(valOffset);
				Util::Trim(model);
				if (!model.empty())
				{
					return model;
				}
			}
		}

		return std::nullopt;
	}
#endif

#if __BSD__
	void CPU::Init()
	{
		int mib[2];
		size_t len = BUFFER_SIZE;
		char model[BUFFER_SIZE];

		mib[0] = CTL_HW;
		mib[1] = HW_MODEL;
		if (sysctl(mib, 2, model, &len, nullptr, 0) != -1)
		{
			m_model = model;
			Util::Trim(m_model);
		}
		else
		{
			debug("Failed to get CPU model from sysctl(2): HW_MODEL");
		}

		auto result = GetCPUArch();
		if (result.has_value())
		{
			m_arch = std::move(result.value());
		}
	}
#endif

#ifdef __APPLE__
	void CPU::Init()
	{
		size_t len = BUFFER_SIZE;
		char buffer[BUFFER_SIZE];
		if (sysctlbyname("machdep.cpu.brand_string", &buffer, &len, nullptr, 0) == 0)
		{
			m_model = buffer;
			Util::Trim(m_model);
		}
		else
		{
			debug("Failed to get CPU model from sysctlbyname: machdep.cpu.brand_string");
		}

		auto result = GetCPUArch();
		if (result.has_value())
		{
			m_arch = std::move(result.value());
		}
	}
#endif

	std::optional<std::string> CPU::GetCPUArch() const
	{
		struct utsname uts;
		if (uname(&uts) == 0 && uts.machine[0] != '\0')
		{
			return GetCanonicalCPUArch(uts.machine);
		}

		return std::nullopt;
	}
}
