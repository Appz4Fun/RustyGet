/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
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
#include "TestFramework.h"

extern char** environ;
char* (*g_EnvironmentVariables)[];
int g_ArgumentCount;
char* (*g_Arguments)[];
QueueCoordinator* g_QueueCoordinator;
UrlCoordinator* g_UrlCoordinator;
StatMeter* g_StatMeter;
HistoryCoordinator* g_HistoryCoordinator;
DupeCoordinator* g_DupeCoordinator;
DiskState* g_DiskState;
Scanner* g_Scanner;
FeedCoordinator* g_FeedCoordinator;
Maintenance* g_Maintenance;
ArticleCache* g_ArticleCache;
System::SystemInfo* g_SystemInfo;
SystemHealth::Service* g_SystemHealth;

void Reload(){}
void ExitProc(){}

// An options set built from command-line style entries, active while it lives
// (the daemon builds its own in rust/src/daemon.rs). The set that was active
// before is active again afterwards.
class TestOptions final
{
public:
	typedef std::vector<const char*> CmdOptList;

	explicit TestOptions(CmdOptList* cmdOpts) : m_previous(nzbget_rs_options_active())
	{
		int stateFlags = 0;
		m_rs = nzbget_rs_options_new("rustyget/rustyget", nullptr, 1, 1,
			cmdOpts && !cmdOpts->empty() ? cmdOpts->data() : nullptr,
			cmdOpts ? cmdOpts->size() : 0,
			32 | 8, Util::VersionRevision(), nullptr, nullptr, nullptr, &stateFlags);
	}

	~TestOptions()
	{
		nzbget_rs_options_free(m_rs);
		nzbget_rs_options_set_active(m_previous);
	}

	TestOptions(const TestOptions&) = delete;
	TestOptions& operator=(const TestOptions&) = delete;

private:
	NzbgetRsOptions* m_rs = nullptr;
	const NzbgetRsOptions* m_previous = nullptr;
};

// The pause flags the post-processing tests toggle (rust/src/ffi.rs work state).
class TestWorkState final
{
public:
	TestWorkState() : m_rs(nzbget_rs_workstate_new()) {}
	~TestWorkState() { nzbget_rs_workstate_free(m_rs); }

	void SetTempPausePostprocess(bool v) { nzbget_rs_workstate_ctl(m_rs, 5, 1, v ? 1 : 0); }
	bool GetTempPausePostprocess() const { return nzbget_rs_workstate_ctl(m_rs, 5, 0, 0) != 0; }

private:
	NzbgetRsWorkState* m_rs = nullptr;
};

TestWorkState* g_WorkState;

struct InitGlobals
{
	TestOptions::CmdOptList m_cmdOpts;
	NzbgetRsLog* m_log = nullptr;
	TestOptions* m_options = nullptr;
	NzbgetRsServerPool* m_serverPool = nullptr;

	InitGlobals()
	{
#ifdef DEBUG
		m_log = nzbget_rs_log_new(1);
#else
		m_log = nzbget_rs_log_new(0);
#endif
		m_options = new TestOptions(&m_cmdOpts);
		g_EnvironmentVariables = (char*(*)[])environ;
		g_ArgumentCount = 0;
		g_Arguments = nullptr;
		g_WorkState = new TestWorkState();
		g_DiskState = new DiskState();
		InitRsBuildEnv();
		m_serverPool = nzbget_rs_serverpool_new();
		g_Scanner = new Scanner();
	}

	~InitGlobals()
	{
		nzbget_rs_serverpool_free(m_serverPool);
		nzbget_rs_log_free(m_log);
		delete m_options;
		delete g_WorkState;
		delete g_DiskState;
		delete g_Scanner;
	}
};

BOOST_GLOBAL_FIXTURE(InitGlobals);

#include "postprocess/DirectUnpackTest.h"
#include "queue/LiveParSourceClaimTest.h"
// DirectUnpackTest.h opens the PostprocessTest suite; ParCheckerTest.h used to close it.
BOOST_AUTO_TEST_SUITE_END()

int main(int argc, char* argv[])
{
	return TestFramework::RunAll(argc, argv);
}
