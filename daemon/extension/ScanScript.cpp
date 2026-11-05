/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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
#include "ScanScript.h"
#include "Scanner.h"
#include "Options.h"
#include "Log.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

class ScanScriptCheck : public NzbScriptController
{
protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script) { has |= script->GetScanScript(); }
	bool has = false;
	friend class ScanScriptController;
};


bool ScanScriptController::HasScripts()
{
	ScanScriptCheck check;
	check.ExecuteScriptList(g_Options->GetExtensions());
	// a category's extensions replace the global ones for its nzbs
	for (Options::Category& category : g_Options->GetCategories())
	{
		check.ExecuteScriptList(category.GetExtensions());
	}
	return check.has;
}

void ScanScriptController::ExecuteScripts(
	const char* nzbFilename,
	NzbInfo* nzbInfo,
	const char* directory,
	std::string& nzbName,
	std::string& category,
	int* priority,
	NzbParameterList* parameters,
	bool* addTop,
	bool* addPaused,
	std::string& dupeKey,
	int* dupeScore,
	EDupeMode* dupeMode)
{
	if (nzbInfo && nzbInfo->GetSkipScriptProcessing())
	{
		return;
	}

	ScanScriptController scriptController(nzbName, category, dupeKey);
	scriptController.m_nzbFilename = nzbFilename;
	scriptController.m_url = nzbInfo ? nzbInfo->GetUrl() : "";
	scriptController.m_directory = directory;
	scriptController.m_parameters = parameters;
	scriptController.m_priority = priority;
	scriptController.m_addTop = addTop;
	scriptController.m_addPaused = addPaused;
	scriptController.m_dupeScore = dupeScore;
	scriptController.m_dupeMode = dupeMode;
	scriptController.m_prefixLen = 0;

	const char* extensions = g_Options->GetExtensions();

	if (!category.empty())
	{
		Options::Category* categoryObj = g_Options->FindCategory(category.c_str(), false);
		if (categoryObj && !Util::EmptyStr(categoryObj->GetExtensions()))
		{
			extensions = categoryObj->GetExtensions();
		}
	}

	scriptController.ExecuteScriptList(extensions);
}

void ScanScriptController::ExecuteScript(std::shared_ptr<const Extension::Script> script)
{
	if (!script->GetScanScript() || !FileSystem::FileExists(m_nzbFilename))
	{
		return;
	}

	PrintMessage(Message::mkInfo, "Executing scan-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbFilename));

	SetArgs({script->GetEntry()});

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_scan_script_format_info_name(
		script->GetName(), FileSystem::BaseFileName(m_nzbFilename));
	BString<1024> infoName("%s", infoNameBuf.data ? infoNameBuf.data : "");
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	SetLogPrefix(script->GetDisplayName());
	m_prefixLen = nzbget_rs_sfc_script_ctrl_calc_prefix_len(strlen(script->GetDisplayName()));
	PrepareParams(script->GetName());

	Execute();

	SetLogPrefix(nullptr);
}

void ScanScriptController::PrepareParams(const char* scriptName)
{
	ResetEnv();

	SetEnvVar("NZBNP_FILENAME", m_nzbFilename);
	SetEnvVar("NZBNP_URL", m_url);
	SetEnvVar("NZBNP_NZBNAME", !m_nzbName.empty() ? m_nzbName.c_str() : FileSystem::BaseFileName(m_nzbFilename));
	SetEnvVar("NZBNP_CATEGORY", m_category.c_str());
	SetIntEnvVar("NZBNP_PRIORITY", *m_priority);
	SetIntEnvVar("NZBNP_TOP", *m_addTop ? 1 : 0);
	SetIntEnvVar("NZBNP_PAUSED", *m_addPaused ? 1 : 0);
	SetEnvVar("NZBNP_DUPEKEY", m_dupeKey.c_str());
	SetIntEnvVar("NZBNP_DUPESCORE", *m_dupeScore);

	SetEnvVar("NZBNP_DUPEMODE", nzbget_rs_tpr_script_dupe_mode_name((int)*m_dupeMode));

	// remove trailing slash
	BString<1024> dir = m_directory;
	size_t trimmedLen = nzbget_rs_sfc_scan_script_trim_trailing_sep_len(dir, PATH_SEPARATOR);
	dir[trimmedLen] = '\0';
	SetEnvVar("NZBNP_DIRECTORY", dir);

	PrepareEnvScript(m_parameters, scriptName);
}

void ScanScriptController::AddMessage(Message::EKind kind, const char* text)
{
	const char* msgText = text + m_prefixLen;

	NzbgetRsScanScriptCmd cmd{};
	int cmdKind = nzbget_rs_sus_parse_scan_script_cmd(msgText, &cmd);
	if (cmdKind != 0)
	{
		debug("Command %s detected", msgText + 6);
		switch (cmdKind)
		{
			case 1:
				m_nzbName = msgText + cmd.val_offset;
				break;
			case 2:
				m_category = msgText + cmd.val_offset;
				g_Scanner->InitPPParameters(m_category.c_str(), m_parameters, true);
				break;
			case 3:
			{
				std::string param(msgText + cmd.key_offset, cmd.key_len);
				m_parameters->SetParameter(param.c_str(), msgText + cmd.val_offset);
				break;
			}
			case 4:
				*m_priority = cmd.int_val;
				break;
			case 5:
				*m_addTop = cmd.int_val != 0;
				break;
			case 6:
				*m_addPaused = cmd.int_val != 0;
				break;
			case 7:
				m_dupeKey = msgText + cmd.val_offset;
				break;
			case 8:
				*m_dupeScore = cmd.int_val;
				break;
			case 9:
				*m_dupeMode = cmd.int_val == 1 ? dmAll : (cmd.int_val == 2 ? dmForce : dmScore);
				break;
			case -2:
				error("Invalid value \"%s\" for command \"DUPEMODE\" received from %s",
					msgText + cmd.val_offset, GetInfoName());
				break;
			default:
				error("Invalid command \"%s\" received from %s", msgText, GetInfoName());
				break;
		}
		return;
	}
	ScriptController::AddMessage(kind, text);
}
