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


#ifndef SERVERPOOL_H

#include <algorithm>
#include <array>
#include <atomic>
#include <exception>
#include <functional>
#include <iomanip>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <vector>
#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "Connection.h"
#include "DiskState.h"
#include "DiskState.h"
#include "nzbget_rs.h"

#ifndef SERVERPOOL_H
#define SERVERPOOL_H

#ifndef NNTPCONNECTION_H
#define NNTPCONNECTION_H

class NntpConnection : public Connection
{
public:
	static constexpr int CONNECTION_LINEBUFFER_SIZE = 1024 * 10;

	NntpConnection(NewsServer* newsServer) :
		Connection(newsServer->GetHost(), newsServer->GetPort(), newsServer->GetTls()), m_newsServer(newsServer)
	{
		m_lineBuf.Reserve(CONNECTION_LINEBUFFER_SIZE);
		SetCipher(newsServer->GetCipher());
		SetIPVersion((Connection::EIPVersion)nzbget_rs_ntw_nntp_map_ip_version(
			newsServer->GetIpVersion(),
			(int)Connection::ipV4,
			(int)Connection::ipV6,
			(int)Connection::ipAuto));

#ifndef DISABLE_TLS
		SetCertVerifLevel(newsServer->GetCertVerificationLevel());
#endif
	}

	virtual bool Connect()
	{
		debug("Opening connection to %s", GetHost());

		if (m_status == csConnected)
		{
			return true;
		}
		m_authRejected = false;
		m_activeGroup = nullptr;

		if (!Connection::Connect())
		{
			return false;
		}

		// rust/src/nntp.rs: the greeting, and the login when one is set
		RsExchange exchange{this, nullptr};
		NzbgetRsNntpIo io = exchange.Io();
		int authError = m_authError, authRejected = m_authRejected;
		int result = 0;
		nzbget_rs_nntp_handshake(&io, &authError, &authRejected, &result);
		m_authError = authError;
		m_authRejected = authRejected;
		exchange.Rethrow();
		int hsStatus = nzbget_rs_dqh_nntp_conn_classify_handshake_result(result);
		if (hsStatus == 1)
		{
			Disconnect();
			return false;
		}
		return hsStatus == 0;
	}

	virtual bool Disconnect()
	{
		if (m_status == csConnected)
		{
			Request("quit\r\n");
		}
		m_activeGroup = nullptr;
		return Connection::Disconnect();
	}

	NewsServer* GetNewsServer() { return m_newsServer; }

	const char* Request(const char* req)
	{
		if (!req)
		{
			return nullptr;
		}

		// rust/src/nntp.rs: the request, and a login when the server asks for one
		RsExchange exchange{this, nullptr};
		NzbgetRsNntpIo io = exchange.Io();
		int authError = m_authError, authRejected = m_authRejected;
		char* answer = nullptr;
		nzbget_rs_nntp_request(&io, req, &authError, &authRejected, &answer);
		m_authError = authError;
		m_authRejected = authRejected;
		exchange.Rethrow();
		return answer;
	}

	const char* JoinGroup(const char* grp)
	{
		if (nzbget_rs_dqh_nntp_conn_is_already_in_group(*m_activeGroup, grp) != 0)
		{
			// already in group
			strcpy(m_lineBuf, "211 ");
			return m_lineBuf;
		}

		// rust/src/nntp.rs
		RsExchange exchange{this, nullptr};
		NzbgetRsNntpIo io = exchange.Io();
		int authError = m_authError, authRejected = m_authRejected;
		char* answer = nullptr;
		int joined = 0;
		nzbget_rs_nntp_join_group(&io, grp, &authError, &authRejected, &answer, &joined);
		m_authError = authError;
		m_authRejected = authRejected;
		exchange.Rethrow();
		if (joined)
		{
			m_activeGroup = grp;
		}
		return answer;
	}

	bool GetAuthError() { return m_authError; }
	// the server refused the login for good (502), not for now (481: too many connections)
	bool GetAuthRejected() { return m_authRejected; }

private:
	NewsServer* m_newsServer;
	CString m_activeGroup;
	CharBuffer m_lineBuf;
	bool m_authError = false;
	bool m_authRejected = false;

#ifdef DEBUG
	static constexpr bool RsDebugMessages = true;
#else
	static constexpr bool RsDebugMessages = false;
#endif

	// the connection as rust/src/nntp.rs drives it (NzbgetRsNntpIo)
	struct RsExchange
	{
		NntpConnection* connection;
		std::exception_ptr error;

		template <typename F>
		static int Call(void* ctx, F f) noexcept
		{
			RsExchange* exchange = static_cast<RsExchange*>(ctx);
			try
			{
				f(exchange->connection);
				return 0;
			}
			catch (...)
			{
				exchange->error = std::current_exception();
				return -1;
			}
		}

		NzbgetRsNntpIo Io()
		{
			NewsServer* server = connection->m_newsServer;
			return {this, RsWriteLine, RsReadLine, RsReportError, RsDebugMessages ? RsDebug : nullptr, RsCancelled, server->GetUser(),
				server->GetPassword(), server->GetName(), server->GetHost(), connection->GetHost()};
		}

		void Rethrow()
		{
			if (error)
			{
				std::rethrow_exception(error);
			}
		}
	};

	static int RsWriteLine(void* ctx, const char* line)
	{
		return RsExchange::Call(ctx, [line](NntpConnection* c) { c->WriteLine(line); });
	}

	static int RsReadLine(void* ctx, char** line)
	{
		return RsExchange::Call(ctx, [line](NntpConnection* c) { *line = c->ReadLine(c->m_lineBuf, c->m_lineBuf.Size(), nullptr); });
	}

	static int RsReportError(void* ctx, const char* prefix, const char* arg)
	{
		return RsExchange::Call(ctx, [prefix, arg](NntpConnection* c) { c->ReportError(prefix, arg, false, 0); });
	}

	static int RsDebug(void* ctx, const char* msg)
	{
		return RsExchange::Call(ctx, [msg](NntpConnection*) { debug("%s", msg); });
	}

	static int RsCancelled(void* ctx, int* cancelled)
	{
		return RsExchange::Call(ctx, [cancelled](NntpConnection* c) { *cancelled = c->GetStatus() == csCancelled; });
	}
};

#endif

class ServerPool : public Debuggable
{
public:
	typedef std::vector<NewsServer*> RawServerList;

	void SetTimeout(int timeout) { m_timeout = timeout; }
	void SetRetryInterval(int retryInterval) { m_retryInterval = retryInterval; }
	void AddServer(std::unique_ptr<NewsServer> newsServer);
	void InitConnections();
	int GetMaxNormLevel() { return m_maxNormLevel; }
	Servers* GetServers() { return &m_servers; } // Only for read access (no lockings)
	NewsServer* GetServerById(int id);
	NntpConnection* GetConnection(int level, NewsServer* wantServer, RawServerList* ignoreServers);
	void FreeConnection(NntpConnection* connection, bool used);
	void CloseUnusedConnections();
	void Changed();
	int GetGeneration() { return m_generation; }
	void BlockServer(NewsServer* newsServer);
	bool IsServerBlocked(NewsServer* newsServer);

protected:
	virtual void LogDebugInfo();

private:
	class PooledConnection : public NntpConnection
	{
	public:
		using NntpConnection::NntpConnection;
		bool GetInUse() { return m_inUse; }
		void SetInUse(bool inUse) { m_inUse = inUse; }
		time_t GetFreeTime() { return m_freeTime; }
		void SetFreeTimeNow();
	private:
		bool m_inUse = false;
		time_t m_freeTime = 0;
	};

	typedef std::vector<int> Levels;
	typedef std::vector<std::unique_ptr<PooledConnection>> Connections;

	Servers m_servers;
	RawServerList m_sortedServers;
	Connections m_connections;
	Levels m_levels;
	int m_maxNormLevel = 0;
	Mutex m_connectionsMutex;
	int m_timeout = 60;
	int m_retryInterval = 0;
	int m_generation = 0;

	void NormalizeLevels();
};

extern ServerPool* g_ServerPool;

#include <algorithm>
#include "Util.h"
#include "nzbget_rs.h"

inline constexpr int CONNECTION_HOLD_SECODNS = 5;

inline void ServerPool::PooledConnection::SetFreeTimeNow()
{
	m_freeTime = Util::CurrentTime();
}

inline NewsServer* ServerPool::GetServerById(int id)
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

inline void ServerPool::AddServer(std::unique_ptr<NewsServer> newsServer)
{
	debug("Adding server to ServerPool");

	m_servers.push_back(std::move(newsServer));
	m_sortedServers.push_back(m_servers.back().get());
}

inline void ServerPool::NormalizeLevels()
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

inline void ServerPool::InitConnections()
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

inline NntpConnection* ServerPool::GetConnection(int level, NewsServer* wantServer, RawServerList* ignoreServers)
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

inline void ServerPool::FreeConnection(NntpConnection* connection, bool used)
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

inline void ServerPool::BlockServer(NewsServer* newsServer)
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

inline bool ServerPool::IsServerBlocked(NewsServer* newsServer)
{
	if (!newsServer->GetBlockTime())
	{
		return false;
	}

	time_t curTime = Util::CurrentTime();
	return nzbget_rs_server_pool_is_blocked(newsServer->GetBlockTime(), curTime, m_retryInterval) != 0;
}

inline void ServerPool::CloseUnusedConnections()
{
	Guard guard(m_connectionsMutex);

	time_t curtime = Util::CurrentTime();

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

inline void ServerPool::Changed()
{
	debug("Server config has been changed");

	InitConnections();
	CloseUnusedConnections();
}

inline void ServerPool::LogDebugInfo()
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

#ifndef NZBGET_TLS_CIPHER_BACKEND_H
#define NZBGET_TLS_CIPHER_BACKEND_H

#if !defined(DISABLE_TLS) && (defined(__APPLE__) || defined(__linux__))
#include <openssl/rand.h>
#include <array>
#include <limits>
#include <memory>
#include <vector>

namespace TlsCipherBackend
{
	namespace Detail
	{
		constexpr int RecordSize = 16384;
		struct Context { std::string snapshot; };
		struct Crypto
		{
			const EVP_CIPHER* cipher = nullptr;
			OpenSSL::EVPCipherCtxPtr encrypt{EVP_CIPHER_CTX_new(), &EVP_CIPHER_CTX_free};
			OpenSSL::EVPCipherCtxPtr decrypt{EVP_CIPHER_CTX_new(), &EVP_CIPHER_CTX_free};
			std::array<unsigned char, 64> key{};
			std::array<unsigned char, 4> prefix{};
			std::array<unsigned char, RecordSize> plaintext{};
			std::array<unsigned char, RecordSize + EVP_MAX_BLOCK_LENGTH> output{};
			std::vector<std::array<unsigned char, RecordSize + EVP_MAX_BLOCK_LENGTH>> ciphertext;
			std::vector<std::array<unsigned char, 16>> tags;
			std::vector<std::array<unsigned char, 12>> nonces;
			uint64_t first = 0;
			int tagSize = 16;
			bool ccm = false;
			bool encryptKeyed = false, decryptKeyed = false;
			~Crypto() { OPENSSL_cleanse(key.data(), key.size()); }
			bool Init(std::string name)
			{
				if (name.ends_with("@8")) { tagSize = 8; name.resize(name.size() - 2); }
				cipher = EVP_get_cipherbyname(name.c_str());
				if (!cipher || !encrypt || !decrypt) return false;
				int mode = EVP_CIPHER_mode(cipher), keySize = EVP_CIPHER_key_length(cipher);
				ccm = mode == EVP_CIPH_CCM_MODE;
				if (mode != EVP_CIPH_GCM_MODE && !ccm && EVP_CIPHER_nid(cipher) != NID_chacha20_poly1305) return false;
				if (keySize <= 0 || keySize > static_cast<int>(key.size()) || RAND_bytes(key.data(), keySize) != 1 || RAND_bytes(prefix.data(), prefix.size()) != 1) return false;
				nzbget_rs_dqh_tls_cipher_fill_test_plaintext(plaintext.data(), RecordSize);
				return EVP_EncryptInit_ex(encrypt.get(), cipher, nullptr, nullptr, nullptr) == 1 &&
					EVP_DecryptInit_ex(decrypt.get(), cipher, nullptr, nullptr, nullptr) == 1 &&
					EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_SET_IVLEN, 12, nullptr) == 1 &&
					EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_IVLEN, 12, nullptr) == 1;
			}
			bool Prepare(uint64_t sequence, uint32_t count)
			{
				if (!nzbget_rs_dqh_tls_cipher_is_prepare_batch_valid(sequence, count)) return false;
				first = sequence; ciphertext.resize(count); tags.resize(count); nonces.resize(count);
				for (uint32_t i = 0; i < count; ++i)
				{
					auto& nonce = nonces[i];
					nzbget_rs_dqh_tls_cipher_write_record_nonce(prefix.data(), sequence + i, nonce.data());
					int size = 0, tail = 0;
					unsigned char aad[5]{23, 3, 3, 0x40, static_cast<unsigned char>(tagSize)};
					if (ccm && EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, nullptr) != 1) return false;
					if (EVP_EncryptInit_ex(encrypt.get(), nullptr, nullptr, encryptKeyed ? nullptr : key.data(), nonce.data()) != 1) return false;
					encryptKeyed = true;
					if (ccm && EVP_EncryptUpdate(encrypt.get(), nullptr, &size, nullptr, RecordSize) != 1) return false;
					if (EVP_EncryptUpdate(encrypt.get(), nullptr, &size, aad, sizeof(aad)) != 1 ||
						EVP_EncryptUpdate(encrypt.get(), ciphertext[i].data(), &size, plaintext.data(), RecordSize) != 1 || size != RecordSize ||
						EVP_EncryptFinal_ex(encrypt.get(), ciphertext[i].data() + size, &tail) != 1 || tail != 0 ||
						EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_GET_TAG, tagSize, tags[i].data()) != 1) return false;
				}
				if (Record(sequence) != RecordSize) return false;
				tags[0][0] ^= 1;
				bool rejected = Record(sequence) == -1;
				tags[0][0] ^= 1;
				ERR_clear_error();
				return rejected && Record(sequence) == RecordSize;
			}
			int Record(uint64_t sequence)
			{
				if (!nzbget_rs_dqh_tls_cipher_is_record_seq_in_range(sequence, first, ciphertext.size())) return -1;
				size_t i = static_cast<size_t>(sequence - first);
				unsigned char aad[5]{23, 3, 3, 0x40, static_cast<unsigned char>(tagSize)};
				int size = 0, tail = 0;
				if (ccm && EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, tags[i].data()) != 1) return -1;
				if (EVP_DecryptInit_ex(decrypt.get(), nullptr, nullptr, decryptKeyed ? nullptr : key.data(), nonces[i].data()) != 1) return -1;
				decryptKeyed = true;
				if (ccm && EVP_DecryptUpdate(decrypt.get(), nullptr, &size, nullptr, RecordSize) != 1) return -1;
				if (EVP_DecryptUpdate(decrypt.get(), nullptr, &size, aad, sizeof(aad)) != 1 ||
					EVP_DecryptUpdate(decrypt.get(), output.data(), &size, ciphertext[i].data(), RecordSize) != 1 || size != RecordSize) return -1;
				if (!ccm && (EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, tags[i].data()) != 1 ||
					EVP_DecryptFinal_ex(decrypt.get(), output.data() + size, &tail) != 1 || tail != 0)) return -1;
				return nzbget_rs_dqh_mem_slices_equal(output.data(), plaintext.data(), RecordSize) != 0 ? RecordSize : -1;
			}
		};

		inline int Snapshot(void* context, const unsigned char** data, size_t* length) noexcept
		{
			try
			{
				auto& value = static_cast<Context*>(context)->snapshot;
				if (!g_ScriptConfig->ReadCipherSnapshot(value)) return 0;
				*data = reinterpret_cast<const unsigned char*>(value.data()); *length = value.size(); return 1;
			}
			catch (...) { return 0; }
		}
		inline int Suites(void*, NzbgetTlsEmitSuite emit, void* context) noexcept
		{
			try
			{
				OpenSSL::SSLCtxPtr tls(SSL_CTX_new(TLS_method()), &SSL_CTX_free);
				if (!tls) return 0;
				int minimum = SSL_CTX_get_min_proto_version(tls.get()), maximum = SSL_CTX_get_max_proto_version(tls.get());
				const auto* suites = SSL_CTX_get_ciphers(tls.get());
				for (int i = 0; i < sk_SSL_CIPHER_num(suites); ++i)
				{
					const auto* suite = sk_SSL_CIPHER_value(suites, i);
					const char* suiteName = SSL_CIPHER_get_name(suite);
					bool tls13 = nzbget_rs_dqh_tls_is_tls13_version_str(SSL_CIPHER_get_version(suite)) != 0;
					int version = tls13 ? TLS1_3_VERSION : TLS1_2_VERSION;
					if (nzbget_rs_dqh_tls_is_proto_version_out_of_bounds(version, minimum, maximum) != 0) continue;
					int auth = SSL_CIPHER_get_auth_nid(suite), kx = SSL_CIPHER_get_kx_nid(suite);
					unsigned flags = nzbget_rs_dqh_tls_compute_suite_flags(
						SSL_CIPHER_is_aead(suite) ? 1 : 0,
						tls13 ? 1 : 0,
						(auth == NID_auth_rsa || auth == NID_auth_ecdsa) ? 1 : 0,
						(kx == NID_kx_ecdhe || kx == NID_kx_dhe) ? 1 : 0);
					const EVP_CIPHER* bulk = EVP_get_cipherbynid(SSL_CIPHER_get_cipher_nid(suite));
					NzbgetRsBuf bulkBuf = nzbget_rs_dqh_tls_format_bulk_cipher_name(
						bulk ? EVP_CIPHER_name(bulk) : nullptr, suiteName);
					unsigned bits = nzbget_rs_dqh_tls_resolve_cipher_bits(
						suiteName, (unsigned)SSL_CIPHER_get_bits(suite, nullptr));
					int emitted = emit(context, suiteName, bulkBuf.data ? bulkBuf.data : "", tls13 ? 13 : 12, bits, flags);
					nzbget_rs_free(bulkBuf);
					if (!emitted) return 0;
				}
				return 1;
			}
			catch (...) { return 0; }
		}
		inline void* Prepare(void*, const char* algorithm) noexcept
		{
			try { auto crypto = std::make_unique<Crypto>(); return crypto->Init(algorithm) ? crypto.release() : nullptr; }
			catch (...) { return nullptr; }
		}
		inline int PrepareBatch(void* context, uint64_t first, uint32_t count) noexcept
		{
			try { return static_cast<Crypto*>(context)->Prepare(first, count) ? 1 : 0; } catch (...) { return 0; }
		}
		inline int Record(void* context, uint64_t sequence) noexcept
		{
			try { return static_cast<Crypto*>(context)->Record(sequence); } catch (...) { return -1; }
		}
		inline void FreeCrypto(void* context) noexcept { delete static_cast<Crypto*>(context); ERR_clear_error(); }

		class ProbeConnection final : public NntpConnection
		{
		public:
			explicit ProbeConnection(NewsServer* server) : NntpConnection(server) {}
		protected:
			void PrintError(const char*) override {}
		};
		inline int Probe(void*, const NzbgetTlsServer* server, const char* cipher, int protocol, const char* numeric,
			uint64_t milliseconds, NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			try
			{
				int precheck = nzbget_rs_dqh_tls_cipher_precheck_probe_args(
					cancel(cancelContext) ? 1 : 0,
					milliseconds,
					server->verification ? 1 : 0,
					TlsSocket::HasCertificateStore() ? 1 : 0);
				if (precheck != 0) return precheck;
				auto deadline = std::make_shared<SocketDeadline>();
				deadline->end = std::chrono::steady_clock::now() + std::chrono::milliseconds(milliseconds);
				deadline->cancelled = [=] { return cancel(cancelContext) != 0; };
				NewsServer value(server->id, true, "TLS cipher probe", server->host, server->port, server->ip_version,
					server->username, server->password, false, true, cipher, 1, 0, 0, 0, false, server->verification);
				ProbeConnection connection(&value);
				std::vector<std::string> addresses;
				std::string remaining(numeric);
				for (size_t pos = 0; pos < remaining.size();)
				{
					size_t end = remaining.find('\n', pos);
					addresses.push_back(remaining.substr(pos, end == std::string::npos ? end : end - pos));
					if (end == std::string::npos) break; pos = end + 1;
				}
				if (!nzbget_rs_dqh_tls_cipher_is_probe_addr_list_valid(addresses.size())) return -2;
				connection.SetResolvedAddresses(addresses); connection.SetDeadline(deadline);
				connection.SetSuppressErrors(true); connection.SetTimeout(10);
				bool connected = connection.Connect();
				int result = nzbget_rs_dqh_tls_resolve_probe_result(
					connected ? 1 : 0,
					connection.GetNegotiatedCipher(),
					cipher,
					connection.GetNegotiatedVersion(),
					protocol,
					TLS1_3_VERSION,
					TLS1_2_VERSION,
					connection.GetAuthError() ? 1 : 0,
					connection.GetTlsProbeFailure());
				result = nzbget_rs_dqh_tls_cipher_classify_probe_deadline_override(
					result,
					cancel(cancelContext) ? 1 : 0,
					std::chrono::steady_clock::now() >= deadline->end ? 1 : 0);
				connection.SetBroken(); connection.Disconnect();
				return result;
			}
			catch (...) { ERR_clear_error(); return -1; }
		}
		inline int Commit(void*, const unsigned char* original, size_t originalLength, const unsigned char* replacement, size_t replacementLength,
			NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			try
			{
				auto result = g_ScriptConfig->CommitCipherPatch(
					std::string_view(reinterpret_cast<const char*>(original), originalLength),
					std::string_view(reinterpret_cast<const char*>(replacement), replacementLength), [=] { return cancel(cancelContext) != 0; });
				switch (result)
				{
					case ScriptConfig::CipherCommit::Saved: return 1;
					case ScriptConfig::CipherCommit::Unchanged: return 0;
					case ScriptConfig::CipherCommit::Conflict: return -2;
					case ScriptConfig::CipherCommit::Cancelled: return -3;
					default: return -1;
				}
			}
			catch (...) { return -1; }
		}
		inline void Destroy(void* context) noexcept { delete static_cast<Context*>(context); }
		inline std::string Response(NzbgetRsBuf value)
		{
			struct Release { NzbgetRsBuf value; ~Release() { nzbget_rs_free(value); } } release{value};
			return std::string(value.data, value.len);
		}
	}

	inline bool Available() { return true; }
	inline NzbgetTlsBackend Create()
	{
		return {new Detail::Context(), Detail::Snapshot, Detail::Suites, Detail::Prepare, Detail::PrepareBatch, Detail::Record, Detail::FreeCrypto, Detail::Probe, Detail::Commit, Detail::Destroy};
	}
	inline std::string Start()
	{
		NzbgetTlsBackend io = Create();
		return Detail::Response(nzbget_rs_tls_cipher_start(&io));
	}
	inline std::string Status(const char* id) { return Detail::Response(nzbget_rs_tls_cipher_status(id)); }
	inline std::string Cancel(const char* id) { return Detail::Response(nzbget_rs_tls_cipher_cancel(id)); }
	inline void Shutdown() { nzbget_rs_tls_cipher_shutdown(); }
}
#else
namespace TlsCipherBackend
{
	inline bool Available() { return false; }
	inline std::string Status(const char*) { return R"({"Available":false,"State":"unavailable","Message":"TLS cipher optimization requires a TLS-enabled Rust backend and a supported atomic configuration adapter."})"; }
	inline std::string Start() { return Status(nullptr); }
	inline std::string Cancel(const char*) { return Status(nullptr); }
	inline void Shutdown() {}
	inline NzbgetTlsBackend Create() { return {}; }
}
#endif
#endif

#ifndef STATMETER_H
#define STATMETER_H

class ServerVolume
{
public:
	struct Articles
	{
		uint32 failed;
		uint32 success;
	};
	struct Stats
	{
		uint32 bytes;
		Articles articles;
	};

	using VolumeArray = std::vector<int64>;
	using ArticlesArray = std::vector<Articles>;

	VolumeArray* BytesPerSeconds() { return &m_bytesPerSeconds; }
	VolumeArray* BytesPerMinutes() { return &m_bytesPerMinutes; }
	VolumeArray* BytesPerHours() { return &m_bytesPerHours; }
	VolumeArray* BytesPerDays() { return &m_bytesPerDays; }

	const ArticlesArray& GetArticlesPerDays() const { return m_articlesPerDays; }
	void SetArticlesPerDays(ArticlesArray articles) { m_articlesPerDays = std::move(articles); }
	void SetFirstDay(int firstDay) { m_firstDay = firstDay; }
	int GetFirstDay() { return m_firstDay; }
	void SetTotalBytes(int64 totalBytes) { m_totalBytes = totalBytes; }
	int64 GetTotalBytes() { return m_totalBytes; }
	void SetCustomBytes(int64 customBytes) { m_customBytes = customBytes; }
	int64 GetCustomBytes() { return m_customBytes; }
	int GetSecSlot() { return m_secSlot; }
	int GetMinSlot() { return m_minSlot; }
	int GetHourSlot() { return m_hourSlot; }
	int GetDaySlot() { return m_daySlot; }
	time_t GetDataTime() { return m_dataTime; }
	void SetDataTime(time_t dataTime) { m_dataTime = dataTime; }
	time_t GetCustomTime() { return m_customTime; }
	void SetCustomTime(time_t customTime) { m_customTime = customTime; }
	time_t GetCountersResetTime() { return m_countersResetTime; }
	void SetCountersResetTime(time_t time) { m_countersResetTime = time; }

	void AddStats(Stats stats);
	void CalcSlots(time_t locCurTime);
	void Reset();
	void ResetCustom();
	void LogDebugInfo();

private:
	void ResetVolume(VolumeArray& volume);
	void ResetArticles();

	VolumeArray m_bytesPerSeconds = VolumeArray(60);
	VolumeArray m_bytesPerMinutes = VolumeArray(60);
	VolumeArray m_bytesPerHours = VolumeArray(24);
	VolumeArray m_bytesPerDays;
	ArticlesArray m_articlesPerDays;

	int m_firstDay = 0;
	int64 m_totalBytes = 0;
	int64 m_customBytes = 0;

	/** Date/time when the data was last updated (time is in C/Unix format) */
	time_t m_dataTime = 0;

	/** Date/time of the last reset of custom counter (time is in C/Unix format) */
	time_t m_customTime = Util::CurrentTime();

	/** Date/time of the last reset of all counters (time is in C/Unix format) */
	time_t m_countersResetTime = Util::CurrentTime();

	int m_secSlot = 0;
	int m_minSlot = 0;
	int m_hourSlot = 0;
	int m_daySlot = 0;
};

using ServerVolumes = std::vector<ServerVolume>;
using GuardedServerVolumes = GuardedPtr<ServerVolumes>;

class StatMeter : public Debuggable
{
public:
	StatMeter();
	void Init();
	int64 CalcCurrentDownloadSpeed();
	int64 CalcMomentaryDownloadSpeed();
	void AddSpeedReading(int bytes);
	void AddServerStats(ServerVolume::Stats stats, int serverId);
	void CalcTotalStat(int* upTimeSec, int* dnTimeSec, int64* allBytes, bool* standBy);
	void CalcQuotaUsage(int64& monthBytes, int64& dayBytes);
	void IntervalCheck();
	void EnterLeaveStandBy(bool enter);
	GuardedServerVolumes GuardServerVolumes();
	// the next interval check saves the volumes (changed through the guard)
	void MarkChanged() { m_statChanged = true; }
	void Save();
	bool Load(bool* perfectServerMatch);

protected:
	virtual void LogDebugInfo();

private:
	// speed meter
	static const int SPEEDMETER_SLOTS = 30;
	static const int SPEEDMETER_SLOTSIZE = 1; //Split elapsed time into this number of secs.
	std::array<std::atomic<int>, SPEEDMETER_SLOTS> m_speedBytes;
	std::array<std::atomic<int>, SPEEDMETER_SLOTS> m_speedTime;
	std::atomic<time_t> m_speedCorrection{0};
	std::atomic<time_t> m_curSecTime{0};
	std::atomic<int64> m_speedTotalBytes{0};
	std::atomic<int> m_speedBytesIndex{0};
	std::atomic<int> m_curSecBytes{0};
	std::atomic<int> m_speedStartTime{0};
	std::mutex m_speedTotalBytesMtx;

	// time
	std::atomic<int64> m_allBytes{0};
	std::atomic<time_t> m_startServer{0};
	std::atomic<time_t> m_pausedFrom{0};
	std::atomic<time_t> m_startDownload{0};
	std::atomic<bool> m_standBy{true};
	time_t m_lastCheck = 0;
	time_t m_lastTimeOffset = 0;

	// data volume
	std::atomic<bool> m_statChanged{false};
	ServerVolumes m_serverVolumes;
	Mutex m_volumeMutex;

	void ResetSpeedStat();
	void AdjustTimeOffset();
	void CheckQuota();
	int CalcMonthSlots(const ServerVolume& volume);
};

extern StatMeter* g_StatMeter;

inline void ServerVolume::CalcSlots(time_t locCurTime)
{
	NzbgetRsVolumeSlots slots;
	nzbget_rs_volume_calc_slots(locCurTime, &m_firstDay, &slots);
	m_secSlot = slots.sec;
	m_minSlot = slots.min;
	m_hourSlot = slots.hour;
	m_daySlot = slots.day;
	if (slots.inRange)
	{
		size_t newBytesDaysLen = nzbget_rs_tpr_server_volume_required_day_len(
			m_daySlot, m_bytesPerDays.size());
		if (newBytesDaysLen > 0)
		{
			m_bytesPerDays.resize(newBytesDaysLen);
		}

		size_t newArtDaysLen = nzbget_rs_tpr_server_volume_required_day_len(
			m_daySlot, m_articlesPerDays.size());
		if (newArtDaysLen > 0)
		{
			m_articlesPerDays.resize(newArtDaysLen);
		}
	}
}

inline void ServerVolume::AddStats(Stats stats)
{
	time_t curTime = Util::CurrentTime();
	time_t locCurTime = curTime + g_WorkState->GetLocalTimeOffset();
	time_t locDataTime = m_dataTime + g_WorkState->GetLocalTimeOffset();

	int lastMinSlot = m_minSlot;
	int lastHourSlot = m_hourSlot;

	CalcSlots(locCurTime);

	int64 bytes = static_cast<int64>(stats.bytes);
	NzbgetRsVolumeSlots slots{m_secSlot, m_minSlot, m_hourSlot, m_daySlot, 0};
	nzbget_rs_volume_add(m_bytesPerSeconds.data(), m_bytesPerSeconds.size(), m_bytesPerMinutes.data(),
		m_bytesPerMinutes.size(), m_bytesPerHours.data(), m_bytesPerHours.size(), &slots, lastMinSlot,
		lastHourSlot, locCurTime, locDataTime, bytes);
	size_t daySlotIdx = m_daySlot >= 0 ? static_cast<size_t>(m_daySlot) : 0;
	int64 curDayBytes = m_daySlot >= 0 ? m_bytesPerDays[daySlotIdx] : 0;
	int curDayFailed = m_daySlot >= 0 ? m_articlesPerDays[daySlotIdx].failed : 0;
	int curDaySuccess = m_daySlot >= 0 ? m_articlesPerDays[daySlotIdx].success : 0;
	int updateDay = 0;
	int64 newDayBytes = 0, newTotalBytes = 0, newCustomBytes = 0;
	int newDayFailed = 0, newDaySuccess = 0;
	nzbget_rs_statmeter_accum_day_and_total_volume(
		m_daySlot, curDayBytes, curDayFailed, curDaySuccess,
		m_totalBytes, m_customBytes, bytes, stats.articles.failed, stats.articles.success,
		&updateDay, &newDayBytes, &newDayFailed, &newDaySuccess, &newTotalBytes, &newCustomBytes);
	if (updateDay != 0)
	{
		m_bytesPerDays[daySlotIdx] = newDayBytes;
		m_articlesPerDays[daySlotIdx].failed = newDayFailed;
		m_articlesPerDays[daySlotIdx].success = newDaySuccess;
	}
	m_totalBytes = newTotalBytes;
	m_customBytes = newCustomBytes;

	m_dataTime = curTime;
}

inline void ServerVolume::Reset()
{
	m_totalBytes = 0;
	m_dataTime = 0;
	m_firstDay = 0;
	m_countersResetTime = Util::CurrentTime();

	ResetVolume(m_bytesPerSeconds);
	ResetVolume(m_bytesPerMinutes);
	ResetVolume(m_bytesPerHours);
	ResetVolume(m_bytesPerDays);

	ResetArticles();
	ResetCustom();
}

inline void ServerVolume::ResetCustom()
{
	m_customBytes = 0;
	m_customTime = Util::CurrentTime();
}

inline void ServerVolume::ResetVolume(VolumeArray& volume)
{
	for (int64& size : volume)
	{
		size = 0;
	}
}

inline void ServerVolume::ResetArticles()
{
	for (Articles& articles : m_articlesPerDays)
	{
		articles.failed = 0;
		articles.success = 0;
	}
}

inline void ServerVolume::LogDebugInfo()
{
	info("   ---------- ServerVolume");

	NzbgetRsBuf secsBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerSeconds.data(), m_bytesPerSeconds.size());
	info("Secs: %s", secsBuf.data ? secsBuf.data : "");
	nzbget_rs_free(secsBuf);

	NzbgetRsBuf minsBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerMinutes.data(), m_bytesPerMinutes.size());
	info("Mins: %s", minsBuf.data ? minsBuf.data : "");
	nzbget_rs_free(minsBuf);

	NzbgetRsBuf hoursBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerHours.data(), m_bytesPerHours.size());
	info("Hours: %s", hoursBuf.data ? hoursBuf.data : "");
	nzbget_rs_free(hoursBuf);

	NzbgetRsBuf daysBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		m_firstDay, m_bytesPerDays.data(), m_bytesPerDays.size());
	info("Days: %s", daysBuf.data ? daysBuf.data : "");
	nzbget_rs_free(daysBuf);
}

inline StatMeter::StatMeter()
{
	debug("Creating StatMeter");

	ResetSpeedStat();
}

inline void StatMeter::Init()
{
	m_startServer = Util::CurrentTime();
	m_lastCheck = m_startServer.load();
	AdjustTimeOffset();

	m_serverVolumes.resize(1 + g_ServerPool->GetServers()->size());
}

inline void StatMeter::AdjustTimeOffset()
{
	time_t utcTime = Util::CurrentTime();
	tm tmSplittedTime;
	gmtime_r(&utcTime, &tmSplittedTime);
	tmSplittedTime.tm_isdst = -1;
	time_t locTime = mktime(&tmSplittedTime);
	int localDelta = 0;
	int totalOffset = nzbget_rs_statmeter_calc_local_time_offset(
		(int64_t)utcTime, (int64_t)locTime, g_Options->GetTimeCorrection(), &localDelta);
	g_WorkState->SetLocalTimeOffset(totalOffset);
	m_lastTimeOffset = utcTime;

	debug("UTC delta: %i (%i+%i)", g_WorkState->GetLocalTimeOffset(), localDelta, g_Options->GetTimeCorrection());
}

inline void StatMeter::IntervalCheck()
{
	time_t m_curTime = Util::CurrentTime();
	int64 clockDelta = 0;
	int adjustDownload = 0;
	int adjustTz = 0;
	if (nzbget_rs_sus_eval_interval_check(
		static_cast<int64>(m_curTime),
		static_cast<int64>(m_lastCheck),
		static_cast<int64>(m_lastTimeOffset),
		static_cast<int64>(m_startDownload.load()),
		m_standBy ? 1 : 0,
		&clockDelta,
		&adjustDownload,
		&adjustTz))
	{
		m_startServer += static_cast<time_t>(clockDelta);
		if (adjustDownload)
		{
			m_startDownload += static_cast<time_t>(clockDelta);
		}
	}
	if (adjustTz)
	{
		AdjustTimeOffset();
	}

	m_lastCheck = m_curTime;

	CheckQuota();

	if (m_statChanged)
	{
		Save();
	}
}

inline void StatMeter::EnterLeaveStandBy(bool enter)
{
	m_standBy = enter;
	if (enter)
	{
		m_pausedFrom = Util::CurrentTime();
	}
	else
	{
		time_t startDownload = 0;
		if (!m_startDownload.compare_exchange_strong(startDownload, Util::CurrentTime()))
		{
			m_startDownload = (time_t)nzbget_rs_statmeter_calc_resumed_download_start(
				(int64_t)m_startDownload.load(),
				(int64_t)Util::CurrentTime(),
				(int64_t)m_pausedFrom.load());
		}

		m_pausedFrom = 0;
		ResetSpeedStat();
	}
}

inline void StatMeter::CalcTotalStat(int* upTimeSec, int* dnTimeSec, int64* allBytes, bool* standBy)
{
	time_t startServer = m_startServer.load();
	*standBy = m_standBy;
	nzbget_rs_sus_calc_total_stat(
		static_cast<int64>(Util::CurrentTime()),
		static_cast<int64>(startServer),
		static_cast<int64>(m_startDownload.load()),
		static_cast<int64>(m_pausedFrom.load()),
		*standBy ? 1 : 0,
		upTimeSec,
		dnTimeSec);
	*allBytes = m_allBytes;
}

inline int64 StatMeter::CalcCurrentDownloadSpeed()
{
	if (m_standBy)
	{
		return 0;
	}
	return nzbget_rs_sus_calc_current_download_speed(
		0,
		static_cast<int64>(Util::CurrentTime()),
		m_speedStartTime.load(),
		m_speedTotalBytes.load());
}

inline int64 StatMeter::CalcMomentaryDownloadSpeed()
{
	time_t curTime = Util::CurrentTime();
	return nzbget_rs_sus_calc_momentary_download_speed(
		static_cast<int64>(curTime),
		static_cast<int64>(m_curSecTime.load()),
		m_curSecBytes.load());
}

inline void StatMeter::AddSpeedReading(int bytes)
{
	std::lock_guard<std::mutex> guard{m_speedTotalBytesMtx};

	time_t curTime = Util::CurrentTime();
	int64 curSecTime = static_cast<int64>(m_curSecTime.load());
	int64 curSecBytes = m_curSecBytes.load();
	int32_t speedBytes[SPEEDMETER_SLOTS];
	int32_t speedTime[SPEEDMETER_SLOTS];
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		speedBytes[i] = m_speedBytes[i].load();
		speedTime[i] = m_speedTime[i].load();
	}
	int speedBytesIndex = m_speedBytesIndex.load();
	int64 speedTotalBytes = m_speedTotalBytes.load();
	int speedStartTime = m_speedStartTime.load();
	int64 speedCorrection = static_cast<int64>(m_speedCorrection);
	int64 allBytes = m_allBytes.load();

	nzbget_rs_sus_add_speed_reading(
		bytes,
		static_cast<int64>(curTime),
		&curSecTime,
		&curSecBytes,
		speedBytes,
		speedTime,
		&speedBytesIndex,
		&speedTotalBytes,
		&speedStartTime,
		&speedCorrection,
		&allBytes);

	m_curSecTime = static_cast<time_t>(curSecTime);
	m_curSecBytes = curSecBytes;
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		m_speedBytes[i] = speedBytes[i];
		m_speedTime[i] = speedTime[i];
	}
	m_speedBytesIndex = speedBytesIndex;
	m_speedTotalBytes = speedTotalBytes;
	m_speedStartTime = speedStartTime;
	m_speedCorrection = static_cast<time_t>(speedCorrection);
	m_allBytes = allBytes;
}

inline void StatMeter::ResetSpeedStat()
{
	time_t curTime = Util::CurrentTime();
	m_speedStartTime = (int)curTime / SPEEDMETER_SLOTSIZE - 1;
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		m_speedBytes[i] = 0;
		m_speedTime[i] = m_speedStartTime.load();
	}
	m_speedBytesIndex = 0;
	m_speedTotalBytes = 0;
	m_speedCorrection = curTime;
	m_curSecTime =	0;
	m_curSecBytes = 0;
}

inline void StatMeter::LogDebugInfo()
{
	info("   ---------- SpeedMeter");
	int32_t speed = 0, timeDiff = 0;
	nzbget_rs_dqh_statmeter_calc_debug_speed_and_timediff(
		CalcCurrentDownloadSpeed(), (int)Util::CurrentTime(),
		m_speedStartTime.load(), SPEEDMETER_SLOTSIZE, &speed, &timeDiff);
	info("      Speed: %i", speed);
	info("      SpeedStartTime: %i", m_speedStartTime.load());
	info("      SpeedTotalBytes: %" PRIi64, m_speedTotalBytes.load());
	info("      SpeedBytesIndex: %i", m_speedBytesIndex.load());
	info("      AllBytes: %" PRIi64, m_allBytes.load());
	info("      Time: %i", (int)Util::CurrentTime());
	info("      TimeDiff: %i", timeDiff);
	for (int i=0; i < SPEEDMETER_SLOTS; i++)
	{
		info("      Bytes[%i]: %i, Time[%i]: %i", i, m_speedBytes[i].load(), i, m_speedTime[i].load());
	}

	Guard guard(m_volumeMutex);
	int index = 0;
	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		info("      ServerVolume %i", index);
		serverVolume.LogDebugInfo();
		index++;
	}
}

inline void StatMeter::AddServerStats(ServerVolume::Stats stats, int serverId)
{
	Guard guard(m_volumeMutex);
	m_serverVolumes[0].AddStats(stats);
	m_serverVolumes[serverId].AddStats(stats);
	m_statChanged = true;
}

inline GuardedServerVolumes StatMeter::GuardServerVolumes()
{
	GuardedServerVolumes serverVolumes(&m_serverVolumes, &m_volumeMutex);

	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		ServerVolume::Stats stats;
		stats.bytes = 0;
		stats.articles.failed = 0;
		stats.articles.success = 0;
		serverVolume.AddStats(stats);
	}

	return serverVolumes;
}

inline void StatMeter::Save()
{
	if (!g_Options->GetServerMode())
	{
		return;
	}

	Guard guard(m_volumeMutex);
	g_DiskState->SaveStats(g_ServerPool->GetServers(), &m_serverVolumes);
	m_statChanged = false;
}

inline bool StatMeter::Load(bool* perfectServerMatch)
{
	Guard guard(m_volumeMutex);

	bool ok = g_DiskState->LoadStats(g_ServerPool->GetServers(), &m_serverVolumes, perfectServerMatch);

	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		serverVolume.CalcSlots(serverVolume.GetDataTime() + g_WorkState->GetLocalTimeOffset());
	}

	return ok;
}

inline void StatMeter::CheckQuota()
{
	if (!nzbget_rs_dqh_statmeter_has_any_quota_configured(
		g_Options->GetDailyQuota(), g_Options->GetMonthlyQuota()))
	{
		return;
	}

	int64 monthBytes, dayBytes;
	CalcQuotaUsage(monthBytes, dayBytes);

	int action = 0;
	bool quotaReached = nzbget_rs_sus_eval_quota_status(
		g_Options->GetMonthlyQuota(),
		g_Options->GetDailyQuota(),
		monthBytes,
		dayBytes,
		g_WorkState->GetQuotaReached() ? 1 : 0,
		&action) != 0;
	if (action == 1)
	{
		warn("Monthly quota reached at %s", *Util::FormatSize(monthBytes));
	}
	else if (action == 2)
	{
		warn("Daily quota reached at %s", *Util::FormatSize(dayBytes));
	}
	else if (action == 3)
	{
		info("Quota lifted");
	}
	g_WorkState->SetQuotaReached(quotaReached);
}

inline void StatMeter::CalcQuotaUsage(int64& monthBytes, int64& dayBytes)
{
	Guard guard(m_volumeMutex);

	ServerVolume totalVolume = m_serverVolumes[0];

	time_t locTime = Util::CurrentTime() + g_WorkState->GetLocalTimeOffset();
	int elapsedSlots = CalcMonthSlots(totalVolume);
	nzbget_rs_sus_calc_quota_usage(
		static_cast<int64>(locTime),
		totalVolume.GetFirstDay(),
		totalVolume.BytesPerDays()->data(),
		totalVolume.BytesPerDays()->size(),
		elapsedSlots,
		&monthBytes,
		&dayBytes);
	debug("month volume: %i MB", nzbget_rs_dqh_statmeter_bytes_to_mb_i32(monthBytes));
}

inline int StatMeter::CalcMonthSlots(const ServerVolume&)
{
	time_t locCurTime = Util::CurrentTime() + g_WorkState->GetLocalTimeOffset();
	return nzbget_rs_statmeter_calc_month_slots(
		static_cast<int64_t>(locCurTime), g_Options->GetQuotaStartDay());
}

#endif

#ifndef ARTICLEWRITER_H
#define ARTICLEWRITER_H

class CachedSegmentData final : public SegmentData
{
public:
	CachedSegmentData() {}
	CachedSegmentData(char* data, int size) : m_data(data), m_size(size) {}
	CachedSegmentData(const CachedSegmentData&) = delete;
	CachedSegmentData(CachedSegmentData&& other) :
		m_data(other.m_data), m_size(other.m_size) { other.m_data = nullptr; other.m_size = 0; }
	CachedSegmentData& operator=(CachedSegmentData&& other);
	~CachedSegmentData() override;
	char* GetData() override { return m_data; }

private:
	char* m_data = nullptr;
	int m_size = 0;

	friend class ArticleCache;
};

class ArticleWriter
{
public:
	// more bytes decoded than the article's range holds (they weren't written)
	bool GetRangeExceeded() { return m_articlePtr > m_articleSize; }
	struct OutputPaths 
	{
	    std::string finalPath;
	    std::string tempPath;
	};
	void SetInfoName(const char* infoName) { m_infoName = infoName ? infoName : ""; }
	void SetInfoName(std::string infoName) { m_infoName = std::move(infoName); }
	void SetFileInfo(FileInfo* fileInfo) { m_fileInfo = fileInfo; }
	void SetArticleInfo(ArticleInfo* articleInfo) { m_articleInfo = articleInfo; }
	void Prepare();
	bool Start(Decoder::EFormat format, const char* filename, int64 fileSize, int64 articleOffset, int articleSize);
	bool Write(char* buffer, int len);
	void Finish(bool success);
	bool GetDuplicate() { return m_duplicate; }
	bool HasCachedBuffer() { return m_articleData.GetData() != nullptr; }
	void LogStartMessage(std::string_view infoFilename, bool directWrite, bool cached);
	std::optional<OutputPaths> SetupOutputFile(DiskFile &outfile,
											std::string_view destDir,
											std::string_view filename,
											std::string_view infoFilename,
											bool directWrite,
											bool cached);
	uint32 ProcessArticles(DiskFile& outfile,
						std::string_view infoFilename,
						std::string_view finalOutputPath,
						bool directWrite,
						bool cached);
	bool CommitDiskFile(DiskFile& outfile,
						std::string_view tempDestPath,
						std::string_view finalOutputPath,
						bool directWrite);
	bool CleanupOldData(bool directWrite,
						std::string_view nzbDestDir,
						std::string_view finalOutputPath);
	void ReportCompletionStatus(std::string_view infoFilename);
	void HandlePostProcessing(uint32 crc, 
							std::string_view currentPath, 
							std::string_view originalFilename, 
							std::string_view nzbDestDir);
	bool CompleteFileParts();
	static bool MoveCompletedFiles(NzbInfo* nzbInfo, const char* oldDestDir);
	void FlushCache();

private:
	bool IsDupeFallbackArticle() const;
	bool GetSkipDiskWrite();

	FileInfo* m_fileInfo;
	ArticleInfo* m_articleInfo;
	DiskFile m_outFile;
	std::string m_tempFilename;
	std::string m_outputFilename;
	std::string m_resultFilename;
	std::string m_infoName;
	Decoder::EFormat m_format = Decoder::efUnknown;
	CachedSegmentData m_articleData;
	int64 m_articleOffset;
	int m_articleSize;
	int m_articlePtr;
	bool m_duplicate = false;
	bool m_commitError = false;

	bool CreateOutputFile(int64 size);
	void BuildOutputFilename();
	void SetWriteBuffer(DiskFile& outFile, int recSize);
};

class ArticleCache final : public Thread
{
public:
	class FlushGuard
	{
	public:
		FlushGuard(FlushGuard&& other) = default;
		~FlushGuard();
	private:
		Guard m_guard;
		FlushGuard(Mutex& mutex);
		friend class ArticleCache;
	};

	void Run() override;
	void Stop() override;
	CachedSegmentData Alloc(int size);
	bool Realloc(CachedSegmentData* segment, int newSize);
	void Free(CachedSegmentData* segment);
	FlushGuard GuardFlush() { return FlushGuard(m_flushMutex); }
	Guard GuardContent() { return Guard(m_contentMutex); }
	bool GetFlushing() { return m_flushing; }
	size_t GetAllocated() { return m_allocated; }
	bool FileBusy(FileInfo* fileInfo) { return fileInfo == m_fileInfo; }
	void WakeupFlush()
	{
		std::lock_guard<std::mutex> guard(m_allocMutex);
		m_allocCond.notify_all();
	}

private:
	std::atomic<size_t> m_allocated{0};
	FileInfo* m_fileInfo = nullptr;
	std::mutex m_allocMutex;
	std::condition_variable m_allocCond;
	Mutex m_flushMutex;
	Mutex m_contentMutex;
	bool m_flushing = false;

	bool CheckFlush(bool flushEverything);
};

extern ArticleCache* g_ArticleCache;

inline CachedSegmentData::~CachedSegmentData()
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
}

inline CachedSegmentData& CachedSegmentData::operator=(CachedSegmentData&& other)
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
	m_data = other.m_data;
	m_size = other.m_size;
	other.m_data = nullptr;
	other.m_size = 0;
	return *this;
}


inline void ArticleWriter::SetWriteBuffer(DiskFile& outFile, int recSize)
{
	int bufSize = nzbget_rs_qhp_calc_write_buffer_size(g_Options->GetWriteBuffer(), recSize);
	if (bufSize > 0)
	{
		outFile.SetWriteBuffer(bufSize);
	}
}

inline void ArticleWriter::Prepare()
{
	BuildOutputFilename();
	m_resultFilename = m_articleInfo->GetResultFilename()
		? m_articleInfo->GetResultFilename()
		: "";
}

inline bool ArticleWriter::IsDupeFallbackArticle() const
{
	return nzbget_rs_dqh_article_writer_is_dupe_fallback_article(
		m_articleInfo ? 1 : 0,
		m_articleInfo ? m_articleInfo->GetDupeFallbackRound() : 0) != 0;
}

inline bool ArticleWriter::Start(Decoder::EFormat format, const char* filename, int64 fileSize,
	int64 articleOffset, int articleSize)
{
	m_outFile.Close();
	m_format = format;
	m_articleOffset = articleOffset;
	m_articleSize = articleSize > 0 ? articleSize : m_articleInfo->GetSize();
	m_articlePtr = 0;

	// prepare file for writing
	if (m_format == Decoder::efYenc)
	{
		if (g_Options->GetDupeCheck() &&
			m_fileInfo->GetNzbInfo()->GetDupeMode() != dmForce &&
			!m_fileInfo->GetNzbInfo()->GetManyDupeFiles())
		{
			bool outputInitialized;
			{
				Guard guard = m_fileInfo->GuardOutputFile();
				outputInitialized = m_fileInfo->GetOutputInitialized();
				if (!g_Options->GetDirectWrite() && !m_fileInfo->GetForceDirectWrite())
				{
					m_fileInfo->SetOutputInitialized(true);
				}
			}

			BString<1024> destFilePath;
			if (filename)
			{
				NzbgetRsBuf dfpBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					m_fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, filename);
				destFilePath.Set(dfpBuf.data ? dfpBuf.data : "", (int)dfpBuf.len);
				nzbget_rs_free(dfpBuf);
			}
			bool isOwnOutputFile = false;
			if (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite())
			{
				Guard guard = m_fileInfo->GuardOutputFile();
				isOwnOutputFile = m_fileInfo->GetForceDirectWrite() ||
					(!m_fileInfo->GetOutputFilename().empty() &&
					 FileSystem::SameFilename(m_fileInfo->GetOutputFilename().c_str(), destFilePath));
			}

			if (!outputInitialized && !isOwnOutputFile && filename &&
				FileSystem::FileExists(destFilePath))
			{
				m_duplicate = true;
				return false;
			}
		}

		if (g_Options->GetDirectWrite() && !IsDupeFallbackArticle())
		{
			Guard guard = m_fileInfo->GuardOutputFile();
			if (!m_fileInfo->GetOutputInitialized() ||
				(!GetSkipDiskWrite() && !FileSystem::FileExists(m_outputFilename.c_str())))
			{
				if (!GetSkipDiskWrite() && !CreateOutputFile(fileSize))
				{
					return false;
				}
				m_fileInfo->SetOutputInitialized(true);
			}
		}
	}

	// allocate cache buffer; yEnc only: a cached article is written at its offset,
	bool isCritical = g_MemoryMonitor &&
		g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL;

	// allocate cache buffer; yEnc only: a cached article is written at its offset,
	// and only yEnc tells it (uuencoded parts all have 0 and overwrote each other,
	// uncached they're appended in order)
	if (g_Options->GetArticleCache() > 0 && !g_Options->GetRawArticle() &&
		m_format == Decoder::efYenc && !IsDupeFallbackArticle() && !isCritical)
	{
		m_articleData = g_ArticleCache->Alloc(m_articleSize);

		while (!m_articleData.GetData() && g_ArticleCache->GetFlushing() &&
			!(g_MemoryMonitor && g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL))
		{
			Util::Sleep(5);
			m_articleData = g_ArticleCache->Alloc(m_articleSize);
		}

		if (!m_articleData.GetData())
		{
			detail("Article cache is full, using disk for %s", m_infoName.c_str());
		}
	}

	if (!m_articleData.GetData())
	{
		if (GetSkipDiskWrite())
		{
			return true;
		}

		bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
			m_format == Decoder::efYenc && !IsDupeFallbackArticle();
		const char* outFilename = directWrite ? m_outputFilename.c_str() : m_tempFilename.c_str();
		if (!m_outFile.Open(outFilename, directWrite ? DiskFile::omReadWrite : DiskFile::omWrite))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
				"Could not %s file %s: %s", nzbget_rs_dqh_article_writer_open_verb(directWrite ? 1 : 0), outFilename,
				*FileSystem::GetLastErrorMessage());
			return false;
		}
		SetWriteBuffer(m_outFile, m_articleInfo->GetSize());

		if (directWrite)
		{
			m_outFile.Seek(m_articleOffset);
		}
	}

	return true;
}

inline bool ArticleWriter::GetSkipDiskWrite()
{
	return (m_fileInfo && m_fileInfo->GetNzbInfo() && m_fileInfo->GetNzbInfo()->GetSkipDiskWrite()) ||
		g_Options->GetSkipWrite();
}

inline bool ArticleWriter::Write(char* buffer, int len)
{
	if (nzbget_rs_awc_article_writer_advance_and_check_range(
		g_Options->GetRawArticle() ? 1 : 0,
		&m_articlePtr,
		len,
		m_articleSize) != 0)
	{
		return true;
	}

	if (!g_Options->GetRawArticle() && m_articleData.GetData())
	{
		memcpy(m_articleData.GetData() + m_articlePtr - len, buffer, len);
		return true;
	}

	if (GetSkipDiskWrite())
	{
		return true;
	}

	return m_outFile.Write(buffer, len) > 0;
}

inline void ArticleWriter::Finish(bool success)
{
	m_outFile.Close();

	if (!success)
	{
		if (!GetSkipDiskWrite())
		{
			FileSystem::DeleteFile(m_tempFilename.c_str());
			FileSystem::DeleteFile(m_resultFilename.c_str());
		}
		return;
	}

	bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
		m_format == Decoder::efYenc && !IsDupeFallbackArticle();

	if (!g_Options->GetRawArticle())
	{
		if (!directWrite && !m_articleData.GetData() && !GetSkipDiskWrite())
		{
			if (!FileSystem::MoveFile(m_tempFilename.c_str(), m_resultFilename.c_str()))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Could not rename file %s to %s: %s", m_tempFilename.c_str(), m_resultFilename.c_str(),
					*FileSystem::GetLastErrorMessage());
			}
			FileSystem::DeleteFile(m_tempFilename.c_str());
		}

		if (m_articleData.GetData())
		{
			if (m_articleSize != m_articlePtr)
			{
				g_ArticleCache->Realloc(&m_articleData, m_articlePtr);
			}
			Guard contentGuard = g_ArticleCache->GuardContent();
			m_articleInfo->AttachSegment(std::make_unique<CachedSegmentData>(std::move(m_articleData)), m_articleOffset, m_articlePtr);
			m_fileInfo->SetCachedArticles(m_fileInfo->GetCachedArticles() + 1);
		}
		else
		{
			m_articleInfo->SetSegmentOffset(m_articleOffset);
			m_articleInfo->SetSegmentSize(m_articlePtr);
		}
	}
	else
	{
		// rawmode
		if (!GetSkipDiskWrite())
		{
			if (!FileSystem::MoveFile(m_tempFilename.c_str(), m_resultFilename.c_str()))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Could not move file %s to %s: %s", m_tempFilename.c_str(), m_resultFilename.c_str(),
					*FileSystem::GetLastErrorMessage());
			}
		}
	}
}

/* creates output file and subdirectores */
inline bool ArticleWriter::CreateOutputFile(int64 size)
{
	bool outExists = FileSystem::FileExists(m_outputFilename.c_str());
	int existingStatus = nzbget_rs_dqh_article_writer_check_existing_output_file(
		outExists ? 1 : 0,
		outExists ? FileSystem::FileSize(m_outputFilename.c_str()) : 0,
		size);
	if (existingStatus == 1)
	{
		// keep existing old file from previous program session
		return true;
	}
	if (existingStatus == 2)
	{
		// delete existing old file from previous program session
		FileSystem::DeleteFile(m_outputFilename.c_str());
	}

	// ensure the directory exist
	BString<1024> destDir;
	destDir.Set(m_outputFilename.c_str(), (int)(FileSystem::BaseFileName(m_outputFilename.c_str()) - m_outputFilename.c_str()));
	CString errmsg;

	if (!FileSystem::ForceDirectories(destDir, errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not create directory %s: %s", *destDir, *errmsg);
		return false;
	}

	if (!FileSystem::AllocateFile(m_outputFilename.c_str(), size, g_Options->GetArticleCache() == 0, errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not create file %s: %s", m_outputFilename.c_str(), *errmsg);
		return false;
	}

	return true;
}

inline void ArticleWriter::BuildOutputFilename()
{
	if (!m_fileInfo || !m_articleInfo)
		return;

	const char* tmpDir = g_Options->GetTempDir();
	const char* destDir = m_fileInfo->GetNzbInfo()->GetDestDir();
	const bool directWrite = g_Options->GetDirectWrite();
	const bool forceDirectWrite = m_fileInfo->GetForceDirectWrite();
	std::optional<Guard> guard;
	if (directWrite || forceDirectWrite)
	{
		guard.emplace(m_fileInfo->GuardOutputFile());
	}
	const std::string& existingOutputFilename = m_fileInfo->GetOutputFilename();

	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};

	NzbgetRsBuf resultBuf{};
	NzbgetRsBuf tempBuf{};
	NzbgetRsBuf outputBuf{};
	int hasOutputFilename = 0;
	int updateFileInfoOutput = 0;
	if (!nzbget_rs_awc_article_writer_build_output_filenames(
		tmpDir,
		destDir,
		PATH_SEPARATOR,
		m_fileInfo->GetId(),
		m_articleInfo->GetPartNumber(),
		directWrite ? 1 : 0,
		forceDirectWrite ? 1 : 0,
		existingOutputFilename.c_str(),
		m_fileInfo->GetOutputInitialized() ? 1 : 0,
		m_fileInfo->GetFilename(),
		nullptr,
		fileExistsCb,
		&resultBuf,
		&tempBuf,
		&outputBuf,
		&hasOutputFilename,
		&updateFileInfoOutput))
	{
		return;
	}

	std::string resultFilename(resultBuf.data ? resultBuf.data : "", resultBuf.len);
	nzbget_rs_free(resultBuf);
	m_articleInfo->SetResultFilename(resultFilename.c_str());

	m_tempFilename.assign(tempBuf.data ? tempBuf.data : "", tempBuf.len);
	nzbget_rs_free(tempBuf);

	if (hasOutputFilename)
	{
		m_outputFilename.assign(outputBuf.data ? outputBuf.data : "", outputBuf.len);
		if (updateFileInfoOutput)
		{
			m_fileInfo->SetOutputFilename(m_outputFilename);
		}
	}
	nzbget_rs_free(outputBuf);
}

inline bool ArticleWriter::CompleteFileParts()
{
	debug("Completing file parts");
	m_commitError = false;
	debug("ArticleFilename: %s", m_fileInfo->GetFilename());

	// 1. Gather context & configuration
	std::string nzbDestDir;
	std::string nzbName;
	std::string filename;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		nzbName = m_fileInfo->GetNzbInfo()->GetName();
		nzbDestDir = m_fileInfo->GetNzbInfo()->GetDestDir();
		filename = m_fileInfo->GetFilename();
	}

	if (m_outputFilename.empty() && !m_fileInfo->GetOutputFilename().empty())
	{
		m_outputFilename = m_fileInfo->GetOutputFilename();
	}
	bool directWriteOpt = g_Options->GetDirectWrite();
	bool forceDirectWrite = m_fileInfo->GetForceDirectWrite();
	bool curOutputInit = m_fileInfo->GetOutputInitialized();
	bool hasOwnOut = false;
	bool hasForceDest = false;
	if ((directWriteOpt || forceDirectWrite) && !curOutputInit)
	{
		BString<1024> destPath;
		{
			NzbgetRsBuf dpBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbDestDir.c_str(), PATH_SEPARATOR, filename.c_str());
			destPath.Set(dpBuf.data ? dpBuf.data : "", (int)dpBuf.len);
			nzbget_rs_free(dpBuf);
		}
		hasOwnOut = !m_outputFilename.empty() && FileSystem::FileExists(m_outputFilename.c_str());
		hasForceDest = !hasOwnOut && forceDirectWrite && FileSystem::FileExists(destPath);
	}
	int newOutputInit = curOutputInit ? 1 : 0;
	bool directWrite = nzbget_rs_awc_article_writer_resolve_direct_write_ready(
		directWriteOpt ? 1 : 0,
		forceDirectWrite ? 1 : 0,
		curOutputInit ? 1 : 0,
		hasOwnOut ? 1 : 0,
		hasForceDest ? 1 : 0,
		&newOutputInit) != 0;
	if (newOutputInit && !curOutputInit)
	{
		m_fileInfo->SetOutputInitialized(true);
	}
	bool cached = m_fileInfo->GetCachedArticles() > 0;

	std::string fullInfoPath;
	{
		NzbgetRsBuf infoPathBuf = nzbget_rs_dqh_article_writer_format_info_path(
			nzbName.c_str(), PATH_SEPARATOR, filename.c_str());
		if (infoPathBuf.data)
		{
			fullInfoPath.assign(infoPathBuf.data, infoPathBuf.len);
		}
		nzbget_rs_free(infoPathBuf);
	}
	LogStartMessage(fullInfoPath, directWrite, cached);

	// 2. Return if no successful articles
	if (m_fileInfo->GetSuccessArticles() == 0)
	{
		detail("Skipping file parts completion for %s: no successful articles", fullInfoPath.c_str());
		if (!GetSkipDiskWrite())
		{
			CleanupOldData(directWrite, nzbDestDir, "");
		}
		ReportCompletionStatus(fullInfoPath);
		return true;
	}

	// 3. Skip all filesystem work when SkipWrite is active
	if (GetSkipDiskWrite())
	{
		ReportCompletionStatus(fullInfoPath);
		// Update in-memory FileInfo state only
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_fileInfo->SetCrc(0); // CRC not computed in SkipWrite mode
			m_fileInfo->SetOutputFilename("");
		}
		return true;
	}

	// 4. Prepare directory
	CString errmsg;
	if (!FileSystem::ForceDirectories(nzbDestDir.c_str(), errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s", nzbDestDir.c_str(), *errmsg);
		return false;
	}

	// 5. Setup output file
	DiskFile outfile;
	auto pathsOpt = SetupOutputFile(outfile, nzbDestDir, filename, fullInfoPath, directWrite, cached);
	if (!pathsOpt)
	{
		return false;
	}

	const auto& [finalOutputPath, tempDestPath] = *pathsOpt;

	// 6. Process data (Write/Join/Move Articles)
	uint32 crc = ProcessArticles(outfile, fullInfoPath, finalOutputPath, directWrite, cached);
	if (m_commitError)
	{
		outfile.Close();
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not commit all article data for %s", fullInfoPath.c_str());
		return false;
	}

	// 7. Finalize file (Close & Move Temp to Final)
	if (!CommitDiskFile(outfile, tempDestPath, finalOutputPath, directWrite))
	{
		return false;
	}

	// 8. Cleanup (Delete parts or old dirs)
	if (!CleanupOldData(directWrite, nzbDestDir, finalOutputPath))
	{
		return false;
	}

	// 9. Report results
	ReportCompletionStatus(fullInfoPath);

	// 10. Post Processing (Renames, Hardlinks, Moves)
	HandlePostProcessing(crc, finalOutputPath, filename, nzbDestDir);
	return true;
}

inline void ArticleWriter::LogStartMessage(std::string_view infoFilename, bool directWrite, bool cached)
{
	if (g_Options->GetRawArticle())
	{
		detail("Moving articles for %s", infoFilename.data());
	}
	else if (directWrite && cached)
	{
		detail("Writing articles for %s", infoFilename.data());
	}
	else if (directWrite)
	{
		detail("Checking articles for %s", infoFilename.data());
	}
	else
	{
		detail("Joining articles for %s", infoFilename.data());
	}
}

inline std::optional<ArticleWriter::OutputPaths>
ArticleWriter::SetupOutputFile(DiskFile &outfile,
							std::string_view destDir,
							std::string_view filename,
							std::string_view infoFilename,
							bool directWrite,
							bool cached)
{
	struct SetupCtx {
		DiskFile* outfile;
		CString forceDirsErr;
	} ctx{&outfile, {}};

	auto sameFilenameCb = [](void*, const char* a, const char* b) -> int {
		return FileSystem::SameFilename(a, b) ? 1 : 0;
	};
	auto makeUniqueCb = [](void*, const char* dir, const char* fname, char* outBuf, size_t outCap) -> size_t {
		CString uniq = FileSystem::MakeUniqueFilename(dir, fname);
		size_t len = uniq.Length();
		if (outBuf && outCap > 0)
		{
			size_t copyLen = len < outCap ? len : outCap - 1;
			memcpy(outBuf, *uniq, copyLen);
			outBuf[copyLen] = '\0';
		}
		return len;
	};
	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	auto forceDirsCb = [](void* rawCtx, const char* path) -> int {
		auto* c = static_cast<SetupCtx*>(rawCtx);
		return FileSystem::ForceDirectories(path, c->forceDirsErr) ? 1 : 0;
	};
	auto deleteFileCb = [](void*, const char* path) -> int {
		return FileSystem::DeleteFile(path) ? 1 : 0;
	};
	auto openFileCb = [](void* rawCtx, const char* path, int readWrite) -> int {
		auto* c = static_cast<SetupCtx*>(rawCtx);
		return c->outfile->Open(path, readWrite ? DiskFile::omReadWrite : DiskFile::omWrite) ? 1 : 0;
	};
	auto createDirCb = [](void*, const char* path) -> int {
		return FileSystem::CreateDirectory(path) ? 1 : 0;
	};

	NzbgetRsBuf finalBuf{};
	NzbgetRsBuf tempBuf{};
	NzbgetRsBuf updatedOutBuf{};
	NzbgetRsBuf parentFailedBuf{};
	NzbgetRsBuf errPathBuf{};
	int hasParentFailed = 0;
	int errKind = 0;
	int ok = nzbget_rs_awc_article_writer_execute_setup_output_file(
		destDir.data(),
		destDir.size(),
		filename.data(),
		filename.size(),
		PATH_SEPARATOR,
		g_Options->GetRawArticle() ? 1 : 0,
		directWrite ? 1 : 0,
		m_fileInfo->GetForceDirectWrite() ? 1 : 0,
		m_outputFilename.data(),
		m_outputFilename.size(),
		&ctx,
		sameFilenameCb,
		makeUniqueCb,
		fileExistsCb,
		forceDirsCb,
		deleteFileCb,
		openFileCb,
		createDirCb,
		&finalBuf,
		&tempBuf,
		&updatedOutBuf,
		&parentFailedBuf,
		&hasParentFailed,
		&errKind,
		&errPathBuf);

	OutputPaths paths;
	paths.finalPath.assign(finalBuf.data ? finalBuf.data : "", finalBuf.len);
	nzbget_rs_free(finalBuf);
	paths.tempPath.assign(tempBuf.data ? tempBuf.data : "", tempBuf.len);
	nzbget_rs_free(tempBuf);
	m_outputFilename.assign(updatedOutBuf.data ? updatedOutBuf.data : "", updatedOutBuf.len);
	nzbget_rs_free(updatedOutBuf);
	std::string parentFailed(parentFailedBuf.data ? parentFailedBuf.data : "", parentFailedBuf.len);
	nzbget_rs_free(parentFailedBuf);
	std::string errPath(errPathBuf.data ? errPathBuf.data : "", errPathBuf.len);
	nzbget_rs_free(errPathBuf);

	if (hasParentFailed)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s",
			parentFailed.c_str(), *ctx.forceDirsErr);
	}

	if (!ok)
	{
		if (errKind == 1)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create file %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		else if (errKind == 2)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not open file %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		else if (errKind == 3)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		return std::nullopt;
	}

	if (outfile.Active())
	{
		SetWriteBuffer(outfile, 0);
	}

	return paths;
}

inline uint32 ArticleWriter::ProcessArticles(DiskFile& outfile,
							std::string_view infoFilename,
							std::string_view finalOutputPath,
							bool directWrite,
							bool cached)
{
	uint32 crc = 0;
	std::unique_ptr<ArticleCache::FlushGuard> flushGuard;

	if (cached)
	{
		flushGuard = std::make_unique<ArticleCache::FlushGuard>(g_ArticleCache->GuardFlush());
	}

	CharBuffer buffer;
	bool firstArticle = true;

	if (!g_Options->GetRawArticle() && !directWrite)
	{
		buffer.Reserve(1024 * 64);
	}

	for (ArticleInfo* pa : m_fileInfo->GetArticles())
	{
		if (pa->GetStatus() != ArticleInfo::aiFinished) continue;

		// 1. Fill gaps (Direct Write)
		if (!g_Options->GetRawArticle() && !directWrite &&
			pa->GetSegmentOffset() > -1 && pa->GetSegmentOffset() > outfile.Position() && outfile.Position() > -1)
		{
			memset(buffer, 0, static_cast<size_t>(buffer.Size()));
			if (!GetSkipDiskWrite())
			{
				while (pa->GetSegmentOffset() > outfile.Position() && outfile.Position() > -1 &&
					outfile.Write(buffer, std::min(pa->GetSegmentOffset() - outfile.Position(), static_cast<int64_t>(buffer.Size()))));
			}
		}

		// 2. Write Content (Cached or Disk)
		if (pa->GetSegmentContent())
		{
			if (!GetSkipDiskWrite())
			{
				if (!outfile.Seek(pa->GetSegmentOffset()))
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not seek while writing cached article %s",
						infoFilename.data());
					continue;
				}
				if (outfile.Write(pa->GetSegmentContent(), pa->GetSegmentSize()) !=
					pa->GetSegmentSize())
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not write cached article %s", infoFilename.data());
				}
			}
			pa->DiscardSegment();
		}
		else if (directWrite && pa->GetDupeFallbackRound() > 0 &&
			pa->GetResultFilename() && !GetSkipDiskWrite())
		{
			// Duplicate articles are staged in their per-article temp file even
			// when DirectWrite is enabled.  Copy only after the queue coordinator
			// has accepted the article's final geometry; never let an unvalidated
			// donor overwrite the shared output during download.
			DiskFile infile;
			if (!nzbget_rs_dqh_article_writer_validate_staged_dupe_size(
				FileSystem::FileSize(pa->GetResultFilename()), pa->GetSegmentSize()))
			{
				m_commitError = true;
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Staged duplicate article has an invalid size for %s", infoFilename.data());
			}
			else if (infile.Open(pa->GetResultFilename(), DiskFile::omRead))
			{
				if (!outfile.Seek(pa->GetSegmentOffset()))
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not seek while committing staged duplicate article %s",
						infoFilename.data());
					infile.Close();
					continue;
				}
				CharBuffer stagedBuffer(1024 * 64);
				int cnt = 0;
				int64 copied = 0;
				while ((cnt = infile.Read(stagedBuffer, stagedBuffer.Size())) > 0)
				{
					if (outfile.Write(stagedBuffer, cnt) != cnt)
					{
						m_commitError = true;
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not commit staged duplicate article %s", infoFilename.data());
						break;
					}
					copied += cnt;
				}
				if (!nzbget_rs_dqh_article_writer_validate_staged_dupe_size(
					copied, pa->GetSegmentSize()))
				{
					m_commitError = true;
				}
				infile.Close();
			}
			else
			{
				m_commitError = true;
			}
		}
		else if (!g_Options->GetRawArticle() && !directWrite && !GetSkipDiskWrite())
		{
			// Read from partial file and append to outfile
			DiskFile infile;
			if (pa->GetResultFilename() && infile.Open(pa->GetResultFilename(), DiskFile::omRead))
			{
				int64_t cnt = buffer.Size();
				while (cnt == buffer.Size())
				{
					cnt = infile.Read(buffer, buffer.Size());
					outfile.Write(buffer, cnt);
				}
				infile.Close();
			}
			else
			{
				int32_t newFailed = 0, newSuccess = 0;
				nzbget_rs_dqh_article_writer_apply_missing_part_counters(
					m_fileInfo->GetFailedArticles(), m_fileInfo->GetSuccessArticles(),
					&newFailed, &newSuccess);
				m_fileInfo->SetFailedArticles(newFailed);
				m_fileInfo->SetSuccessArticles(newSuccess);
				m_fileInfo->GetNzbInfo()->PrintMessage(
					Message::mkError, "Could not find file %s for %s [%i/%zu]",
					pa->GetResultFilename(), infoFilename.data(),
					pa->GetPartNumber(), m_fileInfo->GetArticles()->size());
			}
		}
		else if (g_Options->GetRawArticle())
		{
			// Move raw files
			NzbgetRsBuf dstBuf = nzbget_rs_dqh_article_writer_format_raw_part_dest(
				finalOutputPath.data(), finalOutputPath.size(), PATH_SEPARATOR, pa->GetPartNumber());
			const char* dstFileName = dstBuf.data ? dstBuf.data : "";
			if (!FileSystem::MoveFile(pa->GetResultFilename(), dstFileName))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					pa->GetResultFilename(), dstFileName, *FileSystem::GetLastErrorMessage());
			}
			nzbget_rs_free(dstBuf);
		}

		// 3. Calculate CRC
		if (m_format == Decoder::efYenc)
		{
			int firstArt = firstArticle ? 1 : 0;
			crc = nzbget_rs_dqh_article_writer_step_file_crc(
				firstArt, crc, pa->GetCrc(), pa->GetSegmentSize(), &firstArt);
			firstArticle = firstArt != 0;
		}
	}

	buffer.Clear();
	return crc;
}

inline bool ArticleWriter::CommitDiskFile(DiskFile& outfile,
						std::string_view tempDestPath,
						std::string_view finalOutputPath,
						bool directWrite)
{
	if (!outfile.Active()) return true;

	bool flushed = outfile.Flush();
	bool closed = outfile.Close();
	int commitAction = nzbget_rs_dqh_article_writer_eval_commit_post_close(
		flushed ? 1 : 0, closed ? 1 : 0, directWrite ? 1 : 0);
	if (commitAction < 0)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not flush completed output file %s", finalOutputPath.data());
		return false;
	}

	if (commitAction == 1)
	{
		// a failed rename leaves the complete data under its temporary name
		// (as it always has): it is not a data-commit failure, and failing the
		// file would discard data that par-rename/par-check can still use
		if (!FileSystem::MoveFile(tempDestPath.data(), finalOutputPath.data()))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					tempDestPath.data(), finalOutputPath.data(), *FileSystem::GetLastErrorMessage());
		}
	}
	return true;
}

inline bool ArticleWriter::CleanupOldData(bool directWrite,
						std::string_view nzbDestDir,
						std::string_view finalOutputPath)
{
	std::vector<int> dupeFallbackRounds;
	std::vector<const char*> resultFilenames;
	dupeFallbackRounds.reserve(m_fileInfo->GetArticles()->size());
	resultFilenames.reserve(m_fileInfo->GetArticles()->size());
	for (ArticleInfo* pa : m_fileInfo->GetArticles())
	{
		dupeFallbackRounds.push_back(pa->GetDupeFallbackRound());
		resultFilenames.push_back(pa->GetResultFilename());
	}

	auto sameFilenameCb = [](void*, const char* a, const char* b) -> int {
		return FileSystem::SameFilename(a, b) ? 1 : 0;
	};
	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	CString moveErrorMessage;
	auto deleteFileCb = [](void*, const char* path) -> int {
		return FileSystem::DeleteFile(path) ? 1 : 0;
	};
	auto moveFileCb = [](void* ctx, const char* from, const char* to) -> int {
		if (FileSystem::MoveFile(from, to))
		{
			return 1;
		}
		if (ctx)
		{
			*static_cast<CString*>(ctx) = FileSystem::GetLastErrorMessage();
		}
		return 0;
	};
	auto dirEmptyCb = [](void*, const char* dir) -> int {
		return FileSystem::DirEmpty(dir) ? 1 : 0;
	};
	auto removeDirCb = [](void*, const char* dir) -> int {
		return FileSystem::RemoveDirectory(dir) ? 1 : 0;
	};

	NzbgetRsBuf checkedOldDirBuf{};
	NzbgetRsBuf deletedOldDirBuf{};
	int moveFailed = 0;
	int hasCheckedOldDir = 0;
	int hasDeletedOldDir = 0;
	int clearOutputFilename = 0;
	nzbget_rs_awc_article_writer_execute_cleanup_old_data(
		directWrite ? 1 : 0,
		nzbDestDir.data(),
		nzbDestDir.size(),
		finalOutputPath.data(),
		finalOutputPath.size(),
		m_outputFilename.data(),
		m_outputFilename.size(),
		m_fileInfo->IsHardLinked() ? 1 : 0,
		Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0,
		PATH_SEPARATOR,
		ALT_PATH_SEPARATOR,
		dupeFallbackRounds.data(),
		resultFilenames.data(),
		dupeFallbackRounds.size(),
		&moveErrorMessage,
		sameFilenameCb,
		fileExistsCb,
		deleteFileCb,
		moveFileCb,
		dirEmptyCb,
		removeDirCb,
		&moveFailed,
		&checkedOldDirBuf,
		&hasCheckedOldDir,
		&deletedOldDirBuf,
		&hasDeletedOldDir,
		&clearOutputFilename);

	std::string checkedOldDir(checkedOldDirBuf.data ? checkedOldDirBuf.data : "", checkedOldDirBuf.len);
	nzbget_rs_free(checkedOldDirBuf);
	std::string deletedOldDir(deletedOldDirBuf.data ? deletedOldDirBuf.data : "", deletedOldDirBuf.len);
	nzbget_rs_free(deletedOldDirBuf);

	if (moveFailed)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
			m_outputFilename.c_str(), finalOutputPath.data(), *moveErrorMessage);
	}
	if (hasCheckedOldDir)
	{
		debug("Checking old dir for: %s", m_outputFilename.c_str());
	}
	if (hasDeletedOldDir)
	{
		debug("Deleting old dir: %s", deletedOldDir.c_str());
	}
	if (clearOutputFilename)
	{
		m_fileInfo->SetOutputFilename(nullptr);
	}
	return true;
}

inline void ArticleWriter::ReportCompletionStatus(std::string_view infoFilename)
{
	const int completionKind = nzbget_rs_awc_article_writer_classify_completion_status(
		m_fileInfo->GetTotalArticles(), m_fileInfo->GetSuccessArticles(),
		m_fileInfo->GetMissedArticles(), m_fileInfo->GetFailedArticles());
	if (completionKind == 1)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Successfully downloaded %s", infoFilename.data());
	}
	else if (completionKind == 2)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkWarning, "%i of %i article downloads failed for \"%s\"",
			nzbget_rs_dqh_article_writer_calc_total_failed_articles(
				m_fileInfo->GetMissedArticles(), m_fileInfo->GetFailedArticles()),
			m_fileInfo->GetTotalArticles(), infoFilename.data());
	}
	else
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Partially downloaded %s", infoFilename.data());
	}
}

inline void ArticleWriter::HandlePostProcessing(uint32 crc,
								std::string_view currentPath,
								std::string_view originalFilename,
								std::string_view nzbDestDir)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_fileInfo->SetCrc(crc);
	m_fileInfo->SetOutputFilename(currentPath.data());

	// 1. Rename if file was renamed during completion
	if (nzbget_rs_dqh_article_writer_was_renamed_during_completion(
		m_fileInfo->GetFilename(), originalFilename.data(), originalFilename.size()) != 0)
	{
		CString newOfn = FileSystem::MakeUniqueFilename(nzbDestDir.data(), m_fileInfo->GetFilename());
		if (!FileSystem::MoveFile(m_fileInfo->GetOutputFilename().c_str(), *newOfn))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not rename file %s to %s: %s",
				m_fileInfo->GetOutputFilename().c_str(), *newOfn, *FileSystem::GetLastErrorMessage());
		}
		m_fileInfo->SetOutputFilename(newOfn);
	}

	// 2. Handle Hardlinks
	if (nzbget_rs_dqh_article_writer_should_delete_hardlinked_without_interdir(
		m_fileInfo->IsHardLinked() ? 1 : 0,
		Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0) != 0)
	{
		if (FileSystem::DeleteFile(m_outputFilename.c_str()))
		{
			m_fileInfo->SetOutputFilename(nullptr);
			return;
		}
		else
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Cannot remove hardlink");
		}
	}

	// 3. Move Completed Files (if destination changed)
	if (nzbget_rs_dqh_article_writer_did_dest_dir_change(
		m_fileInfo->GetNzbInfo()->GetDestDir(), nzbDestDir.data(), nzbDestDir.size()) != 0)
	{
		MoveCompletedFiles(m_fileInfo->GetNzbInfo(), nzbDestDir.data());
	}
}

inline void ArticleWriter::FlushCache()
{
	detail("Flushing cache for %s", m_infoName.c_str());

	// a file being retried writes into its existing output (ForceDirectWrite), as
	// Start and Finish do: flushed to temporary files instead, its cached articles
	// were never written to the file although marked done
	bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
		m_fileInfo->GetOutputInitialized();
	DiskFile outfile;
	bool needBufFile = false;
	int flushedArticles = 0;
	int64 flushedSize = 0;

	{
		ArticleCache::FlushGuard flushGuard = g_ArticleCache->GuardFlush();

		std::vector<ArticleInfo*> cachedArticles;

		{
			Guard contentGuard = g_ArticleCache->GuardContent();

			if (m_fileInfo->GetFlushLocked())
			{
				return;
			}

			m_fileInfo->SetFlushLocked(true);

			cachedArticles.reserve(m_fileInfo->GetArticles()->size());
			for (ArticleInfo* pa : m_fileInfo->GetArticles())
			{
				if (pa->GetSegmentContent())
				{
					cachedArticles.push_back(pa);
				}
			}
		}

		for (ArticleInfo* pa : cachedArticles)
		{
			if (m_fileInfo->GetDeleted() && !m_fileInfo->GetNzbInfo()->GetParking())
			{
				// the file was deleted during flushing: stop flushing immediately
				break;
			}

			if (!GetSkipDiskWrite())
			{
				if (directWrite && !outfile.Active())
				{
					const std::string& outputFilename = m_fileInfo->GetOutputFilename();
					if (!outfile.Open(outputFilename.c_str(), DiskFile::omReadWrite))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not open file %s: %s", outputFilename.c_str(),
							*FileSystem::GetLastErrorMessage());
						// prevent multiple error messages
						pa->DiscardSegment();
						flushedArticles++;
						break;
					}
					needBufFile = true;
				}

				BString<1024> destFile;

				if (!directWrite)
				{
					NzbgetRsBuf tmpBuf = nzbget_rs_tpr_article_writer_format_tmp_path(pa->GetResultFilename());
					destFile.Set(tmpBuf.data ? tmpBuf.data : "", (int)tmpBuf.len);
					nzbget_rs_free(tmpBuf);
					if (!outfile.Open(destFile, DiskFile::omWrite))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not create file %s: %s", *destFile,
							*FileSystem::GetLastErrorMessage());
						// prevent multiple error messages
						pa->DiscardSegment();
						flushedArticles++;
						break;
					}
					needBufFile = true;
				}

				if (outfile.Active() && needBufFile)
				{
					SetWriteBuffer(outfile, 0);
					needBufFile = false;
				}

				if (directWrite)
				{
					outfile.Seek(pa->GetSegmentOffset());
				}

				outfile.Write(pa->GetSegmentContent(), pa->GetSegmentSize());

				flushedSize += pa->GetSegmentSize();
				flushedArticles++;

				pa->DiscardSegment();

				if (!directWrite)
				{
					outfile.Close();

					if (!FileSystem::MoveFile(destFile, pa->GetResultFilename()))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not rename file %s to %s: %s", *destFile, pa->GetResultFilename(),
							*FileSystem::GetLastErrorMessage());
					}
				}
			}
			else
			{
				flushedSize += pa->GetSegmentSize();
				flushedArticles++;
				pa->DiscardSegment();
			}
		}

		outfile.Close();

		{
			Guard contentGuard = g_ArticleCache->GuardContent();
			m_fileInfo->SetCachedArticles(m_fileInfo->GetCachedArticles() - flushedArticles);
			m_fileInfo->SetFlushLocked(false);
		}
	}

	double flushedMb = nzbget_rs_dqh_stream_repair_bytes_to_mb(flushedSize);
	if (GetSkipDiskWrite())
	{
		detail("Discarded %i articles (%.2f MB) from cache (SkipWrite) for %s", flushedArticles,
			flushedMb, m_infoName.c_str());
	}
	else
	{
		detail("Saved %i articles (%.2f MB) from cache into disk for %s", flushedArticles,
			flushedMb, m_infoName.c_str());
	}
}

inline bool ArticleWriter::MoveCompletedFiles(NzbInfo* nzbInfo, const char* oldDestDir)
{
	if (nzbInfo->GetSkipDiskWrite())
	{
		return true; // nothing was written, nothing to move
	}

	if (nzbInfo->GetCompletedFiles()->empty())
	{
		return true;
	}

	const fs::path destDir = fs::u8path(nzbInfo->GetDestDir());
	const fs::path oldDir  = fs::u8path(oldDestDir);
	fs::error_code ec;
	fs::create_directories(destDir, ec);
	if (ec)
	{
		nzbInfo->PrintMessage(
			Message::mkError,
			"Could not create directory %s: %s",
			nzbInfo->GetDestDir(),
			ec.message().c_str()
		);
		return false;
	}

	// move already downloaded files to new destination
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		const auto fileName = fs::u8path(completedFile.GetFilename());
		const auto oldPath = oldDir / fileName;
		fs::path newPath = destDir / fileName;

		if (oldPath == newPath || !fs::exists(oldPath, ec)) continue;

		if (fs::exists(newPath, ec))
		{
			newPath = fs::make_unique_filename(newPath);
		}

		if (newPath.has_parent_path())
		{
			fs::create_directories(newPath.parent_path(), ec);
		}

		fs::move_file(oldPath, newPath, ec);
		if (ec)
		{
			const auto newPathStr = fs::u8string(newPath);
			nzbInfo->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
				oldPath.string().c_str(), newPathStr.c_str(), ec.message().c_str());
		}
		else
		{
			fs::path oldDirParent = oldDir.parent_path();
			fs::path parent = oldPath.parent_path();
			while (parent != oldDirParent)
			{
				if (fs::exists(parent, ec) && fs::is_empty(parent, ec))
				{
					fs::remove(parent, ec);
					parent = parent.parent_path();
				}
				else
				{
					break;
				}
			}
		}
	}

	if (fs::exists(oldDir, ec) && fs::is_empty(oldDir, ec))
	{
		bool pendingWrites = false;
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (pendingWrites) break;

			if (fileInfo->GetOutputInitialized() && !fileInfo->GetOutputFilename().empty() &&
				fileInfo->GetActiveDownloads() > 0)
			{
				Guard guard = fileInfo->GuardOutputFile();
				if (fileInfo->GetOutputInitialized())
				{
					pendingWrites = true;
				}
			}
		}

		if (!pendingWrites)
		{
			fs::remove(oldDir, ec);
		}
	}

	return true;
}

inline CachedSegmentData ArticleCache::Alloc(int size)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = nullptr;
	size_t effectiveLimit = g_MemoryMonitor ?
		g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
		nzbget_rs_tpr_article_cache_mb_to_bytes(g_Options->GetArticleCache());

	int wasEmpty = 0;
	if (nzbget_rs_awc_article_cache_can_alloc(effectiveLimit, m_allocated, (size_t)size, &wasEmpty) != 0)
	{
		p = malloc(size);
		if (p)
		{
			if (nzbget_rs_dqh_article_cache_should_write_cache_flag(
				wasEmpty, g_Options->GetServerMode() ? 1 : 0,
				g_Options->GetContinuePartial() ? 1 : 0,
				g_Options->GetSkipWrite() ? 1 : 0) != 0)
			{
				g_DiskState->WriteCacheFlag();
			}
			if (wasEmpty)
			{
				// Resume Run(), the notification arrives later, after releasing m_allocMutex
				m_allocCond.notify_all();
			}
			m_allocated += size;
		}
	}

	return CachedSegmentData((char*)p, p ? size : 0);
}

inline bool ArticleCache::Realloc(CachedSegmentData* segment, int newSize)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = realloc(segment->m_data, newSize);
	if (p)
	{
		m_allocated += newSize - segment->m_size;
		segment->m_size = newSize;
		segment->m_data = (char*)p;
	}

	return p;
}

inline void ArticleCache::Free(CachedSegmentData* segment)
{
	if (segment->m_size)
	{
		free(segment->m_data);

		std::lock_guard<std::mutex> guard(m_allocMutex);
		size_t sub = segment->m_size > 0 ? (size_t)segment->m_size : 0;
		m_allocated = m_allocated >= sub ? (m_allocated - sub) : 0;
		if (m_allocated == 0 && g_Options->GetServerMode() && g_Options->GetContinuePartial())
		{
			g_DiskState->DeleteCacheFlag();
		}
	}
}

inline void ArticleCache::Run()
{
	int resetCounter = 0;
	bool justFlushed = false;
	while (nzbget_rs_dqh_article_cache_should_continue_run_loop(
		IsStopped() ? 1 : 0, m_allocated) != 0)
	{
		size_t effectiveLimit = g_MemoryMonitor ?
			g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
			nzbget_rs_tpr_article_cache_mb_to_bytes(g_Options->GetArticleCache());

		int tier = g_MemoryMonitor ? (int)g_MemoryMonitor->GetTier() : (int)NZBGET_RS_PRESSURE_NORMAL;
		int flushEverything = 0;
		int needEmergencyFlush = 0;
		if (nzbget_rs_qhp_article_cache_should_flush(
				m_allocated, effectiveLimit, tier, justFlushed ? 1 : 0, resetCounter,
				IsStopped() ? 1 : 0, g_Options->GetDirectWrite() ? 1 : 0,
				&flushEverything, &needEmergencyFlush) != 0)
		{
			justFlushed = CheckFlush(flushEverything != 0);
			resetCounter = 0;

			int postFlushAction = nzbget_rs_dqh_article_cache_post_flush_action(
				needEmergencyFlush, m_allocated, justFlushed ? 1 : 0, IsStopped() ? 1 : 0);
			if (postFlushAction == 1)
			{
				MemoryMonitor::TrimHeap();
			}
			else if (postFlushAction == 2)
			{
				Util::Sleep(5);
			}
		}
		else if (!m_allocated)
		{
			std::unique_lock<std::mutex> lk(m_allocMutex);
			m_allocCond.wait(lk, [&] { return IsStopped() || m_allocated > 0; });
			resetCounter = 0;
		}
		else
		{
			Util::Sleep(5);
			resetCounter += 5;
		}
	}
}

inline void ArticleCache::Stop()
{
	Thread::Stop();

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_allocMutex);
	m_allocCond.notify_all();
}

inline bool ArticleCache::CheckFlush(bool flushEverything)
{
	debug("Checking cache, Allocated: %i, FlushEverything: %i", (int)m_allocated, (int)flushEverything);

	BString<1024> infoName;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (m_fileInfo)
			{
				break;
			}

			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (nzbget_rs_awc_article_cache_can_flush_file(
						fileInfo->GetCachedArticles(), fileInfo->GetActiveDownloads(),
						fileInfo->GetNzbInfo()->GetSkipDiskWrite() ? 1 : 0,
						flushEverything ? 1 : 0) != 0)
				{
					m_fileInfo = fileInfo;
					NzbgetRsBuf infoBuf = nzbget_rs_dqh_article_writer_format_info_path(
						m_fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, m_fileInfo->GetFilename());
					infoName.Set(infoBuf.data ? infoBuf.data : "", (int)infoBuf.len);
					nzbget_rs_free(infoBuf);
					break;
				}
			}
		}
	}

	if (m_fileInfo)
	{
		ArticleWriter articleWriter;
		articleWriter.SetFileInfo(m_fileInfo);
		articleWriter.SetInfoName(infoName);
		articleWriter.FlushCache();
		m_fileInfo = nullptr;
		return true;
	}

	debug("Checking cache... nothing to flush");

	return false;
}

inline ArticleCache::FlushGuard::FlushGuard(Mutex& mutex) : m_guard(&mutex)
{
	g_ArticleCache->m_flushing = true;
}

inline ArticleCache::FlushGuard::~FlushGuard()
{
	if (m_guard)
	{
		g_ArticleCache->m_flushing = false;
	}
}


inline MemoryMonitor::MemoryMonitor()
{
	InitOptions();
}

inline void MemoryMonitor::InitOptions()
{
	if (g_Options && !Util::EmptyStr(g_Options->GetMemoryPressurePath()))
	{
		nzbget_rs_memory_pressure_set_mock_root(g_Options->GetMemoryPressurePath());
	}
	else if (g_Options)
	{
		nzbget_rs_memory_pressure_set_mock_root(nullptr);
	}
	UpdateState();
}

inline MemoryMonitor::~MemoryMonitor()
{
	if (IsRunning())
	{
		Stop();
	}
	while (IsRunning())
	{
		Util::Sleep(10);
	}
}

inline void MemoryMonitor::Run()
{
	debug("MemoryMonitor thread started");

	while (!IsStopped())
	{
		UpdateState();

		int interval = g_Options ? g_Options->GetMemoryPressureInterval() : 500;
		if (interval < 100) interval = 100;

		int slept = 0;
		while (nzbget_rs_dqh_memory_monitor_should_keep_sleeping(
			IsStopped() ? 1 : 0, slept, interval) != 0)
		{
			Util::Sleep(50);
			slept += 50;
		}
	}

	debug("MemoryMonitor thread stopped");
}

inline void MemoryMonitor::Stop()
{
	Thread::Stop();
}

inline void MemoryMonitor::UpdateState()
{
	int rawSample = nzbget_rs_memory_pressure_sample();
	int currentTier = m_tier.load(std::memory_order_relaxed);
	int nextTier = currentTier;
	bool transitioned = nzbget_rs_qhp_memory_monitor_step(
		rawSample, currentTier, &m_candidateTier, &m_consecutiveSamples, &nextTier) != 0;

	uint32_t cfgCache = g_Options ? (uint32_t)g_Options->GetArticleCache() : 0;
	uint32_t cfgPar = g_Options ? (uint32_t)g_Options->GetParBuffer() : 0;
	uint32_t cfgLimit = 20;
	uint32_t rsEffCache = (nextTier != NZBGET_RS_PRESSURE_CRITICAL &&
		nextTier != NZBGET_RS_PRESSURE_NORMAL && rawSample == nextTier) ?
		nzbget_rs_memory_pressure_effective_cache(cfgCache) : 0;
	uint32_t rsEffPar = (nextTier != NZBGET_RS_PRESSURE_CRITICAL &&
		nextTier != NZBGET_RS_PRESSURE_NORMAL && rawSample == nextTier) ?
		nzbget_rs_memory_pressure_effective_par_buffer(cfgPar) : 0;
	uint32_t effCache = 0, effPar = 0, effLimit = 0;
	nzbget_rs_awc_memory_monitor_resolve_tier_limits(
		nextTier,
		rawSample,
		cfgCache,
		cfgPar,
		cfgLimit,
		(int)NZBGET_RS_PRESSURE_NORMAL,
		(int)NZBGET_RS_PRESSURE_CONSTRAINED,
		(int)NZBGET_RS_PRESSURE_CRITICAL,
		rsEffCache,
		rsEffPar,
		&effCache,
		&effPar,
		&effLimit);
	m_effectiveCacheMB.store(effCache, std::memory_order_release);
	m_effectiveParBufferMB.store(effPar, std::memory_order_release);
	m_effectiveDownloadsLimit.store(effLimit, std::memory_order_release);

	if (transitioned)
	{
		m_tier.store(nextTier, std::memory_order_release);
		ApplyTierTransition((NzbgetRsPressureTier)currentTier, (NzbgetRsPressureTier)nextTier);
	}
}

inline void MemoryMonitor::ApplyTierTransition(NzbgetRsPressureTier oldTier, NzbgetRsPressureTier newTier)
{
	int action = nzbget_rs_awc_memory_monitor_classify_tier_transition_action(
		(int)oldTier, (int)newTier,
		(int)NZBGET_RS_PRESSURE_NORMAL,
		(int)NZBGET_RS_PRESSURE_CONSTRAINED,
		(int)NZBGET_RS_PRESSURE_CRITICAL);
	if (action == 1)
	{
		warn("System memory pressure CRITICAL (>=90%%): shedding article cache and throttling downloads");
		if (g_ArticleCache)
		{
			g_ArticleCache->WakeupFlush();
		}
		TrimHeap();
	}
	else if (action == 2)
	{
		detail("System memory pressure CONSTRAINED: shrinking article cache ceiling");
		if (g_ArticleCache)
		{
			g_ArticleCache->WakeupFlush();
		}
	}
	else if (action == 3)
	{
		detail("System memory pressure returned to NORMAL: restoring cache and download capacity");
		TrimHeap();
	}
}

#endif

class ArticleFetchLimits
{
public:
	static constexpr int64 MaxDecodedBytes = 64LL * 1024 * 1024;
	static constexpr int64 MaxHeaderBytes = 64LL * 1024;
	static constexpr int64 MaxRawBytes = MaxDecodedBytes * 3 + MaxHeaderBytes;

	bool AddRawBytes(int64 bytes);
	bool HeaderWithinLimit(bool declaredRangeKnown) const;
	bool AddDecodedBytes(int64 bytes, int64 begin, int64 end, int64 fileSize);

private:
	int64 m_rawBytes = 0;
	int64 m_decodedBytes = 0;
};

class ArticleFetcher
{
public:
	struct FetchedArticle
	{
		std::vector<char> Data;	// decoded bytes
		int64 Offset = 0;		// decoded-stream offset (yEnc part begin - 1)
		int64 FileSize = 0;		// total decoded file size from "=ybegin size="
		bool Success = false;
		bool Retry = false;		// interrupted by quota; retry without blaming source
		bool Transient = false;
	};

	static constexpr int MaxTransientAttempts = 4;

	static int TransientRetryDelay(int attempt) { return attempt; }

	FetchedArticle Fetch(const char* messageId, const std::vector<CString>& groups);

	FetchedArticle FetchFromConnection(NntpConnection* connection,
		const char* messageId, const std::vector<CString>& groups);

	void Stop();

	void SetWaitCounter(std::atomic<int>* counter) { m_waitCounter = counter; }

private:
	std::atomic<int>* m_waitCounter = nullptr;
	std::atomic<bool> m_stopped{false};
	Mutex m_connectionMutex;
	NntpConnection* m_connection = nullptr;

	static bool ServerUnreachable(NewsServer* server);
	static void SetServerUnreachable(NewsServer* server, bool unreachable);
	bool WaitBeforeRetry(int seconds);

	void ReleaseConnection(NntpConnection* connection, bool keepConnected);
	void AddServerStats(NntpConnection* connection);
};

class ArticleBatchFetcher
{
public:
	struct Request
	{
		CString MessageId;
		std::shared_ptr<const std::vector<CString>> Groups;
	};
	using FetchFunc = std::function<ArticleFetcher::FetchedArticle(const Request&)>;

	static constexpr int MaxWindowParts = 8;
	static constexpr int64 MaxBufferedBytes = 128LL * 1024 * 1024;

	ArticleBatchFetcher();
	explicit ArticleBatchFetcher(FetchFunc fetchFunc);
	~ArticleBatchFetcher();

	void SetWorkerCount(int workerCount);
	void SetWaitCounter(std::atomic<int>* counter);
	int GetWindowParts() const;
	void Begin(std::vector<Request> requests);
	bool Next(ArticleFetcher::FetchedArticle& result);
	void CancelRemaining();
	void Stop();

private:
	Mutex m_mutex;
	int m_workerCount = 0;
	std::atomic<int>* m_waitCounter = nullptr;
	std::vector<std::unique_ptr<ArticleFetcher>> m_workerFetchers;
	FetchFunc m_fetchFunc;			// test seam; production uses real fetchers
	ArticleFetcher m_inlineFetcher;	// worker count 0 (live mode)
	void* m_rsBatch = nullptr;

	void InitRsBatch();
	void EnsureWorkerFetchers();
};

namespace ArticleFetcherDetail
{
	inline Mutex g_unreachableMutex;
	inline std::set<int> g_unreachableServers; // news server ids
}

inline bool ArticleFetchLimits::AddRawBytes(int64 bytes)
{
	return nzbget_rs_article_fetch_add_raw_bytes(&m_rawBytes, bytes) != 0;
}

inline bool ArticleFetchLimits::HeaderWithinLimit(bool declaredRangeKnown) const
{
	return nzbget_rs_article_fetch_header_within_limit(m_rawBytes, declaredRangeKnown ? 1 : 0) != 0;
}

inline bool ArticleFetchLimits::AddDecodedBytes(int64 bytes, int64 begin, int64 end,
	int64 fileSize)
{
	return nzbget_rs_article_fetch_add_decoded_bytes(&m_decodedBytes, bytes, begin, end, fileSize) != 0;
}

inline ArticleFetcher::FetchedArticle ArticleFetcher::Fetch(const char* messageId,
	const std::vector<CString>& groups)
{
	FetchedArticle result;
	ServerPool::RawServerList failedServers;
	std::map<NewsServer*, int> transientAttempts;
	std::map<NewsServer*, int> staleConnections;

	int level = 0;
	while (level <= g_ServerPool->GetMaxNormLevel() && !m_stopped)
	{
		while (g_WorkState->GetQuotaReached() && !m_stopped)
		{
			Util::Sleep(100);
		}
		if (m_stopped)
		{
			break;
		}
		int eligible = 0;
		std::vector<int> failedIds, failedGroups, failedLevels;
		failedIds.reserve(failedServers.size());
		failedGroups.reserve(failedServers.size());
		failedLevels.reserve(failedServers.size());
		for (NewsServer* failed : failedServers)
		{
			failedIds.push_back(failed->GetId());
			failedGroups.push_back(failed->GetGroup());
			failedLevels.push_back(failed->GetNormLevel());
		}
		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (nzbget_rs_tpr_article_fetcher_is_server_eligible_on_level(
				server->GetActive() ? 1 : 0, server->GetNormLevel(), level,
				server->GetId(), server->GetGroup(),
				failedIds.data(), failedGroups.data(), failedLevels.data(), failedServers.size()) != 0)
			{
				eligible++;
			}
		}
		if (eligible == 0)
		{
			level++;
			continue;
		}

		NntpConnection* connection = nullptr;
		if (m_waitCounter)
		{
			(*m_waitCounter)++;
		}
		while (!connection && !m_stopped)
		{
			if (g_WorkState->GetQuotaReached())
			{
				Util::Sleep(100);
				continue;
			}
			connection = g_ServerPool->GetConnection(level, nullptr, &failedServers);
			if (!connection)
			{
				Util::Sleep(5);
			}
		}
		if (m_waitCounter)
		{
			(*m_waitCounter)--;
		}

		if (!connection)
		{
			level++;
			continue;
		}

		bool stopped = false;
		{
			Guard guard(m_connectionMutex);
			m_connection = connection;
			stopped = m_stopped;
		}
		if (stopped)
		{
			ReleaseConnection(connection, true);
			break;
		}

		bool reused = connection->GetStatus() == Connection::csConnected;
		result = FetchFromConnection(connection, messageId, groups);

		NewsServer* server = connection->GetNewsServer();
		ReleaseConnection(connection, true);

		if (nzbget_rs_dqh_article_fetcher_should_clear_unreachable(
			result.Success ? 1 : 0, result.Transient ? 1 : 0) != 0)
		{
			SetServerUnreachable(server, false);	// it answered
		}
		if (result.Success)
		{
			return result;
		}
		if (result.Retry)
		{
			while (g_WorkState->GetQuotaReached() && !m_stopped)
			{
				Util::Sleep(100);
			}
			continue;
		}
		if (nzbget_rs_dqh_article_fetcher_can_attempt_transient_retry(
			result.Transient ? 1 : 0, ServerUnreachable(server) ? 1 : 0) != 0)
		{
			if (reused && nzbget_rs_tpr_article_fetcher_is_reused_stale_within_max(
				reused ? 1 : 0, ++staleConnections[server], server->GetMaxConnections()) != 0)
			{
				continue;
			}
			int attempt = ++transientAttempts[server];
			if (nzbget_rs_dqh_article_fetcher_has_more_transient_attempts(
				attempt, MaxTransientAttempts) != 0)
			{
				if (WaitBeforeRetry(TransientRetryDelay(attempt)))
				{
					continue;	// ask this server again on a fresh connection
				}
				break;
			}
			SetServerUnreachable(server, true);
		}

		failedServers.push_back(server);
	}

	return result;
}

inline bool ArticleFetcher::ServerUnreachable(NewsServer* server)
{
	Guard guard(ArticleFetcherDetail::g_unreachableMutex);
	return ArticleFetcherDetail::g_unreachableServers.count(server->GetId()) > 0;
}

inline void ArticleFetcher::SetServerUnreachable(NewsServer* server, bool unreachable)
{
	Guard guard(ArticleFetcherDetail::g_unreachableMutex);
	if (unreachable)
	{
		ArticleFetcherDetail::g_unreachableServers.insert(server->GetId());
	}
	else
	{
		ArticleFetcherDetail::g_unreachableServers.erase(server->GetId());
	}
}

inline bool ArticleFetcher::WaitBeforeRetry(int seconds)
{
	for (int waited = 0;
		nzbget_rs_dqh_article_fetcher_should_keep_waiting_retry(
			waited, seconds, m_stopped ? 1 : 0) != 0;
		waited++)
	{
		Util::Sleep(100);
	}
	return !m_stopped;
}

inline void ArticleFetcher::Stop()
{
	m_stopped = true;
	Guard guard(m_connectionMutex);
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
}

inline void ArticleFetcher::ReleaseConnection(NntpConnection* connection, bool keepConnected)
{
	Guard guard(m_connectionMutex);
	AddServerStats(connection);
	if (nzbget_rs_dqh_article_fetcher_should_disconnect_on_release(
		keepConnected ? 1 : 0, (int)connection->GetStatus(), (int)Connection::csCancelled) != 0)
	{
		connection->Disconnect();
	}
	if (m_connection == connection)
	{
		m_connection = nullptr;
	}
	g_ServerPool->FreeConnection(connection, true);
}

inline void ArticleFetcher::AddServerStats(NntpConnection* connection)
{
	int bytesRead = connection->FetchTotalBytesRead();
	if (bytesRead <= 0)
	{
		return;
	}

	ServerVolume::Stats stats;
	stats.bytes = Util::SafeIntCast<int, uint32>(bytesRead);
	stats.articles.failed = 0;
	stats.articles.success = 0;
	g_StatMeter->AddServerStats(stats, connection->GetNewsServer()->GetId());
}

inline ArticleFetcher::FetchedArticle ArticleFetcher::FetchFromConnection(NntpConnection* connection,
	const char* messageId, const std::vector<CString>& groups)
{
	FetchedArticle result;
	if (m_stopped)
	{
		return result;
	}

	if (!connection->Connect())
	{
		detail("Stream repair: could not connect to %s", connection->GetNewsServer()->GetName());
		result.Transient = true;
		return result;
	}

	if (connection->GetNewsServer()->GetJoinGroup())
	{
		const char* response = nullptr;
		for (const CString& group : groups)
		{
			response = connection->JoinGroup(group);
			if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) != 0)
			{
				break;
			}
		}
		if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) == 0)
		{
			detail("Stream repair: could not join groups on %s for %s",
				connection->GetNewsServer()->GetName(), messageId);
			result.Transient = nzbget_rs_dqh_article_fetcher_is_join_group_failure_transient(
				response ? 1 : 0) != 0;
			if (result.Transient)
			{
				connection->Disconnect();
			}
			return result;
		}
	}

	NzbgetRsBuf bodyCmdBuf = nzbget_rs_dqh_article_fetcher_format_body_cmd(messageId);
	BString<1024> bodyCmd;
	bodyCmd.Set(bodyCmdBuf.data ? bodyCmdBuf.data : "", (int)bodyCmdBuf.len);
	nzbget_rs_free(bodyCmdBuf);
	const char* response = connection->Request(bodyCmd);
	if (!response)
	{
		detail("Stream repair: no response from %s for %s", connection->GetNewsServer()->GetName(), messageId);
		connection->Disconnect();
		result.Transient = true;
		return result;
	}
	if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) == 0)
	{
		detail("Stream repair: article %s not available on %s",
			messageId, connection->GetNewsServer()->GetName());
		return result;
	}

	Decoder decoder;
	decoder.Clear();
	decoder.SetCrcCheck(true);
	decoder.SetRawMode(false);

	int readChunk = g_Options->GetArticleReadChunkSize();
	CharBuffer recvBuf(readChunk + 128);	// (decoder slack, see ArticleDownloader)
	ArticleFetchLimits limits;

	while (!m_stopped && !decoder.GetEof())
	{
		if (g_WorkState->GetQuotaReached())
		{
			connection->Disconnect();
			result.Retry = true;
			return result;
		}
		while (nzbget_rs_tpr_article_fetcher_should_throttle_bandwidth(
			m_stopped ? 1 : 0,
			g_WorkState->GetQuotaReached() ? 1 : 0,
			g_WorkState->GetSpeedLimit(),
			g_StatMeter->CalcCurrentDownloadSpeed(),
			g_StatMeter->CalcMomentaryDownloadSpeed()) != 0)
		{
			Util::Sleep(10);
		}
		if (g_WorkState->GetQuotaReached())
		{
			connection->Disconnect();
			result.Retry = true;
			return result;
		}
		if (m_stopped)
		{
			break;
		}

		char* buffer;
		int len;
		connection->ReadBuffer(&buffer, &len);
		if (len == 0)
		{
			len = connection->TryRecv(recvBuf, readChunk);
			buffer = recvBuf;
		}

		if (len <= 0)
		{
			detail("Stream repair: connection to %s lost while fetching %s",
				connection->GetNewsServer()->GetName(), messageId);
			connection->Disconnect();
			result = FetchedArticle();
			result.Transient = true;
			return result;
		}

		g_StatMeter->AddSpeedReading(len);
		if (!limits.AddRawBytes(len))
		{
			detail("Stream repair: article %s from %s exceeded the wire-size limit",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}

		int decodedLen = decoder.DecodeBuffer(buffer, len);
		bool declaredRangeKnown = nzbget_rs_tpr_article_fetcher_is_declared_range_known(
			decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize()) != 0;
		if (!limits.HeaderWithinLimit(declaredRangeKnown))
		{
			detail("Stream repair: article %s from %s exceeded the yEnc-header limit",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}
		if (declaredRangeKnown && !limits.AddDecodedBytes(decodedLen,
			decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize()))
		{
			detail("Stream repair: article %s from %s exceeded its declared decoded range",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}
		if (decodedLen > 0)
		{
			result.Data.insert(result.Data.end(), buffer, buffer + decodedLen);
		}
	}

	if (m_stopped)
	{
		connection->Disconnect();
		return result;
	}

	int64 decOffset = 0;
	int64 decFileSize = 0;
	int decSuccess = 0;
	if (!nzbget_rs_nntp_article_fetcher_eval_decoded_yenc(
		(int)decoder.Check(), (int)Decoder::dsFinished,
		(int)decoder.GetFormat(), (int)Decoder::efYenc,
		decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize(),
		result.Data.size(), &decOffset, &decFileSize, &decSuccess))
	{
		detail("Stream repair: article %s from %s failed decoding",
			messageId, connection->GetNewsServer()->GetName());
		return result;
	}
	result.Offset = decOffset;
	result.FileSize = decFileSize;
	result.Success = decSuccess != 0;

	return result;
}

inline ArticleBatchFetcher::ArticleBatchFetcher()
{
	InitRsBatch();
}

inline ArticleBatchFetcher::ArticleBatchFetcher(FetchFunc fetchFunc) :
	m_fetchFunc(std::move(fetchFunc))
{
	InitRsBatch();
}

inline ArticleBatchFetcher::~ArticleBatchFetcher()
{
	nzbget_rs_article_batch_fetcher_free(m_rsBatch);
	m_rsBatch = nullptr;
}

inline void ArticleBatchFetcher::InitRsBatch()
{
	m_rsBatch = nzbget_rs_article_batch_fetcher_new(
		this,
		[](void* raw, size_t workerIdx, const char* msgId,
			const char* const* groups, size_t groupCount,
			NzbgetRsBuf* outData, int64_t* outOffset, int64_t* outFileSize,
			int* outSuccess, int* outRetry, int* outTransient) -> int {
			auto* self = static_cast<ArticleBatchFetcher*>(raw);
			ArticleFetcher::FetchedArticle fetched;
			try
			{
				auto grpVec = std::make_shared<std::vector<CString>>();
				grpVec->reserve(groupCount);
				for (size_t i = 0; i < groupCount; i++)
				{
					grpVec->emplace_back(groups[i] ? groups[i] : "");
				}
				if (self->m_fetchFunc)
				{
					Request req{CString(msgId ? msgId : ""), grpVec};
					fetched = self->m_fetchFunc(req);
				}
				else
				{
					ArticleFetcher* fetcher = &self->m_inlineFetcher;
					if (workerIdx != SIZE_MAX)
					{
						Guard guard(self->m_mutex);
						if (workerIdx < self->m_workerFetchers.size())
						{
							fetcher = self->m_workerFetchers[workerIdx].get();
						}
					}
					fetched = fetcher->Fetch(msgId ? msgId : "", *grpVec);
				}
			}
			catch (const std::exception& e)
			{
				detail("Batch fetch of %s failed: %s", msgId ? msgId : "", e.what());
				return 0;
			}
			catch (...)
			{
				detail("Batch fetch of %s failed", msgId ? msgId : "");
				return 0;
			}
			*outData = nzbget_rs_alloc_buf(fetched.Data.data(), fetched.Data.size());
			*outOffset = fetched.Offset;
			*outFileSize = fetched.FileSize;
			*outSuccess = fetched.Success ? 1 : 0;
			*outRetry = fetched.Retry ? 1 : 0;
			*outTransient = fetched.Transient ? 1 : 0;
			return 1;
		},
		[](void* raw) {
			auto* self = static_cast<ArticleBatchFetcher*>(raw);
			try
			{
				Guard guard(self->m_mutex);
				self->m_inlineFetcher.Stop();
				for (auto& wf : self->m_workerFetchers)
				{
					wf->Stop();
				}
			}
			catch (...)
			{
			}
		});
}

inline void ArticleBatchFetcher::SetWorkerCount(int workerCount)
{
	m_workerCount = workerCount;
	nzbget_rs_article_batch_fetcher_set_worker_count(m_rsBatch, workerCount);
}

inline void ArticleBatchFetcher::SetWaitCounter(std::atomic<int>* counter)
{
	Guard guard(m_mutex);
	m_waitCounter = counter;
	m_inlineFetcher.SetWaitCounter(counter);
	for (auto& wf : m_workerFetchers)
	{
		wf->SetWaitCounter(counter);
	}
}

inline int ArticleBatchFetcher::GetWindowParts() const
{
	return nzbget_rs_article_batch_fetcher_get_window_parts(m_rsBatch);
}

inline void ArticleBatchFetcher::EnsureWorkerFetchers()
{
	Guard guard(m_mutex);
	while ((int)m_workerFetchers.size() < m_workerCount)
	{
		m_workerFetchers.push_back(std::make_unique<ArticleFetcher>());
		m_workerFetchers.back()->SetWaitCounter(m_waitCounter);
	}
}

inline void ArticleBatchFetcher::Begin(std::vector<Request> requests)
{
	EnsureWorkerFetchers();
	nzbget_rs_article_batch_fetcher_begin(
		m_rsBatch, requests.size(), &requests,
		[](void* raw, size_t idx, const char** outMidPtr, size_t* outMidLen) -> size_t {
			auto* reqs = static_cast<std::vector<Request>*>(raw);
			const Request& r = (*reqs)[idx];
			*outMidPtr = r.MessageId ? *r.MessageId : "";
			*outMidLen = r.MessageId.Length();
			return r.Groups ? r.Groups->size() : 0;
		},
		[](void* raw, size_t reqIdx, size_t grpIdx, const char** outGrpPtr, size_t* outGrpLen) {
			auto* reqs = static_cast<std::vector<Request>*>(raw);
			const CString& g = (*(*reqs)[reqIdx].Groups)[grpIdx];
			*outGrpPtr = g ? *g : "";
			*outGrpLen = g.Length();
		});
}

inline bool ArticleBatchFetcher::Next(ArticleFetcher::FetchedArticle& result)
{
	NzbgetRsBuf dataBuf{nullptr, 0};
	int64_t offset = 0;
	int64_t fileSize = 0;
	int success = 0;
	int retry = 0;
	int transient = 0;
	if (!nzbget_rs_article_batch_fetcher_next(
		m_rsBatch, &dataBuf, &offset, &fileSize, &success, &retry, &transient))
	{
		return false;
	}
	result.Data.assign(dataBuf.data ? dataBuf.data : "", (dataBuf.data ? dataBuf.data : "") + dataBuf.len);
	nzbget_rs_free(dataBuf);
	result.Offset = offset;
	result.FileSize = fileSize;
	result.Success = success != 0;
	result.Retry = retry != 0;
	result.Transient = transient != 0;
	return true;
}

inline void ArticleBatchFetcher::CancelRemaining()
{
	nzbget_rs_article_batch_fetcher_cancel_remaining(m_rsBatch);
}

inline void ArticleBatchFetcher::Stop()
{
	nzbget_rs_article_batch_fetcher_stop(m_rsBatch);
}

#endif
#endif
