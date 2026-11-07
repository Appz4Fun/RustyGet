/*
 *  This file is part of RustyGet.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

#include "nzbget.h"

#include "HttpClient.h"
#include "Util.h"
#include "nzbget_rs.h"

#include <sstream>

namespace Network
{
	const std::string& HttpClient::GetLocalIP() const
	{
		return m_localIP;
	}

	std::string HttpClient::GetProtocol() const
	{
#ifndef DISABLE_TLS
		return "443";
#else
		return "80";
#endif
	}

	std::string HttpClient::GetHeaders(const std::string& method, const std::string& host) const
	{
#ifndef DISABLE_GZIP
		int includeGzip = 1;
#else
		int includeGzip = 0;
#endif
		NzbgetRsBuf buf = nzbget_rs_snh_http_client_build_headers(
			method.data(), method.size(),
			host.data(), host.size(),
			Util::VersionRevision(),
			includeGzip);
		std::string headers(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return headers;
	}

	std::future<Response> HttpClient::GET(std::string host_)
	{
		return std::async(std::launch::async, [this, host = std::move(host_)]() -> Response
		{
			addrinfo hints{};
			hints.ai_family = AF_UNSPEC;
			hints.ai_socktype = SOCK_STREAM;
			addrinfo* res = nullptr;
			std::string port = GetProtocol();
			if (getaddrinfo(host.c_str(), port.c_str(), &hints, &res) != 0 || !res)
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
				m_localIP = ipStr;
			}

			std::string req = GetHeaders("GET", host);
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

			size_t headerEnd = rawResp.find("\r\n\r\n");
			if (headerEnd == std::string::npos)
			{
				throw std::runtime_error("Invalid HTTP response");
			}

			std::istringstream stream(rawResp.substr(0, headerEnd + 2));
			std::string httpVersion;
			unsigned statusCode = 0;
			stream >> httpVersion >> statusCode;
			if (!nzbget_rs_snh_http_client_validate_version(
				httpVersion.data(), httpVersion.size(), stream ? 1 : 0))
			{
				throw std::runtime_error("Invalid response");
			}

			std::string dummyLine;
			std::getline(stream, dummyLine);

			Headers headers;
			std::string line;
			while (std::getline(stream, line) && line != "\r" && !line.empty())
			{
				size_t colonPos = 0;
				if (nzbget_rs_snh_http_client_find_header_colon(line.c_str(), line.size(), &colonPos))
				{
					std::string key = line.substr(0, colonPos);
					std::string value = line.substr(colonPos + 1);
					Util::Trim(value);
					headers.emplace(std::move(key), std::move(value));
				}
			}

			std::string body = rawResp.substr(headerEnd + 4);
			Util::Trim(body);

			Response response;
			response.statusCode = statusCode;
			response.headers = std::move(headers);
			response.body = std::move(body);
			return response;
		});
	}
}
