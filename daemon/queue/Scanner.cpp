/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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

#include <filesystem>
#include <fstream>
#include <memory>

#include "Scanner.h"
#include "Options.h"
#include "WorkState.h"
#include "Log.h"
#include "QueueCoordinator.h"
#include "ScanScript.h"
#include "Util.h"
#include "FileSystem.h"
#include "Unpack.h"
#include "nzbget_rs.h"

int Scanner::m_idGen = 0;

Scanner::QueueData::QueueData(
	const char* filename,
	const char* nzbName,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	EAddStatus* addStatus,
	int* nzbId) 
	: m_filename{ filename ? filename : "" }
	, m_nzbName{ nzbName ? nzbName : "" }
	, m_category{ category ? category : "" }
	, m_autoCategory{ autoCategory }
	, m_priority{ priority }
	, m_dupeKey{ dupeKey ? dupeKey : "" }
	, m_dupeScore{ dupeScore }
	, m_dupeMode{ dupeMode }
	, m_addTop{ addTop }
	, m_addPaused{ addPaused }
	, m_urlInfo{ urlInfo }
	, m_addStatus{ addStatus }
	, m_nzbId{ nzbId }
{
	if (parameters)
	{
		m_parameters.CopyFrom(parameters);
	}
}

void Scanner::QueueData::SetAddStatus(EAddStatus addStatus)
{
	if (m_addStatus)
	{
		*m_addStatus = addStatus;
	}
}

void Scanner::QueueData::SetNzbId(int nzbId)
{
	if (m_nzbId)
	{
		*m_nzbId = nzbId;
	}
}


void Scanner::InitOptions()
{
	m_nzbDirInterval = 1;
	m_scanScript = ScanScriptController::HasScripts();

	CleanupStaleUnpackDir();
}

int Scanner::ServiceInterval()
{
	return m_requestedNzbDirScan ? Service::Now :
		g_Options->GetNzbDirInterval() <= 0 ? Service::Sleep :
		m_nzbDirInterval;
}

void Scanner::ServiceWork()
{
	debug("Scanner service work");

	if (!DownloadQueue::IsLoaded())
	{
		return;
	}

	m_nzbDirInterval = g_Options->GetNzbDirInterval();

	if (g_WorkState->GetPauseScan() && !m_requestedNzbDirScan)
	{
		return;
	}

	debug("Scanner service work: doing work");

	std::lock_guard<std::mutex> guard{m_scanMutex};

	// check nzbdir every g_Options->GetNzbDirInterval() seconds or if requested
	bool checkStat = !m_requestedNzbDirScan;
	m_requestedNzbDirScan = false;
	m_scanning = true;

	if (g_Options->GetNzbDirArchiveScan())
	{
		CheckIncomingArchives(g_Options->GetNzbDirPath(), checkStat);
	}

	CheckIncomingNzbs(g_Options->GetNzbDir(), "", checkStat);
	if (!checkStat && m_scanScript)
	{
		// if immediate scan requested, we need second scan to process files extracted by scan-scripts
		CheckIncomingNzbs(g_Options->GetNzbDir(), "", checkStat);
	}
	m_scanning = false;

	// if NzbDirFileAge is less than NzbDirInterval (that can happen if NzbDirInterval
	// is set for rare scans like once per hour) we make 4 scans:
	//   - one additional scan is necessary to check sizes of detected files;
	//   - another scan is required to check files which were extracted by scan-scripts;
	//   - third scan is needed to check sizes of extracted files.
	nzbget_rs_dsp_scanner_next_pass(
		g_Options->GetNzbDirInterval(),
		g_Options->GetNzbDirFileAge(),
		m_scanScript ? 1 : 0,
		m_pass,
		&m_nzbDirInterval,
		&m_pass);

	DropOldFiles();
	m_queueList.clear();
}

void Scanner::CheckIncomingArchives(const fs::path& dir, bool checkStat)
{
	const auto archives = FindArchives(dir, checkStat);
	UnpackArchives(archives);
}

std::vector<fs::path> Scanner::FindArchives(const fs::path& dir, bool checkStat)
{
	// the non-throwing forms: an unreadable subdirectory (lost+found, a
	// folder of another user) or one removed during the walk threw out of
	// the scanner thread and ended the program
	std::vector<fs::path> archives;
	fs::error_code ec;
	auto it = fs::recursive_directory_iterator(dir, fs::directory_options::skip_permission_denied, ec);
	for (; !ec && it != fs::recursive_directory_iterator(); it.increment(ec))
	{
		const auto& path = it->path();
		const std::string filename = fs::u8string(path.filename());
		fs::error_code typeEc;

		if (ShouldSkipItem(filename))
		{
			if (it->is_directory(typeEc))
			{
				it.disable_recursion_pending();
			}
			continue;
		}

		if (!it->is_regular_file(typeEc))
		{
			continue;
		}

		// like an nzb, an archive is taken once it stopped changing: one
		// still being copied in failed to extract and was moved aside for good
		if (Unpack::IsArchive(path) && CanProcessFile(fs::u8string(path).c_str(), checkStat))
		{
			archives.push_back(path);
		}
	}
	if (ec)
	{
		warn("Could not scan %s for archives: %s", fs::u8string(dir).c_str(), ec.message().c_str());
	}

	archives.shrink_to_fit();
	return archives;
}

void Scanner::UnpackArchives(const std::vector<fs::path>& archives)
{
	if (archives.empty()) return;

	const auto processor = Incoming::ArchiveProcessor({
		g_Options->GetTempDirPath() / UNPACK_DIR,
		g_Options->GetNzbDirPath() / PROCESSED_DIR,
		g_Options->GetNzbDirPath() / BROKEN_DIR,
		g_Options->GetNzbDirArchiveAction()
	});

	for (const auto& archive : archives)
	{
		processor.Process(archive, archive.parent_path());
	}
}

void Scanner::CleanupStaleUnpackDir()
{
	const fs::path unpackDir = g_Options->GetTempDirPath() / UNPACK_DIR;
	fs::error_code ec;
	fs::remove_all(unpackDir, ec);
	if (ec)
	{
		warn("Could not remove stale unpack directory '%s/%s': %s (code %d)", 
			g_Options->GetTempDir(), UNPACK_DIR.data(), ec.message().c_str(), ec.value());	
	}
}

/**
* Check if there are files in directory for incoming nzb-files
* and add them to download queue
*/
void Scanner::CheckIncomingNzbs(const char* directory, const char* category, bool checkStat)
{
	DirBrowser dir(directory);
	while (const char* filename = dir.Next())
	{
		if (ShouldSkipItem(filename))
		{
			continue;
		}

		BString<1024> fullfilename("%s%c%s", directory, PATH_SEPARATOR, filename);
		bool isDirectory = FileSystem::DirectoryExists(fullfilename);

		if (isDirectory)
		{
			ProcessSubdirectory(fullfilename, filename, category, checkStat);
		}
		else if (!isDirectory && CanProcessFile(fullfilename, checkStat))
		{
			ProcessIncomingFile(directory, filename, fullfilename, category);
		}
	}
}

void Scanner::ProcessSubdirectory(const char* fullPath, const char* filename, const char* category, bool checkStat)
{
	BString<1024> subCategory;
	if (category && category[0] != '\0')
	{
		subCategory.Format("%s%c%s", category, PATH_SEPARATOR, filename);
	}
	else
	{
		subCategory = filename;
	}
	CheckIncomingNzbs(fullPath, subCategory, checkStat);
}

/**
 * Only files which were not changed during last g_pOptions->GetNzbDirFileAge() seconds
 * can be processed. That prevents the processing of files, which are currently being
 * copied into nzb-directory (eg. being downloaded in web-browser).
 */
bool Scanner::CanProcessFile(const char* fullFilename, bool checkStat)
{
	if (!nzbget_rs_dsp_can_process_extension(fullFilename))
	{
		return false;
	}

	if (!checkStat)
	{
		return true;
	}

	int64 size = FileSystem::FileSize(fullFilename);
	time_t current = Util::CurrentTime();
	bool canProcess = false;
	bool inList = false;

	for (FileList::iterator it = m_fileList.begin(); it != m_fileList.end(); ++it)
	{
		FileData& fileData = *it;
		if (!strcmp(fileData.GetFilename(), fullFilename))
		{
			inList = true;
			int64 updatedSize = fileData.GetSize();
			int64 updatedLastChange = (int64)fileData.GetLastChange();
			if (nzbget_rs_dsp_eval_file_age(
				fileData.GetSize(), size, (int64)fileData.GetLastChange(), (int64)current,
				g_Options->GetNzbDirFileAge(), &updatedSize, &updatedLastChange))
			{
				canProcess = true;
				m_fileList.erase(it);
			}
			else
			{
				fileData.SetSize(updatedSize);
				fileData.SetLastChange((time_t)updatedLastChange);
			}
			break;
		}
	}

	if (!inList)
	{
		m_fileList.emplace_back(fullFilename, size, current);
	}

	return canProcess;
}

/**
 * Remove old files from the list of monitored files.
 * Normally these files are deleted from the list when they are processed.
 * However if a file was detected by function "CanProcessFile" once but wasn't
 * processed later (for example if the user deleted it), it will stay in the list,
 * until we remove it here.
 */
void Scanner::DropOldFiles()
{
	time_t current = Util::CurrentTime();

	m_fileList.erase(std::remove_if(m_fileList.begin(), m_fileList.end(),
		[current](FileData& fileData)
		{
			if (nzbget_rs_dsp_should_drop_scan_file(
				(int64)fileData.GetLastChange(), (int64)current,
				g_Options->GetNzbDirInterval(), g_Options->GetNzbDirFileAge()))
			{
				debug("Removing file %s from scan file list", fileData.GetFilename());
				return true;
			}
			return false;
		}),
		m_fileList.end());
}

bool Scanner::ShouldSkipItem(std::string_view filename)
{
	std::string nameStr(filename);
	std::string procStr(PROCESSED_DIR);
	std::string brokStr(BROKEN_DIR);
	return nzbget_rs_scanner_should_skip_item(
		nameStr.c_str(), procStr.c_str(), brokStr.c_str()) != 0;
}

void Scanner::ProcessIncomingFile(
	const char* directory,
	const char* baseFilename,
	const char* fullFilename,
	const char* category
)
{
	const char* extension = strrchr(baseFilename, '.');
	if (!extension)
	{
		return;
	}

	std::string nzbName;
	std::string nzbCategory = category ? category : "";
	std::string dupeKey;
	NzbParameterList parameters;
	int priority = 0;
	bool addTop = false;
	bool addPaused = false;
	bool autoCategory = nzbCategory.empty();
	int dupeScore = 0;
	EDupeMode dupeMode = dmScore;
	EAddStatus addStatus = asSkipped;
	QueueData* queueData = nullptr;
	NzbInfo* nzbInfo = nullptr;
	int nzbId = 0;

	for (QueueData& queueData1 : m_queueList)
	{
		if (FileSystem::SameFilename(queueData1.GetFilename(), fullFilename))
		{
			queueData = &queueData1;
			nzbName = queueData->GetNzbName();
			nzbCategory = queueData->GetCategory();
			priority = queueData->GetPriority();
			dupeKey = queueData->GetDupeKey();
			dupeScore = queueData->GetDupeScore();
			dupeMode = queueData->GetDupeMode();
			addTop = queueData->GetAddTop();
			addPaused = queueData->GetAddPaused();
			parameters.CopyFrom(queueData->GetParameters());
			nzbInfo = queueData->GetUrlInfo();
			autoCategory = queueData->GetAutoCategory();
		}
	}

	InitPPParameters(nzbCategory.c_str(), &parameters, false);

	bool exists = true;

	bool ranScanScript = m_scanScript && strcasecmp(extension, ".nzb_processed");
	if (ranScanScript)
	{
		ScanScriptController::ExecuteScripts(fullFilename,
			nzbInfo,
			directory,
			nzbName,
			nzbCategory,
			&priority,
			&parameters,
			&addTop,
			&addPaused,
			dupeKey,
			&dupeScore,
			&dupeMode
		);
		exists = FileSystem::FileExists(fullFilename);
	}
	int renameToProcessed = 0;
	int incAction = nzbget_rs_scanner_classify_incoming_extension(
		ranScanScript ? 1 : 0, exists ? 1 : 0, extension, &renameToProcessed);
	if (renameToProcessed)
	{
		CString bakname2;
		bool renameOK = FileSystem::RenameBak(fullFilename, "processed", false, bakname2);
		if (!renameOK)
		{
			error("Could not rename file %s to %s: %s", fullFilename, *bakname2,
				*FileSystem::GetLastErrorMessage());
		}
	}
	if (incAction == 1)
	{
		CString renamedName;
		bool renameOK = FileSystem::RenameBak(fullFilename, "nzb", true, renamedName);
		if (renameOK)
		{
			bool added = AddFileToQueue(
				renamedName,
				nzbName.c_str(),
				nzbCategory.c_str(),
				autoCategory,
				priority,
				dupeKey.c_str(),
				dupeScore,
				dupeMode,
				&parameters,
				addTop,
				addPaused,
				nzbInfo,
				&nzbId
			);
			addStatus = added ? asSuccess : asFailed;
		}
		else
		{
			error("Could not rename file %s to %s: %s", fullFilename, *renamedName,
				*FileSystem::GetLastErrorMessage());
			addStatus = asFailed;
		}
	}
	else if (incAction == 2)
	{
		bool added = AddFileToQueue(
			fullFilename,
			nzbName.c_str(),
			nzbCategory.c_str(),
			autoCategory,
			priority,
			dupeKey.c_str(),
			dupeScore,
			dupeMode,
			&parameters,
			addTop,
			addPaused,
			nzbInfo,
			&nzbId
		);
		addStatus = added ? asSuccess : asFailed;
	}

	if (queueData)
	{
		queueData->SetAddStatus(addStatus);
		queueData->SetNzbId(nzbId);
	}
}

void Scanner::InitPPParameters(const char* category, NzbParameterList* parameters, bool reset)
{
	bool unpack = g_Options->GetUnpack();
	const char* extensions = g_Options->GetExtensions();

	if (!Util::EmptyStr(category))
	{
		Options::Category* categoryObj = g_Options->FindCategory(category, false);
		if (categoryObj)
		{
			unpack = categoryObj->GetUnpack();
			if (!Util::EmptyStr(categoryObj->GetExtensions()))
			{
				extensions = categoryObj->GetExtensions();
			}
		}
	}

	if (reset)
	{
		g_ExtensionManager->ForEach([&parameters](auto script)
			{
				BString<1024> paramName("%s:", script->GetName());
				parameters->SetParameter(paramName, nullptr);
			}
		);
	}

	if (!parameters->Find("*Unpack:"))
	{
		parameters->SetParameter("*Unpack:", unpack ? "yes" : "no");
	}

	if (!Util::EmptyStr(extensions))
	{
		// create pp-parameter for each post-processing or queue- script
		g_ExtensionManager->ForEach(
			[&](auto script)
			{
				Tokenizer tok(extensions, ",;");
				while (const char* scriptName = tok.Next())
				{
					if (strcmp(scriptName, script->GetName()) != 0) continue;

					BString<1024> paramName("%s:", scriptName);
					if ((script->GetPostScript() || script->GetQueueScript()) && !parameters->Find(paramName))
					{
						parameters->SetParameter(paramName, "yes");
					}
				}
			});
	}
}

CString Scanner::ResolveCategory(const char* category, const char* filename)
{
	CString useCategory = category ? category : "";
	Options::Category* categoryObj = g_Options->FindCategory(useCategory, true);
	if (categoryObj && strcmp(useCategory, categoryObj->GetName()) != 0)
	{
		useCategory = categoryObj->GetName();
		detail("Category '%s' matched to '%s' for '%s'", category, *useCategory, filename);
	}
	return useCategory;
}

void Scanner::DetectAndSetCategory(const NzbFile& nzbFile, NzbInfo& nzbInfo, const char* nzbName)
{
	const std::string& nzbCategory = nzbFile.GetCategoryFromFile();
	if (nzbCategory.empty())
	{
		detail("Auto-category: no category detected in '%s'", nzbName);
		return;
	}
	Options::Category* categoryObj = g_Options->FindCategory(nzbCategory.c_str(), true);
	if (categoryObj)
	{
		const char* category = categoryObj->GetName();
		detail("Category '%s' matched to '%s' for '%s'", category, nzbCategory.c_str(), nzbName);
		nzbInfo.SetCategory(category);
	}
	else
	{
		detail("Auto-category: using detected category '%s' for '%s'", nzbCategory.c_str(), nzbName);
		nzbInfo.SetCategory(nzbCategory.c_str());
	}
}

bool Scanner::AddFileToQueue(
	const char* filename,
	const char* nzbName,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	int* nzbId
)
{
	const char* basename = FileSystem::BaseFileName(filename);

	info("Adding collection %s to queue", basename);

	NzbFile nzbFile(filename, category);
	bool ok = nzbFile.Parse();
	if (!ok)
	{
		error("Could not add collection %s to queue", basename);
	}

	CString bakname2;
	const char* bakSuffix = ok ? "queued" : "error";
	if (!FileSystem::RenameBak(filename, bakSuffix, false, bakname2))
	{
		ok = false;
		error("Could not rename file %s to %s: %s", filename, *bakname2,
			*FileSystem::GetLastErrorMessage());
	}

	std::unique_ptr<NzbInfo> nzbInfo = nzbFile.DetachNzbInfo();
	nzbInfo->SetQueuedFilename(bakname2);

	if (autoCategory)
	{
		DetectAndSetCategory(nzbFile, *nzbInfo, nzbName);
		InitPPParameters(nzbInfo->GetCategory(), parameters, true);
	}

	if (!Util::EmptyStr(nzbName))
	{
		nzbInfo->SetName(nullptr);
		nzbInfo->SetFilename(nzbName);
		nzbInfo->BuildDestDirName();
	}

	if (ok)
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			// a warning when the nzb stops short of what the subject declares or
			// leaves out much of a file (a fragment listing 1 of 9374 parts); a
			// segment or two an indexer dropped is common and only a detail
			if (fileInfo->GetMissedArticles() > 0)
			{
				bool cutShort = nzbFile.GetCutShortFiles().count(fileInfo) > 0;
				int sev = nzbget_rs_dsp_classify_missed_articles(
					fileInfo->GetMissedArticles(), fileInfo->GetTotalArticles(), cutShort ? 1 : 0);
				int presentArticles = fileInfo->GetTotalArticles() - fileInfo->GetMissedArticles();
				nzbInfo->PrintMessage(
					sev == 2 ? Message::mkWarning : Message::mkDetail,
					"Collection %s lists only %i of the %i articles of %s: the missing ones count as failed",
					nzbInfo->GetName(), presentArticles,
					fileInfo->GetTotalArticles(), fileInfo->GetFilename());
			}
		}
	}

	nzbInfo->SetDupeKey(dupeKey);
	nzbInfo->SetDupeScore(dupeScore);
	nzbInfo->SetDupeMode(dupeMode);
	nzbInfo->SetPriority(priority);
	if (urlInfo)
	{
		nzbInfo->SetUrl(urlInfo->GetUrl());
		nzbInfo->SetUrlStatus(urlInfo->GetUrlStatus());
		nzbInfo->SetFeedId(urlInfo->GetFeedId());
		nzbInfo->SetDupeHint(urlInfo->GetDupeHint());
		nzbInfo->SetDesiredServerId(urlInfo->GetDesiredServerId());
		nzbInfo->SetSkipScriptProcessing(urlInfo->GetSkipScriptProcessing());
		nzbInfo->SetSkipDiskWrite(urlInfo->GetSkipDiskWrite());
	}

	if (!nzbFile.GetPassword().empty())
	{
		nzbInfo->GetParameters()->SetParameter("*Unpack:Password", nzbFile.GetPassword().c_str());
	}

	nzbInfo->GetParameters()->CopyFrom(parameters);

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		fileInfo->SetPaused(addPaused);
	}

	NzbInfo* addedNzb = nullptr;

	if (ok)
	{
		addedNzb = g_QueueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), std::move(urlInfo), addTop);
	}
	else if (urlInfo)
	{
		for (Message& message : nzbInfo->GuardCachedMessages())
		{
			urlInfo->AddMessage(message.GetKind(), message.GetText(), false);
		}
	}
	else
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsScan);
		addedNzb = g_QueueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), std::move(urlInfo), addTop);
	}

	if (nzbId)
	{
		*nzbId = addedNzb ? addedNzb->GetId() : 0;
	}

	return ok;
}

void Scanner::ScanNzbDir(bool syncMode)
{
	{
		std::lock_guard<std::mutex> guard{m_scanMutex};
		m_scanning = true;
		m_requestedNzbDirScan = true;
		WakeUp();
	}

	while (syncMode && (m_scanning || m_requestedNzbDirScan))
	{
		Util::Sleep(100);
	}
}

Scanner::EAddStatus Scanner::AddArchive(const char* filename, const char* category,
										bool autoCategory, int priority, const char* dupeKey,
										int dupeScore, EDupeMode dupeMode,
										NzbParameterList* parameters, bool addTop, bool addPaused,
										NzbInfo* urlInfo, const char* buffer, int bufSize)
{
	if (Util::EmptyStr(filename))
	{
		error("Failed to add the archive to the download queue: Archive filename cannot be empty");
		return EAddStatus::asFailed;
	}

	if (g_Options->GetTempDirPath().empty())
	{
		error("Failed to create file '%s': '%s' is required and cannot be empty",
			filename, Options::TEMPDIR.data());
		return EAddStatus::asFailed;
	}

	if (g_Options->GetNzbDirPath().empty())
	{
		error("Failed to create file '%s': '%s' is required and cannot be empty",
			filename, Options::NZBDIR.data());
		return EAddStatus::asFailed;
	}

	const auto archiveFilename = FileSystem::MakeValidFilename(filename);
	const auto archiveFilePath = g_Options->GetTempDirPath() / *archiveFilename;

	fs::error_code ec;
	{
		std::ofstream file(archiveFilePath.c_str(), std::ios::binary);
		if (!file.is_open())
		{
			error("Failed to create archive file '%s' in temporary directory: %s",
				*archiveFilename, std::strerror(errno));
			return EAddStatus::asFailed;
		}

		file.write(buffer, bufSize);
		file.close();
		if (!file)
		{
			error("Failed to write data to archive file '%s': %s",
				*archiveFilename, std::strerror(errno));
			fs::remove(archiveFilePath, ec);
			return EAddStatus::asFailed;
		}
	}

	if (!g_Options->GetNzbDirArchiveScan()) 
	{
		const auto finalArchiveFilePath = g_Options->GetNzbDirPath() / *archiveFilename;
		fs::move_file(archiveFilePath, finalArchiveFilePath, ec);
		if (ec)
		{
			error("Failed to move file '%s' to %s: %s",
				*archiveFilename, Options::NZBDIR.data(), ec.message().c_str());
			return EAddStatus::asFailed;
		}

		info("Skipping processing of '%s': option '%s' is disabled",
			*archiveFilename, Options::NZBDIRARCHIVESCAN.data());
		return EAddStatus::asSkipped;
	}

	std::vector<EAddStatus> statuses;
	{
		std::lock_guard<std::mutex> guard{m_scanMutex};

		const auto processor = Incoming::ArchiveProcessor({
			g_Options->GetTempDirPath() / UNPACK_DIR,
			g_Options->GetNzbDirPath() / PROCESSED_DIR,
			g_Options->GetNzbDirPath() / BROKEN_DIR,
			g_Options->GetNzbDirArchiveAction()
		});

		auto nzbFiles = processor.Process(archiveFilePath, g_Options->GetNzbDirPath());
		if (!nzbFiles)
		{
			return EAddStatus::asFailed;
		}

		if (nzbFiles->empty())
		{
			return EAddStatus::asSkipped;
		}

		// the url item goes with the first nzb only: adding it takes the item
		// out of the queue and frees it, and the next nzb of the archive read
		// the freed item (and could take another queue entry out with it).
		// Each nzb reports how it went (sized up front: the entries point here)
		statuses.assign(nzbFiles->size(), EAddStatus::asSkipped);
		size_t index = 0;
		for (const auto& nzbFile : *nzbFiles)
		{
			const auto nzbFilePathStr = fs::u8string(nzbFile);
			const auto fileName = fs::u8string(nzbFile.filename());
			const CString useCategory = ResolveCategory(category, fileName.c_str());
			m_queueList.emplace_back(
				nzbFilePathStr.c_str(), fileName.c_str(), useCategory,
				autoCategory, priority, dupeKey, dupeScore, dupeMode,
				parameters, addTop, addPaused,
				index == 0 ? urlInfo : nullptr,
				&statuses[index], nullptr);
			index++;
		}
		// requested under the lock, as in AddExternalFile: a periodic scan
		// waiting for the lock skipped the young files, cleared their entries
		// and they were added later without category, key, score or url (F14)
		m_requestedNzbDirScan = true;
	}

	ScanNzbDir(true);

	std::vector<int> statusInts;
	statusInts.reserve(statuses.size());
	for (EAddStatus st : statuses)
	{
		statusInts.push_back((int)st);
	}
	return (EAddStatus)nzbget_rs_scanner_resolve_archive_add_status(
		urlInfo ? 1 : 0,
		statusInts.data(),
		statusInts.size(),
		(int)EAddStatus::asSuccess,
		(int)EAddStatus::asSkipped);
}

Scanner::EAddStatus Scanner::AddArchive(
	const char* filename,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	const char* tmpFilename)
{
	CharBuffer buffer;
	if (!FileSystem::LoadFileIntoBuffer(tmpFilename, buffer, false))
	{
		error("Could not open file '%s'", tmpFilename);
		return EAddStatus::asFailed;
	}
	fs::error_code ec;
	fs::remove(tmpFilename, ec);
	if (ec)
	{
		const auto msg = ec.message();
		error("Failed to remove '%s': %s (code %d)", tmpFilename, msg.c_str(), ec.value());
	}

	return AddArchive(
		filename,
		category,
		autoCategory,
		priority,
		dupeKey,
		dupeScore,
		dupeMode,
		parameters,
		addTop,
		addPaused,
		urlInfo,
		*buffer,
		buffer.Size()
	);
}

Scanner::EAddStatus Scanner::AddExternalFile(
	const char* nzbName,
	const char* category,
	bool autoCategory,
	int priority,
	const char* dupeKey,
	int dupeScore,
	EDupeMode dupeMode,
	NzbParameterList* parameters,
	bool addTop,
	bool addPaused,
	NzbInfo* urlInfo,
	const char* fileName,
	const char* buffer,
	int bufSize,
	int* nzbId
)
{
	bool nzb = false;
	BString<1024> tempFileName;

	// the names are picked and taken under the lock: two appends at once picked the
	// same temporary or queue file name, one overwrote the other and returned -1 (F14)
	std::unique_lock<std::mutex> guard{m_scanMutex};

	if (fileName)
	{
		tempFileName = fileName;
	}
	else
	{
		int num = ++m_idGen;
		while (tempFileName.Empty() || FileSystem::FileExists(tempFileName))
		{
			tempFileName.Format("%s%cnzb-%i.tmp", g_Options->GetTempDir(), PATH_SEPARATOR, num);
			num++;
		}

		if (!FileSystem::SaveBufferIntoFile(tempFileName, buffer, bufSize))
		{
			error("Could not create file %s", *tempFileName);
			return asFailed;
		}

		nzb = nzbget_rs_dsp_is_nzb_xml_prefix((const uint8_t*)buffer, (size_t)std::max(0, bufSize)) != 0;
	}

	// move file into NzbDir, make sure the file name is unique
	// a name that is a full path (a client passing its file) drops the folder; any
	// other "/" belongs to the name (indexers post "[hash] [16/22] - Title"), and it
	// lost everything up to it ("22] - Title"). MakeValidFilename makes it safe
	bool absolutePath = nzbget_rs_scanner_is_absolute_nzb_name(nzbName) != 0;
	CString validNzbName = FileSystem::MakeValidFilename(absolutePath ? FileSystem::BaseFileName(nzbName) : nzbName);
	{
		NzbgetRsBuf normBuf = nzbget_rs_dsp_normalize_external_nzb_name(nzbName, validNzbName, nzb ? 1 : 0);
		std::string normStr(normBuf.data ? normBuf.data : "", normBuf.len);
		nzbget_rs_free(normBuf);
		validNzbName = normStr.c_str();
	}

	BString<1024> scanFileName("%s%c%s", g_Options->GetNzbDir(), PATH_SEPARATOR, *validNzbName);

	const char* validCStr = validNzbName ? *validNzbName : "";
	size_t validLen = strlen(validCStr);
	ptrdiff_t dotOff = nzbget_rs_file_extension(validCStr, validLen);
	bool hasExt = dotOff >= 0;
	std::string basePart(validCStr, hasExt ? (size_t)dotOff : validLen);
	std::string extPart = hasExt ? std::string(validCStr + dotOff + 1) : std::string();

	int num = 2;
	while (FileSystem::FileExists(scanFileName))
	{
		if (hasExt)
		{
			scanFileName.Format("%s%c%s_%i.%s", g_Options->GetNzbDir(), PATH_SEPARATOR,
				basePart.c_str(), num, extPart.c_str());
		}
		else
		{
			scanFileName.Format("%s%c%s_%i", g_Options->GetNzbDir(), PATH_SEPARATOR,
				basePart.c_str(), num);
		}
		num++;
	}

	EAddStatus addStatus;

	{
		if (!FileSystem::MoveFile(tempFileName, scanFileName))
		{
			error("Could not move file %s to %s: %s", *tempFileName, *scanFileName,
				*FileSystem::GetLastErrorMessage());
			FileSystem::DeleteFile(tempFileName);
			return asFailed;
		}

		CString useCategory = ResolveCategory(category, nzbName);

		addStatus = asSkipped;
		m_queueList.emplace_back(
			scanFileName,
			nzbName,
			useCategory,
			autoCategory,
			priority,
			dupeKey,
			dupeScore,
			dupeMode,
			parameters,
			addTop,
			addPaused,
			urlInfo,
			&addStatus,
			nzbId
		);
		// requested under the lock: a periodic scan that took the lock first skipped
		// the file (too young) and cleared its entry, and the file was added later
		// without the name, key and score it was sent with (F14)
		m_requestedNzbDirScan = true;
	}
	guard.unlock();

	ScanNzbDir(true);

	return addStatus;
}
