/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2015-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "FeedScript.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

static const int FEED_SUCCESS = 93;

void FeedScriptController::ExecuteScripts(const char* feedScript, const char* feedFile, int feedId, bool* success)
{
	FeedScriptController scriptController;
	scriptController.m_feedFile = feedFile;
	scriptController.m_feedId = feedId;

	scriptController.ExecuteScriptList(feedScript);

	if (success)
	{
		*success = scriptController.m_success;
	}
}

void FeedScriptController::ExecuteScript(std::shared_ptr<const Extension::Script> script)
{
	if (!script->GetFeedScript())
	{
		return;
	}

	PrintMessage(Message::mkInfo, "Executing feed-script %s for Feed%i", script->GetName(), m_feedId);

	SetArgs({script->GetEntry()});

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_feed_script_format_info_name(
		script->GetName(), m_feedId, 0);
	BString<1024> infoName("%s", infoNameBuf.data ? infoNameBuf.data : "");
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	SetLogPrefix(script->GetDisplayName());
	PrepareParams(script->GetName());

	int exitCode = Execute();

	if (nzbget_rs_euh_feed_script_is_success(exitCode) == 0)
	{
		NzbgetRsBuf upInfoNameBuf = nzbget_rs_tpr_feed_script_format_info_name(
			script->GetName(), m_feedId, 1);
		PrintMessage(Message::mkError, "%s failed", upInfoNameBuf.data ? upInfoNameBuf.data : "");
		nzbget_rs_free(upInfoNameBuf);
		m_success = false;
	}

	SetLogPrefix(nullptr);
}

void FeedScriptController::PrepareParams(const char* scriptName)
{
	ResetEnv();

	SetEnvVar("NZBFP_FILENAME", m_feedFile);
	SetIntEnvVar("NZBFP_FEEDID", m_feedId);

	PrepareEnvScript(nullptr, scriptName);
}
