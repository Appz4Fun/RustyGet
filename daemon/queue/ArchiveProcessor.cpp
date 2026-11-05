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


#include "nzbget.h"

#include "Unpack.h"
#include "Log.h"
#include "ArchiveProcessor.h"
#include "nzbget_rs.h"

namespace Incoming
{

namespace
{
	const char* LOG_PREFIX = "ArchiveProcessor:";

	void ArchiveLogCb(void* /*ctx*/, int kind, const char* msg)
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
}

std::optional<std::vector<fs::path>> ArchiveProcessor::Process(const fs::path& archiveFile, const fs::path& destDir) const
{
	const std::string filenameStr = fs::u8string(archiveFile.filename());
	info("%s Extracting '%s'", LOG_PREFIX, filenameStr.c_str());

	const auto unpackDir = m_config.unpackDir / fs::make_unique_filename();

	if (!Extract(archiveFile, unpackDir))
	{
		return std::nullopt;
	}

	std::vector<fs::path> to;
	const std::string archiveStr = fs::u8string(archiveFile);
	const std::string unpackStr = fs::u8string(unpackDir);
	const std::string destStr = fs::u8string(destDir);
	const std::string processedStr = fs::u8string(m_config.processedDir);

	nzbget_rs_archive_processor_finish(
		archiveStr.c_str(),
		unpackStr.c_str(),
		destStr.c_str(),
		processedStr.c_str(),
		(int)m_config.action,
		&to,
		&ArchiveLogCb,
		[](void* rawCtx, const char* movedPath) {
			auto* out = static_cast<std::vector<fs::path>*>(rawCtx);
			if (movedPath)
			{
				out->push_back(fs::u8path(movedPath));
			}
		});

	return to;
}

bool ArchiveProcessor::Extract(const fs::path& archiveFile, const fs::path& unpackDir) const
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

}
