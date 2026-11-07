/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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

#ifndef NETWORK_SPEED_TEST_H
#define NETWORK_SPEED_TEST_H

#include <array>
#include <chrono>
#include <string_view>

#include "DataAnalytics.h"
#include "Connection.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace Network
{
	class SpeedTest final
	{
	public:
		SpeedTest() = default;
		SpeedTest(const SpeedTest&) = delete;
		SpeedTest operator=(const SpeedTest&) = delete;

		double RunTest() noexcept(false)
		{
			info("Download speed test starting...");

			for (TestSpec test : m_testSpecs)
			{
				if (!RunTest(test)) break;
				if (!IsRequiredMoreTests()) break;
			}

			if (m_dataset.empty())
			{
				error("%s", m_errMsg.data());
				throw std::runtime_error(m_errMsg.data());
			}

			double finalSpeed = GetFinalSpeed();

			info("Download speed %.2f Mbps", finalSpeed);

			return finalSpeed;
		}

	private:
		struct TestSpec
		{
			const char* name;
			uint32_t bytes;
			int iterations;
			std::chrono::seconds timeout;
		};

		enum BytesToDownload : uint32_t
		{
			KiB = 1024,
			KiB100 = 1024 * 100,
			MiB1 = 1024 * 1024,
			MiB10 = 1024 * 1024 * 10,
			MiB25 = 1024 * 1024 * 25,
			MiB100 = 1024 * 1024 * 100,
			MiB250 = 1024 * 1024 * 250,
			GiB1 = 1024 * 1024 * 1024,
		};

		static constexpr std::array<TestSpec, 7> m_testSpecs
		{ {
			{ "100 KiB", KiB100, 10, std::chrono::seconds{1} },
			{ "1 MiB", MiB1, 8, std::chrono::seconds{2} },
			{ "10 MiB", MiB10, 6, std::chrono::seconds{2} },
			{ "25 MiB", MiB25, 4, std::chrono::seconds{2} },
			{ "100 MiB", MiB100, 3, std::chrono::seconds{3} },
			{ "250 MiB", MiB250, 2, std::chrono::seconds{4} },
			{ "1 GiB", GiB1, 2, std::chrono::seconds{5} },
		} };
		static constexpr size_t m_bufferSize = 8 * KiB;
		static constexpr std::string_view m_host = "speed.cloudflare.com";
		static constexpr std::string_view m_path = "__down";
		static constexpr std::string_view m_query = "?bytes=";
		static constexpr std::string_view m_errMsg = "No speed data was collected during the network speed test";

		bool RunTest(TestSpec testSpec)
		{
			DataAnalytics data;
			auto [name, size, iterations, timeout] = testSpec;
			int failures = 0;

			while (--iterations)
			{
				auto [downloaded, elapsedTime] = ExecuteTest(MakeRequest(size), timeout);
				if (nzbget_rs_dqh_speed_test_is_iteration_failed(
					elapsedTime.count(), downloaded, size) != 0)
				{
					++failures;
					if (HasExceededMaxFailures(failures, iterations))
					{
						return false;
					}

					continue;
				}

				double speed = CalculateSpeedInMbps(downloaded, elapsedTime.count());
				data.AddMeasurement(speed);

				info("Downloaded %s at %.2f Mbps", name, speed);
			}

			data.Compute();
			m_dataset.push_back(std::move(data));

			return true;
		}

		std::pair<uint32_t, std::chrono::duration<double>> ExecuteTest(const std::string& request, std::chrono::seconds timeout)
		{
			using namespace std::chrono;
			Connection connection(m_host.data(), m_port, m_useTls);
			connection.SetTimeout(1);

			if (!connection.Connect())
			{
				return { 0, seconds{0} };
			}

			if (!connection.Send(request.c_str(), request.size()))
			{
				return { 0, seconds{0} };
			}

			uint32_t totalRecieved = 0;
			char buffer[m_bufferSize];
			auto start = steady_clock::now();
			while (int recieved = connection.TryRecv(buffer, m_bufferSize))
			{
				if (nzbget_rs_dqh_speed_test_should_stop_recv_loop(
					recieved,
					duration<double>(steady_clock::now() - start).count(),
					duration<double>(timeout).count()) != 0)
				{
					break;
				}

				totalRecieved += recieved;
			}
			auto finish = steady_clock::now();

			return { totalRecieved, duration<double>(finish - start) };
		}

		std::string MakeRequest(uint32_t filesize) const
		{
			NzbgetRsBuf buf = nzbget_rs_snh_speed_test_make_request(
				m_path.data(), m_path.size(),
				m_query.data(), m_query.size(),
				filesize,
				m_host.data(), m_host.size());
			std::string req(buf.data ? buf.data : "", buf.len);
			nzbget_rs_free(buf);
			return req;
		}

		double CalculateSpeedInMbps(uint32_t bytes, double timeSec) const
		{
			return nzbget_rs_snh_speed_test_calc_mbps(bytes, timeSec);
		}

		bool IsRequiredMoreTests() const
		{
			if (m_dataset.size() < 2)
				return nzbget_rs_snh_speed_test_is_required_more(m_dataset.size(), 0.0, 0.0) != 0;

			const DataAnalytics& curr = m_dataset.back();
			const DataAnalytics& prev = m_dataset[m_dataset.size() - 2];
			return nzbget_rs_snh_speed_test_is_required_more(
				m_dataset.size(), curr.GetMedian(), prev.GetPercentile25()) != 0;
		}

		bool HasExceededMaxFailures(int failures, int iterations) const
		{
			return nzbget_rs_snh_speed_test_exceeded_max_failures(failures, iterations) != 0;
		}

		double GetFinalSpeed() const
		{
			const auto& dataAnalytics = m_dataset.front();
			std::vector<double> p90s;
			p90s.reserve(m_dataset.size());
			for (const auto& analytics : m_dataset)
			{
				p90s.push_back(analytics.GetPercentile90());
			}
			return nzbget_rs_snh_speed_test_final_speed(
				dataAnalytics.GetMedian(), p90s.data(), p90s.size());
		}

#ifdef DISABLE_TLS
		bool m_useTls = false;
		int m_port = 80;
#else
		bool m_useTls = true;
		int m_port = 443;
#endif
		std::vector<DataAnalytics> m_dataset;
	};
}

#endif
