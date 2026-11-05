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

#include "CheckAndRepairValidator.h"
#include "Options.h"
#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth::CheckAndRepair
{
CheckAndRepairValidator::CheckAndRepairValidator(const Options& options) : m_options(options)
{
}

SectionReport CheckAndRepairValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	const size_t hwThreads = std::thread::hardware_concurrency();
	nzbget_rs_health_validate_check_and_repair_section(
		m_options.GetCrcCheck() ? 1 : 0,
		static_cast<int>(m_options.GetParCheck()),
		m_options.GetParRepair() ? 1 : 0,
		m_options.GetParQuick() ? 1 : 0,
		m_options.GetParBuffer(),
		m_options.GetParThreads(),
		hwThreads,
		m_options.GetParRename() ? 1 : 0,
		m_options.GetRarRename() ? 1 : 0,
		m_options.GetDirectRename() ? 1 : 0,
		m_options.GetHardLinking() ? 1 : 0,
		static_cast<int>(m_options.GetHealthCheck()),
		m_options.GetParTimeLimit(),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

}  // namespace SystemHealth::CheckAndRepair
