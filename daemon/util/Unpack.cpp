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
	NzbgetRsBuf buf = nzbget_rs_euh_extractor_make_password(m_password.data(), m_password.size());
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

ScriptController::ArgList SevenZip::MakeArgs() const
{
	ScriptController::ArgList args;

	args.push_back(fs::u8string(m_tool).c_str());
	args.push_back("x");
	args.push_back("-y");

	args.push_back(nzbget_rs_euh_sevenzip_overwrite_switch(static_cast<int>(m_mode)));

	args.push_back(MakePassword().c_str());
	args.push_back(("-o" + fs::u8string(m_outputDir)).c_str());
	args.push_back(fs::u8string(m_archive).c_str());

	return args;
}

bool SevenZip::IsSupported(const fs::path& path)
{
	auto s = fs::u8string(path);
	return nzbget_rs_euh_sevenzip_is_supported(s.data(), s.size()) != 0;
}

bool SevenZip::DecodeExitCode(int ec) const
{
	const char* msg = nullptr;
	if (nzbget_rs_euh_sevenzip_decode_exit_code(ec, &msg) != 0)
	{
		return true;
	}
	if (msg && *msg)
	{
		error("%s", msg);
	}
	else
	{
		error("Unknown 7-Zip error (%d)", ec);
	}
	return false;
}

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
	auto s = fs::u8string(path);
	return nzbget_rs_euh_unrar_is_supported(s.data(), s.size()) != 0;
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

ZipFastExtractor::ZipFastExtractor(fs::path archive, fs::path outputDir,
								   std::string password, OverwriteMode mode)
	: m_archive(std::move(archive)),
	  m_outputDir(std::move(outputDir)),
	  m_password(std::move(password)),
	  m_mode(mode)
{
}

bool ZipFastExtractor::IsSupported(const fs::path& path)
{
	auto s = fs::u8string(path);
	return nzbget_rs_unpack_can_extract_with_zipfast(s.c_str()) != 0;
}

bool ZipFastExtractor::Extract()
{
	auto archiveStr = fs::u8string(m_archive);
	auto outputStr = fs::u8string(m_outputDir);
	auto logCb = [](void* /*ctx*/, int kind, const char* msg)
	{
		if (!msg) return;
		switch (kind)
		{
			case 0:
				info("%s", msg);
				break;
			case 1:
				error("%s", msg);
				break;
			case 2:
				warn("%s", msg);
				break;
			default:
				detail("%s", msg);
				break;
		}
	};
	return nzbget_rs_unpack_extract_with_zipfast(
		archiveStr.c_str(),
		outputStr.c_str(),
		m_password.c_str(),
		static_cast<int>(m_mode),
		logCb,
		nullptr) != 0;
}

bool IsArchive(const fs::path& file)
{
	return ZipFastExtractor::IsSupported(file) || SevenZip::IsSupported(file) || Unrar::IsSupported(file);
}

ExtractorPtr MakeExtractor(fs::path archive, fs::path outputDir,
						   std::string password, OverwriteMode mode)
{
	const std::string archiveName = fs::u8string(archive.filename());

	if (ZipFastExtractor::IsSupported(archive) || SevenZip::IsSupported(archive) || Unrar::IsSupported(archive))
	{
		return std::make_unique<ZipFastExtractor>(std::move(archive), std::move(outputDir),
												  std::move(password), mode);
	}

	error("Unpack: Could not process '%s': Unsupported archive format: %s", archiveName.c_str(), fs::u8string(archive.extension()).c_str());
	return nullptr;
}
}  // namespace Unpack
