/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
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

#include "Status.h"
#include "ExtensionScriptsValidator.h"
#include "Validators.h"
#include "Options.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace SystemHealth::ExtensionScripts
{
ExtensionScriptsValidator::ExtensionScriptsValidator(const Options& options,
													 const ExtensionManager::Manager& manager)
	: m_options(options), m_extensionManager(manager)
{
	m_validators.reserve(5);
	m_validators.push_back(std::make_unique<ExtensionListValidator>(options, manager));
	m_validators.push_back(std::make_unique<ScriptOrderValidator>(options));
	m_validators.push_back(std::make_unique<ScriptPauseQueueValidator>(options));
	m_validators.push_back(std::make_unique<ShellOverrideValidator>(options));
	m_validators.push_back(std::make_unique<EventIntervalValidator>(options));
}

Status ExtensionListValidator::Validate() const
{
	std::string_view extensions = m_options.GetExtensions();
	if (extensions.empty()) return Status::Ok();

	std::string message;
	Tokenizer tokDir(extensions.data(), ",;");
	while (const char* scriptNameRaw = tokDir.Next())
	{
		// Strip file extension from script name (e.g., "QueueSort.py" -> "QueueSort")
		std::string scriptName(scriptNameRaw);
		scriptName.resize(
			nzbget_rs_euh_health_strip_script_ext_len(scriptName.data(), scriptName.size()));

		const auto extension =
			m_extensionManager.FindIf([&](const auto ext) 
			{
				std::string_view name = ext->GetName();
				return name == scriptName; 
			});
		if (!extension)
		{
			if (!message.empty()) message += "; ";
			message += std::string("'") + scriptNameRaw + "' doesn't exist";
			continue;
		}

		const auto exists = File::Exists(extension.value()->GetEntry());
		if (!exists.IsOk())
		{
			if (!message.empty()) message += "; ";
			message += exists.GetMessage() + " ";
		}
	}

	if (message.empty()) return Status::Ok();

	return Status::Warning(std::move(message));
}

Status ScriptPauseQueueValidator::Validate() const
{
	return Status::Ok();
}

Status ShellOverrideValidator::Validate() const
{
	std::string_view path = m_options.GetShellOverride();
	if (path.empty()) return Status::Ok();

	std::string message;
	std::string pathStr(path);
	Tokenizer tok(pathStr.data(), ",;");
	while (const char* shellover = tok.Next())
	{
		size_t eqPos = 0;
		if (nzbget_rs_dqh_diskstate_find_char_pos(shellover, '=', &eqPos) != 0)
		{
			std::string cmd(shellover, eqPos);
			const char* actualCmd = shellover + eqPos + 1;
			const auto exists = File::Exists(actualCmd);
			if (!exists.IsOk())
			{
				if (!message.empty()) message += "; ";
				message += exists.GetMessage() + " ";
				continue;
			}
			const auto exe = File::Executable(actualCmd);
			if (!exe.IsOk())
			{
				if (!message.empty()) message += "; ";
				message += exe.GetMessage() + " ";
			}
		}
	}

	if (message.empty()) return Status::Ok();

	return Status::Warning(std::move(message));
}

Status EventIntervalValidator::Validate() const
{
	int val = m_options.GetEventInterval();
	if (nzbget_rs_euh_health_validate_event_interval(val) == 0)
	{
		return Status::Error("'" + std::string(Options::EVENTINTERVAL) +
							 "' cannot be less than -1");
	}

	return Status::Ok();
}

Status ScriptOrderValidator::Validate() const
{
	return Status::Ok();
}
}  // namespace SystemHealth::ExtensionScripts
