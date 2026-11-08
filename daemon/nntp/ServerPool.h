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
#define SERVERPOOL_H

#include <exception>
#include "Log.h"
#include "Options.h"
#include "Connection.h"
#include "nzbget_rs.h"

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

#endif
