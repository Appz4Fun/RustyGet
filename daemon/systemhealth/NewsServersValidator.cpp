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

#include "NewsServersValidator.h"
#include "Validators.h"
#include "nzbget_rs.h"

namespace SystemHealth::NewsServers
{

NewsServersValidator::NewsServersValidator(const Servers& servers)
	: SectionGroupValidator(MakeServerValidators(servers)), m_servers(servers)
{
}

std::vector<std::unique_ptr<SectionValidator>> NewsServersValidator::MakeServerValidators(
	const Servers& servers) const
{
	std::vector<std::unique_ptr<SectionValidator>> validators;
	validators.reserve(servers.size());
	for (const auto& server : servers)
	{
		validators.push_back(std::make_unique<NewsServer::NewsServerValidator>(*server));
	}
	return validators;
}

SectionReport NewsServersValidator::Validate() const
{
	SectionReport report = SectionGroupValidator::Validate();
	std::vector<const char*> hostPtrs;
	std::vector<size_t> hostLens;
	std::vector<uint8_t> actives;
	std::vector<int32_t> maxConns;
	std::vector<int32_t> levels;
	hostPtrs.reserve(m_servers.size());
	hostLens.reserve(m_servers.size());
	actives.reserve(m_servers.size());
	maxConns.reserve(m_servers.size());
	levels.reserve(m_servers.size());
	for (const auto& server : m_servers)
	{
		std::string_view host = server->GetHost();
		hostPtrs.push_back(host.data());
		hostLens.push_back(host.size());
		actives.push_back(server->GetActive() ? 1 : 0);
		maxConns.push_back(server->GetMaxConnections());
		levels.push_back(server->GetLevel());
	}
	auto appendIssue = +[](void* ctx, const char*, size_t, int severity, const char* msg_ptr, size_t msg_len)
	{
		auto* issues = static_cast<std::vector<Status>*>(ctx);
		Severity sev = Severity::Ok;
		if (severity == 1) sev = Severity::Info;
		else if (severity == 2) sev = Severity::Warning;
		else if (severity == 3) sev = Severity::Error;
		if (sev != Severity::Ok)
		{
			issues->push_back(
				Status::FromRaw(sev, std::string(msg_ptr ? msg_ptr : "", msg_len)));
		}
	};
	nzbget_rs_health_validate_news_servers_group(
		hostPtrs.data(),
		hostLens.data(),
		actives.data(),
		maxConns.data(),
		levels.data(),
		m_servers.size(),
		&report.issues,
		appendIssue);
	return report;
}

Status AnyPrimaryServerExistsValidator::Validate() const
{
	std::vector<int32_t> levels;
	std::vector<uint8_t> actives;
	levels.reserve(m_servers.size());
	actives.reserve(m_servers.size());
	for (const auto& server : m_servers)
	{
		levels.push_back(server->GetLevel());
		actives.push_back(server->GetActive() ? 1 : 0);
	}
	NzbgetRsBuf msg{};
	int sev = nzbget_rs_health_validate_primary_server_exists(
		levels.data(), actives.data(), levels.size(), &msg);
	return Status::FromRust(sev, msg);
}

}  // namespace SystemHealth::NewsServers
