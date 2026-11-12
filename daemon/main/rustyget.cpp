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


#include "Util.h"
#include "nzbget_rs.h"

#ifndef DISABLE_NSERV
inline int NServMain(int argc, char* argv[])
{
	Log log;
	struct NServCtx
	{
		Log* log;
		std::unique_ptr<Options> options;
		std::unique_ptr<Frontend> frontend;
	} ctx{&log, nullptr, nullptr};

	NzbgetRsNservEnv env{};
	env.ctx = &ctx;
	env.version = Util::VersionRevision();
	env.write_stdout_cb = +[](const char* data, size_t len) { fwrite(data, 1, len, stdout); };
	env.log_cb = +[](void*, int level, const char* msg) {
		const char* s = msg ? msg : "";
		if (level == 0) debug("%s", s);
		else if (level == 1) detail("%s", s);
		else if (level == 2) info("%s", s);
		else if (level == 3) warn("%s", s);
		else if (level == 4) error("%s", s);
	};
	env.runtime_cb = +[](void* raw, int op, int arg, const char* const* opts, size_t optLen) {
		auto* c = static_cast<NServCtx*>(raw);
		if (op == 0)
		{
			Options::CmdOptList cmdOpts;
			for (size_t i = 0; i < optLen; ++i) cmdOpts.push_back(opts[i]);
			c->options = std::make_unique<Options>(&cmdOpts, nullptr);
			c->log->InitOptions();
			Connection::Init();
#ifndef DISABLE_TLS
			TlsSocket::Init();
#endif
			signal(SIGPIPE, SIG_IGN);
			c->frontend = std::make_unique<Frontend>(3);
			c->frontend->Start();
		}
		else if (op == 1) { if (c->frontend) c->frontend->Stop(); }
		else if (op == 2) { while (c->frontend && c->frontend->IsRunning()) Util::Sleep(arg); }
		else if (op == 3) { while (getchar()) Util::Sleep(200); }
	};
	env.tls_host_cb = &Connection::HostCb;
#ifdef DEBUG
	env.debug_on = 1;
#endif
	return nzbget_rs_nserv_run(argc, const_cast<const char* const*>(argv), &env);
}
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

static void DaemonGlobalLogCb(int level, const char* msg)
{
	const char* s = msg ? msg : "";
	if (level == 0) debug("%s", s);
	else if (level == 1) detail("%s", s);
	else if (level == 2) info("%s", s);
	else if (level == 3) warn("%s", s);
	else if (level == 4) error("%s", s);
}

static void DaemonLogCb(void*, int level, const char* msg) { DaemonGlobalLogCb(level, msg); }
static int DaemonReloadCb(int sub);

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
	DiskService() { g_WorkState->Attach(this); }

protected:
	int ServiceInterval() override
	{
		NzbgetRsDaemonEnv env{};
		env.a1 = g_Options->GetDiskSpace();
		env.a2 = g_WorkState->GetDownloading() ? 1 : 0;
		return nzbget_rs_daemon_exec(0, &env);
	}

	void ServiceWork() override
	{
		NzbgetRsDaemonEnv env{};
		env.a1 = g_Options->GetDiskSpace();
		env.a2 = g_WorkState->GetDownloading() ? 1 : 0;
		env.s1 = g_Options->GetDestDir();
		env.s2 = g_Options->GetInterDir();
		env.s3 = g_Options->GetRequiredDir();
		env.log_cb = DaemonLogCb;
		env.action_cb = +[](void*, int sub, int, const char*, const char*) -> int {
			if (sub == 0) g_WorkState->SetPauseDownload(true);
			else if (sub == 1) { g_WorkState->SetTempPauseDownload(false); g_WorkState->SetTempPausePostprocess(false); }
			return 0;
		};
		nzbget_rs_daemon_exec(1, &env);
	}

	void Update(Subject* caller, void* aspect) override
	{
		(void)caller; (void)aspect;
		WakeUp();
	}
};

class RustyGet final : public Options::Extender
{
public:
	~RustyGet() override { Cleanup(); }

	bool GetReloading() const
	{
		NzbgetRsDaemonEnv env{};
		env.a1 = 0;
		return nzbget_rs_daemon_exec(3, &env) != 0;
	}

	int Phase1_Init(bool reload)
	{
		{
			NzbgetRsDaemonEnv rst{};
			rst.a1 = reload ? 1 : 0;
			nzbget_rs_daemon_exec(2, &rst);
		}
		m_log = std::make_unique<Log>();

		debug("RustyGet %s", Util::VersionRevision());

		if (!reload)
		{
			Connection::Init();
#ifndef DISABLE_TLS
			TlsSocket::Init();
#endif
		}

		CreateGlobals();
		BootConfig();

		if (m_memoryMonitor) m_memoryMonitor->InitOptions();

		{
			NzbgetRsDaemonEnv uenv{};
			uenv.a1 = m_options->GetUMask();
#ifdef DEBUG
			uenv.a2 = 1;
#endif
			uenv.log_cb = DaemonLogCb;
			FileSystem::uMask = static_cast<mode_t>(nzbget_rs_daemon_exec(4, &uenv));
		}

		m_scanner->InitOptions();
		m_queueScriptCoordinator->InitOptions();
#ifndef DISABLE_TLS
		TlsSocket::InitOptions(g_Options->GetCertCheck() ? g_Options->GetCertStore() : "");
#endif

		ProcessDirect();

		if (g_Options->GetServerMode() && g_Options->GetSystemHealthCheck())
		{
			m_systemHealth = std::make_unique<SystemHealth::Service>(
				*g_Options, *g_ServerPool->GetServers(), *g_FeedCoordinator, *m_scheduler, *g_Log);
			g_SystemHealth = m_systemHealth.get();
			m_systemHealth->DiagnoseAndLog();
		}

		NzbgetRsDaemonEnv act{};
		act.ctx = this;
		act.a1 = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 5);
		act.a2 = m_options->GetServerMode() ? 1 : 0;
		act.a3 = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6);
		act.a4 = g_Options->GetCrashDump() ? 1 : 0;
#ifdef DEBUG
		act.a5 = g_Options->GetCrashTrace() ? 1 : 0;
#endif
		act.a6 = m_options->GetControlPort();
		act.s1 = Util::VersionRevision();
		act.s2 = m_options->GetLockFile();
		act.s3 = m_options->GetDaemonUsername();
		act.s4 = m_options->GetConfigFilename();
		act.s5 = m_options->GetControlIp();
		act.segv_cb = +[](void*, const char* m) { error("%s", m); };
		act.log_cb = DaemonLogCb;
		act.action_cb = &RustyGet::DaemonActionCb;
		return nzbget_rs_daemon_exec(5, &act);
	}

	int Phase2_SubsystemsAndState(bool reload)
	{
		(void)reload;
		if (!nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6))
		{
			m_serverPool->InitConnections();
			m_statMeter->Init();
		}
		return 0;
	}

	int Phase3_ServerActivation()
	{
		StartRemoteServer();
		StartFrontend();

		NzbgetRsDaemonEnv env{};
		env.ctx = this;
		env.a1 = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6);
		env.a2 = m_options->GetServerMode() ? 1 : 0;
		env.a3 = !nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 4) ? 1 : 0;
		env.s1 = nzbget_rs_cmdline_get_str(m_cmdLine.get(), 34);
		env.s2 = nzbget_rs_cmdline_get_str(m_cmdLine.get(), 36);
		env.global_log_cb = DaemonGlobalLogCb;
		env.reload_cb = DaemonReloadCb;
		env.log_cb = DaemonLogCb;
		env.action_cb = &RustyGet::DaemonActionCb;
		return nzbget_rs_daemon_exec(7, &env);
	}

	void Phase4_Shutdown(bool reload)
	{
		NzbgetRsDaemonEnv env{};
		env.ctx = this;
		env.a1 = reload ? 1 : 0;
		env.a2 = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6);
		env.action_cb = &RustyGet::DaemonActionCb;
		nzbget_rs_daemon_exec(11, &env);

		TlsCipherBackend::Shutdown();
		if (!GetReloading())
		{
#ifndef DISABLE_TLS
			TlsSocket::Final();
#endif
			Connection::Final();
		}
	}

	bool HasActiveUnpack() const
	{
		return m_prePostProcessor ? m_prePostProcessor->HasActiveUnpack(false) : false;
	}

	void Stop(bool reload)
	{
		NzbgetRsDaemonEnv env{};
		env.ctx = this;
		env.a1 = reload ? 1 : 0;
		env.a2 = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6);
		env.log_cb = DaemonLogCb;
		env.action_cb = &RustyGet::DaemonActionCb;
		nzbget_rs_daemon_exec(8, &env);
	}

	// Options::Extender
	void AddNewsServer(int id, bool active, const char* name, const char* host,
		int port, int ipVersion, const char* user, const char* pass, bool joinGroup,
		bool tls, const char* cipher, int maxConnections, int retention,
		int level, int group, bool optional, unsigned int certVerificationfLevel) override
	{
		m_serverPool->AddServer(std::make_unique<NewsServer>(id, active, name, host, port, ipVersion, user, pass, joinGroup,
			tls, cipher, maxConnections, retention, level, group, optional, certVerificationfLevel));
	}

	void AddFeed(int id, const char* name, const char* url, int interval,
		const char* filter, bool backlog, bool pauseNzb, const char* category,
		int categorySource, int priority, const char* feedScript,
		unsigned int certVerifLevel) override
	{
		m_feedCoordinator->AddFeed(id, name, url, backlog, interval, filter,
			pauseNzb, category, categorySource, priority, feedScript, certVerifLevel);
	}

	void AddTask(int id, int hours, int minutes, int weekDaysBits,
		Options::ESchedulerCommand command, const char* param) override
	{
		m_scheduler->AddTask(id, hours, minutes, weekDaysBits, (Scheduler::ECommand)command, param);
	}

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
	std::unique_ptr<NzbgetRsCmdline, decltype(&nzbget_rs_cmdline_free)> m_cmdLine{nullptr, nzbget_rs_cmdline_free};

	void CreateGlobals()
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

	void BootConfig()
	{
		debug("Parsing command line");
		if (g_ArgumentCount < 1 || !g_Arguments || !*g_Arguments)
		{
			throw std::invalid_argument("Invalid command line");
		}
		m_cmdLine.reset(nzbget_rs_cmdline_new(
			g_ArgumentCount,
			(const char* const*)(*g_Arguments),
			[](void*, const char* ptr, size_t len) {
				if (ptr && len > 0) fwrite(ptr, 1, len, stdout);
			},
			nullptr));
		if (!m_cmdLine)
		{
			throw std::runtime_error("Command line parsing failed");
		}
		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 2))
		{
			printf("RustyGet version: %s\n", Util::VersionRevision());
			exit(0);
		}
		bool printUsage = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 1) != 0;
		if (printUsage || nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 23) || g_ArgumentCount <= 1)
		{
			NzbgetRsBuf buf = nzbget_rs_cmdline_format_usage(
				FileSystem::BaseFileName(((const char**)(*g_Arguments))[0]), PATH_SEPARATOR, 1);
			if (buf.data) printf("%s", buf.data);
			nzbget_rs_free(buf);
			exit(printUsage ? 0 : 1);
		}

		debug("Reading options");
		Options::CmdOptList cmdOptList;
		for (int i = 0; const char* opt = nzbget_rs_cmdline_get_str(m_cmdLine.get(), 1000 + i); i++)
		{
			cmdOptList.push_back(opt);
		}
		m_options = std::make_unique<Options>((*g_Arguments)[0],
			nzbget_rs_cmdline_get_str(m_cmdLine.get(), 30),
			nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 0) != 0,
			&cmdOptList, this);
		m_options->SetRemoteClientMode(nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6) != 0);
		m_options->SetServerMode(nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 4) != 0);
		m_options->SetDaemonMode(nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 5) != 0);
		bool testBacktrace = false;
#ifdef DEBUG
		testBacktrace = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 15) != 0;
#endif
		int clientOp = nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 7);
		m_options->SetDirectClientMode(
			testBacktrace ||
			nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 16) != 0 ||
			nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 17) != 0 ||
			clientOp != 0 ||
			nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 3) != 0);
		m_workState->SetPauseDownload(nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 22) != 0);
		m_workState->SetSpeedLimit(g_Options->GetDownloadRate());

		m_log->InitOptions();

		if (m_options->GetFatalError())
		{
			exit(1);
		}
		else if (m_options->GetConfigErrors() && clientOp == 0)
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

	void Cleanup()
	{
		NzbgetRsDaemonEnv env{};
		env.a1 = m_options ? 1 : 0;
		env.s1 = m_options ? m_options->GetLockFile() : nullptr;
		env.log_cb = DaemonLogCb;
		nzbget_rs_daemon_exec(12, &env);

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

	void ProcessDirect()
	{
#ifdef DEBUG
		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 15))
		{
			TestSegFault(); // never returns
		}
#endif

		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 16))
		{
			WebDownloader downloader;
			downloader.SetUrl(nzbget_rs_cmdline_get_str(m_cmdLine.get(), 35));
			downloader.SetForce(true);
			downloader.SetRetry(false);
			downloader.SetOutputFilename(nzbget_rs_cmdline_get_str(m_cmdLine.get(), 31));
			downloader.SetInfoName("WebGet");

			WebDownloader::EStatus status = downloader.DownloadWithRedirects(5);
			exit(status == WebDownloader::adFinished ? 0 : 1);
		}

		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 17))
		{
#ifndef DISABLE_TLS
			bool ok = Maintenance::VerifySignature(
				nzbget_rs_cmdline_get_str(m_cmdLine.get(), 35),
				nzbget_rs_cmdline_get_str(m_cmdLine.get(), 33),
				nzbget_rs_cmdline_get_str(m_cmdLine.get(), 32));
			exit(ok ? 93 : 1);
#else
			printf("ERROR: Could not verify signature, the program was compiled without OpenSSL support\n");
			exit(1);
#endif
		}

		// client request
		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 7) != 0)
		{
			ProcessClientRequest(); // never returns
		}

		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 3))
		{
			nzbget_rs_options_query(g_Options->RsHandle(), 1, nullptr, 0, nullptr, nullptr, 0,
				+[](uintptr_t, const char* n, const char* v, int) { printf("%s = \"%s\"\n", n, v); });
			exit(0);
		}
	}

	void StartRemoteServer()
	{
		if (!m_options->GetServerMode()) return;

		NzbgetRsWebServeEnv webInit{};
		webInit.log_cb = [](void*, int, const char* m) { error("%s", m); };
		nzbget_rs_web_serve(0, &webInit);
		m_remoteServer = std::make_unique<RemoteServer>(false);
		m_remoteServer->Start();

		NzbgetRsDaemonEnv senv{};
		senv.a1 = m_options->GetSecureControl() ? 1 : 0;
		senv.s1 = m_options->GetControlIp();
		if (nzbget_rs_daemon_exec(6, &senv))
		{
			m_remoteSecureServer = std::make_unique<RemoteServer>(true);
			m_remoteSecureServer->Start();
		}
	}

	void StopRemoteServer()
	{
		if (m_remoteServer) { debug("stopping RemoteServer"); m_remoteServer->Stop(); }
		if (m_remoteSecureServer) { debug("stopping RemoteSecureServer"); m_remoteSecureServer->Stop(); }
		while ((m_remoteServer && m_remoteServer->IsRunning()) ||
			(m_remoteSecureServer && m_remoteSecureServer->IsRunning()))
		{
			Util::Sleep(50);
		}
		debug("RemoteServer stopped");
	}

	void StartFrontend()
	{
		if (!nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 5))
		{
			switch (m_options->GetOutputMode())
			{
			case Options::omNCurses:
#ifndef DISABLE_CURSES
				m_frontend = std::make_unique<Frontend>(2);
				break;
#endif
			case Options::omColored:
				m_frontend = std::make_unique<Frontend>(1);
				break;
			case Options::omLoggable:
				m_frontend = std::make_unique<Frontend>(0);
				break;
			}
		}
		if (m_frontend) m_frontend->Start();
	}

	static int DaemonActionCb(void* raw, int sub, int a2, const char* s1, const char* s2)
	{
		auto* self = static_cast<RustyGet*>(raw);
		switch (sub)
		{
		case 2: {
#ifdef DEBUG
			std::stringstream ss;
			ss << *self->m_systemInfo;
			std::string line;
			while (std::getline(ss, line)) detail("%s", line.c_str());
#endif
			return 0;
		}
		case 3:
			return (self->m_frontend && self->m_frontend->IsRunning()) ? 1 : 0;
		case 4:
			self->m_queueCoordinator->Start();
			self->m_urlCoordinator->Start();
			self->m_dupeSearch->Start();
			self->m_prePostProcessor->Start();
			self->m_feedCoordinator->Start();
			self->m_serviceCoordinator->Start();
			if (self->m_options->GetArticleCache() > 0) self->m_articleCache->Start();
			if (self->m_memoryMonitor) self->m_memoryMonitor->Start();
			return 0;
		case 5:
			if (a2 == 0)
				return (self->m_queueCoordinator->IsRunning() || self->m_urlCoordinator->IsRunning() ||
					self->m_dupeSearch->IsRunning() || self->m_prePostProcessor->IsRunning() ||
					self->m_feedCoordinator->IsRunning() || self->m_serviceCoordinator->IsRunning() ||
					(self->m_memoryMonitor && self->m_memoryMonitor->IsRunning()) || self->m_articleCache->IsRunning()) ? 1 : 0;
			return (self->m_queueCoordinator->HasMoreJobs() ? 1 : 0) |
				(self->m_urlCoordinator->HasMoreJobs() ? 2 : 0) |
				(self->m_prePostProcessor->HasMoreJobs() ? 4 : 0);
		case 6:
			if (!self->m_queueCoordinator->IsStopped()) self->m_queueCoordinator->Stop();
			if (!self->m_urlCoordinator->IsStopped()) self->m_urlCoordinator->Stop();
			if (!self->m_dupeSearch->IsStopped()) self->m_dupeSearch->Stop();
			if (!self->m_prePostProcessor->IsStopped()) self->m_prePostProcessor->Stop();
			if (!self->m_feedCoordinator->IsStopped()) self->m_feedCoordinator->Stop();
			if (!self->m_articleCache->IsStopped()) self->m_articleCache->Stop();
			if (self->m_memoryMonitor && !self->m_memoryMonitor->IsStopped()) self->m_memoryMonitor->Stop();
			if (!self->m_serviceCoordinator->IsStopped()) self->m_serviceCoordinator->Stop();
			return 0;
		case 7:
			return (self->m_prePostProcessor && self->m_prePostProcessor->HasActiveUnpack(false)) ? 1 : 0;
		case 8:
			if (a2 != 0)
			{
				if (self->m_frontend) { debug("Stopping Frontend"); self->m_frontend->Stop(); }
			}
			else if (self->m_queueCoordinator)
			{
				debug("Stopping QueueCoordinator");
				self->m_serviceCoordinator->Stop();
				self->m_queueCoordinator->Stop();
				self->m_urlCoordinator->Stop();
				self->m_dupeSearch->Stop();
				self->m_prePostProcessor->Stop();
				self->m_feedCoordinator->Stop();
				self->m_articleCache->Stop();
				if (self->m_memoryMonitor) self->m_memoryMonitor->Stop();
				self->m_queueScriptCoordinator->Stop();
			}
			return 0;
		case 10:
			ExitProc();
			return 0;
		case 11: {
			const char* cat = s1 ? s1 : "";
			NzbFile nzbFile(s2, cat);
			if (!nzbFile.Parse()) printf("Parsing NZB-document %s failed\n\n", s2 ? s2 : "N/A");
			else
			{
				std::unique_ptr<NzbInfo> nzbInfo = nzbFile.DetachNzbInfo();
				if (!nzbFile.GetPassword().empty())
					nzbInfo->GetParameters()->SetParameter("*Unpack:Password", nzbFile.GetPassword().c_str());
				self->m_scanner->InitPPParameters(cat, nzbInfo->GetParameters(), false);
				self->m_queueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), nullptr, false);
			}
			return 0;
		}
		case 12:
			nzbget_rs_script_terminate_all(&ScriptHostCb);
			self->StopRemoteServer();
			if (self->m_frontend)
			{
				if (a2 != 0) { debug("Stopping Frontend"); self->m_frontend->Stop(); }
				while (self->m_frontend->IsRunning())
				{
					NzbgetRsDaemonEnv q{};
					q.a1 = 2;
					q.a2 = 1;
					if (nzbget_rs_daemon_exec(3, &q)) ExitProc();
					Util::Sleep(50);
				}
				debug("Frontend stopped");
			}
			return 0;
		default:
			return 0;
		}
	}

	void ProcessClientRequest()
	{
		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 7) == 0) return;
		bool ok = nzbget_rs_cmdline_run_client(
			m_cmdLine.get(),
			m_options->GetControlIp(),
			m_options->GetControlPort(),
			m_options->GetControlUsername(),
			m_options->GetControlPassword(),
			[](void*, const char* ptr, size_t len) { if (ptr && len > 0) fwrite(ptr, 1, len, stdout); },
			[](void*, const char* msg) { ::perror(msg); },
			nullptr) != 0;
		exit(ok ? 0 : 1);
	}
};

std::unique_ptr<RustyGet> g_RustyGet;

static int DaemonReloadCb(int sub)
{
	if (sub == 0) return (g_RustyGet ? 1 : 0) | ((g_PrePostProcessor && g_PrePostProcessor->HasActiveUnpack(false)) ? 2 : 0);
	if (sub == 1 && g_RustyGet) g_RustyGet->Stop(true);
	else if (sub == 3 && g_RustyGet) g_RustyGet->Stop(false);
	return 0;
}

void Reload()
{
	NzbgetRsDaemonEnv env{};
	env.global_log_cb = DaemonGlobalLogCb;
	env.reload_cb = DaemonReloadCb;
	nzbget_rs_daemon_exec(9, &env);
}

void ExitProc()
{
	NzbgetRsDaemonEnv env{};
	env.global_log_cb = DaemonGlobalLogCb;
	env.reload_cb = DaemonReloadCb;
	nzbget_rs_daemon_exec(10, &env);
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

