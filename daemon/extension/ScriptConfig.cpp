/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2023 Denis <denis@nzbget.com>
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

#include "Util.h"
#include "FileSystem.h"
#include "Options.h"
#include "Log.h"
#include "ScriptConfig.h"
#include "nzbget_rs.h"

bool ScriptConfig::ReadCipherSnapshot(std::string& bytes)
{
	if (!g_Options || !g_Options->GetConfigFilename())
	{
		return false;
	}
	NzbgetRsBuf buf{};
	if (!nzbget_rs_script_config_read_cipher_snapshot(g_Options->GetConfigFilename(), &buf))
	{
		return false;
	}
	bytes.assign(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return true;
}

ScriptConfig::CipherCommit ScriptConfig::CommitCipherPatch(std::string_view expected, std::string_view replacement,
	const std::function<bool()>& cancel)
{
	if (!g_Options || !g_Options->GetConfigFilename())
	{
		return CipherCommit::Failed;
	}
	int res = nzbget_rs_script_config_commit_cipher_patch(
		g_Options->GetConfigFilename(),
		expected.data(),
		expected.size(),
		replacement.data(),
		replacement.size(),
		const_cast<std::function<bool()>*>(&cancel),
		[](void* ctx) -> int
		{
			try
			{
				const auto* fn = static_cast<const std::function<bool()>*>(ctx);
				return (*fn)() ? 1 : 0;
			}
			catch (...)
			{
				return 1;
			}
		},
		[](void*)
		{
			try
			{
				warn("TLS cipher settings were replaced, but the configuration directory could not be synchronized");
			}
			catch (...)
			{
			}
		});
	switch (res)
	{
		case 0: return CipherCommit::Saved;
		case 1: return CipherCommit::Unchanged;
		case 2: return CipherCommit::Conflict;
		case 3: return CipherCommit::Cancelled;
		default: return CipherCommit::Failed;
	}
}

void ScriptConfig::InitOptions()
{
	InitConfigTemplates();
}

bool ScriptConfig::LoadConfig(Options::OptEntries* optEntries)
{
	if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
	{
		return false;
	}
	return nzbget_rs_script_config_load(
		g_Options->GetConfigFilename(),
		optEntries,
		[](void* ctx, const char* name, const char* value)
		{
			try
			{
				static_cast<Options::OptEntries*>(ctx)->emplace_back(name, value);
			}
			catch (...)
			{
			}
		}) != 0;
}

// Versioned, unambiguous saved-value token shared with Config.cipherBaseline.
// Missing fields differ from present empty fields; duplicate keys are last-wins.
std::string ScriptConfig::CipherBaseline(const Options::OptEntries& optEntries)
{
	std::vector<const char*> names;
	std::vector<const char*> values;
	names.reserve(optEntries.size());
	values.reserve(optEntries.size());
	for (const auto& entry : optEntries)
	{
		if (!entry.GetName() || !entry.GetValue()) continue;
		names.push_back(entry.GetName());
		values.push_back(entry.GetValue());
	}
	NzbgetRsBuf buf = nzbget_rs_sus_build_cipher_baseline(names.data(), values.data(), names.size());
	std::string token(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return token;
}

bool ScriptConfig::SaveConfig(Options::OptEntries* optEntries, const char* cipherBaseline, bool* conflict)
{
	if (conflict) *conflict = false;
	if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
	{
		return false;
	}
	std::vector<const char*> names;
	std::vector<const char*> values;
	names.reserve(optEntries->size());
	values.reserve(optEntries->size());
	for (const auto& entry : *optEntries)
	{
		if (entry.GetName() && entry.GetValue())
		{
			names.push_back(entry.GetName());
			values.push_back(entry.GetValue());
		}
	}
	int conflictFlag = 0;
	int saved = nzbget_rs_script_config_save(
		g_Options->GetConfigFilename(),
		names.data(),
		values.data(),
		names.size(),
		cipherBaseline,
		&conflictFlag);
	if (conflict)
	{
		*conflict = conflictFlag != 0;
	}
	return saved != 0;
}

bool ScriptConfig::LoadConfigTemplates(ConfigTemplates* configTemplates)
{
	CharBuffer buffer;
	if (!FileSystem::LoadFileIntoBuffer(g_Options->GetConfigTemplate(), buffer, true))
	{
		return false;
	}
	configTemplates->emplace_back(ConfigTemplate(buffer));

	return true;
}

void ScriptConfig::InitConfigTemplates()
{
	if (!LoadConfigTemplates(&m_configTemplates))
	{
		error("Could not read configuration templates");
	}
}
