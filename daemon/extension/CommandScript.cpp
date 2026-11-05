/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.net>
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
#include "CommandScript.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

static const int COMMANDPROCESS_SUCCESS = 93;
static const int COMMANDPROCESS_ERROR = 94;

bool CommandScriptController::StartScript(const char* scriptName, const char* command,
	std::unique_ptr<Options::OptEntries> modifiedOptions)
{
	CommandScriptController* scriptController = new CommandScriptController();
	scriptController->m_script = scriptName;
	scriptController->m_command = command;
	scriptController->m_logId = g_CommandScriptLog->Reset();
	scriptController->m_modifiedOptions = std::move(modifiedOptions);

	scriptController->SetAutoDestroy(true);

	scriptController->Start();

	auto found = g_ExtensionManager->FindIf([&](auto script)
		{
			return strcmp(scriptName, script->GetName()) == 0;
		}
	);
	
	return found.has_value();
}

void CommandScriptController::Run()
{
	ExecuteScriptList(m_script);
}

void CommandScriptController::ExecuteScript(std::shared_ptr<const Extension::Script> script)
{
	PrintMessage(Message::mkInfo, "Executing script %s with command %s", script->GetName(), *m_command);

	SetArgs({script->GetEntry()});

	NzbgetRsBuf infoBuf = nzbget_rs_dqh_cmd_script_format_info_name(
		script->GetName(), *m_command, 0);
	BString<1024> infoName;
	infoName.Set(infoBuf.data ? infoBuf.data : "", (int)infoBuf.len);
	nzbget_rs_free(infoBuf);
	SetInfoName(infoName);

	SetLogPrefix(script->GetDisplayName());
	PrepareParams(script->GetName());

	int exitCode = Execute();

	NzbgetRsBuf upInfoBuf = nzbget_rs_dqh_cmd_script_format_info_name(
		script->GetName(), *m_command, 1);
	infoName.Set(upInfoBuf.data ? upInfoBuf.data : "", (int)upInfoBuf.len);
	nzbget_rs_free(upInfoBuf);
	SetLogPrefix(nullptr);

	switch (nzbget_rs_euh_cmd_script_classify_exit_code(exitCode))
	{
		case 0:
			PrintMessage(Message::mkInfo, "%s successful", *infoName);
			break;
		case 1:
			PrintMessage(Message::mkError, "%s failed", *infoName);
			break;
		default:
			PrintMessage(Message::mkError, "%s failed (terminated with unknown status)", *infoName);
			break;
	}
}

void CommandScriptController::PrepareParams(const char* scriptName)
{
	ResetEnv();

	SetEnvVar("NZBCP_COMMAND", m_command);

	PrepareEnvScript(nullptr, scriptName);
}

const char* CommandScriptController::GetOptValue(const char* name, const char* value)
{
	Options::OptEntry* entry = m_modifiedOptions->FindOption(name);
	return entry ? entry->GetValue() : value;
}

void CommandScriptController::AddMessage(Message::EKind kind, const char * text)
{
	NzbScriptController::AddMessage(kind, text);
	g_CommandScriptLog->AddMessage(m_logId, kind, text);
}


int CommandScriptLog::Reset()
{
	Guard guard(m_logMutex);
	m_messages.clear();
	return ++m_idScriptGen;
}

void CommandScriptLog::AddMessage(int scriptId, Message::EKind kind, const char * text)
{
	Guard guard(m_logMutex);

	// save only messages from the last started script
	if (nzbget_rs_sfc_cmd_script_should_append_log_message(scriptId, m_idScriptGen) != 0)
	{
		m_messages.emplace_back(++m_idMessageGen, kind, Util::CurrentTime(), text);
	}
}
