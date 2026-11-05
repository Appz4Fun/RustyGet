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

#ifndef EXTENSION_H
#define EXTENSION_H

#include "ManifestFile.h"

namespace Extension
{
	struct Kind
	{
		bool post = false;
		bool scan = false;
		bool queue = false;
		bool scheduler = false;
		bool feed = false;
	};

	class Script final
	{
	public:
		Script() = default;
		~Script() = default;

		Script& operator=(const Script&) = delete;
		Script(const Script&) = delete;

		Script(Script&& other) = default;
		Script& operator=(Script&& other) = default;

		void SetEntry(std::string entry) { m_entry = std::move(entry); }
		const char* GetEntry() const { return m_entry.c_str(); }
		void SetLocation(std::string location) { m_location = std::move(location); }
		const char* GetLocation() const { return m_location.c_str(); }
		void SetRootDir(std::string dir) { m_rootDir = std::move(dir); }
		const char* GetRootDir() const { return m_rootDir.c_str(); }
		void SetAuthor(std::string author) { m_author = std::move(author); }
		const char* GetAuthor() const { return m_author.c_str(); }
		void SetVersion(std::string version) { m_version = std::move(version); }
		void SetNzbgetMinVersion(std::string version) { m_nzbgetMinVersion = std::move(version); }
		const char* GetVersion() const { return m_version.c_str(); }
		const char* GetNzbgetMinVersion() const { return m_nzbgetMinVersion.c_str(); }
		void SetLicense(std::string license) { m_license = std::move(license); }
		const char* GetLicense() const { return m_license.c_str(); }
		void SetHomepage(std::string homepage) { m_homepage = std::move(homepage); }
		const char* GetHomepage() const { return m_homepage.c_str(); }
		void SetName(std::string name) { m_name = std::move(name); }
		const char* GetName() const { return m_name.c_str(); }
		void SetDisplayName(std::string displayName) { m_displayName = std::move(displayName); }
		const char* GetDisplayName() const { return m_displayName.c_str(); }
		void SetAbout(std::string about) { m_about = std::move(about); }
		const char* GetAbout() const { return m_about.c_str(); }
		void SetDescription(std::vector<std::string> description) { m_description = std::move(description); }
		const std::vector<std::string>& GetDescription() const { return m_description; }
		void SetKind(Kind kind) { m_kind = std::move(kind); }
		bool GetPostScript() const { return m_kind.post; }
		bool GetScanScript() const { return m_kind.scan; }
		bool GetQueueScript() const { return m_kind.queue; }
		bool GetSchedulerScript() const { return m_kind.scheduler; }
		bool GetFeedScript() const { return m_kind.feed; }
		void SetQueueEvents(std::string queueEvents) { m_queueEvents = std::move(queueEvents); }
		const char* GetQueueEvents() const { return m_queueEvents.c_str(); }
		void SetTaskTime(std::string taskTime) { m_taskTime = std::move(taskTime); }
		const char* GetTaskTime() const { return m_taskTime.c_str(); }
		void SetRequirements(std::vector<std::string> requirements) { m_requirements = std::move(requirements); }
		const std::vector<std::string>& GetRequirements() const { return m_requirements; }
		void SetOptions(std::vector<ManifestFile::Option> options) { m_options = std::move(options); }
		const std::vector<ManifestFile::Option>& GetOptions() const { return m_options; }
		void SetCommands(std::vector<ManifestFile::Command> commands) { m_commands = std::move(commands); }
		const std::vector<ManifestFile::Command>& GetCommands() const { return m_commands; }

	private:
		Kind m_kind;
		std::string m_entry;
		std::string m_location;
		std::string m_rootDir;
		std::string m_author;
		std::string m_version;
		std::string m_nzbgetMinVersion;
		std::string m_homepage;
		std::string m_license;
		std::string m_name;
		std::string m_displayName;
		std::string m_about;
		std::string m_queueEvents;
		std::string m_taskTime;
		std::vector<std::string> m_description;
		std::vector<std::string> m_requirements;
		std::vector<ManifestFile::Option> m_options;
		std::vector<ManifestFile::Command> m_commands;
	};

	std::string ToJsonStr(const Script& script);
	std::string ToXmlStr(const Script& script);
}

#endif
