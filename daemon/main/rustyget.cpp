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
	} ctx{&log, nullptr};

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
	env.runtime_cb = +[](void* raw, int op, int, const char* const* opts, size_t optLen) {
		auto* c = static_cast<NServCtx*>(raw);
		if (op == 0)
		{
			Options::CmdOptList cmdOpts;
			for (size_t i = 0; i < optLen; ++i) cmdOpts.push_back(opts[i]);
			c->options = std::make_unique<Options>(&cmdOpts, nullptr);
			c->log->InitOptions();
			debug("Initializing global connection data");
#ifndef DISABLE_TLS
			TlsSocket::Init();
#endif
			signal(SIGPIPE, SIG_IGN);
		}
		else if (op == 3) { while (getchar()) Util::Sleep(200); }
	};
	env.tls_host_cb = &TlsHostCb;
#ifdef DEBUG
	env.debug_on = 1;
#endif
	return nzbget_rs_nserv_run(argc, const_cast<const char* const*>(argv), &env);
}
#endif

// Globals
Log* g_Log;
Options* g_Options;
WorkState* g_WorkState;
ServerPool* g_ServerPool;
QueueCoordinator* g_QueueCoordinator;
PrePostProcessor* g_PrePostProcessor;
DiskState* g_DiskState;
ArticleCache* g_ArticleCache;
QueueScriptCoordinator* g_QueueScriptCoordinator;

int g_ArgumentCount;
char* (*g_EnvironmentVariables)[] = nullptr;
char* (*g_Arguments)[] = nullptr;

extern void ExitProc();
extern void Reload();

static int DaemonReloadCb(int sub);

class RustyGet final
{
public:
	~RustyGet() { Cleanup(); }

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
			debug("Initializing global connection data");
#ifndef DISABLE_TLS
			TlsSocket::Init();
#endif
		}

		CreateGlobals();

		NzbgetRsDaemonEnv cenv{};
		cenv.a1 = g_ArgumentCount;
		cenv.ctx = (g_Arguments && *g_Arguments) ? *g_Arguments : nullptr;
		int rc = nzbget_rs_daemon_exec(13, &cenv);
		if (rc == -1) throw std::invalid_argument("Invalid command line");
		if (rc < 0) throw std::runtime_error("Command line parsing failed");

		m_options = std::make_unique<Options>();
		(void)nzbget_rs_daemon_exec(14, &cenv);

#ifndef DISABLE_TLS
		TlsSocket::InitOptions(g_Options->GetCertCheck() ? g_Options->GetCertStore() : "");
#endif

		NzbgetRsDaemonEnv act{};
		act.ctx = this;
		act.a1 = -1;
		act.segv_cb = +[](void*, const char* m) { error("%s", m); };
		act.action_cb = &RustyGet::DaemonActionCb;
		return nzbget_rs_daemon_exec(5, &act);
	}

	int Phase3_ServerActivation()
	{
		if (m_options->GetServerMode())
		{
			const NzbgetRsRpcHost rpcHost = BuildRpcHost();
			nzbget_rs_remote_srv_ctl(0, &rpcHost);
		}

		NzbgetRsDaemonEnv env{};
		env.ctx = this;
		env.a1 = -1;
		env.reload_cb = DaemonReloadCb;
		env.action_cb = &RustyGet::DaemonActionCb;
		return nzbget_rs_daemon_exec(7, &env);
	}

	void Phase4_Shutdown(bool reload)
	{
		NzbgetRsDaemonEnv env{};
		env.ctx = this;
		env.a1 = reload ? 1 : 0;
		env.a2 = -1;
		env.action_cb = &RustyGet::DaemonActionCb;
		nzbget_rs_daemon_exec(11, &env);

		TlsCipherBackend::Shutdown();
		if (!GetReloading())
		{
#ifndef DISABLE_TLS
			TlsSocket::Final();
#endif
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
		env.a2 = -1;
		env.action_cb = &RustyGet::DaemonActionCb;
		nzbget_rs_daemon_exec(8, &env);
	}

private:
	// globals
	std::unique_ptr<Log> m_log;
	std::unique_ptr<Options> m_options;
	std::unique_ptr<WorkState> m_workState;
	std::unique_ptr<ServerPool> m_serverPool;
	std::unique_ptr<QueueCoordinator> m_queueCoordinator;
	std::unique_ptr<PrePostProcessor> m_prePostProcessor;
	std::unique_ptr<DiskState> m_diskState;
	std::unique_ptr<ArticleCache> m_articleCache;
	std::unique_ptr<QueueScriptCoordinator> m_queueScriptCoordinator;

	void CreateGlobals()
	{
		m_workState = std::make_unique<WorkState>();
		g_WorkState = m_workState.get();

		m_serverPool = std::make_unique<ServerPool>();
		g_ServerPool = m_serverPool.get();

		m_queueCoordinator = std::make_unique<QueueCoordinator>();
		g_QueueCoordinator = m_queueCoordinator.get();

		m_prePostProcessor = std::make_unique<PrePostProcessor>();
		g_PrePostProcessor = m_prePostProcessor.get();

		m_articleCache = std::make_unique<ArticleCache>();
		g_ArticleCache = m_articleCache.get();

		m_queueScriptCoordinator = std::make_unique<QueueScriptCoordinator>();
		g_QueueScriptCoordinator = m_queueScriptCoordinator.get();

		m_diskState = std::make_unique<DiskState>();
		g_DiskState = m_diskState.get();
	}

	void Cleanup()
	{
		NzbgetRsDaemonEnv env{};
		env.a1 = m_options ? 1 : 0;
		nzbget_rs_daemon_exec(12, &env);

		g_PrePostProcessor = nullptr;
		g_QueueCoordinator = nullptr;
		g_DiskState = nullptr;
		g_ServerPool = nullptr;
		g_ArticleCache = nullptr;
		g_QueueScriptCoordinator = nullptr;
	}

	static int DaemonActionCb(void* raw, int sub, int a2, const char*, const char*)
	{
		auto* self = static_cast<RustyGet*>(raw);
		switch (sub)
		{
		case 4:
			self->m_queueCoordinator->Start();
			self->m_prePostProcessor->Start();
			if (self->m_options->GetArticleCache() > 0) self->m_articleCache->Start();
			return 0;
		case 5:
			if (a2 == 0)
				return (self->m_queueCoordinator->IsRunning() ||
					self->m_prePostProcessor->IsRunning() ||
					self->m_articleCache->IsRunning()) ? 1 : 0;
			return (self->m_queueCoordinator->HasMoreJobs() ? 1 : 0) |
				(self->m_prePostProcessor->HasMoreJobs() ? 4 : 0);
		case 6:
			if (!self->m_queueCoordinator->IsStopped()) self->m_queueCoordinator->Stop();
			if (!self->m_prePostProcessor->IsStopped()) self->m_prePostProcessor->Stop();
			if (!self->m_articleCache->IsStopped()) self->m_articleCache->Stop();
			return 0;
		case 7:
			return (self->m_prePostProcessor && self->m_prePostProcessor->HasActiveUnpack(false)) ? 1 : 0;
		case 8:
			if (a2 == 0 && self->m_queueCoordinator)
			{
				debug("Stopping QueueCoordinator");
				self->m_queueCoordinator->Stop();
				self->m_prePostProcessor->Stop();
				self->m_articleCache->Stop();
				self->m_queueScriptCoordinator->Stop();
			}
			return 0;
		case 10:
			ExitProc();
			return 0;
		default:
			return 0;
		}
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
	env.reload_cb = DaemonReloadCb;
	nzbget_rs_daemon_exec(9, &env);
}

void ExitProc()
{
	NzbgetRsDaemonEnv env{};
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

