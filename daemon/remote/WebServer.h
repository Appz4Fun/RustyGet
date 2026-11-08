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
#include "Connection.h"
#include "Util.h"

class XmlCommand;

class XmlRpcProcessor
{
public:
	enum ERpcProtocol
	{
		rpUndefined,
		rpXmlRpc,
		rpJsonRpc,
		rpJsonPRpc
	};

	enum EHttpMethod
	{
		hmPost,
		hmGet
	};

	enum EUserAccess
	{
		uaControl,
		uaRestricted,
		uaAdd
	};

	void Execute();
	void SetHttpMethod(EHttpMethod httpMethod) { m_httpMethod = httpMethod; }
	void SetUserAccess(EUserAccess userAccess) { m_userAccess = userAccess; }
	// whether JSON-P requests are answered (option JsonP); off unless set
	void SetJsonP(bool jsonP) { m_jsonP = jsonP; }
	void SetUrl(const char* url);
	void SetRequest(char* request) { m_request = request; }
	const char* GetResponse() { return m_response; }
	const char* GetContentType() { return m_contentType; }
	static bool IsRpcRequest(const char* url);
	bool IsSafeMethod() { return m_safeMethod; }

private:
	char* m_request = nullptr;
	const char* m_contentType = nullptr;
	ERpcProtocol m_protocol;
	EHttpMethod m_httpMethod;
	EUserAccess m_userAccess;
	CString m_url;
	StringBuilder m_response;
	bool m_safeMethod = false;
	bool m_jsonP = false;

	// what Dispatch does with a command: run it, or answer with an error, inside the
	// JSON-P callback or plain
	enum EDispatch
	{
		dpExecute,
		dpRefuseInCallback,
		dpRefuse
	};

	static EUserAccess Access(ERpcProtocol protocol, EUserAccess userAccess);
	static EDispatch Decide(ERpcProtocol protocol, EHttpMethod httpMethod, bool safeMethod, bool error,
		bool jsonP, int* errCode, const char** errText);
	void Dispatch();
	std::unique_ptr<XmlCommand> CreateCommand(const char* methodName);
	void MutliCall();
	void BuildResponse(const char* response, const char* callbackFunc, bool fault, const char* requestId);
	void BuildErrorResponse(int errCode, const char* errText);
	void BuildJsonPRefusal(XmlCommand* command, int errCode, const char* errText, const char* requestId);
};

class XmlCommand
{
public:
	XmlCommand();
	virtual ~XmlCommand();
	virtual void Execute() = 0;
	void PrepareParams();
	void SetRequest(char* request) { m_request = request; m_requestPtr = m_request; }
	void SetProtocol(XmlRpcProcessor::ERpcProtocol protocol) { m_protocol = protocol; }
	void SetHttpMethod(XmlRpcProcessor::EHttpMethod httpMethod) { m_httpMethod = httpMethod; }
	void SetUserAccess(XmlRpcProcessor::EUserAccess userAccess) { m_userAccess = userAccess; }
	const char* GetResponse() { return m_response; }
	const char* GetCallbackFunc() { return m_callbackFunc; }
	bool GetFault() { return m_fault; }
	virtual bool IsSafeMethod() { return false; };
	virtual bool IsError() { return false; };

protected:
	char* m_request = nullptr;
	char* m_requestPtr = nullptr;
	char* m_callbackFunc = nullptr;
	StringBuilder m_response;
	bool m_fault = false;
	XmlRpcProcessor::ERpcProtocol m_protocol = XmlRpcProcessor::rpUndefined;
	XmlRpcProcessor::EHttpMethod m_httpMethod;
	XmlRpcProcessor::EUserAccess m_userAccess;

	void BuildErrorResponse(int errCode, const char* errText, ...);
	void BuildBoolResponse(bool ok);
	void BuildIntResponse(int value);
	void AppendResponse(const char* part);
	void AppendFmtResponse(const char* format, ...);
	void AppendCondResponse(const char* part, bool cond);
	bool IsJson();
	bool NextParamAsInt(int* value);
	bool NextParamAsBool(bool* value);
	bool NextParamAsStr(char** valueBuf);
	CString EncodeStr(const char* str);
	void DecodeStr(char* str);
};

#include "nzbget_rs.h"
#include "QueueCoordinator.h"
#include "Util.h"
#include "Util.h"
#include "QueueCoordinator.h"
#include "FeedCoordinator.h"
#include "ServerPool.h"
#include "Util.h"
#include <cerrno>
#include <climits>
#include "Util.h"
#include "ServerPool.h"
#include "ServerPool.h"
#include "DiskState.h"
#include "QueueCoordinator.h"
#include "PrePostProcessor.h"
#include "FeedCoordinator.h"
namespace
{
	// Pauses downloads for the duration of a speed test and restores the previous
	// state. Tests at the same time share it: the first saves the state, the last
	// restores it (a second test restored "paused", saved while the first ran).
	class PauseGuard
	{
	public:
		PauseGuard()
		{
			std::lock_guard<std::mutex> lock(Mutex());
			if (Active()++ == 0)
			{
				WasPaused() = g_WorkState->GetPauseDownload();
				g_WorkState->SetPauseDownload(true);
			}
		}
		PauseGuard(const PauseGuard&) = delete;
		PauseGuard& operator=(const PauseGuard&) = delete;
		~PauseGuard()
		{
			std::lock_guard<std::mutex> lock(Mutex());
			if (--Active() == 0)
			{
				g_WorkState->SetPauseDownload(WasPaused());
			}
		}

	private:
		static std::mutex& Mutex() { static std::mutex mutex; return mutex; }
		static int& Active() { static int active = 0; return active; }
		static bool& WasPaused() { static bool wasPaused = false; return wasPaused; }
	};

	double MiBPerSec(uint64_t bytes, double ms)
	{
		return nzbget_rs_ntw_xmlrpc_mib_per_sec(bytes, ms);
	}

	void AppendRsBuf(StringBuilder& response, NzbgetRsBuf buf)
	{
		if (buf.data && buf.len > 0)
		{
			response.Append(buf.data, static_cast<int>(buf.len));
		}
		nzbget_rs_free(buf);
	}
}

extern void ExitProc();
extern void Reload();

class SafeXmlCommand : public XmlCommand
{
public:
	~SafeXmlCommand() override;
	bool IsSafeMethod() override { return true; }
};

class ErrorXmlCommand final : public XmlCommand
{
public:
	ErrorXmlCommand(int errCode, const char* errText) :
		m_errCode(errCode), m_errText(errText) {}
	void Execute() override;
	bool IsError() override { return true; }

private:
	int m_errCode;
	const char* m_errText;
};

class PauseUnpauseXmlCommand final : public XmlCommand
{
public:
	enum EPauseAction
	{
		paDownload,
		paPostProcess,
		paScan
	};

	PauseUnpauseXmlCommand(bool pause, EPauseAction pauseAction) :
		m_pause(pause), m_pauseAction(pauseAction) {}
	void Execute() override;

private:
	bool m_pause;
	EPauseAction m_pauseAction;
};

class ScheduleResumeXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ShutdownXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ReloadXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class VersionXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class DumpDebugXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class SetDownloadRateXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class StatusXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class SysInfoXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class SystemHealthXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class LogXmlCommand: public SafeXmlCommand
{
public:
	~LogXmlCommand() override;
	void Execute() override;

protected:
	int m_idFrom;
	int m_nrEntries;
	virtual GuardedMessageList GuardMessages();
};

class NzbInfoXmlCommand: public SafeXmlCommand
{
public:
	~NzbInfoXmlCommand() override;

protected:
	void AppendNzbInfoFields(NzbInfo* nzbInfo);
	void AppendPostInfoFields(PostInfo* postInfo, int logEntries, bool postQueue);
};

class ListFilesXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ListGroupsXmlCommand final : public NzbInfoXmlCommand
{
public:
	void Execute() override;
private:
	const char* DetectStatus(NzbInfo* nzbInfo);
};

class EditQueueXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class DownloadXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class AppendFleetXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class PostQueueXmlCommand final : public NzbInfoXmlCommand
{
public:
	void Execute() override;
};

class WriteLogXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ClearLogXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ScanXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class HistoryXmlCommand final : public NzbInfoXmlCommand
{
public:
	void Execute() override;
private:
	const char* DetectStatus(HistoryInfo* historyInfo);
};

class UrlQueueXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ConfigXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class LoadConfigXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class LoadExtensionsXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class DownloadExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class UpdateExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class DeleteExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class TestExtensionXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class SaveConfigXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ConfigTemplatesXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ViewFeedXmlCommand final : public XmlCommand
{
public:
	ViewFeedXmlCommand(bool preview) : m_preview(preview) {}
	void Execute() override;
private:
	bool m_preview;
};

class FetchFeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class EditServerXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class ReadUrlXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class CheckUpdatesXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class StartUpdateXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class LogUpdateXmlCommand final : public LogXmlCommand
{
protected:
	GuardedMessageList GuardMessages() override;
};

class ServerVolumesXmlCommand final : public SafeXmlCommand
{
public:
	void Execute() override;
};

class ResetServerVolumeXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class LoadLogXmlCommand final : public LogXmlCommand
{
protected:
	void Execute() override;
	GuardedMessageList GuardMessages() override;
private:
	int m_nzbId;
	NzbInfo* m_nzbInfo;
	MessageList m_messages;
	std::unique_ptr<GuardedDownloadQueue> m_downloadQueue;
};

class TestServerXmlCommand final : public XmlCommand
{
public:
	void Execute() override;

	struct ServerParams
	{
		std::string host;
		std::string username;
		std::string password;
		std::string cipher;
		int port;
		int timeout;
		int certVerifLevel;
		bool encryption;
	};

private:
	CString m_errText;

	class TestConnection final : public NntpConnection
	{
	public:
		TestConnection(NewsServer* newsServer, TestServerXmlCommand* owner) :
			NntpConnection(newsServer), m_owner(owner) {
		}
	protected:
		TestServerXmlCommand* m_owner;
		void PrintError(const char* errMsg) override { m_owner->PrintError(errMsg); }
	};

	void PrintError(const char* errMsg);
};

class TlsCipherXmlCommand final : public XmlCommand
{
public:
	explicit TlsCipherXmlCommand(int operation) : m_operation(operation) {}
	void Execute() override
	{
		const NzbgetRsJsonLimits limits{0x7ffffffeu, 0x7fffffffu / 24u, 0x3fffffffu};
		int fault = 0;
		NzbgetRsBuf buf = nzbget_rs_xmlrpc_execute_tls_cipher(
			m_operation,
			(int)m_userAccess, (int)XmlRpcProcessor::uaControl,
			(int)m_protocol, (int)XmlRpcProcessor::rpJsonRpc,
			(int)m_httpMethod, (int)XmlRpcProcessor::hmPost,
			IsJson() ? 1 : 0,
			m_request,
			limits,
			nullptr,
			[](void*, int opKind, const char* jobId) -> NzbgetRsBuf
			{
				std::string resp = opKind == 0 ? TlsCipherBackend::Start() :
					opKind == 1 ? TlsCipherBackend::Status(jobId) :
					TlsCipherBackend::Cancel(jobId ? jobId : "");
				return nzbget_rs_alloc_buf(resp.data(), resp.size());
			},
			&fault);
		m_fault = fault != 0;
		AppendRsBuf(m_response, buf);
	}
private:
	int m_operation;
};

class TestServerSpeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class TestDiskSpeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class TestNetworkSpeedXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class StartScriptXmlCommand final : public XmlCommand
{
public:
	void Execute() override;
};

class LogScriptXmlCommand final : public LogXmlCommand
{
protected:
	virtual GuardedMessageList GuardMessages();
};


//*****************************************************************
// XmlRpcProcessor

inline void XmlRpcProcessor::SetUrl(const char* url)
{
	NzbgetRsBuf urlBuf = nzbget_rs_xmlrpc_normalize_url(url);
	m_url.Set(urlBuf.data ? urlBuf.data : "", (int)urlBuf.len);
	nzbget_rs_free(urlBuf);
}

inline bool XmlRpcProcessor::IsRpcRequest(const char* url)
{
	return nzbget_rs_rco_rpc_is_rpc_request(url) != 0;
}

inline void XmlRpcProcessor::Execute()
{
	m_protocol = (ERpcProtocol)nzbget_rs_rpc_protocol(m_url);
	if (m_protocol == rpUndefined)
	{
		error("internal error: invalid rpc-request: %s", *m_url);
		return;
	}

	Dispatch();
}

inline void XmlRpcProcessor::Dispatch()
{
	char* request = m_request;

	BString<100> methodName;
	// echoed as sent: cut to 99 characters it made the response invalid JSON (an id
	// past 4 KB is left out rather than cut)
	CString requestId;

	const char* params = nullptr;
	const char* requestIdPtr = nullptr;
	int requestIdLen = 0;
	nzbget_rs_rpc_route(m_url, m_request, m_httpMethod == hmGet, m_protocol, methodName, &params,
		&requestIdPtr, &requestIdLen);
	request = const_cast<char*>(params);
	if (requestIdPtr)
	{
		requestId.Set(requestIdPtr, requestIdLen);
	}

	debug("MethodName=%s", *methodName);

	m_userAccess = Access(m_protocol, m_userAccess);

	if (nzbget_rs_dqh_xmlrpc_is_multicall_post(
		methodName, m_protocol == rpXmlRpc ? 1 : 0, m_httpMethod == hmPost ? 1 : 0) != 0)
	{
		MutliCall();
	}
	else
	{
		std::unique_ptr<XmlCommand> command = CreateCommand(methodName);
		command->SetRequest(request);
		command->SetProtocol(m_protocol);
		command->SetHttpMethod(m_httpMethod);
		command->SetUserAccess(m_userAccess);
		command->PrepareParams();
		m_safeMethod = command->IsSafeMethod();
		int errCode = 0;
		const char* errText = nullptr;
		switch (Decide(m_protocol, m_httpMethod, m_safeMethod, command->IsError(), m_jsonP, &errCode, &errText))
		{
			case dpExecute:
				command->Execute();
				BuildResponse(command->GetResponse(), command->GetCallbackFunc(), command->GetFault(), requestId);
				break;

			case dpRefuseInCallback:
				BuildJsonPRefusal(command.get(), errCode, errText, requestId);
				break;

			case dpRefuse:
				BuildErrorResponse(errCode, errText);
				break;
		}
	}
}

inline XmlRpcProcessor::EUserAccess XmlRpcProcessor::Access(ERpcProtocol protocol, EUserAccess userAccess)
{
	// rust/src/rpcroute.rs
	return (EUserAccess)nzbget_rs_rpc_access(protocol, userAccess);
}

inline XmlRpcProcessor::EDispatch XmlRpcProcessor::Decide(ERpcProtocol protocol, EHttpMethod httpMethod,
	bool safeMethod, bool error, bool jsonP, int* errCode, const char** errText)
{
	// rust/src/rpcroute.rs
	return (EDispatch)nzbget_rs_rpc_decide(protocol, httpMethod, safeMethod, error, jsonP, errCode, errText);
}

inline void XmlRpcProcessor::MutliCall()
{
	bool error = false;
	StringBuilder response;

	response.Append("<array><data>");

	char* requestPtr = m_request;
	size_t structOff = 0;
	while (nzbget_rs_rol_xmlrpc_multicall_find_struct_end(requestPtr, &structOff))
	{
		char* callEnd = requestPtr + structOff;
		*callEnd = '\0';
		debug("MutliCall, request=%s", requestPtr);
		size_t nameOff = 0;
		size_t paramsOff = 0;
		if (!nzbget_rs_rol_xmlrpc_multicall_locate_parts(requestPtr, structOff, &nameOff, &paramsOff))
		{
			error = true;
			break;
		}
		char* nameEnd = requestPtr + nameOff;

		char methodName[100];
		methodName[0] = '\0';
		WebUtil::XmlParseTagValue(nameEnd, "string", methodName, sizeof(methodName), nullptr);
		debug("MutliCall, MethodName=%s", methodName);

		std::unique_ptr<XmlCommand> command = CreateCommand(methodName);
		// a call of the multicall runs as the caller, with the caller's access: it ran
		// with full control access (a restricted user read the passwords through it);
		// its parameters are its "params" member, not the start of the call
		command->SetProtocol(m_protocol);
		command->SetHttpMethod(m_httpMethod);
		command->SetUserAccess(m_userAccess);
		command->SetRequest(requestPtr + paramsOff);
		m_safeMethod |= command->IsSafeMethod();
		command->Execute();

		debug("MutliCall, Response=%s", command->GetResponse());

		bool wrapArray = nzbget_rs_roc_xmlrpc_multicall_needs_array_wrap(command->GetResponse()) != 0;
		if (wrapArray)
		{
			response.Append("<array><data>");
		}
		response.Append("<value>");
		response.Append(command->GetResponse());
		response.Append("</value>");
		if (wrapArray)
		{
			response.Append("</data></array>");
		}

		requestPtr = callEnd + 9; //strlen("</struct>")
	}

	if (error)
	{
		BuildErrorResponse(4, "Parse error");
	}
	else
	{
		response.Append("</data></array>");
		BuildResponse(response, "", false, nullptr);
	}
}

inline void XmlRpcProcessor::BuildResponse(const char* response, const char* callbackFunc,
	bool fault, const char* requestId)
{
	debug("Response=%s", response);

	bool xmlRpc = m_protocol == rpXmlRpc;

	NzbgetRsBuf head, tail;
	nzbget_rs_rpc_envelope(m_protocol, fault, callbackFunc, requestId, &head, &tail);
	m_response.Append(head.data);
	m_response.Append(response);
	m_response.Append(tail.data);
	nzbget_rs_free(head);
	nzbget_rs_free(tail);

	m_contentType = nzbget_rs_dqh_xmlrpc_response_content_type(xmlRpc ? 1 : 0);
}

inline void XmlRpcProcessor::BuildErrorResponse(int errCode, const char* errText)
{
	ErrorXmlCommand command(errCode, errText);
	command.SetProtocol(m_protocol);
	command.Execute();
	BuildResponse(command.GetResponse(), "", command.GetFault(), nullptr);
}

// a JSON-P refusal goes through the caller's callback, so the page that loaded it
// sees it
inline void XmlRpcProcessor::BuildJsonPRefusal(XmlCommand* command, int errCode, const char* errText,
	const char* requestId)
{
	ErrorXmlCommand refusal(errCode, errText);
	refusal.SetProtocol(m_protocol);
	refusal.Execute();
	BuildResponse(refusal.GetResponse(), command->GetCallbackFunc(), refusal.GetFault(), requestId);
}

inline std::unique_ptr<XmlCommand> XmlRpcProcessor::CreateCommand(const char* methodName)
{
	int accessDenied = nzbget_rs_ntw_xmlrpc_check_user_access_denied(
		(int)m_userAccess, (int)uaAdd, (int)uaRestricted, methodName);
	if (accessDenied == 1)
	{
		warn("Received request \"%s\" from add-user, access denied", methodName);
		return std::make_unique<ErrorXmlCommand>(401, "Access denied");
	}
	if (accessDenied == 2)
	{
		warn("Received request \"%s\" from restricted user, access denied", methodName);
		return std::make_unique<ErrorXmlCommand>(401, "Access denied");
	}

	switch (nzbget_rs_xmlrpc_match_command(methodName))
	{
		case 1: return std::make_unique<PauseUnpauseXmlCommand>(true, PauseUnpauseXmlCommand::paDownload);
		case 2: return std::make_unique<PauseUnpauseXmlCommand>(false, PauseUnpauseXmlCommand::paDownload);
		case 3: return std::make_unique<ShutdownXmlCommand>();
		case 4: return std::make_unique<ReloadXmlCommand>();
		case 5: return std::make_unique<VersionXmlCommand>();
		case 6: return std::make_unique<DumpDebugXmlCommand>();
		case 7: return std::make_unique<SetDownloadRateXmlCommand>();
		case 8: return std::make_unique<StatusXmlCommand>();
		case 9: return std::make_unique<SysInfoXmlCommand>();
		case 10: return std::make_unique<SystemHealthXmlCommand>();
		case 11: return std::make_unique<LogXmlCommand>();
		case 12: return std::make_unique<ListFilesXmlCommand>();
		case 13: return std::make_unique<ListGroupsXmlCommand>();
		case 14: return std::make_unique<EditQueueXmlCommand>();
		case 15: return std::make_unique<DownloadXmlCommand>();
		case 16: return std::make_unique<AppendFleetXmlCommand>();
		case 17: return std::make_unique<PostQueueXmlCommand>();
		case 18: return std::make_unique<WriteLogXmlCommand>();
		case 19: return std::make_unique<ClearLogXmlCommand>();
		case 20: return std::make_unique<LoadLogXmlCommand>();
		case 21: return std::make_unique<ScanXmlCommand>();
		case 22: return std::make_unique<PauseUnpauseXmlCommand>(true, PauseUnpauseXmlCommand::paPostProcess);
		case 23: return std::make_unique<PauseUnpauseXmlCommand>(false, PauseUnpauseXmlCommand::paPostProcess);
		case 24: return std::make_unique<PauseUnpauseXmlCommand>(true, PauseUnpauseXmlCommand::paScan);
		case 25: return std::make_unique<PauseUnpauseXmlCommand>(false, PauseUnpauseXmlCommand::paScan);
		case 26: return std::make_unique<ScheduleResumeXmlCommand>();
		case 27: return std::make_unique<HistoryXmlCommand>();
		case 28: return std::make_unique<UrlQueueXmlCommand>();
		case 29: return std::make_unique<ConfigXmlCommand>();
		case 30: return std::make_unique<LoadConfigXmlCommand>();
		case 31: return std::make_unique<LoadExtensionsXmlCommand>();
		case 32: return std::make_unique<DownloadExtensionXmlCommand>();
		case 33: return std::make_unique<UpdateExtensionXmlCommand>();
		case 34: return std::make_unique<DeleteExtensionXmlCommand>();
		case 35: return std::make_unique<TestExtensionXmlCommand>();
		case 36: return std::make_unique<SaveConfigXmlCommand>();
		case 37: return std::make_unique<ConfigTemplatesXmlCommand>();
		case 38: return std::make_unique<ViewFeedXmlCommand>(false);
		case 39: return std::make_unique<ViewFeedXmlCommand>(true);
		case 40: return std::make_unique<FetchFeedXmlCommand>();
		case 41: return std::make_unique<EditServerXmlCommand>();
		case 42: return std::make_unique<ReadUrlXmlCommand>();
		case 43: return std::make_unique<CheckUpdatesXmlCommand>();
		case 44: return std::make_unique<StartUpdateXmlCommand>();
		case 45: return std::make_unique<LogUpdateXmlCommand>();
		case 46: return std::make_unique<ServerVolumesXmlCommand>();
		case 47: return std::make_unique<ResetServerVolumeXmlCommand>();
		case 48: return std::make_unique<TlsCipherXmlCommand>(0);
		case 49: return std::make_unique<TlsCipherXmlCommand>(1);
		case 50: return std::make_unique<TlsCipherXmlCommand>(2);
		case 51: return std::make_unique<TestServerXmlCommand>();
		case 52: return std::make_unique<TestServerSpeedXmlCommand>();
		case 53: return std::make_unique<TestDiskSpeedXmlCommand>();
		case 54: return std::make_unique<TestNetworkSpeedXmlCommand>();
		case 55: return std::make_unique<StartScriptXmlCommand>();
		case 56: return std::make_unique<LogScriptXmlCommand>();
		default: return std::make_unique<ErrorXmlCommand>(1, "Invalid procedure");
	}
}


//*****************************************************************
// Base command

inline XmlCommand::XmlCommand()
	: m_protocol(XmlRpcProcessor::rpUndefined)
	, m_httpMethod(XmlRpcProcessor::hmPost)
	, m_userAccess(XmlRpcProcessor::uaControl)
{
	m_response.Reserve(1024 * 10 - 1);
}

inline XmlCommand::~XmlCommand() = default;
inline SafeXmlCommand::~SafeXmlCommand() = default;
inline LogXmlCommand::~LogXmlCommand() = default;
inline NzbInfoXmlCommand::~NzbInfoXmlCommand() = default;

inline bool XmlCommand::IsJson()
{
	return nzbget_rs_dqh_xmlrpc_is_json_protocol(
		(int)m_protocol,
		(int)XmlRpcProcessor::rpJsonRpc,
		(int)XmlRpcProcessor::rpJsonPRpc) != 0;
}

inline void XmlCommand::AppendResponse(const char* part)
{
	m_response.Append(part);
}

inline void XmlCommand::AppendFmtResponse(const char* format, ...)
{
	va_list args;
	va_start(args, format);
	m_response.AppendFmtV(format, args);
	va_end(args);
}

inline void XmlCommand::AppendCondResponse(const char* part, bool cond)
{
	if (cond)
	{
		m_response.Append(part);
	}
}

inline void XmlCommand::BuildErrorResponse(int errCode, const char* errText, ...)
{
	char fullText[1024];

	va_list ap;
	va_start(ap, errText);
	vsnprintf(fullText, 1024, errText, ap);
	fullText[1024-1] = '\0';
	va_end(ap);

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_error(IsJson() ? 1 : 0, errCode, fullText));

	m_fault = true;
}

inline void XmlCommand::BuildBoolResponse(bool ok)
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_bool(IsJson() ? 1 : 0, ok ? 1 : 0));
}

inline void XmlCommand::BuildIntResponse(int value)
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_int(IsJson() ? 1 : 0, value));
}

inline void XmlCommand::PrepareParams()
{
	if (nzbget_rs_dqh_xmlrpc_should_skip_to_json_post_params(
		IsJson() ? 1 : 0, (int)m_httpMethod, (int)XmlRpcProcessor::hmPost) != 0)
	{
		// without "params" the request is emptied: no parameters follow
		m_requestPtr = nzbget_rs_rpc_skip_to_params(m_requestPtr);
	}

	if (m_protocol == XmlRpcProcessor::rpJsonPRpc)
	{
		NextParamAsStr(&m_callbackFunc);
	}
}

inline bool XmlCommand::NextParamAsInt(int* value)
{
	return nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 0, value, nullptr);
}

inline bool XmlCommand::NextParamAsBool(bool* value)
{
	int intValue;
	if (!nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 1, &intValue, nullptr))
	{
		return false;
	}
	*value = intValue != 0;
	return true;
}

inline bool XmlCommand::NextParamAsStr(char** value)
{
	return nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 2, nullptr, value);
}

inline CString XmlCommand::EncodeStr(const char* str)
{
	if (!str)
	{
		return "";
	}

	CString result;
	if (IsJson())
	{
		result = WebUtil::JsonEncode(str);
	}
	else
	{
		result = WebUtil::XmlEncode(str);
	}
	return result;
}

inline void XmlCommand::DecodeStr(char* str)
{
	if (IsJson())
	{
		WebUtil::JsonDecode(str);
	}
	else
	{
		WebUtil::XmlDecode(str);
	}
}

//*****************************************************************
// Commands

inline void ErrorXmlCommand::Execute()
{
	BuildErrorResponse(m_errCode, m_errText);
}

inline void PauseUnpauseXmlCommand::Execute()
{
	bool ok = true;

	g_WorkState->SetResumeTime(0);

	switch (m_pauseAction)
	{
		case paDownload:
			g_WorkState->SetPauseDownload(m_pause);
			break;

		case paPostProcess:
			g_WorkState->SetPausePostProcess(m_pause);
			break;

		case paScan:
			g_WorkState->SetPauseScan(m_pause);
			break;

		default:
			ok = false;
	}

	BuildBoolResponse(ok);
}

// bool scheduleresume(int Seconds)
inline void ScheduleResumeXmlCommand::Execute()
{
	int seconds = 0;
	bool hasParam = NextParamAsInt(&seconds);
	int64_t resumeTime = 0;
	if (!nzbget_rs_dqh_xmlrpc_calc_schedule_resume_time(
		hasParam ? 1 : 0, seconds, (int64_t)Util::CurrentTime(), &resumeTime))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	g_WorkState->SetResumeTime((time_t)resumeTime);

	BuildBoolResponse(true);
}

inline void ShutdownXmlCommand::Execute()
{
	BuildBoolResponse(true);
	ExitProc();
}

inline void ReloadXmlCommand::Execute()
{
	BuildBoolResponse(true);
	Reload();
}

inline void VersionXmlCommand::Execute()
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(IsJson() ? 1 : 0, Util::VersionRevision(), 0, 1023));
}

inline void DumpDebugXmlCommand::Execute()
{
	g_Log->LogDebugInfo();
	BuildBoolResponse(true);
}

inline void SetDownloadRateXmlCommand::Execute()
{
	int rate = 0;
	// in KB/s: a rate past INT_MAX / 1024 wrapped around to a tiny limit
	int bytesPerSec = 0;
	if (!NextParamAsInt(&rate) ||
		!nzbget_rs_rol_xmlrpc_validate_download_rate(rate, &bytesPerSec))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	g_WorkState->SetSpeedLimit(bytesPerSec);
	BuildBoolResponse(true);
}

inline void StatusXmlCommand::Execute()
{
	int postJobCount = 0;
	int urlCount = 0;
	int64 remainingSize, forcedSize;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbget_rs_dqh_xmlrpc_accum_queue_job_counts(
				nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
				(int)nzbInfo->GetKind(),
				(int)NzbInfo::nkUrl,
				&postJobCount,
				&urlCount);
		}
		downloadQueue->CalcRemainingSize(&remainingSize, &forcedSize);
	}

	int64 articleCache = g_ArticleCache->GetAllocated();
	const char* memoryPressure = g_MemoryMonitor ? g_MemoryMonitor->GetTierName() : "NORMAL";
	int effectiveArticleCacheMB = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveArticleCacheMB() : g_Options->GetArticleCache();
	int effectiveParBufferMB = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveParBufferMB(g_Options->GetParBuffer()) : g_Options->GetParBuffer();

	int upTimeSec, downloadTimeSec;
	int64 allBytes;
	bool serverStandBy;
	g_StatMeter->CalcTotalStat(&upTimeSec, &downloadTimeSec, &allBytes, &serverStandBy);

	int64 monthBytes, dayBytes;
	g_StatMeter->CalcQuotaUsage(monthBytes, dayBytes);

	int64 freeDiskSpace = 0;
	int64 totalDiskSpace = 0;
	int64 freeInterDiskSpace = 0;
	int64 totalInterDiskSpace = 0;

	if (!Util::EmptyStr(g_Options->GetDestDir()))
	{
		auto res = FileSystem::GetDiskState(g_Options->GetDestDir());
		if (res.has_value())
		{
			const auto& value = res.value();
			freeDiskSpace = value.available;
			totalDiskSpace = value.total;
		}
	}

	bool interDirEmpty = Util::EmptyStr(g_Options->GetInterDir());
	if (!interDirEmpty)
	{
		auto res = FileSystem::GetDiskState(g_Options->GetInterDir());
		if (res.has_value())
		{
			const auto& value = res.value();
			freeInterDiskSpace = value.available;
			totalInterDiskSpace = value.total;
		}
	}

	NzbgetRsRpcStatusInput statusFields{};
	statusFields.remaining_size = remainingSize;
	statusFields.forced_size = forcedSize;
	statusFields.article_cache = articleCache;
	statusFields.all_bytes = allBytes;
	statusFields.month_bytes = monthBytes;
	statusFields.day_bytes = dayBytes;
	statusFields.memory_pressure = memoryPressure;
	statusFields.effective_article_cache_mb = effectiveArticleCacheMB;
	statusFields.effective_par_buffer_mb = effectiveParBufferMB;
	statusFields.download_rate = g_StatMeter->CalcCurrentDownloadSpeed();
	statusFields.download_limit = (int)(g_WorkState->GetSpeedLimit());
	statusFields.thread_count = Thread::GetThreadCount() - 1; // not counting itself
	statusFields.post_job_count = postJobCount;
	statusFields.url_count = urlCount;
	statusFields.up_time_sec = upTimeSec;
	statusFields.download_time_sec = downloadTimeSec;
	statusFields.download_paused = g_WorkState->GetPauseDownload() ? 1 : 0;
	statusFields.server_stand_by = serverStandBy ? 1 : 0;
	statusFields.post_paused = g_WorkState->GetPausePostProcess() ? 1 : 0;
	statusFields.scan_paused = g_WorkState->GetPauseScan() ? 1 : 0;
	statusFields.quota_reached = g_WorkState->GetQuotaReached() ? 1 : 0;
	statusFields.free_disk_space = freeDiskSpace;
	statusFields.total_disk_space = totalDiskSpace;
	statusFields.inter_dir_empty = interDirEmpty ? 1 : 0;
	statusFields.free_inter_disk_space = freeInterDiskSpace;
	statusFields.total_inter_disk_space = totalInterDiskSpace;
	statusFields.server_time = (int)Util::CurrentTime();
	statusFields.resume_time = (int)g_WorkState->GetResumeTime();
	statusFields.feed_active = g_FeedCoordinator->HasActiveDownloads() ? 1 : 0;
	statusFields.queued_scripts = g_QueueScriptCoordinator->GetQueueSize();

	std::vector<NzbgetRsRpcServerStatus> servers;
	servers.reserve(g_ServerPool->GetServers()->size());
	for (NewsServer* server : g_ServerPool->GetServers())
	{
		servers.push_back(NzbgetRsRpcServerStatus{ server->GetId(), server->GetActive() ? 1 : 0 });
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_status(
		IsJson() ? 1 : 0, &statusFields, servers.data(), servers.size()));
}

inline void SysInfoXmlCommand::Execute()
{
	std::string response = IsJson()
		? System::ToJsonStr(*g_SystemInfo)
		: System::ToXmlStr(*g_SystemInfo);

	AppendResponse(response.c_str());
}

inline void SystemHealthXmlCommand::Execute()
{
	if (g_Options->GetSystemHealthCheck())
	{
		AppendRsBuf(m_response, g_SystemHealth->DiagnoseSerialized(IsJson()));
	}
	else
	{
		BuildErrorResponse(1, "System health check is disabled in configuration");
	}
}

// struct[] log(idfrom, entries)
inline void LogXmlCommand::Execute()
{
	m_idFrom = 0;
	m_nrEntries = 0;
	if (!NextParamAsInt(&m_idFrom) || !NextParamAsInt(&m_nrEntries) ||
		!nzbget_rs_roc_xmlrpc_log_params_valid(m_idFrom, m_nrEntries))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	debug("iIDFrom=%i", m_idFrom);
	debug("iNrEntries=%i", m_nrEntries);

	GuardedMessageList messages = GuardMessages();

	int start = messages->size();
	nzbget_rs_rco_rpc_log_slice_range(
		(int)messages->size(),
		messages->empty() ? 0 : (int)messages->front().GetId(),
		m_idFrom, m_nrEntries, &start, &m_nrEntries);

	std::vector<NzbgetRsRpcLogEntry> entries;
	if ((uint32)start < messages->size())
	{
		entries.reserve(messages->size() - (uint32)start);
		for (uint32 i = (uint32)start; i < messages->size(); i++)
		{
			Message& message = messages->at(i);
			entries.push_back(NzbgetRsRpcLogEntry{
				(int)message.GetId(),
				(int)message.GetKind(),
				(int)message.GetTime(),
				message.GetText()
			});
		}
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_log_list(
		IsJson() ? 1 : 0, entries.data(), entries.size()));
}

inline GuardedMessageList LogXmlCommand::GuardMessages()
{
	return g_Log->GuardMessages();
}

// struct[] listfiles(int IDFrom, int IDTo, int NZBID)
// For backward compatibility with 0.8 parameter "NZBID" is optional
inline void ListFilesXmlCommand::Execute()
{
	int idStart = 0;
	int idEnd = 0;
	bool hasIdStart = NextParamAsInt(&idStart);
	bool hasIdEnd = hasIdStart ? NextParamAsInt(&idEnd) : false;
	if (!nzbget_rs_roc_xmlrpc_list_files_params_valid(hasIdStart ? 1 : 0, hasIdEnd ? 1 : 0, idStart, idEnd, 0))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	// For backward compatibility with 0.8 parameter "NZBID" is optional (error checking omitted)
	int nzbId = 0;
	NextParamAsInt(&nzbId);

	if (!nzbget_rs_roc_xmlrpc_list_files_params_valid(hasIdStart ? 1 : 0, hasIdEnd ? 1 : 0, idStart, idEnd, nzbId))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	debug("iIDStart=%i", idStart);
	debug("iIDEnd=%i", idEnd);

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			int progress = 0;
			if (nzbget_rs_rco_rpc_list_files_match_and_progress(
				nzbId, fileInfo->GetNzbInfo()->GetId(), fileInfo->GetId(),
				idStart, idEnd, fileInfo->GetSize(), fileInfo->GetRemainingSize(),
				fileInfo->GetMissedSize(), fileInfo->GetFailedSize(),
				fileInfo->GetSuccessSize(), &progress))
			{
				NzbgetRsRpcFileItem item{};
				item.id = fileInfo->GetId();
				item.file_size = fileInfo->GetSize();
				item.remaining_size = fileInfo->GetRemainingSize();
				item.post_time = (int)fileInfo->GetTime();
				item.filename_confirmed = fileInfo->GetFilenameConfirmed() ? 1 : 0;
				item.paused = fileInfo->GetPaused() ? 1 : 0;
				item.nzb_id = fileInfo->GetNzbInfo()->GetId();
				item.nzb_name = fileInfo->GetNzbInfo()->GetName();
				item.nzb_filename = fileInfo->GetNzbInfo()->GetFilename();
				item.subject = fileInfo->GetSubject();
				item.filename = fileInfo->GetFilename();
				item.dest_dir = fileInfo->GetNzbInfo()->GetDestDir();
				item.category = fileInfo->GetNzbInfo()->GetCategory();
				item.priority = fileInfo->GetNzbInfo()->GetPriority();
				item.active_downloads = fileInfo->GetActiveDownloads();
				item.progress = progress;

				AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_file_item(
					IsJson() ? 1 : 0, index++ > 0 ? 1 : 0, &item));
			}
		}
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline void NzbInfoXmlCommand::AppendNzbInfoFields(NzbInfo* nzbInfo)
{
	time_t postStartTime = nzbInfo->GetPostInfo() ? nzbInfo->GetPostInfo()->GetStartTime() : 0;
	time_t curTime = postStartTime ? Util::CurrentTime() : 0;

	NzbgetRsRpcNzbInfoInput fields{};
	fields.id = nzbInfo->GetId();
	fields.name = nzbInfo->GetName();
	fields.kind = (int)nzbInfo->GetKind();
	fields.url = nzbInfo->GetUrl();
	fields.filename = nzbInfo->GetFilename();
	fields.dest_dir = nzbInfo->GetDestDir();
	fields.final_dir = nzbInfo->GetFinalDir();
	fields.category = nzbInfo->GetCategory();
	fields.par_status = (int)nzbInfo->GetParStatus();
	fields.extra_par_blocks = nzbInfo->GetExtraParBlocks();
	fields.unpack_status = (int)nzbInfo->GetUnpackStatus();
	fields.move_status = (int)nzbInfo->GetMoveStatus();
	fields.script_status = (int)nzbInfo->GetScriptStatuses()->CalcTotalStatus();
	fields.delete_status = (int)nzbInfo->GetDeleteStatus();
	fields.mark_status = (int)nzbInfo->GetMarkStatus();
	fields.url_status = (int)nzbInfo->GetUrlStatus();
	fields.size = nzbInfo->GetSize();
	fields.file_count = nzbInfo->GetFileCount();
	fields.min_time = (int)nzbInfo->GetMinTime();
	fields.max_time = (int)nzbInfo->GetMaxTime();
	fields.total_articles = nzbInfo->GetTotalArticles();
	fields.current_success_articles = nzbInfo->GetCurrentSuccessArticles();
	fields.current_failed_articles = nzbInfo->GetCurrentFailedArticles();
	fields.dupe_recovered_articles = nzbInfo->GetDupeRecoveredArticles();
	fields.dupe_recovered_bytes = nzbInfo->GetDupeRecoveredBytes();
	fields.dupe_recovered_holes = nzbInfo->GetDupeRecoveredHoles();
	fields.live_repairing = nzbInfo->GetLiveRepairThread() != nullptr ? 1 : 0;
	fields.health = nzbInfo->CalcHealth();
	fields.critical_health = nzbInfo->CalcCriticalHealth(false);
	fields.dupe_key = nzbInfo->GetDupeKey();
	fields.dupe_score = nzbInfo->GetDupeScore();
	fields.dupe_mode = (int)nzbInfo->GetDupeMode();
	fields.downloaded_size = nzbInfo->GetDownloadedSize();
	fields.download_sec = nzbInfo->GetDownloadSec();
	fields.post_total_sec = nzbInfo->GetPostTotalSec();
	fields.has_post_start_time = postStartTime != 0 ? 1 : 0;
	fields.cur_time = (int64)curTime;
	fields.post_start_time = (int64)postStartTime;
	fields.par_sec = nzbInfo->GetParSec();
	fields.repair_sec = nzbInfo->GetRepairSec();
	fields.unpack_sec = nzbInfo->GetUnpackSec();
	fields.message_count = nzbInfo->GetMessageCount();
	fields.cached_message_count = nzbInfo->GetCachedMessageCount();

	std::vector<NzbgetRsRpcParamItem> params;
	params.reserve(nzbInfo->GetParameters()->size());
	for (NzbParameter& parameter : nzbInfo->GetParameters())
	{
		params.push_back(NzbgetRsRpcParamItem{ parameter.GetName(), parameter.GetValue() });
	}

	std::vector<NzbgetRsRpcScriptItem> scripts;
	scripts.reserve(nzbInfo->GetScriptStatuses()->size());
	for (ScriptStatus& scriptStatus : nzbInfo->GetScriptStatuses())
	{
		scripts.push_back(NzbgetRsRpcScriptItem{ scriptStatus.GetName(), (int)scriptStatus.GetStatus() });
	}

	std::vector<NzbgetRsRpcServerStatItem> stats;
	stats.reserve(nzbInfo->GetCurrentServerStats()->size());
	for (ServerStat& serverStat : nzbInfo->GetCurrentServerStats())
	{
		stats.push_back(NzbgetRsRpcServerStatItem{
			serverStat.GetServerId(),
			serverStat.GetSuccessArticles(),
			serverStat.GetFailedArticles()
		});
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_nzb_info_fields(
		IsJson() ? 1 : 0, &fields,
		params.data(), params.size(),
		scripts.data(), scripts.size(),
		stats.data(), stats.size()));
}

inline void NzbInfoXmlCommand::AppendPostInfoFields(PostInfo* postInfo, int logEntries, bool postQueue)
{
	time_t curTime = postInfo ? Util::CurrentTime() : 0;

	std::optional<GuardedMessageList> messages;
	std::vector<NzbgetRsRpcLogEntry> entries;
	if (logEntries > 0 && postInfo)
	{
		messages.emplace(postInfo->GetNzbInfo()->GuardCachedMessages());
		int start = 0;
		if (nzbget_rs_rol_xmlrpc_post_info_log_slice(
			logEntries, (int)(*messages)->size(), &start, &logEntries))
		{
			entries.reserve((*messages)->size() - (uint32)start);
			for (uint32 i = (uint32)start; i < (*messages)->size(); i++)
			{
				Message& message = (*messages)->at(i);
				entries.push_back(NzbgetRsRpcLogEntry{
					(int)message.GetId(),
					(int)message.GetKind(),
					(int)message.GetTime(),
					message.GetText()
				});
			}
		}
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_post_info_fields(
		IsJson() ? 1 : 0,
		postQueue ? 1 : 0,
		postInfo ? 1 : 0,
		postInfo ? postInfo->GetProgressLabel() : nullptr,
		postInfo ? postInfo->GetStageProgress() : 0,
		(int64)curTime,
		postInfo ? (int64)postInfo->GetStageTime() : 0,
		postInfo ? (int64)postInfo->GetStartTime() : 0,
		entries.data(),
		entries.size()));
}

// struct[] listgroups(int NumberOfLogEntries)
inline void ListGroupsXmlCommand::Execute()
{
	int nrEntries = 0;
	NextParamAsInt(&nrEntries);

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		const char* status = DetectStatus(nzbInfo);

		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_group_item_start(
			IsJson() ? 1 : 0,
			index++ > 0 ? 1 : 0,
			nzbInfo->GetId(),
			nzbInfo->GetRemainingSize(),
			nzbInfo->GetPausedSize(),
			(int)nzbInfo->GetFileList()->size(),
			nzbInfo->GetRemainingParCount(),
			nzbInfo->GetPriority(),
			nzbInfo->GetActiveDownloads(),
			status));

		AppendNzbInfoFields(nzbInfo);
		AppendCondResponse(",\n", IsJson());
		AppendPostInfoFields(nzbInfo->GetPostInfo(), nrEntries, false);

		AppendResponse(IsJson() ? "}" : "</struct></value>\n");
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline const char* ListGroupsXmlCommand::DetectStatus(NzbInfo* nzbInfo)
{
	bool hasPostInfo = nzbInfo->GetPostInfo() != nullptr;
	int postStage = hasPostInfo ? (int)nzbInfo->GetPostInfo()->GetStage() : 0;
	bool queueScriptActive = false;
	bool hasQueueScriptJob = nzbget_rs_dqh_xmlrpc_should_check_queue_script_job(
		hasPostInfo ? 1 : 0, postStage, (int)PostInfo::ptQueued) != 0 &&
		g_QueueScriptCoordinator->HasJob(nzbInfo->GetId(), &queueScriptActive);
	return nzbget_rs_rco_rpc_detect_group_status(
		hasPostInfo ? 1 : 0,
		postStage,
		hasQueueScriptJob ? 1 : 0,
		queueScriptActive ? 1 : 0,
		(int)nzbInfo->GetDirectUnpackStatus(),
		nzbInfo->GetActiveDownloads(),
		(int)nzbInfo->GetKind(),
		nzbInfo->GetPausedSize(),
		nzbInfo->GetRemainingSize());
}

// v18:
//    bool editqueue(string Command, string Args, int[] IDs)
// v17:
//    bool editqueue(string Command, int Offset, string Args, int[] IDs)
inline void EditQueueXmlCommand::Execute()
{
	int action = -1;
	NzbgetRsBuf argsOrErr{nullptr, 0};
	int errCode = 0;
	IdList idList;
	int okParse = nzbget_rs_xmlrpc_parse_edit_queue(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		(int)DownloadQueue::eaFileMoveOffset,
		(int)DownloadQueue::eaGroupMoveOffset,
		&action,
		&argsOrErr,
		&errCode,
		&idList,
		[](void* ctx, int id)
		{
			static_cast<IdList*>(ctx)->push_back(id);
		});
	if (!okParse)
	{
		BuildErrorResponse(errCode, "%s", argsOrErr.data ? argsOrErr.data : "Invalid parameter");
		nzbget_rs_free(argsOrErr);
		return;
	}

	bool ok = DownloadQueue::Guard()->EditList(&idList, nullptr, DownloadQueue::mmId,
		(DownloadQueue::EEditAction)action, argsOrErr.data ? argsOrErr.data : "");
	nzbget_rs_free(argsOrErr);

	BuildBoolResponse(ok);
}
// v26.0: Added archived NZBs support.
//   bool append(string filename, string Category, int Priority, bool AddToTop, string Content, bool AddPaused, string DupeKey, int DupeScore, string DupeMode, bool autoCategory)
// v25.2: Added new parameter "autoCategory".
//   bool append(string NZBFilename, string Category, int Priority, bool AddToTop, string Content, bool AddPaused, string DupeKey, int DupeScore, string DupeMode, bool autoCategory)
// v16:
//   int append(string NZBFilename, string NZBContent, string Category, int Priority, bool AddToTop, bool AddPaused, string DupeKey, int DupeScore, string DupeMode, struct[] Parameters)
// v13 (new param order and new result type):
//   int append(string NZBFilename, string NZBContent, string Category, int Priority, bool AddToTop, bool AddPaused, string DupeKey, int DupeScore, string DupeMode)
// v12 (backward compatible, some params are optional):
//   bool append(string NZBFilename, string Category, int Priority, bool AddToTop, string Content, bool AddPaused, string DupeKey, int DupeScore, string DupeMode)
// appendfleet(DupeKey, Category, Priority, Timeout, Name1, Content1, Name2, Content2, ...):
// a content is the nzb-file in base64 or its url; see Fleet
inline void AppendFleetXmlCommand::Execute()
{
	if (!IsJson())
	{
		BuildErrorResponse(2, "appendfleet needs JSON-RPC");
		return;
	}
	Fleet::Request request;
	char* dupeKey = nullptr;
	char* category = nullptr;
	// an empty key is derived from the first member's name (Fleet)
	if (!NextParamAsStr(&dupeKey))
	{
		BuildErrorResponse(2, "Invalid parameter (DupeKey)");
		return;
	}
	if (!NextParamAsStr(&category))
	{
		BuildErrorResponse(2, "Invalid parameter (Category)");
		return;
	}
	if (!NextParamAsInt(&request.priority))
	{
		BuildErrorResponse(2, "Invalid parameter (Priority)");
		return;
	}
	if (!NextParamAsInt(&request.timeoutSec))
	{
		BuildErrorResponse(2, "Invalid parameter (Timeout)");
		return;
	}
	DecodeStr(dupeKey);
	DecodeStr(category);
	request.dupeKey = dupeKey;
	request.category = category;
	char* name = nullptr;
	while (NextParamAsStr(&name))
	{
		char* content = nullptr;
		if (!NextParamAsStr(&content))
		{
			BuildErrorResponse(2, "Invalid parameter (Content of %s)", name);
			return;
		}
		if (nzbget_rs_dqh_xmlrpc_is_fleet_member_limit_reached(
			request.members.size(), Fleet::MaxMembers) != 0)
		{
			BuildErrorResponse(2, "Too many members (at most %i)", Fleet::MaxMembers);
			return;
		}
		DecodeStr(name);
		DecodeStr(content);
		Fleet::Member member;
		member.name = name;
		if (nzbget_rs_ntw_xmlrpc_is_http_url(content) != 0)
		{
			member.url = content;
		}
		else
		{
			int len = WebUtil::DecodeBase64(content, 0, content);
			member.data.assign(content, len);
		}
		request.members.push_back(std::move(member));
	}

	Fleet::Result result = Fleet::Append(std::move(request));

	std::vector<NzbgetRsRpcFleetMember> entries;
	entries.reserve(result.members.size());
	for (const Fleet::Entry& entry : result.members)
	{
		entries.push_back(NzbgetRsRpcFleetMember{
			entry.nzbId,
			entry.name.c_str(),
			entry.rank,
			entry.alive,
			entry.status.c_str(),
			entry.sameAs,
			entry.sameAsRank,
			entry.reason.c_str()
		});
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_fleet_result(
		result.chosen, result.complete ? 1 : 0, result.reason.c_str(),
		entries.data(), entries.size()));
}

inline void DownloadXmlCommand::Execute()
{
	bool v13 = true;

	char* filename;
	if (!NextParamAsStr(&filename))
	{
		BuildErrorResponse(2, "Invalid parameter (Filename)");
		return;
	}

	char* content;
	if (!NextParamAsStr(&content))
	{
		BuildErrorResponse(2, "Invalid parameter (Content)");
		return;
	}

	char* category;
	if (!NextParamAsStr(&category))
	{
		v13 = false;
		category = content;
	}

	DecodeStr(filename);
	DecodeStr(category);

	debug("FileName=%s", filename);

	// For backward compatibility with 0.8 parameter "Priority" is optional (error checking omitted)
	int priority = 0;
	NextParamAsInt(&priority);

	bool addTop;
	if (!NextParamAsBool(&addTop))
	{
		BuildErrorResponse(2, "Invalid parameter (AddTop)");
		return;
	}

	if (!v13 && !NextParamAsStr(&content))
	{
		BuildErrorResponse(2, "Invalid parameter (FileContent)");
		return;
	}

	DecodeStr(content);

	bool addPaused = false;
	char* dupeKey = nullptr;
	int dupeScore = 0;
	bool autoCategory = false;
	EDupeMode dupeMode = dmScore;
	if (NextParamAsBool(&addPaused))
	{
		if (!NextParamAsStr(&dupeKey))
		{
			BuildErrorResponse(2, "Invalid parameter (DupeKey)");
			return;
		}
		DecodeStr(dupeKey);
		if (!NextParamAsInt(&dupeScore))
		{
			BuildErrorResponse(2, "Invalid parameter (DupeScore)");
			return;
		}
		char* dupeModeStr = nullptr;
		int parsedDupeMode = NextParamAsStr(&dupeModeStr)
			? nzbget_rs_ntw_xmlrpc_parse_dupe_mode(
				dupeModeStr, (int)dmScore, (int)dmAll, (int)dmForce)
			: -1;
		if (parsedDupeMode < 0)
		{
			BuildErrorResponse(2, "Invalid parameter (DupeMode)");
			return;
		}
		dupeMode = (EDupeMode)parsedDupeMode;

		NextParamAsBool(&autoCategory);
	}
	else if (v13)
	{
		BuildErrorResponse(2, "Invalid parameter (AddPaused)");
		return;
	}

	NzbParameterList Params;
	if (v13)
	{
		char* paramName = nullptr;
		char* paramValue = nullptr;
		while (NextParamAsStr(&paramName))
		{
			if (!NextParamAsStr(&paramValue))
			{
				BuildErrorResponse(2, "Invalid parameter (Parameters)");
				return;
			}
			// decoded like every other string: a password with a quote or a
			// backslash was stored escaped, and unpacking failed
			DecodeStr(paramName);
			DecodeStr(paramValue);
			Params.SetParameter(paramName, paramValue);
		}
	}

	if (nzbget_rs_ntw_xmlrpc_is_http_url(content) != 0)
	{
		// add url
		std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
		nzbInfo->SetKind(NzbInfo::nkUrl);
		nzbInfo->SetUrl(content);
		nzbInfo->SetFilename(filename);
		nzbInfo->SetCategory(category);
		nzbInfo->SetAutoCategory(autoCategory);
		nzbInfo->SetPriority(priority);
		nzbInfo->SetAddUrlPaused(addPaused);
		nzbInfo->SetDupeKey(dupeKey ? dupeKey : "");
		nzbInfo->SetDupeScore(dupeScore);
		nzbInfo->SetDupeMode(dupeMode);
		nzbInfo->GetParameters()->CopyFrom(&Params);
		int nzbId = nzbInfo->GetId();

		info("Queue %s", *nzbInfo->MakeNiceUrlName(content, filename));

		g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), addTop);

		if (v13)
		{
			BuildIntResponse(nzbId);
		}
		else
		{
			BuildBoolResponse(true);
		}
	}
	else
	{
		int len = WebUtil::DecodeBase64(content, 0, content);
		content[len] = '\0';
		if (nzbget_rs_unpack_can_extract_with_zipfast(filename) != 0)
		{
			const auto status = g_Scanner->AddArchive(
				filename,
				category,
				autoCategory,
				priority,
				dupeKey,
				dupeScore,
				dupeMode,
				Params.empty() ? nullptr : &Params,
				addTop,
				addPaused,
				nullptr,
				content,
				len
			);
			BuildBoolResponse(status == Scanner::asSuccess);
		}
		else
		{
			int nzbId = -1;
			g_Scanner->AddExternalFile(
				filename,
				category,
				autoCategory,
				priority,
				dupeKey,
				dupeScore,
				dupeMode,
				Params.empty() ? nullptr : &Params,
				addTop,
				addPaused,
				nullptr,
				nullptr,
				content,
				len,
				&nzbId
			);

			int appendMode = nzbget_rs_dqh_xmlrpc_classify_append_result_mode(
				v13 ? 1 : 0, nzbId);
			if (appendMode == 1)
			{
				BuildIntResponse(nzbId);
			}
			else
			{
				BuildBoolResponse(appendMode == 2);
			}
		}
	}
}

// deprecated
inline void PostQueueXmlCommand::Execute()
{
	int nrEntries = 0;
	NextParamAsInt(&nrEntries);

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		PostInfo* postInfo = nzbInfo->GetPostInfo();
		if (!postInfo)
		{
			continue;
		}

		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_postqueue_item_start(
			IsJson() ? 1 : 0,
			index++ > 0 ? 1 : 0,
			nzbInfo->GetId(),
			postInfo->GetNzbInfo()->GetName(),
			(int)postInfo->GetStage(),
			postInfo->GetFileProgress()));

		AppendNzbInfoFields(postInfo->GetNzbInfo());
		AppendCondResponse(",\n", IsJson());
		AppendPostInfoFields(postInfo, nrEntries, true);

		AppendResponse(IsJson() ? "}" : "</struct></value>\n");
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline void WriteLogXmlCommand::Execute()
{
	char* kind;
	char* text;
	if (!NextParamAsStr(&kind) || !NextParamAsStr(&text))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	DecodeStr(text);

	debug("Kind=%s, Text=%s", kind, text);

	switch (nzbget_rs_rco_rpc_parse_write_log_kind(kind))
	{
		case 0: info("%s", text); break;
		case 1: warn("%s", text); break;
		case 2: error("%s", text); break;
		case 4: detail("%s", text); break;
		case 3: debug("%s", text); break;
		default:
			BuildErrorResponse(3, "Invalid Kind");
			return;
	}

	BuildBoolResponse(true);
}

inline void ClearLogXmlCommand::Execute()
{
	g_Log->Clear();

	BuildBoolResponse(true);
}

inline void ScanXmlCommand::Execute()
{
	bool syncMode = false;
	// optional parameter "SyncMode"
	NextParamAsBool(&syncMode);

	g_Scanner->ScanNzbDir(syncMode);
	BuildBoolResponse(true);
}

// struct[] history(bool hidden)
// Parameter "hidden" is optional (new in v12)
inline void HistoryXmlCommand::Execute()
{
	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	bool dup = false;
	NextParamAsBool(&dup);

	int index = 0;

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	for (HistoryInfo* historyInfo : guard->GetHistory())
	{
		if (nzbget_rs_roc_xmlrpc_history_should_skip_dup(
			(int)historyInfo->GetKind(), (int)HistoryInfo::hkDup, dup ? 1 : 0))
		{
			continue;
		}

		NzbInfo* nzbInfo = nullptr;
		const char* status = DetectStatus(historyInfo);
		int hasComma = index++ > 0 ? 1 : 0;
		int entryKind = nzbget_rs_dqh_xmlrpc_classify_history_entry_kind(
			(int)historyInfo->GetKind(),
			(int)HistoryInfo::hkNzb,
			(int)HistoryInfo::hkUrl,
			(int)HistoryInfo::hkDup);

		if (entryKind == 1)
		{
			nzbInfo = historyInfo->GetNzbInfo();

			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_history_nzb_start(
				IsJson() ? 1 : 0,
				hasComma,
				historyInfo->GetId(),
				historyInfo->GetName(),
				nzbInfo->GetParkedFileCount(),
				!nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
				(int)historyInfo->GetTime(),
				status));
		}
		else if (entryKind == 2)
		{
			DupInfo* dupInfo = historyInfo->GetDupInfo();

			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_history_dup_item(
				IsJson() ? 1 : 0,
				hasComma,
				historyInfo->GetId(),
				historyInfo->GetName(),
				(int)historyInfo->GetTime(),
				dupInfo->GetSize(),
				dupInfo->GetDupeKey(),
				dupInfo->GetDupeScore(),
				(int)dupInfo->GetDupeMode(),
				(int)dupInfo->GetStatus(),
				status));
		}

		if (nzbInfo)
		{
			AppendNzbInfoFields(nzbInfo);
		}

		AppendResponse(IsJson() ? "}" : "</struct></value>");
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

inline const char* HistoryXmlCommand::DetectStatus(HistoryInfo* historyInfo)
{
	const char* status = "FAILURE/INTERNAL_ERROR";
	int entryKind = nzbget_rs_dqh_xmlrpc_classify_history_entry_kind(
		(int)historyInfo->GetKind(),
		(int)HistoryInfo::hkNzb,
		(int)HistoryInfo::hkUrl,
		(int)HistoryInfo::hkDup);

	if (entryKind == 1)
	{
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		status = nzbInfo->MakeTextStatus(false);
	}
	else if (entryKind == 2)
	{
		DupInfo* dupInfo = historyInfo->GetDupInfo();
		status = nzbget_rs_rco_rpc_detect_history_dup_status((int)dupInfo->GetStatus());
	}

	return status;
}

// Deprecated in v13
inline void UrlQueueXmlCommand::Execute()
{
	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	int index = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetKind() == NzbInfo::nkUrl)
		{
			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_urlqueue_item(
				IsJson() ? 1 : 0,
				index++ > 0 ? 1 : 0,
				nzbInfo->GetId(),
				nzbInfo->GetFilename(),
				nzbInfo->GetUrl(),
				nzbInfo->GetName(),
				nzbInfo->GetCategory(),
				nzbInfo->GetPriority()));
		}
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

// struct[] config()
inline void ConfigXmlCommand::Execute()
{
	Options::GuardedOptEntries optEntries = g_Options->GuardOptEntries();
	std::vector<NzbgetRsRpcConfigItem> entries;
	entries.reserve(optEntries->size());
	for (Options::OptEntry& optEntry : optEntries)
	{
		entries.push_back(NzbgetRsRpcConfigItem{
			optEntry.GetName(),
			optEntry.GetValue(),
			optEntry.Restricted() ? 1 : 0
		});
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_config_list(
		IsJson() ? 1 : 0,
		(int)m_userAccess,
		(int)XmlRpcProcessor::uaRestricted,
		entries.data(),
		entries.size()));
}

// struct[] loadconfig()
inline void LoadConfigXmlCommand::Execute()
{
	int fault = 0;
	NzbgetRsBuf buf = nzbget_rs_xmlrpc_execute_load_config(
		IsJson() ? 1 : 0,
		(int)m_userAccess,
		(int)XmlRpcProcessor::uaRestricted,
		g_Options ? g_Options->GetConfigFilename() : nullptr,
		&fault);
	m_fault = fault != 0;
	AppendRsBuf(m_response, buf);
}

inline void LoadExtensionsXmlCommand::Execute()
{
	bool loadFromDisk = false;
	bool isJson = IsJson();

	if (!NextParamAsBool(&loadFromDisk))
	{
		BuildErrorResponse(2, "Invalid parameter (Load from disk)");
		return;
	}

	if (loadFromDisk)
	{
		const auto& error = g_ExtensionManager->LoadExtensions();
		if (error)
		{
			BuildErrorResponse(3, "%s", error.value().c_str());
			return;
		}
	}

	AppendResponse(isJson ? "[\n" : "<array><data>\n");

	int index = 0;

	g_ExtensionManager->ForEach([&](auto extension)
		{
			const std::string response = isJson
				? Extension::ToJsonStr(*extension)
				: Extension::ToXmlStr(*extension);

			AppendCondResponse(",\n", nzbget_rs_dqh_xmlrpc_should_emit_json_array_comma(
				isJson ? 1 : 0, index++) != 0);
			AppendResponse(response.c_str());
		}
	);

	AppendResponse(isJson ? "\n]" : "</data></array>\n");
}

inline void DownloadExtensionXmlCommand::Execute()
{
	char* url;
	if (!NextParamAsStr(&url))
	{
		BuildErrorResponse(2, "Invalid parameter (URL)");
		return;
	}
	DecodeStr(url);

	char* extName;
	if (!NextParamAsStr(&extName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension name)");
		return;
	}
	DecodeStr(extName);

	if (Util::EmptyStr(g_Options->GetTempDir()))
	{
		BuildErrorResponse(3, "\"TempDir\" is not specified");
		return;
	}

	const auto result = g_ExtensionManager->DownloadExtension(url, extName);
	const auto& filename = std::get<1>(result);
	// the downloaded archive stayed in TempDir whenever the install didn't
	// happen (a successful install removes it)
	auto removeDownload = [&filename]()
		{
			fs::error_code ec;
			fs::remove(filename, ec);
		};
	bool ok = std::get<0>(result) == WebDownloader::adFinished;
	if (!ok)
	{
		removeDownload();
		BuildErrorResponse(3, "Failed to read URL");
		return;
	}

	if (g_Options->GetScriptDirPaths().empty())
	{
		removeDownload();
		BuildErrorResponse(3, "\"ScriptDir\" is not specified");
		return;
	}

	const auto error = g_ExtensionManager->InstallExtension(filename, g_Options->GetScriptDirPaths().front());
	if (error)
	{
		removeDownload();
		BuildErrorResponse(3, "%s", error.value().c_str());
		return;
	}

	BuildBoolResponse(true);
}

inline void UpdateExtensionXmlCommand::Execute()
{
	char* url;
	if (!NextParamAsStr(&url))
	{
		BuildErrorResponse(2, "Invalid parameter (URL)");
		return;
	}
	DecodeStr(url);

	char* extName;
	if (!NextParamAsStr(&extName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension name)");
		return;
	}
	DecodeStr(extName);

	const auto result = g_ExtensionManager->DownloadExtension(url, extName);
	bool ok = std::get<0>(result) == WebDownloader::adFinished;
	if (!ok)
	{
		// a partly downloaded archive stayed in TempDir
		fs::error_code ec;
		fs::remove(std::get<1>(result), ec);
		BuildErrorResponse(3, "Failed to read URL");
		return;
	}

	const auto filename = std::get<1>(result);
	const auto error = g_ExtensionManager->UpdateExtension(filename, extName);
	if (error)
	{
		BuildErrorResponse(3, "%s", error.value().c_str());
		return;
	}

	BuildBoolResponse(true);
}

inline void DeleteExtensionXmlCommand::Execute()
{
	char* extName;
	if (!NextParamAsStr(&extName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension name)");
		return;
	}
	DecodeStr(extName);

	const auto error = g_ExtensionManager->DeleteExtension(extName);
	if (error)
	{
		BuildErrorResponse(2, "%s", error.value().c_str());
		return;
	}

	BuildBoolResponse(true);
}

inline void TestExtensionXmlCommand::Execute()
{
	char* extEntryFileName;
	if (!NextParamAsStr(&extEntryFileName))
	{
		BuildErrorResponse(2, "Invalid parameter (Extension entry file name)");
		return;
	}
	DecodeStr(extEntryFileName);

	const auto found = Util::FindExecutorProgram(extEntryFileName, g_Options->GetShellOverride());
	if (!found)
	{
		BuildErrorResponse(2, "Failed to find the corresponding executor");
		return;
	}

	BuildBoolResponse(true);
}

// bool saveconfig(struct[] data)
inline void SaveConfigXmlCommand::Execute()
{
	int fault = 0;
	NzbgetRsBuf buf = nzbget_rs_xmlrpc_execute_save_config(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		g_Options ? g_Options->GetConfigFilename() : nullptr,
		&fault);
	m_fault = fault != 0;
	AppendRsBuf(m_response, buf);
}

// struct[] configtemplates(bool loadFromDisk)
// parameter "loadFromDisk" is optional (new in v14)
inline void ConfigTemplatesXmlCommand::Execute()
{
	bool loadFromDisk = false;
	NextParamAsBool(&loadFromDisk);

	ScriptConfig::ConfigTemplates* configTemplates = g_ScriptConfig->GetConfigTemplates();

	ScriptConfig::ConfigTemplates loadedConfigTemplates;
	if (loadFromDisk)
	{
		if (!g_ScriptConfig->LoadConfigTemplates(&loadedConfigTemplates))
		{
			BuildErrorResponse(3, "Could not read configuration templates");
			return;
		}
		configTemplates = &loadedConfigTemplates;
	}

	std::vector<const char*> templates;
	templates.reserve(configTemplates->size());
	for (ScriptConfig::ConfigTemplate& configTemplate : configTemplates)
	{
		templates.push_back(configTemplate.GetTemplate());
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_config_templates(
		IsJson() ? 1 : 0, templates.data(), templates.size()));
}

// struct[] viewfeed(int id)
// v12:
// struct[] previewfeed(string name, string url, string filter, bool pauseNzb, string category,
//		int priority, bool includeNonMatching, int cacheTimeSec, string cacheId)
// v16:
// struct[] previewfeed(int id, string name, string url, string filter, bool backlog, bool pauseNzb, string category,
//		int priority, int interval, string feedfilter, bool includeNonMatching, int cacheTimeSec, string cacheId)
inline void ViewFeedXmlCommand::Execute()
{
	bool includeNonMatching = false;
	std::shared_ptr<FeedItemList> feedItems;

	if (m_preview)
	{
		int id = 0;
		char* name;
		char* url;
		char* filter;
		bool backlog = true;
		bool pauseNzb;
		char* category;
		char* categorySourceStr = nullptr;	// optional: unset when not sent
		auto categorySource = FeedInfo::CategorySource::NZBFile;
		int interval = 0;
		int priority;
		char* feedFilter = nullptr;
		char* cacheId;
		int cacheTimeSec;

		// if the first parameter is int then it's the v16 signature
		bool v16 = NextParamAsInt(&id);

		if (!NextParamAsStr(&name) || !NextParamAsStr(&url) || !NextParamAsStr(&filter) ||
			(v16 && !NextParamAsBool(&backlog)) || !NextParamAsBool(&pauseNzb) ||
			!NextParamAsStr(&category) || !NextParamAsInt(&priority) ||
			(v16 && (!NextParamAsInt(&interval) || !NextParamAsStr(&feedFilter))) ||
			!NextParamAsBool(&includeNonMatching) || !NextParamAsInt(&cacheTimeSec) ||
			!NextParamAsStr(&cacheId))
		{
			BuildErrorResponse(2, "Invalid parameter");
			return;
		}

		NextParamAsStr(&categorySourceStr);
		if (categorySourceStr)
		{
			categorySource = (FeedInfo::CategorySource)nzbget_rs_ntw_xmlrpc_parse_category_source(
				categorySourceStr,
				(int)FeedInfo::CategorySource::NZBFile,
				(int)FeedInfo::CategorySource::FeedFile,
				(int)FeedInfo::CategorySource::Auto);
		}

		DecodeStr(name);
		DecodeStr(url);
		DecodeStr(filter);
		DecodeStr(cacheId);
		DecodeStr(category);

		debug("Url=%s", url);
		debug("Filter=%s", filter);

		feedItems = g_FeedCoordinator->PreviewFeed(
			id,
			name,
			url,
			filter,
			backlog,
			pauseNzb,
			category,
			categorySource,
			priority,
			interval,
			feedFilter,
			cacheTimeSec,
			cacheId
		);
	}
	else
	{
		int id = 0;
		if (!NextParamAsInt(&id) || !NextParamAsBool(&includeNonMatching))
		{
			BuildErrorResponse(2, "Invalid parameter");
			return;
		}

		debug("ID=%i", id);

		feedItems = g_FeedCoordinator->ViewFeed(id);
	}

	if (!feedItems)
	{
		BuildErrorResponse(3, "Could not read feed");
		return;
	}

	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");
	int index = 0;

	for (FeedItemInfo& feedItemInfo : feedItems.get())
	{
		int sizeMB = 0;
		if (nzbget_rs_rol_xmlrpc_view_feed_should_include(
			includeNonMatching ? 1 : 0, (int)feedItemInfo.GetMatchStatus(),
			(int)FeedItemInfo::msAccepted, feedItemInfo.GetSize(), &sizeMB))
		{
			NzbgetRsRpcFeedItem item{};
			item.title = feedItemInfo.GetTitle();
			item.filename = feedItemInfo.GetFilename();
			item.url = feedItemInfo.GetUrl();
			item.size = feedItemInfo.GetSize();
			item.size_mb = sizeMB;
			item.category = feedItemInfo.GetCategory();
			item.add_category = feedItemInfo.GetAddCategory().c_str();
			item.pause_nzb = feedItemInfo.GetPauseNzb() ? 1 : 0;
			item.priority = feedItemInfo.GetPriority();
			item.time = (int)feedItemInfo.GetTime();
			item.match_status = (int)feedItemInfo.GetMatchStatus();
			item.match_rule = feedItemInfo.GetMatchRule();
			item.dupe_key = feedItemInfo.GetDupeKey();
			item.dupe_score = feedItemInfo.GetDupeScore();
			item.dupe_mode = (int)feedItemInfo.GetDupeMode();
			item.status = (int)feedItemInfo.GetStatus();

			AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_feed_item(
				IsJson() ? 1 : 0, index++ > 0 ? 1 : 0, &item));
		}
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

// bool fetchfeed(int ID)
inline void FetchFeedXmlCommand::Execute()
{
	int id;
	if (!NextParamAsInt(&id))
	{
		BuildErrorResponse(2, "Invalid parameter (ID)");
		return;
	}

	g_FeedCoordinator->FetchFeed(id);

	BuildBoolResponse(true);
}

// bool editserver(int ID, bool Active)
inline void EditServerXmlCommand::Execute()
{
	bool ok = false;
	int first = true;

	int id;
	while (NextParamAsInt(&id))
	{
		first = false;

		bool active;
		if (!NextParamAsBool(&active))
		{
			BuildErrorResponse(2, "Invalid parameter");
			return;
		}

		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (nzbget_rs_dqh_xmlrpc_matches_edit_server_id(server->GetId(), id) != 0)
			{
				server->SetActive(active);
				ok = true;
			}
		}
	}

	if (first)
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	if (ok)
	{
		g_ServerPool->Changed();
	}

	BuildBoolResponse(ok);
}

// string readurl(string url, string infoname)
inline void ReadUrlXmlCommand::Execute()
{
	char* url;
	if (!NextParamAsStr(&url))
	{
		BuildErrorResponse(2, "Invalid parameter (URL)");
		return;
	}
	DecodeStr(url);

	char* infoName;
	if (!NextParamAsStr(&infoName))
	{
		BuildErrorResponse(2, "Invalid parameter (InfoName)");
		return;
	}
	DecodeStr(infoName);

	// generate temp file name
	BString<1024> tempFileName;
	int num = 1;
	while (num == 1 || FileSystem::FileExists(tempFileName))
	{
		NzbgetRsBuf tmpBuf = nzbget_rs_tpr_xmlrpc_format_readurl_temp_filename(
			g_Options->GetTempDir(), PATH_SEPARATOR, num);
		tempFileName.Set(tmpBuf.data ? tmpBuf.data : "", (int)tmpBuf.len);
		nzbget_rs_free(tmpBuf);
		num++;
	}

	std::unique_ptr<WebDownloader> downloader = std::make_unique<WebDownloader>();
	downloader->SetUrl(url);
	downloader->SetForce(true);
	downloader->SetRetry(false);
	downloader->SetOutputFilename(tempFileName);
	downloader->SetInfoName(infoName);

	// do sync download
	WebDownloader::EStatus status = downloader->DownloadWithRedirects(5);
	bool ok = status == WebDownloader::adFinished;

	downloader.reset();

	if (ok)
	{
		CharBuffer fileContent;
		FileSystem::LoadFileIntoBuffer(tempFileName, fileContent, true);
		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(IsJson() ? 1 : 0, fileContent, 1, 0));
	}
	else
	{
		BuildErrorResponse(3, "Could not read url");
	}

	FileSystem::DeleteFile(tempFileName);
}

// string checkupdates()
inline void CheckUpdatesXmlCommand::Execute()
{
	CString updateInfo;
	bool ok = g_Maintenance->CheckUpdates(updateInfo);

	if (ok)
	{
		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(IsJson() ? 1 : 0, updateInfo, 1, 0));
	}
	else
	{
		BuildErrorResponse(3, "Could not read update info from update-info-script");
	}
}

// bool startupdate(string branch)
inline void StartUpdateXmlCommand::Execute()
{
	char* branchName;
	if (!NextParamAsStr(&branchName))
	{
		BuildErrorResponse(2, "Invalid parameter (Branch)");
		return;
	}
	DecodeStr(branchName);

	Maintenance::EBranch branch;
	int branchCode = nzbget_rs_rco_rpc_parse_update_branch(branchName);
	if (branchCode < 0)
	{
		BuildErrorResponse(2, "Invalid parameter (Branch)");
		return;
	}
	branch = (Maintenance::EBranch)branchCode;

	bool ok = g_Maintenance->StartUpdate(branch);

	BuildBoolResponse(ok);
}

// struct[] logupdate(idfrom, entries)
inline GuardedMessageList LogUpdateXmlCommand::GuardMessages()
{
	return g_Maintenance->GuardMessages();
}

// struct[] servervolumes(bool BytesPerSeconds, bool BytesPerMinutes, bool BytesPerHours, bool BytesPerDays, bool ArticlesPerDays)
inline void ServerVolumesXmlCommand::Execute()
{
	AppendResponse(IsJson() ? "[\n" : "<array><data>\n");

	bool BytesPer[] = {true, true, true, true};
	NextParamAsBool(&BytesPer[0])
		&& NextParamAsBool(&BytesPer[1])
		&& NextParamAsBool(&BytesPer[2])
		&& NextParamAsBool(&BytesPer[3]);

	bool articlesPerDays = true;
	NextParamAsBool(&articlesPerDays);

	int index = 0;

	for (ServerVolume& serverVolume : g_StatMeter->GuardServerVolumes())
	{
		const auto& articles = serverVolume.GetArticlesPerDays();
		std::vector<NzbgetRsRpcArticlePair> articleEntries;
		if (articlesPerDays)
		{
			articleEntries.reserve(articles.size());
			for (const auto& art : articles)
			{
				articleEntries.push_back(NzbgetRsRpcArticlePair{ art.failed, art.success });
			}
		}

		const ServerVolume::VolumeArray* bps = serverVolume.BytesPerSeconds();
		const ServerVolume::VolumeArray* bpm = serverVolume.BytesPerMinutes();
		const ServerVolume::VolumeArray* bph = serverVolume.BytesPerHours();
		const ServerVolume::VolumeArray* bpd = serverVolume.BytesPerDays();

		NzbgetRsRpcServerVolumeItem volFields{};
		volFields.server_id = index;
		volFields.data_time = static_cast<int>(serverVolume.GetDataTime());
		volFields.first_day = serverVolume.GetFirstDay();
		volFields.total_bytes = serverVolume.GetTotalBytes();
		volFields.custom_bytes = serverVolume.GetCustomBytes();
		volFields.custom_time = static_cast<int>(serverVolume.GetCustomTime());
		volFields.counters_reset_time = static_cast<int>(serverVolume.GetCountersResetTime());
		volFields.sec_slot = serverVolume.GetSecSlot();
		volFields.min_slot = serverVolume.GetMinSlot();
		volFields.hour_slot = serverVolume.GetHourSlot();
		volFields.day_slot = serverVolume.GetDaySlot();
		volFields.include_seconds = BytesPer[0] ? 1 : 0;
		volFields.bytes_per_seconds = bps->data();
		volFields.bytes_per_seconds_len = bps->size();
		volFields.include_minutes = BytesPer[1] ? 1 : 0;
		volFields.bytes_per_minutes = bpm->data();
		volFields.bytes_per_minutes_len = bpm->size();
		volFields.include_hours = BytesPer[2] ? 1 : 0;
		volFields.bytes_per_hours = bph->data();
		volFields.bytes_per_hours_len = bph->size();
		volFields.include_days = BytesPer[3] ? 1 : 0;
		volFields.bytes_per_days = bpd->data();
		volFields.bytes_per_days_len = bpd->size();
		volFields.include_articles = articlesPerDays ? 1 : 0;
		volFields.articles_per_days = articleEntries.data();
		volFields.articles_per_days_len = articleEntries.size();

		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_server_volume_item(
			IsJson() ? 1 : 0,
			index,
			&volFields));

		index++;
	}

	AppendResponse(IsJson() ? "\n]" : "</data></array>\n");
}

// bool resetservervolume(int serverid, string counter);
inline void ResetServerVolumeXmlCommand::Execute()
{
	int serverId;
	char* counter;
	if (!NextParamAsInt(&serverId) || !NextParamAsStr(&counter))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	int counterKind = nzbget_rs_ntw_xmlrpc_validate_reset_counter(counter);
	if (counterKind == 0)
	{
		BuildErrorResponse(3, "Invalid Counter");
		return;
	}
	bool reset = counterKind == 1;

	bool ok = false;
	int index = 0;
	for (ServerVolume& serverVolume : g_StatMeter->GuardServerVolumes())
	{
		if (nzbget_rs_dqh_xmlrpc_matches_reset_server_volume(index, serverId) != 0)
		{
			if (reset)
				serverVolume.Reset();
			else
				serverVolume.ResetCustom();
			ok = true;
		}
		index++;
	}

	// saved like a download's counts: a restart before the next download
	// brought the old counters back
	if (ok)
	{
		g_StatMeter->MarkChanged();
	}

	BuildBoolResponse(ok);
}

// struct[] loadlog(nzbid, logidfrom, logentries)
inline void LoadLogXmlCommand::Execute()
{
	m_nzbInfo = nullptr;
	m_nzbId = 0;
	if (!NextParamAsInt(&m_nzbId))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	LogXmlCommand::Execute();
	m_downloadQueue.reset();
}

inline GuardedMessageList LoadLogXmlCommand::GuardMessages()
{
	// TODO: optimize for m_iIDFrom and m_iNrEntries
	g_DiskState->LoadNzbMessages(m_nzbId, &m_messages);

	if (m_messages.empty())
	{
		// When we returing cached messages from m_nzbInfo we have to make sure that
		// m_nzbInfo isn't destroyed during we are working with CachedMessages.
		// For that purpose we set a lock on DownloadQueue,
		// which will be released later in LoadLogXmlCommand::Execute().

		m_downloadQueue = std::make_unique<GuardedDownloadQueue>(DownloadQueue::Guard());
		m_nzbInfo = (*m_downloadQueue)->GetQueue()->Find(m_nzbId);
		if (m_nzbInfo)
		{
			return m_nzbInfo->GuardCachedMessages();
		}
		else
		{
			// No nzb-object available, the lock on DownloadQueue can be released right now.
			m_downloadQueue.reset();
		}
	}

	return GuardedMessageList(&m_messages, nullptr);
}

// string testserver(string host, int port, string username, string password, bool encryption, string cipher, int timeout, int certVerifLevel);
inline void TestServerXmlCommand::Execute()
{
	TestServerXmlCommand::ServerParams params;
	char* host;
	int port;
	char* username;
	char* password;
	bool encryption;
	char* cipher;
	int timeout;
	int certVerifLevel;

	// a JSON body only with POST: over GET (JSON-P) m_request is the query string
	if (nzbget_rs_dqh_xmlrpc_should_parse_json_post_test_server(
		IsJson() ? 1 : 0, m_request != nullptr ? 1 : 0,
		(int)m_httpMethod, (int)XmlRpcProcessor::hmPost) != 0)
	{
		// rust/src/rpcparams.rs; a std::exception while copying the strings
		// gives "Invalid parameters", as in ParseRequestParams
		struct Context
		{
			ServerParams* params;
			bool failed;
		} context{&params, false};
		const NzbgetRsJsonLimits limits{0x7ffffffeu, 0x7fffffffu / 24u, 0x3fffffffu};
		int parsed = nzbget_rs_test_server_params(m_request, limits, &context,
			[](void* ctx, const NzbgetRsTestServerParams* from) noexcept
			{
				Context& context = *static_cast<Context*>(ctx);
				try
				{
					ServerParams p;
					p.host.assign(from->host.data, from->host.len);
					p.port = from->port;
					p.username.assign(from->username.data, from->username.len);
					p.password.assign(from->password.data, from->password.len);
					p.encryption = from->encryption != 0;
					p.cipher.assign(from->cipher.data, from->cipher.len);
					p.timeout = from->timeout;
					p.certVerifLevel = from->certVerifLevel;
					*context.params = std::move(p);
				}
				catch (const std::exception&)
				{
					context.failed = true;
				}
			});
		if (parsed == 0)
		{
			BuildErrorResponse(2, "Invalid JSON");
			return;
		}
		if (parsed < 0 || context.failed)
		{
			BuildErrorResponse(2, "Invalid parameters");
			return;
		}
	}
	else if (!NextParamAsStr(&host) || !NextParamAsInt(&port) || !NextParamAsStr(&username) ||
		!NextParamAsStr(&password) || !NextParamAsBool(&encryption) ||
		!NextParamAsStr(&cipher) || !NextParamAsInt(&timeout) ||
		!NextParamAsInt(&certVerifLevel))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}
	else
	{
		DecodeStr(host);
		DecodeStr(username);
		DecodeStr(password);
		DecodeStr(cipher);
		params.host = host;
		params.port = port;
		params.username = username;
		params.password = password;
		params.encryption = encryption;
		params.cipher = cipher;
		params.timeout = timeout;
		params.certVerifLevel = certVerifLevel;
	}

	if (!nzbget_rs_dqh_xmlrpc_is_valid_cert_verif_level(
		params.certVerifLevel, (int)Options::ECertVerifLevel::Count))
	{
		BuildErrorResponse(2, "Invalid parameter (Certificate Verification Level).");
		return;
	}

	NewsServer server(
		0,
		true,
		"test server",
		params.host.c_str(),
		params.port,
		0,
		params.username.c_str(),
		params.password.c_str(),
		false,
		params.encryption,
		params.cipher.c_str(),
		1,
		0,
		0,
		0,
		false,
		params.certVerifLevel
	);
	TestConnection connection(&server, this);
	connection.SetTimeout(nzbget_rs_rol_xmlrpc_test_server_effective_timeout(
		params.timeout, g_Options->GetArticleTimeout()));
	connection.SetSuppressErrors(false);

	bool ok = connection.Connect();
	if (ok)
	{
		// generate a unique non-existent message-id since we don't want a real article to be returned
		BString<1024> id;
		while (id.Length() < 30)
		{
			NzbgetRsBuf rBuf = nzbget_rs_dqh_conn_format_port_str(rand());
			id.Append(rBuf.data ? rBuf.data : "", (int)rBuf.len);
			nzbget_rs_free(rBuf);
		}
		NzbgetRsBuf cmdBuf = nzbget_rs_tpr_xmlrpc_format_testserver_article_cmd(*id);
		const char* response = connection.Request(cmdBuf.data ? cmdBuf.data : "");
		nzbget_rs_free(cmdBuf);
		ok = nzbget_rs_rol_xmlrpc_test_server_response_ok(response) != 0;
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(
		IsJson() ? 1 : 0,
		nzbget_rs_dqh_xmlrpc_select_test_server_err_text(ok ? 1 : 0, *m_errText),
		0,
		1023));
}

inline void TestServerXmlCommand::PrintError(const char* errMsg)
{
	if (m_errText.Empty())
	{
		m_errText = EncodeStr(errMsg);
	}
}

inline void TestServerSpeedXmlCommand::Execute()
{
	char* nzbFileUrl;
	if (!NextParamAsStr(&nzbFileUrl))
	{
		BuildErrorResponse(2, "Invalid parameter: NZB file url");
		return;
	}
	DecodeStr(nzbFileUrl);

	int serverId;
	if (!NextParamAsInt(&serverId))
	{
		BuildErrorResponse(2, "Invalid parameter: Server ID");
		return;
	}

	NewsServer* server = g_ServerPool->GetServerById(serverId);
	int serverErr = nzbget_rs_roc_xmlrpc_test_server_speed_check_server(
		server != nullptr ? 1 : 0,
		server && server->GetActive() ? 1 : 0,
		server ? server->GetMaxConnections() : 0);
	if (serverErr != 0)
	{
		BuildErrorResponse(2, nzbget_rs_dqh_xmlrpc_test_server_speed_err_msg(serverErr));
		return;
	}

	std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
	nzbInfo->SetKind(NzbInfo::nkUrl);
	nzbInfo->SetUrl(nzbFileUrl);
	nzbInfo->SetFilename(nzbFileUrl);
	nzbInfo->SetPriority(NzbInfo::FORCE_PRIORITY);
	nzbInfo->SetExtraPriority(NzbInfo::FORCE_PRIORITY);
	nzbInfo->SetAddUrlPaused(false);
	nzbInfo->SetDupeMode(EDupeMode::dmForce);
	nzbInfo->SetDesiredServerId(serverId);
	nzbInfo->SetSkipScriptProcessing(true);
	nzbInfo->SetSkipDiskWrite(true);

	g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), true);

	BuildBoolResponse(true);
}

inline void TestDiskSpeedXmlCommand::Execute()
{
	NzbgetRsBuf dirPathBuf{nullptr, 0};
	size_t bufferSizeBytes = 0;
	uint64_t maxFileSizeBytes = 0;
	int timeoutSec = 0;
	const char* errMsg = nullptr;
	if (!nzbget_rs_xmlrpc_parse_test_disk_speed(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		&dirPathBuf,
		&bufferSizeBytes,
		&maxFileSizeBytes,
		&timeoutSec,
		&errMsg))
	{
		nzbget_rs_free(dirPathBuf);
		BuildErrorResponse(2, "%s", errMsg ? errMsg : "Invalid parameter");
		return;
	}
	CString dirPath(dirPathBuf.data ? dirPathBuf.data : "");
	nzbget_rs_free(dirPathBuf);

	PauseGuard pauseGuard;

	std::string dir(*dirPath ? *dirPath : "");
	fs::error_code ec;
	const fs::path fsDirPath = fs::u8path(dir);
	fs::space_info space = fs::space(fsDirPath, ec);
	std::string spaceErr = ec ? ec.message() : std::string();

	uint64_t writeBytes = 0;
	double writeMs = 0.0;
	uint64_t readBytes = 0;
	double readMs = 0.0;
	NzbgetRsBuf errBuf{nullptr, 0};
	int rc = nzbget_rs_benchmark_run(
		dir.data(), dir.size(),
		bufferSizeBytes,
		maxFileSizeBytes,
		timeoutSec,
		ec ? 0 : 1,
		space.available,
		spaceErr.data(), spaceErr.size(),
		&writeBytes, &writeMs, &readBytes, &readMs,
		&errBuf);
	if (rc != 0)
	{
		std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
		nzbget_rs_free(errBuf);
		warn("Disk speed test for %s failed: %s", *dirPath, err.c_str());
		BuildErrorResponse(2, "%s", err.c_str());
		return;
	}
	nzbget_rs_free(errBuf);

	info("Disk speed test for %s: write %.1f MiB/s, read %.1f MiB/s",
		*dirPath, MiBPerSec(writeBytes, writeMs), MiBPerSec(readBytes, readMs));

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_disk_speed(
		IsJson() ? 1 : 0, writeBytes, writeMs, readBytes, readMs));
}

inline void TestNetworkSpeedXmlCommand::Execute()
{
	// a pause that was on before the test stays on
	PauseGuard pauseGuard;
	auto executeIterCb = [](
		void*,
		const char* reqPtr,
		size_t reqLen,
		int64_t timeoutSec,
		uint32_t* outDownloaded,
		double* outElapsed) -> int
	{
		using namespace std::chrono;
#ifdef DISABLE_TLS
		Connection connection("speed.cloudflare.com", 80, false);
#else
		Connection connection("speed.cloudflare.com", 443, true);
#endif
		connection.SetTimeout(1);
		if (!connection.Connect() || !connection.Send(reqPtr, reqLen))
		{
			if (outDownloaded) *outDownloaded = 0;
			if (outElapsed) *outElapsed = 0.0;
			return 0;
		}
		uint32_t totalReceived = 0;
		char buffer[8 * 1024];
		auto start = steady_clock::now();
		while (int received = connection.TryRecv(buffer, sizeof(buffer)))
		{
			if (nzbget_rs_dqh_speed_test_should_stop_recv_loop(
				received,
				duration<double>(steady_clock::now() - start).count(),
				static_cast<double>(timeoutSec)) != 0)
			{
				break;
			}
			totalReceived += static_cast<uint32_t>(received);
		}
		auto finish = steady_clock::now();
		if (outDownloaded) *outDownloaded = totalReceived;
		if (outElapsed) *outElapsed = duration<double>(finish - start).count();
		return 1;
	};
	auto logCb = [](void*, int kind, const char* msg)
	{
		if (!msg) return;
		if (kind == 1)
		{
			error("%s", msg);
		}
		else
		{
			info("%s", msg);
		}
	};
	double speedMbps = 0.0;
	NzbgetRsBuf errBuf{nullptr, 0};
	int ok = nzbget_rs_network_speed_test_run(nullptr, executeIterCb, logCb, &speedMbps, &errBuf);
	if (!ok)
	{
		std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
		nzbget_rs_free(errBuf);
		BuildErrorResponse(2, "%s", err.c_str());
		return;
	}
	nzbget_rs_free(errBuf);
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_network_speed(
		IsJson() ? 1 : 0, speedMbps));
}

// bool startscript(string script, string command, string context, struct[] options);
inline void StartScriptXmlCommand::Execute()
{
	NzbgetRsBuf scriptBuf{nullptr, 0};
	NzbgetRsBuf commandBuf{nullptr, 0};
	NzbgetRsBuf contextBuf{nullptr, 0};
	std::unique_ptr<Options::OptEntries> optEntries = std::make_unique<Options::OptEntries>();
	if (!nzbget_rs_xmlrpc_parse_start_script(
		&m_requestPtr,
		m_httpMethod == XmlRpcProcessor::hmGet ? 1 : 0,
		IsJson() ? 1 : 0,
		&scriptBuf,
		&commandBuf,
		&contextBuf,
		optEntries.get(),
		[](void* ctx, const char* name, const char* val)
		{
			static_cast<Options::OptEntries*>(ctx)->emplace_back(name, val);
		}))
	{
		nzbget_rs_free(scriptBuf);
		nzbget_rs_free(commandBuf);
		nzbget_rs_free(contextBuf);
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}
	nzbget_rs_free(contextBuf);

	bool ok = CommandScriptController::StartScript(
		scriptBuf.data ? scriptBuf.data : "",
		commandBuf.data ? commandBuf.data : "",
		std::move(optEntries));
	nzbget_rs_free(scriptBuf);
	nzbget_rs_free(commandBuf);

	BuildBoolResponse(ok);
}

// struct[] logscript(idfrom, entries)
inline GuardedMessageList LogScriptXmlCommand::GuardMessages()
{
	return g_CommandScriptLog->GuardMessages();
}


#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "Util.h"

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

#include "Util.h"
#include "Util.h"
#include "QueueCoordinator.h"
#include "Util.h"
#include "Util.h"
#include "DiskState.h"
#include "QueueCoordinator.h"
#include "ServerPool.h"
#include "QueueCoordinator.h"
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
