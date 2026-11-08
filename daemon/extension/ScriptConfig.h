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

#ifndef SCRIPTCONFIG_H
#define SCRIPTCONFIG_H

#include "NString.h"
#include "Container.h"
#include "Options.h"
#include "FileSystem.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"
#include <string_view>
#include <functional>

class ScriptConfig
{
public:

	class ConfigTemplate
	{
	public:
		ConfigTemplate(const char* templ)
			: m_template(templ) {}
		const char* GetTemplate() { return m_template; }

	private:
		CString m_template;
	};

	using ConfigTemplates = std::deque<ConfigTemplate>;

	void InitOptions()
	{
		InitConfigTemplates();
	}

	bool LoadConfig(Options::OptEntries* optEntries)
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

	bool SaveConfig(Options::OptEntries* optEntries, const char* cipherBaseline = nullptr, bool* conflict = nullptr)
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

	static std::string CipherBaseline(const Options::OptEntries& optEntries)
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

	enum class CipherCommit { Saved, Unchanged, Conflict, Cancelled, Failed };

	bool ReadCipherSnapshot(std::string& bytes)
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

	CipherCommit CommitCipherPatch(std::string_view expected, std::string_view replacement, const std::function<bool()>& cancel)
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

	bool LoadConfigTemplates(ConfigTemplates* configTemplates)
	{
		CharBuffer buffer;
		if (!FileSystem::LoadFileIntoBuffer(g_Options->GetConfigTemplate(), buffer, true))
		{
			return false;
		}
		configTemplates->emplace_back(ConfigTemplate(buffer));

		return true;
	}

	ConfigTemplates* GetConfigTemplates() { return &m_configTemplates; }

private:
	ConfigTemplates m_configTemplates;

	void InitConfigTemplates()
	{
		if (!LoadConfigTemplates(&m_configTemplates))
		{
			error("Could not read configuration templates");
		}
	}
};

extern ScriptConfig* g_ScriptConfig;

#endif
