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
	env.conn_cb = +[](uintptr_t rawConn, int op, int a1, int a2, const char* s1, const char* s2, char* outBuf) -> uintptr_t {
		if (op == 0)
		{
			auto* conn = new Connection(s1 ? s1 : "", a1, s2 != nullptr);
			conn->SetTimeout(10);
			conn->SetSuppressErrors(false);
			return reinterpret_cast<uintptr_t>(conn);
		}
		auto* conn = reinterpret_cast<Connection*>(rawConn);
		if (!conn) return 0;
		if (op == 1) return conn->Bind() ? 1 : 0;
		if (op == 2) return reinterpret_cast<uintptr_t>(conn->Accept().release());
		if (op == 3) { conn->SetSuppressErrors(true); conn->Cancel(); return 1; }
		if (op == 4)
		{
			if (a1 == 1) conn->SetGracefull(true);
			if (a1 == 1 || a1 == 2 || a2 == 1) conn->Disconnect();
			delete conn;
#ifndef DISABLE_TLS
			if (a2 == 1) OpenSSL::StopSSLThread();
#endif
			return 0;
		}
		if (op == 5)
		{
			conn->SetSuppressErrors(false);
#ifndef DISABLE_TLS
			if (s1 && !conn->StartTls(false, s1, s2)) return 0;
#endif
			const char* host = conn->GetHost();
			if (outBuf && a1 > 0) snprintf(outBuf, static_cast<size_t>(a1), "%s", host ? host : "");
			return 1;
		}
		if (op == 6)
		{
			int bytesRead = 0;
			return conn->ReadLine(outBuf, a1, &bytesRead) ? 1 : 0;
		}
		if (op == 7)
		{
			if (a2 != 0) conn->WriteLine(s1 ? s1 : "");
			else if (s1 && a1 > 0) conn->Send(s1, a1);
			return 1;
		}
		return 0;
	};
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

static int SigOp(int sub)
{
	return nzbget_rs_daemon_service_op(8, sub, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
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
		return m_waitingRequiredDir ? 1 :
			g_Options->GetDiskSpace() > 0 && g_WorkState->GetDownloading() ? 1 :
			Service::Sleep;
	}

	void ServiceWork() override
	{
		debug("Disk service work");

		if (g_Options->GetDiskSpace() > 0 && g_WorkState->GetDownloading())
		{
			debug("Disk service work: check disk space");
			int destLow = 0, interLow = 0;
			nzbget_rs_daemon_service_op(2, g_Options->GetDiskSpace(), 0, 0, 0, 0,
				g_Options->GetDestDir(), g_Options->GetInterDir(), &destLow, &interLow, nullptr, nullptr);
			if (destLow)
			{
				warn("Low disk space on %s. Pausing download", g_Options->GetDestDir());
				g_WorkState->SetPauseDownload(true);
			}
			if (interLow)
			{
				warn("Low disk space on %s. Pausing download", g_Options->GetInterDir());
				g_WorkState->SetPauseDownload(true);
			}
		}

		if (m_waitingRequiredDir)
		{
			debug("Disk service work: check required dir");
			int nextReported = m_waitingReported ? 1 : 0;
			if (!nzbget_rs_daemon_service_op(3, m_waitingReported ? 1 : 0, 0, 0, 0, 0,
				g_Options->GetRequiredDir(), nullptr, &nextReported, nullptr, nullptr,
				[](void*, const char* dir) { info("Waiting for required directory %s", dir); }))
			{
				m_waitingReported = nextReported != 0;
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
	}

	void Update(Subject* caller, void* aspect) override
	{
		(void)caller; (void)aspect;
		WakeUp();
	}

private:
	bool m_waitingRequiredDir = true;
	bool m_waitingReported = false;
};

// the daemon holds the lock on the lock-file: only then is it ours to delete
// at shutdown. Not a member: a reload makes a new NZBGet without daemonizing,
// and a second daemon that fails to get the lock must leave it in place.
static bool g_LockFileOwned = false;

class RustyGet final : public Options::Extender
{
public:
	~RustyGet() override
	{
		Cleanup();
	}

	bool GetReloading() { return m_reloading; }
	bool GetStopped() const { return m_stopped; }

	int Phase1_Init(bool reload)
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

		FileSystem::uMask = static_cast<mode_t>(nzbget_rs_daemon_service_op(
			7, m_options->GetUMask(), 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr));

#ifdef DEBUG
		debug("Using %o umask", FileSystem::uMask);
#endif

		m_scanner->InitOptions();
		m_queueScriptCoordinator->InitOptions();
#ifndef DISABLE_TLS
		TlsSocket::InitOptions(g_Options->GetCertCheck() ? g_Options->GetCertStore() : "");
#endif

		ProcessDirect();

		if (g_Options->GetServerMode() && g_Options->GetSystemHealthCheck())
		{
			m_systemHealth = std::make_unique<SystemHealth::Service>(
				*g_Options,
				*g_ServerPool->GetServers(),
				*g_FeedCoordinator,
				m_scheduler->GetTasks(),
				*g_Log);
			g_SystemHealth = m_systemHealth.get();

			m_systemHealth->DiagnoseAndLog();
		}

		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 5))
		{
			if (!m_reloading)
			{
				int f = fork();
				if (f < 0) exit(1); /* fork error */
				if (f > 0) exit(0); /* parent exits */

				/* child (daemon) continues */
				setsid();

				int owned = 0, truncFailed = 0;
				int err = nzbget_rs_daemon_service_op(1, 0, 0, 0, 0, 0,
					m_options->GetLockFile(), m_options->GetDaemonUsername(), &owned, &truncFailed, nullptr, nullptr);
				if (err != 0)
				{
					const char* msg = err == 1 ? "could not create lock-file %s" :
						err == 2 ? "lock-file %s is not a regular file" :
						err == 3 ? "could not acquire lock on lock-file %s" :
						err == 4 ? "invalid DaemonUsername" : "could not drop privileges";
					error("%s", *(err <= 3 ? BString<1024>("Starting daemon failed: %s", *BString<1024>(msg, m_options->GetLockFile()))
						: BString<1024>("Starting daemon failed: %s", msg)));
					exit(1);
				}
				g_LockFileOwned = owned != 0;
				if (truncFailed)
				{
					warn("Could not truncate lock-file %s", m_options->GetLockFile());
				}
			}
			info("RustyGet %s daemon-mode", Util::VersionRevision());
		}
		else if (m_options->GetServerMode())
		{
			info("RustyGet %s server-mode", Util::VersionRevision());
		}
		else if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6))
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

		int crashTrace = 0;
#ifdef DEBUG
		crashTrace = g_Options->GetCrashTrace() ? 1 : 0;
#endif
		nzbget_rs_daemon_service_op(0, g_Options->GetCrashDump() ? 1 : 0, crashTrace, 0, 0, 0,
			nullptr, nullptr, nullptr, nullptr, nullptr, [](void*, const char* m) { error("%s", m); });

		return 0;
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

		if (!nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6))
		{
			if (!nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 4) && !SigOp(0))
			{
				const char* rawCat = nzbget_rs_cmdline_get_str(m_cmdLine.get(), 34);
				const char* category = rawCat ? rawCat : "";
				const char* argFilename = nzbget_rs_cmdline_get_str(m_cmdLine.get(), 36);
				NzbFile nzbFile(argFilename, category);
				if (!nzbFile.Parse())
				{
					printf("Parsing NZB-document %s failed\n\n", argFilename ? argFilename : "N/A");
				}
				else
				{
					std::unique_ptr<NzbInfo> nzbInfo = nzbFile.DetachNzbInfo();
					if (!nzbFile.GetPassword().empty())
					{
						nzbInfo->GetParameters()->SetParameter("*Unpack:Password", nzbFile.GetPassword().c_str());
					}
					m_scanner->InitPPParameters(category, nzbInfo->GetParameters(), false);
					m_queueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), nullptr, false);
				}
			}

			DoMainLoop();
		}
		else
		{
			if (SigOp(1))
			{
				ExitProc();
			}
			while (m_frontend && !m_stopped && !m_reloading && m_frontend->IsRunning())
			{
				if (SigOp(1))
				{
					ExitProc();
				}
				else if (SigOp(2))
				{
					Reload();
				}
				Util::Sleep(50);
			}
		}

		return 0;
	}

	void Phase4_Shutdown(bool reload)
	{
		m_reloading = reload;
		nzbget_rs_lifecycle_reload_op(2, nullptr);
		ScriptController::TerminateAll();

		StopRemoteServer();
		StopFrontend();
		nzbget_rs_lifecycle_reload_op(2, nullptr);

		TlsCipherBackend::Shutdown();
		if (!m_reloading)
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
		if (!reload)
		{
			nzbget_rs_lifecycle_reload_op(1, nullptr);
			SigOp(5);
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
			SigOp(3);
			info("Stopping, please wait...");
		}

		if (m_reloading && m_prePostProcessor && m_prePostProcessor->HasActiveUnpack(false))
		{
			info("Waiting for active unpack to complete before in-process reload...");
			while (m_prePostProcessor && m_prePostProcessor->HasActiveUnpack(false) && !SigOp(0))
			{
				Util::Sleep(100);
			}
		}
		if (SigOp(1))
		{
			m_reloading = false;
			SigOp(5);
		}
		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6))
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
		m_scheduler->AddTask(std::make_unique<Scheduler::Task>(id, hours, minutes, weekDaysBits,
			(Scheduler::ECommand)command, param));
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

	bool m_reloading = false;
	bool m_stopped = false;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;

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
			if (buf.data)
			{
				printf("%s", buf.data);
			}
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
			for (Options::OptEntry& optEntry : g_Options->GuardOptEntries())
			{
				printf("%s = \"%s\"\n", optEntry.GetName(), optEntry.GetValue());
			}
			exit(0);
		}
	}

	void StartRemoteServer()
	{
		if (!m_options->GetServerMode())
		{
			return;
		}

		NzbgetRsWebServeEnv webInit{};
		webInit.log_cb = [](void*, int, const char* m) { error("%s", m); };
		nzbget_rs_web_serve(0, &webInit);
		m_remoteServer = std::make_unique<RemoteServer>(false);
		m_remoteServer->Start();

		if (nzbget_rs_daemon_service_op(4, m_options->GetSecureControl() ? 1 : 0, 0, 0, 0, 0,
			m_options->GetControlIp(), nullptr, nullptr, nullptr, nullptr, nullptr))
		{
			m_remoteSecureServer = std::make_unique<RemoteServer>(true);
			m_remoteSecureServer->Start();
		}
	}

	void StopRemoteServer()
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

		if (m_frontend)
		{
			m_frontend->Start();
		}
	}

	void StopFrontend()
	{
		if (m_frontend)
		{
			if (!nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 6) || m_stopped)
			{
				debug("Stopping Frontend");
				m_frontend->Stop();
			}
			while (m_frontend->IsRunning())
			{
				if (SigOp(1))
				{
					ExitProc();
				}
				Util::Sleep(50);
			}
			debug("Frontend stopped");
		}
	}

	void DoMainLoop()
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
			if (SigOp(1))
			{
				ExitProc();
			}
			else if (SigOp(2))
			{
				Reload();
			}

			if (nzbget_rs_daemon_service_op(5, m_options->GetServerMode() ? 1 : 0,
				m_queueCoordinator->HasMoreJobs() ? 1 : 0, m_urlCoordinator->HasMoreJobs() ? 1 : 0,
				m_prePostProcessor->HasMoreJobs() ? 1 : 0, 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr))
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
							return m_stopped || SigOp(4);
						});
					}

					if (SigOp(1))
					{
						ExitProc();
					}
					else if (SigOp(2))
					{
						Reload();
					}
				}
			}
		}

		debug("Main program loop terminated");
	}

	void ProcessClientRequest()
	{
		if (nzbget_rs_cmdline_get_flag(m_cmdLine.get(), 7) == 0)
		{
			return;
		}
		bool ok = nzbget_rs_cmdline_run_client(
			m_cmdLine.get(),
			m_options->GetControlIp(),
			m_options->GetControlPort(),
			m_options->GetControlUsername(),
			m_options->GetControlPassword(),
			[](void*, const char* ptr, size_t len) {
				if (ptr && len > 0) fwrite(ptr, 1, len, stdout);
			},
			[](void*, const char* msg) { ::perror(msg); },
			nullptr) != 0;
		exit(ok ? 0 : 1);
	}
};

std::unique_ptr<RustyGet> g_RustyGet;

void Reload()
{
	nzbget_rs_lifecycle_reload_op(0, [](int action) -> int {
		switch (action)
		{
			case 0:
				return (!g_RustyGet || g_RustyGet->GetReloading() || g_RustyGet->GetStopped() || SigOp(0)) ? 1 : 0;
			case 1:
				debug("Daemon is already reloading or stopping");
				return 0;
			case 2:
				debug("Reload request debounced, target extended");
				return 0;
			case 3:
				return SigOp(0);
			case 4:
				return ((g_PrePostProcessor && g_PrePostProcessor->HasActiveUnpack(false)) ? 1 : 0) |
					((!g_RustyGet || g_RustyGet->GetReloading()) ? 2 : 0);
			case 5:
				info("Unpack is actively executing; deferring reload until unpack completes...");
				return 0;
			case 6:
				return SigOp(1);
			case 7:
				ExitProc();
				return 0;
			case 8:
				if (g_RustyGet && !g_RustyGet->GetReloading())
				{
					info("Reloading...");
					g_RustyGet->Stop(true);
				}
				return 0;
			default:
				return 0;
		}
	});
}

void ExitProc()
{
	nzbget_rs_lifecycle_reload_op(1, nullptr);
	SigOp(5);

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

