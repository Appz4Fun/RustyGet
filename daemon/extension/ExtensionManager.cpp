/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2023-2026 Denis <denis@nzbget.com>
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

#include <atomic>

#include "Util.h"
#include "Unpack.h"
#include "ExtensionLoader.h"
#include "ExtensionManager.h"
#include "Options.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

namespace ExtensionManager
{
	void Manager::ForEach(std::function<void(const ExtensionPtr)> callback) const
	{
		std::shared_lock<std::shared_mutex> lock{ m_mutex };

		std::for_each(std::cbegin(m_extensions), std::cend(m_extensions), callback);
	}

	std::optional<ExtensionPtr>
	Manager::FindIf(std::function<bool(ExtensionPtr)> comparator) const
	{
		std::shared_lock<std::shared_mutex> lock{ m_mutex };

		auto found = std::find_if(std::cbegin(m_extensions), std::cend(m_extensions), comparator);
		if (found != std::cend(m_extensions))
		{
			return *found;
		}

		return std::nullopt;
	}

	std::pair<WebDownloader::EStatus, fs::path>
	Manager::DownloadExtension(const std::string& url, const std::string& extName)
	{
		// a name of our own, not the extension's: that one comes from the request
		// ("../x" wrote anywhere the daemon can), and two downloads of one
		// extension at once wrote the same file
		static std::atomic<int> counter{0};
		NzbgetRsBuf zipNameBuf = nzbget_rs_tpr_ext_mgr_format_temp_zip_name(
			(int64_t)Util::CurrentTime(), ++counter);
		fs::path tmpFileName = g_Options->GetTempDirPath() /
			std::string(zipNameBuf.data ? zipNameBuf.data : "", zipNameBuf.len);
		nzbget_rs_free(zipNameBuf);

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

	std::optional<std::string>
	Manager::UpdateExtension(const fs::path& filename, const std::string& extName)
	{
		std::unique_lock<std::shared_mutex> lock{m_mutex};

		auto extensionIt = GetByName(extName);
		if (extensionIt == std::end(m_extensions))
		{
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_lookup_error(
				extName.data(), extName.size(), 0);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		if (extensionIt->use_count() > 1)
		{
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_lookup_error(
				extName.data(), extName.size(), 1);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		// the installed extension is moved aside, not deleted, until the new one
		// is in place: a failed install puts it back
		fs::path targetPath;
		auto moveAsideError = GetExtensionPath(*(*extensionIt), targetPath);
		std::string targetPathStr = fs::u8string(targetPath);
		NzbgetRsBuf backupBuf = nzbget_rs_dqh_ext_mgr_format_update_backup_path(
			targetPathStr.data(), targetPathStr.size());
		fs::path backupPath = fs::u8path(std::string(backupBuf.data ? backupBuf.data : "", backupBuf.len));
		nzbget_rs_free(backupBuf);
		if (!moveAsideError)
		{
			fs::error_code ec;
			fs::remove_all(backupPath, ec);
			fs::rename(targetPath, backupPath, ec);
			if (ec)
			{
				std::string ecMsg = ec.message();
				NzbgetRsBuf maBuf = nzbget_rs_dqh_ext_mgr_format_install_or_delete_error(
					targetPathStr.data(), targetPathStr.size(), ecMsg.data(), ecMsg.size(), 3);
				moveAsideError = std::string(maBuf.data ? maBuf.data : "", maBuf.len);
				nzbget_rs_free(maBuf);
			}
		}
		if (moveAsideError)
		{
			fs::error_code ec;
			fs::remove(filename, ec);
			if (ec)
			{
				std::string fnStr = fs::u8string(filename);
				std::string ecMsg = ec.message();
				NzbgetRsBuf cmpBuf = nzbget_rs_dqh_ext_mgr_format_compound_update_error(
					moveAsideError->data(), moveAsideError->size(),
					fnStr.data(), fnStr.size(),
					nullptr, 0,
					ecMsg.data(), ecMsg.size(), 0);
				std::string err(cmpBuf.data ? cmpBuf.data : "", cmpBuf.len);
				nzbget_rs_free(cmpBuf);
				return err;
			}

			return moveAsideError;
		}
		const auto rootDir = (*extensionIt)->GetRootDir();
		const auto installExtError = InstallExtension(filename, rootDir);
		if (installExtError)
		{
			fs::error_code ec;
			fs::remove(filename, ec);
			fs::remove_all(targetPath, ec);
			fs::rename(backupPath, targetPath, ec);
			if (ec)
			{
				std::string backupStr = fs::u8string(backupPath);
				std::string ecMsg = ec.message();
				NzbgetRsBuf cmpBuf = nzbget_rs_dqh_ext_mgr_format_compound_update_error(
					installExtError->data(), installExtError->size(),
					targetPathStr.data(), targetPathStr.size(),
					backupStr.data(), backupStr.size(),
					ecMsg.data(), ecMsg.size(), 1);
				std::string err(cmpBuf.data ? cmpBuf.data : "", cmpBuf.len);
				nzbget_rs_free(cmpBuf);
				return err;
			}
			return installExtError;
		}

		fs::error_code ec;
		fs::remove_all(backupPath, ec);
		m_extensions.erase(extensionIt);
		return std::nullopt;
	}

	std::optional<std::string> Manager::InstallExtension(const fs::path& file,
														 const fs::path& dest)
	{
		const auto extractor =
			Unpack::MakeExtractor(file, dest, "", Unpack::OverwriteMode::Overwrite);
		std::string fileStr = fs::u8string(file);

		if (!extractor)
		{
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_install_or_delete_error(
				fileStr.data(), fileStr.size(), nullptr, 0, 0);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		if (!extractor->Extract())
		{
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_install_or_delete_error(
				fileStr.data(), fileStr.size(), nullptr, 0, 1);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		fs::error_code ec;
		fs::remove(file, ec);
		if (ec)
		{
			std::string ecMsg = ec.message();
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_install_or_delete_error(
				fileStr.data(), fileStr.size(), ecMsg.data(), ecMsg.size(), 2);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		return std::nullopt;
	}

	std::optional<std::string>
	Manager::DeleteExtension(const std::string& name)
	{
		std::unique_lock<std::shared_mutex> lock{m_mutex};

		auto extensionIt = GetByName(name);
		if (extensionIt == std::end(m_extensions))
		{
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_lookup_error(
				name.data(), name.size(), 2);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		if (extensionIt->use_count() > 1)
		{
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_lookup_error(
				name.data(), name.size(), 3);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		const auto err = DeleteExtension(*(*extensionIt));
		if (err)
		{
			return err;
		}

		m_extensions.erase(extensionIt);
		return std::nullopt;
	}

	std::optional<std::string>
	Manager::LoadExtensions()
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

	std::optional<std::string>
	Manager::DeleteExtension(const Extension::Script& extension)
	{
		fs::path targetPath;
		const auto err = GetExtensionPath(extension, targetPath);
		if (err)
		{
			return err;
		}

		fs::error_code ec;
		fs::remove_all(targetPath, ec);
		if (ec)
		{
			std::string tpStr = fs::u8string(targetPath);
			std::string ecMsg = ec.message();
			NzbgetRsBuf errBuf = nzbget_rs_dqh_ext_mgr_format_install_or_delete_error(
				tpStr.data(), tpStr.size(), ecMsg.data(), ecMsg.size(), 4);
			std::string err(errBuf.data ? errBuf.data : "", errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		return std::nullopt;
	}

	std::optional<std::string>
	Manager::GetExtensionPath(const Extension::Script& extension, fs::path& path)
	{
		const char* location = extension.GetLocation();

		ptrdiff_t count = std::count_if(
			std::begin(m_extensions),
			std::end(m_extensions),
			[&location](const auto& ext) { return nzbget_rs_dqh_bytes_equal(location, ext->GetLocation()) != 0; }
		);

		if (count > 1)
		{
			// for backward compatibility, when multiple V1 extensions placed 
			// in the same directory in which case we have to delete an entry file,
			// not the entire directory.
			location = extension.GetEntry();
		}

		fs::path targetPath(location);

		path = std::move(targetPath);
		return std::nullopt;
	}
	
	void Manager::LoadExtensionDir(const char* directory, bool isSubDir, const char* rootDir)
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
			if (nzbget_rs_feh_ext_should_skip_dir_entry(filename))
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

	void Manager::CreateTasks() const
	{
		for (const auto& extension : m_extensions)
		{
			if (!extension->GetSchedulerScript() || Util::EmptyStr(extension->GetTaskTime()))
			{
				continue;
			}
			if (nzbget_rs_feh_ext_is_in_list(g_Options->GetExtensions(), extension->GetName()))
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

	bool Manager::Exists(const std::string& name) const
	{
		return GetByName(name) != std::end(m_extensions);
	}

	void Manager::Sort(const char* orderStr)
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

	std::string Manager::GetExtensionName(const std::string& fileName) const
	{
		size_t stemLen = nzbget_rs_feh_ext_get_stem_len(fileName.data(), fileName.size());
		if (stemLen < fileName.size())
		{
			return fileName.substr(0, stemLen);
		}
		return fileName;
	}

	Extensions::const_iterator Manager::GetByName(const std::string& name) const
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
}
