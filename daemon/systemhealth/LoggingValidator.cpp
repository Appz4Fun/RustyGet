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

#include "LoggingValidator.h"
#include "Options.h"
#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth::Logging
{
LoggingValidator::LoggingValidator(const Options& options) : m_options(options)
{
}

SectionReport LoggingValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	std::string logFile = m_options.GetLogFilePath().string();
	nzbget_rs_health_validate_logging_section(
		static_cast<int>(m_options.GetWriteLog()),
		logFile.data(), logFile.size(),
		m_options.GetRotateLog(),
		m_options.GetLogBuffer(),
		m_options.GetCrashDump() ? 1 : 0,
		m_options.GetTimeCorrection(),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

Status WriteLogValidator::Validate() const
{
	std::string logFile = m_options.GetLogFilePath().string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_write_log(
		static_cast<int>(m_options.GetWriteLog()),
		logFile.data(), logFile.size(),
		&msg);
	return Status::FromRust(sev, msg);
}

Status RotateLogValidator::Validate() const
{
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_rotate_log(
		static_cast<int>(m_options.GetWriteLog()),
		m_options.GetRotateLog(),
		&msg);
	return Status::FromRust(sev, msg);
}

}  // namespace SystemHealth::Logging
