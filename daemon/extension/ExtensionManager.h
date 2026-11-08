/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2023-2025 Denis <denis@nzbget.com>
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

#ifndef EXTENSION_MANAGER_H
#define EXTENSION_MANAGER_H

#include <atomic>
#include <vector>
#include <utility>
#include <memory>
#include <shared_mutex>
#include <optional>
#include <functional>
#include "FileSystem.h"
#include "WebDownloader.h"
#include "Extension.h"
#include "Util.h"
#include "ExtensionLoader.h"
#include "Options.h"
#include "nzbget_rs.h"

namespace ExtensionManager
{
	using ExtensionPtr = std::shared_ptr<const Extension::Script>;
	using Extensions = std::vector<ExtensionPtr>;

	class Manager final
	{
	public:
		Manager() = default;
		~Manager() = default;

		Manager(const Manager&) = delete;
		Manager& operator=(const Manager&) = delete;

		Manager(Manager&&) = delete;
		Manager& operator=(Manager&&) = delete;

		std::optional<std::string> 
		InstallExtension(const fs::path& file, const fs::path& dest)
		{
			std::string fileStr = fs::u8string(file);
			std::string destStr = fs::u8string(dest);
			NzbgetRsBuf errBuf = nzbget_rs_ext_mgr_install_extension(
				fileStr.c_str(),
				destStr.c_str(),
				[](void*, int kind, const char* msg) {
					const char* text = msg ? msg : "";
					if (kind == 1)
					{
						error("%s", text);
					}
					else if (kind == 2)
					{
						warn("%s", text);
					}
					else if (kind == 0)
					{
						info("%s", text);
					}
					else
					{
						detail("%s", text);
					}
				},
				nullptr);
			if (errBuf.data)
			{
				std::string err(errBuf.data, errBuf.len);
				nzbget_rs_free(errBuf);
				return err;
			}
			nzbget_rs_free(errBuf);
			return std::nullopt;
		}

		std::optional<std::string> 
		UpdateExtension(const fs::path& filename, const std::string& extName)
		{
			std::unique_lock<std::shared_mutex> lock{m_mutex};

			auto extensionIt = GetByName(extName);
			bool found = extensionIt != std::end(m_extensions);
			int64_t useCount = found ? extensionIt->use_count() : 0;
			fs::path targetPath;
			if (found)
			{
				GetExtensionPath(*(*extensionIt), targetPath);
			}
			std::string targetPathStr = fs::u8string(targetPath);
			std::string archiveStr = fs::u8string(filename);

			NzbgetRsBuf backupBuf{};
			NzbgetRsBuf prepErrBuf = nzbget_rs_ext_mgr_prepare_update(
				extName.c_str(),
				found ? 1 : 0,
				useCount,
				targetPathStr.c_str(),
				archiveStr.c_str(),
				&backupBuf);
			std::string backupPathStr(backupBuf.data ? backupBuf.data : "", backupBuf.len);
			nzbget_rs_free(backupBuf);

			if (prepErrBuf.data)
			{
				std::string err(prepErrBuf.data, prepErrBuf.len);
				nzbget_rs_free(prepErrBuf);
				return err;
			}
			nzbget_rs_free(prepErrBuf);

			const auto rootDir = (*extensionIt)->GetRootDir();
			const auto installExtError = InstallExtension(filename, rootDir);
			NzbgetRsBuf finishErrBuf = nzbget_rs_ext_mgr_finish_update(
				targetPathStr.c_str(),
				backupPathStr.c_str(),
				archiveStr.c_str(),
				installExtError ? installExtError->c_str() : nullptr);
			if (finishErrBuf.data)
			{
				std::string err(finishErrBuf.data, finishErrBuf.len);
				nzbget_rs_free(finishErrBuf);
				return err;
			}
			nzbget_rs_free(finishErrBuf);

			m_extensions.erase(extensionIt);
			return std::nullopt;
		}

		std::optional<std::string>
		DeleteExtension(const std::string& name)
		{
			std::unique_lock<std::shared_mutex> lock{m_mutex};

			auto extensionIt = GetByName(name);
			bool found = extensionIt != std::end(m_extensions);
			int64_t useCount = found ? extensionIt->use_count() : 0;
			fs::path targetPath;
			if (found)
			{
				GetExtensionPath(*(*extensionIt), targetPath);
			}
			std::string targetPathStr = fs::u8string(targetPath);

			NzbgetRsBuf errBuf = nzbget_rs_ext_mgr_delete_extension(
				name.c_str(),
				found ? 1 : 0,
				useCount,
				targetPathStr.c_str());
			if (errBuf.data)
			{
				std::string err(errBuf.data, errBuf.len);
				nzbget_rs_free(errBuf);
				return err;
			}
			nzbget_rs_free(errBuf);

			m_extensions.erase(extensionIt);
			return std::nullopt;
		}

		std::optional<std::string>
		LoadExtensions()
		{
			const char* scriptDir = g_Options->GetScriptDir();
			if (Util::EmptyStr(scriptDir))
			{
				return std::string("\"ScriptDir\" is not specified");
			}

			std::unique_lock<std::shared_mutex> lock{m_mutex};

			m_extensions.clear();

			Tokenizer tokDir(scriptDir, ",;");
			while (const char* extensionDir = tokDir.Next())
			{
				LoadExtensionDir(extensionDir, false, extensionDir);
			}

			Sort(g_Options->GetScriptOrder());
			CreateTasks();
			m_extensions.shrink_to_fit();

			return std::nullopt;
		}

		std::pair<WebDownloader::EStatus, fs::path>
		DownloadExtension(const std::string& url, const std::string& extName)
		{
			// a name of our own, not the extension's: that one comes from the request
			// ("../x" wrote anywhere the daemon can), and two downloads of one
			// extension at once wrote the same file
			static std::atomic<int> counter{0};
			fs::path tmpFileName = g_Options->GetTempDirPath() /
				("ext-" + std::to_string(static_cast<int64_t>(Util::CurrentTime())) + "-" + std::to_string(++counter) + ".zip");

			const auto tmpFileNameStr = fs::u8string(tmpFileName);

			std::unique_ptr<WebDownloader> downloader = std::make_unique<WebDownloader>();
			downloader->SetUrl(url.c_str());
			downloader->SetForce(true);
			downloader->SetRetry(false);
			downloader->SetOutputFilename(tmpFileNameStr.c_str());
			downloader->SetInfoName(extName.c_str());

			WebDownloader::EStatus status = downloader->DownloadWithRedirects(5);
			downloader.reset();

			return std::make_pair(status, std::move(tmpFileName));
		}

		void ForEach(std::function<void(const ExtensionPtr)> callback) const
		{
			std::shared_lock<std::shared_mutex> lock{ m_mutex };

			std::for_each(std::cbegin(m_extensions), std::cend(m_extensions), callback);
		}

		std::optional<ExtensionPtr> 
		FindIf(std::function<bool(ExtensionPtr)> comparator) const
		{
			std::shared_lock<std::shared_mutex> lock{ m_mutex };

			auto found = std::find_if(std::cbegin(m_extensions), std::cend(m_extensions), comparator);
			if (found != std::cend(m_extensions))
			{
				return *found;
			}

			return std::nullopt;
		}

	private:
		void LoadExtensionDir(const char* directory, bool isSubDir, const char* rootDir)
		{
			Extension::Script extension;

			if (ExtensionLoader::V2::Load(extension, directory, rootDir)) {
				if (!Exists(extension.GetName()))
				{
					m_extensions.emplace_back(std::make_shared<Extension::Script>(std::move(extension)));
					return;
				}
			}

			DirBrowser dir(directory);
			while (const char* filename = dir.Next())
			{
				if (filename[0] == '.' || filename[0] == '_')
					continue;

				NzbgetRsBuf entryBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					directory, PATH_SEPARATOR, filename);
				const std::string entry(entryBuf.data ? entryBuf.data : "", entryBuf.len);
				nzbget_rs_free(entryBuf);
				if (!FileSystem::DirectoryExists(entry.c_str()))
				{
					std::string name = GetExtensionName(filename);
					if (Exists(name))
					{
						continue;
					}

					const char* location = isSubDir ? directory : entry.c_str();
					extension.SetEntry(entry);
					extension.SetName(std::move(name));
					if (ExtensionLoader::V1::Load(extension, location, rootDir))
					{
						m_extensions.emplace_back(std::make_shared<Extension::Script>(std::move(extension)));
					}
				}
				else if (!isSubDir)
				{
					LoadExtensionDir(entry.c_str(), true, rootDir);
				}
			}
		}

		void CreateTasks() const
		{
			for (const auto& extension : m_extensions)
			{
				if (extension->GetSchedulerScript() &&
					!Util::EmptyStr(extension->GetTaskTime()) &&
					nzbget_rs_ext_is_in_list(g_Options->GetExtensions(), extension->GetName()) != 0)
				{
					g_Options->CreateSchedulerTask(
						0,
						extension->GetTaskTime(),
						nullptr,
						Options::scScript,
						extension->GetName()
					);
				}
			}
		}

		Extensions::const_iterator GetByName(const std::string& name) const
		{
			return std::find_if(
				std::begin(m_extensions),
				std::end(m_extensions),
				[&name](const auto& ext)
				{
					return ext->GetName() == name;
				}
			);
		}

		bool Exists(const std::string& name) const
		{
			return GetByName(name) != std::end(m_extensions);
		}

		void Sort(const char* orderStr)
		{
			if (m_extensions.empty())
			{
				return;
			}

			size_t count = m_extensions.size();
			std::vector<const char*> names(count);
			for (size_t i = 0; i < count; ++i)
			{
				names[i] = m_extensions[i]->GetName();
			}
			std::vector<size_t> sortedIndices(count);
			nzbget_rs_feh_ext_sort_indices(names.data(), count, orderStr ? orderStr : "", sortedIndices.data());

			Extensions sorted;
			sorted.reserve(count);
			for (size_t idx : sortedIndices)
			{
				sorted.push_back(std::move(m_extensions[idx]));
			}
			m_extensions = std::move(sorted);
		}

		std::string GetExtensionName(const std::string& fileName) const
		{
			NzbgetRsBuf buf = nzbget_rs_ext_mgr_get_extension_name(fileName.data(), fileName.size());
			std::string res(buf.data ? buf.data : "", buf.len);
			nzbget_rs_free(buf);
			return res;
		}

		std::optional<std::string>
		GetExtensionPath(const Extension::Script& extension, fs::path& path)
		{
			const char* location = extension.GetLocation() ? extension.GetLocation() : "";
			const char* entry = extension.GetEntry() ? extension.GetEntry() : "";

			std::vector<const char*> allLocs;
			std::vector<size_t> allLens;
			allLocs.reserve(m_extensions.size());
			allLens.reserve(m_extensions.size());
			for (const auto& ext : m_extensions)
			{
				const char* l = ext->GetLocation() ? ext->GetLocation() : "";
				allLocs.push_back(l);
				allLens.push_back(strlen(l));
			}

			NzbgetRsBuf buf = nzbget_rs_ext_mgr_resolve_extension_path(
				location, strlen(location),
				entry, strlen(entry),
				allLocs.empty() ? nullptr : allLocs.data(),
				allLens.empty() ? nullptr : allLens.data(),
				allLocs.size());
			path = fs::path(std::string(buf.data ? buf.data : "", buf.len));
			nzbget_rs_free(buf);
			return std::nullopt;
		}

		Extensions m_extensions;
		mutable std::shared_mutex m_mutex;
	};
}

extern ExtensionManager::Manager* g_ExtensionManager;

#endif
