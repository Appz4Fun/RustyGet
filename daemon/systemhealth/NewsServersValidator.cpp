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
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

namespace SystemHealth::NewsServers
{

NewsServersValidator::NewsServersValidator(const Servers& servers)
	: SectionGroupValidator(MakeServerValidators(servers)), m_servers(servers)
{
	m_validators.reserve(3);
	m_validators.push_back(std::make_unique<ServersConfiguredValidator>(servers));
	m_validators.push_back(std::make_unique<AnyServerActiveValidator>(servers));
	m_validators.push_back(std::make_unique<AnyPrimaryServerExistsValidator>(servers));
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

Status ServersConfiguredValidator::Validate() const
{
	if (m_servers.empty() ||
		(m_servers.size() == 1 && m_servers.front()->GetHost() == DEFAULT_SERVER_HOST))
		return Status::Error("No servers are configured");

	return Status::Ok();
}

Status AnyServerActiveValidator::Validate() const
{
	auto anyActive =
		std::any_of(m_servers.cbegin(), m_servers.cend(), [](const auto& server)
					{ return server->GetActive() && server->GetMaxConnections() > 0; });
	if (!anyActive) return Status::Error("At least one server must be active");

	return Status::Ok();
}

Status AnyPrimaryServerExistsValidator::Validate() const
{
#ifdef NZBGET_USE_RUST
	std::vector<int32_t> levels;
	std::vector<uint8_t> actives;
	levels.reserve(m_servers.size());
	actives.reserve(m_servers.size());
	for (const auto& server : m_servers)
	{
		levels.push_back(server->GetLevel());
		actives.push_back(server->GetActive() ? 1 : 0);
	}
	switch (nzbget_rs_euh_health_validate_primary_server(
		levels.data(), actives.data(), levels.size()))
	{
		case 1:
			return Status::Error("At least one server must be configured");
		case 2:
			return Status::Error("At least one primary server must be active");
		default:
			return Status::Ok();
	}
#else
	if (m_servers.empty())
	{
		return Status::Error("At least one server must be configured");
	}

	int minLevel = m_servers.front()->GetLevel();
	for (const auto& server : m_servers)
	{
		if (server->GetLevel() < minLevel)
		{
			minLevel = server->GetLevel();
		}
	}

	auto anyPrimaryActive = std::any_of(m_servers.cbegin(), m_servers.cend(),
		[minLevel](const auto& server) { return server->GetLevel() == minLevel && server->GetActive(); });

	if (!anyPrimaryActive)
	{
		return Status::Error("At least one primary server must be active");
	}

	return Status::Ok();
#endif
}

}  // namespace SystemHealth::NewsServers
