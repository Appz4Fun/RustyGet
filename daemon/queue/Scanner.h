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
 *
 */


#ifndef SCANNER_H
#define SCANNER_H

#include <mutex>
#include <string>
#include "Util.h"
#include "DownloadInfo.h"
#include "Options.h"
#include "Log.h"
#include "DiskState.h"

class Scanner final : public Service
{
public:
	static constexpr std::string_view PROCESSED_DIR = "_processed";
	static constexpr std::string_view BROKEN_DIR = "_broken";
	static constexpr std::string_view UNPACK_DIR = "_unpack";

	enum EAddStatus
	{
		asSkipped,
		asSuccess,
		asFailed
	};

	void InitOptions();
	void ScanNzbDir(bool syncMode);
	EAddStatus AddArchive(
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
		const char* buffer,
		int bufSize
	);
	EAddStatus AddArchive(
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
		const char* tmpFilename
	);
	EAddStatus AddExternalFile(
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
	);
	void InitPPParameters(const char* category, NzbParameterList* parameters, bool reset);
	CString ResolveCategory(const char* category, const char* filename);

protected:
	int ServiceInterval() override;
	void ServiceWork() override;

private:
	class FileData
	{
	public:
		FileData(const char* filename, int64 size, time_t lastChange)
			: m_filename{ filename ? filename : "" }
			, m_size{ size }
			, m_lastChange{ lastChange } {}
		const char* GetFilename() const { return m_filename.c_str(); }
		int64 GetSize() { return m_size; }
		void SetSize(int64 size) { m_size = size; }
		time_t GetLastChange() { return m_lastChange; }
		void SetLastChange(time_t lastChange) { m_lastChange = lastChange; }
	private:
		std::string m_filename;
		int64 m_size;
		time_t m_lastChange;
	};

	using FileList = std::deque<FileData>;

	class QueueData
	{
	public:
		QueueData(
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
			int* nzbId
		);
		const char* GetFilename() const { return m_filename.c_str(); }
		const char* GetNzbName() const { return m_nzbName.c_str(); }
		const char* GetCategory() const { return m_category.c_str(); }
		int GetPriority() { return m_priority; }
		const char* GetDupeKey() const { return m_dupeKey.c_str(); }
		int GetDupeScore() { return m_dupeScore; }
		EDupeMode GetDupeMode() { return m_dupeMode; }
		NzbParameterList* GetParameters() { return &m_parameters; }
		bool GetAddTop() { return m_addTop; }
		bool GetAddPaused() { return m_addPaused; }
		bool GetAutoCategory() const { return m_autoCategory; }
		NzbInfo* GetUrlInfo() { return m_urlInfo; }
		void SetAddStatus(EAddStatus addStatus);
		void SetNzbId(int nzbId);
	private:
		NzbParameterList m_parameters;
		std::string m_filename;
		std::string m_nzbName;
		std::string m_category;
		std::string m_dupeKey;
		int m_priority;
		int m_dupeScore;
		bool m_autoCategory;
		bool m_addTop;
		bool m_addPaused;
		EDupeMode m_dupeMode;
		NzbInfo* m_urlInfo;
		EAddStatus* m_addStatus;
		int* m_nzbId;
	};

	using QueueList = std::deque<QueueData>;

	FileList m_fileList;
	QueueList m_queueList;
	std::mutex m_scanMutex;
	std::atomic<bool> m_requestedNzbDirScan{false};
	std::atomic<bool> m_scanning{false};
	inline static int m_idGen = 0;
	int m_nzbDirInterval = 0;
	int m_pass = 0;
	bool m_scanScript = false;
	
	/**
	 * @brief Detects the category from the NZB file and sets it in the NzbInfo object.
	 *
	 * If a category is found in the NZB file and matches a configured category, it is set in nzbInfo.
	 * Otherwise, the detected category (if any) is set directly.
	 */
	void DetectAndSetCategory(const NzbFile& nzbFile, NzbInfo& nzbInfo, const char* nzbName);
	void CheckIncomingArchives(const fs::path& dir, bool checkStat);
	std::vector<fs::path> FindArchives(const fs::path& dir, bool checkStat);
	void UnpackArchives(const std::vector<fs::path>& archives);
	std::optional<std::vector<fs::path>> ProcessArchive(const fs::path& archiveFile, const fs::path& destDir);
	void CleanupStaleUnpackDir();
	void CheckIncomingNzbs(const char* directory, const char* category, bool checkStat);
	bool ShouldSkipItem(std::string_view filename);
	bool AddFileToQueue(
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
	);
	void ProcessIncomingFile(
		const char* directory, 
		const char* baseFilename,
		const char* fullFilename,
		const char* category
	);

	/**
	 * @brief Process subdirectories recursively to build category paths
	 *
	 * When a subdirectory is encountered, it's treated as a category name.
	 * The category path is built by concatenating the parent category with
	 * the subdirectory name. This allows for nested
	 * category structures like "Movies/Action" or "TV/Comedy".
	 */
	void ProcessSubdirectory(
		const char* fullPath, 
		const char* filename, 
		const char* category, 
		bool checkStat
	);
	bool CanProcessFile(const char* fullFilename, bool checkStat);
	void DropOldFiles();
};

extern Scanner* g_Scanner;

#include "Log.h"
#include "Connection.h"
#include "DownloadInfo.h"
class UrlDownloader;

class UrlCoordinator : public Thread, public Observer, public Debuggable
{
public:
	UrlCoordinator();
	~UrlCoordinator() override;
	void Run() override;
	void Stop() override;
	void Update(Subject* caller, void* aspect) override;

	// Editing the queue
	void AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst);
	bool HasMoreJobs() const { return m_hasMoreJobs; }
	bool DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory);

protected:
	void LogDebugInfo() override;

private:
	using ActiveDownloads = std::list<UrlDownloader*>;

	class DownloadQueueObserver final : public Observer
	{
	public:
		UrlCoordinator* m_owner;
		void Update(Subject* caller, void* aspect) override { m_owner->DownloadQueueUpdate(caller, aspect); }
	};

	ActiveDownloads m_activeDownloads;
	std::atomic<bool> m_hasMoreJobs{true};
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	DownloadQueueObserver m_downloadQueueObserver;

	NzbInfo* GetNextUrl(DownloadQueue* downloadQueue);
	void StartUrlDownload(NzbInfo* nzbInfo);
	void UrlCompleted(UrlDownloader* urlDownloader);
	void ResetHangingDownloads();
	void WaitJobs();
	void DownloadQueueUpdate(Subject* caller, void* aspect);
};

extern UrlCoordinator* g_UrlCoordinator;

class UrlDownloader : public WebDownloader
{
public:
	void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
	NzbInfo* GetNzbInfo() { return m_nzbInfo; }
	const char* GetCategory() const { return m_category; }

protected:
	void ProcessHeader(const char* line) override;

private:
	NzbInfo* m_nzbInfo;
	CString m_category;
};



#include <filesystem>
#include <fstream>
#include <memory>
#include <optional>

#include "Options.h"
#include "Log.h"
#include "QueueCoordinator.h"
#include "ExtensionManager.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

class ScanScriptCheck : public NzbScriptController
{
protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script) { has |= script->GetScanScript(); }
	bool has = false;
	friend class ScanScriptController;
};

class ScanScriptController : public NzbScriptController
{
public:
	ScanScriptController(
		std::string& nzbName,
		std::string& category,
		std::string& dupeKey)
		: m_nzbFilename(nullptr)
		, m_url(nullptr)
		, m_directory(nullptr)
		, m_nzbName{ nzbName }
		, m_category{ category }
		, m_dupeKey{ dupeKey }
		, m_priority(nullptr)
		, m_dupeScore(nullptr)
		, m_addTop(nullptr)
		, m_addPaused(nullptr)
		, m_dupeMode(nullptr)
		, m_parameters(nullptr) {}

	static void ExecuteScripts(
		const char* nzbFilename,
		NzbInfo* nzbInfo,
		const char* directory,
		std::string& nzbName,
		std::string& category,
		int* priority,
		NzbParameterList* parameters,
		bool* addTop,
		bool* addPaused,
		std::string& dupeKey,
		int* dupeScore,
		EDupeMode* dupeMode)
	{
		if (nzbInfo && nzbInfo->GetSkipScriptProcessing())
		{
			return;
		}

		ScanScriptController scriptController(nzbName, category, dupeKey);
		scriptController.m_nzbFilename = nzbFilename;
		scriptController.m_url = nzbInfo ? nzbInfo->GetUrl() : "";
		scriptController.m_directory = directory;
		scriptController.m_parameters = parameters;
		scriptController.m_priority = priority;
		scriptController.m_addTop = addTop;
		scriptController.m_addPaused = addPaused;
		scriptController.m_dupeScore = dupeScore;
		scriptController.m_dupeMode = dupeMode;
		scriptController.m_prefixLen = 0;

		const char* extensions = g_Options->GetExtensions();

		if (!category.empty())
		{
			Options::Category* categoryObj = g_Options->FindCategory(category.c_str(), false);
			if (categoryObj && !Util::EmptyStr(categoryObj->GetExtensions()))
			{
				extensions = categoryObj->GetExtensions();
			}
		}

		scriptController.ExecuteScriptList(extensions);
	}

	static bool HasScripts()
	{
		ScanScriptCheck check;
		check.ExecuteScriptList(g_Options->GetExtensions());
		// a category's extensions replace the global ones for its nzbs
		for (Options::Category& category : g_Options->GetCategories())
		{
			check.ExecuteScriptList(category.GetExtensions());
		}
		return check.has;
	}

protected:
	void ExecuteScript(std::shared_ptr<const Extension::Script> script) override
	{
		if (!script->GetScanScript() || !FileSystem::FileExists(m_nzbFilename))
		{
			return;
		}

		PrintMessage(Message::mkInfo, "Executing scan-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbFilename));

		SetArgs({script->GetEntry()});

		BString<1024> infoName("scan-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbFilename));
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		m_prefixLen = static_cast<int>(strlen(script->GetDisplayName())) + 2; // 2 = strlen(": ");
		PrepareParams(script->GetName());

		Execute();

		SetLogPrefix(nullptr);
	}

	void AddMessage(Message::EKind kind, const char* text) override
	{
		const char* msgText = text + m_prefixLen;

		NzbgetRsScanScriptCmd cmd{};
		int cmdKind = nzbget_rs_sus_parse_scan_script_cmd(msgText, &cmd);
		if (cmdKind != 0)
		{
			debug("Command %s detected", msgText + 6);
			switch (cmdKind)
			{
				case 1:
					m_nzbName = msgText + cmd.val_offset;
					break;
				case 2:
					m_category = msgText + cmd.val_offset;
					g_Scanner->InitPPParameters(m_category.c_str(), m_parameters, true);
					break;
				case 3:
				{
					std::string param(msgText + cmd.key_offset, cmd.key_len);
					m_parameters->SetParameter(param.c_str(), msgText + cmd.val_offset);
					break;
				}
				case 4:
					*m_priority = cmd.int_val;
					break;
				case 5:
					*m_addTop = cmd.int_val != 0;
					break;
				case 6:
					*m_addPaused = cmd.int_val != 0;
					break;
				case 7:
					m_dupeKey = msgText + cmd.val_offset;
					break;
				case 8:
					*m_dupeScore = cmd.int_val;
					break;
				case 9:
					*m_dupeMode = cmd.int_val == 1 ? dmAll : (cmd.int_val == 2 ? dmForce : dmScore);
					break;
				case -2:
					error("Invalid value \"%s\" for command \"DUPEMODE\" received from %s",
						msgText + cmd.val_offset, GetInfoName());
					break;
				default:
					error("Invalid command \"%s\" received from %s", msgText, GetInfoName());
					break;
			}
			return;
		}
		ScriptController::AddMessage(kind, text);
	}

	void SetPriority(int* priority) { m_priority = priority; }
	void SetDupeScore(int* dupeScore) { m_dupeScore = dupeScore; }
	void SetDupeMode(EDupeMode* dupeMode) { m_dupeMode = dupeMode; }
	void SetAddTop(bool* addTop) { m_addTop = addTop; }
	void SetAddPaused(bool* addPaused) { m_addPaused = addPaused; }
	void SetParameters(NzbParameterList* parameters) { m_parameters = parameters; }

private:
	const char* m_nzbFilename;
	const char* m_url;
	const char* m_directory;
	std::string& m_nzbName;
	std::string& m_category;
	std::string& m_dupeKey;
	int* m_priority;
	int* m_dupeScore;
	bool* m_addTop;
	bool* m_addPaused;
	EDupeMode* m_dupeMode;
	NzbParameterList* m_parameters;
	int m_prefixLen = 0;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		nzbget_rs_scan_script_prepare_env(
			m_nzbFilename,
			FileSystem::BaseFileName(m_nzbFilename),
			m_url,
			m_nzbName.c_str(),
			m_category.c_str(),
			*m_priority,
			*m_addTop ? 1 : 0,
			*m_addPaused ? 1 : 0,
			m_dupeKey.c_str(),
			*m_dupeScore,
			static_cast<int>(*m_dupeMode),
			m_directory,
			PATH_SEPARATOR,
			this,
			[](void* ctx, const char* name, const char* value)
			{
				static_cast<ScanScriptController*>(ctx)->SetEnvVar(name, value);
			},
			[](void* ctx, const char* name, int value)
			{
				static_cast<ScanScriptController*>(ctx)->SetIntEnvVar(name, value);
			});

		PrepareEnvScript(m_parameters, scriptName);
	}
};


inline Scanner::QueueData::QueueData(
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

inline void Scanner::QueueData::SetAddStatus(EAddStatus addStatus)
{
	if (m_addStatus)
	{
		*m_addStatus = addStatus;
	}
}

inline void Scanner::QueueData::SetNzbId(int nzbId)
{
	if (m_nzbId)
	{
		*m_nzbId = nzbId;
	}
}


inline void Scanner::InitOptions()
{
	m_nzbDirInterval = 1;
	m_scanScript = ScanScriptController::HasScripts();

	CleanupStaleUnpackDir();
}

inline int Scanner::ServiceInterval()
{
	return m_requestedNzbDirScan ? Service::Now :
		g_Options->GetNzbDirInterval() <= 0 ? Service::Sleep :
		m_nzbDirInterval;
}

inline void Scanner::ServiceWork()
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

inline void Scanner::CheckIncomingArchives(const fs::path& dir, bool checkStat)
{
	const auto archives = FindArchives(dir, checkStat);
	UnpackArchives(archives);
}

inline std::vector<fs::path> Scanner::FindArchives(const fs::path& dir, bool checkStat)
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
		const std::string pathStr = fs::u8string(path);
		if (nzbget_rs_unpack_can_extract_with_zipfast(pathStr.c_str()) != 0 &&
			CanProcessFile(pathStr.c_str(), checkStat))
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

inline std::optional<std::vector<fs::path>> Scanner::ProcessArchive(const fs::path& archiveFile, const fs::path& destDir)
{
	const auto unpackDir = (g_Options->GetTempDirPath() / UNPACK_DIR) / fs::make_unique_filename();
	std::vector<fs::path> to;
	const std::string archiveStr = fs::u8string(archiveFile);
	const std::string unpackStr = fs::u8string(unpackDir);
	const std::string destStr = fs::u8string(destDir);
	const std::string processedStr = fs::u8string(g_Options->GetNzbDirPath() / PROCESSED_DIR);
	const std::string brokenStr = fs::u8string(g_Options->GetNzbDirPath() / BROKEN_DIR);

	if (!nzbget_rs_archive_processor_process(
		archiveStr.c_str(),
		unpackStr.c_str(),
		destStr.c_str(),
		processedStr.c_str(),
		brokenStr.c_str(),
		(int)g_Options->GetNzbDirArchiveAction(),
		&to,
		[](void* /*ctx*/, int kind, const char* msg) {
			const char* text = msg ? msg : "";
			if (kind == 1)
			{
				warn("%s", text);
			}
			else if (kind == 2)
			{
				error("%s", text);
			}
			else if (kind == 3)
			{
				detail("%s", text);
			}
			else
			{
				info("%s", text);
			}
		},
		[](void* rawCtx, const char* movedPath) {
			auto* out = static_cast<std::vector<fs::path>*>(rawCtx);
			if (movedPath)
			{
				out->push_back(fs::u8path(movedPath));
			}
		}))
	{
		return std::nullopt;
	}

	return to;
}

inline void Scanner::UnpackArchives(const std::vector<fs::path>& archives)
{
	if (archives.empty()) return;

	for (const auto& archive : archives)
	{
		ProcessArchive(archive, archive.parent_path());
	}
}

inline void Scanner::CleanupStaleUnpackDir()
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
inline void Scanner::CheckIncomingNzbs(const char* directory, const char* category, bool checkStat)
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

inline void Scanner::ProcessSubdirectory(const char* fullPath, const char* filename, const char* category, bool checkStat)
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
inline bool Scanner::CanProcessFile(const char* fullFilename, bool checkStat)
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
inline void Scanner::DropOldFiles()
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

inline bool Scanner::ShouldSkipItem(std::string_view filename)
{
	std::string nameStr(filename);
	std::string procStr(PROCESSED_DIR);
	std::string brokStr(BROKEN_DIR);
	return nzbget_rs_scanner_should_skip_item(
		nameStr.c_str(), procStr.c_str(), brokStr.c_str()) != 0;
}

inline void Scanner::ProcessIncomingFile(
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

inline void Scanner::InitPPParameters(const char* category, NzbParameterList* parameters, bool reset)
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

inline CString Scanner::ResolveCategory(const char* category, const char* filename)
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

inline void Scanner::DetectAndSetCategory(const NzbFile& nzbFile, NzbInfo& nzbInfo, const char* nzbName)
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

inline bool Scanner::AddFileToQueue(
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

inline void Scanner::ScanNzbDir(bool syncMode)
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

inline Scanner::EAddStatus Scanner::AddArchive(const char* filename, const char* category,
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

		auto nzbFiles = ProcessArchive(archiveFilePath, g_Options->GetNzbDirPath());
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

inline Scanner::EAddStatus Scanner::AddArchive(
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

inline Scanner::EAddStatus Scanner::AddExternalFile(
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

#
inline void UrlDownloader::ProcessHeader(const char* line)
{
	WebDownloader::ProcessHeader(line);

	size_t start, length;
	NzbgetRsBuf name{}, value{};
	switch (nzbget_rs_url_dnzb_header(line, &start, &length, &name, &value))
	{
		case 1:
			m_category = length > 0 ? CString(line + start, (int)length) : CString("");
			debug("Category: %s", *m_category);
			break;

		case 2:
			debug("X-DNZB: %s", name.data);
			debug("Value: %s", value.data);
			m_nzbInfo->GetParameters()->SetParameter(name.data, value.data);
			nzbget_rs_free(name);
			nzbget_rs_free(value);
			break;
	}
}

inline UrlCoordinator::UrlCoordinator()
{
	m_downloadQueueObserver.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);
}

inline UrlCoordinator::~UrlCoordinator()
{
	debug("Destroying UrlCoordinator");

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		delete urlDownloader;
	}
	m_activeDownloads.clear();

	debug("UrlCoordinator destroyed");
}

inline void UrlCoordinator::Run()
{
	debug("Entering UrlCoordinator-loop");

	while (!DownloadQueue::IsLoaded())
	{
		Util::Sleep(20);
	}

	time_t lastReset = 0;
	while (!IsStopped())
	{
		bool downloadStarted = false;

		{
			NzbInfo* nzbInfo = nullptr;
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			if ((int)m_activeDownloads.size() < g_Options->GetUrlConnections())
			{
				nzbInfo = GetNextUrl(downloadQueue);
				if (nzbInfo && (!g_WorkState->GetPauseDownload() || g_Options->GetUrlForce()))
				{
					StartUrlDownload(nzbInfo);
					downloadStarted = true;
				}
			}
			m_hasMoreJobs = !m_activeDownloads.empty() || nzbInfo != nullptr;
		}

		if (lastReset != Util::CurrentTime())
		{
			ResetHangingDownloads();
			lastReset = Util::CurrentTime();
		}

		if (!m_hasMoreJobs && !IsStopped())
		{
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait(lk, [&] { return m_hasMoreJobs || IsStopped(); });
		}
		else
		{
			Util::Sleep(downloadStarted ? 0 : 100);
		}
	}

	WaitJobs();

	debug("Exiting UrlCoordinator-loop");
}

inline void UrlCoordinator::WaitJobs()
{
	debug("UrlCoordinator: waiting for Downloads to complete");

	while (true)
	{
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			if (m_activeDownloads.empty())
			{
				break;
			}
		}
		Util::Sleep(100);
		ResetHangingDownloads();
	}

	debug("UrlCoordinator: Downloads are completed");
}

inline void UrlCoordinator::Stop()
{
	Thread::Stop();

	debug("Stopping UrlDownloads");
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		for (UrlDownloader* urlDownloader : m_activeDownloads)
		{
			urlDownloader->Stop();
		}
	}
	debug("UrlDownloads are notified");

	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

inline void UrlCoordinator::DownloadQueueUpdate(Subject*, void* aspect)
{
	debug("Notification from download queue received");

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaUrlAdded ||
		queueAspect->action == DownloadQueue::eaUrlReturned)
	{
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_hasMoreJobs = true;
		m_waitCond.notify_all();
	}
}

inline void UrlCoordinator::ResetHangingDownloads()
{
	if (g_Options->GetUrlTimeout() == 0)
	{
		return;
	}

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	time_t tm = Util::CurrentTime();

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		if (nzbget_rs_url_coord_is_hanging_download(
			static_cast<int64>(tm),
			static_cast<int64>(urlDownloader->GetLastUpdateTime()),
			g_Options->GetUrlTimeout(),
			static_cast<int>(urlDownloader->GetStatus()),
			static_cast<int>(UrlDownloader::adRunning)))
		{
			error("Cancelling hanging url download %s", urlDownloader->GetInfoName());
			urlDownloader->Stop();
		}
	}
}

inline void UrlCoordinator::LogDebugInfo()
{
	info("   ---------- UrlCoordinator");

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	info("    Active Downloads: %i", (int)m_activeDownloads.size());
	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		urlDownloader->LogDebugInfo();
	}
}

inline NzbInfo* UrlCoordinator::GetNextUrl(DownloadQueue* downloadQueue)
{
	size_t n = downloadQueue->GetQueue()->size();
	if (n == 0)
	{
		return nullptr;
	}
	std::vector<NzbInfo*> items;
	std::vector<int> kinds;
	std::vector<int> urlStatuses;
	std::vector<int> deleteStatuses;
	std::vector<int> priorities;
	items.reserve(n);
	kinds.reserve(n);
	urlStatuses.reserve(n);
	deleteStatuses.reserve(n);
	priorities.reserve(n);
	for (NzbInfo* nzbInfo1 : downloadQueue->GetQueue())
	{
		items.push_back(nzbInfo1);
		kinds.push_back(static_cast<int>(nzbInfo1->GetKind()));
		urlStatuses.push_back(static_cast<int>(nzbInfo1->GetUrlStatus()));
		deleteStatuses.push_back(static_cast<int>(nzbInfo1->GetDeleteStatus()));
		priorities.push_back(nzbInfo1->GetPriority());
	}
	int idx = nzbget_rs_url_coord_select_next_url(
		kinds.data(),
		urlStatuses.data(),
		deleteStatuses.data(),
		priorities.data(),
		n,
		static_cast<int>(NzbInfo::nkUrl),
		static_cast<int>(NzbInfo::lsNone),
		static_cast<int>(NzbInfo::dsNone));
	return idx >= 0 && static_cast<size_t>(idx) < items.size() ? items[idx] : nullptr;
}

inline void UrlCoordinator::StartUrlDownload(NzbInfo* nzbInfo)
{
	debug("Starting new UrlDownloader");

	UrlDownloader* urlDownloader = new UrlDownloader();
	urlDownloader->SetAutoDestroy(true);
	urlDownloader->Attach(this);
	urlDownloader->SetNzbInfo(nzbInfo);
	urlDownloader->SetUrl(nzbInfo->GetUrl());
	urlDownloader->SetForce(g_Options->GetUrlForce());
	urlDownloader->SetInfoName(nzbInfo->MakeNiceUrlName(nzbInfo->GetUrl(), nzbInfo->GetFilename()));
	NzbgetRsBuf outPathBuf = nzbget_rs_url_coord_format_temp_path(
		g_Options->GetTempDir(), PATH_SEPARATOR, nzbInfo->GetId());
	urlDownloader->SetOutputFilename(outPathBuf.data ? outPathBuf.data : "");
	nzbget_rs_free(outPathBuf);

	nzbInfo->SetActiveDownloads(1);
	nzbInfo->SetUrlStatus(NzbInfo::lsRunning);

	m_activeDownloads.push_back(urlDownloader);
	urlDownloader->Start();
}

inline void UrlCoordinator::Update(Subject* caller, void*)
{
	debug("Notification from UrlDownloader received");

	UrlDownloader* urlDownloader = (UrlDownloader*)caller;
	if (urlDownloader->GetStatus() == WebDownloader::adFinished ||
		urlDownloader->GetStatus() == WebDownloader::adFailed ||
		urlDownloader->GetStatus() == WebDownloader::adRetry)
	{
		UrlCompleted(urlDownloader);
	}
}

inline void UrlCoordinator::UrlCompleted(UrlDownloader* urlDownloader)
{
	debug("URL downloaded");

	NzbInfo* nzbInfo = urlDownloader->GetNzbInfo();

	const char* origname;
	if (urlDownloader->GetOriginalFilename())
	{
		origname = urlDownloader->GetOriginalFilename();
	}
	else
	{
		origname = FileSystem::BaseFileName(nzbInfo->GetUrl());
	}

	CString filename = FileSystem::MakeValidFilename(origname);

	debug("Filename: [%s]", *filename);

	bool retry;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		m_activeDownloads.erase(std::find(m_activeDownloads.begin(), m_activeDownloads.end(), urlDownloader));

		int urlStatus = nzbInfo->GetUrlStatus();
		int deleteStatus = nzbInfo->GetDeleteStatus();
		int again = 0;
		nzbget_rs_url_completed(urlDownloader->GetStatus(), nzbInfo->GetDeleting(), &urlStatus, &deleteStatus, &again);
		retry = again != 0;
		if (nzbInfo->GetDeleting())
		{
			nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)deleteStatus);
			nzbInfo->SetDeleting(false);
		}
		nzbInfo->SetUrlStatus((NzbInfo::EUrlStatus)urlStatus);

		if (!retry)
		{
			DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlCompleted, downloadQueue, nzbInfo, nullptr};
			downloadQueue->Notify(&aspect);
		}
	}

	if (retry)
	{
		nzbInfo->SetActiveDownloads(0);
		return;
	}

	if (nzbInfo->GetUrlStatus() == NzbInfo::lsFinished)
	{
		const char* name = !Util::EmptyStr(nzbInfo->GetFilename())
			? nzbInfo->GetFilename() : *filename;
		const char* category = !Util::EmptyStr(nzbInfo->GetCategory())
			? nzbInfo->GetCategory() : urlDownloader->GetCategory();

		Scanner::EAddStatus addStatus;
		if (nzbget_rs_unpack_can_extract_with_zipfast(name) != 0)
		{
			addStatus = g_Scanner->AddArchive(
				name,
				category,
				nzbInfo->GetAutoCategory(),
				nzbInfo->GetPriority(),
				nzbInfo->GetDupeKey(),
				nzbInfo->GetDupeScore(),
				nzbInfo->GetDupeMode(),
				nzbInfo->GetParameters(),
				false,
				nzbInfo->GetAddUrlPaused(),
				nzbInfo,
				urlDownloader->GetOutputFilename()
			);
		}
		else 
		{
			addStatus = g_Scanner->AddExternalFile(
				name,
				category,
				nzbInfo->GetAutoCategory(),
				nzbInfo->GetPriority(),
				nzbInfo->GetDupeKey(),
				nzbInfo->GetDupeScore(),
				nzbInfo->GetDupeMode(),
				nzbInfo->GetParameters(),
				false,
				nzbInfo->GetAddUrlPaused(),
				nzbInfo,
				urlDownloader->GetOutputFilename(),
				nullptr,
				0,
				nullptr
			);
		}

		if (addStatus == Scanner::asSuccess)
		{
			return;
		}

		nzbInfo->SetUrlStatus(addStatus == Scanner::asFailed
			? NzbInfo::lsScanFailed : NzbInfo::lsScanSkipped);
	}

	g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeUrlCompleted);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		nzbInfo->SetActiveDownloads(0);

		DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlFailed, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);
	}
}

inline bool UrlCoordinator::DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory)
{
	if (nzbInfo->GetActiveDownloads() > 0)
	{
		info("Deleting active URL %s", nzbInfo->GetName());
		nzbInfo->SetDeleting(true);
		nzbInfo->SetAvoidHistory(avoidHistory);

		for (UrlDownloader* urlDownloader : m_activeDownloads)
		{
			if (urlDownloader->GetNzbInfo() == nzbInfo)
			{
				urlDownloader->Stop();
				return true;
			}
		}

		return false;
	}

	info("Deleting URL %s", nzbInfo->GetName());

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
	}
	nzbInfo->SetUrlStatus(NzbInfo::lsNone);

	DownloadQueue::Aspect deletedAspect = {DownloadQueue::eaUrlDeleted, downloadQueue, nzbInfo, nullptr};
	downloadQueue->Notify(&deletedAspect);

	return true;
}

inline void UrlCoordinator::AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst)
{
	debug("Adding URL to queue");

	NzbInfo* addedNzb = nzbInfo.get();

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	DownloadQueue::Aspect foundAspect = {DownloadQueue::eaUrlFound, downloadQueue, addedNzb, nullptr};
	downloadQueue->Notify(&foundAspect);

	if (addedNzb->GetDeleteStatus() != NzbInfo::dsManual)
	{
		downloadQueue->GetQueue()->Add(std::move(nzbInfo), addFirst);

		DownloadQueue::Aspect addedAspect = {DownloadQueue::eaUrlAdded, downloadQueue, addedNzb, nullptr};
		downloadQueue->Notify(&addedAspect);
	}

	downloadQueue->Save();
}


#endif
