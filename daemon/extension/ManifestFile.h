/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2023-2024 Denis <denis@nzbget.com>
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

#ifndef MANIFEST_FILE_H
#define MANIFEST_FILE_H

#include <exception>
#include <string>
#include <vector>
#include <variant>
#include "Json.h"
#include "FileSystem.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace ManifestFile
{
	using SelectOption = std::variant<double, std::string>;

	inline constexpr size_t BUFFER_SIZE = 4096;
	inline const char* MANIFEST_FILE = "manifest.json";
	inline const char* DEFAULT_SECTION_NAME = "options";

	struct Section
	{
		bool multi;
		std::string name;
		std::string prefix;
	};

	struct Option
	{
		Section section;
		std::string name;
		std::string displayName;
		std::vector<std::string> description;
		SelectOption value;
		std::vector<SelectOption> select;
	};

	struct Command
	{
		Section section;
		std::string name;
		std::string displayName;
		std::string action;
		std::vector<std::string> description;
	};

	struct Manifest
	{
		std::string author;
		std::string main;
		std::string kind;
		std::string name;
		std::string homepage;
		std::string displayName;
		std::string version;
		std::string nzbgetMinVersion;
		std::string license;
		std::string about;
		std::string queueEvents;
		std::string taskTime;
		std::vector<std::string> description;
		std::vector<std::string> requirements;
		std::vector<Option> options;
		std::vector<Command> commands;
	};

	namespace detail
	{
		inline std::string Str(const NzbgetRsStr& str)
		{
			return std::string(str.data, str.len);
		}

		inline std::vector<std::string> Strs(const NzbgetRsStr* strs, size_t count)
		{
			std::vector<std::string> out;
			out.reserve(count);
			for (size_t i = 0; i < count; ++i)
			{
				out.push_back(Str(strs[i]));
			}
			return out;
		}

		inline SelectOption Select(const NzbgetRsManifestSelect& select)
		{
			if (select.isString)
			{
				return Str(select.string);
			}
			return select.number;
		}

		inline Section MakeSection(const NzbgetRsManifestSection& section)
		{
			return Section{section.multi != 0, Str(section.name), Str(section.prefix)};
		}

		inline void Copy(const NzbgetRsManifest& from, Manifest& manifest)
		{
			std::string* fields[] = {&manifest.author, &manifest.main, &manifest.homepage, &manifest.about,
				&manifest.version, &manifest.name, &manifest.displayName, &manifest.kind, &manifest.license,
				&manifest.taskTime, &manifest.queueEvents, &manifest.nzbgetMinVersion};
			for (size_t i = 0; i < 11; ++i)
			{
				if (from.has[i])
				{
					*fields[i] = Str(from.fields[i]);
				}
			}

			for (size_t i = 0; i < from.optionCount; ++i)
			{
				const NzbgetRsManifestOption& rsOption = from.options[i];
				Option option{};
				option.section = MakeSection(rsOption.section);
				option.name = Str(rsOption.name);
				option.displayName = Str(rsOption.displayName);
				option.description = Strs(rsOption.description, rsOption.descriptionCount);
				option.value = Select(rsOption.value);
				for (size_t j = 0; j < rsOption.selectCount; ++j)
				{
					option.select.emplace_back(Select(rsOption.select[j]));
				}
				manifest.options.emplace_back(std::move(option));
			}
			manifest.options.shrink_to_fit();

			for (size_t i = 0; i < from.commandCount; ++i)
			{
				const NzbgetRsManifestCommand& rsCommand = from.commands[i];
				Command command{};
				command.section = MakeSection(rsCommand.section);
				command.name = Str(rsCommand.name);
				command.displayName = Str(rsCommand.displayName);
				command.action = Str(rsCommand.action);
				command.description = Strs(rsCommand.description, rsCommand.descriptionCount);
				manifest.commands.emplace_back(std::move(command));
			}
			manifest.commands.shrink_to_fit();

			for (size_t i = 0; i < from.sectionCount; ++i)
			{
				Section section = MakeSection(from.sections[i]);
				for (auto& option : manifest.options)
				{
					if (option.section.name == section.name) option.section = section;
				}
				for (auto& command : manifest.commands)
				{
					if (command.section.name == section.name) command.section = section;
				}
			}
			for (size_t i = 0; i < from.descriptionCount; ++i)
			{
				manifest.description.emplace_back(Str(from.description[i]));
			}
			for (size_t i = 0; i < from.requirementsCount; ++i)
			{
				manifest.requirements.emplace_back(Str(from.requirements[i]));
			}
			if (from.has[11]) manifest.nzbgetMinVersion = Str(from.fields[11]);
		}
	}

	inline bool Load(Manifest& manifest, const char* directory)
	{
		NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			directory, PATH_SEPARATOR, MANIFEST_FILE);
		std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
		nzbget_rs_free(pathBuf);
		DiskFile file;
		if (!file.Open(path.c_str(), DiskFile::omRead)) return false;

		std::string jsonStr;
		char buffer[BUFFER_SIZE];
		while (file.ReadLine(buffer, BUFFER_SIZE))
		{
			jsonStr += buffer;
		}

		struct Context
		{
			Manifest* manifest;
			std::exception_ptr error;
		} context{&manifest, nullptr};
		const NzbgetRsJsonLimits limits{Json::string::max_size(), Json::JsonArray::max_size(), Json::JsonObject::max_size()};
		int loaded = nzbget_rs_manifest_load(jsonStr.data(), jsonStr.size(), limits, &context,
			[](void* ctx, const NzbgetRsManifest* from) noexcept
			{
				Context& context = *static_cast<Context*>(ctx);
				try
				{
					detail::Copy(*from, *context.manifest);
				}
				catch (...)
				{
					context.error = std::current_exception();
				}
			});

		if (loaded == -1)
		{
			error("Failed to parse %s. Syntax error.", path.c_str());
			return false;
		}

		try
		{
			if (loaded == -2)
			{
				error("Failed to parse %s. The manifest isn't a JSON object.", path.c_str());
				return false;
			}
			if (context.error)
			{
				std::rethrow_exception(context.error);
			}
			if (loaded == -3)
			{
				const Json::JsonValue notObject;
				(void)notObject.as_object();
			}
		}
		catch (const std::exception& e)
		{
			error("Failed to parse %s: %s", path.c_str(), e.what());
			return false;
		}

		return loaded == 1;
	}
};

#endif
