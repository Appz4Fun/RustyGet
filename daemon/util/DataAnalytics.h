/*
 *  This file is part of RustyGet.
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

#ifndef DATAANALYTICS_H
#define DATAANALYTICS_H

#include <vector>
#include "nzbget_rs.h"

class DataAnalytics final
{
public:
	DataAnalytics() = default;
	~DataAnalytics() = default;

	double GetMin() const { return m_min; }
	double GetMax() const { return m_max; }
	double GetAvg() const { return m_avg; }
	double GetMedian() const { return m_median; }
	double GetPercentile25() const { return m_percentile25; }
	double GetPercentile75() const { return m_percentile75; }
	double GetPercentile90() const { return m_percentile90; }

	void Compute()
	{
		if (m_measurements.empty())
		{
			return;
		}

		nzbget_rs_euh_data_analytics_compute(
			m_measurements.data(), m_measurements.size(),
			&m_min, &m_max, &m_avg, &m_median,
			&m_percentile25, &m_percentile75, &m_percentile90);
	}

	void AddMeasurement(double measurement)
	{
		m_measurements.push_back(measurement);
	}

private:
	double CalculatePercentile(double percentile)
	{
		return nzbget_rs_euh_data_analytics_percentile(
			m_measurements.data(), m_measurements.size(), percentile);
	}

	double m_min = 0.0;
	double m_max = 0.0;
	double m_avg = 0.0;
	double m_median = 0.0;
	double m_percentile25 = 0.0;
	double m_percentile75 = 0.0;
	double m_percentile90 = 0.0;
	std::vector<double> m_measurements;
};

#endif
