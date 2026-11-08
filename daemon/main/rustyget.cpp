/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "TlsCipherBackend.h"
#include "nzbget_rs.h"

#include "ServerPool.h"
#include "Log.h"
#include "NzbFile.h"
#include "Options.h"
#include "WorkState.h"
#include "ScriptConfig.h"
#include "Thread.h"
#include "Frontend.h"
#include "NCursesFrontend.h"
#include "QueueCoordinator.h"
#include "UrlCoordinator.h"
#include "DupeSearch.h"
#include "WebServer.h"
#include "RemoteClient.h"
#include "MessageBase.h"
#include "DiskState.h"
#include "PrePostProcessor.h"
#include "HistoryCoordinator.h"
#include "TwinCheck.h"
#include "DupeCoordinator.h"
#include "Scheduler.h"
#include "Scanner.h"
#include "FeedCoordinator.h"
#include "Service.h"
#include "Maintenance.h"
#include "ArticleWriter.h"
#include "StatMeter.h"
#include "QueueScript.h"
#include "Util.h"
#include "FileSystem.h"
#include "CommandScript.h"
#include "ExtensionManager.h"
#include "SystemInfo.h"
#include "SystemHealth.h"

#ifndef DISABLE_NSERV
#include "NServMain.h"
#endif

#ifdef DEBUG
#include <sstream>
#include <iostream>
#endif

#include <locale>

// Globals
Log* g_Log;
Options* g_Options;
WorkState* g_WorkState;
ServerPool* g_ServerPool;
QueueCoordinator* g_QueueCoordinator;
UrlCoordinator* g_UrlCoordinator;
StatMeter* g_StatMeter;
PrePostProcessor* g_PrePostProcessor;
HistoryCoordinator* g_HistoryCoordinator;
DupeCoordinator* g_DupeCoordinator;
DiskState* g_DiskState;
Scanner* g_Scanner;
FeedCoordinator* g_FeedCoordinator;
Maintenance* g_Maintenance;
ArticleCache* g_ArticleCache;
QueueScriptCoordinator* g_QueueScriptCoordinator;
ServiceCoordinator* g_ServiceCoordinator;
ScriptConfig* g_ScriptConfig;
CommandScriptLog* g_CommandScriptLog;
ExtensionManager::Manager* g_ExtensionManager;
System::SystemInfo* g_SystemInfo;
SystemHealth::Service* g_SystemHealth;

int g_ArgumentCount;
char* (*g_EnvironmentVariables)[] = nullptr;
char* (*g_Arguments)[] = nullptr;

extern void ExitProc();
extern void Reload();
std::atomic<bool> g_SigHupPending{false};
std::atomic<bool> g_SigExitPending{false};
static std::atomic<bool> s_shutdownInitiated{false};

#ifdef DEBUG
typedef void(*sighandler)(int);
inline std::vector<sighandler> SignalProcList;
#endif

static void PrintBacktrace()
{
#ifdef HAVE_BACKTRACE
	printf("Segmentation fault, tracing...\n");

	void *array[100];
	size_t size = backtrace(array, 100);
	char **strings = backtrace_symbols(array, size);

	printf("Obtained %zu stack frames\n", size);
	for (size_t i = 0; i < size; i++)
	{
		printf("%s\n", strings[i]);
	}

	error("Segmentation fault, tracing...");
	error("Obtained %zd stack frames", size);
	for (size_t i = 0; i < size; i++)
	{
		error("Stacktrace: %s", strings[i]);
	}

	free(strings);
#else
	error("Segmentation fault");
#endif
}

static void SignalProc(int signum)
{
	switch (signum)
	{
#ifdef SIGHUP
		case SIGHUP:
			if (s_shutdownInitiated.load(std::memory_order_relaxed))
			{
				return;
			}
			g_SigHupPending.store(true, std::memory_order_seq_cst);
			nzbget_rs_signal_notify(signum);
			break;
#endif

		case SIGINT:
		case SIGTERM:
		{
			if (s_shutdownInitiated.exchange(true, std::memory_order_seq_cst))
			{
				return;
			}
			signal(SIGINT, SIG_IGN);
			signal(SIGTERM, SIG_IGN);
#ifdef SIGHUP
			signal(SIGHUP, SIG_IGN);
#endif
			g_SigExitPending.store(true, std::memory_order_seq_cst);
			nzbget_rs_signal_notify(signum);
			break;
		}

		case SIGCHLD:
			break;

#ifdef DEBUG
		case SIGSEGV:
			signal(SIGSEGV, SIG_DFL);
			PrintBacktrace();
			break;
#endif
	}
}

static void InstallErrorHandler()
{
	if (g_Options->GetCrashDump())
	{
		nzbget_rs_enable_core_dump();
	}

	bool exitPending = g_SigExitPending.load(std::memory_order_seq_cst);
	s_shutdownInitiated.store(exitPending, std::memory_order_seq_cst);
	g_SigHupPending.store(false, std::memory_order_seq_cst);

	struct sigaction sa;
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = SignalProc;
	sigemptyset(&sa.sa_mask);
	sigaddset(&sa.sa_mask, SIGINT);
	sigaddset(&sa.sa_mask, SIGTERM);
#ifdef SIGHUP
	sigaddset(&sa.sa_mask, SIGHUP);
#endif
	sa.sa_flags = SA_RESTART;

	sigaction(SIGINT, &sa, nullptr);
	sigaction(SIGTERM, &sa, nullptr);
#ifdef SIGHUP
	sigaction(SIGHUP, &sa, nullptr);
#endif
	signal(SIGPIPE, SIG_IGN);
#ifdef DEBUG
	if (g_Options->GetCrashTrace())
	{
		signal(SIGSEGV, SignalProc);
	}
#endif
#ifdef SIGCHLD_HANDLER
	signal(SIGCHLD, SignalProc);
#endif
}

#ifdef DEBUG
static void TestSegFault()
{
	char* N = nullptr;
	*N = '\0';
}
#endif

class DiskService : public Service, public Observer
{
public:
	DiskService()
	{
		g_WorkState->Attach(this);
	}

protected:
	int ServiceInterval() override
	{
		return nzbget_rs_euh_disk_service_interval(
			m_waitingRequiredDir ? 1 : 0,
			g_Options->GetDiskSpace(),
			g_WorkState->GetDownloading() ? 1 : 0,
			Service::Sleep);
	}

	void ServiceWork() override
	{
		debug("Disk service work");

		if (nzbget_rs_dqh_disk_service_should_check_disk_space(
			g_Options->GetDiskSpace(), g_WorkState->GetDownloading() ? 1 : 0) != 0)
		{
			CheckDiskSpace();
		}

		if (m_waitingRequiredDir)
		{
			CheckRequiredDir();
		}
	}

	void Update(Subject* caller, void* aspect) override
	{
		(void)caller; (void)aspect;
		WakeUp();
	}

private:
	bool m_waitingRequiredDir = true;
	bool m_waitingReported = false;

	void CheckDiskSpace()
	{
		debug("Disk service work: check disk space");
		int destLow = 0;
		int interLow = 0;
		nzbget_rs_disk_service_check_disk_space(
			g_Options->GetDestDir(),
			g_Options->GetInterDir(),
			g_Options->GetDiskSpace(),
			&destLow,
			&interLow);
		if (destLow != 0)
		{
			warn("Low disk space on %s. Pausing download", g_Options->GetDestDir());
			g_WorkState->SetPauseDownload(true);
		}
		if (interLow != 0)
		{
			warn("Low disk space on %s. Pausing download", g_Options->GetInterDir());
			g_WorkState->SetPauseDownload(true);
		}
	}

	void CheckRequiredDir()
	{
		debug("Disk service work: check required dir");
		int nextWaitingReported = m_waitingReported ? 1 : 0;
		int allExist = nzbget_rs_disk_service_check_required_dirs(
			g_Options->GetRequiredDir(),
			m_waitingReported ? 1 : 0,
			&nextWaitingReported,
			nullptr,
			+[](void*, const char* dir)
			{
				info("Waiting for required directory %s", dir);
			});
		m_waitingReported = nextWaitingReported != 0;
		if (!allExist)
		{
			return;
		}
		if (m_waitingReported)
		{
			info("All required directories available");
		}
		g_WorkState->SetTempPauseDownload(false);
		g_WorkState->SetTempPausePostprocess(false);
		m_waitingRequiredDir = false;
	}
};

class CommandLineParser
{
public:
	enum EClientOperation
	{
		opClientNoOperation,
		opClientRequestDownload,
		opClientRequestListFiles,
		opClientRequestListGroups,
		opClientRequestListStatus,
		opClientRequestSetRate,
		opClientRequestDumpDebug,
		opClientRequestEditQueue,
		opClientRequestLog,
		opClientRequestShutdown,
		opClientRequestReload,
		opClientRequestVersion,
		opClientRequestPostQueue,
		opClientRequestWriteLog,
		opClientRequestScanSync,
		opClientRequestScanAsync,
		opClientRequestDownloadPause,
		opClientRequestDownloadUnpause,
		opClientRequestPostPause,
		opClientRequestPostUnpause,
		opClientRequestScanPause,
		opClientRequestScanUnpause,
		opClientRequestHistory,
		opClientRequestHistoryAll
	};
	enum EMatchMode
	{
		mmId = 1,
		mmName,
		mmRegEx
	};

	typedef std::vector<int> IdList;
	typedef std::vector<CString> NameList;

	CommandLineParser(int argc, const char* argv[])
	{
		if (argc < 1 || !argv)
		{
			throw std::invalid_argument("Invalid command line");
		}
		m_args.reserve(argc);
		for (int i = 0; i < argc; i++)
		{
			m_args.emplace_back(argv[i]);
		}

		std::vector<char*> args;
		args.reserve(static_cast<size_t>(argc) + 1);
		for (CString& arg : m_args) args.push_back(arg);
		args.push_back(nullptr);
		RsSink sink{this, args.data(), nullptr};
		NzbgetRsCmdlineSink rsSink{nullptr, &sink, RsSetInt, RsSetStr, RsSteal, RsPushOption, RsPushId, RsPushName, RsError};
#ifdef HAVE_GETOPT_LONG
		const int useLong = 1;
#else
		const int useLong = 0;
#endif
		int result = nzbget_rs_cmdline_parse(argc, args.data(), useLong, &rsSink);
		sink.SyncArgs();
		if (sink.error)
		{
			std::rethrow_exception(sink.error);
		}
		if (result != 0)
		{
			throw std::runtime_error("Command line parsing failed");
		}
	}

	void PrintUsage(const char* com)
	{
		NzbgetRsBuf buf = nzbget_rs_cmdline_format_usage(
			FileSystem::BaseFileName(com), PATH_SEPARATOR, 1);
		if (buf.data)
		{
			printf("%s", buf.data);
		}
		nzbget_rs_free(buf);
	}

	bool GetErrors() { return m_errors; }
	bool GetNoConfig() { return m_noConfig; }
	const char* GetConfigFilename() { return m_configFilename; }
	bool GetServerMode() { return m_serverMode; }
	bool GetDaemonMode() { return m_daemonMode; }
	bool GetRemoteClientMode() { return m_remoteClientMode; }
	EClientOperation GetClientOperation() { return m_clientOperation; }
	NameList* GetOptionList() { return &m_optionList; }
	int GetEditQueueAction() { return m_editQueueAction; }
	int GetEditQueueOffset() { return m_editQueueOffset; }
	IdList* GetEditQueueIdList() { return &m_editQueueIdList; }
	NameList* GetEditQueueNameList() { return &m_editQueueNameList; }
	EMatchMode GetMatchMode() { return m_matchMode; }
	const char* GetEditQueueText() { return m_editQueueText; }
	const char* GetArgFilename() { return m_argFilename; }
	const char* GetAddCategory() { return m_addCategory; }
	bool GetAutoCategory() const { return m_autoCategory; }
	bool GetAddPaused() { return m_addPaused; }
	const char* GetLastArg() { return m_lastArg; }
	int GetAddPriority() { return m_addPriority; }
	const char* GetAddNzbFilename() { return m_addNzbFilename; }
	bool GetAddTop() { return m_addTop; }
	const char* GetAddDupeKey() { return m_addDupeKey; }
	int GetAddDupeScore() { return m_addDupeScore; }
	int GetAddDupeMode() { return m_addDupeMode; }
	int GetSetRate() { return m_setRate; }
	int GetLogLines() { return m_logLines; }
	int GetWriteLogKind() { return m_writeLogKind; }
	bool GetTestBacktrace() { return m_testBacktrace; }
	bool GetWebGet() { return m_webGet; }
	const char* GetWebGetFilename() { return m_webGetFilename; }
	bool GetSigVerify() { return m_sigVerify; }
	const char* GetPubKeyFilename() { return m_pubKeyFilename; }
	const char* GetSigFilename() { return m_sigFilename; }
	bool GetPrintOptions() { return m_printOptions; }
	bool GetPrintVersion() { return m_printVersion; }
	bool GetPrintUsage() { return m_printUsage; }
	bool GetPauseDownload() const { return m_pauseDownload; }

private:
	void SetAddCategory(const char* category)
	{
		m_addCategory = category;
		m_autoCategory = false;
	}

	bool m_noConfig = false;
	CString m_configFilename;
	bool m_errors = false;
	bool m_printVersion = false;
	bool m_printUsage = false;
	bool m_serverMode = false;
	bool m_daemonMode = false;
	bool m_remoteClientMode = false;
	EClientOperation m_clientOperation;
	NameList m_optionList;
	int m_editQueueAction = 0;
	int m_editQueueOffset = 0;
	IdList m_editQueueIdList;
	NameList m_editQueueNameList;
	EMatchMode m_matchMode = mmId;
	CString m_editQueueText;
	CString m_argFilename;
	CString m_addCategory;
	bool m_autoCategory = true;
	int m_addPriority = 0;
	bool m_addPaused = false;
	CString m_addNzbFilename;
	CString m_lastArg;
	bool m_printOptions = false;
	bool m_addTop = false;
	CString m_addDupeKey;
	int m_addDupeScore = 0;
	int m_addDupeMode = 0;
	int m_setRate = 0;
	int m_logLines = 0;
	int m_writeLogKind = 0;
	bool m_testBacktrace = false;
	bool m_webGet = false;
	CString m_webGetFilename;
	bool m_sigVerify = false;
	CString m_pubKeyFilename;
	CString m_sigFilename;
	bool m_pauseDownload = false;

	std::vector<CString> m_args;
	void ReportError(const char* errMessage)
	{
		m_errors = true;
		printf("%s\n", errMessage);
	}

	struct RsSink
	{
		CommandLineParser* parser;
		char** argv;
		std::exception_ptr error;

		void SyncArgs() noexcept
		{
			for (CString& arg : parser->m_args) arg.Unbind();
			for (size_t i = 0; i < parser->m_args.size(); i++) parser->m_args[i].Bind(argv[i]);
		}

		template <typename F>
		static int Call(void* ctx, F f) noexcept
		{
			RsSink* sink = static_cast<RsSink*>(ctx);
			try
			{
				f(sink->parser);
				return 0;
			}
			catch (...)
			{
				sink->error = std::current_exception();
				return -1;
			}
		}
	};

	static int RsSetInt(void* ctx, int field, int value)
	{
		static const int EDIT_ACTIONS[] = {
			DownloadQueue::eaPostDelete, DownloadQueue::eaHistoryDelete, DownloadQueue::eaHistoryReturn,
			DownloadQueue::eaHistoryProcess, DownloadQueue::eaHistoryRedownload, DownloadQueue::eaHistoryRetryFailed,
			DownloadQueue::eaHistorySetParameter, DownloadQueue::eaHistoryMarkBad, DownloadQueue::eaHistoryMarkGood,
			DownloadQueue::eaHistoryMarkSuccess, DownloadQueue::eaGroupMoveTop, DownloadQueue::eaFileMoveTop,
			DownloadQueue::eaGroupMoveBottom, DownloadQueue::eaFileMoveBottom, DownloadQueue::eaGroupPause,
			DownloadQueue::eaFilePause, DownloadQueue::eaGroupPauseAllPars, DownloadQueue::eaFilePauseAllPars,
			DownloadQueue::eaGroupPauseExtraPars, DownloadQueue::eaFilePauseExtraPars, DownloadQueue::eaGroupResume,
			DownloadQueue::eaFileResume, DownloadQueue::eaGroupDelete, DownloadQueue::eaFileDelete,
			DownloadQueue::eaGroupParkDelete, DownloadQueue::eaGroupSortFiles, DownloadQueue::eaGroupApplyCategory,
			DownloadQueue::eaGroupSetCategory, DownloadQueue::eaGroupSetName, DownloadQueue::eaGroupMerge,
			DownloadQueue::eaFileSplit, DownloadQueue::eaGroupSetParameter, DownloadQueue::eaGroupSetPriority,
			DownloadQueue::eaGroupMoveOffset, DownloadQueue::eaFileMoveOffset};
		static const int DUPE_MODES[] = {dmScore, dmAll, dmForce};
		static const int LOG_KINDS[] = {(int)Message::mkInfo, (int)Message::mkWarning, (int)Message::mkError,
			(int)Message::mkDetail, (int)Message::mkDebug};
		return RsSink::Call(ctx, [field, value](CommandLineParser* p)
		{
			switch (field)
			{
				case 0: p->m_noConfig = value; break;
				case 1: p->m_printUsage = value; break;
				case 2: p->m_printVersion = value; break;
				case 3: p->m_printOptions = value; break;
				case 4: p->m_serverMode = value; break;
				case 5: p->m_daemonMode = value; break;
				case 6: p->m_remoteClientMode = value; break;
				case 7: p->m_clientOperation = (EClientOperation)value; break;
				case 8: p->m_addTop = value; break;
				case 9: p->m_addPaused = value; break;
				case 10: p->m_addPriority = value; break;
				case 11: p->m_addDupeScore = value; break;
				case 12: p->m_addDupeMode = DUPE_MODES[value]; break;
				case 13: p->m_matchMode = (EMatchMode)value; break;
				case 15: p->m_testBacktrace = value; break;
				case 16: p->m_webGet = value; break;
				case 17: p->m_sigVerify = value; break;
				case 18: p->m_logLines = value; break;
				case 19: p->m_editQueueAction = EDIT_ACTIONS[value]; break;
				case 20: p->m_editQueueOffset = value; break;
				case 21: p->m_writeLogKind = LOG_KINDS[value]; break;
				case 22: p->m_pauseDownload = value; break;
				case 23: p->m_errors = value; break;
			}
		});
	}

	static int RsSetStr(void* ctx, int field, const char* value)
	{
		return RsSink::Call(ctx, [field, value](CommandLineParser* p)
		{
			switch (field)
			{
				case 30: p->m_configFilename = value; break;
				case 31: p->m_webGetFilename = value; break;
				case 32: p->m_pubKeyFilename = value; break;
				case 33: p->m_sigFilename = value; break;
				case 34: p->SetAddCategory(value); break;
				case 35: p->m_lastArg = value; break;
				case 36: p->m_argFilename = value; break;
				case 40: p->m_setRate = (int)(atof(value) * 1024); break;
			}
		});
	}

	static int RsSteal(void* ctx, int field, int index)
	{
		return RsSink::Call(ctx, [ctx, field, index](CommandLineParser* p)
		{
			RsSink* sink = static_cast<RsSink*>(ctx);
			sink->SyncArgs();
			CString& arg = p->m_args[index];
			switch (field)
			{
				case 37: p->m_addNzbFilename = std::move(arg); break;
				case 38: p->m_addDupeKey = std::move(arg); break;
				case 39: p->m_editQueueText = std::move(arg); break;
			}
			sink->argv[index] = arg;
		});
	}

	static int RsPushOption(void* ctx, const char* value)
	{
		return RsSink::Call(ctx, [value](CommandLineParser* p) { p->m_optionList.push_back(value); });
	}

	static int RsPushId(void* ctx, int id)
	{
		return RsSink::Call(ctx, [id](CommandLineParser* p) { p->m_editQueueIdList.push_back(id); });
	}

	static int RsPushName(void* ctx, const char* value)
	{
		return RsSink::Call(ctx, [value](CommandLineParser* p) { p->m_editQueueNameList.push_back(value); });
	}

	static int RsError(void* ctx, const char* msg)
	{
		return RsSink::Call(ctx, [msg](CommandLineParser* p) { p->ReportError(msg); });
	}
};

class RustyGet final : public Options::Extender
{
public:
	~RustyGet() override;
	void Stop(bool reload);
	bool GetReloading() { return m_reloading; }
	bool GetStopped() const { return m_stopped; }

	int Phase1_Init(bool reload);
	int Phase2_SubsystemsAndState(bool reload);
	int Phase3_ServerActivation();
	void Phase4_Shutdown(bool reload);
	bool HasActiveUnpack() const;

	// Options::Extender
	void AddNewsServer(int id, bool active, const char* name, const char* host,
		int port, int ipVersion, const char* user, const char* pass, bool joinGroup,
		bool tls, const char* cipher, int maxConnections, int retention,
		int level, int group, bool optional, unsigned int certVerificationfLevel) override;
	void AddFeed(int id, const char* name, const char* url, int interval,
		const char* filter, bool backlog, bool pauseNzb, const char* category,
		FeedInfo::CategorySource categorySource, int priority, const char* feedScript,
		unsigned int certVerifLevel) override;
	void AddTask(int id, int hours, int minutes, int weekDaysBits,
		Options::ESchedulerCommand command, const char* param) override;

private:
	// globals
	std::unique_ptr<Log> m_log;
	std::unique_ptr<Options> m_options;
	std::unique_ptr<WorkState> m_workState;
	std::unique_ptr<ServerPool> m_serverPool;
	std::unique_ptr<QueueCoordinator> m_queueCoordinator;
	std::unique_ptr<UrlCoordinator> m_urlCoordinator;
	std::unique_ptr<DupeSearch> m_dupeSearch;
	std::unique_ptr<StatMeter> m_statMeter;
	std::unique_ptr<PrePostProcessor> m_prePostProcessor;
	std::unique_ptr<HistoryCoordinator> m_historyCoordinator;
	std::unique_ptr<TwinCheck> m_twinCheck;
	std::unique_ptr<DupeCoordinator> m_dupeCoordinator;
	std::unique_ptr<DiskState> m_diskState;
	std::unique_ptr<Scanner> m_scanner;
	std::unique_ptr<FeedCoordinator> m_feedCoordinator;
	std::unique_ptr<Maintenance> m_maintenance;
	std::unique_ptr<ArticleCache> m_articleCache;
	std::unique_ptr<QueueScriptCoordinator> m_queueScriptCoordinator;
	std::unique_ptr<ServiceCoordinator> m_serviceCoordinator;
	std::unique_ptr<ScriptConfig> m_scriptConfig;
	std::unique_ptr<CommandScriptLog> m_commandScriptLog;
	std::unique_ptr<ExtensionManager::Manager> m_extensionManager;
	std::unique_ptr<System::SystemInfo> m_systemInfo;
	std::unique_ptr<SystemHealth::Service> m_systemHealth;
	std::unique_ptr<MemoryMonitor> m_memoryMonitor;

	// non-globals
	std::unique_ptr<Thread> m_frontend;
	std::unique_ptr<RemoteServer> m_remoteServer;
	std::unique_ptr<RemoteServer> m_remoteSecureServer;
	std::unique_ptr<DiskService> m_diskService;
	std::unique_ptr<Scheduler> m_scheduler;
	std::unique_ptr<CommandLineParser> m_commandLineParser;

	bool m_reloading = false;
	bool m_stopped = false;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;

	void Final();
	void BootConfig();
	void CreateGlobals();
	void Cleanup();
	void PrintOptions();
	void ProcessDirect();
	void ProcessClientRequest();
	void ProcessWebGet();
	void ProcessSigVerify();
	void StartRemoteServer();
	void StopRemoteServer();
	void StartFrontend();
	void StopFrontend();
	void ProcessStandalone();
	void DoMainLoop();
	void Daemonize();
};

std::unique_ptr<RustyGet> g_RustyGet;

// the daemon holds the lock on the lock-file: only then is it ours to delete
// at shutdown. Not a member: a reload makes a new NZBGet without daemonizing,
// and a second daemon that fails to get the lock must leave it in place.
static bool g_LockFileOwned = false;

RustyGet::~RustyGet()
{
	Cleanup();
}

int RustyGet::Phase1_Init(bool reload)
{
	m_reloading = reload;
	m_log = std::make_unique<Log>();

	debug("RustyGet %s", Util::VersionRevision());

	if (!m_reloading)
	{
		Connection::Init();
#ifndef DISABLE_TLS
		TlsSocket::Init();
#endif
	}

	CreateGlobals();

	BootConfig();

	if (m_memoryMonitor)
	{
		m_memoryMonitor->InitOptions();
	}

	mode_t uMask = static_cast<mode_t>(m_options->GetUMask());
	if (nzbget_rs_ccm_nzbget_is_valid_umask((uint32_t)uMask) != 0)
	{
		/* set newly created file permissions */
		FileSystem::uMask = uMask;
		umask(FileSystem::uMask);
	}
	else
	{
		FileSystem::uMask = umask(0);
		umask(FileSystem::uMask);
	}

#ifdef DEBUG
	debug("Using %o umask", FileSystem::uMask);
#endif

	m_scanner->InitOptions();
	m_queueScriptCoordinator->InitOptions();
#ifndef DISABLE_TLS
	TlsSocket::InitOptions(g_Options->GetCertCheck() ? g_Options->GetCertStore() : "");
#endif

	ProcessDirect();

	if (nzbget_rs_dqh_nzbget_should_run_system_health_check(
		g_Options->GetServerMode() ? 1 : 0,
		g_Options->GetSystemHealthCheck() ? 1 : 0) != 0)
	{
		m_systemHealth = std::make_unique<SystemHealth::Service>(
			*g_Options,
			*g_ServerPool->GetServers(),
			*g_FeedCoordinator->GetFeeds(),
			m_scheduler->GetTasks(),
			*g_Log);
		g_SystemHealth = m_systemHealth.get();

		m_systemHealth->DiagnoseAndLog();
	}

	if (m_commandLineParser->GetDaemonMode())
	{
		if (!m_reloading)
		{
			Daemonize();
		}
		info("RustyGet %s daemon-mode", Util::VersionRevision());
	}
	else if (m_options->GetServerMode())
	{
		info("RustyGet %s server-mode", Util::VersionRevision());
	}
	else if (m_commandLineParser->GetRemoteClientMode())
	{
		info("RustyGet %s remote-mode", Util::VersionRevision());
	}

	if (m_options->GetServerMode())
	{
		info("using %s", m_options->GetConfigFilename());
		info("RustyGet runs on %s:%i", m_options->GetControlIp(), m_options->GetControlPort());
	}

#ifdef DEBUG
	std::stringstream ss;
	ss << *m_systemInfo;
	std::string line;
	while (std::getline(ss, line))
	{
		detail("%s", line.c_str());
	}
#endif

	m_reloading = false;

	InstallErrorHandler();

	return 0;
}

int RustyGet::Phase2_SubsystemsAndState(bool reload)
{
	(void)reload;
	if (!m_commandLineParser->GetRemoteClientMode())
	{
		m_serverPool->InitConnections();
		m_statMeter->Init();
	}
	return 0;
}

void RustyGet::Final()
{
	TlsCipherBackend::Shutdown();
	if (!m_reloading)
	{
#ifndef DISABLE_TLS
		TlsSocket::Final();
#endif
		Connection::Final();
	}
}

void RustyGet::CreateGlobals()
{
	m_workState = std::make_unique<WorkState>();
	g_WorkState = m_workState.get();

	m_serviceCoordinator = std::make_unique<ServiceCoordinator>();
	g_ServiceCoordinator = m_serviceCoordinator.get();

	m_serverPool = std::make_unique<ServerPool>();
	g_ServerPool = m_serverPool.get();

	m_queueCoordinator = std::make_unique<QueueCoordinator>();
	g_QueueCoordinator = m_queueCoordinator.get();

	m_statMeter = std::make_unique<StatMeter>();
	g_StatMeter = m_statMeter.get();

	m_scanner = std::make_unique<Scanner>();
	g_Scanner = m_scanner.get();

	m_prePostProcessor = std::make_unique<PrePostProcessor>();
	g_PrePostProcessor = m_prePostProcessor.get();

	m_historyCoordinator = std::make_unique<HistoryCoordinator>();
	m_twinCheck = std::make_unique<TwinCheck>();
	g_HistoryCoordinator = m_historyCoordinator.get();

	m_dupeCoordinator = std::make_unique<DupeCoordinator>();
	g_DupeCoordinator = m_dupeCoordinator.get();

	m_urlCoordinator = std::make_unique<UrlCoordinator>();
	g_UrlCoordinator = m_urlCoordinator.get();

	m_dupeSearch = std::make_unique<DupeSearch>();
	g_DupeSearch = m_dupeSearch.get();

	m_feedCoordinator = std::make_unique<FeedCoordinator>();
	g_FeedCoordinator = m_feedCoordinator.get();

	m_articleCache = std::make_unique<ArticleCache>();
	g_ArticleCache = m_articleCache.get();

	m_memoryMonitor = std::make_unique<MemoryMonitor>();
	g_MemoryMonitor = m_memoryMonitor.get();

	m_maintenance = std::make_unique<Maintenance>();
	g_Maintenance = m_maintenance.get();

	m_queueScriptCoordinator = std::make_unique<QueueScriptCoordinator>();
	g_QueueScriptCoordinator = m_queueScriptCoordinator.get();

	m_diskState = std::make_unique<DiskState>();
	g_DiskState = m_diskState.get();

	m_scriptConfig = std::make_unique<ScriptConfig>();
	g_ScriptConfig = m_scriptConfig.get();

	m_commandScriptLog = std::make_unique<CommandScriptLog>();
	g_CommandScriptLog = m_commandScriptLog.get();

	m_extensionManager = std::make_unique<ExtensionManager::Manager>();
	g_ExtensionManager = m_extensionManager.get();

	m_systemInfo = std::make_unique<System::SystemInfo>();
	g_SystemInfo = m_systemInfo.get();

	m_scheduler = std::make_unique<Scheduler>();

	m_diskService = std::make_unique<DiskService>();
}

void RustyGet::BootConfig()
{
	debug("Parsing command line");
	m_commandLineParser = std::make_unique<CommandLineParser>(g_ArgumentCount, (const char**)(*g_Arguments));
	if (m_commandLineParser->GetPrintVersion())
	{
		printf("RustyGet version: %s\n", Util::VersionRevision());
		exit(0);
	}
	int bootExitCode = 0;
	if (nzbget_rs_dqh_nzbget_classify_boot_usage_or_error(
		m_commandLineParser->GetPrintUsage() ? 1 : 0,
		m_commandLineParser->GetErrors() ? 1 : 0,
		g_ArgumentCount,
		&bootExitCode) != 0)
	{
		m_commandLineParser->PrintUsage(((const char**)(*g_Arguments))[0]);
		exit(bootExitCode);
	}

	debug("Reading options");
	m_options = std::make_unique<Options>((*g_Arguments)[0], m_commandLineParser->GetConfigFilename(),
		m_commandLineParser->GetNoConfig(), (Options::CmdOptList*)m_commandLineParser->GetOptionList(), this);
	m_options->SetRemoteClientMode(m_commandLineParser->GetRemoteClientMode());
	m_options->SetServerMode(m_commandLineParser->GetServerMode());
	m_options->SetDaemonMode(m_commandLineParser->GetDaemonMode());
	int testBacktrace = 0;
#ifdef DEBUG
	testBacktrace = m_commandLineParser->GetTestBacktrace() ? 1 : 0;
#endif
	m_options->SetDirectClientMode(nzbget_rs_roc_nzbget_boot_is_direct_client_mode(
		testBacktrace,
		m_commandLineParser->GetWebGet() ? 1 : 0,
		m_commandLineParser->GetSigVerify() ? 1 : 0,
		(int)m_commandLineParser->GetClientOperation(),
		(int)CommandLineParser::opClientNoOperation,
		m_commandLineParser->GetPrintOptions() ? 1 : 0) != 0);
	m_workState->SetPauseDownload(m_commandLineParser->GetPauseDownload());
	m_workState->SetSpeedLimit(g_Options->GetDownloadRate());

	m_log->InitOptions();

	if (m_options->GetFatalError())
	{
		exit(1);
	}
	else if (nzbget_rs_roc_nzbget_boot_should_pause_on_config_errors(
		m_options->GetConfigErrors() ? 1 : 0,
		(int)m_commandLineParser->GetClientOperation(),
		(int)CommandLineParser::opClientNoOperation))
	{
		info("Pausing all activities due to errors in configuration");
		m_workState->SetPauseDownload(true);
		m_workState->SetPausePostProcess(true);
		m_workState->SetPauseScan(true);
	}

	m_serverPool->SetTimeout(m_options->GetArticleTimeout());
	m_serverPool->SetRetryInterval(m_options->GetArticleInterval());

	m_scriptConfig->InitOptions();

	m_extensionManager->LoadExtensions();
}

void RustyGet::Cleanup()
{
	debug("Cleaning up global objects");

	if (m_options && !m_reloading && g_LockFileOwned)
	{
		info("Deleting lock file");
		FileSystem::DeleteFile(m_options->GetLockFile());
	}

	g_UrlCoordinator = nullptr;
	g_DupeSearch = nullptr;
	g_PrePostProcessor = nullptr;
	g_Scanner = nullptr;
	g_HistoryCoordinator = nullptr;
	g_DupeCoordinator = nullptr;
	g_QueueCoordinator = nullptr;
	g_DiskState = nullptr;
	g_ScriptConfig = nullptr;
	g_ServerPool = nullptr;
	g_FeedCoordinator = nullptr;
	g_ArticleCache = nullptr;
	if (m_memoryMonitor)
	{
		m_memoryMonitor->Stop();
		while (m_memoryMonitor->IsRunning())
		{
			Util::Sleep(10);
		}
	}
	g_MemoryMonitor = nullptr;
	m_memoryMonitor.reset();
	g_QueueScriptCoordinator = nullptr;
	// destroyed while the global still points to it: its destructor waits for
	// a running update-script, whose thread finishes through g_Maintenance
	m_maintenance.reset();
	g_Maintenance = nullptr;
	g_StatMeter = nullptr;
	g_CommandScriptLog = nullptr;
	g_ExtensionManager = nullptr;
}

void RustyGet::ProcessDirect()
{
#ifdef DEBUG
	if (m_commandLineParser->GetTestBacktrace())
	{
		TestSegFault(); // never returns
	}
#endif

	if (m_commandLineParser->GetWebGet())
	{
		ProcessWebGet(); // never returns
	}

	if (m_commandLineParser->GetSigVerify())
	{
		ProcessSigVerify(); // never returns
	}

	// client request
	if (m_commandLineParser->GetClientOperation() != CommandLineParser::opClientNoOperation)
	{
		ProcessClientRequest(); // never returns
	}

	if (m_commandLineParser->GetPrintOptions())
	{
		PrintOptions(); // never returns
	}
}

void RustyGet::StartRemoteServer()
{
	if (!m_options->GetServerMode())
	{
		return;
	}

	WebProcessor::Init();
	m_remoteServer = std::make_unique<RemoteServer>(false);
	m_remoteServer->Start();

	if (nzbget_rs_ccm_nzbget_should_start_secure_server(
		m_options->GetSecureControl() ? 1 : 0, 1, m_options->GetControlIp()) != 0)
	{
		m_remoteSecureServer = std::make_unique<RemoteServer>(true);
		m_remoteSecureServer->Start();
	}
}

void RustyGet::StopRemoteServer()
{
	if (m_remoteServer)
	{
		debug("stopping RemoteServer");
		m_remoteServer->Stop();
	}

	if (m_remoteSecureServer)
	{
		debug("stopping RemoteSecureServer");
		m_remoteSecureServer->Stop();
	}

	int maxWaitMSec = 5000;
	while (((m_remoteServer && m_remoteServer->IsRunning()) ||
		(m_remoteSecureServer && m_remoteSecureServer->IsRunning())) &&
		maxWaitMSec > 0)
	{
		Util::Sleep(100);
		maxWaitMSec -= 100;
	}

	if (m_remoteServer && m_remoteServer->IsRunning())
	{
		m_remoteServer->ForceStop();
	}

	if (m_remoteSecureServer && m_remoteSecureServer->IsRunning())
	{
		m_remoteSecureServer->ForceStop();
	}

	maxWaitMSec = 5000;
	while (((m_remoteServer && m_remoteServer->IsRunning()) ||
		(m_remoteSecureServer && m_remoteSecureServer->IsRunning())) &&
		maxWaitMSec > 0)
	{
		Util::Sleep(100);
		maxWaitMSec -= 100;
	}

	if (m_remoteServer && m_remoteServer->IsRunning())
	{
		debug("Killing RemoteServer");
		m_remoteServer->Kill();
	}

	if (m_remoteSecureServer && m_remoteSecureServer->IsRunning())
	{
		debug("Killing RemoteSecureServer");
		m_remoteSecureServer->Kill();
	}

	debug("RemoteServer stopped");
}

void RustyGet::StartFrontend()
{
	if (!m_commandLineParser->GetDaemonMode())
	{
		switch (m_options->GetOutputMode())
		{
		case Options::omNCurses:
#ifndef DISABLE_CURSES
			m_frontend = std::make_unique<NCursesFrontend>();
			break;
#endif

		case Options::omColored:
			m_frontend = std::make_unique<ColoredFrontend>();
			break;

		case Options::omLoggable:
			m_frontend = std::make_unique<LoggableFrontend>();
			break;
		}
	}

	if (m_frontend)
	{
		m_frontend->Start();
	}
}

void RustyGet::StopFrontend()
{
	if (m_frontend)
	{
		if (nzbget_rs_dqh_nzbget_should_stop_frontend_immediately(
			m_commandLineParser->GetRemoteClientMode() ? 1 : 0,
			m_stopped ? 1 : 0) != 0)
		{
			debug("Stopping Frontend");
			m_frontend->Stop();
		}
		while (m_frontend->IsRunning())
		{
			if (g_SigExitPending.exchange(false))
			{
				ExitProc();
			}
			Util::Sleep(50);
		}
		debug("Frontend stopped");
	}
}

void RustyGet::ProcessStandalone()
{
	const char* category = m_commandLineParser->GetAddCategory() ? m_commandLineParser->GetAddCategory() : "";
	NzbFile nzbFile(m_commandLineParser->GetArgFilename(), category);
	if (!nzbFile.Parse())
	{
		printf("Parsing NZB-document %s failed\n\n",
			m_commandLineParser->GetArgFilename() ? m_commandLineParser->GetArgFilename() : "N/A");
		return;
	}
	std::unique_ptr<NzbInfo> nzbInfo = nzbFile.DetachNzbInfo();

	if (!nzbFile.GetPassword().empty())
	{
		nzbInfo->GetParameters()->SetParameter("*Unpack:Password", nzbFile.GetPassword().c_str());
	}

	m_scanner->InitPPParameters(category, nzbInfo->GetParameters(), false);
	m_queueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), nullptr, false);
}

static std::mutex g_reloadMutex;
static std::condition_variable g_reloadCond;
static bool g_reloadPending{false};
static bool g_reloadWorkerRunning{false};
static std::chrono::steady_clock::time_point g_reloadTargetTime;
static std::chrono::steady_clock::time_point g_reloadFirstRequestTime;
static std::thread g_reloadThread;

static void CancelAndJoinReloadThread()
{
	std::unique_lock<std::mutex> lock(g_reloadMutex);
	g_reloadPending = false;
	g_reloadCond.notify_all();
	if (g_reloadThread.joinable() && g_reloadThread.get_id() != std::this_thread::get_id())
	{
		g_reloadCond.wait(lock, [] { return !g_reloadWorkerRunning; });
		g_reloadThread.join();
	}
}

void RustyGet::DoMainLoop()
{
	debug("Entering main program loop");

	m_queueCoordinator->Start();
	m_urlCoordinator->Start();
	m_dupeSearch->Start();
	m_prePostProcessor->Start();
	m_feedCoordinator->Start();
	m_serviceCoordinator->Start();
	if (m_options->GetArticleCache() > 0)
	{
		m_articleCache->Start();
	}
	if (m_memoryMonitor)
	{
		m_memoryMonitor->Start();
	}

	// enter main program-loop
	while (m_queueCoordinator->IsRunning() ||
		m_urlCoordinator->IsRunning() ||
		m_dupeSearch->IsRunning() ||
		m_prePostProcessor->IsRunning() ||
		m_feedCoordinator->IsRunning() ||
		m_serviceCoordinator->IsRunning() ||
		(m_memoryMonitor && m_memoryMonitor->IsRunning()) ||
		m_articleCache->IsRunning())
	{
		if (g_SigExitPending.exchange(false))
		{
			ExitProc();
		}
		else if (g_SigHupPending.exchange(false))
		{
			Reload();
		}

		if (nzbget_rs_ccm_nzbget_should_stop_standalone(
			m_options->GetServerMode() ? 1 : 0,
			m_queueCoordinator->HasMoreJobs() ? 1 : 0,
			m_urlCoordinator->HasMoreJobs() ? 1 : 0,
			m_prePostProcessor->HasMoreJobs() ? 1 : 0) != 0)
		{
			// Standalone-mode: download completed
			if (!m_queueCoordinator->IsStopped())
			{
				m_queueCoordinator->Stop();
			}
			if (!m_urlCoordinator->IsStopped())
			{
				m_urlCoordinator->Stop();
			}
			if (!m_dupeSearch->IsStopped())
			{
				m_dupeSearch->Stop();
			}
			if (!m_prePostProcessor->IsStopped())
			{
				m_prePostProcessor->Stop();
			}
			if (!m_feedCoordinator->IsStopped())
			{
				m_feedCoordinator->Stop();
			}
			if (!m_articleCache->IsStopped())
			{
				m_articleCache->Stop();
			}
			if (m_memoryMonitor && !m_memoryMonitor->IsStopped())
			{
				m_memoryMonitor->Stop();
			}
			if (!m_serviceCoordinator->IsStopped())
			{
				m_serviceCoordinator->Stop();
			}
		}
		Util::Sleep(100);

		if (m_options->GetServerMode() && !m_stopped)
		{
			// wait for stop signal or pending POSIX signal
			while (!m_stopped)
			{
				{
					std::unique_lock<std::mutex> lk(m_waitMutex);
					m_waitCond.wait_for(lk, std::chrono::milliseconds(50), [&]{
						return m_stopped || g_SigExitPending.load() || g_SigHupPending.load();
					});
				}

				if (g_SigExitPending.exchange(false))
				{
					ExitProc();
				}
				else if (g_SigHupPending.exchange(false))
				{
					Reload();
				}
			}
		}
	}

	debug("Main program loop terminated");
}

int RustyGet::Phase3_ServerActivation()
{
	StartRemoteServer();
	StartFrontend();

	if (!m_commandLineParser->GetRemoteClientMode())
	{
		if (!m_commandLineParser->GetServerMode() && !g_SigExitPending.load())
		{
			ProcessStandalone();
		}

		DoMainLoop();
	}
	else
	{
		if (g_SigExitPending.exchange(false))
		{
			ExitProc();
		}
		while (m_frontend && !m_stopped && !m_reloading && m_frontend->IsRunning())
		{
			if (g_SigExitPending.exchange(false))
			{
				ExitProc();
			}
			else if (g_SigHupPending.exchange(false))
			{
				Reload();
			}
			Util::Sleep(50);
		}
	}

	return 0;
}

void RustyGet::Phase4_Shutdown(bool reload)
{
	m_reloading = reload;
	CancelAndJoinReloadThread();
	ScriptController::TerminateAll();

	StopRemoteServer();
	StopFrontend();
	CancelAndJoinReloadThread();

	Final();
}

bool RustyGet::HasActiveUnpack() const
{
	return m_prePostProcessor ? m_prePostProcessor->HasActiveUnpack(false) : false;
}

void RustyGet::ProcessClientRequest()
{
	RemoteClient Client;
	bool ok = false;

	switch (m_commandLineParser->GetClientOperation())
	{
		case CommandLineParser::opClientRequestListFiles:
			ok = Client.RequestServerList(true, false, m_commandLineParser->GetMatchMode() == CommandLineParser::mmRegEx ? m_commandLineParser->GetEditQueueText() : nullptr);
			break;

		case CommandLineParser::opClientRequestListGroups:
			ok = Client.RequestServerList(false, true, m_commandLineParser->GetMatchMode() == CommandLineParser::mmRegEx ? m_commandLineParser->GetEditQueueText() : nullptr);
			break;

		case CommandLineParser::opClientRequestListStatus:
			ok = Client.RequestServerList(false, false, nullptr);
			break;

		case CommandLineParser::opClientRequestDownloadPause:
			ok = Client.RequestServerPauseUnpause(true, rpDownload);
			break;

		case CommandLineParser::opClientRequestDownloadUnpause:
			ok = Client.RequestServerPauseUnpause(false, rpDownload);
			break;

		case CommandLineParser::opClientRequestSetRate:
			ok = Client.RequestServerSetDownloadRate(m_commandLineParser->GetSetRate());
			break;

		case CommandLineParser::opClientRequestDumpDebug:
			ok = Client.RequestServerDumpDebug();
			break;

		case CommandLineParser::opClientRequestEditQueue:
			ok = Client.RequestServerEditQueue((DownloadQueue::EEditAction)m_commandLineParser->GetEditQueueAction(),
				m_commandLineParser->GetEditQueueOffset(), m_commandLineParser->GetEditQueueText(),
				m_commandLineParser->GetEditQueueIdList(), m_commandLineParser->GetEditQueueNameList(),
				(ERemoteMatchMode)m_commandLineParser->GetMatchMode());
			break;

		case CommandLineParser::opClientRequestLog:
			ok = Client.RequestServerLog(m_commandLineParser->GetLogLines());
			break;

		case CommandLineParser::opClientRequestShutdown:
			ok = Client.RequestServerShutdown();
			break;

		case CommandLineParser::opClientRequestReload:
			ok = Client.RequestServerReload();
			break;

		case CommandLineParser::opClientRequestDownload:
			ok = Client.RequestServerDownload(
				m_commandLineParser->GetAddNzbFilename(),
				m_commandLineParser->GetArgFilename(),
				m_commandLineParser->GetAddCategory(),
				m_commandLineParser->GetAutoCategory(),
				m_commandLineParser->GetAddTop(),
				m_commandLineParser->GetAddPaused(),
				m_commandLineParser->GetAddPriority(),
				m_commandLineParser->GetAddDupeKey(),
				m_commandLineParser->GetAddDupeMode(),
				m_commandLineParser->GetAddDupeScore());
			break;

		case CommandLineParser::opClientRequestVersion:
			ok = Client.RequestServerVersion();
			break;

		case CommandLineParser::opClientRequestPostQueue:
			ok = Client.RequestPostQueue();
			break;

		case CommandLineParser::opClientRequestWriteLog:
			ok = Client.RequestWriteLog(m_commandLineParser->GetWriteLogKind(), m_commandLineParser->GetLastArg());
			break;

		case CommandLineParser::opClientRequestScanAsync:
			ok = Client.RequestScan(false);
			break;

		case CommandLineParser::opClientRequestScanSync:
			ok = Client.RequestScan(true);
			break;

		case CommandLineParser::opClientRequestPostPause:
			ok = Client.RequestServerPauseUnpause(true, rpPostProcess);
			break;

		case CommandLineParser::opClientRequestPostUnpause:
			ok = Client.RequestServerPauseUnpause(false, rpPostProcess);
			break;

		case CommandLineParser::opClientRequestScanPause:
			ok = Client.RequestServerPauseUnpause(true, rpScan);
			break;

		case CommandLineParser::opClientRequestScanUnpause:
			ok = Client.RequestServerPauseUnpause(false, rpScan);
			break;

		case CommandLineParser::opClientRequestHistory:
		case CommandLineParser::opClientRequestHistoryAll:
			ok = Client.RequestHistory(m_commandLineParser->GetClientOperation() == CommandLineParser::opClientRequestHistoryAll);
			break;

		case CommandLineParser::opClientNoOperation:
			return;
	}

	exit(ok ? 0 : 1);
}

void RustyGet::ProcessWebGet()
{
	WebDownloader downloader;
	downloader.SetUrl(m_commandLineParser->GetLastArg());
	downloader.SetForce(true);
	downloader.SetRetry(false);
	downloader.SetOutputFilename(m_commandLineParser->GetWebGetFilename());
	downloader.SetInfoName("WebGet");

	WebDownloader::EStatus status = downloader.DownloadWithRedirects(5);
	exit(nzbget_rs_ccm_nzbget_webget_exit_code((int)status, (int)WebDownloader::adFinished));
}

void RustyGet::ProcessSigVerify()
{
#ifndef DISABLE_TLS
	bool ok = Maintenance::VerifySignature(m_commandLineParser->GetLastArg(),
		m_commandLineParser->GetSigFilename(), m_commandLineParser->GetPubKeyFilename());
	exit(nzbget_rs_ccm_nzbget_sigverify_exit_code(ok ? 1 : 0));
#else
	printf("ERROR: Could not verify signature, the program was compiled without OpenSSL support\n");
	exit(1);
#endif
}

void RustyGet::Stop(bool reload)
{
	if (!reload)
	{
		{
			std::lock_guard<std::mutex> reloadGuard(g_reloadMutex);
			g_reloadPending = false;
			g_reloadCond.notify_all();
		}
		nzbget_rs_signal_notify(SIGTERM);
	}

	{
		std::lock_guard<std::mutex> guard(m_waitMutex);
		if (m_stopped && !m_reloading && reload)
		{
			debug("Ignoring reload request because shutdown is already in progress");
			return;
		}
		m_reloading = reload;
	}

	if (!m_reloading)
	{
		g_SigHupPending.store(false);
		info("Stopping, please wait...");
	}

	if (m_reloading && m_prePostProcessor && m_prePostProcessor->HasActiveUnpack(false))
	{
		info("Waiting for active unpack to complete before in-process reload...");
		while (m_prePostProcessor && m_prePostProcessor->HasActiveUnpack(false) && !g_SigExitPending.load())
		{
			Util::Sleep(100);
		}
	}
	if (g_SigExitPending.exchange(false))
	{
		m_reloading = false;
		nzbget_rs_signal_notify(SIGTERM);
	}
	if (m_commandLineParser->GetRemoteClientMode())
	{
		if (m_frontend)
		{
			debug("Stopping Frontend");
			m_frontend->Stop();
		}
	}
	else
	{
		if (m_queueCoordinator)
		{
			debug("Stopping QueueCoordinator");
			m_serviceCoordinator->Stop();
			m_queueCoordinator->Stop();
			m_urlCoordinator->Stop();
			m_dupeSearch->Stop();
			m_prePostProcessor->Stop();
			m_feedCoordinator->Stop();
			m_articleCache->Stop();
			if (m_memoryMonitor)
			{
				m_memoryMonitor->Stop();
			}
			m_queueScriptCoordinator->Stop();
		}
	}

	// trigger stop/reload signal
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_stopped = true;
	m_waitCond.notify_all();
}

void RustyGet::PrintOptions()
{
	for (Options::OptEntry& optEntry : g_Options->GuardOptEntries())
	{
		printf("%s = \"%s\"\n", optEntry.GetName(), optEntry.GetValue());
	}
	exit(0);
}

void RustyGet::Daemonize()
{
	int f = fork();
	if (f < 0) exit(1); /* fork error */
	if (f > 0) exit(0); /* parent exits */

	/* child (daemon) continues */
	setsid();

	int lockOwned = 0;
	int warnTruncate = 0;
	int err = nzbget_rs_daemonize_setup(
		m_options->GetLockFile(),
		m_options->GetDaemonUsername(),
		&lockOwned,
		&warnTruncate);
	if (err == 1)
	{
		error("Starting daemon failed: could not create lock-file %s", m_options->GetLockFile());
		exit(1);
	}
	if (err == 2)
	{
		error("Starting daemon failed: lock-file %s is not a regular file", m_options->GetLockFile());
		exit(1);
	}
	if (err == 3)
	{
		error("Starting daemon failed: could not acquire lock on lock-file %s", m_options->GetLockFile());
		exit(1);
	}
	if (err == 4)
	{
		error("Starting daemon failed: invalid DaemonUsername");
		exit(1);
	}
	if (err == 5)
	{
		error("Starting daemon failed: could not drop privileges");
		exit(1);
	}
	if (lockOwned != 0)
	{
		g_LockFileOwned = true;
	}
	if (warnTruncate != 0)
	{
		warn("Could not truncate lock-file %s", m_options->GetLockFile());
	}
}

void RustyGet::AddNewsServer(int id, bool active, const char* name, const char* host,
	int port, int ipVersion, const char* user, const char* pass, bool joinGroup, bool tls,
	const char* cipher, int maxConnections, int retention, int level, int group, bool optional,
	unsigned int certVerificationfLevel)
{
	m_serverPool->AddServer(std::make_unique<NewsServer>(id, active, name, host, port, ipVersion, user, pass, joinGroup,
		tls, cipher, maxConnections, retention, level, group, optional, certVerificationfLevel));
}

void RustyGet::AddFeed(int id, const char* name, const char* url, int interval, const char* filter,
	bool backlog, bool pauseNzb, const char* category, FeedInfo::CategorySource categorySource, int priority, const char* feedScript,
	unsigned int certVeriflevel)
{
	m_feedCoordinator->AddFeed(std::make_unique<FeedInfo>(id, name, url, backlog, interval, filter,
		pauseNzb, category, categorySource, priority, feedScript, certVeriflevel));
}

void RustyGet::AddTask(int id, int hours, int minutes, int weekDaysBits,
	Options::ESchedulerCommand command, const char* param)
{
	m_scheduler->AddTask(std::make_unique<Scheduler::Task>(id, hours, minutes, weekDaysBits,
		(Scheduler::ECommand)command, param));
}

void Reload()
{
	if (!g_RustyGet || g_RustyGet->GetReloading() || g_RustyGet->GetStopped() || g_SigExitPending.load())
	{
		debug("Daemon is already reloading or stopping");
		return;
	}

	auto now = std::chrono::steady_clock::now();
	std::unique_lock<std::mutex> lock(g_reloadMutex);

	constexpr auto DEBOUNCE_DELAY = std::chrono::milliseconds(100);
	constexpr auto MAX_BURST_DELAY = std::chrono::milliseconds(1500);

	if (!g_reloadPending)
	{
		g_reloadCond.wait(lock, [] { return !g_reloadWorkerRunning; });
		if (!g_RustyGet || g_RustyGet->GetReloading() || g_RustyGet->GetStopped() || g_SigExitPending.load())
		{
			debug("Daemon is already reloading or stopping");
			return;
		}
		if (g_reloadThread.joinable())
		{
			std::thread finishedThread = std::move(g_reloadThread);
			lock.unlock();
			finishedThread.join();
			lock.lock();
			if (g_reloadPending || g_reloadWorkerRunning ||
				!g_RustyGet || g_RustyGet->GetReloading() || g_RustyGet->GetStopped() || g_SigExitPending.load())
			{
				return;
			}
		}

		g_reloadPending = true;
		g_reloadWorkerRunning = true;
		g_reloadFirstRequestTime = now;
		g_reloadTargetTime = now + DEBOUNCE_DELAY;

		g_reloadThread = std::thread([]() {
			std::unique_lock<std::mutex> lock(g_reloadMutex);

			while (g_reloadPending && !g_SigExitPending.load())
			{
				auto now = std::chrono::steady_clock::now();
				if (now >= g_reloadTargetTime ||
					now - g_reloadFirstRequestTime >= std::chrono::milliseconds(1500))
				{
					break;
				}

				g_reloadCond.wait_for(lock, g_reloadTargetTime - now);
			}

			if (!g_reloadPending && !g_SigExitPending.load())
			{
				g_reloadWorkerRunning = false;
				g_reloadCond.notify_all();
				return;
			}

			bool loggedDeferral = false;
			while (g_reloadPending && !g_SigExitPending.load())
			{
				lock.unlock();
				bool activeUnpack = g_PrePostProcessor && g_PrePostProcessor->HasActiveUnpack(false);
				bool alreadyReloading = !g_RustyGet || g_RustyGet->GetReloading();
				if (activeUnpack && !loggedDeferral)
				{
					info("Unpack is actively executing; deferring reload until unpack completes...");
					loggedDeferral = true;
				}
				lock.lock();
				if (!g_reloadPending || g_SigExitPending.load() || !activeUnpack || alreadyReloading)
				{
					break;
				}
				g_reloadCond.wait_for(lock, std::chrono::milliseconds(100));
			}

			if (g_SigExitPending.exchange(false))
			{
				g_reloadPending = false;
				lock.unlock();
				ExitProc();
				lock.lock();
				g_reloadWorkerRunning = false;
				g_reloadCond.notify_all();
				return;
			}

			if (!g_reloadPending)
			{
				g_reloadWorkerRunning = false;
				g_reloadCond.notify_all();
				return;
			}

			g_reloadPending = false;
			lock.unlock();

			if (g_RustyGet && !g_RustyGet->GetReloading())
			{
				info("Reloading...");
				g_RustyGet->Stop(true);
			}

			lock.lock();
			g_reloadWorkerRunning = false;
			g_reloadCond.notify_all();
		});
	}
	else
	{
		int64 elapsedMs = std::chrono::duration_cast<std::chrono::milliseconds>(
			now - g_reloadFirstRequestTime).count();
		int64 clampedMs = nzbget_rs_roc_nzbget_reload_clamp_debounce_ms(elapsedMs, 100, 1500);
		g_reloadTargetTime = g_reloadFirstRequestTime + std::chrono::milliseconds(clampedMs);
		g_reloadCond.notify_one();
		debug("Reload request debounced, target extended");
	}
}

void ExitProc()
{
	{
		std::lock_guard<std::mutex> guard(g_reloadMutex);
		g_reloadPending = false;
		g_reloadCond.notify_all();
	}
	nzbget_rs_signal_notify(SIGTERM);

	if (g_RustyGet)
	{
		g_RustyGet->Stop(false);
	}
}

extern "C" {

void nzbget_c_set_process_args(int argc, char* argv[], char* argp[])
{
	g_ArgumentCount = argc;
	g_Arguments = (char*(*)[])argv;
	g_EnvironmentVariables = (char*(*)[])argp;
}

void nzbget_c_util_init(void)
{
	Util::Init();
}

int nzbget_c_nserv_main(int argc, char* argv[])
{
#ifndef DISABLE_NSERV
	return NServMain(argc, argv);
#else
	(void)argc; (void)argv;
	printf("ERROR: Could not start NServ, the program was compiled without NServ\n");
	return 1;
#endif
}

int nzbget_c_win_service_check(int argc, char* argv[])
{
	(void)argc; (void)argv;
	return 0;
}

int nzbget_c_phase1_init(bool reloading)
{
	static bool atexitRegistered = false;
	if (!atexitRegistered)
	{
		atexit([] { g_RustyGet.reset(); });
		atexitRegistered = true;
	}

	g_RustyGet = std::make_unique<RustyGet>();
	return g_RustyGet->Phase1_Init(reloading);
}

int nzbget_c_phase2_subsystems_create_and_recover(bool reloading)
{
	return g_RustyGet ? g_RustyGet->Phase2_SubsystemsAndState(reloading) : 0;
}

int nzbget_c_phase3_server_activate(void)
{
	return g_RustyGet ? g_RustyGet->Phase3_ServerActivation() : 0;
}

void nzbget_c_phase4_shutdown(bool reloading)
{
	if (g_RustyGet)
	{
		g_RustyGet->Phase4_Shutdown(reloading);
	}
	g_RustyGet.reset();
}

bool nzbget_c_has_active_unpack(void)
{
	return g_RustyGet ? g_RustyGet->HasActiveUnpack() : (g_PrePostProcessor ? g_PrePostProcessor->HasActiveUnpack(false) : false);
}

bool nzbget_c_is_reloading(void)
{
	return g_RustyGet ? g_RustyGet->GetReloading() : false;
}

void nzbget_c_stop_server(bool reloading)
{
	if (g_RustyGet)
	{
		g_RustyGet->Stop(reloading);
	}
}

void nzbget_c_reload(void)
{
	Reload();
}

void nzbget_c_exit_proc(void)
{
	ExitProc();
}

}

