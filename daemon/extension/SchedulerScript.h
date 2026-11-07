/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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


#ifndef SCHEDULERSCRIPT_H
#define SCHEDULERSCRIPT_H

#include "NString.h"
#include "NzbScript.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

class SchedulerScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run()
	{
		if (m_externalProcess)
		{
			ExecuteExternalProcess();
		}
		else
		{
			ExecuteScriptList(m_script);
		}
	}

	static void StartScript(const char* param, bool externalProcess, int taskId)
	{
		std::vector<CString> argv;
		if (externalProcess && (argv = Util::SplitCommandLine(param)).empty())
		{
			error("Could not execute scheduled process-script, failed to parse command line: %s", param);
			return;
		}

		SchedulerScriptController* scriptController = new SchedulerScriptController();

		scriptController->m_externalProcess = externalProcess;
		scriptController->m_script = param;
		scriptController->m_taskId = taskId;

		if (externalProcess)
		{
			scriptController->SetArgs(std::move(argv));
		}

		scriptController->SetAutoDestroy(true);

		scriptController->Start();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		if (!script->GetSchedulerScript())
		{
			return;
		}

		BString<1024> taskName;
		if (m_taskId > 0)
		{
			taskName.Format(" for Task%i", m_taskId);
		}
		else
		{
			taskName = "";
		}

		PrintMessage(Message::mkInfo, "Executing scheduler-script %s%s", script->GetName(), *taskName);

		SetArgs({script->GetEntry()});

		BString<1024> infoName("scheduler-script %s%s", script->GetName(), *taskName);
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		PrepareParams(script->GetName());

		Execute();

		SetLogPrefix(nullptr);
	}

private:
	CString m_script;
	bool m_externalProcess;
	int m_taskId;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		SetIntEnvVar("NZBSP_TASKID", m_taskId);

		PrepareEnvScript(nullptr, scriptName);
	}

	void ExecuteExternalProcess()
	{
		info("Executing scheduled process-script %s for Task%i", FileSystem::BaseFileName(GetScript()), m_taskId);

		BString<1024> infoName("scheduled process-script %s for Task%i", FileSystem::BaseFileName(GetScript()), m_taskId);
		SetInfoName(infoName);

		BString<1024> logPrefix = FileSystem::BaseFileName(GetScript());
		if (char* ext = strrchr(logPrefix, '.')) *ext = '\0';
		SetLogPrefix(logPrefix);

		Execute();
	}
};

#endif
