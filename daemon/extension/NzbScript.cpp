/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "NzbScript.h"
#include "Options.h"
#include "Log.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

void NzbScriptController::PrepareEnvScript(NzbParameterList* parameters, const char* scriptName)
{
	std::vector<const char*> pnames;
	std::vector<const char*> pvals;
	if (parameters)
	{
		pnames.reserve(parameters->size());
		pvals.reserve(parameters->size());
		for (NzbParameter& parameter : parameters)
		{
			pnames.push_back(parameter.GetName());
			pvals.push_back(parameter.GetValue());
		}
	}
	nzbget_rs_nzb_script_prepare_env(
		scriptName ? scriptName : "",
		parameters != nullptr ? 1 : 0,
		pnames.data(),
		pvals.data(),
		pnames.size(),
		this,
		[](void* ctx, const char* prefix, const char* name, const char* value)
		{
			static_cast<NzbScriptController*>(ctx)->SetEnvVarSpecial(prefix, name, value);
		},
		[](void* ctx, const char* stripPrefix)
		{
			static_cast<NzbScriptController*>(ctx)->PrepareEnvOptions(stripPrefix);
		});
}

void NzbScriptController::ExecuteScriptList(const char* scriptList)
{
	if (Util::EmptyStr(scriptList))
	{
		return;
	}

	Tokenizer tok(scriptList, ",;");
	while (const char* scriptName = tok.Next())
	{
		auto found = g_ExtensionManager->FindIf([&scriptName](auto script)
			{
				return strcmp(scriptName, script->GetName()) == 0;
			}
		);

		if (found)
		{
			ExecuteScript(std::move(found.value()));
		}
	}
}
