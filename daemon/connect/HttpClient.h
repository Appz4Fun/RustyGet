/*
 *  This file is part of RustyGet.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

#ifndef HTTP_CLIENT_H
#define HTTP_CLIENT_H

#include <future>
#include <string>
#include <unordered_map>

namespace Network
{
	using Headers = std::unordered_map<std::string, std::string>;

	struct Response
	{
		Headers headers;
		std::string body;
		unsigned statusCode = 0;
	};

	class HttpClient final
	{
	public:
		HttpClient() noexcept(false) = default;
		HttpClient(const HttpClient&) = delete;
		HttpClient operator=(const HttpClient&) = delete;
		~HttpClient() = default;

		std::future<Response> GET(std::string host);
		const std::string& GetLocalIP() const;

	private:
		std::string GetHeaders(const std::string& method, const std::string& host) const;
		std::string GetProtocol() const;
		std::string m_localIP;
	};
}

#endif
