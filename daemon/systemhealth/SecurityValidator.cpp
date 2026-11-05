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

#include "Options.h"
#include "Status.h"
#include "SecurityValidator.h"
#include "nzbget_rs.h"

namespace SystemHealth::Security
{

SectionReport SecurityValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	std::string_view controlIp = m_options.GetControlIp();
	std::string_view controlUser = m_options.GetControlUsername();
	std::string_view controlPass = m_options.GetControlPassword();
	std::string_view restrictedUser = m_options.GetRestrictedUsername();
	std::string_view restrictedPass = m_options.GetRestrictedPassword();
	std::string_view addUser = m_options.GetAddUsername();
	std::string_view addPass = m_options.GetAddPassword();
	std::string_view secureCert = m_options.GetSecureCert();
	std::string_view secureKey = m_options.GetSecureKey();
	std::string_view daemonUser = m_options.GetDaemonUsername();

	nzbget_rs_health_validate_security_section(
		controlIp.data(), controlIp.size(),
		m_options.GetControlPort(),
		controlUser.data(), controlUser.size(),
		controlPass.data(), controlPass.size(),
		restrictedUser.data(), restrictedUser.size(),
		restrictedPass.data(), restrictedPass.size(),
		addUser.data(), addUser.size(),
		addPass.data(), addPass.size(),
		m_options.GetFormAuth() ? 1 : 0,
		m_options.GetSecureControl() ? 1 : 0,
		secureCert.data(), secureCert.size(),
		secureKey.data(), secureKey.size(),
		m_options.GetSecurePort(),
		m_options.GetCertCheck() ? 1 : 0,
		m_options.GetCertStorePath().empty() ? 1 : 0,
		m_options.GetDaemonMode() ? 1 : 0,
		daemonUser.data(), daemonUser.size(),
		m_options.GetUMask(),
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

Status ControlPasswordValidator::Validate() const
{
	std::string_view password = m_options.GetControlPassword();
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_control_password(
		password.data(), password.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status FormAuthValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_form_auth(
		m_options.GetFormAuth() ? 1 : 0,
		m_options.GetSecureControl() ? 1 : 0,
		Util::EmptyStr(m_options.GetControlPassword()) ? 1 : 0,
		Util::EmptyStr(m_options.GetAddUsername()) ? 1 : 0,
		Util::EmptyStr(m_options.GetRestrictedUsername()) ? 1 : 0,
		&msg);
	return Status::FromRust(sev, msg);
}

}  // namespace SystemHealth::Security
