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

#include "IncomingNzbValidator.h"
#include "nzbget_rs.h"

namespace SystemHealth::IncomingNzb
{
IncomingNzbValidator::IncomingNzbValidator(const Options& options) : m_options(options)
{
}

SectionReport IncomingNzbValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	nzbget_rs_health_validate_incoming_nzb_section(
		m_options.GetAppendCategoryDir() ? 1 : 0,
		m_options.GetNzbDirInterval(),
		m_options.GetNzbDirFileAge(),
		m_options.GetDupeCheck() ? 1 : 0,
		m_options.GetDupeSearch() ? 1 : 0,
		static_cast<int>(m_options.GetHealthCheck()),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}
}  // namespace SystemHealth::IncomingNzb
