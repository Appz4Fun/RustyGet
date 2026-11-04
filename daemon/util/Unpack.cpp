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

#include "Options.h"
#include "Unpack.h"
#include "Log.h"
#include "nzbget_rs.h"

namespace Unpack
{
Extractor::~Extractor() = default;

ExtractorBase::ExtractorBase(fs::path tool, fs::path archive,
							 fs::path outputDir, std::string password,
							 OverwriteMode mode)
	: m_tool(std::move(tool)),
	  m_archive(std::move(archive)),
	  m_outputDir(std::move(outputDir)),
	  m_password(std::move(password)),
	  m_mode(mode)
{
}

bool ExtractorBase::Extract()
{
	if (!CheckPrerequisites()) return false;

	auto args = MakeArgs();
	ScriptController executor;
	executor.SetArgs(std::move(args));

	int exitCode = executor.Execute();
	if (executor.IsTerminated())
	{
		error("Unpack: Extraction process for '%s' was interrupted", fs::u8string(m_archive).c_str());
		return false;
	}

	return DecodeExitCode(exitCode);
}

bool ExtractorBase::CheckPrerequisites() const
{
	fs::error_code ec;
	fs::create_directories(m_outputDir, ec);
	if (ec)
	{
		error("Unpack: Failed to create the output directory '%s': %s", 
			  fs::u8string(m_outputDir).c_str(), ec.message().c_str());
		return false;
	}

	return true;
}

std::string ExtractorBase::MakePassword() const
{
	char buf[512];
	size_t len = nzbget_rs_awc_unpack_make_password_arg(
		m_password.data(), m_password.size(), buf, sizeof(buf));
	if (len < sizeof(buf))
	{
		return std::string(buf, len);
	}
	std::string out(len, '\0');
	nzbget_rs_awc_unpack_make_password_arg(
		m_password.data(), m_password.size(), out.data(), len + 1);
	return out;
}

bool IsArchive(const fs::path& file)
{
	return SevenZip::IsSupported(file) || Unrar::IsSupported(file);
}

ExtractorPtr MakeExtractor(fs::path archive, fs::path outputDir,
						   std::string password, OverwriteMode mode)
{
	const std::string archiveName = fs::u8string(archive.filename());

	auto validateTool = [&archiveName](const fs::path& toolPath, std::string_view optionName) -> std::optional<fs::path>
	{
		fs::error_code ec;
		bool exists = !toolPath.empty() && fs::exists(toolPath, ec);
		int status = nzbget_rs_awc_unpack_validate_tool_status(
			toolPath.empty() ? 1 : 0, ec ? 1 : 0, exists ? 1 : 0);
		if (status == 1)
		{
			error("Unpack: Could not process '%s': %s is not configured", archiveName.c_str(), std::string(optionName).c_str());
			return std::nullopt;
		}
		if (status == 2)
		{
			error("Unpack: Could not process '%s': Failed to access '%s': %s", archiveName.c_str(), fs::u8string(toolPath).c_str(), ec.message().c_str());
			return std::nullopt;
		}
		if (status == 3)
		{
			error("Unpack: Could not process '%s': %s doesn't exist", archiveName.c_str(), fs::u8string(toolPath).c_str());
			return std::nullopt;
		}
		return toolPath;
	};

	if (SevenZip::IsSupported(archive))
	{
		const auto tool = validateTool(g_Options->GetSevenZipPath(), Options::SEVENZIPCMD);
		if (!tool) return nullptr;
		return std::make_unique<SevenZip>(*tool, std::move(archive), std::move(outputDir),
										  std::move(password), mode);
	}

	if (Unrar::IsSupported(archive))
	{
		const auto tool = validateTool(g_Options->GetUnrarPath(), Options::UNRARCMD);
		if (!tool) return nullptr;
		return std::make_unique<Unrar>(*tool, std::move(archive), std::move(outputDir),
									   std::move(password), mode);
	}

	error("Unpack: Could not process '%s': Unsupported archive format: %s", archiveName.c_str(), fs::u8string(archive.extension()).c_str());
	return nullptr;
}
}  // namespace Unpack
