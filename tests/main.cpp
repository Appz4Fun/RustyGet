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

#include "nzbget.h"

#include "TestFramework.h"

#include "ServerPool.h"
#include "Log.h"
#include "Options.h"
#include "QueueCoordinator.h"
#include "UrlCoordinator.h"
#include "DiskState.h"
#include "PrePostProcessor.h"
#include "HistoryCoordinator.h"
#include "DupeCoordinator.h"
#include "Scanner.h"
#include "FeedCoordinator.h"
#include "Maintenance.h"
#include "ArticleWriter.h"
#include "StatMeter.h"
#include "QueueScript.h"
#include "ExtensionManager.h"
#include "Scheduler.h"

extern char** environ;
char* (*g_EnvironmentVariables)[];
int g_ArgumentCount;
char* (*g_Arguments)[];
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

void Reload(){}
void ExitProc(){}

struct InitGlobals
{
	Options::CmdOptList m_cmdOpts;

	InitGlobals()
	{
		g_Log = new Log();
		g_Options = new Options(&m_cmdOpts, nullptr);
		g_EnvironmentVariables = (char*(*)[])environ;
		g_ArgumentCount = 0;
		g_Arguments = nullptr;
		g_WorkState = new WorkState();
		g_DiskState = new DiskState();
		g_ServerPool = new ServerPool();
		g_ServiceCoordinator = new ServiceCoordinator();
		g_Scanner = new Scanner();
		g_ExtensionManager = new ExtensionManager::Manager();
	}

	~InitGlobals()
	{
		delete g_ServerPool;
		delete g_Log;
		delete g_Options;
		delete g_WorkState;
		delete g_DiskState;
		delete g_Scanner;
		delete g_ServiceCoordinator;
		delete g_ExtensionManager;
	}
};

BOOST_GLOBAL_FIXTURE(InitGlobals);

#include "postprocess/DirectUnpackTest.h"
#include "postprocess/ParCheckerTest.h"
#include "connect/TlsCipherTest.h"

int main(int argc, char* argv[])
{
	return TestFramework::RunAll(argc, argv);
}
