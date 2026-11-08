/*
 *  This file is part of RustyGet.
 *
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

#ifndef SYSTEM_INFO_H
#define SYSTEM_INFO_H

#include <cstring>
#include <exception>
#include <functional>
#include <ostream>
#include <stdexcept>
#include <string>
#include <sys/utsname.h>
#include <sys/socket.h>
#include <netdb.h>
#include <arpa/inet.h>
#include <unistd.h>
#include "Log.h"
#include "Util.h"
#include "TlsSocket.h"
#include "nzbget_rs.h"

namespace System
{
	inline const char* IP_SERVICE = "ip.nzbget.com";

	struct Network
	{
		std::string publicIP;
		std::string privateIP;
	};

	inline Network GetNetwork()
	{
		Network network;

		try
		{
			std::string host = IP_SERVICE;
			addrinfo hints{};
			hints.ai_family = AF_UNSPEC;
			hints.ai_socktype = SOCK_STREAM;
			addrinfo* res = nullptr;
#ifndef DISABLE_TLS
			const char* port = "443";
#else
			const char* port = "80";
#endif
			if (getaddrinfo(host.c_str(), port, &hints, &res) != 0 || !res)
			{
				throw std::runtime_error("Failed to resolve host: " + host);
			}

			int fd = -1;
			for (addrinfo* p = res; p != nullptr; p = p->ai_next)
			{
				fd = socket(p->ai_family, p->ai_socktype, p->ai_protocol);
				if (fd < 0) continue;

				timeval tv{10, 0};
				setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
				setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));

				if (connect(fd, p->ai_addr, p->ai_addrlen) == 0)
				{
					break;
				}
				close(fd);
				fd = -1;
			}
			freeaddrinfo(res);

			if (fd < 0)
			{
				throw std::runtime_error("Failed to connect to host: " + host);
			}

			std::string localIP;
			sockaddr_storage localAddr{};
			socklen_t addrLen = sizeof(localAddr);
			if (getsockname(fd, reinterpret_cast<sockaddr*>(&localAddr), &addrLen) == 0)
			{
				char ipStr[INET6_ADDRSTRLEN] = {0};
				if (localAddr.ss_family == AF_INET)
				{
					inet_ntop(AF_INET, &reinterpret_cast<sockaddr_in*>(&localAddr)->sin_addr, ipStr, sizeof(ipStr));
				}
				else if (localAddr.ss_family == AF_INET6)
				{
					inet_ntop(AF_INET6, &reinterpret_cast<sockaddr_in6*>(&localAddr)->sin6_addr, ipStr, sizeof(ipStr));
				}
				localIP = ipStr;
			}

#ifndef DISABLE_GZIP
			int includeGzip = 1;
#else
			int includeGzip = 0;
#endif
			NzbgetRsBuf reqBuf = nzbget_rs_snh_http_client_build_headers(
				"GET", 3, host.data(), host.size(), Util::VersionRevision(), includeGzip);
			std::string req(reqBuf.data ? reqBuf.data : "", reqBuf.len);
			nzbget_rs_free(reqBuf);

			std::string rawResp;
#ifndef DISABLE_TLS
			SSL_CTX* ctx = SSL_CTX_new(TLS_client_method());
			if (!ctx)
			{
				close(fd);
				throw std::runtime_error("Failed to create SSL_CTX");
			}
			SSL_CTX_set_default_verify_paths(ctx);
			SSL* ssl = SSL_new(ctx);
			if (!ssl)
			{
				SSL_CTX_free(ctx);
				close(fd);
				throw std::runtime_error("Failed to create SSL");
			}
			SSL_set_fd(ssl, fd);
			SSL_set_tlsext_host_name(ssl, host.c_str());
			if (SSL_connect(ssl) != 1)
			{
				SSL_free(ssl);
				SSL_CTX_free(ctx);
				close(fd);
				OpenSSL::StopSSLThread();
				throw std::runtime_error("TLS handshake failed");
			}

			size_t written = 0;
			while (written < req.size())
			{
				int n = SSL_write(ssl, req.data() + written, static_cast<int>(req.size() - written));
				if (n <= 0) break;
				written += static_cast<size_t>(n);
			}

			char chunk[4096];
			for (;;)
			{
				int n = SSL_read(ssl, chunk, sizeof(chunk));
				if (n <= 0) break;
				rawResp.append(chunk, static_cast<size_t>(n));
			}

			SSL_shutdown(ssl);
			SSL_free(ssl);
			SSL_CTX_free(ctx);
			close(fd);
			OpenSSL::StopSSLThread();
#else
			size_t written = 0;
			while (written < req.size())
			{
				ssize_t n = send(fd, req.data() + written, req.size() - written, 0);
				if (n <= 0) break;
				written += static_cast<size_t>(n);
			}

			char chunk[4096];
			for (;;)
			{
				ssize_t n = recv(fd, chunk, sizeof(chunk), 0);
				if (n <= 0) break;
				rawResp.append(chunk, static_cast<size_t>(n));
			}
			close(fd);
#endif

			uint32_t statusCode = 0;
			NzbgetRsBuf bodyBuf{nullptr, 0, 0};
			if (!nzbget_rs_http_client_parse_response(rawResp.data(), rawResp.size(), &statusCode, &bodyBuf))
			{
				throw std::runtime_error("Invalid HTTP response");
			}
			std::string body(bodyBuf.data ? bodyBuf.data : "", bodyBuf.len);
			nzbget_rs_free(bodyBuf);

			if (statusCode == 200)
			{
				network.publicIP = std::move(body);
				network.privateIP = std::move(localIP);
			}
		}
		catch (const std::exception& e)
		{
			detail("Failed to get public and private IP: %s", e.what());
		}

		return network;
	}

	class SystemInfo final
	{
	public:
		SystemInfo(std::function<Network()> networkFetcher = GetNetwork)
			: m_networkFetcher{std::move(networkFetcher)}
		{
			utsname uts{};
			if (uname(&uts) == 0)
			{
				m_sysname = uts.sysname;
				m_release = uts.release;
				m_machine = uts.machine;
			}
#ifndef DISABLE_TLS
#ifdef LIBRESSL_VERSION_TEXT
			m_sslLibName = "LibreSSL";
			m_sslRawVersion = LIBRESSL_VERSION_TEXT;
			m_sslExtractAfterSpace = true;
#elif defined(OPENSSL_FULL_VERSION_STR)
			m_sslLibName = "OpenSSL";
			m_sslRawVersion = OPENSSL_FULL_VERSION_STR;
			m_sslExtractAfterSpace = false;
#else
			m_sslLibName = "OpenSSL";
			m_sslRawVersion = OPENSSL_VERSION_TEXT;
			m_sslExtractAfterSpace = true;
#endif
#endif
		}

		~SystemInfo() = default;

		Network GetNetworkInfo() const
		{
			return m_networkFetcher();
		}

		std::string Format(int mode, bool withNetwork) const
		{
			Network net = withNetwork ? GetNetworkInfo() : Network{};
			NzbgetRsBuf buf = nzbget_rs_system_info_format(
				mode,
				m_sysname.c_str(),
				m_release.c_str(),
				m_machine.c_str(),
				m_sslLibName.c_str(),
				m_sslRawVersion.c_str(),
				m_sslExtractAfterSpace ? 1 : 0,
				net.publicIP.data(), net.publicIP.size(),
				net.privateIP.data(), net.privateIP.size());
			std::string result(buf.data ? buf.data : "", buf.len);
			nzbget_rs_free(buf);
			return result;
		}

		friend std::ostream& operator<<(std::ostream& os, const SystemInfo& sysinfo)
		{
			std::string text = sysinfo.Format(0, false);
			if (!text.empty())
			{
				os.write(text.data(), static_cast<std::streamsize>(text.size()));
			}
			return os;
		}

	private:
		std::string m_sysname;
		std::string m_release;
		std::string m_machine;
		std::string m_sslLibName;
		std::string m_sslRawVersion;
		bool m_sslExtractAfterSpace = false;
		std::function<Network()> m_networkFetcher;
	};

	inline std::string ToJsonStr(const SystemInfo& sysInfo)
	{
		return sysInfo.Format(1, true);
	}

	inline std::string ToXmlStr(const SystemInfo& sysInfo)
	{
		return sysInfo.Format(2, true);
	}
}

extern System::SystemInfo* g_SystemInfo;

#endif
