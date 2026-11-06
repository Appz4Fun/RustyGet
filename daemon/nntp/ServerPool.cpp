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

				for (int i = connections; i < newsServer->GetMaxConnections(); i++)
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

	if (level < 0 || m_sortedServers.empty() || m_connections.empty())
	{
		return nullptr;
	}

	size_t srvCount = m_sortedServers.size();
	std::vector<int32_t> srvIds(srvCount);
	std::vector<int32_t> srvGroups(srvCount);
	std::vector<int32_t> srvNormLevels(srvCount);
	std::vector<uint8_t> srvActive(srvCount);
	std::vector<uint8_t> srvOptional(srvCount);
	std::vector<uint8_t> srvBlocked(srvCount);
	for (size_t i = 0; i < srvCount; ++i)
	{
		NewsServer* srv = m_sortedServers[i];
		srvIds[i] = srv->GetId();
		srvGroups[i] = srv->GetGroup();
		srvNormLevels[i] = srv->GetNormLevel();
		srvActive[i] = srv->GetActive() ? 1 : 0;
		srvOptional[i] = srv->GetOptional() ? 1 : 0;
		srvBlocked[i] = IsServerBlocked(srv) ? 1 : 0;
	}

	size_t connCount = m_connections.size();
	std::vector<size_t> connSrvIndices(connCount);
	std::vector<uint8_t> connInUse(connCount);
	std::vector<uint8_t> connConnected(connCount);
	for (size_t i = 0; i < connCount; ++i)
	{
		PooledConnection* conn = m_connections[i].get();
		NewsServer* srv = conn->GetNewsServer();
		size_t sIdx = srvCount;
		for (size_t j = 0; j < srvCount; ++j)
		{
			if (m_sortedServers[j] == srv)
			{
				sIdx = j;
				break;
			}
		}
		connSrvIndices[i] = sIdx;
		connInUse[i] = conn->GetInUse() ? 1 : 0;
		connConnected[i] = conn->GetStatus() == Connection::csConnected ? 1 : 0;
	}

	std::vector<int32_t> ignIds;
	std::vector<int32_t> ignGroups;
	std::vector<int32_t> ignNormLevels;
	if (ignoreServers && !wantServer)
	{
		ignIds.reserve(ignoreServers->size());
		ignGroups.reserve(ignoreServers->size());
		ignNormLevels.reserve(ignoreServers->size());
		for (NewsServer* ign : *ignoreServers)
		{
			ignIds.push_back(ign->GetId());
			ignGroups.push_back(ign->GetGroup());
			ignNormLevels.push_back(ign->GetNormLevel());
		}
	}

	std::vector<size_t> unblockIndices(srvCount);
	size_t unblockCount = 0;
	size_t chosenConnIdx = 0;
	int32_t chosenLevel = 0;
	int found = nzbget_rs_server_pool_select_connection(
		level,
		m_levels.empty() ? nullptr : m_levels.data(),
		m_levels.size(),
		srvIds.data(),
		srvGroups.data(),
		srvNormLevels.data(),
		srvActive.data(),
		srvOptional.data(),
		srvBlocked.data(),
		srvCount,
		connSrvIndices.data(),
		connInUse.data(),
		connConnected.data(),
		connCount,
		wantServer ? 1 : 0,
		wantServer ? wantServer->GetId() : 0,
		wantServer ? wantServer->GetGroup() : 0,
		ignIds.empty() ? nullptr : ignIds.data(),
		ignGroups.empty() ? nullptr : ignGroups.data(),
		ignNormLevels.empty() ? nullptr : ignNormLevels.data(),
		ignIds.size(),
		rand(),
		unblockIndices.data(),
		&unblockCount,
		&chosenConnIdx,
		&chosenLevel);

	for (size_t i = 0; i < unblockCount; ++i)
	{
		m_sortedServers[unblockIndices[i]]->SetBlockTime(0);
	}

	if (found != 0)
	{
		PooledConnection* connection = m_connections[chosenConnIdx].get();
		connection->SetInUse(true);
		m_levels[chosenLevel]--;
		return connection;
	}

	return nullptr;
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

	if (connection->GetNewsServer()->GetNormLevel() > -1 && connection->GetNewsServer()->GetActive())
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
		nzbget_rs_nntp_server_pool_eval_block_server(
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
			if (!connection->GetInUse() &&
				(connection->GetNewsServer()->GetNormLevel() == -1 || !connection->GetNewsServer()->GetActive()))
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
