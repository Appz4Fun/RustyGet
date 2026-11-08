/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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

#ifndef EXTENSION_LOADER_H
#define EXTENSION_LOADER_H

#include <exception>
#include <fstream>
#include <memory>
#include <vector>
#include "Extension.h"
#include "ManifestFile.h"
#include "ScriptConfig.h"
#include "FileSystem.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace ExtensionLoader
{
	inline Extension::Kind GetScriptKind(const std::string& line)
	{
		int kindBits = nzbget_rs_ext_script_kind(line.data(), line.size());
		Extension::Kind kind;
		kind.post = (kindBits & 1) != 0;
		kind.scan = (kindBits & 2) != 0;
		kind.queue = (kindBits & 4) != 0;
		kind.scheduler = (kindBits & 8) != 0;
		kind.feed = (kindBits & 16) != 0;
		return kind;
	}

	namespace V1
	{
		inline void BuildDisplayName(Extension::Script& script)
		{
			NzbgetRsBuf shortBuf = nzbget_rs_dqh_unpack_ctrl_strip_last_extension(script.GetName());
			const char* displayName = FileSystem::BaseFileName(shortBuf.data ? shortBuf.data : "");
			script.SetDisplayName(displayName);
			nzbget_rs_free(shortBuf);
		}

		inline bool Load(Extension::Script& script, const char* location, const char* rootDir)
		{
			std::ifstream file(script.GetEntry());
			if (!file.is_open())
			{
				return false;
			}
			struct Reader
			{
				std::ifstream& file;
				std::string line;
				std::exception_ptr error;
			} reader{file, {}, {}};
			auto next = [](void* context, NzbgetRsStr* line) noexcept -> int
			{
				auto& r = *static_cast<Reader*>(context);
				try
				{
					if (!std::getline(r.file, r.line)) return 0;
					*line = {r.line.data(), r.line.size()};
					return 1;
				}
				catch (...)
				{
					r.error = std::current_exception();
					return -1;
				}
			};
			auto rightSpace = [](int byte) noexcept -> int
			{
				int ch = static_cast<char>(byte);
#ifdef __GLIBC__
				return std::isspace(ch);
#else
				return ch < 0 ? 0 : std::isspace(ch);
#endif
			};
			std::unique_ptr<NzbgetRsExtV1, void (*)(NzbgetRsExtV1*)> parsed(
				nzbget_rs_ext_v1_read(next, &reader, rightSpace), nzbget_rs_ext_v1_free);
			if (reader.error) std::rethrow_exception(reader.error);
			if (!parsed)
			{
				return false;
			}
			const NzbgetRsExtV1* h = parsed.get();
			auto text = [](NzbgetRsStr str) { return std::string(str.data ? str.data : "", str.len); };
			auto selectOpt = [h](size_t i, size_t j)
			{
				double num = 0;
				NzbgetRsStr str{};
				if (nzbget_rs_ext_v1_select(h, i, j, &num, &str) == 1)
				{
					return ManifestFile::SelectOption(num);
				}
				return ManifestFile::SelectOption(std::string(str.data ? str.data : "", str.len));
			};
			auto fillItem = [h, &text](auto& item, int command, size_t i)
			{
				item.section.name = text(nzbget_rs_ext_v1_item_text(h, command, i, 0, 0));
				item.section.prefix = text(nzbget_rs_ext_v1_item_text(h, command, i, 1, 0));
				item.section.multi = nzbget_rs_ext_v1_item_count(h, command, i, 0) != 0;
				item.name = text(nzbget_rs_ext_v1_item_text(h, command, i, 2, 0));
				item.displayName = item.name;
				for (size_t j = 0; j < nzbget_rs_ext_v1_item_count(h, command, i, 1); j++)
				{
					item.description.push_back(text(nzbget_rs_ext_v1_item_text(h, command, i, 4, j)));
				}
			};

			int kindBits = nzbget_rs_ext_v1_kind(h);
			Extension::Kind kind;
			kind.post = kindBits & 1;
			kind.scan = kindBits & 2;
			kind.queue = kindBits & 4;
			kind.scheduler = kindBits & 8;
			kind.feed = kindBits & 16;

			std::string about = text(nzbget_rs_ext_v1_text(h, 0, 0));
			std::string queueEvents = text(nzbget_rs_ext_v1_text(h, 1, 0));
			std::string taskTime = text(nzbget_rs_ext_v1_text(h, 2, 0));
			std::vector<std::string> description;
			for (size_t i = 0; i < nzbget_rs_ext_v1_count(h, 0); i++)
			{
				description.push_back(text(nzbget_rs_ext_v1_text(h, 3, i)));
			}
			std::vector<std::string> requirements;
			for (size_t i = 0; i < nzbget_rs_ext_v1_count(h, 1); i++)
			{
				requirements.push_back(text(nzbget_rs_ext_v1_text(h, 4, i)));
			}
			std::vector<ManifestFile::Option> options;
			for (size_t i = 0; i < nzbget_rs_ext_v1_count(h, 2); i++)
			{
				ManifestFile::Option option{};
				fillItem(option, 0, i);
				option.value = selectOpt(i, SIZE_MAX);
				for (size_t j = 0; j < nzbget_rs_ext_v1_item_count(h, 0, i, 2); j++)
				{
					option.select.push_back(selectOpt(i, j));
				}
				options.push_back(std::move(option));
			}
			std::vector<ManifestFile::Command> commands;
			for (size_t i = 0; i < nzbget_rs_ext_v1_count(h, 3); i++)
			{
				ManifestFile::Command command{};
				fillItem(command, 1, i);
				command.action = text(nzbget_rs_ext_v1_item_text(h, 1, i, 3, 0));
				commands.push_back(std::move(command));
			}
			parsed.reset();

			BuildDisplayName(script);
			Util::TrimRight(about);

			requirements.shrink_to_fit();
			options.shrink_to_fit();
			commands.shrink_to_fit();

			script.SetLocation(location);
			script.SetRootDir(rootDir);
			script.SetRequirements(std::move(requirements));
			script.SetKind(std::move(kind));
			script.SetQueueEvents(std::move(queueEvents));
			script.SetAbout(std::move(about));
			script.SetDescription(std::move(description));
			script.SetTaskTime(std::move(taskTime));
			script.SetOptions(std::move(options));
			script.SetCommands(std::move(commands));

			return true;
		}
	}

	namespace V2
	{
		inline bool Load(Extension::Script& script, const char* location, const char* rootDir)
		{
			ManifestFile::Manifest manifest{};
			if (!ManifestFile::Load(manifest, location))
				return false;

			NzbgetRsBuf entryBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				location, PATH_SEPARATOR, manifest.main.c_str());
			std::string entry(entryBuf.data ? entryBuf.data : "", entryBuf.len);
			nzbget_rs_free(entryBuf);
			script.SetEntry(std::move(entry));
			script.SetLocation(location);
			script.SetRootDir(rootDir);
			script.SetAuthor(std::move(manifest.author));
			script.SetHomepage(std::move(manifest.homepage));
			script.SetLicense(std::move(manifest.license));
			script.SetVersion(std::move(manifest.version));
			script.SetNzbgetMinVersion(std::move(manifest.nzbgetMinVersion));
			script.SetDisplayName(std::move(manifest.displayName));
			script.SetName(std::move(manifest.name));
			script.SetAbout(std::move(manifest.about));
			script.SetDescription(std::move(manifest.description));
			script.SetKind(GetScriptKind(manifest.kind));
			script.SetQueueEvents(std::move(manifest.queueEvents));
			script.SetTaskTime(std::move(manifest.taskTime));
			script.SetRequirements(std::move(manifest.requirements));
			script.SetCommands(std::move(manifest.commands));
			script.SetOptions(std::move(manifest.options));
			return true;
		}
	}
}

#endif
