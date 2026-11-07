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

#include <regex>

#include "SystemInfo.h"
#include "Options.h"
#include "FileSystem.h"
#include "Log.h"
#include "Json.h"
#include "Xml.h"
#include "nzbget_rs.h"

namespace System
{
	constexpr size_t BUFFER_SIZE = 512;

	SystemInfo::SystemInfo(std::function<Network()> networkFetcher)
		: m_networkFetcher{std::move(networkFetcher)}
	{
		InitLibsInfo();
	}

	const std::vector<Library>& SystemInfo::GetLibraries() const
	{
		return m_libraries;
	}

	void SystemInfo::InitLibsInfo()
	{
		m_libraries.reserve(11);
		m_libraries.push_back({ "zipfast", "0.1.0" });
		m_libraries.push_back({ "zipview", "0.1.0" });
		m_libraries.push_back({ "par-rs", "0.2.0" });
		m_libraries.push_back({ "nntmux-yenc", "0.1.0 (SIMD)" });
		m_libraries.push_back({ "flate2 (zlib-rs)", "1.1.11" });
		m_libraries.push_back({ "zstd", "0.14.0" });
		m_libraries.push_back({ "bzip2", "0.6.1" });
		m_libraries.push_back({ "crc32fast", "1.5.2" });
		m_libraries.push_back({ "quick-xml", "0.41.0" });
		m_libraries.push_back({ "md-5", "0.11.0" });

#ifndef DISABLE_TLS
#ifdef LIBRESSL_VERSION_TEXT
		NzbgetRsBuf verStrBuf = nzbget_rs_dqh_sysinfo_extract_version_after_space(LIBRESSL_VERSION_TEXT);
		m_libraries.push_back({ "LibreSSL", std::string(verStrBuf.data ? verStrBuf.data : "", verStrBuf.len) });
		nzbget_rs_free(verStrBuf);
#elif defined(OPENSSL_FULL_VERSION_STR)
		m_libraries.push_back({ "OpenSSL", OPENSSL_FULL_VERSION_STR });
#else 
		NzbgetRsBuf verStrBuf = nzbget_rs_dqh_sysinfo_extract_version_after_space(OPENSSL_VERSION_TEXT);
		m_libraries.push_back({ "OpenSSL", std::string(verStrBuf.data ? verStrBuf.data : "", verStrBuf.len) });
		nzbget_rs_free(verStrBuf);
#endif
#endif

		m_libraries.shrink_to_fit();
	}

	const CPU& SystemInfo::GetCPUInfo() const
	{
		return m_cpu;
	}

	const OS& SystemInfo::GetOSInfo() const
	{
		return m_os;
	}

	Network SystemInfo::GetNetworkInfo() const
	{
		return m_networkFetcher();
	}

	std::vector<Tool> SystemInfo::GetTools() const
	{
		std::vector<Tool> tools{
			{ "UnRAR / 7-Zip / Archive Engine", "Built-in", "zipfast + zipview + native Rust decoders" },
			{ "Parity Verification & Repair", "Built-in", "par-rs (streaming + post-download)" },
			{ "Extended Unpacker & Permission Fixer", "Built-in", "First-class native pipeline" },
			{ "yEnc SIMD Decoder", "Built-in", "nntmux-yenc" },
		};

		return tools;
	}

	Tool SystemInfo::GetPython() const
	{
		Tool tool;

		tool.name = "Python";

		auto result = Util::FindShellOverriddenExecutor(".py", g_Options->GetShellOverride());
		if (result.has_value())
		{
			if (FileSystem::FileExists(result.value().c_str()))
			{
				tool.path = std::move(result.value());
				Util::Trim(tool.path);
			}
			else
			{
				detail("Failed to find Python: '%s' doesn't exist", result.value().c_str());
				return tool;
			}
		}
		else
		{
			result = FindPython();
			if (!result.has_value())
			{
				detail("Failed to find Python");
				return tool;
			}

			tool.path = std::move(result.value());
			Util::Trim(tool.path);
		}

		result = GetPythonVersion(tool.path);
		if (!result.has_value())
		{
			detail("Failed to get Python version");
			return tool;
		}

		tool.version = std::move(result.value());

		return tool;
	}

	Tool SystemInfo::GetUnrar() const
	{
		Tool tool;

		tool.name = "UnRAR";
		tool.path = GetToolPath(g_Options->GetUnrarCmd());
		tool.version = GetUnpackerVersion(tool.path, "UNRAR");

		return tool;
	}

	Tool SystemInfo::GetSevenZip() const
	{
		Tool tool;

		tool.name = "7-Zip";
		tool.path = GetToolPath(g_Options->GetSevenZipCmd());
		tool.version = GetUnpackerVersion(tool.path, tool.name.c_str());

		return tool;
	}

	std::string SystemInfo::GetToolPath(const char* cmd) const
	{
		if (Util::EmptyStr(cmd))
		{
			return "";
		}

		std::string path = FileSystem::ExtractFilePathFromCmd(cmd);

		auto result = FileSystem::GetRealPath(path);

		if (result.has_value() && FileSystem::FileExists(result.value().c_str()))
		{
			return result.value();
		}

		std::string fullCmd = Util::FIND_CMD + std::string(cmd);

		auto pipe = Util::MakePipe(fullCmd);
		if (!pipe)
		{
			return "";
		}

		char buffer[BUFFER_SIZE];
		if (fgets(buffer, BUFFER_SIZE, pipe.get()))
		{
			std::string resultPath{ buffer };
			Util::Trim(resultPath);
			return resultPath;
		}

		return "";
	}

	std::string SystemInfo::GetUnpackerVersion(const std::string& path, const char* marker) const
	{
		if (path.empty())
		{
			return "";
		}

		std::string cmd = FileSystem::EscapePathForShell(path);

		auto pipe = Util::MakePipe(cmd);
		if (!pipe)
		{
			return "";
		}

		std::string version;
		char buffer[BUFFER_SIZE];
		while (!feof(pipe.get()))
		{
			if (fgets(buffer, BUFFER_SIZE, pipe.get()) != nullptr)
			{
				if (nzbget_rs_dqh_sysinfo_line_contains_marker(buffer, marker) != 0)
				{
					version = ParseUnpackerVersion(buffer);
					break;
				}
			}
		}

		return version;
	}

	std::optional<std::string> SystemInfo::FindPython() const
	{
		auto result = Util::FindPython();
		if (!result.has_value())
		{
			return std::nullopt;
		}

		std::string cmd = Util::FIND_CMD + result.value();

		auto pipe = Util::MakePipe(cmd);
		if (!pipe)
		{
			return std::nullopt;
		}

		char buffer[BUFFER_SIZE];
		if (fgets(buffer, BUFFER_SIZE, pipe.get()))
		{
			std::string path{ buffer };
			Util::Trim(path);
			return path;
		}

		return std::nullopt;
	}

	std::optional<std::string> SystemInfo::GetPythonVersion(const std::string& path) const
	{
		std::string cmd = FileSystem::EscapePathForShell(path) + " --version 2>&1";
		{
			auto pipe = Util::MakePipe(cmd);
			if (!pipe)
			{
				return "";
			}

			char buffer[BUFFER_SIZE];
			if (fgets(buffer, BUFFER_SIZE, pipe.get()))
			{
				// e.g. Python 3.12.3
				std::string version{ buffer };
				Util::Trim(version);
				size_t start = 0;
				size_t len = 0;
				if (nzbget_rs_snh_sys_extract_python_version(version.c_str(), version.size(), &start, &len))
				{
					return version.substr(start, len);
				}
			}
		}

		return std::nullopt;
	}

	std::string SystemInfo::ParseUnpackerVersion(const std::string& line) const
	{
		// e.g. 7-Zip (a) 19.00 (x64) : Copyright (c) 1999-2018 Igor Pavlov : 2019-02-21
		// e.g. UNRAR 5.70 x64 freeware      Copyright (c) 1993-2019 Alexander Roshal
		size_t start = 0;
		size_t len = 0;
		if (nzbget_rs_snh_sys_parse_unpacker_version(line.c_str(), line.size(), &start, &len))
		{
			return line.substr(start, len);
		}
		return "";
	}

	std::string SystemInfo::ParseParVersion(const std::string& line) const
	{
		// e.g. par2cmdline-turbo version 1.1.1
		size_t pos = 0;
		if (nzbget_rs_snh_sys_parse_par_version(line.c_str(), line.size(), &pos))
		{
			return line.substr(pos);
		}
		return "";
	}

	std::ostream& operator<<(std::ostream& os, const SystemInfo& sysinfo)
	{
		const auto& osInfo = sysinfo.GetOSInfo();
		const auto& cpu = sysinfo.GetCPUInfo();
		const auto tools = sysinfo.GetTools();
		const auto& libraries = sysinfo.GetLibraries();

		std::vector<const char*> tNames(tools.size()), tVers(tools.size()), tPaths(tools.size());
		std::vector<size_t> tNameLens(tools.size()), tVerLens(tools.size()), tPathLens(tools.size());
		for (size_t i = 0; i < tools.size(); ++i)
		{
			tNames[i] = tools[i].name.data();
			tNameLens[i] = tools[i].name.size();
			tVers[i] = tools[i].version.data();
			tVerLens[i] = tools[i].version.size();
			tPaths[i] = tools[i].path.data();
			tPathLens[i] = tools[i].path.size();
		}

		std::vector<const char*> lNames(libraries.size()), lVers(libraries.size());
		std::vector<size_t> lNameLens(libraries.size()), lVerLens(libraries.size());
		for (size_t i = 0; i < libraries.size(); ++i)
		{
			lNames[i] = libraries[i].name.data();
			lNameLens[i] = libraries[i].name.size();
			lVers[i] = libraries[i].version.data();
			lVerLens[i] = libraries[i].version.size();
		}

		NzbgetRsBuf buf = nzbget_rs_system_info_to_display_text(
			osInfo.GetName().data(), osInfo.GetName().size(),
			osInfo.GetVersion().data(), osInfo.GetVersion().size(),
			cpu.GetModel().data(), cpu.GetModel().size(),
			cpu.GetArch().data(), cpu.GetArch().size(),
			tNames.empty() ? nullptr : tNames.data(),
			tNameLens.empty() ? nullptr : tNameLens.data(),
			tVers.empty() ? nullptr : tVers.data(),
			tVerLens.empty() ? nullptr : tVerLens.data(),
			tPaths.empty() ? nullptr : tPaths.data(),
			tPathLens.empty() ? nullptr : tPathLens.data(),
			tools.size(),
			lNames.empty() ? nullptr : lNames.data(),
			lNameLens.empty() ? nullptr : lNameLens.data(),
			lVers.empty() ? nullptr : lVers.data(),
			lVerLens.empty() ? nullptr : lVerLens.data(),
			libraries.size());
		if (buf.data && buf.len > 0)
		{
			os.write(buf.data, static_cast<std::streamsize>(buf.len));
		}
		nzbget_rs_free(buf);
		return os;
	}

	std::string ToJsonStr(const SystemInfo& sysInfo)
	{
		const auto& os = sysInfo.GetOSInfo();
		const auto network = sysInfo.GetNetworkInfo();
		const auto& cpu = sysInfo.GetCPUInfo();
		const auto tools = sysInfo.GetTools();
		const auto& libraries = sysInfo.GetLibraries();

		std::vector<const char*> tNames(tools.size()), tVers(tools.size()), tPaths(tools.size());
		std::vector<size_t> tNameLens(tools.size()), tVerLens(tools.size()), tPathLens(tools.size());
		for (size_t i = 0; i < tools.size(); ++i)
		{
			tNames[i] = tools[i].name.data();
			tNameLens[i] = tools[i].name.size();
			tVers[i] = tools[i].version.data();
			tVerLens[i] = tools[i].version.size();
			tPaths[i] = tools[i].path.data();
			tPathLens[i] = tools[i].path.size();
		}

		std::vector<const char*> lNames(libraries.size()), lVers(libraries.size());
		std::vector<size_t> lNameLens(libraries.size()), lVerLens(libraries.size());
		for (size_t i = 0; i < libraries.size(); ++i)
		{
			lNames[i] = libraries[i].name.data();
			lNameLens[i] = libraries[i].name.size();
			lVers[i] = libraries[i].version.data();
			lVerLens[i] = libraries[i].version.size();
		}

		NzbgetRsBuf buf = nzbget_rs_system_info_to_json(
			os.GetName().data(), os.GetName().size(),
			os.GetVersion().data(), os.GetVersion().size(),
			cpu.GetModel().data(), cpu.GetModel().size(),
			cpu.GetArch().data(), cpu.GetArch().size(),
			network.publicIP.data(), network.publicIP.size(),
			network.privateIP.data(), network.privateIP.size(),
			tNames.empty() ? nullptr : tNames.data(),
			tNameLens.empty() ? nullptr : tNameLens.data(),
			tVers.empty() ? nullptr : tVers.data(),
			tVerLens.empty() ? nullptr : tVerLens.data(),
			tPaths.empty() ? nullptr : tPaths.data(),
			tPathLens.empty() ? nullptr : tPathLens.data(),
			tools.size(),
			lNames.empty() ? nullptr : lNames.data(),
			lNameLens.empty() ? nullptr : lNameLens.data(),
			lVers.empty() ? nullptr : lVers.data(),
			lVerLens.empty() ? nullptr : lVerLens.data(),
			libraries.size());
		std::string result(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return result;
	}

	std::string ToXmlStr(const SystemInfo& sysInfo)
	{
		const auto& os = sysInfo.GetOSInfo();
		const auto network = sysInfo.GetNetworkInfo();
		const auto& cpu = sysInfo.GetCPUInfo();
		const auto tools = sysInfo.GetTools();
		const auto& libraries = sysInfo.GetLibraries();

		std::vector<const char*> tNames(tools.size()), tVers(tools.size()), tPaths(tools.size());
		std::vector<size_t> tNameLens(tools.size()), tVerLens(tools.size()), tPathLens(tools.size());
		for (size_t i = 0; i < tools.size(); ++i)
		{
			tNames[i] = tools[i].name.data();
			tNameLens[i] = tools[i].name.size();
			tVers[i] = tools[i].version.data();
			tVerLens[i] = tools[i].version.size();
			tPaths[i] = tools[i].path.data();
			tPathLens[i] = tools[i].path.size();
		}

		std::vector<const char*> lNames(libraries.size()), lVers(libraries.size());
		std::vector<size_t> lNameLens(libraries.size()), lVerLens(libraries.size());
		for (size_t i = 0; i < libraries.size(); ++i)
		{
			lNames[i] = libraries[i].name.data();
			lNameLens[i] = libraries[i].name.size();
			lVers[i] = libraries[i].version.data();
			lVerLens[i] = libraries[i].version.size();
		}

		NzbgetRsBuf buf = nzbget_rs_system_info_to_xml(
			os.GetName().data(), os.GetName().size(),
			os.GetVersion().data(), os.GetVersion().size(),
			cpu.GetModel().data(), cpu.GetModel().size(),
			cpu.GetArch().data(), cpu.GetArch().size(),
			network.publicIP.data(), network.publicIP.size(),
			network.privateIP.data(), network.privateIP.size(),
			tNames.empty() ? nullptr : tNames.data(),
			tNameLens.empty() ? nullptr : tNameLens.data(),
			tVers.empty() ? nullptr : tVers.data(),
			tVerLens.empty() ? nullptr : tVerLens.data(),
			tPaths.empty() ? nullptr : tPaths.data(),
			tPathLens.empty() ? nullptr : tPathLens.data(),
			tools.size(),
			lNames.empty() ? nullptr : lNames.data(),
			lNameLens.empty() ? nullptr : lNameLens.data(),
			lVers.empty() ? nullptr : lVers.data(),
			lVerLens.empty() ? nullptr : lVerLens.data(),
			libraries.size());
		std::string result(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return result;
	}
}
