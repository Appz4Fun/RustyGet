/*
 *  This file is part of RustyGet.
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

#ifndef UNPACK_H
#define UNPACK_H

#include <memory>
#include <string>
#include "FileSystem.h"
#include "Log.h"
#include "Options.h"
#include "nzbget_rs.h"

namespace Unpack
{
enum class OverwriteMode
{
	Skip,
	Overwrite,
	AutoRename
};

class Extractor
{
public:
	virtual bool Extract() = 0;
	virtual ~Extractor() = default;
};

class ZipFastExtractor final : public Extractor
{
public:
	ZipFastExtractor(fs::path archive, fs::path outputDir,
		std::string password, OverwriteMode mode)
		: m_archive(std::move(archive)),
		  m_outputDir(std::move(outputDir)),
		  m_password(std::move(password)),
		  m_mode(mode)
	{
	}

	bool Extract() override
	{
		auto archiveStr = fs::u8string(m_archive);
		auto outputStr = fs::u8string(m_outputDir);
		auto logCb = +[](void* /*ctx*/, int kind, const char* msg)
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
			-1,
			logCb,
			nullptr,
			nullptr) != 0;
	}

	static bool IsSupported(const fs::path& path)
	{
		auto s = fs::u8string(path);
		return nzbget_rs_unpack_can_extract_with_zipfast(s.c_str()) != 0 ||
			nzbget_rs_euh_sevenzip_is_supported(s.data(), s.size()) != 0 ||
			nzbget_rs_euh_unrar_is_supported(s.data(), s.size()) != 0;
	}

private:
	const fs::path m_archive;
	const fs::path m_outputDir;
	const std::string m_password;
	const OverwriteMode m_mode;
};

using ExtractorPtr = std::unique_ptr<Extractor>;

inline bool IsArchive(const fs::path& file)
{
	return ZipFastExtractor::IsSupported(file);
}

inline ExtractorPtr MakeExtractor(fs::path archive, fs::path outputDir,
						   std::string password, OverwriteMode mode)
{
	const std::string archiveName = fs::u8string(archive.filename());

	if (ZipFastExtractor::IsSupported(archive))
	{
		return std::make_unique<ZipFastExtractor>(std::move(archive), std::move(outputDir),
												  std::move(password), mode);
	}

	error("Unpack: Could not process '%s': Unsupported archive format: %s", archiveName.c_str(), fs::u8string(archive.extension()).c_str());
	return nullptr;
}
}  // namespace Unpack

#endif
