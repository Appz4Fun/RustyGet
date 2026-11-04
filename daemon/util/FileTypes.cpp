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
 *  along with this program. If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include "FileTypes.h"
#include "Util.h"
#include <fstream>
#include <cstring>

#include "nzbget_rs.h"

namespace FileTypes
{

// rust/src/filetypes.rs
bool IsSevenZipExt(std::string_view ext)
{
	return nzbget_rs_file_type(0, ext.data(), ext.size()) != 0;
}

bool IsRarExt(std::string_view ext)
{
	return nzbget_rs_file_type(1, ext.data(), ext.size()) != 0;
}

bool IsRarVolumeExt(std::string_view ext)
{
	return nzbget_rs_file_type(2, ext.data(), ext.size()) != 0;
}

bool IsNumericVolumeExt(std::string_view ext)
{
	return nzbget_rs_file_type(3, ext.data(), ext.size()) != 0;
}

bool IsAllDigitsExt(std::string_view ext)
{
	return nzbget_rs_file_type(4, ext.data(), ext.size()) != 0;
}

bool IsArchiveExt(std::string_view ext)
{
	return nzbget_rs_file_type(5, ext.data(), ext.size()) != 0;
}

bool IsDiscStructureExt(std::string_view ext)
{
	return nzbget_rs_file_type(6, ext.data(), ext.size()) != 0;
}

bool IsDiscStructureDir(std::string_view dirname)
{
	return nzbget_rs_file_type(7, dirname.data(), dirname.size()) != 0;
}

bool IsDiscDescriptorExt(std::string_view ext)
{
	return nzbget_rs_file_type(8, ext.data(), ext.size()) != 0;
}

bool IsDiscImageExt(std::string_view ext)
{
	return nzbget_rs_file_type(9, ext.data(), ext.size()) != 0;
}

bool IsGenericDiscImageExt(std::string_view ext)
{
	return nzbget_rs_file_type(10, ext.data(), ext.size()) != 0;
}

bool IsClutterDir(std::string_view dirname)
{
	return nzbget_rs_file_type(11, dirname.data(), dirname.size()) != 0;
}

bool IsClutterFile(std::string_view filename)
{
	return nzbget_rs_file_type(12, filename.data(), filename.size()) != 0;
}

bool IsParityExt(std::string_view ext)
{
	return nzbget_rs_file_type(13, ext.data(), ext.size()) != 0;
}

bool IsVideoExt(std::string_view ext)
{
	return nzbget_rs_file_type(14, ext.data(), ext.size()) != 0;
}

bool IsAudioExt(std::string_view ext)
{
	return nzbget_rs_file_type(15, ext.data(), ext.size()) != 0;
}

bool IsSubtitleExt(std::string_view ext)
{
	return nzbget_rs_file_type(16, ext.data(), ext.size()) != 0;
}

bool IsNfoExt(std::string_view ext)
{
	return nzbget_rs_file_type(17, ext.data(), ext.size()) != 0;
}

bool IsBookExt(std::string_view ext)
{
	return nzbget_rs_file_type(18, ext.data(), ext.size()) != 0;
}

bool IsImageExt(std::string_view ext)
{
	return nzbget_rs_file_type(19, ext.data(), ext.size()) != 0;
}

bool IsSampleStem(std::string_view stem)
{
	return nzbget_rs_file_type(20, stem.data(), stem.size()) != 0;
}

bool IsSevenZipFile(std::string_view filename)
{
	return nzbget_rs_file_type(21, filename.data(), filename.size()) != 0;
}

bool IsRarFile(std::string_view filename)
{
	return nzbget_rs_file_type(22, filename.data(), filename.size()) != 0;
}

bool IsArchiveFile(std::string_view filename)
{
	return nzbget_rs_file_type(23, filename.data(), filename.size()) != 0;
}

bool IsSampleFile(std::string_view filename)
{
	return nzbget_rs_file_type(24, filename.data(), filename.size()) != 0;
}

bool IsKnownCollectionExt(std::string_view ext)
{
	return nzbget_rs_file_type(25, ext.data(), ext.size()) != 0;
}

std::string_view SniffExtension(std::span<const uint8_t> header)
{
	size_t len = 0;
	const char* ext = nzbget_rs_sniff_extension(header.data(), header.size(), &len);
	return std::string_view(ext, len);
}

}

namespace FileTypes
{

// Keep native path handling and ifstream read/error semantics on the C++ side.
// Header classification above is implemented in Rust when enabled.
std::string_view SniffExtension(const std::filesystem::path& filePath)
{
	std::error_code ec;
	if (!std::filesystem::is_regular_file(filePath, ec))
	{
		return "";
	}

	std::ifstream file(filePath, std::ios::binary);
	if (!file)
	{
		return "";
	}

	uint8_t buffer[512];
	file.read(reinterpret_cast<char*>(buffer), sizeof(buffer));
	std::streamsize bytesRead = file.gcount();
	if (bytesRead <= 0)
	{
		return "";
	}

	return SniffExtension(std::span<const uint8_t>(buffer, static_cast<size_t>(bytesRead)));
}

}
