/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2012-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef WEBSERVER_H
#define WEBSERVER_H

#include "NString.h"
#include "Connection.h"

inline constexpr int TOKEN_SIZE = 48 + 1;

class WebProcessor
{
public:
	enum EHttpMethod
	{
		hmPost,
		hmGet,
		hmOptions
	};

	static void Init();
	void Execute();
	void SetConnection(Connection* connection) { m_connection = connection; }
	void SetUrl(const char* url) { m_url = url; }
	void SetHttpMethod(EHttpMethod httpMethod) { m_httpMethod = httpMethod; }
	bool GetKeepAlive() { return m_keepAlive; }

private:
	enum EUserAccess
	{
		uaControl,
		uaRestricted,
		uaAdd
	};

	Connection* m_connection = nullptr;
	CString m_request;
	CString m_url;
	EHttpMethod m_httpMethod;
	EUserAccess m_userAccess;
	bool m_rpcRequest;
	bool m_authorized;
	bool m_gzip;
	CString m_origin;
	int m_contentLen;
	char m_authInfo[256+1];
	char m_authToken[TOKEN_SIZE];
	inline static char m_serverAuthToken[3][TOKEN_SIZE]{};
	CString m_forwardedFor;
	CString m_oldETag;
	bool m_keepAlive = false;
	std::hash<std::string_view> m_hasher;

	void Dispatch();
	void SendAuthResponse();
	void SendOptionsResponse();
	void SendErrorResponse(const char* errCode, bool printWarning);
	void SendSingleFileResponse();
	void SendMultiFileResponse();
	void SendBodyResponse(const char* body, int bodyLen, const char* contentType, bool cachable);
	void SendRedirectResponse(const char* url);
	const char* DetectContentType(const char* filename);
	void ParseHeaders();
	bool ParseUrl();
	bool CheckCredentials();
};

#include <climits>
#include "nzbget_rs.h"
#include "XmlRpc.h"
#include "Log.h"
#include "Options.h"
#include "Util.h"
#include "FileSystem.h"

#ifndef DISABLE_TLS
#include <openssl/rand.h>
#endif

namespace WebServerDetail
{
	inline constexpr const char* ERR_HTTP_BAD_REQUEST = "400 Bad Request";
	inline constexpr const char* ERR_HTTP_NOT_FOUND = "404 Not Found";
	inline constexpr const char* ERR_HTTP_SERVICE_UNAVAILABLE = "503 Service Unavailable";
	inline constexpr int MAX_UNCOMPRESSED_SIZE = 500;

	inline void GenerateSecureToken(char* token)
	{
		// 1 byte = 2 hex characters.
		// For a token string of size TOKEN_SIZE (including \0), we need half that many random bytes.
		constexpr size_t BYTES_NEEDED = (TOKEN_SIZE - 1) / 2;

#ifndef DISABLE_TLS
		unsigned char randBytes[BYTES_NEEDED];
		if (RAND_bytes(randBytes, static_cast<int>(BYTES_NEEDED)) == 1)
		{
			nzbget_rs_dqh_webserver_encode_hex_token(
				randBytes, BYTES_NEEDED, token, TOKEN_SIZE);
			return;
		}
#endif

		try
		{
			std::random_device rd;
			std::uniform_int_distribution<unsigned int> dist(0, 0xFF);
			uint8_t fallbackBytes[BYTES_NEEDED];
			for (size_t i = 0; i < BYTES_NEEDED; ++i)
			{
				fallbackBytes[i] = static_cast<uint8_t>(dist(rd));
			}
			nzbget_rs_dqh_webserver_encode_hex_token(
				fallbackBytes, BYTES_NEEDED, token, TOKEN_SIZE);
		}
		catch (const std::exception& ex)
		{
			error("Could not generate secure authentication token: %s", ex.what());
			exit(1);
		}
	}

	// tolower as WildMask's C++ code did: on a char, signed or not
	inline int WebFold(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		return tolower(ch);
#else
		return ch < 0 ? ch : tolower(ch);
#endif
	}
}

inline void WebProcessor::Init()
{
	if (m_serverAuthToken[0][0] != 0)
	{
		// already initialized
		return;
	}

	for (size_t j = uaControl; j <= uaAdd; ++j)
	{
		WebServerDetail::GenerateSecureToken(m_serverAuthToken[j]);
	}
}

inline void WebProcessor::Execute()
{
	m_gzip =false;
	m_userAccess = uaControl;
	m_authInfo[0] = '\0';
	m_authToken[0] = '\0';

	ParseHeaders();

	int preParseMethod = nzbget_rs_dqh_webserver_classify_pre_parse_method(
		(int)m_httpMethod, (int)hmPost, (int)hmOptions, m_contentLen);
	if (preParseMethod == 1)
	{
		// answered and closed: without an answer a kept-alive client waited until
		// its timeout, and a body (chunked) left unread would be the next request
		error("Invalid-request: content length is 0");
		m_keepAlive = false;
		SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, false);
		return;
	}

	if (preParseMethod == 2)
	{
		SendOptionsResponse();
		return;
	}

	if (!ParseUrl())
	{
		return;
	}

	m_rpcRequest = XmlRpcProcessor::IsRpcRequest(m_url);
	m_authorized = CheckCredentials();

	if (nzbget_rs_sfw_web_proc_should_send_auth_response(
		g_Options->GetFormAuth() ? 1 : 0, m_rpcRequest ? 1 : 0, m_authorized ? 1 : 0) != 0)
	{
		SendAuthResponse();
		return;
	}

	if (m_httpMethod == hmPost)
	{
		// a body of a size no request has: the buffer for it couldn't be allocated
		// (its size + 1 overflowed at INT_MAX) and was written through anyway. A
		// request without credentials (the login form) is small
		if (!nzbget_rs_ntw_web_proc_is_content_len_valid(m_contentLen, m_authorized ? 1 : 0))
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, true);
			return;
		}

		// reading http body (request content)
		m_request.Reserve(m_contentLen);
		if (!m_request)
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_SERVICE_UNAVAILABLE, true);
			return;
		}
		m_request[m_contentLen] = '\0';

		if (!m_connection->Recv(m_request, m_contentLen))
		{
			error("Invalid-request: could not read data");
			m_keepAlive = false;
			return;
		}
		debug("Request=%s", *m_request);
	}

	debug("request received from %s", m_connection->GetRemoteAddr());

	Dispatch();
}

inline void WebProcessor::ParseHeaders()
{
	// Keep the entire initialized buffer: in unusual locales strncasecmp can
	// match at an early NUL, but C++ still reads values at fixed offsets.
	char buffer[1024]{};
	m_contentLen = 0;
	while (char* p = m_connection->ReadLine(buffer, sizeof(buffer), nullptr))
	{
		size_t crPos = 0;
		if (nzbget_rs_dqh_scanner_find_last_char_pos(p, '\r', &crPos) != 0)
		{
			p[crPos] = '\0';
		}
		debug("header=%s", p);

		size_t start = 0, len = 0;
		int number = 0;
		int kind = nzbget_rs_web_header(p, sizeof(buffer), Util::EmptyStr(m_authInfo), &start, &len, &number);
		std::string value(p + start, len);
		switch (kind)
		{
			case 1:
				m_contentLen = number;
				break;
			case 2:
				m_authInfo[WebUtil::DecodeBase64(p + start, 0, m_authInfo)] = '\0';
				break;
			case 3:
				error("Invalid-request: auth-info too big");
				return;
			case 4:
				m_gzip = number != 0;
				break;
			case 5:
				m_origin = value.c_str();
				break;
			case 6:
				strncpy(m_authToken, value.c_str(), sizeof(m_authToken) - 1);
				m_authToken[sizeof(m_authToken) - 1] = '\0';
				break;
			case 7:
				m_forwardedFor = value.c_str();
				break;
			case 8:
				m_oldETag = value.c_str();
				break;
			case 9:
				m_keepAlive = true;
				break;
			case 10:
				debug("URL=%s", *m_url);
				return;
		}
	}

	debug("URL=%s", *m_url);
}

// false when it answered the request itself (a redirect)
inline bool WebProcessor::ParseUrl()
{
	// rust/src/webserver.rs
	int redirect = 0;
	NzbgetRsBuf auth{};
	NzbgetRsBuf url = nzbget_rs_web_parse_url(m_url, strlen(m_url), &redirect, &auth);
	// Both buffers belong to Rust, including if constructing target throws.
	auto release = [](NzbgetRsBuf* buf) { nzbget_rs_free(*buf); };
	std::unique_ptr<NzbgetRsBuf, decltype(release)> urlOwner(&url, release);
	std::unique_ptr<NzbgetRsBuf, decltype(release)> authOwner(&auth, release);
	std::string target(url.data, url.len);
	if (redirect)
	{
		SendRedirectResponse(target.c_str());
		return false;
	}
	if (auth.data)
	{
		size_t len = std::min(auth.len, sizeof(m_authInfo) - 1);
		memcpy(m_authInfo, auth.data, len);
		m_authInfo[len] = '\0';
	}
	m_url = target.c_str();

	debug("Final URL=%s", *m_url);
	return true;
}

inline bool WebProcessor::CheckCredentials()
{
	// rust/src/webserver.rs
	NzbgetRsWebCredentials input{};
	input.users[0] = g_Options->GetControlUsername();
	input.users[1] = g_Options->GetControlPassword();
	input.users[2] = g_Options->GetRestrictedUsername();
	input.users[3] = g_Options->GetRestrictedPassword();
	input.users[4] = g_Options->GetAddUsername();
	input.users[5] = g_Options->GetAddPassword();
	input.authorizedIp = g_Options->GetAuthorizedIp();
	input.remoteAddr = m_connection->GetRemoteAddr();
	input.authInfo = m_authInfo;
	input.authToken = m_authToken;
	for (int j = uaControl; j <= uaAdd; j++)
	{
		input.serverTokens[j] = m_serverAuthToken[j];
	}
#ifdef __GLIBC__
	input.lowerTable = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#endif
	input.charSigned = CHAR_MIN < 0;
	input.fold = WebServerDetail::WebFold;

	NzbgetRsWebCheck check{};
	nzbget_rs_web_check_credentials(&input, &check);
	if (check.authCut >= 0)
	{
		m_authInfo[check.authCut] = '\0';
	}
	if (check.access >= 0)
	{
		m_userAccess = (EUserAccess)check.access;
	}
	if (check.warn)
	{
		NzbgetRsBuf fwdBuf = nzbget_rs_dqh_webserver_format_forwarded_for_suffix(*m_forwardedFor);
		warn("Request received on port %i from %s%s, but username (%s) or password invalid",
			g_Options->GetControlPort(), m_connection->GetRemoteAddr(),
			fwdBuf.data ? fwdBuf.data : "",
			m_authInfo);
		nzbget_rs_free(fwdBuf);
	}
	return check.authorized != 0;
}

inline void WebProcessor::Dispatch()
{
	if (m_url[0] != '/')
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, true);
		return;
	}

	if (m_rpcRequest)
	{
		XmlRpcProcessor processor;
		processor.SetRequest(m_request);
		processor.SetHttpMethod(m_httpMethod == hmGet ? XmlRpcProcessor::hmGet : XmlRpcProcessor::hmPost);
		processor.SetUserAccess((XmlRpcProcessor::EUserAccess)m_userAccess);
		processor.SetJsonP(g_Options->GetJsonP());
		processor.SetUrl(m_url);
		processor.Execute();
		SendBodyResponse(processor.GetResponse(), strlen(processor.GetResponse()), processor.GetContentType(), processor.IsSafeMethod());
		return;
	}

	if (m_httpMethod != hmGet)
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_BAD_REQUEST, true);
		return;
	}

	int getRoute = nzbget_rs_sus_web_classify_get_dispatch(m_url);
	if (getRoute == 0)
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, true);
		return;
	}
	if (nzbget_rs_dqh_webserver_classify_ext_route(getRoute) != 0)
	{
		if (!m_authorized)
		{
			SendAuthResponse();
			return;
		}

		const auto& scriptDirs = g_Options->GetScriptDirPaths();
		if (scriptDirs.empty())
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_SERVICE_UNAVAILABLE, true);
			return;
		}

		if (getRoute == 1)
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, false);
			return;
		}

		std::string_view extUrl = m_url + 12;
		bool found = false;
		for (const auto& scriptDir : scriptDirs)
		{
			fs::path filePath = scriptDir / extUrl;
			fs::error_code ec;
			auto resolvedDir = fs::canonical(scriptDir, ec);
			if (ec) continue;

			auto resolvedPath = fs::canonical(filePath, ec);
			if (ec) continue;

			auto rel = resolvedPath.lexically_relative(resolvedDir);
			if (nzbget_rs_dqh_webserver_is_rel_path_outside_root(
				rel.empty() ? 1 : 0,
				rel.empty() ? '\0' : static_cast<char>(rel.native().front())) != 0) continue;

			CharBuffer body;
			std::string filePathStr = fs::u8string(resolvedPath);
			if (FileSystem::LoadFileIntoBuffer(filePathStr.c_str(), body, true))
			{
				const char* contentType = DetectContentType(filePathStr.c_str());
				int len = body.Size() - 1;
				SendBodyResponse(body, len, contentType, true);
				found = true;
				break;
			}
		}

		if (!found)
		{
			SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, false);
		}
		return;
	}

	if (Util::EmptyStr(g_Options->GetWebDir()))
	{
		SendErrorResponse(WebServerDetail::ERR_HTTP_SERVICE_UNAVAILABLE, true);
		return;
	}

	if (getRoute == 3)
	{
		SendMultiFileResponse();
	}
	else
	{
		SendSingleFileResponse();
	}
}

inline void WebProcessor::SendAuthResponse()
{
	NzbgetRsBuf hdr = nzbget_rs_web_format_auth_response(
		g_Options->GetFormAuth() ? 1 : 0, m_keepAlive ? 1 : 0, Util::VersionRevision());
	debug("ResponseHeader=%s", hdr.data ? hdr.data : "");
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
}

inline void WebProcessor::SendOptionsResponse()
{
	NzbgetRsBuf hdr = nzbget_rs_web_format_options_response(
		m_keepAlive ? 1 : 0, m_origin.Str(), Util::VersionRevision());
	debug("ResponseHeader=%s", hdr.data ? hdr.data : "");
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
}

inline void WebProcessor::SendErrorResponse(const char* errCode, bool printWarning)
{
	if (printWarning)
	{
		warn("Web-Server: %s, Resource: %s", errCode, *m_url);
	}

	NzbgetRsBuf body{};
	NzbgetRsBuf hdr = nzbget_rs_web_format_error_response(
		errCode, m_keepAlive ? 1 : 0, Util::VersionRevision(), &body);
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	if (body.data)
	{
		m_connection->Send(body.data, (int)body.len);
	}
	nzbget_rs_free(hdr);
	nzbget_rs_free(body);
}

inline void WebProcessor::SendRedirectResponse(const char* url)
{
	NzbgetRsBuf hdr = nzbget_rs_web_format_redirect_response(
		url, m_keepAlive ? 1 : 0, Util::VersionRevision());
	debug("ResponseHeader=%s", hdr.data ? hdr.data : "");
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
}

inline void WebProcessor::SendBodyResponse(const char* body, int bodyLen, const char* contentType, bool cachable)
{
	BString<1024> newETag;
	bool unchanged = false;

	if (cachable)
	{
		// over the whole body: as a C string it ended at the first zero byte, and
		// every image (PNG, ICO...) had the same ETag - a changed one was answered
		// "not modified"
		size_t hash = m_hasher(std::string_view(body, bodyLen));
		NzbgetRsBuf etagBuf = nzbget_rs_tpr_web_format_quoted_hex_etag(hash);
		newETag.Set(etagBuf.data ? etagBuf.data : "", (int)etagBuf.len);
		nzbget_rs_free(etagBuf);

		unchanged = nzbget_rs_sfw_web_proc_is_etag_unchanged(m_oldETag, newETag) != 0;
		if (unchanged)
		{
			body = "";
			bodyLen = 0;
		}
	}

#ifndef DISABLE_GZIP
	CharBuffer gbuf;
	bool gzip = nzbget_rs_ntw_web_proc_should_use_gzip(
		m_gzip ? 1 : 0, bodyLen, WebServerDetail::MAX_UNCOMPRESSED_SIZE) != 0;
	if (gzip)
	{
		uint32 outLen = ZLib::GZipLen(bodyLen);
		gbuf.Reserve(outLen);
		int gzippedLen = ZLib::GZip(body, bodyLen, *gbuf, outLen);
		if (nzbget_rs_tpr_web_is_gzip_smaller(gzippedLen, bodyLen) != 0)
		{
			body = gbuf;
			bodyLen = gzippedLen;
		}
		else
		{
			gzip = false;
		}
	}
#else
	bool gzip = false;
#endif

	NzbgetRsBuf hdr = nzbget_rs_web_format_body_response_header(
		unchanged ? 1 : 0,
		m_keepAlive ? 1 : 0,
		m_origin.Str(),
		g_Options->GetFormAuth() ? 1 : 0,
		m_authorized ? m_serverAuthToken[m_userAccess] : "",
		bodyLen,
		contentType,
		gzip ? 1 : 0,
		cachable ? *newETag : nullptr,
		Util::VersionRevision());

	debug("[%s] (%s) %s", *m_url, *m_oldETag, hdr.data ? hdr.data : "");

	// Send the request answer
	if (hdr.data)
	{
		m_connection->Send(hdr.data, (int)hdr.len);
	}
	nzbget_rs_free(hdr);
	m_connection->Send(body, bodyLen);
}

inline void WebProcessor::SendSingleFileResponse()
{
	const char* defRes = nzbget_rs_ntw_web_proc_default_resource(m_url);

	BString<1024> filename;
	{
		NzbgetRsBuf pathBuf = nzbget_rs_tpr_web_format_single_file_path(
			g_Options->GetWebDir(), *m_url, defRes);
		filename.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
		nzbget_rs_free(pathBuf);
	}

	debug("serving file: %s", *filename);

	CharBuffer body;
	if (!FileSystem::LoadFileIntoBuffer(filename, body, true))
	{
		// do not print warnings "404 not found" for certain files (by base
		// name: the full path never matched them; and the flag passed on is
		// "print warning", so no missing file was ever warned about)
		const char* baseName = FileSystem::BaseFileName(filename);
		bool ignorable = nzbget_rs_sus_web_is_ignorable_404(baseName) != 0;

		SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, !ignorable);
		return;
	}

	const char* contentType = DetectContentType(filename);
	int len = body.Size() - 1;

#ifdef DEBUG
	if (nzbget_rs_dqh_webserver_is_html_content_type(contentType) != 0)
	{
		Util::ReduceStr(body, "<!-- %if-debug%", "");
		Util::ReduceStr(body, "<!-- %if-not-debug% -->", "<!--");
		Util::ReduceStr(body, "<!-- %end% -->", "-->");
		Util::ReduceStr(body, "%end% -->", "");
		len = strlen(body);
	}
#endif

	SendBodyResponse(body, len, contentType, true);
}

inline void WebProcessor::SendMultiFileResponse()
{
	debug("serving multiple files: %s", *m_url);

	StringBuilder response;
	size_t qPos = 0;
	nzbget_rs_dqh_diskstate_find_char_pos(m_url, '?', &qPos);
	char* filelist = (char*)m_url + qPos;
	*filelist++ = '\0';

	Tokenizer tok(filelist, "+");
	while (const char* filename = tok.Next())
	{
		NzbgetRsBuf diskFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetWebDir(), PATH_SEPARATOR, filename);
		BString<1024> diskFilename;
		diskFilename.Set(diskFnBuf.data ? diskFnBuf.data : "", (int)diskFnBuf.len);
		nzbget_rs_free(diskFnBuf);

		CharBuffer body;
		if (!FileSystem::LoadFileIntoBuffer(diskFilename, body, true))
		{
			warn("Web-Server: %s, Resource: /%s", WebServerDetail::ERR_HTTP_NOT_FOUND, filename);
			SendErrorResponse(WebServerDetail::ERR_HTTP_NOT_FOUND, false);
			return;
		}

		response.Append(body);
	}

	SendBodyResponse(response, response.Length(), DetectContentType(m_url), true);
}

inline const char* WebProcessor::DetectContentType(const char* filename)
{
	return nzbget_rs_sus_web_detect_content_type(filename);
}

#include <system_error>
#include "BinRpc.h"

class RequestProcessor : public Thread, public Subject
{
public:
	~RequestProcessor()
	{
		m_connection->Disconnect();

#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}

	void Run() override
	{
		Execute();
		Notify(nullptr);
	}

	void Stop() override
	{
		Thread::Stop();
		m_connection->Cancel();
	}

	void SetTls(bool tls) { m_tls = tls; }
	void SetConnection(std::unique_ptr<Connection>&& connection) { m_connection = std::move(connection); }

private:
	bool m_tls;
	std::unique_ptr<Connection> m_connection;

	bool ServWebRequest(const char* signature)
	{
		char buffer[1024];
		if (!m_connection->ReadLine(buffer, sizeof(buffer), nullptr))
		{
			return false;
		}

		WebProcessor::EHttpMethod httpMethod = WebProcessor::hmGet;
		char* url = buffer;
		int rsMethod = 0;
		size_t urlOffset = 0;
		size_t urlLen = 0;
		if (!nzbget_rs_rco_remote_parse_web_request_line(
			signature, buffer, &rsMethod, &urlOffset, &urlLen))
		{
			return false;
		}
		httpMethod = (WebProcessor::EHttpMethod)rsMethod;
		url = buffer + urlOffset;
		url[urlLen] = '\0';

		debug("url: %s", url);

		WebProcessor processor;
		processor.SetConnection(m_connection.get());
		processor.SetUrl(url);
		processor.SetHttpMethod(httpMethod);
		processor.Execute();

		return processor.GetKeepAlive();
	}

	void Execute()
	{
		bool ok = false;
		m_connection->SetSuppressErrors(true);

#ifndef DISABLE_TLS
		if (m_tls && !m_connection->StartTls(false, g_Options->GetSecureCert(), g_Options->GetSecureKey()))
		{
			debug("Could not establish secure connection to web-client: Start TLS failed");
			return;
		}
#endif

		uint32 signature = 0;
		if (!m_connection->Recv((char*)&signature, 4))
		{
			debug("Could not read request signature");
			return;
		}

		int sigKind = nzbget_rs_rco_remote_classify_signature(
			(const char*)&signature, (uint32)NZBMESSAGE_SIGNATURE);
		if (sigKind == 1)
		{
			ok = true;
			BinRpcProcessor processor;
			processor.SetConnection(m_connection.get());
			processor.Execute();
		}
		else if (sigKind == 2)
		{
			ok = true;
			while (ServWebRequest((char*)&signature))
			{
				if (!m_connection->Recv((char*)&signature, 4))
				{
					debug("Could not read request signature");
					break;
				}
			}

			m_connection->SetGracefull(true);
			m_connection->Disconnect();
		}

		if (!ok)
		{
			int warnPort = nzbget_rs_ntw_remote_server_select_port(
				m_tls ? 1 : 0, g_Options->GetSecurePort(), g_Options->GetControlPort());
			warn("Non-RustyGet request received on port %i from %s", warnPort, m_connection->GetRemoteAddr());
		}
	}
};

class RemoteServer : public Thread, public Observer
{
public:
	RemoteServer(bool tls) : m_tls(tls) {}

	~RemoteServer()
	{
#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}

	void Run() override
	{
		debug("Entering RemoteServer-loop");

#ifndef DISABLE_TLS
		if (m_tls)
		{
			if (nzbget_rs_dqh_remote_server_is_cert_or_key_missing(
				g_Options->GetSecureCert(),
				FileSystem::FileExists(g_Options->GetSecureCert()) ? 1 : 0) != 0)
			{
				error("Could not initialize TLS, secure certificate is not configured or the cert-file was not found. Check option <SecureCert>");
				return;
			}

			if (nzbget_rs_dqh_remote_server_is_cert_or_key_missing(
				g_Options->GetSecureKey(),
				FileSystem::FileExists(g_Options->GetSecureKey()) ? 1 : 0) != 0)
			{
				error("Could not initialize TLS, secure key is not configured or the key-file was not found. Check option <SecureKey>");
				return;
			}
		}
#endif

		while (!IsStopped())
		{
			bool bind = true;

			if (!m_connection)
			{
				int listenPort = nzbget_rs_ntw_remote_server_select_port(
					m_tls ? 1 : 0, g_Options->GetSecurePort(), g_Options->GetControlPort());
				m_connection = std::make_unique<Connection>(g_Options->GetControlIp(),
					listenPort,
					m_tls);
				m_connection->SetTimeout(g_Options->GetRemoteTimeout());
				m_connection->SetSuppressErrors(false);
				bind = m_connection->Bind();
				if (IsStopped())
				{
					break;
				}
			}

			std::unique_ptr<Connection> acceptedConnection;
			if (bind)
			{
				acceptedConnection = m_connection->Accept();
			}
			if (bind && !acceptedConnection && m_connection && m_connection->GetAcceptBusy() && !IsStopped())
			{
				Util::Sleep(200);
				continue;
			}
			if (nzbget_rs_dqh_remote_server_should_reset_listener(
				bind ? 1 : 0, acceptedConnection != nullptr ? 1 : 0) != 0)
			{
				if (IsStopped())
				{
					break;
				}
				m_connection.reset();
				Util::Sleep(500);
				continue;
			}

			if (!IsStopped())
			{
				Guard guard(m_processorsMutex);
				time_t now = Util::CurrentTime();
				int shouldWarn = 0;
				if (nzbget_rs_ntw_remote_server_should_warn_cap(
					(int)m_activeProcessors.size(), MaxProcessors,
					(int64_t)now, (int64_t)m_lastCapWarning, &shouldWarn) != 0)
				{
					if (shouldWarn)
					{
						m_lastCapWarning = now;
						warn("Too many connections to the web interface/API (%i); refusing new ones", MaxProcessors);
					}
					continue;
				}
				RequestProcessor* commandThread = new RequestProcessor();
				commandThread->SetAutoDestroy(true);
				commandThread->SetConnection(std::move(acceptedConnection));
#ifndef DISABLE_TLS
				commandThread->SetTls(m_tls);
#endif
				m_activeProcessors.push_back(commandThread);
				commandThread->Attach(this);
				try
				{
					commandThread->Start();
				}
				catch (const std::system_error& e)
				{
					m_activeProcessors.pop_back();
					commandThread->Detach(this);
					delete commandThread;
					warn("Could not start a thread for a web interface/API connection: %s", e.what());
					Util::Sleep(200);
				}
			}
		}

		if (m_connection)
		{
			m_connection->Disconnect();
		}

		debug("RemoteServer: waiting for request processor to complete");
		bool completed = false;
		while (!completed)
		{
			{
				Guard guard(m_processorsMutex);
				completed = m_activeProcessors.size() == 0;
			}
			Util::Sleep(100);
		}
		debug("RemoteServer: request processor are completed");

		debug("Exiting RemoteServer-loop");
	}

	void Stop() override
	{
		Thread::Stop();
		if (m_connection)
		{
			m_connection->SetSuppressErrors(true);
			m_connection->SetForceClose(true);
			m_connection->Cancel();
		}

		debug("Stopping RequestProcessors");
		{
			Guard guard(m_processorsMutex);
			for (RequestProcessor* requestProcessor : m_activeProcessors)
			{
				requestProcessor->Stop();
			}
		}
		debug("RequestProcessors are notified");
		debug("RemoteServer stop end");
	}

	void ForceStop()
	{
		debug("Killing RequestProcessors");
		Guard guard(m_processorsMutex);
		for (RequestProcessor* requestProcessor : m_activeProcessors)
		{
			requestProcessor->Kill();
		}
		m_activeProcessors.clear();
		debug("RequestProcessors are killed");
	}

	void Update(Subject* caller, void* aspect) override
	{
		(void)aspect;
		debug("Notification from RequestProcessor received");

		RequestProcessor* requestProcessor = (RequestProcessor*)caller;
		Guard guard(m_processorsMutex);
		auto it = std::find(m_activeProcessors.begin(), m_activeProcessors.end(), requestProcessor);
		if (it != m_activeProcessors.end())
		{
			m_activeProcessors.erase(it);
		}
	}

private:
	typedef std::deque<RequestProcessor*> RequestProcessors;

	bool m_tls;
	std::unique_ptr<Connection> m_connection;
	RequestProcessors m_activeProcessors;
	Mutex m_processorsMutex;
	time_t m_lastCapWarning = 0;
	static const int MaxProcessors = 256;
};

#endif
