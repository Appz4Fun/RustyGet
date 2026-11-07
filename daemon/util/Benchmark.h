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

#ifndef BENCHMARK_H
#define BENCHMARK_H

#include <chrono>
#include <cstdint>
#include <stdexcept>
#include <string>
#include "FileSystem.h"
#include "nzbget_rs.h"

namespace Benchmark
{
	struct Result
	{
		uint64_t writeBytes = 0;
		double writeMs = 0.0;
		uint64_t readBytes = 0;
		double readMs = 0.0;
	};

	class DiskBenchmark final
	{
	public:
		static constexpr size_t DEFAULT_BLOCK_SIZE = size_t{1024} * 1024;
		static constexpr size_t MAX_BLOCK_SIZE = size_t{512} * 1024 * 1024;
		static constexpr uint64_t FREE_SPACE_RESERVE = 100ull * 1024 * 1024;

		static uint64_t LimitByFreeSpace(uint64_t requested, uint64_t available)
		{
			return nzbget_rs_euh_benchmark_limit_by_free_space(requested, available, FREE_SPACE_RESERVE);
		}

		Result Run(
			const std::string& dir,
			size_t blockSizeBytes,
			uint64_t maxFileSizeBytes,
			std::chrono::seconds timeout) const
		{
			fs::error_code ec;
			const fs::path dirPath = fs::u8path(dir);
			fs::space_info space = fs::space(dirPath, ec);
			std::string spaceErr = ec ? ec.message() : std::string();

			uint64_t writeBytes = 0;
			double writeMs = 0.0;
			uint64_t readBytes = 0;
			double readMs = 0.0;
			NzbgetRsBuf errBuf{nullptr, 0};
			int rc = nzbget_rs_benchmark_run(
				dir.data(), dir.size(),
				blockSizeBytes,
				maxFileSizeBytes,
				timeout.count(),
				ec ? 0 : 1,
				space.available,
				spaceErr.data(), spaceErr.size(),
				&writeBytes, &writeMs, &readBytes, &readMs,
				&errBuf);
			std::string errMsg(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			if (rc == 1)
			{
				throw std::invalid_argument(errMsg);
			}
			if (rc != 0)
			{
				throw std::runtime_error(errMsg);
			}
			return Result{writeBytes, writeMs, readBytes, readMs};
		}
	};
}

#endif
