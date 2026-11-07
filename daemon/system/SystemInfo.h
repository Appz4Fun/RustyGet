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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

#ifndef SYSTEM_INFO_H
#define SYSTEM_INFO_H

#include <functional>
#include <ostream>
#include <string>
#include <vector>
#include "OS.h"
#include "CPU.h"
#include "Network.h"
#include "OpenSSL.h"
#include "nzbget_rs.h"

namespace System
{
	struct Library
	{
		std::string name;
		std::string version;
	};

	struct Tool
	{
		std::string name;
		std::string version;
		std::string path;
	};

	class SystemInfo final
	{
	public:
		SystemInfo(std::function<Network()> networkFetcher = GetNetwork)
			: m_networkFetcher{std::move(networkFetcher)}
		{
			InitLibsInfo();
		}

		~SystemInfo() = default;

		std::vector<Tool> GetTools() const
		{
			return {
				{ "UnRAR / 7-Zip / Archive Engine", "Built-in", "zipfast + zipview + native Rust decoders" },
				{ "Parity Verification & Repair", "Built-in", "par-rs (streaming + post-download)" },
				{ "Extended Unpacker & Permission Fixer", "Built-in", "First-class native pipeline" },
				{ "yEnc SIMD Decoder", "Built-in", "nntmux-yenc" },
			};
		}

		Network GetNetworkInfo() const
		{
			return m_networkFetcher();
		}

		const std::vector<Library>& GetLibraries() const
		{
			return m_libraries;
		}

		const CPU& GetCPUInfo() const
		{
			return m_cpu;
		}

		const OS& GetOSInfo() const
		{
			return m_os;
		}

		friend std::ostream& operator<<(std::ostream& os, const SystemInfo& sysinfo)
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

	private:
		void InitLibsInfo()
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

		CPU m_cpu;
		OS m_os;
		std::vector<Library> m_libraries;
		std::function<Network()> m_networkFetcher;
	};

	inline std::string ToJsonStr(const SystemInfo& sysInfo)
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

	inline std::string ToXmlStr(const SystemInfo& sysInfo)
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

extern System::SystemInfo* g_SystemInfo;

#endif
