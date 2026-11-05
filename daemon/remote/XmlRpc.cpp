/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2023-2026 Denis <denis@nzbget.com>
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


#include "nzbget.h"
#include "XmlRpc.h"

#include "nzbget_rs.h"
#include "Fleet.h"
#include "Log.h"
#include "Options.h"
#include "WorkState.h"
#include "Scanner.h"
#include "FeedCoordinator.h"
#include "ServerPool.h"
#include "Util.h"
#include <cerrno>
#include <climits>
#include "FileSystem.h"
#include "Maintenance.h"
#include "StatMeter.h"
#include "ArticleWriter.h"
#include "DiskState.h"
#include "ScriptConfig.h"
#include "QueueScript.h"
#include "CommandScript.h"
#include "UrlCoordinator.h"
#include "ExtensionManager.h"
#include "SystemInfo.h"
#include "Benchmark.h"
#include "NetworkSpeedTest.h"
#include "Xml.h"
#include "Unpack.h"
#include "SystemHealth.h"
#include "TlsCipherBackend.h"

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
		if (!nzbget_rs_roc_xmlrpc_tls_cipher_access_allowed(
			(int)m_userAccess, (int)XmlRpcProcessor::uaControl,
			(int)m_protocol, (int)XmlRpcProcessor::rpJsonRpc,
			(int)m_httpMethod, (int)XmlRpcProcessor::hmPost))
		{
			BuildErrorResponse(401, "TLS cipher optimization requires control-account JSON-RPC POST access"); return;
		}
		auto request = m_request ? Json::Deserialize(m_request) : std::nullopt;
		if (!request || !request->is_object() || !request->as_object().contains("params") || !request->as_object().at("params").is_array())
		{
			BuildErrorResponse(2, "Invalid cipher optimizer parameters"); return;
		}
		const auto& params = request->as_object().at("params").as_array();
		std::string id;
		bool firstIsStr = !params.empty() && params[0].is_string();
		const char* firstPtr = firstIsStr ? params[0].as_string().data() : nullptr;
		size_t firstLen = firstIsStr ? params[0].as_string().size() : 0;
		if (!nzbget_rs_roc_xmlrpc_tls_cipher_validate_job_id(
			m_operation, params.size(), firstIsStr ? 1 : 0, firstPtr, firstLen))
		{
			BuildErrorResponse(2, "Invalid cipher job ID"); return;
		}
		if (!params.empty()) id = Json::value_to<std::string>(params[0]);
		std::string response = m_operation == 0 ? TlsCipherBackend::Start() :
			m_operation == 1 ? TlsCipherBackend::Status(id.empty() ? nullptr : id.c_str()) : TlsCipherBackend::Cancel(id.c_str());
		auto parsed = Json::Deserialize(response);
		if (!parsed || !parsed->is_object()) { BuildErrorResponse(3, "Invalid TLS optimizer response"); return; }
		const auto& object = parsed->as_object();
		if (object.contains("Error") && object.at("Error").is_string())
			BuildErrorResponse(3, "%s", Json::value_to<std::string>(object.at("Error")).c_str());
		else AppendResponse(response.c_str());
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

void XmlRpcProcessor::SetUrl(const char* url)
{
	// the path is decoded here, each query parameter after it is split off: decoded
	// first, an encoded "&" (%26) inside a value split that value in two
	size_t pathSetLen = 0;
	size_t queryOffset = 0;
	bool hasQuery = nzbget_rs_tpr_xmlrpc_split_url_path_and_query(url, &pathSetLen, &queryOffset) != 0;
	CString path;
	path.Set(url, (int)pathSetLen);
	WebUtil::UrlDecode(path);
	m_url = CString::FormatStr("%s%s", *path, hasQuery ? url + queryOffset : "");
}

bool XmlRpcProcessor::IsRpcRequest(const char* url)
{
	return nzbget_rs_rco_rpc_is_rpc_request(url) != 0;
}

void XmlRpcProcessor::Execute()
{
	m_protocol = (ERpcProtocol)nzbget_rs_rpc_protocol(m_url);
	if (m_protocol == rpUndefined)
	{
		error("internal error: invalid rpc-request: %s", *m_url);
		return;
	}

	Dispatch();
}

void XmlRpcProcessor::Dispatch()
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

XmlRpcProcessor::EUserAccess XmlRpcProcessor::Access(ERpcProtocol protocol, EUserAccess userAccess)
{
	// rust/src/rpcroute.rs
	return (EUserAccess)nzbget_rs_rpc_access(protocol, userAccess);
}

XmlRpcProcessor::EDispatch XmlRpcProcessor::Decide(ERpcProtocol protocol, EHttpMethod httpMethod,
	bool safeMethod, bool error, bool jsonP, int* errCode, const char** errText)
{
	// rust/src/rpcroute.rs
	return (EDispatch)nzbget_rs_rpc_decide(protocol, httpMethod, safeMethod, error, jsonP, errCode, errText);
}

void XmlRpcProcessor::MutliCall()
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

void XmlRpcProcessor::BuildResponse(const char* response, const char* callbackFunc,
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

	m_contentType = xmlRpc ? "text/xml" : "application/json";
}

void XmlRpcProcessor::BuildErrorResponse(int errCode, const char* errText)
{
	ErrorXmlCommand command(errCode, errText);
	command.SetProtocol(m_protocol);
	command.Execute();
	BuildResponse(command.GetResponse(), "", command.GetFault(), nullptr);
}

// a JSON-P refusal goes through the caller's callback, so the page that loaded it
// sees it
void XmlRpcProcessor::BuildJsonPRefusal(XmlCommand* command, int errCode, const char* errText,
	const char* requestId)
{
	ErrorXmlCommand refusal(errCode, errText);
	refusal.SetProtocol(m_protocol);
	refusal.Execute();
	BuildResponse(refusal.GetResponse(), command->GetCallbackFunc(), refusal.GetFault(), requestId);
}

std::unique_ptr<XmlCommand> XmlRpcProcessor::CreateCommand(const char* methodName)
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

XmlCommand::XmlCommand()
	: m_protocol(XmlRpcProcessor::rpUndefined)
	, m_httpMethod(XmlRpcProcessor::hmPost)
	, m_userAccess(XmlRpcProcessor::uaControl)
{
	m_response.Reserve(1024 * 10 - 1);
}

XmlCommand::~XmlCommand() = default;
SafeXmlCommand::~SafeXmlCommand() = default;
LogXmlCommand::~LogXmlCommand() = default;
NzbInfoXmlCommand::~NzbInfoXmlCommand() = default;

bool XmlCommand::IsJson()
{
	return m_protocol == XmlRpcProcessor::rpJsonRpc || m_protocol == XmlRpcProcessor::rpJsonPRpc;
}

void XmlCommand::AppendResponse(const char* part)
{
	m_response.Append(part);
}

void XmlCommand::AppendFmtResponse(const char* format, ...)
{
	va_list args;
	va_start(args, format);
	m_response.AppendFmtV(format, args);
	va_end(args);
}

void XmlCommand::AppendCondResponse(const char* part, bool cond)
{
	if (cond)
	{
		m_response.Append(part);
	}
}

void XmlCommand::BuildErrorResponse(int errCode, const char* errText, ...)
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

void XmlCommand::BuildBoolResponse(bool ok)
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_bool(IsJson() ? 1 : 0, ok ? 1 : 0));
}

void XmlCommand::BuildIntResponse(int value)
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_int(IsJson() ? 1 : 0, value));
}

void XmlCommand::PrepareParams()
{
	if (IsJson() && m_httpMethod == XmlRpcProcessor::hmPost)
	{
		// without "params" the request is emptied: no parameters follow
		m_requestPtr = nzbget_rs_rpc_skip_to_params(m_requestPtr);
	}

	if (m_protocol == XmlRpcProcessor::rpJsonPRpc)
	{
		NextParamAsStr(&m_callbackFunc);
	}
}

bool XmlCommand::NextParamAsInt(int* value)
{
	return nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 0, value, nullptr);
}

bool XmlCommand::NextParamAsBool(bool* value)
{
	int intValue;
	if (!nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 1, &intValue, nullptr))
	{
		return false;
	}
	*value = intValue != 0;
	return true;
}

bool XmlCommand::NextParamAsStr(char** value)
{
	return nzbget_rs_rpc_next_param(&m_requestPtr, m_httpMethod == XmlRpcProcessor::hmGet, IsJson(), 2, nullptr, value);
}

CString XmlCommand::EncodeStr(const char* str)
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

void XmlCommand::DecodeStr(char* str)
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

void ErrorXmlCommand::Execute()
{
	BuildErrorResponse(m_errCode, m_errText);
}

void PauseUnpauseXmlCommand::Execute()
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
void ScheduleResumeXmlCommand::Execute()
{
	int seconds = 0;
	if (!NextParamAsInt(&seconds) || seconds < 0)
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	time_t curTime = Util::CurrentTime();

	g_WorkState->SetResumeTime(curTime + seconds);

	BuildBoolResponse(true);
}

void ShutdownXmlCommand::Execute()
{
	BuildBoolResponse(true);
	ExitProc();
}

void ReloadXmlCommand::Execute()
{
	BuildBoolResponse(true);
	Reload();
}

void VersionXmlCommand::Execute()
{
	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(IsJson() ? 1 : 0, Util::VersionRevision(), 0, 1023));
}

void DumpDebugXmlCommand::Execute()
{
	g_Log->LogDebugInfo();
	BuildBoolResponse(true);
}

void SetDownloadRateXmlCommand::Execute()
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

void StatusXmlCommand::Execute()
{
	int postJobCount = 0;
	int urlCount = 0;
	int64 remainingSize, forcedSize;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			postJobCount += nzbInfo->GetPostInfo() ? 1 : 0;
			urlCount += nzbInfo->GetKind() == NzbInfo::nkUrl ? 1 : 0;
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

void SysInfoXmlCommand::Execute()
{
	std::string response = IsJson()
		? System::ToJsonStr(*g_SystemInfo)
		: System::ToXmlStr(*g_SystemInfo);

	AppendResponse(response.c_str());
}

void SystemHealthXmlCommand::Execute()
{
	if (g_Options->GetSystemHealthCheck())
	{
		const auto report = g_SystemHealth->Diagnose();
		const std::string response =
			IsJson() ? SystemHealth::ToJsonStr(report) : SystemHealth::ToXmlStr(report);

		AppendResponse(response.c_str());	
	}
	else
	{
		BuildErrorResponse(1, "System health check is disabled in configuration");
	}
}

// struct[] log(idfrom, entries)
void LogXmlCommand::Execute()
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

GuardedMessageList LogXmlCommand::GuardMessages()
{
	return g_Log->GuardMessages();
}

// struct[] listfiles(int IDFrom, int IDTo, int NZBID)
// For backward compatibility with 0.8 parameter "NZBID" is optional
void ListFilesXmlCommand::Execute()
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

void NzbInfoXmlCommand::AppendNzbInfoFields(NzbInfo* nzbInfo)
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

void NzbInfoXmlCommand::AppendPostInfoFields(PostInfo* postInfo, int logEntries, bool postQueue)
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
void ListGroupsXmlCommand::Execute()
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

const char* ListGroupsXmlCommand::DetectStatus(NzbInfo* nzbInfo)
{
	bool hasPostInfo = nzbInfo->GetPostInfo() != nullptr;
	int postStage = hasPostInfo ? (int)nzbInfo->GetPostInfo()->GetStage() : 0;
	bool queueScriptActive = false;
	bool hasQueueScriptJob = hasPostInfo && postStage == (int)PostInfo::ptQueued &&
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
void EditQueueXmlCommand::Execute()
{
	char* editCommand;
	if (!NextParamAsStr(&editCommand))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}
	debug("EditCommand=%s", editCommand);

	int action = nzbget_rs_rco_rpc_lookup_edit_command(editCommand);

	if (action == -1)
	{
		BuildErrorResponse(3, "Invalid action");
		return;
	}

	int offset = 0;
	bool hasOffset = NextParamAsInt(&offset);

	char* args;
	if (!NextParamAsStr(&args))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}
	debug("Args=%s", args);

	DecodeStr(args);

	// a number past the int range wrapped around (F27: a score of 2147483648 was
	// stored as -2147483648, the lowest), a parameter needs a name and a value,
	// and a dupe mode is one of three (the edit refused others, but the call said true)
	if (!nzbget_rs_rco_rpc_validate_edit_queue_args(action, args, 1))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	NzbgetRsBuf offsetStrBuf = nzbget_rs_dqh_conn_format_port_str(offset);
	BString<100> offsetStr;
	offsetStr.Set(offsetStrBuf.data ? offsetStrBuf.data : "", (int)offsetStrBuf.len);
	nzbget_rs_free(offsetStrBuf);
	if (nzbget_rs_tpr_xmlrpc_should_use_edit_offset(
		hasOffset ? 1 : 0, action,
		(int)DownloadQueue::eaFileMoveOffset,
		(int)DownloadQueue::eaGroupMoveOffset) != 0)
	{
		args = *offsetStr;
	}

	IdList idList;
	int id = 0;
	while (NextParamAsInt(&id))
	{
		idList.push_back(id);
	}

	bool ok = DownloadQueue::Guard()->EditList(&idList, nullptr, DownloadQueue::mmId,
		(DownloadQueue::EEditAction)action, args);

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
void AppendFleetXmlCommand::Execute()
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
		if ((int)request.members.size() >= Fleet::MaxMembers)
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

void DownloadXmlCommand::Execute()
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
		if (Unpack::IsArchive(filename))
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

			if (v13)
			{
				BuildIntResponse(nzbId);
			}
			else
			{
				BuildBoolResponse(nzbId > 0);
			}
		}
	}
}

// deprecated
void PostQueueXmlCommand::Execute()
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

void WriteLogXmlCommand::Execute()
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

void ClearLogXmlCommand::Execute()
{
	g_Log->Clear();

	BuildBoolResponse(true);
}

void ScanXmlCommand::Execute()
{
	bool syncMode = false;
	// optional parameter "SyncMode"
	NextParamAsBool(&syncMode);

	g_Scanner->ScanNzbDir(syncMode);
	BuildBoolResponse(true);
}

// struct[] history(bool hidden)
// Parameter "hidden" is optional (new in v12)
void HistoryXmlCommand::Execute()
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

		if (historyInfo->GetKind() == HistoryInfo::hkNzb ||
			historyInfo->GetKind() == HistoryInfo::hkUrl)
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
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
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

const char* HistoryXmlCommand::DetectStatus(HistoryInfo* historyInfo)
{
	const char* status = "FAILURE/INTERNAL_ERROR";

	if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		status = nzbInfo->MakeTextStatus(false);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		DupInfo* dupInfo = historyInfo->GetDupInfo();
		status = nzbget_rs_rco_rpc_detect_history_dup_status((int)dupInfo->GetStatus());
	}

	return status;
}

// Deprecated in v13
void UrlQueueXmlCommand::Execute()
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
void ConfigXmlCommand::Execute()
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
void LoadConfigXmlCommand::Execute()
{
	Options::OptEntries optEntries;
	if (!g_ScriptConfig->LoadConfig(&optEntries))
	{
		BuildErrorResponse(3, "Could not read configuration file");
		return;
	}

	std::vector<NzbgetRsRpcConfigItem> entries;
	entries.reserve(optEntries.size());
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

void LoadExtensionsXmlCommand::Execute()
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

			AppendCondResponse(",\n", isJson && index++ > 0);
			AppendResponse(response.c_str());
		}
	);

	AppendResponse(isJson ? "\n]" : "</data></array>\n");
}

void DownloadExtensionXmlCommand::Execute()
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

void UpdateExtensionXmlCommand::Execute()
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

void DeleteExtensionXmlCommand::Execute()
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

void TestExtensionXmlCommand::Execute()
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
void SaveConfigXmlCommand::Execute()
{
	Options::OptEntries optEntries;
	std::string cipherBaseline;
	bool hasCipherBaseline = false;

	// The file is rewritten from the entries given: options left out are dropped.
	// An entry that doesn't read as a name and a string value (a number, or "Value"
	// before "Name") ended the list where it stood, and every option after it - all
	// of them for an empty list - was deleted, the call reporting success.
	char* name;
	char* value;
	bool malformed = false;
	while (true)
	{
		if (IsJson())
		{
			char* key1;
			char* key2;
			if (!NextParamAsStr(&key1))
			{
				break;
			}
			if (!NextParamAsStr(&name) || !NextParamAsStr(&key2) || !NextParamAsStr(&value))
			{
				malformed = true;
				break;
			}
			int keyKind = nzbget_rs_rol_xmlrpc_save_config_check_json_keys(key1, key2);
			if (keyKind == 2)
			{
				std::swap(name, value);
			}
			else if (keyKind != 1)
			{
				malformed = true;
				break;
			}
		}
		else
		{
			if (!NextParamAsStr(&name))
			{
				break;
			}
			if (!NextParamAsStr(&value))
			{
				malformed = true;
				break;
			}
		}
		DecodeStr(name);
		bool baselineEntry = nzbget_rs_dqh_xmlrpc_is_tls_cipher_baseline_entry(name) != 0;
		// The token contains only ASCII field names and hex-encoded UTF-8.
		// Reject encoded NUL rather than accepting a decoder-truncated token.
		if (baselineEntry && !nzbget_rs_rol_xmlrpc_save_config_raw_baseline_valid(value))
		{
			malformed = true;
			break;
		}
		DecodeStr(value);
		if (baselineEntry)
		{
			if (!nzbget_rs_rol_xmlrpc_save_config_decoded_baseline_valid(
				hasCipherBaseline ? 1 : 0, value))
			{
				malformed = true;
				break;
			}
			hasCipherBaseline = true;
			cipherBaseline = value;
			continue; // RPC metadata is never a configuration option.
		}
		optEntries.emplace_back(name, value);
	}

	if (malformed || optEntries.empty())
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}

	// Configuration Conflict Validation (R7)
	// Rule 7.1: ServerX.Encryption=no with ServerX.Cipher configured
	std::vector<const char*> optNames;
	std::vector<const char*> optValues;
	optNames.reserve(optEntries.size());
	optValues.reserve(optEntries.size());
	for (const auto& entry : optEntries)
	{
		optNames.push_back(entry.GetName() ? entry.GetName() : "");
		optValues.push_back(entry.GetValue() ? entry.GetValue() : "");
	}
	int cipherConflictServer = nzbget_rs_rco_rpc_save_config_find_cipher_conflict(
		optNames.data(), optValues.data(), optNames.size());
	if (cipherConflictServer > 0)
	{
		BuildErrorResponse(3, "Configuration conflict: Server%i.Cipher cannot be configured when Server%i.Encryption is disabled. Clear the cipher suite or enable encryption.", cipherConflictServer, cipherConflictServer);
		return;
	}

	auto optVal = [&optEntries](const char* name) -> const char*
	{
		if (Options::OptEntry* entry = optEntries.FindOption(name))
		{
			return entry->GetValue() ? entry->GetValue() : "";
		}
		return nullptr;
	};
	int conflictRule = nzbget_rs_rco_rpc_save_config_check_global_conflicts(
		optVal("DupeSearch"), optVal("HealthCheck"),
		optVal("FlushQueue"), optVal("DirectWrite"),
		optVal("UnpackPauseQueue"), optVal("DirectUnpack"));
	if (conflictRule == 2)
	{
		BuildErrorResponse(3, "Configuration conflict: 'DupeSearch=yes' is incompatible with 'HealthCheck=park'. DupeSearch requires 'HealthCheck=dupe' to fetch alternative donor releases when an NZB fails.");
		return;
	}
	if (conflictRule == 3)
	{
		BuildErrorResponse(3, "Configuration conflict: 'DirectWrite=yes' requires 'FlushQueue=yes'. Disabling queue flushing during direct disk writes causes disk state desynchronization and corruption upon unexpected shutdown.");
		return;
	}
	if (conflictRule == 4)
	{
		BuildErrorResponse(3, "Configuration conflict: 'DirectUnpack=yes' requires 'UnpackPauseQueue=yes'. Disabling queue pause during direct unpack causes concurrent unpack disk thrashing.");
		return;
	}

	// save to config file
	bool conflict = false;
	bool ok = g_ScriptConfig->SaveConfig(&optEntries, hasCipherBaseline ? cipherBaseline.c_str() : nullptr, &conflict);
	if (conflict)
	{
		BuildErrorResponse(3, "News server settings changed in another operation. Refresh Settings before saving; your edits have been retained.");
		return;
	}

	BuildBoolResponse(ok);
}

// struct[] configtemplates(bool loadFromDisk)
// parameter "loadFromDisk" is optional (new in v14)
void ConfigTemplatesXmlCommand::Execute()
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
void ViewFeedXmlCommand::Execute()
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
void FetchFeedXmlCommand::Execute()
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
void EditServerXmlCommand::Execute()
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
			if (server->GetId() == id)
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
void ReadUrlXmlCommand::Execute()
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
void CheckUpdatesXmlCommand::Execute()
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
void StartUpdateXmlCommand::Execute()
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
GuardedMessageList LogUpdateXmlCommand::GuardMessages()
{
	return g_Maintenance->GuardMessages();
}

// struct[] servervolumes(bool BytesPerSeconds, bool BytesPerMinutes, bool BytesPerHours, bool BytesPerDays, bool ArticlesPerDays)
void ServerVolumesXmlCommand::Execute()
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
void ResetServerVolumeXmlCommand::Execute()
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
		if (index == serverId || serverId == -1)
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
void LoadLogXmlCommand::Execute()
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

GuardedMessageList LoadLogXmlCommand::GuardMessages()
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
void TestServerXmlCommand::Execute()
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
	if (IsJson() && m_request && m_httpMethod == XmlRpcProcessor::hmPost)
	{
		// rust/src/rpcparams.rs; a std::exception while copying the strings
		// gives "Invalid parameters", as in ParseRequestParams
		struct Context
		{
			ServerParams* params;
			bool failed;
		} context{&params, false};
		const NzbgetRsJsonLimits limits{Json::string::max_size(), Json::JsonArray::max_size(), Json::JsonObject::max_size()};
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

	if (params.certVerifLevel < 0 || params.certVerifLevel >= Options::ECertVerifLevel::Count)
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
			id.AppendFmt("%i", rand());
		}
		NzbgetRsBuf cmdBuf = nzbget_rs_tpr_xmlrpc_format_testserver_article_cmd(*id);
		const char* response = connection.Request(cmdBuf.data ? cmdBuf.data : "");
		nzbget_rs_free(cmdBuf);
		ok = nzbget_rs_rol_xmlrpc_test_server_response_ok(response) != 0;
	}

	AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_string(
		IsJson() ? 1 : 0,
		ok ? "" : m_errText.Empty() ? "Unknown error" : *m_errText,
		0,
		1023));
}

void TestServerXmlCommand::PrintError(const char* errMsg)
{
	if (m_errText.Empty())
	{
		m_errText = EncodeStr(errMsg);
	}
}

void TestServerSpeedXmlCommand::Execute()
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
		BuildErrorResponse(2, serverErr == 1 ? "Server not found" :
			serverErr == 2 ? "Server is not active" : "Server has no connections configured");
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

void TestDiskSpeedXmlCommand::Execute()
{
	char* dirPath;
	int writeBufferKiB;
	int maxFileSizeGiB;
	int timeoutSec;

	if (!NextParamAsStr(&dirPath))
	{
		BuildErrorResponse(2, "Invalid argument (Path)");
		return;
	}
	DecodeStr(dirPath);

	if (!NextParamAsInt(&writeBufferKiB))
	{
		BuildErrorResponse(2, "Invalid argument (Write Buffer)");
		return;
	}

	if (writeBufferKiB < 0)
	{
		BuildErrorResponse(2, "Write Buffer cannot be negative");
		return;
	}

	if (!NextParamAsInt(&maxFileSizeGiB))
	{
		BuildErrorResponse(2, "Invalid argument (Max file size)");
		return;
	}

	if (maxFileSizeGiB < 1)
	{
		BuildErrorResponse(2, "Max file size must be positive");
		return;
	}

	if (!NextParamAsInt(&timeoutSec))
	{
		BuildErrorResponse(2, "Invalid argument (Timeout)");
		return;
	}

	if (timeoutSec < 1)
	{
		BuildErrorResponse(2, "Timeout must be positive");
		return;
	}

	try
	{
		size_t bufferSizeBytes = 0;
		uint64_t maxFileSizeBytes = 0;
		if (nzbget_rs_roc_xmlrpc_test_disk_speed_validate_params(
			writeBufferKiB, maxFileSizeGiB, timeoutSec, &bufferSizeBytes, &maxFileSizeBytes) != 0)
		{
			BuildErrorResponse(2, "Invalid parameter");
			return;
		}

		PauseGuard pauseGuard;

		Benchmark::DiskBenchmark db;
		Benchmark::Result res = db.Run(
			dirPath,
			bufferSizeBytes,
			maxFileSizeBytes,
			std::chrono::seconds(timeoutSec)
		);

		info("Disk speed test for %s: write %.1f MiB/s, read %.1f MiB/s",
			dirPath, MiBPerSec(res.writeBytes, res.writeMs), MiBPerSec(res.readBytes, res.readMs));

		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_disk_speed(
			IsJson() ? 1 : 0, res.writeBytes, res.writeMs, res.readBytes, res.readMs));
	}
	catch (const std::exception& e)
	{
		warn("Disk speed test for %s failed: %s", dirPath, e.what());
		BuildErrorResponse(2, "%s", e.what());
	}
}

void TestNetworkSpeedXmlCommand::Execute()
{
	// a pause that was on before the test stays on
	PauseGuard pauseGuard;
	try
	{
		Network::SpeedTest sp;
		double speedMbps = sp.RunTest();
		AppendRsBuf(m_response, nzbget_rs_xmlrpc_format_network_speed(
			IsJson() ? 1 : 0, speedMbps));
	}
	catch (const std::exception& e)
	{
		BuildErrorResponse(2, "%s", e.what());
	}
}

// bool startscript(string script, string command, string context, struct[] options);
void StartScriptXmlCommand::Execute()
{
	char* script;
	char* command;
	char* context;
	if (!NextParamAsStr(&script) || !NextParamAsStr(&command) || !NextParamAsStr(&context))
	{
		BuildErrorResponse(2, "Invalid parameter");
		return;
	}
	DecodeStr(script);
	DecodeStr(command);
	DecodeStr(context);

	std::unique_ptr<Options::OptEntries> optEntries = std::make_unique<Options::OptEntries>();

	char* name;
	char* value;
	char* dummy;
	while ((IsJson() && NextParamAsStr(&dummy) && NextParamAsStr(&name) &&
		NextParamAsStr(&dummy) && NextParamAsStr(&value)) ||
		(!IsJson() && NextParamAsStr(&name) && NextParamAsStr(&value)))
	{
		DecodeStr(name);
		DecodeStr(value);
		optEntries->emplace_back(name, value);
	}

	bool ok = CommandScriptController::StartScript(script, command, std::move(optEntries));

	BuildBoolResponse(ok);
}

// struct[] logscript(idfrom, entries)
GuardedMessageList LogScriptXmlCommand::GuardMessages()
{
	return g_CommandScriptLog->GuardMessages();
}
