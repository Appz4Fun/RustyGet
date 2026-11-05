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
	if (nzbget_rs_sfc_post_script_should_skip_execute(
		script->GetPostScript() ? 1 : 0,
		m_postInfo->GetRequestParCheck() ? 1 : 0) != 0)
	{
		return;
	}

	m_script = std::move(script);

	PrintMessage(Message::mkInfo, "Executing post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());

	NzbgetRsBuf progressLabelBuf = nzbget_rs_tpr_post_script_format_progress_label(m_script->GetName());
	BString<1024> progressLabel("%s", progressLabelBuf.data ? progressLabelBuf.data : "");
	nzbget_rs_free(progressLabelBuf);

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		m_postInfo->SetProgressLabel(progressLabel);
	}

	SetArgs({m_script->GetEntry()});

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_script_format_info_name(
		m_script->GetName(), m_postInfo->GetNzbInfo()->GetName(), 0);
	BString<1024> infoName("%s", infoNameBuf.data ? infoNameBuf.data : "");
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	
	SetLogPrefix(m_script->GetDisplayName());
	m_prefixLen = nzbget_rs_sfc_script_ctrl_calc_prefix_len(strlen(m_script->GetDisplayName()));
	PrepareParams(m_script->GetName());

	int exitCode = Execute();

	NzbgetRsBuf upInfoNameBuf = nzbget_rs_tpr_post_script_format_info_name(
		m_script->GetName(), m_postInfo->GetNzbInfo()->GetName(), 1);
	infoName.Format("%s", upInfoNameBuf.data ? upInfoNameBuf.data : "");
	nzbget_rs_free(upInfoNameBuf);

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

	SetIntEnvVar("NZBPP_NZBID", m_postInfo->GetNzbInfo()->GetId());
	SetEnvVar("NZBPP_NZBNAME", m_postInfo->GetNzbInfo()->GetName());
	SetEnvVar("NZBPP_DIRECTORY", m_postInfo->GetNzbInfo()->GetDestDir());
	SetEnvVar("NZBPP_NZBFILENAME", m_postInfo->GetNzbInfo()->GetFilename());
	SetEnvVar("NZBPP_QUEUEDFILE", m_postInfo->GetNzbInfo()->GetQueuedFilename());
	SetEnvVar("NZBPP_URL", m_postInfo->GetNzbInfo()->GetUrl());
	SetEnvVar("NZBPP_FINALDIR", m_postInfo->GetNzbInfo()->GetFinalDir());
	SetEnvVar("NZBPP_CATEGORY", m_postInfo->GetNzbInfo()->GetCategory());
	SetIntEnvVar("NZBPP_HEALTH", m_postInfo->GetNzbInfo()->CalcHealth());
	SetIntEnvVar("NZBPP_CRITICALHEALTH", m_postInfo->GetNzbInfo()->CalcCriticalHealth(false));

	SetEnvVar("NZBPP_DUPEKEY", m_postInfo->GetNzbInfo()->GetDupeKey());
	SetIntEnvVar("NZBPP_DUPESCORE", m_postInfo->GetNzbInfo()->GetDupeScore());

	SetEnvVar("NZBPP_DUPEMODE",
		nzbget_rs_tpr_script_dupe_mode_name((int)m_postInfo->GetNzbInfo()->GetDupeMode()));

	BString<1024> status = m_postInfo->GetNzbInfo()->MakeTextStatus(true);
	SetEnvVar("NZBPP_STATUS", status);

	size_t totalStatusLen = nzbget_rs_tpr_post_script_total_status_len(status);
	status[totalStatusLen] = '\0';
	SetEnvVar("NZBPP_TOTALSTATUS", status);

	const char* scriptStatusName[] = { "NONE", "FAILURE", "SUCCESS" };
	SetEnvVar("NZBPP_SCRIPTSTATUS", scriptStatusName[m_postInfo->GetNzbInfo()->GetScriptStatuses()->CalcTotalStatus()]);

	// deprecated
	int parStatusCodes[] = { 0, 0, 1, 2, 3, 4 };
	NzbInfo::EParStatus parStatus = (NzbInfo::EParStatus)nzbget_rs_sfc_post_script_resolve_compat_par_status(
		(int)m_postInfo->GetNzbInfo()->GetParStatus(),
		(int)NzbInfo::psFailure,
		(int)m_postInfo->GetNzbInfo()->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)m_postInfo->GetNzbInfo()->GetMarkStatus(),
		(int)NzbInfo::ksBad);
	SetIntEnvVar("NZBPP_PARSTATUS", parStatusCodes[parStatus]);

	// deprecated
	int unpackStatus[] = { 0, 0, 1, 2, 3, 4 };
	SetIntEnvVar("NZBPP_UNPACKSTATUS", unpackStatus[m_postInfo->GetNzbInfo()->GetUnpackStatus()]);

	// deprecated
	SetIntEnvVar("NZBPP_HEALTHDELETED", (int)m_postInfo->GetNzbInfo()->GetDeleteStatus() == NzbInfo::dsHealth);

	SetIntEnvVar("NZBPP_TOTALARTICLES", (int)m_postInfo->GetNzbInfo()->GetTotalArticles());
	SetIntEnvVar("NZBPP_SUCCESSARTICLES", (int)m_postInfo->GetNzbInfo()->GetSuccessArticles());
	SetIntEnvVar("NZBPP_FAILEDARTICLES", (int)m_postInfo->GetNzbInfo()->GetFailedArticles());

	for (ServerStat& serverStat : m_postInfo->GetNzbInfo()->GetServerStats())
	{
		NzbgetRsBuf succNameBuf = nzbget_rs_tpr_post_script_format_server_env_name(
			serverStat.GetServerId(), 0);
		SetIntEnvVar(succNameBuf.data ? succNameBuf.data : "", serverStat.GetSuccessArticles());
		nzbget_rs_free(succNameBuf);

		NzbgetRsBuf failNameBuf = nzbget_rs_tpr_post_script_format_server_env_name(
			serverStat.GetServerId(), 1);
		SetIntEnvVar(failNameBuf.data ? failNameBuf.data : "", serverStat.GetFailedArticles());
		nzbget_rs_free(failNameBuf);
	}

	PrepareEnvScript(m_postInfo->GetNzbInfo()->GetParameters(), scriptName);
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

	if (nzbget_rs_sfc_post_script_is_paused_wait_active(
		g_WorkState->GetPausePostProcess() ? 1 : 0,
		m_postInfo->GetNzbInfo()->GetForcePriority() ? 1 : 0,
		0) != 0)
	{
		time_t stageTime = m_postInfo->GetStageTime();
		time_t startTime = m_postInfo->GetStartTime();
		time_t waitTime = Util::CurrentTime();

		while (nzbget_rs_sfc_post_script_is_paused_wait_active(
			g_WorkState->GetPausePostProcess() ? 1 : 0,
			m_postInfo->GetNzbInfo()->GetForcePriority() ? 1 : 0,
			IsStopped() ? 1 : 0) != 0)
		{
			Util::Sleep(100);

			int64_t newStage = 0;
			int64_t newStart = 0;
			nzbget_rs_sfc_post_script_calc_paused_timestamps(
				(int64_t)Util::CurrentTime(), (int64_t)waitTime,
				(int64_t)stageTime, (int64_t)startTime,
				&newStage, &newStart);
			if (stageTime > 0)
			{
				m_postInfo->SetStageTime((time_t)newStage);
			}
			if (startTime > 0)
			{
				m_postInfo->SetStartTime((time_t)newStart);
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
