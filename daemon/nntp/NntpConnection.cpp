/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#include "nzbget.h"
#include "Log.h"
#include "NntpConnection.h"
#include <exception>
#include "nzbget_rs.h"
#include "Connection.h"
#include "NewsServer.h"

static const int CONNECTION_LINEBUFFER_SIZE = 1024*10;

NntpConnection::NntpConnection(NewsServer* newsServer) :
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

namespace
{
#ifdef DEBUG
constexpr bool RsDebugMessages = true;
#else
constexpr bool RsDebugMessages = false;
#endif
}

// a callback that threw stops the exchange; the exception is rethrown after
// Rust returned
struct NntpConnection::RsExchange
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
		// no debug callback where debug() is compiled out: Rust then doesn't build the texts
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

int NntpConnection::RsWriteLine(void* ctx, const char* line)
{
	return RsExchange::Call(ctx, [line](NntpConnection* c) { c->WriteLine(line); });
}

int NntpConnection::RsReadLine(void* ctx, char** line)
{
	return RsExchange::Call(ctx, [line](NntpConnection* c) { *line = c->ReadLine(c->m_lineBuf, c->m_lineBuf.Size(), nullptr); });
}

int NntpConnection::RsReportError(void* ctx, const char* prefix, const char* arg)
{
	return RsExchange::Call(ctx, [prefix, arg](NntpConnection* c) { c->ReportError(prefix, arg, false, 0); });
}

int NntpConnection::RsDebug(void* ctx, const char* msg)
{
	return RsExchange::Call(ctx, [msg](NntpConnection*) { debug("%s", msg); });
}

int NntpConnection::RsCancelled(void* ctx, int* cancelled)
{
	return RsExchange::Call(ctx, [cancelled](NntpConnection* c) { *cancelled = c->GetStatus() == csCancelled; });
}


const char* NntpConnection::Request(const char* req)
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

const char* NntpConnection::JoinGroup(const char* grp)
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

bool NntpConnection::Connect()
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

bool NntpConnection::Disconnect()
{
	if (m_status == csConnected)
	{
		Request("quit\r\n");
	}
	// also when the connection broke (a timeout, a reset) or was cancelled: kept,
	// the next connection skipped GROUP for that group, and a server that needs
	// it answered "no such article"
	m_activeGroup = nullptr;
	return Connection::Disconnect();
}
