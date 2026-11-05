/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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
#include "PostScript.h"
#include "Log.h"
#include "Util.h"
#include "Options.h"
#include "WorkState.h"
#include "nzbget_rs.h"

static const int POSTPROCESS_PARCHECK = 92;
static const int POSTPROCESS_SUCCESS = 93;
static const int POSTPROCESS_ERROR = 94;
static const int POSTPROCESS_NONE = 95;

void PostScriptController::StartJob(PostInfo* postInfo)
{
	if (!postInfo)
	{
		return;
	}

	if (postInfo->GetNzbInfo() && postInfo->GetNzbInfo()->GetSkipScriptProcessing())
	{
		return;
	}

	PostScriptController* scriptController = new PostScriptController();
	scriptController->m_postInfo = postInfo;
	scriptController->SetAutoDestroy(false);
	scriptController->m_prefixLen = 0;

	postInfo->SetPostThread(scriptController);

	scriptController->Start();
}

void PostScriptController::Run()
{
	StringBuilder scriptCommaList;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		size_t pcount = m_postInfo->GetNzbInfo()->GetParameters()->size();
		std::vector<const char*> pnames;
		std::vector<const char*> pvals;
		pnames.reserve(pcount);
		pvals.reserve(pcount);
		for (NzbParameter& parameter : m_postInfo->GetNzbInfo()->GetParameters())
		{
			pnames.push_back(parameter.GetName());
			pvals.push_back(parameter.GetValue());
		}
		NzbgetRsBuf listBuf = nzbget_rs_sus_build_enabled_script_comma_list(pnames.data(), pvals.data(), pcount);
		if (listBuf.data)
		{
			scriptCommaList.Append(listBuf.data);
		}
		nzbget_rs_free(listBuf);
		m_postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
	}

	ExecuteScriptList(scriptCommaList);

	m_postInfo->SetStage(PostInfo::ptFinished);
	m_postInfo->SetWorking(false);
}

void PostScriptController::ExecuteScript(std::shared_ptr<const Extension::Script> script)
{
	// if any script has requested par-check, do not execute other scripts
	if (!script->GetPostScript() || m_postInfo->GetRequestParCheck())
	{
		return;
	}

	m_script = std::move(script);

	PrintMessage(Message::mkInfo, "Executing post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());

	BString<1024> progressLabel("Executing post-process-script %s", m_script->GetName());
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		m_postInfo->SetProgressLabel(progressLabel);
	}

	SetArgs({m_script->GetEntry()});

	BString<1024> infoName("post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());
	SetInfoName(infoName);

	SetLogPrefix(m_script->GetDisplayName());
	m_prefixLen = static_cast<int>(strlen(m_script->GetDisplayName())) + 2; // 2 = strlen(": ");
	PrepareParams(m_script->GetName());

	int exitCode = Execute();

	infoName[0] = 'P';

	SetLogPrefix(nullptr);
	ScriptStatus::EStatus status = AnalyseExitCode(exitCode, infoName);

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		m_postInfo->GetNzbInfo()->GetScriptStatuses()->emplace_back(m_script->GetName(), status);
	}
}

void PostScriptController::PrepareParams(const char* scriptName)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();

	ResetEnv();

	NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
	BString<1024> status = nzbInfo->MakeTextStatus(true);
	std::vector<int> serverIds;
	std::vector<int> serverSuccess;
	std::vector<int> serverFailed;
	serverIds.reserve(nzbInfo->GetServerStats()->size());
	serverSuccess.reserve(nzbInfo->GetServerStats()->size());
	serverFailed.reserve(nzbInfo->GetServerStats()->size());
	for (ServerStat& serverStat : nzbInfo->GetServerStats())
	{
		serverIds.push_back(serverStat.GetServerId());
		serverSuccess.push_back(serverStat.GetSuccessArticles());
		serverFailed.push_back(serverStat.GetFailedArticles());
	}

	NzbgetRsPostScriptEnv env{};
	env.nzb_id = nzbInfo->GetId();
	env.nzb_name = nzbInfo->GetName();
	env.dest_dir = nzbInfo->GetDestDir();
	env.filename = nzbInfo->GetFilename();
	env.queued_filename = nzbInfo->GetQueuedFilename();
	env.url = nzbInfo->GetUrl();
	env.final_dir = nzbInfo->GetFinalDir();
	env.category = nzbInfo->GetCategory();
	env.health = nzbInfo->CalcHealth();
	env.critical_health = nzbInfo->CalcCriticalHealth(false);
	env.dupe_key = nzbInfo->GetDupeKey();
	env.dupe_score = nzbInfo->GetDupeScore();
	env.dupe_mode = static_cast<int>(nzbInfo->GetDupeMode());
	env.status = status;
	env.script_status = static_cast<int>(nzbInfo->GetScriptStatuses()->CalcTotalStatus());
	env.par_status = static_cast<int>(nzbInfo->GetParStatus());
	env.unpack_status = static_cast<int>(nzbInfo->GetUnpackStatus());
	env.delete_status = static_cast<int>(nzbInfo->GetDeleteStatus());
	env.mark_status = static_cast<int>(nzbInfo->GetMarkStatus());
	env.total_articles = nzbInfo->GetTotalArticles();
	env.success_articles = nzbInfo->GetSuccessArticles();
	env.failed_articles = nzbInfo->GetFailedArticles();
	env.server_ids = serverIds.data();
	env.server_success_articles = serverSuccess.data();
	env.server_failed_articles = serverFailed.data();
	env.server_count = serverIds.size();

	nzbget_rs_post_script_prepare_env(
		&env,
		this,
		[](void* ctx, const char* name, const char* value)
		{
			static_cast<PostScriptController*>(ctx)->SetEnvVar(name, value);
		},
		[](void* ctx, const char* name, int value)
		{
			static_cast<PostScriptController*>(ctx)->SetIntEnvVar(name, value);
		});

	PrepareEnvScript(nzbInfo->GetParameters(), scriptName);
}

ScriptStatus::EStatus PostScriptController::AnalyseExitCode(int exitCode, const char* upInfoName)
{
	// The ScriptStatus is accumulated for all scripts:
	// If any script has failed the status is "failure", etc.

#ifndef DISABLE_PARCHECK
	constexpr int parcheckEnabled = 1;
#else
	constexpr int parcheckEnabled = 0;
#endif
	int msgAction = 0;
	int reqParCheck = 0;
	int status = nzbget_rs_sus_analyse_post_script_exit_code(
		exitCode,
		static_cast<int>(m_postInfo->GetNzbInfo()->GetParStatus()),
		static_cast<int>(NzbInfo::psSkipped),
		parcheckEnabled,
		static_cast<int>(ScriptStatus::srNone),
		static_cast<int>(ScriptStatus::srFailure),
		static_cast<int>(ScriptStatus::srSuccess),
		&msgAction,
		&reqParCheck);
	switch (msgAction)
	{
		case 1:
			PrintMessage(Message::mkInfo, "%s successful", upInfoName);
			break;
		case 2:
			PrintMessage(Message::mkError, "%s failed", upInfoName);
			break;
		case 3:
			PrintMessage(Message::mkInfo, "%s skipped", upInfoName);
			break;
		case 4:
			PrintMessage(Message::mkError, "%s requested par-check/repair, but the collection was already checked", upInfoName);
			break;
		case 5:
			PrintMessage(Message::mkInfo, "%s requested par-check/repair", upInfoName);
			break;
		default:
			PrintMessage(Message::mkError, "%s failed (terminated with unknown status)", upInfoName);
			break;
	}
	if (reqParCheck)
	{
		m_postInfo->SetRequestParCheck(true);
		m_postInfo->SetForceRepair(true);
	}
	return static_cast<ScriptStatus::EStatus>(status);
}

void PostScriptController::AddMessage(Message::EKind kind, const char* text)
{
	const char* msgText = text + m_prefixLen;

	NzbgetRsPostScriptCmd cmd{};
	int cmdKind = nzbget_rs_sus_parse_post_script_cmd(msgText, &cmd);
	if (cmdKind != 0)
	{
		debug("Command %s detected", msgText + 6);
		if (cmdKind == 1)
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->SetFinalDir(msgText + cmd.val_offset);
		}
		else if (cmdKind == 2)
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->SetDestDir(msgText + cmd.val_offset);
		}
		else if (cmdKind == 3)
		{
			std::string param(msgText + cmd.key_offset, cmd.key_len);
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->GetParameters()->SetParameter(param.c_str(), msgText + cmd.val_offset);
		}
		else if (cmdKind == 4)
		{
			SetLogPrefix(nullptr);
			PrintMessage(Message::mkWarning, "Marking %s as bad", m_postInfo->GetNzbInfo()->GetName());
			SetLogPrefix(m_script->GetDisplayName());
			m_postInfo->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
		}
		else
		{
			m_postInfo->GetNzbInfo()->PrintMessage(Message::mkError,
				"Invalid command \"%s\" received from %s", msgText, GetInfoName());
		}
	}
	else
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		m_postInfo->SetProgressLabel(text);
	}

	if (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority())
	{
		time_t stageTime = m_postInfo->GetStageTime();
		time_t startTime = m_postInfo->GetStartTime();
		time_t waitTime = Util::CurrentTime();

		while (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority() && !IsStopped())
		{
			Util::Sleep(100);

			time_t delta = Util::CurrentTime() - waitTime;
			if (stageTime > 0)
			{
				m_postInfo->SetStageTime(stageTime + delta);
			}
			if (startTime > 0)
			{
				m_postInfo->SetStartTime(startTime + delta);
			}
		}
	}
}

void PostScriptController::Stop()
{
	debug("Stopping post-process-script");
	Thread::Stop();
	Terminate();
}
