/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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
#include "ServerPool.h"
#include "Util.h"
#include "nzbget_rs.h"

static const int CONNECTION_HOLD_SECODNS = 5;

void ServerPool::PooledConnection::SetFreeTimeNow()
{
	m_freeTime = Util::CurrentTime();
}

NewsServer* ServerPool::GetServerById(int id)
{
	auto serverIt = std::find_if(
		begin(m_servers),
		end(m_servers),
		[&](const std::unique_ptr<NewsServer>& server)
		{
			return server->GetId() == id;
		}
	);

	if (serverIt != end(m_servers))
	{
		return serverIt->get();
	}

	return nullptr;
}


void ServerPool::AddServer(std::unique_ptr<NewsServer> newsServer)
{
	debug("Adding server to ServerPool");

	m_servers.push_back(std::move(newsServer));
	m_sortedServers.push_back(m_servers.back().get());
}

/*
 * Calculate normalized levels for all servers.
 * Normalized Level means: starting from 0 with step 1.
 * The servers of minimum Level must be always used even if they are not active;
 * this is to prevent backup servers to act as main servers.
**/
void ServerPool::NormalizeLevels()
{
	if (m_servers.empty())
	{
		return;
	}

	size_t count = m_sortedServers.size();
	std::vector<NzbgetRsServerNormInput> inputs(count);
	std::vector<size_t> sortedIndices(count);
	std::vector<int> normLevels(count);
	std::vector<NewsServer*> origServers = m_sortedServers;
	for (size_t i = 0; i < count; i++)
	{
		inputs[i].level = origServers[i]->GetLevel();
		inputs[i].active = origServers[i]->GetActive() ? 1 : 0;
		inputs[i].max_connections = origServers[i]->GetMaxConnections();
	}
	m_maxNormLevel = nzbget_rs_server_pool_normalize_levels(
		inputs.data(), count, sortedIndices.data(), normLevels.data());
	for (size_t i = 0; i < count; i++)
	{
		origServers[i]->SetNormLevel(normLevels[i]);
		m_sortedServers[i] = origServers[sortedIndices[i]];
	}
}

void ServerPool::InitConnections()
{
	debug("Initializing connections in ServerPool");

	Guard guard(m_connectionsMutex);

	NormalizeLevels();
	m_levels.clear();

	for (NewsServer* newsServer : m_sortedServers)
	{
		newsServer->SetBlockTime(0);
		int normLevel = newsServer->GetNormLevel();
		if (newsServer->GetNormLevel() > -1)
		{
			if ((int)m_levels.size() <= normLevel)
			{
				m_levels.push_back(0);
			}

			if (newsServer->GetActive())
			{
				int connections = 0;

				for (PooledConnection* connection : &m_connections)
				{
					if (connection->GetNewsServer() == newsServer)
					{
						connections++;
					}
				}

				int missingConns = nzbget_rs_cwp_server_pool_calc_missing_connections(
					connections, newsServer->GetMaxConnections());
				for (int i = 0; i < missingConns; i++)
				{
					std::unique_ptr<PooledConnection> connection = std::make_unique<PooledConnection>(newsServer);
					connection->SetTimeout(m_timeout);
					m_connections.push_back(std::move(connection));
					connections++;
				}

				m_levels[normLevel] += connections;
			}
		}
	}

	m_generation++;
}

/* Returns connection from any server on a given level or nullptr if there is no free connection at the moment.
 * If all servers are blocked and all are optional a connection from the next level is returned instead.
 */
NntpConnection* ServerPool::GetConnection(int level, NewsServer* wantServer, RawServerList* ignoreServers)
{
	Guard guard(m_connectionsMutex);

	// an inactive server's level is -1 (a desired server disabled since)
	if (level < 0)
	{
		return nullptr;
	}

	for (; nzbget_rs_dqh_server_pool_has_available_level_conns(
		level, m_levels.size(),
		(level >= 0 && level < (int)m_levels.size()) ? m_levels[level] : 0) != 0; level++)
	{
		NntpConnection* connection = LockedGetConnection(level, wantServer, ignoreServers);
		if (connection)
		{
			return connection;
		}

		for (NewsServer* newsServer : m_sortedServers)
		{
			if (!nzbget_rs_ntw_server_pool_can_advance_level(
				newsServer->GetNormLevel(), level,
				newsServer->GetActive() ? 1 : 0,
				newsServer->GetOptional() ? 1 : 0,
				IsServerBlocked(newsServer) ? 1 : 0))
			{
				return nullptr;
			}
		}
	}

	return nullptr;
}

NntpConnection* ServerPool::LockedGetConnection(int level, NewsServer* wantServer, RawServerList* ignoreServers)
{
	if (!nzbget_rs_dqh_server_pool_has_available_level_conns(
		level, m_levels.size(),
		(level >= 0 && level < (int)m_levels.size()) ? m_levels[level] : 0))
	{
		return nullptr;
	}

	PooledConnection* connection = nullptr;
	std::vector<PooledConnection*> candidates;
	candidates.reserve(m_connections.size());

	for (PooledConnection* candidateConnection : &m_connections)
	{
		NewsServer* candidateServer = candidateConnection->GetNewsServer();
		int matchWant = nzbget_rs_ntw_server_pool_match_want_server(
			wantServer ? 1 : 0,
			candidateServer->GetId(),
			wantServer ? wantServer->GetId() : 0,
			wantServer ? wantServer->GetGroup() : 0,
			candidateServer->GetGroup());
		bool isConn = candidateConnection->GetStatus() == Connection::csConnected;
		bool isBlocked = !isConn && IsServerBlocked(candidateServer);
		if (nzbget_rs_aws_server_pool_is_candidate_connection_eligible(
			candidateConnection->GetInUse() ? 1 : 0,
			candidateServer->GetActive() ? 1 : 0,
			candidateServer->GetNormLevel(),
			level,
			matchWant,
			isConn ? 1 : 0,
			isBlocked ? 1 : 0) != 0)
		{
			// free connection found, check if it's not from the server which should be ignored
			bool useConnection = true;
			if (ignoreServers && !wantServer)
			{
				for (NewsServer* ignoreServer : ignoreServers)
				{
					if (nzbget_rs_ntw_server_pool_match_ignore_server(
						candidateServer->GetId(),
						ignoreServer->GetId(),
						ignoreServer->GetGroup(),
						candidateServer->GetGroup(),
						ignoreServer->GetNormLevel(),
						candidateServer->GetNormLevel()) != 0)
					{
						useConnection = false;
						break;
					}
				}
			}

			candidateServer->SetBlockTime(0);

			if (useConnection)
			{
				candidates.push_back(candidateConnection);
			}
		}
	}

	if (!candidates.empty())
	{
		// Peeking a random free connection. This is better than taking the first
		// available connection because provides better distribution across news servers,
		// especially when one of servers becomes unavailable or doesn't have requested articles.
		size_t randomIndex = nzbget_rs_dqh_server_pool_pick_candidate_index(rand(), candidates.size());
		connection = candidates[randomIndex];
		connection->SetInUse(true);
	}

	if (connection)
	{
		m_levels[level]--;
	}

	return connection;
}

void ServerPool::FreeConnection(NntpConnection* connection, bool used)
{
	if (used)
	{
		debug("Freeing used connection");
	}

	Guard guard(m_connectionsMutex);

	((PooledConnection*)connection)->SetInUse(false);
	if (used)
	{
		((PooledConnection*)connection)->SetFreeTimeNow();
	}

	if (nzbget_rs_psp_server_pool_should_increment_level_on_free(
		connection->GetNewsServer()->GetNormLevel(),
		connection->GetNewsServer()->GetActive() ? 1 : 0) != 0)
	{
		m_levels[connection->GetNewsServer()->GetNormLevel()]++;
	}
}

void ServerPool::BlockServer(NewsServer* newsServer)
{
	int newBlockInt = 0;
	int shouldWarnInt = 0;
	{
		Guard guard(m_connectionsMutex);
		time_t curTime = Util::CurrentTime();
		nzbget_rs_psp_server_pool_eval_block_server(
			(int64_t)newsServer->GetBlockTime(),
			(int64_t)curTime,
			m_retryInterval,
			&newBlockInt,
			&shouldWarnInt);
		newsServer->SetBlockTime(curTime);
	}

	if (shouldWarnInt != 0)
	{
		warn("Blocking %s (%s) for %i sec", newsServer->GetName(), newsServer->GetHost(), m_retryInterval);
	}
}

bool ServerPool::IsServerBlocked(NewsServer* newsServer)
{
	if (!newsServer->GetBlockTime())
	{
		return false;
	}

	time_t curTime = Util::CurrentTime();
	return nzbget_rs_server_pool_is_blocked(newsServer->GetBlockTime(), curTime, m_retryInterval) != 0;
}

void ServerPool::CloseUnusedConnections()
{
	Guard guard(m_connectionsMutex);

	time_t curtime = Util::CurrentTime();

	// close and free all connections of servers which were disabled since the last check
	m_connections.erase(std::remove_if(m_connections.begin(), m_connections.end(),
		[](std::unique_ptr<PooledConnection>& connection)
		{
			if (nzbget_rs_psp_server_pool_should_erase_disabled_connection(
				connection->GetInUse() ? 1 : 0,
				connection->GetNewsServer()->GetNormLevel(),
				connection->GetNewsServer()->GetActive() ? 1 : 0) != 0)
			{
				debug("Closing (and deleting) unused connection to server%i", connection->GetNewsServer()->GetId());
				if (connection->GetStatus() == Connection::csConnected)
				{
					connection->Disconnect();
				}
				return true;
			}
			return false;
		}),
		m_connections.end());

	// close all opened connections on levels not having any in-use connections
	for (int level = 0; level <= m_maxNormLevel; level++)
	{
		std::vector<uint8_t> inUse;
		std::vector<uint8_t> connected;
		std::vector<int64_t> freeTimes;
		for (PooledConnection* connection : &m_connections)
		{
			if (connection->GetNewsServer()->GetNormLevel() == level)
			{
				inUse.push_back(connection->GetInUse() ? 1 : 0);
				connected.push_back(connection->GetStatus() == Connection::csConnected ? 1 : 0);
				freeTimes.push_back((int64_t)connection->GetFreeTime());
			}
		}
		if (nzbget_rs_server_pool_level_should_close(
			inUse.empty() ? nullptr : inUse.data(),
			connected.empty() ? nullptr : connected.data(),
			freeTimes.empty() ? nullptr : freeTimes.data(),
			inUse.size(), (int64_t)curtime, CONNECTION_HOLD_SECODNS) != 0)
		{
			for (PooledConnection* connection : &m_connections)
			{
				if (nzbget_rs_dqh_server_pool_should_disconnect_idle_level_conn(
					connection->GetNewsServer()->GetNormLevel(), level,
					(int)connection->GetStatus(), (int)Connection::csConnected) != 0)
				{
					debug("Closing (and keeping) unused connection to server%i", connection->GetNewsServer()->GetId());
					connection->Disconnect();
				}
			}
		}
	}
}

void ServerPool::Changed()
{
	debug("Server config has been changed");

	InitConnections();
	CloseUnusedConnections();
}

void ServerPool::LogDebugInfo()
{
	info("   ---------- ServerPool");

	info("    Max-Level: %i", m_maxNormLevel);

	Guard guard(m_connectionsMutex);

	time_t curTime = Util::CurrentTime();

	info("    Servers: %i", (int)m_servers.size());
	for (NewsServer* newsServer : &m_servers)
	{
		int blockSec = nzbget_rs_ntw_server_pool_calc_remaining_block_sec(
			(int64_t)newsServer->GetBlockTime(), m_retryInterval, (int64_t)curTime);
		info("      %i) %s (%s): Level=%i, NormLevel=%i, BlockSec=%i", newsServer->GetId(), newsServer->GetName(),
			newsServer->GetHost(), newsServer->GetLevel(), newsServer->GetNormLevel(), blockSec);
	}

	info("    Levels: %i", (int)m_levels.size());
	int index = 0;
	for (int size : m_levels)
	{
		info("      %i: Free connections=%i", index, size);
		index++;
	}

	info("    Connections: %i", (int)m_connections.size());
	for (PooledConnection* connection : &m_connections)
	{
		info("      %i) %s (%s): Level=%i, NormLevel=%i, InUse:%i", connection->GetNewsServer()->GetId(),
			connection->GetNewsServer()->GetName(), connection->GetNewsServer()->GetHost(),
			connection->GetNewsServer()->GetLevel(), connection->GetNewsServer()->GetNormLevel(),
			(int)connection->GetInUse());
	}
}
