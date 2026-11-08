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
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@sourceforge.net>
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


#ifndef BINRPC_H
#define BINRPC_H

#include "Connection.h"
#include "Frontend.h"

class BinRpcProcessor
{
public:
	BinRpcProcessor();
	void Execute();
	void SetConnection(Connection* connection) { m_connection = connection; }

private:
	SNzbRequestBase m_messageBase;
	Connection* m_connection;

	void Dispatch();
};

#include "Log.h"
#include "Options.h"
#include "QueueEditor.h"
#include "Util.h"
#include "FileSystem.h"
#include "DownloadInfo.h"
#include "Scanner.h"
#include "StatMeter.h"
#include "UrlCoordinator.h"
#include "nzbget_rs.h"

extern void ExitProc();
extern void Reload();

inline const uint32 g_MessageRequestSizes[] =
	{ 0,
		sizeof(SNzbDownloadRequest),
		sizeof(SNzbPauseUnpauseRequest),
		sizeof(SNzbListRequest),
		sizeof(SNzbSetDownloadRateRequest),
		sizeof(SNzbDumpDebugRequest),
		sizeof(SNzbEditQueueRequest),
		sizeof(SNzbLogRequest),
		sizeof(SNzbShutdownRequest),
		sizeof(SNzbReloadRequest),
		sizeof(SNzbVersionRequest),
		sizeof(SNzbPostQueueRequest),
		sizeof(SNzbWriteLogRequest),
		sizeof(SNzbScanRequest),
		sizeof(SNzbHistoryRequest)
	};

static_assert(sizeof(g_MessageRequestSizes) / sizeof(g_MessageRequestSizes[0]) == rrHistory + 1);


class BinCommand
{
public:
	virtual ~BinCommand() {}
	virtual void Execute() = 0;
	void SetConnection(Connection* connection) { m_connection = connection; }
	void SetMessageBase(SNzbRequestBase* messageBase) { m_messageBase = messageBase; }

protected:
	Connection* m_connection;
	SNzbRequestBase* m_messageBase;

	bool ReceiveRequest(void* buffer, int size);
	bool ReceiveTrailing(CharBuffer& buffer, int64 len, int64 maxLen);
	void SendBoolResponse(bool success, const char* text);
};

// a fixed-size string field of a request ends within its field
template <size_t N> inline void TerminateField(char (&field)[N]) { field[N - 1] = '\0'; }

class DownloadBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ListBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class LogBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class PauseUnpauseBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class EditQueueBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class SetDownloadRateBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class DumpDebugBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ShutdownBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ReloadBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class VersionBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class PostQueueBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class WriteLogBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ScanBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class HistoryBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

//*****************************************************************
// BinProcessor

inline BinRpcProcessor::BinRpcProcessor()
{
	m_messageBase.m_signature = (int)NZBMESSAGE_SIGNATURE;
}

inline void BinRpcProcessor::Execute()
{
	// Read the first package which needs to be a request
	if (!m_connection->Recv(((char*)&m_messageBase) + sizeof(m_messageBase.m_signature), sizeof(m_messageBase) - sizeof(m_messageBase.m_signature)))
	{
		warn("Non-RustyGet request received on port %i from %s", g_Options->GetControlPort(), m_connection->GetRemoteAddr());
		return;
	}

	// rust/src/binrpc.rs: the credentials and the request type
	uint32 type = 0;
	if (!nzbget_rs_binrpc_check(reinterpret_cast<const unsigned char*>(&m_messageBase), sizeof(m_messageBase),
		g_Options->GetControlUsername(), g_Options->GetControlPassword(), &type))
	{
		warn("RustyGet request received on port %i from %s, but username or password invalid", g_Options->GetControlPort(), m_connection->GetRemoteAddr());
		return;
	}

	debug("%s request received from %s", nzbget_rs_binrpc_name(type), m_connection->GetRemoteAddr());

	Dispatch();
}


inline void BinRpcProcessor::Dispatch()
{
	// rust/src/binrpc.rs
	uint32 expectedSize = 0, receivedSize = 0;
	if (!nzbget_rs_binrpc_size_ok(reinterpret_cast<const unsigned char*>(&m_messageBase), sizeof(m_messageBase),
		g_MessageRequestSizes, sizeof(g_MessageRequestSizes) / sizeof(g_MessageRequestSizes[0]), &expectedSize, &receivedSize))
	{
		error("Invalid size of request: expected %i Bytes, but received %i Bytes", expectedSize, receivedSize);
		return;
	}

	std::unique_ptr<BinCommand> command;

	switch (ntohl(m_messageBase.m_type))
	{
		case rrDownload:
			command = std::make_unique<DownloadBinCommand>();
			break;

		case rrList:
			command = std::make_unique<ListBinCommand>();
			break;

		case rrLog:
			command = std::make_unique<LogBinCommand>();
			break;

		case rrPauseUnpause:
			command = std::make_unique<PauseUnpauseBinCommand>();
			break;

		case rrEditQueue:
			command = std::make_unique<EditQueueBinCommand>();
			break;

		case rrSetDownloadRate:
			command = std::make_unique<SetDownloadRateBinCommand>();
			break;

		case rrDumpDebug:
			command = std::make_unique<DumpDebugBinCommand>();
			break;

		case rrShutdown:
			command = std::make_unique<ShutdownBinCommand>();
			break;

		case rrReload:
			command = std::make_unique<ReloadBinCommand>();
			break;

		case rrVersion:
			command = std::make_unique<VersionBinCommand>();
			break;

		case rrPostQueue:
			command = std::make_unique<PostQueueBinCommand>();
			break;

		case rrWriteLog:
			command = std::make_unique<WriteLogBinCommand>();
			break;

		case rrScan:
			command = std::make_unique<ScanBinCommand>();
			break;

		case rrHistory:
			command = std::make_unique<HistoryBinCommand>();
			break;

		default:
			error("Received unsupported request %i", ntohl(m_messageBase.m_type));
			break;
	}

	if (command)
	{
		command->SetConnection(m_connection);
		command->SetMessageBase(&m_messageBase);
		command->Execute();
	}
}

// The entries of one kind of a list response, collected under the locks as
// plain structs (the first Inline of them on the stack, the strings pointing
// into the queue or log): rust/src/binrpc.rs encodes them all in one call
template <class T, size_t Inline>
class RsBatch
{
public:
	RsBatch() : m_data(m_inline), m_capacity(Inline) {}
	RsBatch(const RsBatch&) = delete;

	// room for a count of entries known beforehand
	void Reserve(size_t count)
	{
		if (count > m_capacity)
		{
			Grow(count);
		}
	}

	void Add(const T& entry)
	{
		if (m_size == m_capacity)
		{
			Grow(m_capacity * 2);
		}
		m_data[m_size++] = entry;
	}

	const T* Data() const { return m_data; }
	size_t Size() const { return m_size; }

private:
	T m_inline[Inline]; // not initialized: the entries are plain structs
	std::unique_ptr<T[]> m_heap;
	T* m_data;
	size_t m_capacity;
	size_t m_size = 0;

	void Grow(size_t capacity)
	{
		std::unique_ptr<T[]> heap(new T[capacity]);
		memcpy(static_cast<void*>(heap.get()), m_data, m_size * sizeof(T));
		m_heap = std::move(heap);
		m_data = m_heap.get();
		m_capacity = capacity;
	}
};

// Match Rust's repr(C) layout, including pointer alignment on 32-bit targets.
#define RS_LAYOUT(type, first, ints, strings) \
    static_assert(offsetof(type, first) == ((4 * (ints) + alignof(NzbgetRsBinText) - 1) & \
        ~(alignof(NzbgetRsBinText) - 1)) && \
        sizeof(type) == offsetof(type, first) + (strings) * sizeof(NzbgetRsBinText), "BinRpc entry layout")
RS_LAYOUT(NzbgetRsBinNzb, filename, 12, 5);
RS_LAYOUT(NzbgetRsBinParam, name, 1, 2);
RS_LAYOUT(NzbgetRsBinFile, subject, 10, 2);
RS_LAYOUT(NzbgetRsBinLog, text, 3, 1);
RS_LAYOUT(NzbgetRsBinPost, filename, 6, 4);
RS_LAYOUT(NzbgetRsBinHistory, name, 9, 1);
#undef RS_LAYOUT

static inline NzbgetRsBinText RsText(const char* text)
{
	return {text, text ? strlen(text) : 0};
}

// The trailing data of a list response, encoded by rust/src/binrpc.rs
class RsBytes
{
public:
	RsBytes() : m_bytes{} {}
	char* Scratch() { return m_scratch; }
	size_t Capacity() const { return sizeof(m_scratch); }
	RsBytes(const RsBytes&) = delete;
	~RsBytes() { Free(); }

	RsBytes& operator=(NzbgetRsBinResponse bytes)
	{
		Free();
		m_bytes = bytes;
		return *this;
	}

	int Size() { return (int)m_bytes.len; }

	// the bytes, sent after the response
	void Send(Connection* connection)
	{
		if (m_bytes.len > 0)
		{
			connection->Send(m_bytes.data, (int)m_bytes.len);
		}
	}

private:
	NzbgetRsBinResponse m_bytes;
    // A complete small response and its final NUL, alive until Send returns.
    char m_scratch[16384];
    void Free()
    {
        if (m_bytes.ownedCapacity != 0)
        {
            nzbget_rs_free({m_bytes.data, m_bytes.len, m_bytes.ownedCapacity});
        }
    }
};

//*****************************************************************
// Commands

inline void BinCommand::SendBoolResponse(bool success, const char* text)
{
	// rust/src/binrpc.rs: the response, then the text and its NUL
	NzbgetRsBuf response = nzbget_rs_binrpc_bool_response(success, text);
	m_connection->Send(response.data, sizeof(SNzbDownloadResponse));
	m_connection->Send(response.data + sizeof(SNzbDownloadResponse), (int)(response.len - sizeof(SNzbDownloadResponse)));
	nzbget_rs_free(response);
}


inline bool BinCommand::ReceiveRequest(void* buffer, int size)
{
	memcpy(buffer, m_messageBase, sizeof(SNzbRequestBase));
	size -= sizeof(SNzbRequestBase);
	if (size > 0)
	{
		if (!m_connection->Recv(((char*)buffer) + sizeof(SNzbRequestBase), size))
		{
			error("invalid request");
			return false;
		}
	}
	return true;
}

// The data following a request: its length is the client's and is checked (no
// buffer was allocated for 0 or a negative one, and the data was used all the
// same); the buffer gets a terminator after it, as its strings are read with
// the C string functions
inline bool BinCommand::ReceiveTrailing(CharBuffer& buffer, int64 len, int64 maxLen)
{
	if (!nzbget_rs_binrpc_trailing_ok(len, maxLen))
	{
		error("invalid request: data length %" PRIi64, len);
		return false;
	}
	buffer.Reserve((int)len + 1);
	if (!buffer)
	{
		error("invalid request: no memory for %" PRIi64 " bytes", len);
		return false;
	}
	if (len > 0 && !m_connection->Recv(buffer, (int)len))
	{
		error("invalid request");
		return false;
	}
	buffer[(int)len] = '\0';
	return true;
}

inline void PauseUnpauseBinCommand::Execute()
{
	SNzbPauseUnpauseRequest PauseUnpauseRequest;
	if (!ReceiveRequest(&PauseUnpauseRequest, sizeof(PauseUnpauseRequest)))
	{
		return;
	}

	g_WorkState->SetResumeTime(0);

	switch (ntohl(PauseUnpauseRequest.m_action))
	{
		case rpDownload:
			g_WorkState->SetPauseDownload(ntohl(PauseUnpauseRequest.m_pause));
			break;

		case rpPostProcess:
			g_WorkState->SetPausePostProcess(ntohl(PauseUnpauseRequest.m_pause));
			break;

		case rpScan:
			g_WorkState->SetPauseScan(ntohl(PauseUnpauseRequest.m_pause));
			break;
	}

	SendBoolResponse(true, "Pause-/Unpause-Command completed successfully");
}

inline void SetDownloadRateBinCommand::Execute()
{
	SNzbSetDownloadRateRequest SetDownloadRequest;
	if (!ReceiveRequest(&SetDownloadRequest, sizeof(SetDownloadRequest)))
	{
		return;
	}

	g_WorkState->SetSpeedLimit(ntohl(SetDownloadRequest.m_downloadRate));
	SendBoolResponse(true, "Rate-Command completed successfully");
}

inline void DumpDebugBinCommand::Execute()
{
	SNzbDumpDebugRequest DumpDebugRequest;
	if (!ReceiveRequest(&DumpDebugRequest, sizeof(DumpDebugRequest)))
	{
		return;
	}

	g_Log->LogDebugInfo();
	SendBoolResponse(true, "Debug-Command completed successfully");
}

inline void ShutdownBinCommand::Execute()
{
	SNzbShutdownRequest ShutdownRequest;
	if (!ReceiveRequest(&ShutdownRequest, sizeof(ShutdownRequest)))
	{
		return;
	}

	SendBoolResponse(true, "Stopping server");
	ExitProc();
}

inline void ReloadBinCommand::Execute()
{
	SNzbReloadRequest ReloadRequest;
	if (!ReceiveRequest(&ReloadRequest, sizeof(ReloadRequest)))
	{
		return;
	}

	SendBoolResponse(true, "Reloading server");
	Reload();
}

inline void VersionBinCommand::Execute()
{
	SNzbVersionRequest VersionRequest;
	if (!ReceiveRequest(&VersionRequest, sizeof(VersionRequest)))
	{
		return;
	}

	SendBoolResponse(true, Util::VersionRevision());
}

inline void DownloadBinCommand::Execute()
{
	SNzbDownloadRequest DownloadRequest;
	if (!ReceiveRequest(&DownloadRequest, sizeof(DownloadRequest)))
	{
		return;
	}

	TerminateField(DownloadRequest.m_nzbFilename);
	TerminateField(DownloadRequest.m_category);
	TerminateField(DownloadRequest.m_dupeKey);

	int bufLen = (int)ntohl(DownloadRequest.m_trailingDataLength);
	CharBuffer nzbContent;
	if (bufLen <= 0 || !ReceiveTrailing(nzbContent, bufLen, 768 * 1024 * 1024))
	{
		error("invalid request");
		return;
	}

	int priority = ntohl(DownloadRequest.m_priority);
	bool addPaused = ntohl(DownloadRequest.m_addPaused);
	bool addTop = ntohl(DownloadRequest.m_addFirst);
	bool autoCategory = ntohl(DownloadRequest.m_autoCategory);
	int dupeMode = ntohl(DownloadRequest.m_dupeMode);
	int dupeScore = ntohl(DownloadRequest.m_dupeScore);

	bool ok = false;

	if (nzbget_rs_binrpc_is_url(nzbContent))
	{
		// add url
		std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
		nzbInfo->SetKind(NzbInfo::nkUrl);
		nzbInfo->SetUrl(nzbContent);
		nzbInfo->SetFilename(DownloadRequest.m_nzbFilename);
		nzbInfo->SetCategory(DownloadRequest.m_category);
		nzbInfo->SetAutoCategory(autoCategory);
		nzbInfo->SetPriority(priority);
		nzbInfo->SetAddUrlPaused(addPaused);
		nzbInfo->SetDupeKey(DownloadRequest.m_dupeKey);
		nzbInfo->SetDupeScore(dupeScore);
		nzbInfo->SetDupeMode((EDupeMode)dupeMode);

		g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), addTop);

		ok = true;
	}
	else
	{
		ok = g_Scanner->AddExternalFile(
			DownloadRequest.m_nzbFilename,
			DownloadRequest.m_category,
			autoCategory,
			priority,
			DownloadRequest.m_dupeKey,
			dupeScore,
			static_cast<EDupeMode>(dupeMode),
			nullptr,
			addTop,
			addPaused,
			nullptr,
			nullptr,
			nzbContent,
			bufLen,
			nullptr
		) != Scanner::asFailed;
	}

	{
		NzbgetRsBuf msgBuf = nzbget_rs_tpr_binrpc_format_download_response_msg(
			ok ? 1 : 0, FileSystem::BaseFileName(DownloadRequest.m_nzbFilename));
		SendBoolResponse(ok, msgBuf.data ? msgBuf.data : "");
		nzbget_rs_free(msgBuf);
	}
}

inline void ListBinCommand::Execute()
{
	SNzbListRequest ListRequest;
	if (!ReceiveRequest(&ListRequest, sizeof(ListRequest)))
	{
		return;
	}

	SNzbListResponse ListResponse;
	memset(&ListResponse, 0, sizeof(ListResponse));
	ListResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	ListResponse.m_messageBase.m_structSize = htonl(sizeof(ListResponse));
	ListResponse.m_entrySize = htonl(sizeof(SNzbListResponseFileEntry));
	ListResponse.m_regExValid = 0;

	RsBytes entries;

	if (ntohl(ListRequest.m_fileList))
	{
		ERemoteMatchMode matchMode = (ERemoteMatchMode)ntohl(ListRequest.m_matchMode);
		bool matchGroup = ntohl(ListRequest.m_matchGroup);
		TerminateField(ListRequest.m_pattern);
		const char* pattern = ListRequest.m_pattern;

		std::unique_ptr<RegEx> regEx;
		if (matchMode == rmRegEx)
		{
			regEx = std::make_unique<RegEx>(pattern);
			ListResponse.m_regExValid = regEx->IsValid();
		}

		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int nrNzbEntries = downloadQueue->GetQueue()->size();
		int nrPPPEntries = 0;
		int nrFileEntries = 0;

		// the entries, collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinNzb, 16> nzbEntries;
		RsBatch<NzbgetRsBinParam, 32> pppEntries;
		RsBatch<NzbgetRsBinFile, 32> fileEntries;
		size_t pppCount = 0, fileCount = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			pppCount += nzbInfo->GetParameters()->size();
			fileCount += nzbInfo->GetFileList()->size();
		}
		nzbEntries.Reserve(nrNzbEntries);
		pppEntries.Reserve(pppCount);
		fileEntries.Reserve(fileCount);

		// nzb entries
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			uint32 sizeHi, sizeLo, remainingSizeHi, remainingSizeLo, pausedSizeHi, pausedSizeLo;
			Util::SplitInt64(nzbInfo->GetSize(), &sizeHi, &sizeLo);
			Util::SplitInt64(nzbInfo->GetRemainingSize(), &remainingSizeHi, &remainingSizeLo);
			Util::SplitInt64(nzbInfo->GetPausedSize(), &pausedSizeHi, &pausedSizeLo);
			nzbEntries.Add({(uint32)nzbInfo->GetId(), (uint32)nzbInfo->GetKind(), sizeLo, sizeHi, remainingSizeLo,
				remainingSizeHi, pausedSizeLo, pausedSizeHi, (uint32)nzbInfo->GetPausedFileCount(),
				(uint32)nzbInfo->GetRemainingParCount(), (uint32)nzbInfo->GetPriority(),
				nzbget_rs_tpr_binrpc_eval_nzb_group_match(
					matchGroup ? 1 : 0, regEx ? 1 : 0,
					(matchGroup && regEx && regEx->Match(nzbInfo->GetName())) ? 1 : 0),
				RsText(nzbInfo->GetFilename()), RsText(nzbInfo->GetName()), RsText(nzbInfo->GetDestDir()),
				RsText(nzbInfo->GetCategory()), RsText(nzbInfo->GetQueuedFilename())});
		}

		// ppp entries
		int nzbIndex = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbIndex++;
			for (NzbParameter& nzbParameter : nzbInfo->GetParameters())
			{
				pppEntries.Add({(uint32)nzbIndex, RsText(nzbParameter.GetName()), RsText(nzbParameter.GetValue())});
				nrPPPEntries++;
			}
		}

		// file entries: the usual owner is already known from this walk.
		int ownerIndex = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			ownerIndex++;
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				nrFileEntries++;
				int nzbIndex = ownerIndex;
				if (nzbget_rs_dqh_binrpc_should_lookup_unusual_file_owner(
					fileInfo->GetNzbInfo() == nzbInfo ? 1 : 0) != 0)
				{
					// Retain the first-match behavior for an unusual parent.
					nzbIndex = 0;
					for (uint32 i = 0; i < downloadQueue->GetQueue()->size(); i++)
					{
						nzbIndex++;
						if (downloadQueue->GetQueue()->at(i).get() == fileInfo->GetNzbInfo())
						{
							break;
						}
					}
				}

				bool match = false;
				if (nzbget_rs_dqh_binrpc_should_match_file_regex(
					regEx ? 1 : 0, matchGroup ? 1 : 0) != 0)
				{
					NzbgetRsBuf matchNameBuf = nzbget_rs_tpr_binrpc_format_file_match_name(
						fileInfo->GetNzbInfo()->GetName(), FileSystem::BaseFileName(fileInfo->GetFilename()));
					match = regEx->Match(matchNameBuf.data ? matchNameBuf.data : "");
					nzbget_rs_free(matchNameBuf);
				}

				uint32 fileSizeHi, fileSizeLo, remainingSizeHi, remainingSizeLo;
				Util::SplitInt64(fileInfo->GetSize(), &fileSizeHi, &fileSizeLo);
				Util::SplitInt64(fileInfo->GetRemainingSize(), &remainingSizeHi, &remainingSizeLo);
				fileEntries.Add({(uint32)fileInfo->GetId(), (uint32)nzbIndex, fileSizeLo, fileSizeHi, remainingSizeLo,
					remainingSizeHi, (uint32)fileInfo->GetPaused(), (uint32)fileInfo->GetFilenameConfirmed(),
					(uint32)fileInfo->GetActiveDownloads(), (uint32)match,
					RsText(fileInfo->GetSubject()), RsText(fileInfo->GetFilename())});
			}
		}

		entries = nzbget_rs_binrpc_encode_list_into(nzbEntries.Data(), nzbEntries.Size(), pppEntries.Data(), pppEntries.Size(),
			fileEntries.Data(), fileEntries.Size(), entries.Scratch(), entries.Capacity());

		ListResponse.m_nrTrailingNzbEntries = htonl(nrNzbEntries);
		ListResponse.m_nrTrailingPPPEntries = htonl(nrPPPEntries);
		ListResponse.m_nrTrailingFileEntries = htonl(nrFileEntries);
		ListResponse.m_trailingDataLength = htonl(entries.Size());
	}

	if (htonl(ListRequest.m_serverState))
	{
		int postJobCount = 0;
		int64 remainingSize;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				postJobCount += nzbInfo->GetPostInfo() ? 1 : 0;
			}
			downloadQueue->CalcRemainingSize(&remainingSize, nullptr);
		}

		uint32 sizeHi, sizeLo;
		uint32 downloadRate = Util::SafeIntCast<int64, uint32>(g_StatMeter->CalcCurrentDownloadSpeed());
		ListResponse.m_downloadRate = htonl(downloadRate);
		Util::SplitInt64(remainingSize, &sizeHi, &sizeLo);
		ListResponse.m_remainingSizeHi = htonl(sizeHi);
		ListResponse.m_remainingSizeLo = htonl(sizeLo);
		ListResponse.m_downloadLimit = htonl(g_WorkState->GetSpeedLimit());
		ListResponse.m_downloadPaused = htonl(g_WorkState->GetPauseDownload());
		ListResponse.m_postPaused = htonl(g_WorkState->GetPausePostProcess());
		ListResponse.m_scanPaused = htonl(g_WorkState->GetPauseScan());
		ListResponse.m_threadCount = htonl(Thread::GetThreadCount() - 1); // not counting itself
		ListResponse.m_postJobCount = htonl(postJobCount);

		int upTimeSec, dnTimeSec;
		int64 allBytes;
		bool standBy;
		g_StatMeter->CalcTotalStat(&upTimeSec, &dnTimeSec, &allBytes, &standBy);
		ListResponse.m_upTimeSec = htonl(upTimeSec);
		ListResponse.m_downloadTimeSec = htonl(dnTimeSec);
		ListResponse.m_downloadStandBy = htonl(standBy);
		Util::SplitInt64(allBytes, &sizeHi, &sizeLo);
		ListResponse.m_downloadedBytesHi = htonl(sizeHi);
		ListResponse.m_downloadedBytesLo = htonl(sizeLo);
	}

	// Send the request answer
	m_connection->Send((char*) &ListResponse, sizeof(ListResponse));

	// Send the data
	entries.Send(m_connection);
}


inline void LogBinCommand::Execute()
{
	SNzbLogRequest LogRequest;
	if (!ReceiveRequest(&LogRequest, sizeof(LogRequest)))
	{
		return;
	}

	RsBytes entries;
	int nrEntries;
	int bufsize;

	{
		GuardedMessageList messages = g_Log->GuardMessages();

		int start = 0;
		nzbget_rs_binrpc_log_slice_bounds(
			ntohl(LogRequest.m_lines),
			ntohl(LogRequest.m_idFrom),
			messages->size(),
			messages->empty() ? 0 : (uint32)messages->front().GetId(),
			&start, &nrEntries);

		// the messages from start on, collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinLog, 32> log;
		if ((size_t)start < messages->size())
		{
			log.Reserve(messages->size() - (size_t)start);
		}
		for (uint32 i = (uint32)start; i < messages->size(); i++)
		{
			Message& message = messages->at(i);
			log.Add({(uint32)message.GetId(), (uint32)message.GetKind(), (uint32)(int)message.GetTime(),
				RsText(message.GetText())});
		}
		entries = nzbget_rs_binrpc_encode_log_into(log.Data(), log.Size(), entries.Scratch(), entries.Capacity());
		bufsize = entries.Size();
	}

	SNzbLogResponse LogResponse;
	LogResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	LogResponse.m_messageBase.m_structSize = htonl(sizeof(LogResponse));
	LogResponse.m_entrySize = htonl(sizeof(SNzbLogResponseEntry));
	LogResponse.m_nrTrailingEntries = htonl(nrEntries);
	LogResponse.m_trailingDataLength = htonl(bufsize);

	// Send the request answer
	m_connection->Send((char*) &LogResponse, sizeof(LogResponse));

	// Send the data
	if (bufsize > 0)
	{
		entries.Send(m_connection);
	}
}


inline void EditQueueBinCommand::Execute()
{
	SNzbEditQueueRequest EditQueueRequest;
	if (!ReceiveRequest(&EditQueueRequest, sizeof(EditQueueRequest)))
	{
		return;
	}

	int nrIdEntries = ntohl(EditQueueRequest.m_nrTrailingIdEntries);
	int nrNameEntries = ntohl(EditQueueRequest.m_nrTrailingNameEntries);
	int nameEntriesLen = ntohl(EditQueueRequest.m_trailingNameEntriesLen);
	int action = ntohl(EditQueueRequest.m_action);
	int matchMode = ntohl(EditQueueRequest.m_matchMode);
	int offset = ntohl(EditQueueRequest.m_offset);
	int textLen = ntohl(EditQueueRequest.m_textLen);
	uint32 bufLength = ntohl(EditQueueRequest.m_trailingDataLength);

	// the parts' sizes from the client, checked against each other in 64 bits
	// (the unsigned sum wrapped for negative counts) and against the data
	if (!nzbget_rs_binrpc_edit_sizes(nrIdEntries, nrNameEntries, nameEntriesLen, textLen, bufLength))
	{
		error("Invalid struct size");
		return;
	}

	CharBuffer buf;
	if (!ReceiveTrailing(buf, bufLength, 64 * 1024 * 1024))
	{
		return;
	}
	if (textLen > 0)
	{
		buf[textLen - 1] = '\0';
	}

	if (nrIdEntries <= 0 && nrNameEntries <= 0)
	{
		SendBoolResponse(false, "Edit-Command failed: no IDs/Names specified");
		return;
	}

	// no text is an empty one: the edits read it as a string (a priority, a name)
	const char* text = textLen > 0 ? *buf : "";

	IdList cIdList;
	NameList cNameList;

	static_assert(sizeof(IdList::value_type) == sizeof(uint32_t), "ids are 32 bits");
	cIdList.resize(nrIdEntries);
	if (!nzbget_rs_binrpc_edit_split(buf, bufLength, textLen, nrIdEntries, nrNameEntries, (uint32_t*)cIdList.data()))
	{
		error("Invalid struct size");
		return;
	}
	// each name ends within the data: checked above
	cNameList.reserve(nrNameEntries);
	const char* names = buf + textLen + nrIdEntries * sizeof(int32);
	for (int i = 0; i < nrNameEntries; i++)
	{
		cNameList.push_back(names);
		names += strlen(names) + 1;
	}

	bool useOffsetStr = (action == DownloadQueue::eaFileMoveOffset || action == DownloadQueue::eaGroupMoveOffset);
	NzbgetRsBuf offBuf = useOffsetStr ? nzbget_rs_dqh_conn_format_port_str(offset) : NzbgetRsBuf{};
	bool ok = DownloadQueue::Guard()->EditList(
		nrIdEntries > 0 ? &cIdList : nullptr,
		nrNameEntries > 0 ? &cNameList : nullptr,
		(DownloadQueue::EMatchMode)matchMode, (DownloadQueue::EEditAction)action,
		useOffsetStr ? (offBuf.data ? offBuf.data : "0") : text);
	if (useOffsetStr)
	{
		nzbget_rs_free(offBuf);
	}

	if (ok)
	{
		SendBoolResponse(true, "Edit-Command completed successfully");
	}
	else
	{
#ifndef HAVE_REGEX_H
		if ((DownloadQueue::EMatchMode)matchMode == DownloadQueue::mmRegEx)
		{
			SendBoolResponse(false, "Edit-Command failed: the program was compiled without RegEx-support");
			return;
		}
#endif
		SendBoolResponse(false, "Edit-Command failed");
	}
}

inline void PostQueueBinCommand::Execute()
{
	SNzbPostQueueRequest PostQueueRequest;
	if (!ReceiveRequest(&PostQueueRequest, sizeof(PostQueueRequest)))
	{
		return;
	}

	SNzbPostQueueResponse PostQueueResponse;
	memset(&PostQueueResponse, 0, sizeof(PostQueueResponse));
	PostQueueResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	PostQueueResponse.m_messageBase.m_structSize = htonl(sizeof(PostQueueResponse));
	PostQueueResponse.m_entrySize = htonl(sizeof(SNzbPostQueueResponseEntry));

	RsBytes entries;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		// Make a data structure and copy all the elements of the list into it
		NzbList* nzbList = downloadQueue->GetQueue();

		int NrEntries = 0;
		time_t curTime = Util::CurrentTime();
		// collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinPost, 16> jobs;
		jobs.Reserve(nzbList->size());

		for (NzbInfo* nzbInfo : nzbList)
		{
			PostInfo* postInfo = nzbInfo->GetPostInfo();
			if (!postInfo)
			{
				continue;
			}

			NrEntries++;
			jobs.Add({(uint32)nzbInfo->GetId(), (uint32)postInfo->GetStage(), (uint32)postInfo->GetStageProgress(),
				(uint32)postInfo->GetFileProgress(),
				nzbget_rs_tpr_binrpc_calc_elapsed_sec((int64_t)curTime, (int64_t)postInfo->GetStartTime()),
				nzbget_rs_tpr_binrpc_calc_elapsed_sec((int64_t)curTime, (int64_t)postInfo->GetStageTime()),
				RsText(postInfo->GetNzbInfo()->GetFilename()), RsText(postInfo->GetNzbInfo()->GetName()),
				RsText(postInfo->GetNzbInfo()->GetDestDir()), RsText(postInfo->GetProgressLabel())});
		}
		entries = nzbget_rs_binrpc_encode_post_queue_into(jobs.Data(), jobs.Size(), entries.Scratch(), entries.Capacity());

		PostQueueResponse.m_nrTrailingEntries = htonl(NrEntries);
		PostQueueResponse.m_trailingDataLength = htonl(entries.Size());
	}

	// Send the request answer
	m_connection->Send((char*) &PostQueueResponse, sizeof(PostQueueResponse));

	// Send the data
	entries.Send(m_connection);
}


inline void WriteLogBinCommand::Execute()
{
	SNzbWriteLogRequest WriteLogRequest;
	if (!ReceiveRequest(&WriteLogRequest, sizeof(WriteLogRequest)))
	{
		return;
	}

	CharBuffer recvBuffer;
	if (!ReceiveTrailing(recvBuffer, (int)ntohl(WriteLogRequest.m_trailingDataLength), 1024 * 1024))
	{
		return;
	}

	bool OK = true;
	switch ((Message::EKind)ntohl(WriteLogRequest.m_kind))
	{
		case Message::mkDetail:
			detail("%s", *recvBuffer);
			break;
		case Message::mkInfo:
			info("%s", *recvBuffer);
			break;
		case Message::mkWarning:
			warn("%s", *recvBuffer);
			break;
		case Message::mkError:
			error("%s", *recvBuffer);
			break;
		case Message::mkDebug:
			debug("%s", *recvBuffer);
			break;
		default:
			OK = false;
	}
	SendBoolResponse(OK, nzbget_rs_dqh_binrpc_write_log_response_msg(OK ? 1 : 0));
}

inline void ScanBinCommand::Execute()
{
	SNzbScanRequest ScanRequest;
	if (!ReceiveRequest(&ScanRequest, sizeof(ScanRequest)))
	{
		return;
	}

	bool syncMode = ntohl(ScanRequest.m_syncMode);

	g_Scanner->ScanNzbDir(syncMode);
	SendBoolResponse(true, nzbget_rs_dqh_binrpc_scan_response_msg(syncMode ? 1 : 0));
}

inline void HistoryBinCommand::Execute()
{
	SNzbHistoryRequest HistoryRequest;
	if (!ReceiveRequest(&HistoryRequest, sizeof(HistoryRequest)))
	{
		return;
	}

	bool showHidden = ntohl(HistoryRequest.m_hidden);

	SNzbHistoryResponse HistoryResponse;
	memset(&HistoryResponse, 0, sizeof(HistoryResponse));
	HistoryResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	HistoryResponse.m_messageBase.m_structSize = htonl(sizeof(HistoryResponse));
	HistoryResponse.m_entrySize = htonl(sizeof(SNzbHistoryResponseEntry));

	RsBytes entries;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int nrEntries = 0;
		// collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinHistory, 32> history;
		history.Reserve(downloadQueue->GetHistory()->size());
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (nzbget_rs_tpr_binrpc_should_include_history_entry(
				(int)historyInfo->GetKind(), (int)HistoryInfo::hkDup, showHidden ? 1 : 0))
			{
				nrEntries++;
				// fields a kind doesn't set are 0
				uint32 sizeHi = 0, sizeLo = 0, fileCount = 0, parStatus = 0, scriptStatus = 0, urlStatus = 0;
				int histFieldKind = nzbget_rs_dqh_binrpc_classify_history_entry_fields(
					(int)historyInfo->GetKind(),
					(int)HistoryInfo::hkNzb,
					(int)HistoryInfo::hkDup,
					(int)HistoryInfo::hkUrl,
					showHidden ? 1 : 0);
				if (histFieldKind == 1)
				{
					NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
					Util::SplitInt64(nzbInfo->GetSize(), &sizeHi, &sizeLo);
					fileCount = nzbInfo->GetFileCount();
					parStatus = nzbInfo->GetParStatus();
					scriptStatus = nzbInfo->GetScriptStatuses()->CalcTotalStatus();
				}
				else if (histFieldKind == 2)
				{
					DupInfo* dupInfo = historyInfo->GetDupInfo();
					Util::SplitInt64(dupInfo->GetSize(), &sizeHi, &sizeLo);
				}
				else if (histFieldKind == 3)
				{
					NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
					urlStatus = nzbInfo->GetUrlStatus();
				}
				history.Add({(uint32)historyInfo->GetId(), (uint32)historyInfo->GetKind(), (uint32)(int)historyInfo->GetTime(),
					sizeLo, sizeHi, fileCount, parStatus, scriptStatus, urlStatus, RsText(historyInfo->GetName())});
			}
		}
		entries = nzbget_rs_binrpc_encode_history_into(history.Data(), history.Size(), entries.Scratch(), entries.Capacity());

		HistoryResponse.m_nrTrailingEntries = htonl(nrEntries);
		HistoryResponse.m_trailingDataLength = htonl(entries.Size());
	}

	// Send the request answer
	m_connection->Send((char*) &HistoryResponse, sizeof(HistoryResponse));

	// Send the data
	entries.Send(m_connection);
}

#endif


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
