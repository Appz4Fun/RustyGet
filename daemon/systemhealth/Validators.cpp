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

#include <fstream>
#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth
{
Validator::~Validator() = default;

Status RequiredOption(std::string_view name, std::string_view value)
{
	if (value.empty())
	{
		return Status::Error("'" + std::string(name) +
							 "' is required and cannot be empty");
	}

	return Status::Ok();
}

Status RequiredPathOption(std::string_view name, const fs::path& value)
{
	if (value.empty())
	{
		return Status::Error("'" + std::string(name) +
							 "' is required and cannot be empty");
	}

	return Status::Ok();	
}

Status UniquePath(
	std::string_view name, const fs::path& path,
	const std::vector<std::pair<std::string_view, const fs::path&>>& other)
{
	if (path.empty()) return Status::Ok();

	const auto found = std::find_if(other.cbegin(), other.cend(),
									[&](const auto& pair) { return pair.second == path; });

	if (found == other.cend()) return Status::Ok();

	return Status::Warning("'" + std::string(name) + "' and '" + std::string(found->first) +
						   "' are identical that can lead to unexpected behavior");
}

Status CheckPassword(std::string_view password)
{
	int pwCheck = nzbget_rs_feh_health_check_password(password.data(), password.size());
	if (pwCheck == 1)
	{
		return Status::Warning("Password is set to empty");
	}
	if (pwCheck == 2)
	{
		return Status::Info("Password is too short (recommended minimum 8 characters)");
	}
	return Status::Ok();
}

Status CheckPositiveNum(std::string_view name, int value)
{
	if (value < 0) return Status::Error("'" + std::string(name) + "' option must not be negative");
	return Status::Ok();
}

namespace File
{

Status Exists(const fs::path& path)
{
	fs::error_code ec;
	bool exists = fs::exists(path, ec);
	if (ec)
	{
		std::stringstream ss;
		ss << "Failed to check " << path << ": " << ec.message();
		return Status::Error(ss.str());
	}

	if (!exists)
	{
		std::stringstream ss;
		ss << path << " file doesn't exist";
		return Status::Error(ss.str());
	}

	bool isFile = fs::is_regular_file(path, ec);
	if (ec)
	{
		std::stringstream ss;
		ss << "Failed to check type of " << path << ": " << ec.message();
		return Status::Error(ss.str());
	}

	if (!isFile)
	{
		std::stringstream ss;
		ss << path << " exists but is not a regular file";
		return Status::Error(ss.str());
	}

	return Status::Ok();
}

Status Readable(const fs::path& path)
{
	std::ifstream file(path.c_str());
	if (file.is_open())
	{
		return Status::Ok();
	}

	std::stringstream ss;
	ss << "Failed to read " << path << ": " << std::strerror(errno);
	return Status::Error(ss.str());
}

Status Writable(const fs::path& path)
{
	std::ofstream file(path.c_str(), std::ios::app);
	if (file.is_open())
	{
		return Status::Ok();
	}

	std::stringstream ss;
	ss << "Failed to write to " << path << ": " << std::strerror(errno);
	return Status::Error(ss.str());
}

Status Executable(const fs::path& path)
{
	if (access(path.c_str(), X_OK) == 0)
	{
		return Status::Ok();
	}

	std::stringstream ss;
	ss << path << " is not executable: " << std::strerror(errno);
	return Status::Error(ss.str());
}

bool NullOrConsoleDevice(const fs::path& path)
{
	if (path.empty()) return false;

	std::string p = path.string();
	return nzbget_rs_feh_health_is_null_or_console_device(p.data(), p.size()) != 0;
}

}  // namespace File

namespace Directory
{

Status Exists(const fs::path& path)
{
	fs::error_code ec;
	bool exists = fs::exists(path, ec);
	if (ec)
	{
		std::stringstream ss;
		ss << "Failed to check " << path << ": " << ec.message();
		return Status::Error(ss.str());
	}

	if (!exists)
	{
		std::stringstream ss;
		ss << path << " directory does not exist";
		return Status::Error(ss.str());
	}

	bool isDir = fs::is_directory(path, ec);
	if (ec)
	{
		std::stringstream ss;
		ss << "Failed to check type of " << path << ": " << ec.message();
		return Status::Error(ss.str());
	}

	if (!isDir)
	{
		std::stringstream ss;
		ss << path << " is not a directory";
		return Status::Error(ss.str());
	}

	return Status::Ok();
}

Status Readable(const fs::path& path)
{
	fs::error_code ec;
	fs::directory_iterator it(path, ec);
	if (ec)
	{
		std::stringstream ss;
		ss << "Failed to read " << path << " directory: " << ec.message();
		return Status::Error(ss.str());
	}

	return Status::Ok();
}

Status Writable(const fs::path& path)
{
	const fs::path testPath = path / "nzbget_write_test.tmp";
	{
		std::ofstream testFile(testPath.c_str());
		if (!testFile.is_open())
		{
			std::stringstream ss;
			ss << "Failed to write to " << path << ": " << std::strerror(errno);
			return Status::Error(ss.str());
		}

		testFile << "Write test";
		if (testFile.fail())
		{
			testFile.close();
			std::stringstream ss;
			ss << "Failed to write content to " << path << ": " << std::strerror(errno);

			fs::error_code ignore;
			fs::remove(testPath, ignore);
			return Status::Error(ss.str());
		}
	}

	fs::error_code ec;
	fs::remove(testPath, ec);
	if (ec)
	{
		std::stringstream ss;
		ss << "Writable check failed during cleanup for " << path << ": " << ec.message();
		return Status::Warning(ss.str());
	}

	return Status::Ok();
}

}  // namespace Directory

namespace Network
{

Status ValidHost(std::string_view hostname, Connection::EIPVersion ipv)
{
	size_t ipOff = 0, ipLen = 0;
	nzbget_rs_feh_health_extract_host_ip_slice(hostname.data(), hostname.size(), &ipOff, &ipLen);
	std::string_view ip = hostname.substr(ipOff, ipLen);

	boost::system::error_code ecv4, ecv6;
	if (!hostname.empty() && hostname.size() <= 255)
	{
		boost::asio::ip::make_address_v4(ip, ecv4);
		boost::asio::ip::make_address_v6(ip, ecv6);
	}
	else
	{
		ecv4 = boost::asio::error::invalid_argument;
		ecv6 = boost::asio::error::invalid_argument;
	}

	int code = nzbget_rs_feh_health_validate_host(
		hostname.data(), hostname.size(),
		(int)ipv, (int)Connection::EIPVersion::ipV4, (int)Connection::EIPVersion::ipV6,
		!ecv4 ? 1 : 0, !ecv6 ? 1 : 0,
		&ipOff, &ipLen);
	ip = hostname.substr(ipOff, ipLen);

	switch (code)
	{
		case 0:
			return Status::Ok();
		case 1:
			return Status::Error("Hostname or IP address is empty");
		case 2:
			return Status::Error("Hostname is too long (maximum 255 characters)");
		case 3:
			return Status::Error("IPv6 address '" + std::string(ip) + "' is provided, but connection mode is set to IPv4");
		case 4:
			return Status::Error("IPv4 address '" + std::string(ip) + "' is provided, but connection mode is set to IPv6");
		case 5:
			return Status::Error("Remove brackets from the IP address, e.g., '" + std::string(ip) + "'");
		default:
			return Status::Error("Invalid hostname or IP address: '" + std::string(hostname) + "'");
	}
}

Status ValidPort(int port)
{
	if (!nzbget_rs_feh_health_valid_port(port)) return Status::Error("Port must be between 1 and 65535");

	return Status::Ok();
}
}  // namespace Network
}  // namespace SystemHealth
