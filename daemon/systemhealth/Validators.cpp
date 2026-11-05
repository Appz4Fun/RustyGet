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

#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth
{
Validator::~Validator() = default;

Status RequiredOption(std::string_view name, std::string_view value)
{
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_required_option(name.data(), name.size(), value.data(), value.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status RequiredPathOption(std::string_view name, const fs::path& value)
{
	std::string val = value.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_required_option(name.data(), name.size(), val.data(), val.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status UniquePath(
	std::string_view name, const fs::path& path,
	const std::vector<std::pair<std::string_view, const fs::path&>>& other)
{
	std::string pathStr = path.string();
	std::vector<std::string> otherPathStrs;
	otherPathStrs.reserve(other.size());
	std::vector<const char*> otherNames;
	std::vector<size_t> otherNameLens;
	std::vector<const char*> otherPaths;
	std::vector<size_t> otherPathLens;
	otherNames.reserve(other.size());
	otherNameLens.reserve(other.size());
	otherPaths.reserve(other.size());
	otherPathLens.reserve(other.size());

	for (const auto& pair : other)
	{
		otherPathStrs.push_back(pair.second.string());
		otherNames.push_back(pair.first.data());
		otherNameLens.push_back(pair.first.size());
		otherPaths.push_back(otherPathStrs.back().data());
		otherPathLens.push_back(otherPathStrs.back().size());
	}

	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_unique_path(
		name.data(), name.size(),
		pathStr.data(), pathStr.size(),
		otherNames.data(), otherNameLens.data(),
		otherPaths.data(), otherPathLens.data(),
		other.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status CheckPassword(std::string_view password)
{
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_check_password(password.data(), password.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status CheckPositiveNum(std::string_view name, int value)
{
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_check_positive_num(name.data(), name.size(), value, &msg);
	return Status::FromRust(sev, msg);
}

namespace File
{

Status Exists(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_file_exists(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status Readable(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_file_readable(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status Writable(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_file_writable(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status Executable(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_file_executable(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

bool NullOrConsoleDevice(const fs::path& path)
{
	std::string p = path.string();
	return nzbget_rs_health_file_null_or_console_device(p.data(), p.size()) != 0;
}

}  // namespace File

namespace Directory
{

Status Exists(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_directory_exists(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status Readable(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_directory_readable(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status Writable(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_directory_writable(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

}  // namespace Directory

namespace Network
{

Status ValidHost(std::string_view hostname, Connection::EIPVersion ipv)
{
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_network_valid_host(
		hostname.data(), hostname.size(),
		static_cast<int>(ipv),
		&msg);
	return Status::FromRust(sev, msg);
}

Status ValidPort(int port)
{
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_network_valid_port(port, &msg);
	return Status::FromRust(sev, msg);
}
}  // namespace Network
}  // namespace SystemHealth
