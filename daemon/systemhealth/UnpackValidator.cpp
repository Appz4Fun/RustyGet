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

#include "SectionValidator.h"
#include "UnpackValidator.h"
#include "Options.h"
#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth::Unpack
{

UnpackValidator::UnpackValidator(const Options& options) : m_options(options)
{
}

SectionReport UnpackValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	std::string unrarPath = m_options.GetUnrarPath().string();
	std::string sevenZipPath = m_options.GetSevenZipPath().string();
	std::string_view extCleanup = m_options.GetExtCleanupDisk();
	std::string_view unpackPassFile = m_options.GetUnpackPassFile();
	std::string_view unpackIgnoreExt = m_options.GetUnpackIgnoreExt();

	nzbget_rs_health_validate_unpack_section(
		m_options.GetUnpack() ? 1 : 0,
		m_options.GetDirectUnpack() ? 1 : 0,
		m_options.GetUnpackPauseQueue() ? 1 : 0,
		unrarPath.data(), unrarPath.size(),
		sevenZipPath.data(), sevenZipPath.size(),
		extCleanup.data(), extCleanup.size(),
		unpackPassFile.data(), unpackPassFile.size(),
		unpackIgnoreExt.data(), unpackIgnoreExt.size(),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

}  // namespace SystemHealth::Unpack
