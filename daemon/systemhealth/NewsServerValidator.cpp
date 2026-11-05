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

#include "NewsServerValidator.h"
#include "Options.h"
#include "nzbget_rs.h"

namespace SystemHealth::NewsServer
{
SectionReport NewsServerValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();
	std::string_view name = m_server.GetName();
	std::string_view host = m_server.GetHost();
	std::string_view user = m_server.GetUser();
	std::string_view pass = m_server.GetPassword();
	std::string_view cipher = m_server.GetCipher();

	nzbget_rs_health_validate_news_server_section(
		m_server.GetActive() ? 1 : 0,
		name.data(), name.size(),
		m_server.GetLevel(),
		m_server.GetOptional() ? 1 : 0,
		m_server.GetGroup(),
		host.data(), host.size(),
		m_server.GetPort(),
		user.data(), user.size(),
		pass.data(), pass.size(),
		m_server.GetTls() ? 1 : 0,
		cipher.data(), cipher.size(),
		m_server.GetMaxConnections(),
		m_server.GetRetention(),
		static_cast<uint32_t>(m_server.GetCertVerificationLevel()),
		m_server.GetIpVersion(),
		(g_Options && g_Options->GetDupeSearch()) ? 1 : 0,
		&report.options,
		&SectionValidator::AppendOptionCallback);
	return report;
}

Status ServerActiveValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_active(m_server.GetActive() ? 1 : 0, &msg);
	return Status::FromRust(sev, msg);
}

Status ServerOptionalValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_optional(
		m_server.GetActive() ? 1 : 0, m_server.GetOptional() ? 1 : 0, m_server.GetLevel(), &msg);
	return Status::FromRust(sev, msg);
}

Status ServerHostValidator::Validate() const
{
	std::string_view host = m_server.GetHost();
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_host(
		m_server.GetActive() ? 1 : 0, host.data(), host.size(), m_server.GetIpVersion(), &msg);
	return Status::FromRust(sev, msg);
}

Status ServerPortValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_port(
		m_server.GetActive() ? 1 : 0, m_server.GetPort(), m_server.GetTls() ? 1 : 0, &msg);
	return Status::FromRust(sev, msg);
}

Status ServerUsernameValidator::Validate() const
{
	std::string_view username = m_server.GetUser();
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_username(
		m_server.GetActive() ? 1 : 0, username.data(), username.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status ServerPasswordValidator::Validate() const
{
	std::string_view password = m_server.GetPassword();
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_password(
		m_server.GetActive() ? 1 : 0, password.data(), password.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status ServerEncryptionValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_encryption(
		m_server.GetActive() ? 1 : 0, m_server.GetTls() ? 1 : 0, &msg);
	return Status::FromRust(sev, msg);
}

Status ServerCipherValidator::Validate() const
{
	std::string_view cipher = m_server.GetCipher();
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_cipher(
		m_server.GetActive() ? 1 : 0, cipher.data(), cipher.size(), m_server.GetTls() ? 1 : 0, &msg);
	return Status::FromRust(sev, msg);
}

Status ServerConnectionsValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_connections(
		m_server.GetActive() ? 1 : 0, m_server.GetMaxConnections(),
		(g_Options && g_Options->GetDupeSearch()) ? 1 : 0, &msg);
	return Status::FromRust(sev, msg);
}

Status ServerRetentionValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_retention(
		m_server.GetActive() ? 1 : 0, m_server.GetRetention(), &msg);
	return Status::FromRust(sev, msg);
}

Status ServerCertVerificationValidator::Validate() const
{
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_server_cert_verification(
		m_server.GetActive() ? 1 : 0, m_server.GetTls() ? 1 : 0,
		static_cast<uint32_t>(m_server.GetCertVerificationLevel()), &msg);
	return Status::FromRust(sev, msg);
}

}  // namespace SystemHealth::NewsServer
