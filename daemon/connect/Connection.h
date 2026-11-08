/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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


#ifndef CONNECTION_H
#define CONNECTION_H

#include <atomic>
#include <string>
#include <memory>
#include <vector>
#include "NString.h"
#include "TlsSocket.h"

class Connection
{
public:
	enum EStatus
	{
		csConnected,
		csDisconnected,
		csListening,
		csCancelled,
		csBroken
	};

	enum EIPVersion
	{
		ipAuto = 0,
		ipV4 = 4,
		ipV6 = 6
	};

	Connection(const char* host, int port, bool tls);
	Connection(SOCKET socket, bool tls);
	virtual ~Connection();
	static void Init();
	static void Final();
	virtual bool Connect();
	virtual bool Disconnect();
	bool Bind();
	bool Send(const char* buffer, int size);
	bool Recv(char* buffer, int size);
	int TryRecv(char* buffer, int size);
	char* ReadLine(char* buffer, int size, int* bytesRead);
	void ReadBuffer(char** buffer, int *bufLen);
	int WriteLine(const char* buffer);
	std::unique_ptr<Connection> Accept();
	void Cancel();
	const char* GetHost() const { return m_host.c_str(); }
	int GetPort() { return m_port; }
	bool GetTls() { return m_tls; }
	const char* GetCipher() const { return m_cipher.c_str(); }
	void SetCipher(const char* cipher) { m_cipher = cipher ? cipher : ""; }
	void SetTimeout(int timeout) { m_timeout = timeout; }
	void SetResolvedAddresses(const std::vector<std::string>& addresses) { m_resolvedAddresses = addresses; }
	void SetDeadline(std::shared_ptr<const SocketDeadline> deadline) { m_deadline = std::move(deadline); }
#ifndef DISABLE_TLS
	const char* GetNegotiatedCipher() const { return m_tlsSocket ? m_tlsSocket->GetNegotiatedCipher() : ""; }
	int GetNegotiatedVersion() const { return m_tlsSocket ? m_tlsSocket->GetNegotiatedVersion() : 0; }
	int GetTlsProbeFailure() const { return m_tlsProbeFailure; }
#endif
	void SetIPVersion(EIPVersion ipVersion) { m_ipVersion = ipVersion; }
	EStatus GetStatus() { return m_status; }
	// the last Accept() failed for lack of descriptors or memory, not on the listener
	bool GetAcceptBusy() { return m_acceptBusy; }
	// the peer can't be relied on any more (a timeout mid-reply): Disconnect sends
	// nothing and waits for nothing
	void SetBroken() { m_status = csBroken; }
	void SetSuppressErrors(bool suppressErrors);
	bool GetSuppressErrors() { return m_suppressErrors; }
	const char* GetRemoteAddr();
	bool GetGracefull() { return m_gracefull; }
	void SetGracefull(bool gracefull) { m_gracefull = gracefull; }
	void SetForceClose(bool forceClose) { m_forceClose = forceClose; }
#ifndef DISABLE_TLS
	bool StartTls(bool isClient, const char* certFile, const char* keyFile);
	void SetCertVerifLevel(unsigned int level) { m_certVerifLevel = level; }
#endif
	int FetchTotalBytesRead();

protected:
	std::string m_host;
	std::string m_cipher;
	std::vector<std::string> m_resolvedAddresses;
	std::shared_ptr<const SocketDeadline> m_deadline;
	int m_port;
	bool m_tls;
	EIPVersion m_ipVersion = ipAuto;
	SOCKET m_socket = INVALID_SOCKET;
	CharBuffer m_readBuf;
	int m_bufAvail = 0;
	char* m_bufPtr = nullptr;
	std::atomic<EStatus> m_status{csDisconnected};
	int m_timeout = 60;
	bool m_suppressErrors = true;
	bool m_acceptBusy = false;
	BString<100> m_remoteAddr;
	int m_totalBytesRead = 0;
	bool m_gracefull = false;
	bool m_forceClose = false;
#ifndef DISABLE_TLS
	unsigned int m_certVerifLevel;
#endif

	struct SockAddr
	{
		int ai_family;
		int ai_socktype;
		int ai_protocol;
		bool operator==(const SockAddr& rhs) const
			{ return memcmp(this, &rhs, sizeof(SockAddr)) == 0; }
	};

#ifndef DISABLE_TLS
	std::unique_ptr<TlsSocket> m_tlsSocket;
	bool m_tlsError = false;
	int m_tlsProbeFailure = -1;
#endif
#ifndef HAVE_GETADDRINFO
#ifndef HAVE_GETHOSTBYNAME_R
	inline static std::mutex m_getHostByNameMutex;
#endif
#endif

	void ReportError(const char* msgPrefix, const char* msgArg, bool printErrCode, int errCode = 0,
		const char* errMsg = nullptr);
	virtual void PrintError(const char* errMsg);
	int GetLastNetworkError();
	bool DoConnect();
	void DoDisconnect();
	bool InitSocketOpts(SOCKET socket);
	bool ConnectWithTimeout(void* address, int address_len);
#ifndef HAVE_GETADDRINFO
	in_addr_t ResolveHostAddr(const char* host);
#endif
#ifndef DISABLE_TLS
	int recv(SOCKET s, char* buf, int len, int flags);
	int send(SOCKET s, const char* buf, int len, int flags);
	void CloseTls();
#endif
};

#include "nzbget_rs.h"
#include "Log.h"
#include "FileSystem.h"
#include "Options.h"

// sockets aren't inherited by the scripts nzbget starts: a script that outlived
// the program kept the control port listening, and copies of client sockets kept
// connections from closing
#ifdef SOCK_CLOEXEC
#define SOCKET_CLOEXEC SOCK_CLOEXEC
#else
#define SOCKET_CLOEXEC 0
#endif

// a socket is close-on-exec, so that the scripts nzbget starts don't inherit
// its connections and listeners. socket() takes the flag where SOCK_CLOEXEC
// exists, accept4() on Linux; elsewhere (macOS has neither) it is set afterwards
static inline void SetCloseOnExec(SOCKET socket)
{
	if (socket != INVALID_SOCKET)
	{
		fcntl(socket, F_SETFD, fcntl(socket, F_GETFD, 0) | FD_CLOEXEC);
	}
}

#include <poll.h>


static const int CONNECTION_READBUFFER_SIZE = 1024;
// past the data read into a buffer: a decoder writing into it (uuencoded lines) may
// write up to one line's worth more than it was given
static const int DECODE_SLACK = 128;

inline void closesocket_gracefully(SOCKET socket)
{
	char buf[1024];
	struct linger linger;

	// Set linger option to avoid socket hanging out after close. This prevent
	// ephemeral port exhaust problem under high QPS.
	linger.l_onoff = 1;
	linger.l_linger = 1;
	setsockopt(socket, SOL_SOCKET, SO_LINGER, (char *) &linger, sizeof(linger));

	// Send FIN to the client
	shutdown(socket, SHUT_WR);

	// Set non-blocking mode
	int flags;
	flags = fcntl(socket, F_GETFL, 0);
	fcntl(socket, F_SETFL, flags | O_NONBLOCK);

	// Read and discard pending incoming data. If we do not do that and close the
	// socket, the data in the send buffer may be discarded. This
	// behaviour is seen on Windows, when client keeps sending data
	// when server decides to close the connection; then when client
	// does recv() it gets no data back.
	int n;
	do {
		n = recv(socket, buf, sizeof(buf), 0);
	} while (n > 0);

	// Now we know that our FIN is ACK-ed, safe to close
	closesocket(socket);
}

inline void Connection::Init()
{
	debug("Initializing global connection data");
}

inline void Connection::Final()
{
}

inline Connection::Connection(const char* host, int port, bool tls) :
	m_host(host ? host : ""), m_port(port), m_tls(tls)
{
	debug("Creating Connection");

	m_readBuf.Reserve(nzbget_rs_dqh_conn_calc_read_buffer_cap(
		CONNECTION_READBUFFER_SIZE, DECODE_SLACK));
#ifndef DISABLE_TLS
	m_certVerifLevel = Options::ECertVerifLevel::cvStrict;
#endif
}

inline Connection::Connection(SOCKET socket, bool tls)
{
	debug("Creating Connection");

	m_port = 0;
	m_tls = tls;
	m_status = csConnected;
	m_socket = socket;
	m_host = GetRemoteAddr();
	m_bufAvail = 0;
	m_timeout = 60;
	m_suppressErrors = true;
	m_readBuf.Reserve(nzbget_rs_dqh_conn_calc_read_buffer_cap(
		CONNECTION_READBUFFER_SIZE, DECODE_SLACK));
#ifndef DISABLE_TLS
	m_tlsSocket = nullptr;
	m_tlsError = false;
	m_certVerifLevel = Options::ECertVerifLevel::cvStrict;
#endif
}

inline Connection::~Connection()
{
	debug("Destroying Connection");

	Connection::Disconnect();
}

inline void Connection::SetSuppressErrors(bool suppressErrors)
{
	m_suppressErrors = suppressErrors;
#ifndef DISABLE_TLS
	if (m_tlsSocket)
	{
		m_tlsSocket->SetSuppressErrors(suppressErrors);
	}
#endif
}

inline bool Connection::Connect()
{
	debug("Connecting");

	if (m_status == csConnected)
	{
		return true;
	}

	bool res = DoConnect();

	if (res)
	{
		m_status = csConnected;
	}
	else
	{
		DoDisconnect();
	}

	return res;
}

inline bool Connection::Disconnect()
{
	debug("Disconnecting");

	if (m_status == csDisconnected)
	{
		return true;
	}

	DoDisconnect();

	m_status = csDisconnected;
	m_socket = INVALID_SOCKET;
	m_bufAvail = 0;

	return true;
}

inline bool Connection::Bind()
{
	debug("Binding");

	if (m_status == csListening)
	{
		return true;
	}

	int errcode = 0;

	if (nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0)
	{
		struct sockaddr_un addr;
		addr.sun_family = AF_UNIX;
		if (nzbget_rs_dqh_conn_is_unix_path_too_long(m_host.size(), sizeof(addr.sun_path)) != 0)
		{
			ReportError("Binding socket failed for %s: name too long\n", m_host.c_str(), false);
			return false;
		}
		strcpy(addr.sun_path, m_host.c_str());

		m_socket = socket(PF_UNIX, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		unlink(m_host.c_str());

		if (bind(m_socket, (struct sockaddr *)&addr, sizeof(addr)) == -1)
		{
			// Connection failed
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
		}
	}
	else
	{
#ifdef HAVE_GETADDRINFO
		struct addrinfo addr_hints, *addr_list, *addr;

		memset(&addr_hints, 0, sizeof(addr_hints));
		addr_hints.ai_family = nzbget_rs_ccm_conn_select_ai_family(
			(int)m_ipVersion, (int)ipV4, (int)ipV6, AF_INET, AF_INET6, AF_UNSPEC);
		addr_hints.ai_socktype = SOCK_STREAM;
		addr_hints.ai_flags = AI_PASSIVE;    // For wildcard IP address

		NzbgetRsBuf portStrBuf = nzbget_rs_dqh_conn_format_port_str(m_port);
		BString<100> portStr("%s", portStrBuf.data ? portStrBuf.data : "0");
		nzbget_rs_free(portStrBuf);

		int res = getaddrinfo(m_host.c_str(), portStr, &addr_hints, &addr_list);
		if (res != 0)
		{
			int gaiErr = nzbget_rs_dqh_conn_resolve_gai_report_err_code(res, EAI_SYSTEM);
			ReportError("Could not resolve hostname %s", m_host.c_str(), true,
				gaiErr,
				gaiErr != 0 ? gai_strerror(res) : nullptr);
			return false;
		}

		m_socket = INVALID_SOCKET;
		for (addr = addr_list; addr != nullptr; addr = addr->ai_next)
		{
			m_socket = socket(addr->ai_family, addr->ai_socktype | SOCKET_CLOEXEC, addr->ai_protocol);
#ifndef SOCK_CLOEXEC
			SetCloseOnExec(m_socket);
#endif
			if (m_socket != INVALID_SOCKET)
			{
				int opt = 1;
				setsockopt(m_socket, SOL_SOCKET, SO_REUSEADDR, (char*)&opt, sizeof(opt));
				res = bind(m_socket, addr->ai_addr, addr->ai_addrlen);
				if (res != -1)
				{
					// Connection established
					break;
				}
				// Connection failed
				errcode = GetLastNetworkError();
				closesocket(m_socket);
				m_socket = INVALID_SOCKET;
			}
		}

		freeaddrinfo(addr_list);

#else

		struct sockaddr_in	sSocketAddress;
		memset(&sSocketAddress, 0, sizeof(sSocketAddress));
		sSocketAddress.sin_family = AF_INET;
		if (m_host.empty())
		{
			sSocketAddress.sin_addr.s_addr = htonl(INADDR_ANY);
		}
		else
		{
			sSocketAddress.sin_addr.s_addr = ResolveHostAddr(m_host.c_str());
			if (sSocketAddress.sin_addr.s_addr == INADDR_NONE)
			{
				return false;
			}
		}
		sSocketAddress.sin_port = htons(m_port);

		m_socket = socket(PF_INET, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		int opt = 1;
		setsockopt(m_socket, SOL_SOCKET, SO_REUSEADDR, (char*)&opt, sizeof(opt));

		int res = bind(m_socket, (struct sockaddr *) &sSocketAddress, sizeof(sSocketAddress));
		if (res == -1)
		{
			// Connection failed
			errcode = GetLastNetworkError();
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
		}
#endif
	}

	if (m_socket == INVALID_SOCKET)
	{
		ReportError("Binding socket failed for %s", m_host.c_str(), true, errcode);
		return false;
	}

	if (listen(m_socket, 100) < 0)
	{
		ReportError("Listen on socket failed for %s", m_host.c_str(), true);
		closesocket(m_socket);
		m_socket = INVALID_SOCKET;
		return false;
	}

	m_status = csListening;

	return true;
}

inline int Connection::WriteLine(const char* buffer)
{
	//debug("Connection::WriteLine");

	if (m_status != csConnected)
	{
		return -1;
	}

	int res = send(m_socket, buffer, strlen(buffer), 0);
	if (res <= 0)
	{
		m_status = csBroken;
	}

	return res;
}

inline bool Connection::Send(const char* buffer, int size)
{
	debug("Sending data");

	if (m_status != csConnected)
	{
		return false;
	}

	int bytesSent = 0;
	while (bytesSent < size)
	{
		int res = send(m_socket, buffer + bytesSent, size-bytesSent, 0);
		if (res <= 0)
		{
			m_status = csBroken;
			return false;
		}
		bytesSent += res;
	}

	return true;
}

inline char* Connection::ReadLine(char* buffer, int size, int* bytesReadOut)
{
	if (m_status != csConnected)
	{
		return nullptr;
	}

	char* inpBuffer = buffer;
	size--; // for trailing '0'
	int bytesRead = 0;
	int bufAvail = m_bufAvail; // local variable is faster
	char* bufPtr = m_bufPtr; // local variable is faster
	while (size)
	{
		if (!bufAvail)
		{
			bufAvail = recv(m_socket, m_readBuf, CONNECTION_READBUFFER_SIZE, 0);
			if (bufAvail < 0)
			{
				ReportError("Could not receive data on socket from %s", m_host.c_str(), true);
				m_status = csBroken;
				break;
			}
			else if (bufAvail == 0)
			{
				break;
			}
			else
			{
				m_totalBytesRead += bufAvail;
			}
			bufPtr = m_readBuf;
			m_readBuf[bufAvail] = '\0';
		}

		int foundNewline = 0;
		int len = nzbget_rs_rco_conn_read_line_chunk(bufPtr, bufAvail, size, &foundNewline);

		memcpy(inpBuffer, bufPtr, len);
		inpBuffer += len;
		bufPtr += len;
		bufAvail -= len;
		bytesRead += len;
		size -= len;

		if (foundNewline)
		{
			break;
		}
	}
	*inpBuffer = '\0';

	m_bufAvail = nzbget_rs_dqh_conn_clamp_buf_avail(bufAvail); // copy back to member
	m_bufPtr = bufPtr; // copy back to member

	if (bytesReadOut)
	{
		*bytesReadOut = bytesRead;
	}

	if (inpBuffer == buffer)
	{
		return nullptr;
	}

	return buffer;
}

inline std::unique_ptr<Connection> Connection::Accept()
{
	debug("Accepting connection");

	if (m_status != csListening)
	{
		return nullptr;
	}

	m_acceptBusy = false;
	SOCKET socket;
	for (;;)
	{
#if defined(__linux__) && defined(SOCK_CLOEXEC)
		socket = accept4(m_socket, nullptr, nullptr, SOCK_CLOEXEC);
#else
		socket = accept(m_socket, nullptr, nullptr);
		SetCloseOnExec(socket);
#endif
		if (socket == INVALID_SOCKET && m_status != csCancelled)
		{
			// a connection that failed before it was taken: take the next one
			// (the listener was torn down and rebound for each of these)
			int err = errno;
			int enonetVal = 0;
#ifdef ENONET
			enonetVal = ENONET;
#endif
			int acceptClass = nzbget_rs_rco_conn_classify_accept_errno(
				err, EINTR, ECONNABORTED, EPROTO, ENETDOWN,
				ENOPROTOOPT, EHOSTDOWN, EHOSTUNREACH, EOPNOTSUPP,
				ENETUNREACH, enonetVal, EMFILE, ENFILE, ENOBUFS, ENOMEM);
			if (acceptClass == 1)
			{
				continue;
			}
			m_acceptBusy = (acceptClass == 2);
		}
		break;
	}
	if (socket == INVALID_SOCKET && m_status != csCancelled)
	{
		ReportError("Could not accept connection for %s", m_host.c_str(), true);
	}
	if (socket == INVALID_SOCKET)
	{
		return nullptr;
	}

	InitSocketOpts(socket);

	// InitSocketOpts() applied the listener's timeout to the new socket:
	// keep the Connection's own idea of it (used by the poll/select waits)
	// in step instead of the constructor's default
	auto acceptedConnection = std::make_unique<Connection>(socket, m_tls);
	acceptedConnection->SetTimeout(m_timeout);
	return acceptedConnection;
}

inline int Connection::TryRecv(char* buffer, int size)
{
	//debug("Receiving data");

	memset(buffer, 0, size);

	int received = recv(m_socket, buffer, size, 0);

	if (received < 0)
	{
		ReportError("Could not receive data on socket from %s", m_host.c_str(), true);
	}
	else
	{
		m_totalBytesRead += received;
	}

	return received;
}

inline bool Connection::Recv(char * buffer, int size)
{
	//debug("Receiving data (full buffer)");

	memset(buffer, 0, size);

	char* bufPtr = (char*)buffer;
	int NeedBytes = size;

	if (m_bufAvail > 0)
	{
		int len = nzbget_rs_ccm_conn_recv_buffered_chunk_len(size, m_bufAvail);
		memcpy(bufPtr, m_bufPtr, len);
		bufPtr += len;
		m_bufPtr += len;
		m_bufAvail -= len;
		NeedBytes -= len;
	}

	// Read from the socket until nothing remains
	while (NeedBytes > 0)
	{
		int received = recv(m_socket, bufPtr, NeedBytes, 0);
		// Did the recv succeed?
		if (received <= 0)
		{
			ReportError("Could not receive data on socket from %s", m_host.c_str(), true);
			return false;
		}
		bufPtr += received;
		NeedBytes -= received;
		m_totalBytesRead += received;
	}
	return true;
}

inline bool Connection::DoConnect()
{
	debug("Do connecting");
	if (m_deadline && m_deadline->Stopped()) return false;

	m_socket = INVALID_SOCKET;

	if (nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0)
	{
		struct sockaddr_un addr;
		addr.sun_family = AF_UNIX;
		if (nzbget_rs_dqh_conn_is_unix_path_too_long(m_host.size(), sizeof(addr.sun_path)) != 0)
		{
			ReportError("Connection to %s failed: name too long\n", m_host.c_str(), false);
			return false;
		}
		strcpy(addr.sun_path, m_host.c_str());

		m_socket = socket(PF_UNIX, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		if (!ConnectWithTimeout(&addr, sizeof(addr)))
		{
			ReportError("Connection to %s failed", m_host.c_str(), true);
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
			return false;
		}
	}
	else
	{
#ifdef HAVE_GETADDRINFO
		struct addrinfo addr_hints, *addr_list, *addr;

		memset(&addr_hints, 0, sizeof(addr_hints));
		addr_hints.ai_family = nzbget_rs_ccm_conn_select_ai_family(
			(int)m_ipVersion, (int)ipV4, (int)ipV6, AF_INET, AF_INET6, AF_UNSPEC);
		addr_hints.ai_socktype = SOCK_STREAM;

		NzbgetRsBuf portStrBuf = nzbget_rs_dqh_conn_format_port_str(m_port);
		BString<100> portStr("%s", portStrBuf.data ? portStrBuf.data : "0");
		nzbget_rs_free(portStrBuf);

		// Optimizer probes resolve in a bounded owned child, preserving m_host
		// for TLS SNI/certificate validation. AI_NUMERICHOST never queries DNS.
		std::vector<std::unique_ptr<addrinfo, decltype(&freeaddrinfo)>> resolvedLists;
		std::vector<addrinfo> resolvedItems;
		int res;
		if (!m_resolvedAddresses.empty())
		{
			addr_hints.ai_flags |= AI_NUMERICHOST;
			addr_list = nullptr;
			for (const auto& numeric : m_resolvedAddresses)
			{
				addrinfo* list = nullptr;
				if (getaddrinfo(numeric.c_str(), portStr, &addr_hints, &list) == 0 && list)
				{
					resolvedLists.emplace_back(list, &freeaddrinfo);
					for (addrinfo* item = list; item; item = item->ai_next) resolvedItems.push_back(*item);
				}
			}
			for (size_t i = 0; i < resolvedItems.size(); ++i)
				resolvedItems[i].ai_next = i + 1 < resolvedItems.size() ? &resolvedItems[i + 1] : nullptr;
			addr_list = resolvedItems.empty() ? nullptr : &resolvedItems.front();
			res = addr_list ? 0 : EAI_NONAME;
		}
		else if (m_deadline)
		{
			return false; // Probe DNS must never fall back to blocking resolution.
		}
		else res = getaddrinfo(m_host.c_str(), portStr, &addr_hints, &addr_list);
		debug("getaddrinfo for %s: %i", m_host.c_str(), res);

		if (res != 0)
		{
			int gaiErr = nzbget_rs_dqh_conn_resolve_gai_report_err_code(res, EAI_SYSTEM);
			ReportError("Could not resolve hostname %s", m_host.c_str(), true,
						gaiErr,
						gaiErr != 0 ? gai_strerror(res) : nullptr);
			return false;
		}

		std::vector<SockAddr> triedAddr;
		bool connected = false;

		for (addr = addr_list; addr != nullptr; addr = addr->ai_next)
		{
			if (m_deadline && m_deadline->Stopped()) break;
			// don't try the same combinations of ai_family, ai_socktype, ai_protocol multiple times
			SockAddr sa = { addr->ai_family, addr->ai_socktype, addr->ai_protocol };
			if (!m_deadline && std::find(triedAddr.begin(), triedAddr.end(), sa) != triedAddr.end())
			{
				continue;
			}
			triedAddr.push_back(sa);

			if (m_socket != INVALID_SOCKET)
			{
				closesocket(m_socket);
			}

			m_socket = socket(addr->ai_family, addr->ai_socktype | SOCKET_CLOEXEC, addr->ai_protocol);
#ifndef SOCK_CLOEXEC
			SetCloseOnExec(m_socket);
#endif
			if (m_socket == INVALID_SOCKET)
			{
				// try another addr/family/protocol
				continue;
			}

			if (ConnectWithTimeout(addr->ai_addr, addr->ai_addrlen))
			{
				// Connection established
				connected = true;
				break;
			}
		}

		if (nzbget_rs_dqh_conn_should_report_socket_create_failed(
			m_socket == INVALID_SOCKET ? 1 : 0, addr_list ? 1 : 0) != 0)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
		}

		if (nzbget_rs_dqh_conn_should_cleanup_unconnected_socket(
			connected ? 1 : 0, m_socket == INVALID_SOCKET ? 1 : 0) != 0)
		{
			ReportError("Connection to %s failed", m_host.c_str(), true);
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
		}

		if (resolvedLists.empty()) freeaddrinfo(addr_list);

		if (m_socket == INVALID_SOCKET)
		{
			return false;
		}

#else

		struct sockaddr_in	sSocketAddress;
		memset(&sSocketAddress, 0, sizeof(sSocketAddress));
		sSocketAddress.sin_family = AF_INET;
		sSocketAddress.sin_port = htons(m_port);
		sSocketAddress.sin_addr.s_addr = ResolveHostAddr(m_host.c_str());
		if (sSocketAddress.sin_addr.s_addr == INADDR_NONE)
		{
			return false;
		}

		m_socket = socket(PF_INET, SOCK_STREAM | SOCKET_CLOEXEC, 0);
#ifndef SOCK_CLOEXEC
		SetCloseOnExec(m_socket);
#endif
		if (m_socket == INVALID_SOCKET)
		{
			ReportError("Socket creation failed for %s", m_host.c_str(), true);
			return false;
		}

		if (!ConnectWithTimeout(&sSocketAddress, sizeof(sSocketAddress)))
		{
			ReportError("Connection to %s failed", m_host.c_str(), true);
			closesocket(m_socket);
			m_socket = INVALID_SOCKET;
			return false;
		}
#endif
	}

	if (!InitSocketOpts(m_socket))
	{
		return false;
	}

#ifndef DISABLE_TLS
	if (m_tls && !StartTls(true, "", ""))
	{
		return false;
	}
#endif

	return true;
}

inline bool Connection::InitSocketOpts(SOCKET socket)
{
	struct timeval TimeVal;
	TimeVal.tv_sec = m_timeout;
	TimeVal.tv_usec = 0;
	char* optbuf = (char*)&TimeVal;
	int optsize = sizeof(TimeVal);
	int err = setsockopt(socket, SOL_SOCKET, SO_RCVTIMEO, optbuf, optsize);
	if (err != 0)
	{
		ReportError("Socket initialization failed for %s", m_host.c_str(), true);
		return false;
	}
	err = setsockopt(socket, SOL_SOCKET, SO_SNDTIMEO, optbuf, optsize);
	if (err != 0)
	{
		ReportError("Socket initialization failed for %s", m_host.c_str(), true);
		return false;
	}
	return true;
}

inline bool Connection::ConnectWithTimeout(void* address, int address_len)
{
	int flags = 0, error = 0, ret = 0;
	socklen_t len = sizeof(error);

	//set socket nonblocking flag
	flags = fcntl(m_socket, F_GETFL, 0);
	if (flags < 0)
	{
		return false;
	}

	if (fcntl(m_socket, F_SETFL, flags | O_NONBLOCK) < 0)
	{
		return false;
	}

	//initiate non-blocking connect
	ret = connect(m_socket, (struct sockaddr*)address, address_len);
	if (ret < 0)
	{
		int err = GetLastNetworkError();
		if (err != EINPROGRESS)
		{
			return false;
		}
	}

	//connect succeeded right away?
	if (ret != 0)
	{
		// poll: an fd_set holds descriptors below 1024 only, and FD_SET of a
		// higher one (many connections, a raised descriptor limit) wrote past it
		struct pollfd pfd;
		pfd.fd = m_socket;
		pfd.events = POLLIN | POLLOUT;
		pfd.revents = 0;
		if (m_deadline)
		{
			ret = m_deadline->Wait(m_socket, true, true) ? 1 : 0;
			pfd.revents = POLLIN | POLLOUT;
		}
		else ret = poll(&pfd, 1, nzbget_rs_ccm_conn_poll_timeout_ms(m_timeout));
		//we are waiting for connect to complete now
		if (ret < 0)
		{
			return false;
		}
		if (ret == 0)
		{
			//we had a timeout
			errno = ETIMEDOUT;
			return false;
		}

		if (!nzbget_rs_ccm_conn_poll_revents_ready(pfd.revents, POLLIN | POLLOUT | POLLERR | POLLHUP))
		{
			return false;
		}
		//we had a positivite return so a descriptor is ready

		if (getsockopt(m_socket, SOL_SOCKET, SO_ERROR, (char*)&error, &len) < 0)
		{
			return false;
		}

		//check if we had a socket error
		if (error)
		{
			errno = error;
			return false;
		}
	}

	//put socket back in blocking mode
	if (m_deadline) return !m_deadline->Stopped();
	if (fcntl(m_socket, F_SETFL, flags) < 0)
	{
		return false;
	}

	return true;
}

inline void Connection::DoDisconnect()
{
	debug("Do disconnecting");

	if (m_socket != INVALID_SOCKET)
	{
#ifndef DISABLE_TLS
		CloseTls();
#endif
		int is_listening = 0;
		socklen_t len = sizeof(is_listening);
		bool isUnixPath = nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0;
		int sockoptRes = isUnixPath ? getsockopt(m_socket, SOL_SOCKET, SO_ACCEPTCONN, &is_listening, &len) : -1;
		if (nzbget_rs_dqh_conn_should_unlink_listening_unix_socket(
			isUnixPath ? 1 : 0, sockoptRes, is_listening) != 0)
		{
			unlink(m_host.c_str());
		}
		if (m_gracefull)
		{
			closesocket_gracefully(m_socket);
		}
		else
		{
			closesocket(m_socket);
		}
		m_socket = INVALID_SOCKET;
	}

	m_status = csDisconnected;
}

inline void Connection::ReadBuffer(char** buffer, int *bufLen)
{
	*bufLen = m_bufAvail;
	*buffer = m_bufPtr;
	m_bufAvail = 0;
}

inline void Connection::Cancel()
{
	debug("Cancelling connection");
	if (m_socket != INVALID_SOCKET)
	{
		m_status = csCancelled;

		if (m_forceClose)
		{
			SOCKET socket = m_socket;
			m_socket = INVALID_SOCKET;
			shutdown(socket, SHUT_RDWR);
			closesocket(socket);
		}
		else
		{
			shutdown(m_socket, SHUT_RDWR);
		}
	}
}

inline int Connection::GetLastNetworkError()
{
	return errno;
}

inline void Connection::ReportError(const char* msgPrefix, const char* msgArg, bool printErrCode, int errCode, const char* errMsg)
{
#ifndef DISABLE_TLS
	if (m_tlsError)
	{
		// TLS-Error was already reported
		m_tlsError = false;
		return;
	}
#endif

	BString<1024> errPrefix(msgPrefix, msgArg);

	if (printErrCode)
	{
		BString<1024> printErrMsg;
		errCode = nzbget_rs_dqh_conn_resolve_report_err_code(errCode, GetLastNetworkError());
		if (errMsg)
		{
			printErrMsg = errMsg;
		}
		else
		{
			printErrMsg = strerror(errCode);
		}

		NzbgetRsBuf formattedErr = nzbget_rs_dqh_conn_format_report_err_with_code(
			*errPrefix, errCode, (const char*)printErrMsg);
		if (m_suppressErrors)
		{
			debug("%s", formattedErr.data ? formattedErr.data : "");
		}
		else
		{
			PrintError(formattedErr.data ? formattedErr.data : "");
		}
		nzbget_rs_free(formattedErr);
	}
	else
	{
		if (m_suppressErrors)
		{
			debug("%s", *errPrefix);
		}
		else
		{
			PrintError(errPrefix);
		}
	}
}

inline void Connection::PrintError(const char* errMsg)
{
	error("%s", errMsg);
}

#ifndef DISABLE_TLS
inline bool Connection::StartTls(bool isClient, const char* certFile, const char* keyFile)
{
	debug("Starting TLS");

	m_tlsSocket = std::make_unique<TlsSocket>(m_socket, isClient, m_host.c_str(), certFile, keyFile, m_cipher.c_str(), m_certVerifLevel);
	m_tlsSocket->SetSuppressErrors(m_suppressErrors);
	m_tlsSocket->SetDeadline(m_deadline);

	bool started = m_tlsSocket->Start();
	m_tlsProbeFailure = m_tlsSocket->GetProbeFailure();
	return started;
}

inline void Connection::CloseTls()
{
	if (m_tlsSocket)
	{
		m_tlsSocket->Close();
		m_tlsSocket.reset();
	}
}

inline int Connection::recv(SOCKET s, char* buf, int len, int flags)
{
	int received = 0;

	if (m_tlsSocket)
	{
		m_tlsError = false;
		received = m_tlsSocket->Recv(buf, len);
		if (received < 0)
		{
			m_tlsError = true;
			return -1;
		}
	}
	else
	{
		received = ::recv(s, buf, len, flags);
	}
	return received;
}

inline int Connection::send(SOCKET s, const char* buf, int len, int flags)
{
	int sent = 0;

	if (m_tlsSocket)
	{
		m_tlsError = false;
		sent = m_tlsSocket->Send(buf, len);
		if (sent < 0)
		{
			m_tlsError = true;
			return -1;
		}
		return sent;
	}
	else
	{
		sent = ::send(s, buf, len, flags);
		return sent;
	}
}
#endif

#ifndef HAVE_GETADDRINFO
inline in_addr_t Connection::ResolveHostAddr(const char* host)
{
	in_addr_t uaddr = inet_addr(host);
	if (uaddr == INADDR_NONE)
	{
		struct hostent* hinfo;
		bool err = false;
		int h_errnop = 0;
#ifdef HAVE_GETHOSTBYNAME_R
		struct hostent hinfobuf;
		char strbuf[1024];
#ifdef HAVE_GETHOSTBYNAME_R_6
		err = gethostbyname_r(host, &hinfobuf, strbuf, sizeof(strbuf), &hinfo, &h_errnop);
		err = err || (hinfo == nullptr); // error on null hinfo (means 'no entry')
#endif
#ifdef HAVE_GETHOSTBYNAME_R_5
		hinfo = gethostbyname_r(host, &hinfobuf, strbuf, sizeof(strbuf), &h_errnop);
		err = hinfo == nullptr;
#endif
#ifdef HAVE_GETHOSTBYNAME_R_3
		//NOTE: gethostbyname_r with three parameters were not tested
		struct hostent_data hinfo_data;
		hinfo = gethostbyname_r((char*)host, (struct hostent*)hinfobuf, &hinfo_data);
		err = hinfo == nullptr;
#endif
#else
		std::lock_guard<std::mutex> guard(m_getHostByNameMutex);
		hinfo = gethostbyname(host);
		err = hinfo == nullptr;
#endif
		if (err)
		{
			ReportError("Could not resolve hostname %s", host, true, h_errnop, hstrerror(h_errnop));
			return INADDR_NONE;
		}

		memcpy(&uaddr, hinfo->h_addr_list[0], sizeof(uaddr));
	}
	return uaddr;
}
#endif

inline const char* Connection::GetRemoteAddr()
{
	if (nzbget_rs_ccm_conn_is_unix_domain_path(m_host.c_str()) != 0)
	{
		return "-";
	}

	m_remoteAddr.Clear();

	char peerName[1024];
	int peerNameLength = sizeof(peerName);
	if (getpeername(m_socket, (sockaddr*)&peerName, (SOCKLEN_T*)&peerNameLength) >= 0)
	{
		inet_ntop(((sockaddr_in*)&peerName)->sin_family,
			((sockaddr_in*)&peerName)->sin_family == AF_INET6 ?
			(void*)&((sockaddr_in6*)&peerName)->sin6_addr :
			(void*)&((sockaddr_in*)&peerName)->sin_addr,
			m_remoteAddr, m_remoteAddr.Capacity());
		m_remoteAddr[m_remoteAddr.Capacity() - 1] = '\0';
	}

	return m_remoteAddr;
}

inline int Connection::FetchTotalBytesRead()
{
	int total = m_totalBytesRead;
	m_totalBytesRead = 0;
	return total;
}

#endif
