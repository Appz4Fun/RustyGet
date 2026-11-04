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


#include "nzbget.h"

#include "DataAnalytics.h"
#include "nzbget_rs.h"

double DataAnalytics::GetMin() const { return m_min; }
double DataAnalytics::GetMax() const { return m_max; }
double DataAnalytics::GetAvg() const { return m_avg; }
double DataAnalytics::GetMedian() const { return m_median; }
double DataAnalytics::GetPercentile25() const { return m_percentile25; }
double DataAnalytics::GetPercentile75() const { return m_percentile75; }
double DataAnalytics::GetPercentile90() const { return m_percentile90; }

void DataAnalytics::Compute()
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

void DataAnalytics::AddMeasurement(double measuremen)
{
	m_measurements.push_back(measuremen);
}

double DataAnalytics::CalculatePercentile(double percentile)
{
	return nzbget_rs_euh_data_analytics_percentile(
		m_measurements.data(), m_measurements.size(), percentile);
}
