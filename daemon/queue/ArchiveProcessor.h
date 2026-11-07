/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
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


#ifndef ARCHIVE_PROCESSOR_H
#define ARCHIVE_PROCESSOR_H

#include <vector>
#include <optional>
#include "Options.h"
#include "FileSystem.h"
#include "Unpack.h"
#include "Log.h"
#include "nzbget_rs.h"

namespace Incoming
{

struct Config
{
	fs::path unpackDir;
	fs::path processedDir;
	fs::path brokenDir;
	Options::ENzbDirArchiveAction action;
};

class ArchiveProcessor final
{
public:
	explicit ArchiveProcessor(Config config) : m_config(std::move(config))
	{}

	std::optional<std::vector<fs::path>> Process(const fs::path& archiveFile, const fs::path& destDir) const
	{
		const auto unpackDir = m_config.unpackDir / fs::make_unique_filename();
		std::vector<fs::path> to;
		const std::string archiveStr = fs::u8string(archiveFile);
		const std::string unpackStr = fs::u8string(unpackDir);
		const std::string destStr = fs::u8string(destDir);
		const std::string processedStr = fs::u8string(m_config.processedDir);
		const std::string brokenStr = fs::u8string(m_config.brokenDir);

		if (!nzbget_rs_archive_processor_process(
			archiveStr.c_str(),
			unpackStr.c_str(),
			destStr.c_str(),
			processedStr.c_str(),
			brokenStr.c_str(),
			(int)m_config.action,
			&to,
			[](void* /*rawCtx*/, const char* arcPath, const char* unpPath, int* outExtracted) -> int {
				const auto extractor = Unpack::MakeExtractor(
					fs::u8path(arcPath ? arcPath : ""),
					fs::u8path(unpPath ? unpPath : ""),
					"",
					Unpack::OverwriteMode::Overwrite);
				if (!extractor)
				{
					*outExtracted = 0;
					return 0;
				}
				*outExtracted = extractor->Extract() ? 1 : 0;
				return 1;
			},
			&ArchiveLogCb,
			[](void* rawCtx, const char* movedPath) {
				auto* out = static_cast<std::vector<fs::path>*>(rawCtx);
				if (movedPath)
				{
					out->push_back(fs::u8path(movedPath));
				}
			}))
		{
			return std::nullopt;
		}

		return to;
	}

private:
	static void ArchiveLogCb(void* /*ctx*/, int kind, const char* msg)
	{
		const char* text = msg ? msg : "";
		if (kind == 1)
		{
			warn("%s", text);
		}
		else if (kind == 2)
		{
			error("%s", text);
		}
		else if (kind == 3)
		{
			detail("%s", text);
		}
		else
		{
			info("%s", text);
		}
	}

	bool Extract(const fs::path& archiveFile, const fs::path& unpackDir) const
	{
		const auto extractor = Unpack::MakeExtractor(archiveFile, unpackDir, "", Unpack::OverwriteMode::Overwrite);
		if (!extractor)
		{
			return false;
		}

		if (!extractor->Extract())
		{
			const std::string archiveStr = fs::u8string(archiveFile);
			const std::string unpackStr = fs::u8string(unpackDir);
			const std::string brokenStr = fs::u8string(m_config.brokenDir);
			nzbget_rs_archive_processor_handle_extract_failure(
				archiveStr.c_str(),
				unpackStr.c_str(),
				brokenStr.c_str(),
				nullptr,
				&ArchiveLogCb);
			return false;
		}

		return true;
	}

	const Config m_config;
};

}

#endif
