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
#include "nzbget_rs.h"

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

	namespace Detail
	{
		inline void GetOptCb(
			void* ctx, size_t idx,
			const char** outNamePtr, size_t* outNameLen,
			const char** outDispPtr, size_t* outDispLen,
			const char** outSecPtr, size_t* outSecLen,
			int* outMulti,
			const char** outPrefPtr, size_t* outPrefLen,
			int* outValIsNum,
			const char** outValStrPtr, size_t* outValStrLen,
			double* outValNum,
			size_t* outDescCnt, size_t* outSelCnt)
		{
			try
			{
				const auto& opt = static_cast<const Script*>(ctx)->GetOptions()[idx];
				*outNamePtr = opt.name.data();
				*outNameLen = opt.name.size();
				*outDispPtr = opt.displayName.data();
				*outDispLen = opt.displayName.size();
				*outSecPtr = opt.section.name.data();
				*outSecLen = opt.section.name.size();
				*outMulti = opt.section.multi ? 1 : 0;
				*outPrefPtr = opt.section.prefix.data();
				*outPrefLen = opt.section.prefix.size();
				if (const std::string* val = std::get_if<std::string>(&opt.value))
				{
					*outValIsNum = 0;
					*outValStrPtr = val->data();
					*outValStrLen = val->size();
				}
				else if (const double* val = std::get_if<double>(&opt.value))
				{
					*outValIsNum = 1;
					*outValNum = *val;
				}
				*outDescCnt = opt.description.size();
				*outSelCnt = opt.select.size();
			}
			catch (...)
			{
			}
		}

		inline void GetOptDescCb(
			void* ctx, size_t optIdx, size_t descIdx,
			const char** outPtr, size_t* outLen)
		{
			try
			{
				const auto& s = static_cast<const Script*>(ctx)->GetOptions()[optIdx].description[descIdx];
				*outPtr = s.data();
				*outLen = s.size();
			}
			catch (...)
			{
			}
		}

		inline void GetOptSelCb(
			void* ctx, size_t optIdx, size_t selIdx,
			int* outIsNum,
			const char** outStrPtr, size_t* outStrLen,
			double* outNum)
		{
			try
			{
				const auto& sel = static_cast<const Script*>(ctx)->GetOptions()[optIdx].select[selIdx];
				if (const std::string* val = std::get_if<std::string>(&sel))
				{
					*outIsNum = 0;
					*outStrPtr = val->data();
					*outStrLen = val->size();
				}
				else if (const double* val = std::get_if<double>(&sel))
				{
					*outIsNum = 1;
					*outNum = *val;
				}
			}
			catch (...)
			{
			}
		}

		inline void GetCmdCb(
			void* ctx, size_t idx,
			const char** outNamePtr, size_t* outNameLen,
			const char** outDispPtr, size_t* outDispLen,
			const char** outActPtr, size_t* outActLen,
			const char** outSecPtr, size_t* outSecLen,
			int* outMulti,
			const char** outPrefPtr, size_t* outPrefLen,
			size_t* outDescCnt)
		{
			try
			{
				const auto& cmd = static_cast<const Script*>(ctx)->GetCommands()[idx];
				*outNamePtr = cmd.name.data();
				*outNameLen = cmd.name.size();
				*outDispPtr = cmd.displayName.data();
				*outDispLen = cmd.displayName.size();
				*outActPtr = cmd.action.data();
				*outActLen = cmd.action.size();
				*outSecPtr = cmd.section.name.data();
				*outSecLen = cmd.section.name.size();
				*outMulti = cmd.section.multi ? 1 : 0;
				*outPrefPtr = cmd.section.prefix.data();
				*outPrefLen = cmd.section.prefix.size();
				*outDescCnt = cmd.description.size();
			}
			catch (...)
			{
			}
		}

		inline void GetCmdDescCb(
			void* ctx, size_t cmdIdx, size_t descIdx,
			const char** outPtr, size_t* outLen)
		{
			try
			{
				const auto& s = static_cast<const Script*>(ctx)->GetCommands()[cmdIdx].description[descIdx];
				*outPtr = s.data();
				*outLen = s.size();
			}
			catch (...)
			{
			}
		}

		inline std::string SerializeScript(const Script& script, bool asXml)
		{
			const char* entry = script.GetEntry() ? script.GetEntry() : "";
			const char* loc = script.GetLocation() ? script.GetLocation() : "";
			const char* root = script.GetRootDir() ? script.GetRootDir() : "";
			const char* name = script.GetName() ? script.GetName() : "";
			const char* disp = script.GetDisplayName() ? script.GetDisplayName() : "";
			const char* about = script.GetAbout() ? script.GetAbout() : "";
			const char* author = script.GetAuthor() ? script.GetAuthor() : "";
			const char* home = script.GetHomepage() ? script.GetHomepage() : "";
			const char* lic = script.GetLicense() ? script.GetLicense() : "";
			const char* ver = script.GetVersion() ? script.GetVersion() : "";
			const char* minVer = script.GetNzbgetMinVersion() ? script.GetNzbgetMinVersion() : "";
			const char* qevents = script.GetQueueEvents() ? script.GetQueueEvents() : "";
			const char* taskTime = script.GetTaskTime() ? script.GetTaskTime() : "";

			std::vector<const char*> descPtrs;
			std::vector<size_t> descLens;
			descPtrs.reserve(script.GetDescription().size());
			descLens.reserve(script.GetDescription().size());
			for (const auto& line : script.GetDescription())
			{
				descPtrs.push_back(line.data());
				descLens.push_back(line.size());
			}

			std::vector<const char*> reqPtrs;
			std::vector<size_t> reqLens;
			reqPtrs.reserve(script.GetRequirements().size());
			reqLens.reserve(script.GetRequirements().size());
			for (const auto& line : script.GetRequirements())
			{
				reqPtrs.push_back(line.data());
				reqLens.push_back(line.size());
			}

			auto fn = asXml ? nzbget_rs_extension_to_xml : nzbget_rs_extension_to_json;
			NzbgetRsBuf buf = fn(
				entry, strlen(entry),
				loc, strlen(loc),
				root, strlen(root),
				name, strlen(name),
				disp, strlen(disp),
				about, strlen(about),
				author, strlen(author),
				home, strlen(home),
				lic, strlen(lic),
				ver, strlen(ver),
				minVer, strlen(minVer),
				script.GetPostScript() ? 1 : 0,
				script.GetScanScript() ? 1 : 0,
				script.GetQueueScript() ? 1 : 0,
				script.GetSchedulerScript() ? 1 : 0,
				script.GetFeedScript() ? 1 : 0,
				qevents, strlen(qevents),
				taskTime, strlen(taskTime),
				descPtrs.empty() ? nullptr : descPtrs.data(),
				descLens.empty() ? nullptr : descLens.data(),
				descPtrs.size(),
				reqPtrs.empty() ? nullptr : reqPtrs.data(),
				reqLens.empty() ? nullptr : reqLens.data(),
				reqPtrs.size(),
				script.GetOptions().size(),
				script.GetCommands().size(),
				const_cast<Script*>(&script),
				&GetOptCb,
				&GetOptDescCb,
				&GetOptSelCb,
				&GetCmdCb,
				&GetCmdDescCb);
			std::string result(buf.data ? buf.data : "", buf.len);
			nzbget_rs_free(buf);
			return result;
		}
	}

	inline std::string ToJsonStr(const Script& script)
	{
		return Detail::SerializeScript(script, false);
	}

	inline std::string ToXmlStr(const Script& script)
	{
		return Detail::SerializeScript(script, true);
	}
}

#endif
