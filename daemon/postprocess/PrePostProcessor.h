/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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


#ifndef PREPOSTPROCESSOR_H
#define PREPOSTPROCESSOR_H
#include "DiskState.h"

class PrePostProcessor : public Thread, public Observer
{
public:
	PrePostProcessor();
	void Run() override;
	void Stop() override;
	bool HasMoreJobs() { return m_queuedJobs > 0; }
	int GetJobCount() { return m_queuedJobs; }
	bool HasActiveUnpack(bool includeWaitingVolume = true);
	bool HasActiveUnpack(DownloadQueue* downloadQueue, bool includeWaitingVolume = true);
	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action,
		const char* args);
	void NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	// A late rename or a draining paused PAR request can make the collection
	// ready without completing another file. Requires the download-queue lock.
	void CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);

protected:
	void Update(Subject*, void* aspect) override { DownloadQueueUpdate(aspect); }

private:
	int m_queuedJobs = 0;
	RawNzbList m_activeJobs;
	std::recursive_mutex m_activeJobsMutex;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;

	void CheckPostQueue();
	void CheckRequestPar(DownloadQueue* downloadQueue);
	void CleanupJobs(DownloadQueue* downloadQueue);
	bool CanRunMoreJobs(bool* allowPar);
	NzbInfo* PickNextJob(DownloadQueue* downloadQueue, bool allowPar);
	void StartJob(DownloadQueue* downloadQueue, PostInfo* postInfo, bool allowPar);
	void EnterStage(DownloadQueue* downloadQueue, PostInfo* postInfo, PostInfo::EStage stage);
	void SanitisePostQueue();
	void UpdatePauseState();
	void NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool saveQueue);
	void JobCompleted(DownloadQueue* downloadQueue, PostInfo* postInfo);
	bool PostQueueDelete(DownloadQueue* downloadQueue, IdList* idList);
	void DownloadQueueUpdate(void* aspect);
	void DeleteCleanup(NzbInfo* nzbInfo);
	void WaitJobs();
	void FileDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileInfo* fileInfo);
};

extern PrePostProcessor* g_PrePostProcessor;

#include <atomic>
#include <condition_variable>
#include <deque>
#include <map>
#include <mutex>
#include <set>
#include <thread>
#include <vector>
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
#include "Util.h"
#include "Connection.h"
#include "Util.h"
#include "Util.h"
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
		void SetName(std::string name) { m_name = std::move(name); }
		const char* GetName() const { return m_name.c_str(); }
		void SetDisplayName(std::string displayName) { m_displayName = std::move(displayName); }
		const char* GetDisplayName() const { return m_displayName.c_str(); }
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
		void SetJsonStr(std::string json) { m_json = std::move(json); }
		const std::string& GetJsonStr() const { return m_json; }
		void SetXmlStr(std::string xml) { m_xml = std::move(xml); }
		const std::string& GetXmlStr() const { return m_xml; }

	private:
		Kind m_kind;
		std::string m_entry;
		std::string m_location;
		std::string m_rootDir;
		std::string m_name;
		std::string m_displayName;
		std::string m_queueEvents;
		std::string m_taskTime;
		std::string m_json;
		std::string m_xml;
	};

	inline std::string ToJsonStr(const Script& script)
	{
		return script.GetJsonStr();
	}

	inline std::string ToXmlStr(const Script& script)
	{
		return script.GetXmlStr();
	}
}

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

			NzbgetRsBuf errBuf = nzbget_rs_ext_mgr_load_extensions(
				scriptDir,
				g_Options->GetScriptOrder(),
				PATH_SEPARATOR,
				this,
				[](void*, const char* msg)
				{
					error("%s", msg ? msg : "");
				},
				[](
					void* ctx,
					const char* entryPtr, size_t entryLen,
					const char* locPtr, size_t locLen,
					const char* rootPtr, size_t rootLen,
					const char* namePtr, size_t nameLen,
					const char* dispPtr, size_t dispLen,
					const char* qeventsPtr, size_t qeventsLen,
					const char* taskTimePtr, size_t taskTimeLen,
					int postScript, int scanScript, int queueScript, int schedScript, int feedScript,
					const char* jsonPtr, size_t jsonLen,
					const char* xmlPtr, size_t xmlLen)
				{
					auto* mgr = static_cast<Manager*>(ctx);
					Extension::Script ext;
					ext.SetEntry(std::string(entryPtr ? entryPtr : "", entryLen));
					ext.SetLocation(std::string(locPtr ? locPtr : "", locLen));
					ext.SetRootDir(std::string(rootPtr ? rootPtr : "", rootLen));
					ext.SetName(std::string(namePtr ? namePtr : "", nameLen));
					ext.SetDisplayName(std::string(dispPtr ? dispPtr : "", dispLen));
					ext.SetKind({
						postScript != 0,
						scanScript != 0,
						queueScript != 0,
						schedScript != 0,
						feedScript != 0
					});
					ext.SetQueueEvents(std::string(qeventsPtr ? qeventsPtr : "", qeventsLen));
					ext.SetTaskTime(std::string(taskTimePtr ? taskTimePtr : "", taskTimeLen));
					ext.SetJsonStr(std::string(jsonPtr ? jsonPtr : "", jsonLen));
					ext.SetXmlStr(std::string(xmlPtr ? xmlPtr : "", xmlLen));
					mgr->m_extensions.emplace_back(std::make_shared<Extension::Script>(std::move(ext)));
				});
			if (errBuf.data)
			{
				std::string err(errBuf.data, errBuf.len);
				nzbget_rs_free(errBuf);
				return err;
			}
			nzbget_rs_free(errBuf);

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

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef SCRIPTCONTROLLER_H
#define SCRIPTCONTROLLER_H

#include <atomic>
#include "Util.h"

class EnvironmentStrings
{
public:
	void Clear();
	void InitFromCurrentProcess();
	void Append(const char* envstr);
	void Append(CString&& envstr);
	std::vector<char*> GetStrings();

private:
	typedef std::vector<CString> Strings;

	Strings m_strings;
};

class ScriptController
{
public:
	typedef std::vector<CString> ArgList;

	ScriptController();
	virtual ~ScriptController();
	int Execute();
	void Terminate();
	bool Break();
	void Resume();
	void Detach();
	static void TerminateAll();

	const char* GetScript() { return !m_args.empty() ? *m_args[0] : nullptr; }
	void SetWorkingDir(const char* workingDir) { m_workingDir = workingDir; }
	void SetArgs(ArgList&& args) { m_args = std::move(args); }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
	const char* GetInfoName() { return m_infoName; }
	void SetLogPrefix(const char* logPrefix) { m_logPrefix = logPrefix; }
	void SetEnvVar(const char* name, const char* value);
	void SetEnvVarSpecial(const char* prefix, const char* name, const char* value);
	void SetIntEnvVar(const char* name, int value);
	bool IsTerminated() const { return m_terminated; }
	bool HasActiveProcess();

protected:
	void ProcessOutput(char* text);
	virtual bool ReadLine(char* buf, int bufSize, FILE* stream);
	void PrintMessage(Message::EKind kind, const char* format, ...) PRINTF_SYNTAX(3);
	virtual void AddMessage(Message::EKind kind, const char* text);
	bool GetTerminated() { return m_terminated; }
	void ResetEnv();
	void PrepareEnvOptions(const char* stripPrefix);
	void PrepareArgs();
	virtual const char* GetOptValue([[maybe_unused]] const char* name, const char* value) { return value; }
	void StartProcess(int* pipein, int* pipeout);
	int WaitProcess();
	void SetNeedWrite(bool needWrite) { m_needWrite = needWrite; }
	void Write(const char* str);
	void UnregisterRunningScript();

private:
	void CheckEnvSize(const std::vector<char*>& envs);

	ArgList m_args;
	ArgList m_cmdArgs;
	const char* m_workingDir = nullptr;
	CString m_infoName;
	const char* m_logPrefix = nullptr;
	EnvironmentStrings m_environmentStrings;
	std::atomic<bool> m_terminated{false};
	std::atomic<bool> m_completed{false};
	std::atomic<bool> m_detached{false};
	std::atomic<bool> m_needWrite{false};
	// taken with exchange(): Detach() from another thread and Execute() each
	// close it, and only the one that gets it may
	std::atomic<FILE*> m_readpipe{nullptr};
	FILE* m_writepipe = 0;
	char m_cmdLine[2048];
	std::atomic<pid_t> m_processId{0};

	inline static const long ARGS_LIMIT_SIZE = []() -> long
	{
		return sysconf(_SC_ARG_MAX);
	}();

	typedef std::vector<ScriptController*> RunningScripts;

	inline static RunningScripts m_runningScripts{};
	inline static Mutex m_runningMutex{};
};

#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

// System global variable holding environments variables
extern char** environ;
extern char* (*g_EnvironmentVariables)[];

inline constexpr int FORK_ERROR_EXIT_CODE = 254;

#ifdef CHILD_WATCHDOG
class ChildWatchDog : public Thread
{
public:
	void SetProcessId(pid_t processId) { m_processId = processId; }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
protected:
	virtual void Run();
private:
	pid_t m_processId;
	CString m_infoName;
};

inline void ChildWatchDog::Run()
{
	static const int WAIT_SECONDS = 60;
	time_t start = Util::CurrentTime();
	while (!IsStopped() && (Util::CurrentTime() - start) < WAIT_SECONDS)
	{
		Util::Sleep(10);
	}

	if (!IsStopped())
	{
		info("Restarting hanging child process for %s", *m_infoName);
		kill(m_processId, SIGKILL);
	}
}
#endif

inline void EnvironmentStrings::Clear()
{
	m_strings.clear();
}

inline void EnvironmentStrings::InitFromCurrentProcess()
{
	if (!g_EnvironmentVariables)
		return;

	for (int i = 0; (*g_EnvironmentVariables)[i]; i++)
	{
		char* var = (*g_EnvironmentVariables)[i];
		// Ignore all env vars set by NZBGet.
		// This is to avoid the passing of env vars after program update (when NZBGet is
		// started from a script which was started by a previous instance of NZBGet).
		// Format: NZBXX_YYYY (XX are any two characters, YYYY are any number of any characters).
		if (!nzbget_rs_sus_is_nzbget_internal_env_var(var))
		{
			Append(var);
		}
	}
}

inline void EnvironmentStrings::Append(const char* envstr)
{
	m_strings.emplace_back(envstr);
}

inline void EnvironmentStrings::Append(CString&& envstr)
{
	m_strings.push_back(std::move(envstr));
}

/*
 * Returns environment block in format suitable for using with execve.
 */
inline std::vector<char*> EnvironmentStrings::GetStrings()
{
	std::vector<char*> strings;
	strings.reserve(m_strings.size() + 1);
	std::copy(m_strings.begin(), m_strings.end(), std::back_inserter(strings));
	strings.push_back(nullptr);
	return strings;
}

inline ScriptController::ScriptController()
{
	ResetEnv();

	Guard guard(m_runningMutex);
	m_runningScripts.push_back(this);
}

inline ScriptController::~ScriptController()
{
	UnregisterRunningScript();
}

inline void ScriptController::UnregisterRunningScript()
{
	Guard guard(m_runningMutex);
	m_runningScripts.erase(std::remove(m_runningScripts.begin(), m_runningScripts.end(), this), m_runningScripts.end());
}

inline void ScriptController::ResetEnv()
{
	m_environmentStrings.Clear();
	m_environmentStrings.InitFromCurrentProcess();
}

inline void ScriptController::SetEnvVar(const char* name, const char* value)
{
	NzbgetRsBuf envBuf = nzbget_rs_dqh_script_ctrl_format_env_assignment(name, value);
	m_environmentStrings.Append(CString(envBuf.data ? envBuf.data : "", (int)envBuf.len));
	nzbget_rs_free(envBuf);
}

inline void ScriptController::SetIntEnvVar(const char* name, int value)
{
	NzbgetRsBuf valBuf = nzbget_rs_dqh_conn_format_port_str(value);
	SetEnvVar(name, valBuf.data ? valBuf.data : "0");
	nzbget_rs_free(valBuf);
}

/**
 * If szStripPrefix is not nullptr, only options, whose names start with the prefix
 * are processed. The prefix is then stripped from the names.
 * If szStripPrefix is nullptr, all options are processed; without stripping.
 */
inline void ScriptController::PrepareEnvOptions(const char* stripPrefix)
{
	int prefixLen = stripPrefix ? strlen(stripPrefix) : 0;

	for (Options::OptEntry& optEntry : g_Options->GuardOptEntries())
	{
		const char* value = GetOptValue(optEntry.GetName(), optEntry.GetValue());
		if (nzbget_rs_ufs_script_ctrl_matches_env_prefix(optEntry.GetName(), stripPrefix, prefixLen) != 0)
		{
			SetEnvVarSpecial("NZBPO", optEntry.GetName() + prefixLen, value);
		}
		else if (!stripPrefix)
		{
			SetEnvVarSpecial("NZBOP", optEntry.GetName(), value);
		}
	}
}

inline void ScriptController::SetEnvVarSpecial(const char* prefix, const char* name, const char* value)
{
	NzbgetRsBuf varBuf = nzbget_rs_dqh_script_ctrl_format_prefixed_varname(prefix, name);
	BString<1024> varname;
	varname.Set(varBuf.data ? varBuf.data : "", (int)varBuf.len);
	nzbget_rs_free(varBuf);

	// Original name
	SetEnvVar(varname, value);

	NzbgetRsBuf normBuf = nzbget_rs_sus_normalize_special_env_var_name(varname);
	if (normBuf.data && nzbget_rs_dqh_bytes_equal(varname, normBuf.data) == 0)
	{
		SetEnvVar(normBuf.data, value);
	}
	nzbget_rs_free(normBuf);
}

inline void ScriptController::PrepareArgs()
{
	*m_cmdLine = '\0';
	m_cmdArgs.clear();

	if (m_args.size() == 1)
	{
		const auto found = Util::FindExecutorProgram(m_args[0].Str(), g_Options->GetShellOverride());
		int useFound = 0, prependArg = 0;
		nzbget_rs_tpr_script_resolve_single_arg_executor(
			found.has_value() ? 1 : 0,
			found ? found.value().c_str() : nullptr,
			m_args[0].Str(),
			&useFound, &prependArg);
		strncpy(m_cmdLine, useFound ? found.value().c_str() : (const char*)m_args[0], sizeof(m_cmdLine) - 1);
		// an interpreter goes before the script; a script run directly is
		// argv[0] already (m_args), not its own first argument too
		if (prependArg)
		{
			m_cmdArgs.emplace_back(found.value().c_str());
		}

		debug("CmdLine: %s", m_cmdLine);
	}
	else
	{
		strncpy(m_cmdLine, m_args[0], sizeof(m_cmdLine) - 1);
	}
}

inline int ScriptController::Execute()
{
	PrepareEnvOptions(nullptr);
	PrepareArgs();

	m_completed = false;
	m_processId = 0;
	if (m_terminated)
	{
		m_completed = true;
		return -1;
	}
	int exitCode = 0;

#ifdef CHILD_WATCHDOG
	bool childConfirmed = false;
	while (!childConfirmed && !m_terminated)
	{
#endif

	int pipein = -1, pipeout = -1;
	StartProcess(&pipein, &pipeout);
	if (pipein == -1)
	{
		m_completed = true;
		return -1;
	}

	// open the read end
	m_readpipe = fdopen(pipein, "r");
	if (!m_readpipe)
	{
		PrintMessage(Message::mkError, "Could not open read pipe to %s", *m_infoName);
		close(pipein);
		if (pipeout != -1)
		{
			close(pipeout);
		}
		m_completed = true;
		return -1;
	}

	m_writepipe = 0;
	if (m_needWrite)
	{
		// open the write end
		m_writepipe = fdopen(pipeout, "w");
		if (!m_writepipe)
		{
			PrintMessage(Message::mkError, "Could not open write pipe to %s", *m_infoName);
			fclose(m_readpipe.exchange(nullptr));
			close(pipeout);
			m_completed = true;
			return -1;
		}
	}

#ifdef CHILD_WATCHDOG
	debug("Creating child watchdog");
	ChildWatchDog watchDog;
	watchDog.SetAutoDestroy(false);
	watchDog.SetProcessId(m_processId);
	watchDog.SetInfoName(m_infoName);
	watchDog.Start();
#endif

	CharBuffer buf(1024 * 10);

	debug("Entering pipe-loop");
	bool firstLine = true;
	bool startError = false;
	while (!m_terminated && !m_detached)
	{
		FILE* readpipe = m_readpipe;
		if (!readpipe || feof(readpipe))
		{
			break;
		}
		if (ReadLine(buf, buf.Size(), readpipe) && m_readpipe)
		{
#ifdef CHILD_WATCHDOG
			if (!childConfirmed)
			{
				childConfirmed = true;
				watchDog.Stop();
				debug("Child confirmed");
				continue;
			}
#endif
			if (nzbget_rs_ufs_script_ctrl_is_fork_start_error_line(firstLine ? 1 : 0, buf) != 0)
			{
				startError = true;
			}
			ProcessOutput(buf);
			firstLine = false;
		}
	}
	debug("Exited pipe-loop");

#ifdef CHILD_WATCHDOG
	debug("Destroying WatchDog");
	if (!childConfirmed)
	{
		watchDog.Stop();
	}
	while (watchDog.IsRunning())
	{
		Util::Sleep(5);
	}
#endif

	if (FILE* readpipe = m_readpipe.exchange(nullptr))
	{
		fclose(readpipe);
	}

	if (m_writepipe)
	{
		fclose(m_writepipe);
	}

	if (m_terminated && m_infoName)
	{
		warn("Interrupted %s", *m_infoName);
	}

	exitCode = 0;

	if (!m_detached)
	{
		exitCode = WaitProcess();
		exitCode = nzbget_rs_ufs_script_ctrl_normalize_fork_exit_code(
			exitCode, FORK_ERROR_EXIT_CODE, startError ? 1 : 0);
	}

#ifdef CHILD_WATCHDOG
	}	// while (!bChildConfirmed && !m_bTerminated)
#endif

	debug("Exit code %i", exitCode);
	m_completed = true;
	m_processId = 0;
	return exitCode;
}

/*
* Returns file descriptor of the read-pipe or -1 on error.
*/
inline void ScriptController::StartProcess(int* pipein, int* pipeout)
{
	CString workingDir = m_workingDir;
	if (workingDir.Empty())
	{
		workingDir = FileSystem::GetCurrentDirectory();
	}

	const char* script = m_args[0];

	int pin[] = {-1, -1};
	int pout[] = {-1, -1};

	// create the pipes, close-on-exec: a script another thread starts at the
	// same time would otherwise inherit our write end and keep it open for as
	// long as it runs - we'd see no end of output until that script ended too.
	// dup2() onto the child's stdin/stdout/stderr clears the flag there.
	auto makePipe = [](int fds[2])
	{
#ifdef __linux__
		return pipe2(fds, O_CLOEXEC);
#else
		if (pipe(fds))
		{
			return -1;
		}
		fcntl(fds[0], F_SETFD, FD_CLOEXEC);
		fcntl(fds[1], F_SETFD, FD_CLOEXEC);
		return 0;
#endif
	};
	if (makePipe(pin))
	{
		PrintMessage(Message::mkError, "Could not open read pipe: errno %i", errno);
		return;
	}
	if (m_needWrite && makePipe(pout))
	{
		PrintMessage(Message::mkError, "Could not open write pipe: errno %i", errno);
		close(pin[0]);
		close(pin[1]);
		return;
	}

	std::vector<char*> environmentStrings = m_environmentStrings.GetStrings();
	char** envdata = environmentStrings.data();

	CheckEnvSize(environmentStrings);

	std::copy(std::begin(m_args), std::end(m_args), std::back_inserter(m_cmdArgs));
	m_cmdArgs.emplace_back(nullptr);
	char* const* argdata = (char* const*)m_cmdArgs.data();

#ifdef DEBUG
	debug("Starting  process: %s", script);
	for (const char* arg : m_args)
	{
		debug("arg: %s", arg);
	}
#endif

	debug("forking");
	pid_t pid = fork();

	if (pid == -1)
	{
		PrintMessage(Message::mkError, "Could not start %s: errno %i", *m_infoName, errno);
		close(pin[0]);
		close(pin[1]);
		if (m_needWrite)
		{
			close(pout[0]);
			close(pout[1]);
		}
		return;
	}
	else if (pid == 0)
	{
		// here goes the second instance

		// only certain functions may be used here or the program may hang.
		// for a list of functions see chapter "async-signal-safe functions" in
		// http://man7.org/linux/man-pages/man7/signal.7.html

		// create new process group (see Terminate() where it is used)
		setsid();

		// make the pipeout to be the same as stdout and stderr
		dup2(pin[1], 1);
		dup2(pin[1], 2);
		close(pin[0]);
		close(pin[1]);

		if (m_needWrite)
		{
			// make the pipein to be the same as stdin
			dup2(pout[0], 0);
			close(pout[0]);
			close(pout[1]);
		}

#ifdef CHILD_WATCHDOG
		write(1, "\n", 1);
		fsync(1);
#endif

		chdir(workingDir);
		environ = envdata;

		execvp(m_cmdLine, argdata);

		if (errno == EACCES)
		{
			write(1, "[WARNING] Fixing permissions for", 32);
			write(1, script, strlen(script));
			write(1, "\n", 1);
			fsync(1);
			FileSystem::FixExecPermission(script);
			execvp(m_cmdLine, argdata);
		}

		// NOTE: the text "[ERROR] Could not start " is checked later,
		// by changing adjust the dependent code below.
		write(1, "[ERROR] Could not start ", 24);
		write(1, script, strlen(script));
		write(1, ": ", 2);
		char* errtext = strerror(errno);
		write(1, errtext, strlen(errtext));
		write(1, "\n", 1);
		fsync(1);
		_exit(FORK_ERROR_EXIT_CODE);
	}

	// continue the first instance
	debug("forked");
	debug("Child Process-ID: %i", (int)pid);

	m_processId = pid;

	// close unused pipe ends; the caller gets the others only now: on a
	// failed fork they were closed already
	close(pin[1]);
	if (m_needWrite)
	{
		close(pout[0]);
	}
	*pipein = pin[0];
	*pipeout = pout[1];
}

inline int ScriptController::WaitProcess()
{
	int status = 0;
	pid_t res;
	do
	{
		res = waitpid(m_processId, &status, 0);
	} while (res == -1 && errno == EINTR);
	if (res == -1)
	{
		return -1;
	}
	if (WIFEXITED(status))
	{
		int exitCode = WEXITSTATUS(status);
		return exitCode;
	}
	if (WIFSIGNALED(status))
	{
		// not a success: a crashed or killed program used to read as exit
		// code 0. The shell's convention, 128 + signal.
		if (!m_terminated)
		{
			PrintMessage(Message::mkWarning, "%s was killed by signal %i", *m_infoName, WTERMSIG(status));
		}
		return nzbget_rs_ufs_script_ctrl_calc_signal_exit_code(WTERMSIG(status));
	}
	return -1;
}

inline bool ScriptController::HasActiveProcess()
{
	return m_processId.load() > 0 && !m_completed;
}

inline void ScriptController::Terminate()
{
	debug("Stopping %s", *m_infoName);
	m_terminated = true;

	pid_t childPid = m_processId.load();
	pid_t killId = (pid_t)nzbget_rs_ufs_script_ctrl_select_kill_pid(
		(int)childPid, childPid > 0 ? (int)getpgid(childPid) : 0);
	int killRes = childPid > 0 ? kill(killId, SIGKILL) : 0;
	bool ok = nzbget_rs_dqh_script_ctrl_is_terminate_ok(
		(int)childPid, killRes, m_completed ? 1 : 0) != 0;

	if (ok)
	{
		debug("Terminated %s", *m_infoName);
	}
	else
	{
		error("Could not terminate %s", *m_infoName);
	}

	debug("Stopped %s", *m_infoName);
}

inline void ScriptController::TerminateAll()
{
	Guard guard(m_runningMutex);
	for (ScriptController* script : m_runningScripts)
	{
		if (script->HasActiveProcess() && !script->m_detached)
		{
			// send break signal and wait up to 5 seconds for graceful termination
			if (script->Break())
			{
				time_t curtime = Util::CurrentTime();
				while (!script->m_completed && std::abs(curtime - Util::CurrentTime()) <= 10)
				{
					Util::Sleep(100);
				}
			}
			script->Terminate();
		}
	}
}

inline bool ScriptController::Break()
{
	debug("Sending break signal to %s", *m_infoName);

	pid_t pid = m_processId.load();
	bool ok = nzbget_rs_dqh_script_ctrl_can_send_break((int)pid, m_completed ? 1 : 0) != 0 &&
		kill(pid, SIGINT) == 0;

	if (ok)
	{
		debug("Sent break signal to %s", *m_infoName);
	}
	else
	{
		warn("Could not send break signal to %s", *m_infoName);
	}

	return ok;
}

inline void ScriptController::Detach()
{
	debug("Detaching %s", *m_infoName);
	m_detached = true;
	if (FILE* readpipe = m_readpipe.exchange(nullptr))
	{
		fclose(readpipe);
	}
}

inline void ScriptController::Resume()
{
	m_terminated = false;
	m_detached = false;
	m_processId = 0;
}

inline bool ScriptController::ReadLine(char* buf, int bufSize, FILE* stream)
{
	return fgets(buf, bufSize, stream);
}

inline void ScriptController::ProcessOutput(char* text)
{
	debug("Processing output received from script");

	int msgKind = 0;
	size_t msgOffset = 0;
	if (!nzbget_rs_sus_classify_script_output_line(text, &msgKind, &msgOffset))
	{
		return;
	}
	Message::EKind kind = Message::mkInfo;
	switch (msgKind)
	{
		case 1: kind = Message::mkWarning; break;
		case 2: kind = Message::mkError; break;
		case 3: kind = Message::mkDetail; break;
		case 4: kind = Message::mkDebug; break;
		default: kind = Message::mkInfo; break;
	}
	PrintMessage(kind, "%s", text + msgOffset);

	debug("Processing output received from script - completed");
}

inline void ScriptController::AddMessage(Message::EKind kind, const char* text)
{
	switch (kind)
	{
		case Message::mkDetail:
			detail("%s", text);
			break;

		case Message::mkInfo:
			info("%s", text);
			break;

		case Message::mkWarning:
			warn("%s", text);
			break;

		case Message::mkError:
			error("%s", text);
			break;

		case Message::mkDebug:
			debug("%s", text);
			break;
	}
}

inline void ScriptController::PrintMessage(Message::EKind kind, const char* format, ...)
{
	// not cut to a fixed size: subclasses parse commands ("[NZB] ...") from it
	CString tmp2;

	va_list ap;
	va_start(ap, format);
	tmp2.FormatV(format, ap);
	va_end(ap);

	if (m_logPrefix)
	{
		NzbgetRsBuf prefBuf = nzbget_rs_dqh_script_format_prefixed_message(m_logPrefix, *tmp2);
		AddMessage(kind, prefBuf.data ? prefBuf.data : "");
		nzbget_rs_free(prefBuf);
	}
	else
	{
		AddMessage(kind, tmp2);
	}
}

inline void ScriptController::Write(const char* str)
{
	if (m_writepipe && str)
	{
		fwrite(str, 1, strlen(str), m_writepipe);
		fflush(m_writepipe);
	}
}

inline void ScriptController::CheckEnvSize(const std::vector<char*>& envs)
{
	if (ARGS_LIMIT_SIZE <= 0) return;

	size_t limit = static_cast<size_t>(ARGS_LIMIT_SIZE);
	size_t exceededTotal = 0;
	if (nzbget_rs_sus_check_env_size_limit(
		reinterpret_cast<const char* const*>(envs.data()),
		envs.size(),
		sizeof(void*),
		limit,
		&exceededTotal))
	{
		PrintMessage(Message::EKind::mkWarning,
					 "Total environment size (%zu bytes) exceeds OS limit (%zu bytes): %s",
					 exceededTotal, limit, "This may cause process creation to fail");
	}
}

#endif

#include "DiskState.h"
#include "Util.h"

class NzbScriptController : public ScriptController
{
protected:
	void PrepareEnvScript(NzbParameterList* parameters, const char* scriptName)
	{
		std::vector<const char*> pnames;
		std::vector<const char*> pvals;
		if (parameters)
		{
			pnames.reserve(parameters->size());
			pvals.reserve(parameters->size());
			for (NzbParameter& parameter : parameters)
			{
				pnames.push_back(parameter.GetName());
				pvals.push_back(parameter.GetValue());
			}
		}
		nzbget_rs_nzb_script_prepare_env(
			scriptName ? scriptName : "",
			parameters != nullptr ? 1 : 0,
			pnames.data(),
			pvals.data(),
			pnames.size(),
			this,
			[](void* ctx, const char* prefix, const char* name, const char* value)
			{
				static_cast<NzbScriptController*>(ctx)->SetEnvVarSpecial(prefix, name, value);
			},
			[](void* ctx, const char* stripPrefix)
			{
				static_cast<NzbScriptController*>(ctx)->PrepareEnvOptions(stripPrefix);
			});
	}

	void ExecuteScriptList(const char* scriptList)
	{
		if (Util::EmptyStr(scriptList))
		{
			return;
		}

		Tokenizer tok(scriptList, ",;");
		while (const char* scriptName = tok.Next())
		{
			auto found = g_ExtensionManager->FindIf([&scriptName](auto script)
				{
					return strcmp(scriptName, script->GetName()) == 0;
				}
			);

			if (found)
			{
				ExecuteScript(std::move(found.value()));
			}
		}
	}

	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script) = 0;
};

#ifndef COMMANDSCRIPT_H
#define COMMANDSCRIPT_H

class CommandScriptLog
{
public:
	GuardedMessageList GuardMessages() { return GuardedMessageList(&m_messages, &m_logMutex); }

	int Reset()
	{
		Guard guard(m_logMutex);
		m_messages.clear();
		return ++m_idScriptGen;
	}

	void AddMessage(int scriptId, Message::EKind kind, const char* text)
	{
		Guard guard(m_logMutex);

		// save only messages from the last started script
		if (scriptId == m_idScriptGen)
		{
			m_messages.emplace_back(++m_idMessageGen, kind, Util::CurrentTime(), text);
		}
	}

private:
	MessageList m_messages;
	Mutex m_logMutex;
	int m_idMessageGen;
	int m_idScriptGen;
};

extern CommandScriptLog* g_CommandScriptLog;

class CommandScriptController : public Thread, public NzbScriptController
{
public:
	static constexpr int COMMANDPROCESS_SUCCESS = 93;
	static constexpr int COMMANDPROCESS_ERROR = 94;

	virtual void Run()
	{
		ExecuteScriptList(m_script);
	}

	static bool StartScript(const char* scriptName, const char* command, std::unique_ptr<Options::OptEntries> modifiedOptions)
	{
		CommandScriptController* scriptController = new CommandScriptController();
		scriptController->m_script = scriptName;
		scriptController->m_command = command;
		scriptController->m_logId = g_CommandScriptLog->Reset();
		scriptController->m_modifiedOptions = std::move(modifiedOptions);

		scriptController->SetAutoDestroy(true);

		scriptController->Start();

		auto found = g_ExtensionManager->FindIf([&](auto script)
			{
				return strcmp(scriptName, script->GetName()) == 0;
			}
		);

		return found.has_value();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		PrintMessage(Message::mkInfo, "Executing script %s with command %s", script->GetName(), *m_command);

		SetArgs({script->GetEntry()});

		BString<1024> infoName("script %s with command %s", script->GetName(), *m_command);
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		PrepareParams(script->GetName());

		int exitCode = Execute();

		infoName[0] = 'S';
		SetLogPrefix(nullptr);

		switch (exitCode)
		{
			case COMMANDPROCESS_SUCCESS:
				PrintMessage(Message::mkInfo, "%s successful", *infoName);
				break;

			case COMMANDPROCESS_ERROR:
			case -1:
				PrintMessage(Message::mkError, "%s failed", *infoName);
				break;

			default:
				PrintMessage(Message::mkError, "%s failed (terminated with unknown status)", *infoName);
				break;
		}
	}

	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		NzbScriptController::AddMessage(kind, text);
		g_CommandScriptLog->AddMessage(m_logId, kind, text);
	}

	virtual const char* GetOptValue(const char* name, const char* value)
	{
		Options::OptEntry* entry = m_modifiedOptions->FindOption(name);
		return entry ? entry->GetValue() : value;
	}

private:
	CString m_script;
	CString m_command;
	int m_logId;
	std::unique_ptr<Options::OptEntries> m_modifiedOptions;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		SetEnvVar("NZBCP_COMMAND", m_command);

		PrepareEnvScript(nullptr, scriptName);
	}
};

#endif


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef QUEUESCRIPT_H
#define QUEUESCRIPT_H

#include "DiskState.h"

class QueueScriptCoordinator
{
public:
	enum EEvent
	{
		qeFileDownloaded, // lowest priority
		qeUrlCompleted,
		qeNzbMarked,
		qeNzbAdded,
		qeNzbNamed,
		qeNzbDownloaded,
		qeNzbDeleted // highest priority
	};

	void Stop() { m_stopped = true; }
	void InitOptions();
	void EnqueueScript(NzbInfo* nzbInfo, EEvent event);
	void CheckQueue();
	bool HasJob(int nzbId, bool* active);
	int GetQueueSize();
	static NzbInfo* FindNzbInfo(DownloadQueue* downloadQueue, int nzbId);

private:
	class QueueItem
	{
	public:
		QueueItem(int nzbId, std::shared_ptr<const Extension::Script> script, EEvent event) :
			m_nzbId(nzbId), m_script(std::move(script)), m_event(event) {}
		int GetNzbId() { return m_nzbId; }
		const std::shared_ptr<const Extension::Script>& GetScript() const { return m_script; }
		EEvent GetEvent() { return m_event; }
	private:
		int m_nzbId;
		std::shared_ptr<const Extension::Script> m_script;
		EEvent m_event;
	};

	typedef std::deque<std::unique_ptr<QueueItem>> Queue;

	Queue m_queue;
	Mutex m_queueMutex;
	std::unique_ptr<QueueItem> m_curItem;
	bool m_hasQueueScripts = false;
	bool m_stopped = false;

	bool UsableScript(std::shared_ptr<const Extension::Script> script, NzbInfo* nzbInfo, EEvent event);
};

extern QueueScriptCoordinator* g_QueueScriptCoordinator;

#include <algorithm>
#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

class QueueScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run() override;
	static void StartScript(NzbInfo* nzbInfo, std::shared_ptr<const Extension::Script> script, QueueScriptCoordinator::EEvent event);

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script) override;
	virtual void AddMessage(Message::EKind kind, const char* text) override;

private:
	CString m_nzbName;
	CString m_nzbFilename;
	CString m_url;
	CString m_category;
	CString m_destDir;
	CString m_queuedFilename;
	int m_id;
	int m_priority;
	CString m_dupeKey;
	EDupeMode m_dupeMode;
	int m_dupeScore;
	NzbParameterList m_parameters;
	int m_prefixLen;
	std::shared_ptr<const Extension::Script> m_script;
	QueueScriptCoordinator::EEvent m_event;
	bool m_markBad;
	NzbInfo::EDeleteStatus m_deleteStatus;
	NzbInfo::EUrlStatus m_urlStatus;
	NzbInfo::EMarkStatus m_markStatus;

	void PrepareParams(const char* scriptName);
};

inline void QueueScriptController::StartScript(NzbInfo* nzbInfo, std::shared_ptr<const Extension::Script> script, QueueScriptCoordinator::EEvent event)
{
	if (!nzbInfo || nzbInfo->GetSkipScriptProcessing())
	{
		return;
	}

	QueueScriptController* scriptController = new QueueScriptController();

	scriptController->m_nzbName = nzbInfo->GetName();
	scriptController->m_nzbFilename = nzbInfo->GetFilename();
	scriptController->m_url = nzbInfo->GetUrl();
	scriptController->m_category = nzbInfo->GetCategory();
	scriptController->m_destDir = nzbInfo->GetDestDir();
	scriptController->m_queuedFilename = nzbInfo->GetQueuedFilename();
	scriptController->m_id = nzbInfo->GetId();
	scriptController->m_priority = nzbInfo->GetPriority();
	scriptController->m_dupeKey = nzbInfo->GetDupeKey();
	scriptController->m_dupeMode = nzbInfo->GetDupeMode();
	scriptController->m_dupeScore = nzbInfo->GetDupeScore();
	scriptController->m_parameters.CopyFrom(nzbInfo->GetParameters());
	scriptController->m_script = std::move(script);
	scriptController->m_event = event;
	scriptController->m_prefixLen = 0;
	scriptController->m_markBad = false;
	scriptController->m_deleteStatus = nzbInfo->GetDeleteStatus();
	scriptController->m_urlStatus = nzbInfo->GetUrlStatus();
	scriptController->m_markStatus = nzbInfo->GetMarkStatus();
	scriptController->SetAutoDestroy(true);

	scriptController->Start();
}

inline void QueueScriptController::Run()
{
	ExecuteScript(m_script);

	SetLogPrefix(nullptr);

	if (m_markBad)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_id);
		if (nzbInfo)
		{
			nzbInfo->PrintMessage(Message::mkWarning, "Cancelling download and deleting %s", *m_nzbName);
			nzbInfo->SetDeleteStatus(NzbInfo::dsBad);
			downloadQueue->EditEntry(m_id, DownloadQueue::eaGroupDelete, nullptr);
		}
	}

	g_QueueScriptCoordinator->CheckQueue();
}

inline void QueueScriptController::ExecuteScript(std::shared_ptr<const Extension::Script> script)
{
	PrintMessage(m_event == QueueScriptCoordinator::qeFileDownloaded ? Message::mkDetail : Message::mkInfo,
		"Executing queue-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbName));

	SetArgs({script->GetEntry()});

	BString<1024> infoName("queue-script %s for %s", script->GetName(), FileSystem::BaseFileName(m_nzbName));
	SetInfoName(infoName);

	SetLogPrefix(script->GetDisplayName());
	m_prefixLen = static_cast<int>(strlen(script->GetDisplayName())) + 2; // 2 = strlen(": ");
	PrepareParams(script->GetName());

	Execute();

	SetLogPrefix(nullptr);
}

inline void QueueScriptController::PrepareParams(const char* scriptName)
{
	ResetEnv();

	NzbgetRsQueueScriptEnv env{};
	env.nzb_name = m_nzbName;
	env.nzb_id = m_id;
	env.filename = m_nzbFilename;
	env.dest_dir = m_destDir;
	env.queued_filename = m_queuedFilename;
	env.url = m_url;
	env.category = m_category;
	env.priority = m_priority;
	env.dupe_key = m_dupeKey;
	env.dupe_score = m_dupeScore;
	env.dupe_mode = static_cast<int>(m_dupeMode);
	env.event = static_cast<int>(m_event);
	env.delete_status = static_cast<int>(m_deleteStatus);
	env.url_status = static_cast<int>(m_urlStatus);
	env.mark_status = static_cast<int>(m_markStatus);

	nzbget_rs_queue_script_prepare_env(
		&env,
		this,
		[](void* ctx, const char* name, const char* value)
		{
			static_cast<QueueScriptController*>(ctx)->SetEnvVar(name, value);
		},
		[](void* ctx, const char* name, int value)
		{
			static_cast<QueueScriptController*>(ctx)->SetIntEnvVar(name, value);
		});

	PrepareEnvScript(&m_parameters, scriptName);
}

inline void QueueScriptController::AddMessage(Message::EKind kind, const char* text)
{
	const char* msgText = text + m_prefixLen;

	NzbgetRsQueueScriptCmd cmd{};
	int cmdKind = nzbget_rs_sus_parse_queue_script_cmd(
		msgText,
		static_cast<int>(m_event),
		static_cast<int>(QueueScriptCoordinator::qeNzbDownloaded),
		&cmd);
	if (cmdKind != 0)
	{
		debug("Command %s detected", msgText + 6);
		if (cmdKind == 1)
		{
			std::string param(msgText + cmd.key_offset, cmd.key_len);
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->GetParameters()->SetParameter(param.c_str(), msgText + cmd.val_offset);
			}
		}
		else if (cmdKind == 2)
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->SetFinalDir(msgText + cmd.val_offset);
			}
		}
		else if (cmdKind == 3)
		{
			m_markBad = true;
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->PrintMessage(Message::mkWarning, "Marking %s as bad", *m_nzbName);
				nzbInfo->SetMarkStatus(NzbInfo::ksBad);
			}
		}
		else
		{
			error("Invalid command \"%s\" received from %s", msgText, GetInfoName());
		}
		return;
	}
	{
		NzbInfo* nzbInfo = nullptr;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			nzbInfo = QueueScriptCoordinator::FindNzbInfo(downloadQueue, m_id);
			if (nzbInfo)
			{
				nzbInfo->AddMessage(kind, text);
			}
		}

		if (!nzbInfo)
		{
			ScriptController::AddMessage(kind, text);
		}
	}
}

inline void QueueScriptCoordinator::InitOptions()
{
	auto found = g_ExtensionManager->FindIf([&](auto script)
		{
			return script->GetQueueScript() == true;
		}
	);

	m_hasQueueScripts = found.has_value();
}

inline void QueueScriptCoordinator::EnqueueScript(NzbInfo* nzbInfo, EEvent event)
{
	if (!m_hasQueueScripts || !nzbInfo || nzbInfo->GetSkipScriptProcessing())
	{
		return;
	}

	Guard guard(m_queueMutex);

	if (event == qeNzbDownloaded)
	{
		m_queue.erase(std::remove_if(m_queue.begin(), m_queue.end(),
			[nzbInfo](std::unique_ptr<QueueItem>& queueItem)
			{
				return queueItem->GetNzbId() == nzbInfo->GetId();
			}),
			m_queue.end());
	}

	time_t curTime = Util::CurrentTime();
	if (nzbget_rs_sus_should_skip_queue_event_interval(
		static_cast<int>(event),
		static_cast<int>(qeFileDownloaded),
		g_Options->GetEventInterval(),
		static_cast<int64>(curTime),
		static_cast<int64>(nzbInfo->GetQueueScriptTime())))
	{
		return;
	}

	g_ExtensionManager->ForEach([&](auto script)
		{
			if (UsableScript(script, nzbInfo, event))
			{
				bool alreadyQueued = false;
				if (event == qeFileDownloaded)
				{
					for (QueueItem* queueItem : &m_queue)
					{
						if (queueItem->GetNzbId() == nzbInfo->GetId() && queueItem->GetScript() == script)
						{
							alreadyQueued = true;
							break;
						}
					}
				}

				if (!alreadyQueued)
				{
					std::unique_ptr<QueueItem> queueItem = std::make_unique<QueueItem>(nzbInfo->GetId(), script, event);
					if (m_curItem)
					{
						m_queue.push_back(std::move(queueItem));
					}
					else
					{
						m_curItem = std::move(queueItem);
						QueueScriptController::StartScript(nzbInfo, m_curItem->GetScript(), m_curItem->GetEvent());
					}
				}

				nzbInfo->SetQueueScriptTime(Util::CurrentTime());
			}
		}
	);
}

inline bool QueueScriptCoordinator::UsableScript(std::shared_ptr<const Extension::Script> script, NzbInfo* nzbInfo, EEvent event)
{
	size_t pcount = nzbInfo->GetParameters()->size();
	std::vector<const char*> pnames;
	std::vector<const char*> pvals;
	pnames.reserve(pcount);
	pvals.reserve(pcount);
	for (NzbParameter& parameter : nzbInfo->GetParameters())
	{
		pnames.push_back(parameter.GetName());
		pvals.push_back(parameter.GetValue());
	}
	const char* postScript = nullptr;
	if (event == qeUrlCompleted)
	{
		postScript = g_Options->GetExtensions();
		if (!Util::EmptyStr(nzbInfo->GetCategory()))
		{
			Options::Category* categoryObj = g_Options->FindCategory(nzbInfo->GetCategory(), false);
			if (categoryObj && !Util::EmptyStr(categoryObj->GetExtensions()))
			{
				postScript = categoryObj->GetExtensions();
			}
		}
	}
	return nzbget_rs_sus_is_usable_queue_script(
		script->GetQueueScript() ? 1 : 0,
		script->GetQueueEvents(),
		script->GetName(),
		static_cast<int>(event),
		static_cast<int>(qeUrlCompleted),
		pnames.data(),
		pvals.data(),
		pcount,
		postScript) != 0;
}

inline NzbInfo* QueueScriptCoordinator::FindNzbInfo(DownloadQueue* downloadQueue, int nzbId)
{
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(nzbId);
	if (nzbInfo)
	{
		return nzbInfo;
	}

	HistoryInfo* historyInfo = downloadQueue->GetHistory()->Find(nzbId);
	if (historyInfo)
	{
		return historyInfo->GetNzbInfo();
	}

	return nullptr;
}

inline void QueueScriptCoordinator::CheckQueue()
{
	if (m_stopped)
	{
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	Guard guard(m_queueMutex);

	m_curItem.reset();
	NzbInfo* curNzbInfo = nullptr;
	EEvent maxEvent = EEvent::qeFileDownloaded;
	Queue::iterator itCurItem;

	for (Queue::iterator it = m_queue.begin(); it != m_queue.end(); )
	{
		std::unique_ptr<QueueItem>& queueItem = *it;

		NzbInfo* nzbInfo = FindNzbInfo(downloadQueue, queueItem->GetNzbId());

		EEvent event = queueItem->GetEvent();
		bool ignoreEvent = nzbget_rs_sus_should_ignore_queued_script_event(
			nzbInfo ? 1 : 0,
			nzbInfo ? static_cast<int>(nzbInfo->GetDeleteStatus()) : 0,
			static_cast<int>(NzbInfo::dsNone),
			nzbInfo ? static_cast<int>(nzbInfo->GetMarkStatus()) : 0,
			static_cast<int>(NzbInfo::ksBad),
			static_cast<int>(event),
			static_cast<int>(qeNzbDeleted),
			static_cast<int>(qeNzbMarked)) != 0;

		if (ignoreEvent)
		{
			it = m_queue.erase(it);
			if (curNzbInfo)
			{
				curNzbInfo = nullptr;
				it = m_queue.begin();
			}
			continue;
		}

		if (!curNzbInfo || queueItem->GetEvent() > maxEvent)
		{
			itCurItem = it;
			curNzbInfo = nzbInfo;
			maxEvent = queueItem->GetEvent();
		}

		++it;
	}

	if (curNzbInfo)
	{
		m_curItem = std::move(*itCurItem);
		m_queue.erase(itCurItem);
		QueueScriptController::StartScript(curNzbInfo, m_curItem->GetScript(), m_curItem->GetEvent());
	}
}

inline bool QueueScriptCoordinator::HasJob(int nzbId, bool* active)
{
	Guard guard(m_queueMutex);

	bool working = m_curItem && m_curItem->GetNzbId() == nzbId;
	if (active)
	{
		*active = working;
	}
	if (!working)
	{
		for (QueueItem* queueItem : &m_queue)
		{
			working = queueItem->GetNzbId() == nzbId;
			if (working)
			{
				break;
			}
		}
	}

	return working;
}

inline int QueueScriptCoordinator::GetQueueSize()
{
	Guard guard(m_queueMutex);

	return static_cast<int>(m_queue.size()) + (m_curItem ? 1 : 0);
}

#endif

#endif
#include "ServerPool.h"

#ifndef DISABLE_PARCHECK
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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


#ifndef PARCHECKER_H
#define PARCHECKER_H

#ifndef DISABLE_PARCHECK

#include <atomic>
#include <deque>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include "Util.h"
#include "Util.h"

class Repairer;

class ParChecker
{
public:
	virtual ~ParChecker() = default;

	enum EStatus
	{
		psFailed,
		psRepairPossible,
		psRepaired,
		psRepairNotNeeded
	};

	enum EStage
	{
		ptLoadingPars,
		ptVerifyingSources,
		ptRepairing,
		ptVerifyingRepaired,
	};

	void Execute();
	void SetDestDir(const char* destDir) { m_destDir = destDir ? destDir : ""; }
	const char* GetDestDir() { return m_destDir.c_str(); }
	const char* GetParFilename() { return m_parFilename.c_str(); }
	const char* GetInfoName() { return m_infoName.c_str(); }
	void SetInfoName(const char* infoName) { m_infoName = infoName ? infoName : ""; }
	void SetNzbName(const char* nzbName) { m_nzbName = nzbName ? nzbName : ""; }
	void SetParQuick(bool parQuick) { m_parQuick = parQuick; }
	bool GetParQuick() { return m_parQuick; }
	void SetForceRepair(bool forceRepair) { m_forceRepair = forceRepair; }
	bool GetForceRepair() { return m_forceRepair; }
	void SetParFull(bool parFull) { m_parFull = parFull; }
	bool GetParFull() { return m_parFull; }
	EStatus GetStatus() { return m_status; }
	void AddParFile(const char* parFilename);
	void QueueChanged();
	void Cancel();

protected:
	class Segment
	{
	public:
		Segment(bool success, int64 offset, int size, uint32 crc) 
			: m_success(success)
			, m_offset(offset)
			, m_size(size)
			, m_crc(crc) 
		{}
		bool GetSuccess() { return m_success; }
		int64 GetOffset() { return m_offset; }
		int GetSize() { return m_size; }
		uint32 GetCrc() { return m_crc; }

	private:
		bool m_success;
		int64 m_offset;
		int m_size;
		uint32 m_crc;
	};

	typedef std::deque<Segment> SegmentList;

	class DupeSource
	{
	public:
		DupeSource(int id, const char* directory) 
			: m_id(id)
			, m_directory(directory ? directory : "") 
		{}
		int GetId() { return m_id; }
		const char* GetDirectory() { return m_directory.c_str(); }
		int GetUsedBlocks() { return m_usedBlocks; }
		void SetUsedBlocks(int usedBlocks) { m_usedBlocks = usedBlocks; }

	private:
		int m_id;
		int m_usedBlocks = 0;
		std::string m_directory;
	};

	typedef std::deque<DupeSource> DupeSourceList;

	enum EFileStatus
	{
		fsUnknown,
		fsSuccess,
		fsPartial,
		fsFailure
	};

	/**
	* Unpause par2-files
	* returns true, if the files with required number of blocks were unpaused,
	* or false if there are no more files in queue for this collection or not enough blocks
	*/
	virtual bool RequestMorePars(int blockNeeded, int* blockFound) = 0;
	virtual void UpdateProgress() {}
	virtual bool IsStopped() { return false; }
	virtual void Completed() {}
	virtual void PrintMessage([[maybe_unused]] Message::EKind kind,
		[[maybe_unused]] const char* format, ...) PRINTF_SYNTAX(3) {}
	virtual void RegisterParredFile([[maybe_unused]] const char* filename) {}
	virtual void RegisterVerifiedFile([[maybe_unused]] const char* filename) {}
	virtual bool IsParredFile([[maybe_unused]] const char* filename) { return false; }
	virtual EFileStatus FindFileCrc([[maybe_unused]] const char* filename, [[maybe_unused]] uint32* crc,
		[[maybe_unused]] SegmentList* segments) { return fsUnknown; }
	virtual const char* FindFileOrigname([[maybe_unused]] const char* filename) { return nullptr; }
	virtual void RequestDupeSources([[maybe_unused]] DupeSourceList* dupeSourceList) {}
	virtual void StatDupeSources([[maybe_unused]] DupeSourceList* dupeSourceList) {}
	/* when its own par2-files can't repair the set: the smallest par2-file of a twin
	 * (a duplicate of byte-identical files, with its own par2 set) fetched into the
	 * destination; its path, or "" for none */
	virtual std::string RequestTwinIndex() { return ""; }
	/* par2-volumes of that twin holding at least <blockNeeded> blocks of its set,
	 * fetched into the destination; their paths */
	virtual std::vector<std::string> RequestTwinVolumes([[maybe_unused]] int blockNeeded) { return {}; }
	EStage GetStage() { return m_stage; }
	const char* GetProgressLabel() { return m_progressLabel.c_str(); }
	int GetFileProgress() { return m_fileProgress; }
	int GetStageProgress() { return m_stageProgress; }

private:
	typedef std::deque<std::string> FileList;

	std::string m_infoName;
	std::string m_destDir;
	std::string m_nzbName;
	std::string m_parFilename;
	std::string m_progressLabel;
	std::string m_errMsg;

	int m_processedCount;
	int m_filesToRepair;
	int m_extraFiles;
	int m_fileProgress;
	int m_stageProgress;

	EStatus m_status = psFailed;

	std::atomic<EStage> m_stage{ptLoadingPars};
	std::atomic<int> m_quickFiles{0};

	FileList m_queuedParFiles;
	FileList m_processedFiles;
	std::vector<std::string> m_verifiedFiles;
	DupeSourceList m_dupeSources;
	std::mutex m_queuedParFilesMtx;
	std::mutex m_repairerMtx;
	std::mutex m_signalMtx;

	std::unique_ptr<Repairer> m_repairer;

	std::atomic<bool> m_parQuick{false};
	std::atomic<bool> m_parFull{false};

	bool m_queuedParFilesChanged;
	bool m_verifyingExtraFiles;
	bool m_cancelled;
	bool m_hasDamagedFiles;
	bool m_forceRepair = false;

	Repairer* GetRepairer() { return m_repairer.get(); }

public:
	// the par-set's block size once a par2-file is loaded, else 0
	int64 GetBlockSize();
private:

	void Cleanup();
	EStatus RunParCheckAll();
	EStatus RunParCheck(std::string parFilename);
	int PreProcessPar();
	bool LoadMainParBak();
	int ProcessMorePars();
	int ProcessTwinSet();
	bool LoadMorePars();
	bool AddSplittedFragments();
	bool AddMissingFiles();
	bool AddDupeFiles();
	bool AddExtraFiles(bool onlyMissing, bool externalDir, const char* directory);
	bool IsProcessedFile(const char* filename);
	void DeleteLeftovers();
	void SignalFilename(std::string str);
	void SignalProgress(int progress);
	void SignalDone(std::string str, int available, int total);
	void CheckEmptyFiles();
	std::string GetPacketCreator();

	friend class Repairer;
};

#endif

#ifndef DISABLE_PARCHECK

#include <vector>
#include <string>
#include <algorithm>
#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

class Repairer final
{
public:
	Repairer(ParChecker* owner)
		: m_owner(owner)
	{
		int configuredParBuffer = g_Options->GetParBuffer();
		int monitorMem = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveParBufferMB(configuredParBuffer) : 0;
		nzbget_rs_parchecker_calc_repairer_resources(
			g_Options->GetParThreads(), Util::NumberOfCpuCores(),
			g_MemoryMonitor != nullptr ? 1 : 0, monitorMem, configuredParBuffer,
			&m_threadsToUse, &m_memToUse);
		m_handle = nzbget_rs_par_repairer_new(
			m_owner->GetDestDir(),
			m_threadsToUse,
			m_memToUse,
			m_owner->GetParQuick() ? 1 : 0,
			g_Options->GetParScan() == Options::psFull ? 1 : 0,
			g_Options->GetParIgnoreExt());
	}

	~Repairer()
	{
		if (m_handle)
		{
			nzbget_rs_par_repairer_free(m_handle);
			m_handle = nullptr;
		}
	}

	int PreProcess(const std::string& parFilename)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		return nzbget_rs_par_repairer_preprocess(m_handle, parFilename.c_str(), &cbs);
	}

	int Process(bool dorepair)
	{
		if (dorepair)
		{
			m_owner->PrintMessage(Message::mkInfo, "Using %i thread(s) and %i MB to repair %u block(s) for %s",
				m_threadsToUse, m_memToUse, MissingBlockCount(), m_owner->m_nzbName.c_str());
		}
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		int res = nzbget_rs_par_repairer_process(m_handle, dorepair ? 1 : 0, &cbs);
		debug("ParChecker: Process-result=%i", res);
		return res;
	}

	bool LoadPacketsFromFile(const char* filename)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		return nzbget_rs_par_repairer_load_extra_par2_file(m_handle, filename, &cbs) != 0;
	}

	bool VerifyExtraFiles(const std::vector<std::string>& extrafiles, bool isSplitFragment)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		bool anyAdded = false;
		for (const std::string& extra : extrafiles)
		{
			if (nzbget_rs_par_repairer_verify_extra_file(
				m_handle, extra.c_str(), isSplitFragment ? 1 : 0, &cbs) != 0)
			{
				anyAdded = true;
			}
		}
		return anyAdded;
	}

	void Cancel()
	{
		nzbget_rs_par_repairer_cancel(m_handle);
	}

	uint32_t MissingFileCount() const
	{
		return nzbget_rs_par_repairer_missing_file_count(m_handle);
	}

	uint32_t DamagedFileCount() const
	{
		return nzbget_rs_par_repairer_damaged_file_count(m_handle);
	}

	uint32_t MissingBlockCount() const
	{
		return nzbget_rs_par_repairer_missing_block_count(m_handle);
	}

	uint32_t RecoveryBlockCount() const
	{
		return nzbget_rs_par_repairer_recovery_block_count(m_handle);
	}

	int64_t BlockSize() const
	{
		return nzbget_rs_par_repairer_block_size(m_handle);
	}

	std::string Creator() const
	{
		NzbgetRsBuf buf = nzbget_rs_par_repairer_creator(m_handle);
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	size_t SourceFileCount() const
	{
		return nzbget_rs_par_repairer_source_file_count(m_handle);
	}

	const NzbgetRsParRepairer* Handle() const { return m_handle; }

private:
	NzbgetRsParCallbacks MakeCallbacks()
	{
		NzbgetRsParCallbacks cbs{};
		cbs.ctx = this;
		cbs.on_filename = [](void* ctx, const char* name) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalFilename(name ? name : "");
		};
		cbs.on_progress = [](void* ctx, int progress) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalProgress(progress);
		};
		cbs.on_done = [](void* ctx, const char* name, int available, int total, int /*targetExists*/, int /*isComplete*/) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalDone(name ? name : "", available, total);
		};
		cbs.on_quick_verify = [](void* ctx, const char* name, uint32_t* outCrc, const NzbgetRsParSegment** outSegs, size_t* outSegLen) -> int {
			auto* self = static_cast<Repairer*>(ctx);
			uint32 crc = 0;
			ParChecker::SegmentList segments;
			ParChecker::EFileStatus st = self->m_owner->FindFileCrc(name ? name : "", &crc, &segments);
			if (outCrc)
			{
				*outCrc = crc;
			}
			self->m_lastSegments.clear();
			for (ParChecker::Segment& seg : segments)
			{
				self->m_lastSegments.push_back({
					seg.GetSuccess() ? 1 : 0,
					seg.GetOffset(),
					seg.GetSize(),
					seg.GetCrc()
				});
			}
			if (outSegs)
			{
				*outSegs = self->m_lastSegments.empty() ? nullptr : self->m_lastSegments.data();
			}
			if (outSegLen)
			{
				*outSegLen = self->m_lastSegments.size();
			}
			return (int)st;
		};
		cbs.on_quick_verified_inc = [](void* ctx) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->m_quickFiles++;
			self->m_owner->SetParFull(false);
		};
		return cbs;
	}

	ParChecker* m_owner;
	NzbgetRsParRepairer* m_handle = nullptr;
	int m_threadsToUse = 1;
	int m_memToUse = 16;
	std::vector<NzbgetRsParSegment> m_lastSegments;

	friend class ParChecker;
};


inline void ParChecker::Cleanup()
{
	std::lock_guard guard{ m_repairerMtx };

	m_repairer.reset();
	m_queuedParFiles.clear();
	m_processedFiles.clear();
	m_dupeSources.clear();
	m_errMsg = "";

	MemoryMonitor::TrimHeap();
}

inline void ParChecker::Execute()
{
	m_verifiedFiles.clear();
	m_status = RunParCheckAll();

	if (m_status == psRepairNotNeeded && m_parQuick && m_forceRepair && !IsStopped())
	{
		PrintMessage(Message::mkInfo, "Performing full par-check for %s", m_nzbName.c_str());
		m_parQuick = false;
		m_verifiedFiles.clear();
		m_status = RunParCheckAll();
	}

	// Publish only the final pass: a forced full check can overturn a quick
	// result, and its pending duplicate jobs must survive until then.
	for (const std::string& filename : m_verifiedFiles)
	{
		RegisterVerifiedFile(filename.c_str());
	}
	Completed();
}

inline ParChecker::EStatus ParChecker::RunParCheckAll()
{
	ParParser::ParFileList fileList;
	if (!ParParser::FindMainPars(m_destDir.c_str(), &fileList))
	{
		PrintMessage(Message::mkError, "Could not start par-check for %s. Could not find any par-files", m_nzbName.c_str());
		return psFailed;
	}

	EStatus allStatus = psRepairNotNeeded;
	m_parFull = true;

	for (std::string& parFilename : fileList)
	{
		debug("Found par: %s", parFilename.c_str());

		if (!IsStopped())
		{
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				m_destDir.c_str(), PATH_SEPARATOR, parFilename.c_str());
			std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
			nzbget_rs_free(pathBuf);
			std::string fullParFilename = FileSystem::GetRealPath(path)
				.value_or(std::move(path));

			int baseLen = 0;
			ParParser::ParseParFilename(parFilename.c_str(), true, &baseLen, nullptr);
			BString<1024> infoName;
			infoName.Set(parFilename.c_str(), baseLen);

			NzbgetRsBuf parInfoBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				m_nzbName.c_str(), PATH_SEPARATOR, *infoName);
			BString<1024> parInfoName;
			parInfoName.Set(parInfoBuf.data ? parInfoBuf.data : "", (int)parInfoBuf.len);
			nzbget_rs_free(parInfoBuf);
			SetInfoName(parInfoName);

			EStatus status = RunParCheck(std::move(fullParFilename));

			// accumulate total status, the worst status has priority
			if (allStatus > status)
			{
				allStatus = status;
			}
		}
	}

	return allStatus;
}

inline ParChecker::EStatus ParChecker::RunParCheck(std::string parFilename)
{
	Cleanup();
	m_parFilename = std::move(parFilename);
	m_stage = ptLoadingPars;
	m_processedCount = 0;
	m_extraFiles = 0;
	m_quickFiles = 0;
	m_verifyingExtraFiles = false;
	m_hasDamagedFiles = false;
	EStatus status = psFailed;

	PrintMessage(Message::mkInfo, "Verifying %s", m_infoName.c_str());

	debug("par: %s", m_parFilename.c_str());

	m_progressLabel = std::string("Verifying ") + m_infoName;
	m_fileProgress = 0;
	m_stageProgress = 0;
	UpdateProgress();

	int res = PreProcessPar();
	if (IsStopped() || res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		Cleanup();
		return psFailed;
	}

	std::string creator = GetPacketCreator();
	info("Recovery files created by: %s", creator.empty() ? "unknown" : creator.c_str());

	m_stage = ptVerifyingSources;
	res = GetRepairer()->Process(false);

	if (!m_parQuick)
	{
		CheckEmptyFiles();
	}

	bool addedSplittedFragments = false;
	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		addedSplittedFragments = AddSplittedFragments();
		if (addedSplittedFragments)
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (m_hasDamagedFiles && !IsStopped() && GetRepairer()->MissingFileCount() > 0 &&
		!(addedSplittedFragments && res == NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE) &&
		(g_Options->GetParScan() == Options::psExtended || g_Options->GetParScan() == Options::psDupe))
	{
		if (AddMissingFiles())
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		res = ProcessMorePars();
	}

	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE &&
		g_Options->GetParScan() == Options::psDupe)
	{
		if (AddDupeFiles())
		{
			res = GetRepairer()->Process(false);
			if (res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE && !IsStopped())
			{
				res = ProcessMorePars();
			}
		}
	}

	// a twin's par2 set (other block size, other parity level, same files) may hold
	// the recovery blocks its own lacks
	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		res = ProcessTwinSet();
	}

	if (IsStopped())
	{
		Cleanup();
		return psFailed;
	}

	status = psFailed;

	int postVerifyAction = 0;
	status = (EStatus)nzbget_rs_parchecker_eval_post_verify_action(
		res, NZBGET_RS_PAR_RESULT_SUCCESS, NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE,
		m_hasDamagedFiles ? 1 : 0, g_Options->GetParRepair() ? 1 : 0,
		(int)psFailed, (int)psRepairNotNeeded, (int)psRepairPossible,
		&postVerifyAction);
	if (postVerifyAction == 0)
	{
		PrintMessage(Message::mkInfo, "Repair not needed for %s", m_infoName.c_str());
	}
	else if (postVerifyAction == 1)
	{
		PrintMessage(Message::mkInfo, "Repairing %s", m_infoName.c_str());

		m_progressLabel = std::string("Repairing ") + m_infoName;
		m_fileProgress = 0;
		m_stageProgress = 0;
		m_processedCount = 0;
		m_stage = ptRepairing;
		m_filesToRepair = static_cast<int>(GetRepairer()->DamagedFileCount() + GetRepairer()->MissingFileCount());
		UpdateProgress();

		res = GetRepairer()->Process(true);
		if (res == NZBGET_RS_PAR_RESULT_SUCCESS)
		{
			PrintMessage(Message::mkInfo, "Successfully repaired %s", m_infoName.c_str());
			status = psRepaired;
			StatDupeSources(&m_dupeSources);
			DeleteLeftovers();
		}
		else
		{
			status = psFailed;
		}
	}
	else if (postVerifyAction == 2)
	{
		PrintMessage(Message::mkInfo, "Repair possible for %s", m_infoName.c_str());
	}

	int outcomeKind = 0;
	int useCmdErr = 0;
	status = (EStatus)nzbget_rs_parchecker_eval_cancel_or_fail_outcome(
		IsStopped() ? 1 : 0, (int)m_stage, (int)ptRepairing, (int)status,
		(int)psFailed, (int)psRepairPossible, m_errMsg.empty() ? 1 : 0, res,
		&outcomeKind, &useCmdErr);
	if (outcomeKind == 1)
	{
		PrintMessage(Message::mkWarning, "Repair cancelled for %s", m_infoName.c_str());
		m_errMsg = "repair cancelled";
	}
	else if (outcomeKind == 2)
	{
		PrintMessage(Message::mkWarning, "Par-check cancelled for %s", m_infoName.c_str());
		m_errMsg = "par-check cancelled";
	}
	else if (outcomeKind == 3)
	{
		if (useCmdErr != 0)
		{
			m_errMsg = nzbget_rs_parchecker_cmdline_err_str(res);
		}
		PrintMessage(Message::mkError, "Repair failed for %s: %s. Recovery files created by: %s",
			m_infoName.c_str(), m_errMsg.c_str(), creator.empty() ? "unknown" : creator.c_str());
	}

	nzbget_rs_par_repairer_for_each_target_verification(
		GetRepairer()->Handle(),
		(status == psRepaired || status == psRepairNotNeeded) ? 1 : 0,
		&m_verifiedFiles,
		[](void* rawCtx, const char* targetPath, int keepVerified)
		{
			auto* verifiedFiles = static_cast<std::vector<std::string>*>(rawCtx);
			verifiedFiles->erase(std::remove_if(verifiedFiles->begin(), verifiedFiles->end(),
				[targetPath](const std::string& previous)
				{
					return FileSystem::SameFilename(previous.c_str(), targetPath);
				}), verifiedFiles->end());
			if (keepVerified != 0)
			{
				verifiedFiles->emplace_back(targetPath);
			}
		});

	Cleanup();
	return status;
}

inline int ParChecker::PreProcessPar()
{
	int res = NZBGET_RS_PAR_RESULT_REPAIR_FAILED;
	while (!IsStopped() && res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		Cleanup();

		{
			std::lock_guard guard{ m_repairerMtx };
			m_repairer = std::make_unique<Repairer>(this);
		}

		res = GetRepairer()->PreProcess(m_parFilename);
		debug("ParChecker: PreProcess-result=%i", res);

		if (IsStopped())
		{
			PrintMessage(Message::mkError, "Could not verify %s: stopping", m_infoName.c_str());
			m_errMsg = "par-check was stopped";
			return NZBGET_RS_PAR_RESULT_REPAIR_FAILED;
		}

		if (res == NZBGET_RS_PAR_RESULT_INVALID_ARGS)
		{
			PrintMessage(Message::mkError, "Could not start par-check for %s. Par-file: %s", m_infoName.c_str(), m_parFilename.c_str());
			m_errMsg = "Command line could not be parsed";
			return res;
		}

		if (res != NZBGET_RS_PAR_RESULT_SUCCESS)
		{
			PrintMessage(Message::mkWarning, "Could not verify %s: par2-file could not be processed", m_infoName.c_str());
			PrintMessage(Message::mkInfo, "Requesting more par2-files for %s", m_infoName.c_str());
			bool hasMorePars = LoadMainParBak();
			if (!hasMorePars)
			{
				PrintMessage(Message::mkWarning, "No more par2-files found");
				break;
			}
		}
	}

	if (res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		PrintMessage(Message::mkError, "Could not verify %s: par2-file could not be processed", m_infoName.c_str());
		m_errMsg = "par2-file could not be processed";
		return res;
	}

	return res;
}

inline bool ParChecker::LoadMainParBak()
{
	while (!IsStopped())
	{
		bool hasMorePars = false;
		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
			m_queuedParFiles.clear();
		}

		if (hasMorePars)
		{
			return true;
		}

		int blockFound = 0;
		bool requested = RequestMorePars(1, &blockFound);
		if (requested)
		{
			m_progressLabel = "Awaiting additional par-files";
			m_fileProgress = 0;
			UpdateProgress();
		}

		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
			m_queuedParFilesChanged = false;
		}

		if (!requested && !hasMorePars)
		{
			return false;
		}

		if (!hasMorePars)
		{
			// wait until new files are added by "AddParFile" or a change is signaled by "QueueChanged"
			bool queuedParFilesChanged = false;
			while (!queuedParFilesChanged && !IsStopped())
			{
				{
					std::lock_guard guard{ m_queuedParFilesMtx };
					queuedParFilesChanged = m_queuedParFilesChanged;
				}
				Util::Sleep(100);
			}
		}
	}

	return false;
}

inline int ParChecker::ProcessMorePars()
{
	int res = NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;

	bool moreFilesLoaded = true;
	while (!IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		int missingblockcount = static_cast<int>(GetRepairer()->MissingBlockCount()) -
			static_cast<int>(GetRepairer()->RecoveryBlockCount());
		if (missingblockcount <= 0)
		{
			return NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE;
		}

		if (moreFilesLoaded)
		{
			PrintMessage(Message::mkInfo, "Need more %i par-block(s) for %s", missingblockcount, m_infoName.c_str());
		}

		bool hasMorePars;
		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
		}

		if (!hasMorePars)
		{
			int blockFound = 0;
			bool requested = RequestMorePars(missingblockcount, &blockFound);
			if (requested)
			{
				m_progressLabel = "Awaiting additional par-files";
				m_fileProgress = 0;
				UpdateProgress();
			}

			{
				std::lock_guard guard{ m_queuedParFilesMtx };
				hasMorePars = !m_queuedParFiles.empty();
				m_queuedParFilesChanged = false;
			}

			if (!requested && !hasMorePars)
			{
				NzbgetRsBuf errBuf = nzbget_rs_parchecker_format_not_enough_blocks_errmsg(
					missingblockcount, blockFound);
				m_errMsg = errBuf.data ? std::string(errBuf.data, errBuf.len) : std::string();
				nzbget_rs_free(errBuf);
				break;
			}

			if (!hasMorePars)
			{
				// wait until new files are added by "AddParFile" or a change is signaled by "QueueChanged"
				bool queuedParFilesChanged = false;
				while (!queuedParFilesChanged && !IsStopped())
				{
					{
						std::lock_guard guard{ m_queuedParFilesMtx };
						queuedParFilesChanged = m_queuedParFilesChanged;
					}
					Util::Sleep(100);
				}
			}
		}

		if (IsStopped())
		{
			break;
		}

		moreFilesLoaded = LoadMorePars();
		if (moreFilesLoaded)
		{
			res = GetRepairer()->Process(false);
		}
	}

	return res;
}

inline int ParChecker::ProcessTwinSet()
{
	std::string index = RequestTwinIndex();
	if (index.empty() || IsStopped())
	{
		return NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;
	}

	// a set of its own (its packets carry another set id): verified anew, with
	// the files on disk found by the names in its FileDesc packets
	std::string ownParFilename = m_parFilename;
	std::string ownErrMsg = m_errMsg;
	m_parFilename = index;
	m_errMsg.clear();
	int res = PreProcessPar();
	if (res != NZBGET_RS_PAR_RESULT_SUCCESS || IsStopped())
	{
		m_parFilename = ownParFilename;
		m_errMsg = ownErrMsg;
		return NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;
	}

	res = GetRepairer()->Process(false);
	if (res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE && !IsStopped())
	{
		int blockNeeded = std::max(1, static_cast<int>(GetRepairer()->MissingBlockCount()) -
			static_cast<int>(GetRepairer()->RecoveryBlockCount()));
		PrintMessage(Message::mkInfo, "Need %i par-block(s) of the twin's set for %s", blockNeeded, m_infoName.c_str());
		std::vector<std::string> volumes = RequestTwinVolumes(blockNeeded);
		for (const std::string& volume : volumes)
		{
			if (GetRepairer()->LoadPacketsFromFile(volume.c_str()))
			{
				PrintMessage(Message::mkInfo, "File %s of the twin loaded for par-check", FileSystem::BaseFileName(volume.c_str()));
			}
		}
		if (!volumes.empty() && !IsStopped())
		{
			res = GetRepairer()->Process(false);
		}
	}

	NzbgetRsBuf twinErrBuf = nzbget_rs_parchecker_format_twin_err_msg(
		res, NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE, ownErrMsg.c_str());
	if (twinErrBuf.data)
	{
		m_errMsg.assign(twinErrBuf.data, twinErrBuf.len);
		nzbget_rs_free(twinErrBuf);
	}
	return res;
}

inline bool ParChecker::LoadMorePars()
{
	FileList moreFiles;
	{
		std::lock_guard guard{ m_queuedParFilesMtx };
		moreFiles = std::move(m_queuedParFiles);
		m_queuedParFiles.clear();
	}

	for (std::string& parFilename : moreFiles)
	{
		bool loadedOK = GetRepairer()->LoadPacketsFromFile(parFilename.c_str());
		if (loadedOK)
		{
			PrintMessage(Message::mkInfo, "File %s successfully loaded for par-check", FileSystem::BaseFileName(parFilename.c_str()));
		}
		else
		{
			PrintMessage(Message::mkInfo, "Could not load file %s for par-check", FileSystem::BaseFileName(parFilename.c_str()));
		}
	}

	return !moreFiles.empty();
}

inline void ParChecker::AddParFile(const char * parFilename)
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFiles.push_back(parFilename);
	m_queuedParFilesChanged = true;
}

inline void ParChecker::QueueChanged()
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFilesChanged = true;
}

inline bool ParChecker::AddSplittedFragments()
{
	std::vector<std::string> extrafiles;
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}

	struct FragCtx
	{
		ParChecker* self;
		std::vector<std::string>* extrafiles;
	} ctx{this, &extrafiles};

	nzbget_rs_par_repairer_discover_splitted_fragments(
		GetRepairer()->Handle(),
		m_destDir.c_str(),
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size(),
		&ctx,
		[](void* rawCtx, const char* filename) -> int
		{
			return static_cast<FragCtx*>(rawCtx)->self->IsParredFile(filename) ? 1 : 0;
		},
		[](void* rawCtx, const char* current) -> const char*
		{
			return static_cast<FragCtx*>(rawCtx)->self->FindFileOrigname(current);
		},
		[](void* /*rawCtx*/, const char* msg)
		{
			detail("%s", msg);
		},
		[](void* rawCtx, const char* fullPath)
		{
			static_cast<FragCtx*>(rawCtx)->extrafiles->emplace_back(fullPath);
		});

	bool fragmentsAdded = false;

	if (!extrafiles.empty())
	{
		m_extraFiles += extrafiles.size();
		m_verifyingExtraFiles = true;
		PrintMessage(Message::mkInfo, "Found %i splitted fragments for %s", (int)extrafiles.size(), m_infoName.c_str());
		fragmentsAdded = GetRepairer()->VerifyExtraFiles(extrafiles, true);
		m_verifyingExtraFiles = false;
	}

	return fragmentsAdded;
}

inline bool ParChecker::AddMissingFiles()
{
	return AddExtraFiles(true, false, m_destDir.c_str());
}

inline bool ParChecker::AddDupeFiles()
{
	// the folder of the par2 file, not the file itself (which is no folder to scan)
	BString<1024> directory;
	const char* lastSep = strrchr(m_parFilename.c_str(), PATH_SEPARATOR);
	size_t dirLen = lastSep ? static_cast<size_t>(lastSep - m_parFilename.c_str()) : m_parFilename.size();
	directory.Set(m_parFilename.c_str(), (int)dirLen);

	bool added = AddExtraFiles(false, false, directory);

	if (GetRepairer()->MissingBlockCount() > 0)
	{
		// scanning directories of duplicates
		RequestDupeSources(&m_dupeSources);

		if (!m_dupeSources.empty())
		{
			uint32_t wasBlocksMissing = GetRepairer()->MissingBlockCount();
			uint32_t totalExtraBlocksFound = 0;
			for (DupeSource& dupeSource : m_dupeSources)
			{
				if (GetRepairer()->MissingBlockCount() > 0 && FileSystem::DirectoryExists(dupeSource.GetDirectory()))
				{
					uint32_t wasBlocksMissing2 = GetRepairer()->MissingBlockCount();
					bool oneAdded = AddExtraFiles(false, true, dupeSource.GetDirectory());
					added |= oneAdded;
					uint32_t blocksMissing2 = GetRepairer()->MissingBlockCount();
					dupeSource.SetUsedBlocks(nzbget_rs_parchecker_calc_dupe_blocks_delta(
						dupeSource.GetUsedBlocks(), wasBlocksMissing2, blocksMissing2,
						wasBlocksMissing, &totalExtraBlocksFound));
				}
			}

			if (totalExtraBlocksFound > 0)
			{
				PrintMessage(Message::mkInfo, "Found extra %u blocks in dupe sources", totalExtraBlocksFound);
			}
			else
			{
				PrintMessage(Message::mkInfo, "No extra blocks found in dupe sources");
			}
		}
	}

	return added;
}

inline bool ParChecker::AddExtraFiles(bool onlyMissing, bool externalDir, const char* directory)
{
	if (externalDir)
	{
		PrintMessage(Message::mkInfo, "Performing dupe par-scan for %s in %s", m_infoName.c_str(), FileSystem::BaseFileName(directory));
	}
	else
	{
		PrintMessage(Message::mkInfo, "Performing extra par-scan for %s", m_infoName.c_str());
	}

	std::vector<std::string> extrafiles;
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}

	struct ExtraCtx
	{
		ParChecker* self;
		std::vector<std::string>* extrafiles;
	} ctx{this, &extrafiles};

	nzbget_rs_par_repairer_find_and_sort_extra_files(
		directory,
		externalDir ? 1 : 0,
		m_parFilename.c_str(),
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size(),
		&ctx,
		[](void* rawCtx, const char* filename) -> int
		{
			return static_cast<ExtraCtx*>(rawCtx)->self->IsParredFile(filename) ? 1 : 0;
		},
		[](void* rawCtx, const char* fullPath)
		{
			static_cast<ExtraCtx*>(rawCtx)->extrafiles->emplace_back(fullPath);
		});

	// Scan files
	bool filesAdded = false;
	if (!extrafiles.empty())
	{
		m_extraFiles += extrafiles.size();
		m_verifyingExtraFiles = true;

		// adding files one by one until all missing files are found
		size_t idx = 0;
		while (!IsStopped() && idx < extrafiles.size())
		{
			const std::string& extraFile = extrafiles[idx];
			++idx;

			uint32_t wasFilesMissing = GetRepairer()->MissingFileCount();
			uint32_t wasBlocksMissing = GetRepairer()->MissingBlockCount();

			GetRepairer()->VerifyExtraFiles({ extraFile }, false);

			int fileAdded = 0;
			int blockAdded = 0;
			uint32_t blocksDelta = 0;
			int registerParred = 0;
			int abortReason = 0;
			filesAdded = nzbget_rs_parchecker_eval_extra_file_step(
				wasFilesMissing, GetRepairer()->MissingFileCount(),
				wasBlocksMissing, GetRepairer()->MissingBlockCount(),
				externalDir ? 1 : 0, filesAdded ? 1 : 0, onlyMissing ? 1 : 0,
				&fileAdded, &blockAdded, &blocksDelta, &registerParred, &abortReason) != 0;
			if (registerParred != 0)
			{
				PrintMessage(Message::mkInfo, "Found missing file %s", FileSystem::BaseFileName(extraFile.c_str()));
				RegisterParredFile(FileSystem::BaseFileName(extraFile.c_str()));
			}
			else if (blockAdded != 0)
			{
				PrintMessage(Message::mkInfo, "Found %u missing blocks", blocksDelta);
			}

			if (abortReason == 1)
			{
				PrintMessage(Message::mkInfo, "All missing files found, aborting par-scan");
				break;
			}
			if (abortReason == 2)
			{
				PrintMessage(Message::mkInfo, "All missing blocks found, aborting par-scan");
				break;
			}
		}

		m_verifyingExtraFiles = false;
	}

	return filesAdded;
}

inline bool ParChecker::IsProcessedFile(const char* filename)
{
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}
	return nzbget_rs_parchecker_is_processed_file(
		filename,
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size()) != 0;
}

inline void ParChecker::SignalFilename(std::string str)
{
	std::lock_guard guard{ m_signalMtx };

	if (m_stage == ptRepairing)
	{
		m_stage = ptVerifyingRepaired;
	}

	// don't print progress messages when verifying repaired files in quick verification mode,
	// because repaired files are not verified in this mode
	const char* stageMsg = nzbget_rs_parchecker_stage_message((int)m_stage);
	if (!(m_stage == ptVerifyingRepaired && m_parQuick))
	{
		PrintMessage(Message::mkInfo, "%s %s", stageMsg, str.c_str());
	}

	m_progressLabel = std::string(stageMsg) + " " + str;

	if (m_stage == ptLoadingPars || m_stage == ptVerifyingSources)
	{
		m_processedFiles.push_back(std::move(str));
	}

	m_fileProgress = 0;
	UpdateProgress();
}

inline void ParChecker::SignalProgress(int progress)
{
	std::lock_guard guard{ m_signalMtx };

	m_fileProgress = progress;

	int sourceFilesCount = (m_stage == ptRepairing || m_stage == ptVerifyingRepaired)
		? 0
		: static_cast<int>(GetRepairer()->SourceFileCount());
	m_stageProgress = nzbget_rs_parchecker_calc_stage_progress(
		(int)m_stage, m_fileProgress, m_processedCount, m_filesToRepair,
		sourceFilesCount, m_extraFiles, m_quickFiles);
	debug("Current-progress: %i, Total-progress: %i", m_fileProgress, m_stageProgress);

	UpdateProgress();
}

inline void ParChecker::SignalDone(std::string fileName, int available, int total)
{
	std::lock_guard guard{ m_signalMtx };

	++m_processedCount;

	bool ignore = (m_stage == ptVerifyingSources && available < total && !m_verifyingExtraFiles)
		? Util::MatchFileExt(fileName.c_str(), g_Options->GetParIgnoreExt(), ",;")
		: false;
	int newHasDamaged = m_hasDamagedFiles ? 1 : 0;
	int badBlocks = 0;
	if (nzbget_rs_parchecker_eval_signal_done(
		(int)m_stage, (int)ptVerifyingSources, available, total,
		m_verifyingExtraFiles ? 1 : 0, ignore ? 1 : 0,
		m_hasDamagedFiles ? 1 : 0, &newHasDamaged, &badBlocks) != 0)
	{
		m_hasDamagedFiles = newHasDamaged != 0;
		bool fileExists = nzbget_rs_par_repairer_target_file_exists_by_basename(
			GetRepairer()->Handle(), fileName.c_str()) != 0;

		const char* ignoreSuffix = ignore ? " (ignored)" : "";
		if (fileExists)
		{
			PrintMessage(Message::mkWarning, "File %s has %i bad block(s) of total %i block(s)%s",
				fileName.c_str(), badBlocks, total, ignoreSuffix);
		}
		else
		{
			PrintMessage(Message::mkWarning, "File %s with %i block(s) is missing%s",
				fileName.c_str(), total, ignoreSuffix);
		}

		if (!IsProcessedFile(fileName.c_str()))
		{
			m_processedFiles.push_back(std::move(fileName));
		}
	}
}

inline void ParChecker::CheckEmptyFiles()
{
	std::vector<const char*> processedPtrs;
	processedPtrs.reserve(m_processedFiles.size());
	for (const std::string& p : m_processedFiles)
	{
		processedPtrs.push_back(p.c_str());
	}
	if (nzbget_rs_par_repairer_check_empty_files(
		GetRepairer()->Handle(),
		processedPtrs.empty() ? nullptr : processedPtrs.data(),
		processedPtrs.size(),
		this,
		[](void* rawCtx, const char* msg)
		{
			static_cast<ParChecker*>(rawCtx)->PrintMessage(Message::mkWarning, "%s", msg);
		}) != 0)
	{
		m_hasDamagedFiles = true;
	}
}

inline void ParChecker::Cancel()
{
	{
		std::lock_guard guard{ m_repairerMtx };
		if (m_repairer)
		{
			m_repairer->Cancel();
		}
	}
	QueueChanged();
}

inline void ParChecker::DeleteLeftovers()
{
	nzbget_rs_par_repairer_delete_leftovers(
		GetRepairer()->Handle(),
		this,
		[](void* rawCtx, const char* msg)
		{
			static_cast<ParChecker*>(rawCtx)->PrintMessage(Message::mkInfo, "%s", msg);
		});
}

inline std::string ParChecker::GetPacketCreator()
{
	if (!m_repairer || !GetRepairer())
	{
		return "";
	}
	return GetRepairer()->Creator();
}

inline int64 ParChecker::GetBlockSize()
{
	if (!m_repairer || !GetRepairer())
	{
		return 0;
	}
	return GetRepairer()->BlockSize();
}

#endif

#endif

#include "DiskState.h"
#endif

class RepairController : public Thread, public ScriptController
{
public:
	RepairController();
	virtual void Stop();

#ifndef DISABLE_PARCHECK
	virtual void Run();
	static void StartJob(PostInfo* postInfo);
	bool AddPar(FileInfo* fileInfo, bool deleted);

protected:
	void UpdateParCheckProgress();
	void ParCheckCompleted();
	void CheckPauseState(PostInfo* postInfo);
	bool RequestMorePars(NzbInfo* nzbInfo, const char* parFilename, int blockNeeded, int* blockFound);

private:
	class PostParChecker final : public ParChecker
	{
	public:
		PostInfo* GetPostInfo() { return m_postInfo; }
		void SetPostInfo(PostInfo* postInfo) { m_postInfo = postInfo; }
		time_t GetParTime() { return m_parTime; }
		void SetParTime(time_t parTime) { m_parTime = parTime; }
		time_t GetRepairTime() { return m_repairTime; }
		void SetRepairTime(time_t repairTime) { m_repairTime = repairTime; }
		int GetDownloadSec() { return m_downloadSec; }
		void SetDownloadSec(int downloadSec) { m_downloadSec = downloadSec; }
	protected:
		bool RequestMorePars(int blockNeeded, int* blockFound) override;
		void UpdateProgress() override;
		bool IsStopped() override { return m_owner->IsStopped(); };
		void Completed() override { m_owner->ParCheckCompleted(); }
		void PrintMessage(Message::EKind kind, const char* format, ...) override PRINTF_SYNTAX(3);
		void RegisterParredFile(const char* filename) override;
		void RegisterVerifiedFile(const char* filename) override;
		bool IsParredFile(const char* filename) override;
		EFileStatus FindFileCrc(const char* filename, uint32* crc, SegmentList* segments) override;
		const char* FindFileOrigname(const char* filename) override;
		void RequestDupeSources(DupeSourceList* dupeSourceList) override;
		void StatDupeSources(DupeSourceList* dupeSourceList) override;
		std::string RequestTwinIndex() override;
		std::vector<std::string> RequestTwinVolumes(int blockNeeded) override;
	public:
		void DeleteTwinFiles();
	private:
		std::vector<TwinCheck::NzbEntry> m_twinEntries;
		std::string m_twinName;
		std::string m_twinIndex;
		uint64 m_twinBlockSize = 0;
		std::vector<std::string> m_twinFiles;
		ArticleFetcher m_twinFetcher;
		std::string FetchTwinFile(const TwinCheck::NzbEntry& entry, std::vector<char>* keep = nullptr);
		RepairController* m_owner;
		PostInfo* m_postInfo;
		time_t m_parTime;
		time_t m_repairTime;
		int m_downloadSec;

		friend class RepairController;
	};

	struct BlockInfo
	{
		FileInfo* m_fileInfo;
		int m_blockCount;
		BlockInfo(FileInfo* fileInfo, int blockCount) :
			m_fileInfo(fileInfo), m_blockCount(blockCount) {}
	};

	typedef std::deque<BlockInfo> Blocks;

	PostInfo* m_postInfo;
	PostParChecker m_parChecker;

	void FindPars(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* parFilename,
		Blocks& blocks, bool strictParName, bool exactParName, int* blockFound);
#endif
};

class StreamRepairController : public Thread, public ScriptController
{
public:
	virtual void Run();
	virtual void Stop();
	static void StartJob(PostInfo* postInfo);
	static void StartLive(NzbInfo* nzbInfo);

protected:
	virtual void AddMessage(Message::EKind kind, const char* text);

private:
	struct RepairTarget
	{
		int FileId;
		CString Filename;
		int64 DecodedFileSize;
		StreamRangeList Holes;
		bool NeedsRepair() const { return !Holes.empty() || DecodedFileSize <= 0; }
		int64 EncodedSize = 0;
		int ArticleCount = 0;
		bool Recreated = false;
		int64 FailedSize = 0;
		int64 MissedSize = 0;
		int FailedArticles = 0;
		bool IsParFile = false;
		bool PatchEligible = true;
		int PositionalRank = -1;
		int PositionalWindow = 0;
		uint64 StepsHash = 0;
	};

	struct DonorSource
	{
		CString QueuedFilename;
		CString InfoName;
		CString Password;
	};

	PostInfo* m_postInfo = nullptr;
	bool m_liveMode = false;
	int m_nzbId = 0;
	static constexpr int SlowRepairFactor = 3;
	static constexpr int MaxRunFactor = 5;
	static constexpr int MaxConnectionWaitSec = 1800;
	std::thread m_watchdog;
	std::mutex m_watchdogMutex;
	std::condition_variable m_watchdogCond;
	bool m_watchdogDone = false;
	std::atomic<bool> m_timedOut{false};
	std::atomic<int64> m_progressBytes{0};
	int64 m_missingBytes = 0;
	CString m_slowReason;
	static constexpr int DonorCheckSec = 20;
	std::map<std::string, bool> m_donorDead;
	std::atomic<int> m_checkingDonor{0};
	std::atomic<int> m_materializing{0};
	std::atomic<int> m_waitingForConnection{0};
	bool DonorDead(const DonorSource& donor, NzbInfo* donorNzb);
	void StartWatchdog(const std::vector<RepairTarget>& targets);
	void StopWatchdog(const char* nzbName, const std::vector<RepairTarget>& targets);
	bool Interrupted() { return IsStopped() && !m_timedOut; }
	ArticleFetcher m_fetcher;
	ArticleBatchFetcher m_batchFetcher;
	CString m_targetPassword;
	int m_recoveredArticles = 0;
	int64 m_recoveredBytes = 0;
	int m_recoveredHoles = 0;
	int64 m_recoveredFailedSize = 0;
	int64 m_recoveredFailedParSize = 0;
	bool m_holesRemain = false;
	bool m_hasParFiles = false;
	std::vector<RepairTarget> m_targets;

	void RunLive();
	void RepairCompletedLive(std::vector<RepairTarget>& targets);
	void RefreshLiveNames(std::vector<RepairTarget>& targets,
		std::vector<CString>& memberNames);
	void RefreshLiveTargetName(RepairTarget& target);
	void CollectTargets(NzbInfo* nzbInfo, std::vector<RepairTarget>& targets,
		std::vector<CString>& memberNames);
	void ComputePositionalRanks(const char* destDir, std::vector<RepairTarget>& targets,
		const std::vector<CString>& memberNames);
	void CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo,
		std::vector<DonorSource>& donors);
	void ExecRepair(const char* destDir, std::vector<RepairTarget>& targets,
		std::vector<DonorSource>& donors, const std::vector<CString>& memberNames);
	static void MarkRecreated(NzbInfo* nzbInfo, const RepairTarget& target);
	static void MarkRepaired(NzbInfo* nzbInfo, const RepairTarget& target);
	void ExecCrossPackRepair(const char* destDir, std::vector<RepairTarget>& targets,
		std::vector<DonorSource>& donors, const std::vector<CString>& memberNames);
	void ReportRemainingHoles(std::vector<RepairTarget>& targets);
	void RepairCompleted();
};

#include <algorithm>
#include <chrono>
#include <exception>
#include <new>
#include <mutex>
#include "Util.h"
#include "Util.h"
#include "QueueCoordinator.h"
#include "QueueCoordinator.h"
#include "QueueCoordinator.h"
#include "Util.h"
#include "Util.h"
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2018 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
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


#ifndef UNPACK_CONTROLLER_H
#define UNPACK_CONTROLLER_H

#include "Util.h"
#include "DiskState.h"

class UnpackController final : public Thread, public ScriptController
{
public:
	void Run() override;
	void Stop() override;
	static void StartJob(PostInfo* postInfo);
	static bool HasCompletedArchiveFiles(NzbInfo* nzbInfo);
	static Mutex& GetUnpackMutex();
	bool IsUnpacking() const { return m_unpacking; }

	~UnpackController() override;

protected:
	void AddMessage(Message::EKind kind, const char* text) override;

private:
	enum EUnpacker
	{
		upUnrar,
		upSevenZip
	};

	typedef std::vector<CString> FileList;

	PostInfo* m_postInfo = nullptr;
	CString m_name;
	CString m_infoName;
	CString m_infoNameUp;
	CString m_destDir;
	CString m_finalDir;
	CString m_unpackDir;
	CString m_unpackExtendedDir;
	CString m_password;
	EUnpacker m_unpacker;
	bool m_interDir = false;
	bool m_allOkMessageReceived = false;
	bool m_noFilesMessageReceived = false;
	bool m_hasParFiles = false;
	bool m_hasRarFiles = false;
	bool m_hasNotUnpackedRarFiles = false;
	bool m_hasRenamedArchiveFiles = false;
	bool m_hasSevenZipFiles = false;
	bool m_hasSevenZipMultiFiles = false;
	bool m_hasSplittedFiles = false;
	bool m_unpackOk = false;
	bool m_unpackStartError = false;
	bool m_unpackSpaceError = false;
	bool m_unpackDecryptError = false;
	bool m_unpackPasswordError = false;
	bool m_cleanedUpDisk = false;
	bool m_autoTerminated = false;
	bool m_finalDirCreated = false;
	bool m_unpackDirCreated = false;
	bool m_passListTried = false;
	std::atomic<bool> m_unpacking{false};
	std::atomic<std::thread::id> m_execThreadId{};
	FileList m_joinedFiles;
	FileList m_downloadedFiles;

	void ExecuteUnpack(EUnpacker unpacker, const char* password, bool multiVolumes);
	void UnpackArchives(EUnpacker unpacker, bool multiVolumes);
	void JoinSplittedFiles();
	void Completed();
	void CreateUnpackDir();
	bool Cleanup();
	void CheckArchiveFiles();
	void SetProgressLabel(const char* progressLabel);
#ifndef DISABLE_PARCHECK
	void RequestParCheck(bool forceRepair);
#endif
	bool WillRetryWithPass();
};

#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

inline UnpackController::~UnpackController()
{
#ifndef DISABLE_TLS
	OpenSSL::StopSSLThread();
#endif
}

inline Mutex& UnpackController::GetUnpackMutex()
{
	static Mutex s_unpackMutex;
	return s_unpackMutex;
}

inline void UnpackController::StartJob(PostInfo* postInfo)
{
	UnpackController* unpackController = new UnpackController();
	unpackController->m_postInfo = postInfo;
	unpackController->SetAutoDestroy(false);

	postInfo->SetPostThread(unpackController);

	unpackController->Start();
}

inline void UnpackController::Run()
{
	time_t start = Util::CurrentTime();
	m_unpackOk = true;
	bool unpack;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		m_destDir = m_postInfo->GetNzbInfo()->GetDestDir();
		m_finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		m_name = m_postInfo->GetNzbInfo()->GetName();

		for (CompletedFile& completedFile : m_postInfo->GetNzbInfo()->GetCompletedFiles())
		{
			m_downloadedFiles.emplace_back(completedFile.GetFilename());
		}

		NzbParameter* parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:");
		unpack = !(parameter && !strcasecmp(parameter->GetValue(), "no"));

		parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_password = parameter->GetValue();
		}
	}

	m_infoName.Format("unpack for %s", *m_name);
	m_infoNameUp.Format("Unpack for %s", *m_name);

	m_hasParFiles = ParParser::FindMainPars(m_destDir, nullptr);

	if (unpack)
	{
		CheckArchiveFiles();
	}

	SetInfoName(m_infoName);
	SetWorkingDir(m_destDir);

	bool hasFiles = m_hasRarFiles || m_hasSevenZipFiles || m_hasSevenZipMultiFiles || m_hasSplittedFiles;

	if (m_postInfo->GetUnpackTried() && !m_postInfo->GetParRepaired() &&
		(!m_password.Empty() || Util::EmptyStr(g_Options->GetUnpackPassFile()) || m_postInfo->GetPassListTried()))
	{
		PrintMessage(Message::mkInfo, "Second unpack attempt skipped for %s due to par-check not repaired anything", *m_name);
		PrintMessage(Message::mkError,
			m_postInfo->GetLastUnpackStatus() == (int)NzbInfo::usPassword ?
				"%s failed: checksum error in the encrypted file. Corrupt file or wrong password." :
				"%s failed.",
			*m_infoNameUp);
		m_postInfo->GetNzbInfo()->SetUnpackStatus((NzbInfo::EPostUnpackStatus)m_postInfo->GetLastUnpackStatus());
	}
	else if (unpack && hasFiles)
	{
		PrintMessage(Message::mkInfo, "Unpacking %s", *m_name);

		CreateUnpackDir();

		if (m_hasRarFiles)
		{
			if (m_hasNotUnpackedRarFiles || m_unpackDirCreated)
			{
				if (m_postInfo->GetNzbInfo()->GetDirectUnpackStatus() == NzbInfo::nsSuccess)
				{
					PrintMessage(
						m_unpackDirCreated ? Message::mkWarning : Message::mkInfo,
						m_unpackDirCreated ?
							"Could not find files unpacked by direct unpack, unpacking all files again" :
							"Found archive files not processed by direct unpack, unpacking all files again");
				}

				// Discard info about extracted archives to prevent reusing on next unpack attempt
				m_postInfo->GetExtractedArchives()->clear();

				UnpackArchives(upUnrar, false);
			}
			else
			{
				PrintMessage(Message::mkInfo, "Using directly unpacked files");
			}
		}

		if (m_hasSevenZipFiles && m_unpackOk)
		{
			UnpackArchives(upSevenZip, false);
		}

		if (m_hasSevenZipMultiFiles && m_unpackOk)
		{
			UnpackArchives(upSevenZip, true);
		}

		if (m_hasSplittedFiles && m_unpackOk)
		{
			JoinSplittedFiles();
		}

		Completed();

		m_joinedFiles.clear();
	}
	else if (unpack)
	{
#ifndef DISABLE_PARCHECK
		if (m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped &&
			m_postInfo->GetNzbInfo()->GetParRenameStatus() <= NzbInfo::rsSkipped &&
			m_hasParFiles)
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			RequestParCheck(false);
		}
		else
#endif
		if (m_hasRenamedArchiveFiles)
		{
			PrintMessage(Message::mkError, "Could not unpack %s due to renamed archive files", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usFailure);
		}
		else
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
		}
	}
	else
	{
		PrintMessage(Message::mkInfo, "Unpack for %s skipped", *m_name);
		m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
	}

	m_postInfo->GetNzbInfo()->SetUnpackSec(
		m_postInfo->GetNzbInfo()->GetUnpackSec() + (int)(Util::CurrentTime() - start));

	m_postInfo->SetWorking(false);
}

inline void UnpackController::UnpackArchives(EUnpacker unpacker, bool multiVolumes)
{
	if (!m_postInfo->GetUnpackTried() || m_postInfo->GetParRepaired())
	{
		ExecuteUnpack(unpacker, m_password, multiVolumes);
		if (!m_unpackOk && m_hasParFiles && !m_unpackPasswordError &&
			m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped)
		{
			debug("For rar4- or 7z-archives try par-check first, before trying password file");
			return;
		}
	}
	else
	{
		m_unpackOk = false;
		m_unpackDecryptError = m_postInfo->GetLastUnpackStatus() == (int)NzbInfo::usPassword;
	}

	if (!m_unpackOk && !m_unpackStartError && !m_unpackSpaceError &&
		(m_unpackDecryptError || m_unpackPasswordError) &&
		(!GetTerminated() || m_autoTerminated) &&
		m_password.Empty() && !Util::EmptyStr(g_Options->GetUnpackPassFile()))
	{
		struct PassTryCtx
		{
			UnpackController* self;
			EUnpacker unpacker;
			bool multiVolumes;
		} ctx{this, unpacker, multiVolumes};

		auto shouldContinueCb = [](void* rawCtx) -> int
		{
			auto* self = static_cast<PassTryCtx*>(rawCtx)->self;
			return (!self->m_unpackOk && !self->m_unpackStartError && !self->m_unpackSpaceError &&
				(self->m_unpackDecryptError || self->m_unpackPasswordError)) ? 1 : 0;
		};
		auto tryPasswordCb = [](void* rawCtx, const char* password)
		{
			auto* c = static_cast<PassTryCtx*>(rawCtx);
			UnpackController* self = c->self;
			debug("Password line: %s", password);
			if (self->IsStopped() && self->m_autoTerminated)
			{
				self->ScriptController::Resume();
				self->Thread::Resume();
			}
			self->m_unpackDecryptError = false;
			self->m_unpackPasswordError = false;
			self->m_autoTerminated = false;
			self->PrintMessage(Message::mkInfo, "Trying password %s for %s", password, *self->m_name);
			self->ExecuteUnpack(c->unpacker, password, c->multiVolumes);
		};

		if (!nzbget_rs_unpack_controller_try_password_file(
			g_Options->GetUnpackPassFile(), &ctx, shouldContinueCb, tryPasswordCb))
		{
			PrintMessage(Message::mkError, "Could not open file %s", g_Options->GetUnpackPassFile());
			return;
		}

		m_passListTried = !IsStopped() || m_autoTerminated;
	}
}

inline void UnpackController::ExecuteUnpack(EUnpacker unpacker, const char* password, bool multiVolumes)
{
	const bool isSevenZip = unpacker == upSevenZip;
	if (!isSevenZip)
	{
		m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	}
	m_allOkMessageReceived = false;
	m_unpackSpaceError = false;
	m_unpacker = unpacker;

	if (isSevenZip)
	{
		PrintMessage(Message::mkInfo, "Executing 7-Zip");
		SetLogPrefix("7-Zip");
	}
	SetProgressLabel("");
	Guard unpackGuard(GetUnpackMutex());
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;

	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkDetail;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			case 4: msgKind = Message::mkInfo; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->AddMessage(msgKind, msg);
	};

	auto isStoppedCb = [](void* ctx) -> int
	{
		if (!ctx) return 0;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		return (self->IsStopped() || self->GetTerminated()) ? 1 : 0;
	};

	auto resetAllOkCb = [](void* ctx)
	{
		if (ctx) static_cast<UnpackController*>(ctx)->m_allOkMessageReceived = false;
	};

	auto hasAllOkCb = [](void* ctx) -> int
	{
		return (ctx && static_cast<UnpackController*>(ctx)->m_allOkMessageReceived) ? 1 : 0;
	};

	const char* pw = Util::EmptyStr(password) ? "" : password;
	const int cleanupDisk = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
	const int useTempDir = (g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0;
	bool allOk = nzbget_rs_unpack_controller_extract_archives(
		m_destDir,
		m_unpackDir,
		pw,
		isSevenZip ? 1 : 0,
		multiVolumes ? 1 : 0,
		useTempDir,
		cleanupDisk,
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		this,
		resetAllOkCb,
		hasAllOkCb,
		logCb,
		isStoppedCb) != 0;

	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	SetLogPrefix(nullptr);
	SetProgressLabel("");

	m_unpackOk = allOk && m_allOkMessageReceived && !GetTerminated() && !IsStopped();
	m_unpackStartError = false;
}

inline bool UnpackController::WillRetryWithPass()
{
	return (m_unpackDecryptError || m_unpackPasswordError) &&
		m_password.Empty() && !g_Options->GetUnpackPassFilePath().empty();
}

inline void UnpackController::JoinSplittedFiles()
{
	SetLogPrefix("Join");
	SetProgressLabel("");
	m_postInfo->SetStageProgress(0);

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkInfo;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->PrintMessage(msgKind, "%s", msg);
	};

	auto progressCb = [](void* ctx, const char* label, int stageProgress)
	{
		if (!ctx) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		if (label)
		{
			self->SetProgressLabel(label);
		}
		if (stageProgress >= 0 && self->m_postInfo)
		{
			self->m_postInfo->SetStageProgress(stageProgress);
		}
	};

	auto addJoinedCb = [](void* ctx, const char* pieceName)
	{
		if (!ctx || !pieceName) return;
		static_cast<UnpackController*>(ctx)->m_joinedFiles.emplace_back(pieceName);
	};

	const int parSuccess = (m_postInfo && m_postInfo->GetNzbInfo() &&
		m_postInfo->GetNzbInfo()->GetParStatus() == NzbInfo::psSuccess) ? 1 : 0;
	const int writeBufferKb = g_Options ? g_Options->GetWriteBuffer() : 0;

	if (nzbget_rs_unpack_controller_join_splitted_files(
		m_destDir, m_unpackDir, parSuccess, writeBufferKb,
		this, logCb, progressCb, addJoinedCb) == 0)
	{
		m_unpackOk = false;
	}

	SetLogPrefix(nullptr);
	SetProgressLabel("");
}

inline void UnpackController::Completed()
{
	if (!m_autoTerminated && !(m_postInfo && m_postInfo->GetDeleted()) &&
		(IsStopped() || GetTerminated()))
	{
		debug("Unpack stopped due to reload/shutdown, preserving _unpack directory");
		return;
	}

	bool cleanupSuccess = Cleanup();

	if (m_unpackOk && cleanupSuccess)
	{
		PrintMessage(Message::mkInfo, "%s %s", *m_infoNameUp, "successful");
		m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSuccess);
		m_postInfo->GetNzbInfo()->SetUnpackCleanedUpDisk(m_cleanedUpDisk);
		if (g_Options->GetParRename())
		{
			//request par-rename check for extracted files
			m_postInfo->GetNzbInfo()->SetParRenameStatus(NzbInfo::rsNone);
		}
	}
	else
	{
		int parcheckEnabled =
#ifndef DISABLE_PARCHECK
			1;
#else
			0;
#endif
		int forceRepair = 0;
		int unpackStatus = (int)NzbInfo::usFailure;
		int reqPar = nzbget_rs_unpack_controller_eval_completed_failure(
			parcheckEnabled,
			m_unpackOk ? 1 : 0,
			(int)m_postInfo->GetNzbInfo()->GetParStatus(),
			(int)NzbInfo::psSkipped,
			m_postInfo->GetNzbInfo()->GetParFull() ? 1 : 0,
			m_unpackStartError ? 1 : 0,
			m_unpackSpaceError ? 1 : 0,
			m_unpackPasswordError ? 1 : 0,
			m_unpackDecryptError ? 1 : 0,
			GetTerminated() ? 1 : 0,
			IsStopped() ? 1 : 0,
			m_autoTerminated ? 1 : 0,
			m_hasParFiles ? 1 : 0,
			!m_password.Empty() ? 1 : 0,
			!Util::EmptyStr(g_Options->GetUnpackPassFile()) ? 1 : 0,
			m_passListTried ? 1 : 0,
			&forceRepair,
			&unpackStatus);
#ifndef DISABLE_PARCHECK
		if (reqPar != 0)
		{
			RequestParCheck(forceRepair != 0);
		}
		else
#endif
		{
			PrintMessage(Message::mkError, "%s failed", *m_infoNameUp);
			m_postInfo->GetNzbInfo()->SetUnpackStatus((NzbInfo::EPostUnpackStatus)unpackStatus);
		}
	}
}

#ifndef DISABLE_PARCHECK
inline void UnpackController::RequestParCheck(bool forceRepair)
{
	PrintMessage(Message::mkInfo, "%s requested %s", *m_infoNameUp,
		forceRepair ? "par-check with forced repair" : "par-check/repair");
	m_postInfo->SetRequestParCheck(true);
	m_postInfo->SetForceRepair(forceRepair);
	m_postInfo->SetUnpackTried(true);
	m_postInfo->SetPassListTried(m_passListTried);
	m_postInfo->SetLastUnpackStatus(
		m_unpackSpaceError ? (int)NzbInfo::usSpace :
		(m_unpackPasswordError || m_unpackDecryptError) ? (int)NzbInfo::usPassword :
		(int)NzbInfo::usFailure);
}
#endif

inline void UnpackController::CreateUnpackDir()
{
	m_interDir = nzbget_rs_unpack_use_inter_dir(
		m_postInfo->GetNzbInfo()->GetDestDir(), g_Options->GetInterDir(), PATH_SEPARATOR) != 0;

	if (m_interDir && m_finalDir.Empty())
	{
		m_finalDir = m_postInfo->GetNzbInfo()->BuildFinalDirName();

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->SetFinalDir(m_finalDir);
		}

		m_finalDirCreated = !FileSystem::DirectoryExists(m_finalDir);
	}

	const char* destDir = !m_finalDir.Empty() ? *m_finalDir : *m_destDir;

	if (g_Options->GetUseTempUnpackDir())
	{
		m_unpackDir.Format("%s%c%s", destDir, PATH_SEPARATOR, "_unpack");
	}
	else
	{
		m_unpackDir = destDir;
	}

	m_unpackDirCreated = !FileSystem::DirectoryExists(m_unpackDir);

	detail("Unpacking into %s", *m_unpackDir);

	CString errmsg;
	if (!FileSystem::ForceDirectories(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not create directory %s: %s", *m_unpackDir, *errmsg);
	}
}

inline void UnpackController::CheckArchiveFiles()
{
	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	std::vector<const char*> extractedPtrs;
	if (m_postInfo && m_postInfo->GetExtractedArchives())
	{
		extractedPtrs.reserve(m_postInfo->GetExtractedArchives()->size());
		for (const CString& f : *m_postInfo->GetExtractedArchives())
		{
			extractedPtrs.push_back(*f);
		}
	}

	int hasRar = 0, hasNotUnpackedRar = 0, hasRenamed = 0;
	int hasSevenZip = 0, hasSevenZipMulti = 0, hasSplitted = 0;
	nzbget_rs_unpack_controller_check_archive_files(
		m_destDir,
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		extractedPtrs.data(),
		extractedPtrs.size(),
		g_Options ? g_Options->GetUnpackIgnoreExt() : "",
		&hasRar,
		&hasNotUnpackedRar,
		&hasRenamed,
		&hasSevenZip,
		&hasSevenZipMulti,
		&hasSplitted);

	m_hasRarFiles = hasRar != 0;
	m_hasNotUnpackedRarFiles = hasNotUnpackedRar != 0;
	m_hasRenamedArchiveFiles = hasRenamed != 0;
	m_hasSevenZipFiles = hasSevenZip != 0;
	m_hasSevenZipMultiFiles = hasSevenZipMulti != 0;
	m_hasSplittedFiles = hasSplitted != 0;
}

inline bool UnpackController::Cleanup()
{
	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	std::vector<const char*> joinedPtrs;
	joinedPtrs.reserve(m_joinedFiles.size());
	for (const CString& f : m_joinedFiles)
	{
		joinedPtrs.push_back(*f);
	}

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkInfo;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->PrintMessage(msgKind, "%s", msg);
	};

	int cleanedUpDisk = m_cleanedUpDisk ? 1 : 0;
	bool ok = nzbget_rs_unpack_controller_cleanup(
		m_destDir,
		m_finalDir,
		m_unpackDir,
		m_unpackOk ? 1 : 0,
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		m_finalDirCreated ? 1 : 0,
		(g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0,
		m_interDir ? 1 : 0,
		m_hasSplittedFiles ? 1 : 0,
		static_cast<uint32_t>(FileSystem::uMask),
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		joinedPtrs.data(),
		joinedPtrs.size(),
		&cleanedUpDisk,
		this,
		logCb) != 0;
	m_cleanedUpDisk = cleanedUpDisk != 0;
	return ok;
}

inline void UnpackController::AddMessage(Message::EKind kind, const char* text)
{
	BString<1024> msgText = text;
	NzbgetRsUnpackMessageInfo info{};
	nzbget_rs_unpack_parse_message(
		text, m_unpackExtendedDir, m_unpacker == upSevenZip ? 1 : 0, &info);
	if (m_unpacker == upUnrar && info.strip_extracting_dir != 0)
	{
		msgText.Format("Unrar: Extracting %s", text + info.stripped_suffix_offset);
	}

	if (m_postInfo)
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, msgText);
	}

	if (m_postInfo && m_unpacker == upUnrar && info.reset_stage_time != 0)
	{
		m_postInfo->SetStageTime(Util::CurrentTime());
	}

	if (m_postInfo && m_unpacker == upUnrar && info.has_extracting_progress != 0)
	{
		SetProgressLabel(msgText + 7);
	}

	if (m_postInfo && m_unpacker == upUnrar && info.has_extracting_from != 0)
	{
#ifdef DEBUG
		const char *filename = text + 23;
		debug("Filename: %s", filename);
#endif
		SetProgressLabel(text + 7);
	}

	if (info.decrypt_error != 0)
	{
		m_unpackDecryptError = true;
	}

	if (info.password_error != 0)
	{
		m_unpackPasswordError = true;
	}

	if (nzbget_rs_unpack_is_space_error(kind == Message::mkError ? 1 : 0, text) != 0)
	{
		m_unpackSpaceError = true;
	}

	if (!IsStopped() && (m_unpackDecryptError || m_unpackPasswordError || info.crc_or_volume_error))
	{
		if (m_postInfo && !WillRetryWithPass())
		{
			m_postInfo->GetNzbInfo()->AddMessage(
				Message::mkWarning,
				BString<1024>("Cancelling %s due to errors", *m_infoName));
		}

		m_autoTerminated = true;
		Stop();
	}

	if (info.all_ok != 0)
	{
		m_allOkMessageReceived = true;
	}
}

inline void UnpackController::Stop()
{
	debug("Stopping unpack");
	Thread::Stop();
	bool isExecThread = std::this_thread::get_id() == m_execThreadId.load();
	if (m_unpacking && !isExecThread && !m_autoTerminated && HasActiveProcess())
	{
		if (Break())
		{
			for (int i = 0; i < 50 && m_unpacking && HasActiveProcess(); ++i)
			{
				Util::Sleep(100);
			}
		}
		if (m_unpacking)
		{
			Terminate();
		}
	}
	else
	{
		Terminate();
	}
}

inline void UnpackController::SetProgressLabel(const char* progressLabel)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_postInfo->SetProgressLabel(progressLabel);
}

inline bool UnpackController::HasCompletedArchiveFiles(NzbInfo* nzbInfo)
{
	for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_unpack_has_completed_archive_ext(completedFile.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef DIRECTUNPACK_H
#define DIRECTUNPACK_H

#include <atomic>
#include "Util.h"
#include "DiskState.h"

class DirectUnpack final : public Thread, public ScriptController
{
public:
	virtual void Run();
	static void StartJob(NzbInfo* nzbInfo);
	void Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo);
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	static bool IsArchiveFilename(const char* filename);
	bool IsUnpacking() const { return m_unpacking; }
	bool IsWaitingVolume() const { return m_waitingVolume; }

protected:
	virtual void AddMessage(Message::EKind kind, const char* text);

private:
	/**
	 *  @brief Checks if Direct unpack can proceed.
	 *
	 *  @return True if direct unpack is permitted (either the file is not an
 	 *          archive or the archive contains no failed articles), false otherwise.
	 */
	bool CanProceed(FileInfo* fileInfo) const;

	typedef std::deque<CString> ArchiveList;

	int m_nzbId;
	CString m_name;
	CString m_infoName;
	CString m_infoNameUp;
	CString m_destDir;
	CString m_finalDir;
	CString m_unpackDir;
	CString m_unpackExtendedDir;
	CString m_password;
	CString m_waitingFile;
	CString m_progressLabel;
	std::atomic<bool> m_nzbCompleted{false};
	Mutex m_volumeMutex;
	ArchiveList m_archives;
	time_t m_extraStartTime = 0;
	ArchiveList m_extractedArchives;
	bool m_allOkMessageReceived = false;
	bool m_unpackOk = false;
	bool m_finalDirCreated = false;
	bool m_processed = false;
	std::atomic<bool> m_cancelled{false};
	std::atomic<bool> m_waitingVolume{false};
	std::atomic<bool> m_unpacking{false};
	std::atomic<std::thread::id> m_execThreadId{};
	void CreateUnpackDir();
	void FindArchiveFiles();
	void ExecuteUnrar(const char* archiveName);
	void WaitNextVolume(const char* filename);
	void Cleanup();
	bool IsMainArchive(const char* filename);
	void SetProgressLabel(NzbInfo* nzbInfo, const char* progressLabel);
	void AddExtraTime(NzbInfo* nzbInfo);
};

#include <iterator>
#include "Util.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

inline void DirectUnpack::StartJob(NzbInfo* nzbInfo)
{
	DirectUnpack* directUnpack = new DirectUnpack();
	directUnpack->m_nzbId = nzbInfo->GetId();
	directUnpack->m_unpacking = false;
	directUnpack->m_waitingVolume = false;
	directUnpack->SetAutoDestroy(true);

	nzbInfo->SetUnpackThread(directUnpack);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsRunning);

	directUnpack->Start();
}

inline void DirectUnpack::Run()
{
	debug("Entering DirectUnpack-loop for %i", m_nzbId);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo)
		{
			debug("Could not find NzbInfo for %i", m_nzbId);
			m_unpacking = false;
			return;
		}

		m_name = nzbInfo->GetName();
		m_destDir = nzbInfo->GetDestDir();
		m_finalDir = nzbInfo->BuildFinalDirName();

		NzbParameter* parameter = nzbInfo->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_password = parameter->GetValue();
		}
	}

	m_infoName.Format("direct unpack for %s", *m_name);
	m_infoNameUp.Format("Direct unpack for %s", *m_name);

	FindArchiveFiles();

	SetInfoName(m_infoName);
	SetWorkingDir(m_destDir);

	while (!IsStopped())
	{
		CString archive;
		{
			Guard guard(m_volumeMutex);
			if (!m_archives.empty())
			{
				archive = std::move(m_archives.front());
				m_archives.pop_front();
			}
		}

		if (archive)
		{
			m_waitingVolume = false;
			if (!m_processed)
			{
				PrintMessage(Message::mkInfo, "Directly unpacking %s", *m_name);
				Cleanup();
				CreateUnpackDir();
				m_processed = true;
			}

			size_t committedArchives = m_extractedArchives.size();
			ExecuteUnrar(archive);

			if (!m_unpackOk)
			{
				m_extractedArchives.resize(committedArchives);
				break;
			}
		}
		else
		{
			if (m_nzbCompleted)
			{
				break;
			}
			m_waitingVolume = true;
			Util::Sleep(100);
		}
	}

	m_unpacking = false;
	m_waitingVolume = false;

	bool interrupted = !m_cancelled && (IsStopped() || GetTerminated());
	if ((!m_unpackOk && !interrupted) || m_cancelled)
	{
		Cleanup();
	}

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo || nzbInfo->GetUnpackThread() != this)
		{
			debug("Could not find NzbInfo for %s", *m_infoName);
			return;
		}

		nzbInfo->SetUnpackThread(nullptr);
		nzbInfo->SetDirectUnpackStatus(
			!m_processed ? NzbInfo::nsNone :
			m_unpackOk && !interrupted && !m_cancelled ? NzbInfo::nsSuccess :
			interrupted ? NzbInfo::nsNone : NzbInfo::nsFailure);

		if (interrupted)
		{
			debug("%s stopped due to reload/shutdown, preserving unpack state", *m_infoName);
		}
		else if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess ||
			nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsFailure)
		{
			bool ok = nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess;
			nzbInfo->AddMessage(ok ? Message::mkInfo : Message::mkWarning,
				BString<1024>("%s %s", *m_infoNameUp, ok ? "successful" : "failed"));
		}

		if (nzbInfo->GetExtractedArchives() &&
			(nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess || interrupted))
		{
			std::move(m_extractedArchives.begin(), m_extractedArchives.end(),
				std::back_inserter(*nzbInfo->GetExtractedArchives()));
		}

		AddExtraTime(nzbInfo);

		if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(false);
		}
	}

	debug("Exiting DirectUnpack-loop for %i", m_nzbId);
}

inline void DirectUnpack::ExecuteUnrar(const char* archiveName)
{
	m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	m_allOkMessageReceived = false;

	Guard unpackGuard(UnpackController::GetUnpackMutex());
	if (IsStopped())
	{
		return;
	}
	m_waitingVolume = false;
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;

	BString<1024> fullArchivePath("%s%c%s", *m_destDir, PATH_SEPARATOR, archiveName);
	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		Message::EKind msgKind = Message::mkDetail;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			case 4: msgKind = Message::mkInfo; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->AddMessage(msgKind, msg);
		while (self->m_waitingVolume && !self->IsStopped() && !self->GetTerminated())
		{
			Util::Sleep(20);
		}
	};

	auto isStoppedCb = [](void* ctx) -> int
	{
		if (!ctx) return 0;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		return (self->IsStopped() || self->GetTerminated()) ? 1 : 0;
	};

	const char* pw = m_password.Empty() ? "" : *m_password;
	const int cleanupDisk = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
	int ok = nzbget_rs_unpack_extract_with_zipfast(
		fullArchivePath,
		m_unpackDir,
		pw,
		1,
		cleanupDisk,
		logCb,
		this,
		isStoppedCb);

	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	m_waitingVolume = false;

	m_unpackOk = ok != 0 && m_allOkMessageReceived && !GetTerminated() && !IsStopped();
}

inline void DirectUnpack::CreateUnpackDir()
{
	bool useInterDir = nzbget_rs_unpack_use_inter_dir(
		m_destDir, g_Options->GetInterDir(), PATH_SEPARATOR) != 0;

	m_finalDirCreated = useInterDir && !FileSystem::DirectoryExists(m_finalDir);

	const char* destDir = useInterDir && !m_finalDir.Empty() ? *m_finalDir : *m_destDir;

	if (g_Options->GetUseTempUnpackDir())
	{
		m_unpackDir.Format("%s%c%s", destDir, PATH_SEPARATOR, "_unpack");
	}
	else
	{
		m_unpackDir = destDir;
	}

	detail("Unpacking into %s", *m_unpackDir);

	CString errmsg;
	if (!FileSystem::ForceDirectories(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not create directory %s: %s", *m_unpackDir, *errmsg);
	}
}

inline void DirectUnpack::FindArchiveFiles()
{
	Guard guard(m_volumeMutex);
	auto addArchiveCb = [](void* ctx, const char* archiveName)
	{
		if (!ctx || !archiveName) return;
		static_cast<DirectUnpack*>(ctx)->m_archives.emplace_back(archiveName);
	};
	nzbget_rs_direct_unpack_find_archive_files(m_destDir, this, addArchiveCb);
}

inline bool DirectUnpack::IsMainArchive(const char* filename)
{
	return nzbget_rs_direct_unpack_is_main_archive(filename) != 0;
}

inline void DirectUnpack::AddMessage(Message::EKind kind, const char* text)
{
	debug("%s", text);

	BString<1024> msgText = text;
	NzbgetRsUnpackMessageInfo info{};
	nzbget_rs_unpack_parse_message(text, m_unpackExtendedDir, 0, &info);
	if (info.strip_extracting_dir != 0)
	{
		msgText.Format("Unrar: Extracting %s", text + info.stripped_suffix_offset);
	}
	if (info.is_insert_disk != 0)
	{
		BString<1024> filename;
		if (info.insert_disk_name_len > 0)
		{
			filename.Set(text + info.insert_disk_name_offset, (int)info.insert_disk_name_len);
		}
		WaitNextVolume(filename);
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (nzbInfo)
	{
		nzbInfo->AddMessage(kind, msgText);
	}
	else
	{
		ScriptController::AddMessage(kind, msgText);
	}

	if (info.has_extracting_progress != 0 && nzbInfo)
	{
		SetProgressLabel(nzbInfo, msgText + 7);
	}

	if (info.has_extracting_from != 0 && nzbInfo)
	{
		SetProgressLabel(nzbInfo, text + 7);
		m_extractedArchives.emplace_back(text + 23);
	}

	if (!IsStopped() && (info.decrypt_error || info.direct_password_error || info.crc_or_volume_error))
	{
		Stop(downloadQueue, nzbInfo);
	}

	if (info.all_ok != 0)
	{
		m_allOkMessageReceived = true;
	}
}

inline void DirectUnpack::Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Stopping %s", *m_infoName);
	if (!(g_PrePostProcessor && g_PrePostProcessor->IsStopped()))
	{
		m_cancelled = true;
	}
	if (m_processed)
	{
		if (nzbInfo)
		{
			nzbInfo->AddMessage(Message::mkWarning, BString<1024>("Cancelling %s", *m_infoName));
		}
		else
		{
			warn("Cancelling %s", *m_infoName);
		}
	}
	if (nzbInfo)
	{
		AddExtraTime(nzbInfo);
		if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(false);
		}
	}
	Thread::Stop();
	bool isExecThread = std::this_thread::get_id() == m_execThreadId.load();
	if (m_unpacking)
	{
		if (!isExecThread && !m_cancelled && HasActiveProcess() && Break())
		{
			for (int i = 0; i < 50 && m_unpacking && HasActiveProcess(); ++i)
			{
				Util::Sleep(100);
			}
		}
		if (m_unpacking)
		{
			Terminate();
		}
	}
}

inline void DirectUnpack::WaitNextVolume(const char* filename)
{
	debug("WaitNextVolume for %s", filename);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo && (strcmp(m_destDir, nzbInfo->GetDestDir()) ||
			strcmp(m_finalDir, nzbInfo->BuildFinalDirName())))
		{
			nzbInfo->AddMessage(Message::mkWarning,
				BString<1024>("Stopping direct unpack for %s due to changed destination directory", nzbInfo->GetName()));
			Stop(downloadQueue, nzbInfo);
		}
	}

	BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);
	if (FileSystem::FileExists(fullFilename))
	{
		m_waitingVolume = false;
	}
	else
	{
		bool nzbCompleted;
		{
			Guard guard(m_volumeMutex);
			m_waitingFile = filename;
			m_waitingVolume = true;
			nzbCompleted = m_nzbCompleted;
		}
		if (nzbCompleted)
		{
			PrintMessage(Message::mkWarning, "Could not find volume %s", filename);
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
			Stop(downloadQueue, nzbInfo);
		}
	}
}

inline void DirectUnpack::FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	debug("FileDownloaded for %s/%s", fileInfo->GetNzbInfo()->GetName(), fileInfo->GetFilename());

	if (!CanProceed(fileInfo))
	{
		Stop(downloadQueue, fileInfo->GetNzbInfo());
		return;
	}

	Guard guard(m_volumeMutex);
	if (m_waitingFile && !strcasecmp(m_waitingFile, fileInfo->GetFilename()))
	{
		m_waitingFile = nullptr;
		m_waitingVolume = false;
	}

	if (IsMainArchive(fileInfo->GetFilename()))
	{
		m_archives.emplace_back(fileInfo->GetFilename());
	}
}

inline bool DirectUnpack::CanProceed(FileInfo* fileInfo) const
{
	return !IsArchiveFilename(fileInfo->GetFilename()) || fileInfo->GetFailedArticles() == 0;
}

inline void DirectUnpack::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDownloaded for %s", nzbInfo->GetName());

	Guard guard(m_volumeMutex);
	m_nzbCompleted = true;
	if (m_waitingFile)
	{
		nzbInfo->AddMessage(Message::mkWarning,
			BString<1024>("Could not find volume %s", *m_waitingFile));
		Stop(downloadQueue, nzbInfo);
		return;
	}

	m_extraStartTime = Util::CurrentTime();

	if (nzbInfo->GetPostInfo())
	{
		nzbInfo->GetPostInfo()->SetProgressLabel(m_progressLabel);
	}
}

inline void DirectUnpack::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDeleted for %s", nzbInfo->GetName());

	nzbInfo->SetUnpackThread(nullptr);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsFailure);
	Stop(downloadQueue, nzbInfo);
}

inline void DirectUnpack::Cleanup()
{
	debug("Cleanup for %s", *m_infoName);

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		self->PrintMessage(kind == 1 ? Message::mkError : Message::mkInfo, "%s", msg);
	};

	nzbget_rs_direct_unpack_cleanup(
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		m_unpackDir,
		m_finalDirCreated ? 1 : 0,
		m_finalDir,
		this,
		logCb);
}

inline void DirectUnpack::SetProgressLabel(NzbInfo* nzbInfo, const char* progressLabel)
{
	m_progressLabel = progressLabel;
	if (nzbInfo->GetPostInfo())
	{
		nzbInfo->GetPostInfo()->SetProgressLabel(progressLabel);
	}
}

inline void DirectUnpack::AddExtraTime(NzbInfo* nzbInfo)
{
	if (m_extraStartTime)
	{
		int extraSec = (int)(Util::CurrentTime() - m_extraStartTime);
		nzbInfo->SetUnpackSec(nzbInfo->GetUnpackSec() + extraSec);
		nzbInfo->SetPostTotalSec(nzbInfo->GetPostTotalSec() + extraSec);
		m_extraStartTime = 0;
	}
}

inline bool DirectUnpack::IsArchiveFilename(const char* filename)
{
	if (!filename)
	{
		return false;
	}

	return nzbget_rs_file_type(22, filename, strlen(filename)) != 0;
}

#endif

#endif

#include "DiskState.h"
#include "ServerPool.h"
#include "DiskState.h"
#include "nzbget_rs.h"

#ifndef DISABLE_PARCHECK
inline bool RepairController::PostParChecker::RequestMorePars(int blockNeeded, int* blockFound)
{
	return m_owner->RequestMorePars(m_postInfo->GetNzbInfo(), GetParFilename(), blockNeeded, blockFound);
}

inline void RepairController::PostParChecker::UpdateProgress()
{
	m_owner->UpdateParCheckProgress();
}

inline void RepairController::PostParChecker::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, 1024, format, args);
	va_end(args);
	text[1024-1] = '\0';

	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}

inline void RepairController::PostParChecker::RegisterParredFile(const char* filename)
{
	m_postInfo->GetParredFiles()->push_back(filename);
}

inline void RepairController::PostParChecker::RegisterVerifiedFile(const char* filename)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
	StreamRepairJobList* jobs = nzbInfo->GetStreamRepairJobs();
	std::string verifiedPath = FileSystem::GetRealPath(filename).value_or(filename);
	size_t previousSize = jobs->size();
	jobs->erase(std::remove_if(jobs->begin(), jobs->end(),
		[nzbInfo, &verifiedPath](const StreamRepairJob& job)
		{
			std::string currentName = job.GetFilename();
			for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
			{
				if (completedFile.GetId() == job.GetFileId())
				{
					currentName = completedFile.GetFilename();
					break;
				}
			}
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, currentName.c_str());
			std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
			nzbget_rs_free(pathBuf);
			path = FileSystem::GetRealPath(path).value_or(path);
			return FileSystem::SameFilename(path.c_str(), verifiedPath.c_str());
		}), jobs->end());
	if (jobs->size() != previousSize)
	{
		nzbInfo->SetChanged(true);
	}
}

inline bool RepairController::PostParChecker::IsParredFile(const char* filename)
{
	for (CString& parredFile : m_postInfo->GetParredFiles())
	{
		if (nzbget_rs_repair_filename_eq_ignore_case(parredFile, filename) != 0)
		{
			return true;
		}
	}
	return false;
}

inline ParChecker::EFileStatus RepairController::PostParChecker::FindFileCrc(const char* filename,
	uint32* crc, SegmentList* segments)
{
	int id = 0;
	CompletedFile::EStatus status;
	bool reprocess;
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		CompletedFile* completedFile = nullptr;
		for (CompletedFile& completedFile2 : *m_postInfo->GetNzbInfo()->GetCompletedFiles())
		{
			if (nzbget_rs_repair_filename_eq_ignore_case(completedFile2.GetFilename(), filename) != 0)
			{
				completedFile = &completedFile2;
				break;
			}
		}
		if (!completedFile)
		{
			return ParChecker::fsUnknown;
		}
		id = completedFile->GetId();
		*crc = completedFile->GetCrc();
		status = completedFile->GetStatus();
		reprocess = m_postInfo->GetNzbInfo()->GetReprocess();
	}

	debug("Found completed file: %s, CRC: %.8x, Status: %i", filename, *crc, (int)status);

	if (nzbget_rs_repair_should_load_partial_segments(
		(int)status, (int)CompletedFile::cfPartial, id, reprocess ? 1 : 0) != 0)
	{
		FileInfo tmpFileInfo(id);

		if (!g_DiskState->LoadFileState(&tmpFileInfo, nullptr, true))
		{
			return ParChecker::fsUnknown;
		}

		for (ArticleInfo* pa : tmpFileInfo.GetArticles())
		{
			segments->emplace_back(pa->GetStatus() == ArticleInfo::aiFinished,
				pa->GetSegmentOffset(), pa->GetSegmentSize(), pa->GetCrc());
		}
	}

	return (ParChecker::EFileStatus)nzbget_rs_repair_classify_file_crc_status(
		(int)status, reprocess ? 1 : 0, !segments->empty() ? 1 : 0,
		(int)CompletedFile::cfSuccess, (int)CompletedFile::cfFailure, (int)CompletedFile::cfPartial,
		(int)ParChecker::fsSuccess, (int)ParChecker::fsFailure, (int)ParChecker::fsPartial,
		(int)ParChecker::fsUnknown);
}

inline const char* RepairController::PostParChecker::FindFileOrigname(const char* filename)
{
	thread_local std::string origname;
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	for (CompletedFile& completedFile : *m_postInfo->GetNzbInfo()->GetCompletedFiles())
	{
		if (nzbget_rs_repair_filename_eq_ignore_case(completedFile.GetFilename(), filename) != 0)
		{
			if (!completedFile.GetOrigname())
			{
				return nullptr;
			}
			origname = completedFile.GetOrigname();
			return origname.c_str();
		}
	}

	return nullptr;
}

inline void RepairController::PostParChecker::RequestDupeSources(DupeSourceList* dupeSourceList)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	RawNzbList dupeList = g_DupeCoordinator->ListHistoryDupes(downloadQueue, m_postInfo->GetNzbInfo());

	if (!dupeList.empty())
	{
		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		std::vector<const char*> dupeDirs(dupeList.size());
		for (size_t i = 0; i < dupeList.size(); i++)
		{
			dupeDirs[i] = dupeList[i]->GetDestDir();
		}
		std::vector<size_t> usableIndices(dupeList.size());
		size_t usableCount = nzbget_rs_dupematcher_filter_dupe_sources(
			nzbInfo->GetName(),
			nzbInfo->GetDestDir(),
			nzbInfo->GetSize() - nzbInfo->GetParSize(),
			g_Options->GetUnrarCmd(),
			dupeDirs.data(),
			dupeDirs.size(),
			usableIndices.data(),
			[](void* ctx, int kind, const char* msg)
			{
				auto* self = static_cast<PostParChecker*>(ctx);
				self->PrintMessage(kind == 3 ? Message::mkDetail : Message::mkInfo, "%s", msg ? msg : "");
			},
			this);
		for (size_t i = 0; i < usableCount; i++)
		{
			NzbInfo* dupeNzbInfo = dupeList[usableIndices[i]];
			dupeSourceList->emplace_back(dupeNzbInfo->GetId(), dupeNzbInfo->GetDestDir());
		}
	}
}

inline std::string RepairController::PostParChecker::FetchTwinFile(const TwinCheck::NzbEntry& entry, std::vector<char>* keep)
{
	std::vector<char> data;
	if (!TwinCheck::FetchEntry(m_twinFetcher, entry, 1024LL * 1024 * 1024, [this]() { return IsStopped(); }, data))
	{
		return "";
	}
	NzbgetRsBuf nameBuf = nzbget_rs_repair_build_twin_filename(
		entry.filename.c_str(), m_twinFiles.size());
	std::string name(nameBuf.data ? nameBuf.data : "", nameBuf.len);
	nzbget_rs_free(nameBuf);
	NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		GetDestDir(), PATH_SEPARATOR, name.c_str());
	BString<1024> path;
	path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
	nzbget_rs_free(pathBuf);
	if (!FileSystem::SaveBufferIntoFile(*path, data.empty() ? "" : data.data(), data.size()))
	{
		return "";
	}
	m_twinFiles.push_back(*path);
	if (keep)
	{
		*keep = std::move(data);
	}
	return *path;
}

inline std::string RepairController::PostParChecker::RequestTwinIndex()
{
	std::vector<TwinCheck::FileSig> own;
	{
		CharBuffer parBuf;
		if (FileSystem::LoadFileIntoBuffer(GetParFilename(), parBuf, false))
		{
			own = TwinCheck::ParsePar2(parBuf, parBuf.Size());
		}
	}
	std::string ownPrint = TwinCheck::Fingerprint(own);
	if (ownPrint.empty())
	{
		return "";
	}

	struct Candidate
	{
		std::string name;
		std::string nzbFilename;
		bool known;
	};
	std::vector<Candidate> candidates;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		auto consider = [&](NzbInfo* other)
			{
				bool sameNameOrKey = other != nzbInfo && other->GetKind() == NzbInfo::nkNzb &&
					DupeCoordinator::SameNameOrKey(other->GetName(), other->GetDupeKey(),
						nzbInfo->GetName(), nzbInfo->GetDupeKey());
				NzbParameter* print = sameNameOrKey ? other->GetParameters()->Find(TwinCheck::FilesParam) : nullptr;
				if (!nzbget_rs_repair_should_consider_twin_candidate(
					other == nzbInfo ? 1 : 0,
					(int)other->GetKind(),
					(int)NzbInfo::nkNzb,
					sameNameOrKey ? 1 : 0,
					other->GetQueuedFilename(),
					print != nullptr ? 1 : 0,
					(print && ownPrint == print->GetValue()) ? 1 : 0) ||
					!FileSystem::FileExists(other->GetQueuedFilename()))
				{
					return;
				}
				candidates.push_back({other->GetName(), other->GetQueuedFilename(), print != nullptr});
			};
		for (NzbInfo* queued : downloadQueue->GetQueue())
		{
			consider(queued);
		}
		for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				consider(historyInfo->GetNzbInfo());
			}
		}
	}
	std::stable_sort(candidates.begin(), candidates.end(),
		[](const Candidate& a, const Candidate& b) { return a.known > b.known; });

	std::vector<const char*> ownNames;
	std::vector<const char*> ownMd5s;
	ownNames.reserve(own.size());
	ownMd5s.reserve(own.size());
	for (const TwinCheck::FileSig& sig : own)
	{
		ownNames.push_back(sig.name.c_str());
		ownMd5s.push_back(sig.md5.c_str());
	}

	for (const Candidate& candidate : candidates)
	{
		if (IsStopped())
		{
			break;
		}
		std::vector<TwinCheck::NzbEntry> entries = TwinCheck::ReadNzbEntries(candidate.nzbFilename.c_str());
		std::vector<uint8_t> entryPar2(entries.size());
		std::vector<int64_t> entrySizes(entries.size());
		for (size_t i = 0; i < entries.size(); i++)
		{
			entryPar2[i] = entries[i].par2 ? 1 : 0;
			entrySizes[i] = entries[i].size;
		}
		ptrdiff_t bestIdx = nzbget_rs_repair_select_best_twin_par2_index(
			entryPar2.data(), entrySizes.data(), entries.size(), TwinCheck::MaxIndexSize);
		if (bestIdx < 0 || (size_t)bestIdx >= entries.size())
		{
			continue;
		}
		const TwinCheck::NzbEntry* index = &entries[(size_t)bestIdx];

		std::vector<char> data;
		std::string path = FetchTwinFile(*index, &data);
		if (path.empty())
		{
			continue;
		}
		int cmp = nzbget_rs_twin_compare_par2_data(
			ownNames.data(), ownMd5s.data(), own.size(),
			ownPrint.c_str(),
			reinterpret_cast<const uint8_t*>(data.data()), data.size());
		if (cmp != 0)
		{
			PrintMessage(Message::mkDetail, "%s: its par2 set %s", candidate.name.c_str(),
				nzbget_rs_repair_twin_par2_reject_reason(cmp));
			FileSystem::DeleteFile(path.c_str());
			m_twinFiles.pop_back();
			continue;
		}

		m_twinName = candidate.name;
		m_twinEntries = std::move(entries);
		m_twinIndex = index->filename;
		m_twinBlockSize = TwinCheck::BlockSize(data.data(), data.size());
		PrintMessage(Message::mkInfo, "Trying the par2 set of twin %s (blocks of %.1f MB)",
			m_twinName.c_str(), nzbget_rs_dqh_stream_repair_bytes_to_mb(m_twinBlockSize));
		return path;
	}
	return "";
}

inline std::vector<std::string> RepairController::PostParChecker::RequestTwinVolumes(int blockNeeded)
{
	struct Volume
	{
		const TwinCheck::NzbEntry* entry;
		int blocks;
	};
	std::vector<Volume> volumes;
	for (const TwinCheck::NzbEntry& entry : m_twinEntries)
	{
		if (nzbget_rs_repair_should_skip_twin_volume_entry(
			entry.par2 ? 1 : 0, entry.filename.c_str(), m_twinIndex.c_str()) != 0)
		{
			continue;
		}
		int blocks = TwinCheck::VolumeBlocks(entry.filename);
		if (blocks < 0 && m_twinBlockSize > 0)
		{
			blocks = nzbget_rs_repair_estimate_twin_vol_blocks(entry.size, m_twinBlockSize);
		}
		if (blocks > 0)
		{
			volumes.push_back({&entry, blocks});
		}
	}
	std::sort(volumes.begin(), volumes.end(), [](const Volume& a, const Volume& b) { return a.blocks < b.blocks; });

	int available = 0;
	std::vector<int> allVolBlocks;
	allVolBlocks.reserve(volumes.size());
	for (const Volume& volume : volumes)
	{
		allVolBlocks.push_back(volume.blocks);
	}
	bool hasEnoughTwinBlocks = nzbget_rs_repair_sum_and_check_twin_blocks(
		allVolBlocks.data(), allVolBlocks.size(), blockNeeded, &available) != 0;
	std::vector<std::string> paths;
	if (!hasEnoughTwinBlocks)
	{
		PrintMessage(Message::mkInfo, "Twin %s holds %i par-block(s), %i needed: not fetched",
			m_twinName.c_str(), available, blockNeeded);
		return paths;
	}

	int left = blockNeeded;
	while (left > 0 && !volumes.empty() && !IsStopped())
	{
		std::vector<int> volBlocks;
		volBlocks.reserve(volumes.size());
		for (const Volume& v : volumes)
		{
			volBlocks.push_back(v.blocks);
		}
		ptrdiff_t idx = nzbget_rs_repair_pick_twin_volume(volBlocks.data(), volBlocks.size(), left);
		if (idx < 0 || (size_t)idx >= volumes.size())
		{
			break;
		}
		auto it = volumes.begin() + idx;
		PrintMessage(Message::mkInfo, "Fetching %s of twin %s for par-repair (%i block(s))",
			it->entry->filename.c_str(), m_twinName.c_str(), it->blocks);
		std::string path = FetchTwinFile(*it->entry);
		if (!path.empty())
		{
			paths.push_back(path);
			left -= it->blocks;
		}
		volumes.erase(it);
	}
	return paths;
}

inline void RepairController::PostParChecker::DeleteTwinFiles()
{
	for (const std::string& path : m_twinFiles)
	{
		FileSystem::DeleteFile(path.c_str());
	}
	m_twinFiles.clear();
	m_twinEntries.clear();
	m_twinName.clear();
	m_twinIndex.clear();
	m_twinBlockSize = 0;
}

inline void RepairController::PostParChecker::StatDupeSources(DupeSourceList* dupeSourceList)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	int totalExtraParBlocks = 0;
	for (DupeSource& dupeSource : dupeSourceList)
	{
		if (dupeSource.GetUsedBlocks() > 0)
		{
			for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
			{
				if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
					historyInfo->GetNzbInfo()->GetId() == dupeSource.GetId())
				{
					historyInfo->GetNzbInfo()->SetExtraParBlocks(
						historyInfo->GetNzbInfo()->GetExtraParBlocks() - dupeSource.GetUsedBlocks());
				}
			}
		}
		totalExtraParBlocks += dupeSource.GetUsedBlocks();
	}

	m_postInfo->GetNzbInfo()->SetExtraParBlocks(m_postInfo->GetNzbInfo()->GetExtraParBlocks() + totalExtraParBlocks);
}

#endif

inline RepairController::RepairController()
{
	debug("Creating RepairController");

#ifndef DISABLE_PARCHECK
	m_parChecker.m_owner = this;
#endif
}

inline void RepairController::Stop()
{
	debug("Stopping RepairController");
	Thread::Stop();
#ifndef DISABLE_PARCHECK
	m_parChecker.Cancel();
#endif
}

#ifndef DISABLE_PARCHECK

inline void RepairController::StartJob(PostInfo* postInfo)
{
	RepairController* repairController = new RepairController();
	repairController->m_postInfo = postInfo;
	repairController->SetAutoDestroy(false);

	postInfo->SetPostThread(repairController);

	repairController->Start();
}

inline void RepairController::Run()
{
	BString<1024> nzbName;
	CString destDir;
	bool dupeRepaired = false;
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		nzbName = m_postInfo->GetNzbInfo()->GetName();
		destDir = m_postInfo->GetNzbInfo()->GetDestDir();
		dupeRepaired = m_postInfo->GetNzbInfo()->GetDupeRecoveredBytes() > 0;
	}

	m_parChecker.SetPostInfo(m_postInfo);
	m_parChecker.SetDestDir(destDir);
	m_parChecker.SetNzbName(nzbName);
	m_parChecker.SetParTime(Util::CurrentTime());
	m_parChecker.SetDownloadSec(m_postInfo->GetNzbInfo()->GetDownloadSec());
	m_parChecker.SetParQuick(nzbget_rs_repair_should_use_par_quick(
		g_Options->GetParQuick() ? 1 : 0,
		m_postInfo->GetForceParFull() ? 1 : 0,
		dupeRepaired ? 1 : 0) != 0);
	m_parChecker.SetForceRepair(m_postInfo->GetForceRepair());

	m_parChecker.PrintMessage(Message::mkInfo, "Checking pars for %s", *nzbName);

	m_parChecker.Execute();
}

inline bool RepairController::AddPar(FileInfo* fileInfo, bool deleted)
{
	bool sameCollection = fileInfo->GetNzbInfo() == m_parChecker.GetPostInfo()->GetNzbInfo();
	if (sameCollection && !deleted)
	{
		NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
		BString<1024> fullFilename;
		fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
		nzbget_rs_free(fullFnBuf);
		m_parChecker.AddParFile(fullFilename);
	}
	else
	{
		m_parChecker.QueueChanged();
	}
	return sameCollection;
}

inline void RepairController::ParCheckCompleted()
{
	m_parChecker.DeleteTwinFiles();
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	PostInfo* postInfo = m_parChecker.GetPostInfo();

	int rawParStatus = 0, setParRepaired = 0, parRepairedVal = 0, waitTime = 0, parSec = 0;
	nzbget_rs_repair_complete_par_check(
		(int)m_parChecker.GetStatus(),
		(int)postInfo->GetNzbInfo()->GetParStatus(),
		(int)ParChecker::psRepaired, (int)ParChecker::psRepairNotNeeded, (int)ParChecker::psRepairPossible,
		(int)NzbInfo::psSkipped, (int)NzbInfo::psSuccess, (int)NzbInfo::psRepairPossible, (int)NzbInfo::psFailure,
		postInfo->GetNzbInfo()->GetDownloadSec(),
		m_parChecker.GetDownloadSec(),
		(int64_t)Util::CurrentTime(),
		(int64_t)m_parChecker.GetParTime(),
		&rawParStatus, &setParRepaired, &parRepairedVal, &waitTime, &parSec);
	postInfo->GetNzbInfo()->SetParStatus((NzbInfo::EParStatus)rawParStatus);
	if (setParRepaired)
	{
		postInfo->SetParRepaired(parRepairedVal != 0);
	}
	postInfo->SetStartTime(postInfo->GetStartTime() + (time_t)waitTime);
	postInfo->GetNzbInfo()->SetParSec(postInfo->GetNzbInfo()->GetParSec() + parSec);

	postInfo->GetNzbInfo()->SetParFull(m_parChecker.GetParFull());

	postInfo->SetWorking(false);
}

inline bool RepairController::RequestMorePars(NzbInfo* nzbInfo, const char* parFilename, int blockNeeded, int* blockFoundOut)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	Blocks availableBlocks;
	Blocks selectedBlocks;
	int blockFound = 0;
	int curBlockFound = 0;

	FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, true, true, &curBlockFound);
	blockFound += curBlockFound;
	if (blockFound < blockNeeded)
	{
		FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, true, false, &curBlockFound);
		blockFound += curBlockFound;
	}
	if (blockFound < blockNeeded)
	{
		FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, false, false, &curBlockFound);
		blockFound += curBlockFound;
	}

	std::sort(availableBlocks.begin(), availableBlocks.end(),
		[](const BlockInfo& block1, const BlockInfo& block2)
		{
			return block1.m_blockCount < block2.m_blockCount;
		});

	if (blockFound >= blockNeeded)
	{
		std::vector<int> sortedCounts;
		sortedCounts.reserve(availableBlocks.size());
		for (const BlockInfo& b : availableBlocks)
		{
			sortedCounts.push_back(b.m_blockCount);
		}
		std::vector<size_t> selectedIdx(availableBlocks.size());
		int remNeeded = blockNeeded;
		size_t selCount = nzbget_rs_repair_select_more_pars(
			sortedCounts.data(), sortedCounts.size(), blockNeeded,
			selectedIdx.data(), selectedIdx.size(), &remNeeded);
		blockNeeded = remNeeded;
		for (size_t i = 0; i < selCount && i < selectedIdx.size(); ++i)
		{
			selectedBlocks.push_back(availableBlocks[selectedIdx[i]]);
		}

		for (BlockInfo& blockInfo : selectedBlocks)
		{
			if (blockInfo.m_fileInfo->GetPaused())
			{
				m_parChecker.PrintMessage(Message::mkInfo, "Unpausing %s%c%s for par-recovery", nzbInfo->GetName(), PATH_SEPARATOR, blockInfo.m_fileInfo->GetFilename());
				blockInfo.m_fileInfo->SetPaused(false);
				blockInfo.m_fileInfo->SetExtraPriority(true);
			}
		}
	}

	bool hasUnpausedParFiles = false;
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (fileInfo->GetParFile() && !fileInfo->GetPaused())
		{
			hasUnpausedParFiles = true;
			break;
		}
	}

	if (blockFoundOut)
	{
		*blockFoundOut = blockFound;
	}

	return blockNeeded <= 0 || hasUnpausedParFiles;
}

inline void RepairController::FindPars(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* parFilename,
	Blocks& blocks, bool strictParName, bool exactParName, int* blockFound)
{
	*blockFound = 0;

	char* baseParFilename = FileSystem::BaseFileName(parFilename);
	int mainBaseLen = 0;
	if (!ParParser::ParseParFilename(baseParFilename, true, &mainBaseLen, nullptr))
	{
		nzbInfo->PrintMessage(Message::mkError, "Internal error: could not parse filename %s", baseParFilename);
		return;
	}
	BString<1024> mainBaseFilename;
	if (mainBaseLen > 0)
	{
		mainBaseFilename.Set(baseParFilename, mainBaseLen);
	}

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		int parsedBlocks = 0;
		bool parsed = ParParser::ParseParFilename(fileInfo->GetFilename(), fileInfo->GetFilenameConfirmed(), nullptr, &parsedBlocks);
		bool sameCollection = !exactParName ||
			ParParser::SameParCollection(fileInfo->GetFilename(),
				FileSystem::BaseFileName(parFilename), fileInfo->GetFilenameConfirmed());
		int blockCount = 0;
		if (nzbget_rs_repair_eval_find_pars_candidate(
			sameCollection ? 1 : 0,
			0,
			(!exactParName && strictParName) ? 1 : 0,
			fileInfo->GetFilename(),
			mainBaseFilename,
			parsed ? 1 : 0,
			parsedBlocks,
			fileInfo->GetSize(),
			m_parChecker.GetBlockSize(),
			&blockCount))
		{
			bool alreadyAdded = false;
			for (BlockInfo& blockInfo : blocks)
			{
				if (blockInfo.m_fileInfo == fileInfo)
				{
					alreadyAdded = true;
					break;
				}
			}

			if (!alreadyAdded)
			{
				blocks.emplace_back(fileInfo, blockCount);
				*blockFound += blockCount;
			}
		}
	}
}

inline void RepairController::UpdateParCheckProgress()
{
	PostInfo* postInfo;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		postInfo = m_parChecker.GetPostInfo();
		if (m_parChecker.GetFileProgress() == 0)
		{
			postInfo->SetProgressLabel(m_parChecker.GetProgressLabel());
		}
		postInfo->SetFileProgress(m_parChecker.GetFileProgress());
		postInfo->SetStageProgress(m_parChecker.GetStageProgress());
		time_t current = Util::CurrentTime();

		int rawStage = (int)postInfo->GetStage();
		int setRepairTime = 0;
		int newRepairSec = postInfo->GetNzbInfo()->GetRepairSec();
		if (nzbget_rs_repair_eval_par_progress_update(
			(int)postInfo->GetStage(),
			(int)m_parChecker.GetStage(),
			(int)PostInfo::ptLoadingPars,
			(int)PostInfo::ptVerifyingSources,
			(int)PostInfo::ptRepairing,
			(int)PostInfo::ptVerifyingRepaired,
			(int64)current,
			(int64)m_parChecker.GetRepairTime(),
			newRepairSec,
			&rawStage,
			&setRepairTime,
			&newRepairSec) != 0)
		{
			postInfo->SetStage((PostInfo::EStage)rawStage);
			postInfo->SetStageTime(current);
			if (setRepairTime)
			{
				m_parChecker.SetRepairTime(current);
			}
			else if (postInfo->GetStage() == PostInfo::ptVerifyingRepaired)
			{
				postInfo->GetNzbInfo()->SetRepairSec(newRepairSec);
			}
		}

		bool parCancel = false;
		if (!IsStopped())
		{
			int estimatedRepairTime = 0;
			if (nzbget_rs_repair_check_par_time_limit(
				g_Options->GetParTimeLimit(),
				m_parChecker.GetStage() == PostParChecker::ptRepairing ? 1 : 0,
				(int64)(current - postInfo->GetStageTime()),
				postInfo->GetStageProgress(),
				&estimatedRepairTime) != 0)
			{
				debug("Estimated repair time %i seconds", estimatedRepairTime);
				m_parChecker.PrintMessage(Message::mkWarning, "Cancelling par-repair for %s, estimated repair time (%i minutes) exceeds allowed repair time",
					m_parChecker.GetInfoName(),
					estimatedRepairTime / 60);
				parCancel = true;
			}
		}

		if (parCancel)
		{
			Stop();
		}
	}

	CheckPauseState(postInfo);
}

inline void RepairController::CheckPauseState(PostInfo* postInfo)
{
	if (g_WorkState->GetPausePostProcess() && !postInfo->GetNzbInfo()->GetForcePriority())
	{
		time_t stageTime = postInfo->GetStageTime();
		time_t startTime = postInfo->GetStartTime();
		time_t parTime = m_parChecker.GetParTime();
		time_t repairTime = m_parChecker.GetRepairTime();
		time_t waitTime = Util::CurrentTime();

		while (g_WorkState->GetPausePostProcess() &&
			!postInfo->GetNzbInfo()->GetForcePriority() &&
			!IsStopped())
		{
			Util::Sleep(50);

			int64 newStage = 0, newStart = 0, newPar = 0, newRepair = 0;
			nzbget_rs_repair_calc_paused_timestamps(
				(int64)Util::CurrentTime(), (int64)waitTime,
				(int64)stageTime, (int64)startTime, (int64)parTime, (int64)repairTime,
				&newStage, &newStart, &newPar, &newRepair);
			if (stageTime > 0)
			{
				postInfo->SetStageTime((time_t)newStage);
			}
			if (startTime > 0)
			{
				postInfo->SetStartTime((time_t)newStart);
			}
			if (parTime > 0)
			{
				m_parChecker.SetParTime((time_t)newPar);
			}
			if (repairTime > 0)
			{
				m_parChecker.SetRepairTime((time_t)newRepair);
			}
		}
	}
}

#endif

namespace
{
	template <typename Ctx, typename R, typename F>
	R RepairMapCall(void* rawCtx, R failed, F f) noexcept
	{
		Ctx* c = static_cast<Ctx*>(rawCtx);
		if (c->Error)
		{
			return failed;
		}
		try
		{
			return f(c);
		}
		catch (...)
		{
			c->Error = std::current_exception();
			return failed;
		}
	}

	template <typename Ctx, typename F>
	void RepairMapCallVoid(void* rawCtx, F f) noexcept
	{
		Ctx* c = static_cast<Ctx*>(rawCtx);
		if (c->Error)
		{
			return;
		}
		try
		{
			f(c);
		}
		catch (...)
		{
			c->Error = std::current_exception();
		}
	}
}

inline void StreamRepairController::StartJob(PostInfo* postInfo)
{
	StreamRepairController* streamRepairController = new StreamRepairController();
	streamRepairController->m_postInfo = postInfo;
	streamRepairController->SetAutoDestroy(false);

	postInfo->SetPostThread(streamRepairController);

	streamRepairController->Start();
}

inline void StreamRepairController::StartLive(NzbInfo* nzbInfo)
{
	if (DupeArticleFallback::ShouldDeferToPar(nzbInfo))
	{
		return;
	}

	StreamRepairController* streamRepairController = new StreamRepairController();
	streamRepairController->m_liveMode = true;
	streamRepairController->m_nzbId = nzbInfo->GetId();
	streamRepairController->SetAutoDestroy(true);

	nzbInfo->SetLiveRepairThread(streamRepairController);

	streamRepairController->Start();
}

inline void StreamRepairController::Run()
{
	if (m_liveMode)
	{
		RunLive();
		return;
	}

	BString<1024> nzbName;
	CString destDir;
	std::vector<RepairTarget> targets;
	std::vector<DonorSource> donors;
	std::vector<CString> memberNames;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		nzbName = nzbInfo->GetName();
		destDir = nzbInfo->GetDestDir();
		CollectTargets(nzbInfo, targets, memberNames);
		CollectDonors(downloadQueue, nzbInfo, donors);
		m_postInfo->SetProgressLabel("Repairing files from duplicates");
	}

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("stream repair", *nzbName);
	BString<1024> infoName;
	infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	if (targets.empty())
	{
		PrintMessage(Message::mkInfo, "Nothing to stream-repair for %s", *nzbName);
	}
	else if (donors.empty())
	{
		PrintMessage(Message::mkInfo,
			"No duplicate collections found for stream repair of %s", *nzbName);
	}
	else
	{
		StartWatchdog(targets);
		ComputePositionalRanks(destDir, targets, memberNames);
		ExecRepair(destDir, targets, donors, memberNames);

		bool anyHoles = false;
		for (RepairTarget& target : targets)
		{
			anyHoles |= target.NeedsRepair();
		}
		if (anyHoles && !IsStopped())
		{
			ExecCrossPackRepair(destDir, targets, donors, memberNames);
		}
		StopWatchdog(nzbName, targets);
	}

	ReportRemainingHoles(targets);
	m_targets = std::move(targets);
	RepairCompleted();
}

inline void StreamRepairController::RunLive()
{
	BString<1024> nzbName;
	CString destDir;
	std::vector<RepairTarget> targets;
	std::vector<DonorSource> donors;
	std::vector<CString> memberNames;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo || nzbInfo->GetLiveRepairThread() != this)
		{
			return;
		}
		nzbName = nzbInfo->GetName();
		destDir = nzbInfo->GetDestDir();
		CollectTargets(nzbInfo, targets, memberNames);
		CollectDonors(downloadQueue, nzbInfo, donors);

		for (RepairTarget& target : targets)
		{
			for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
			{
				if (job.GetFileId() == target.FileId)
				{
					target.PatchEligible = !job.GetLiveAttempted();
					job.SetLiveAttempted(true);
					break;
				}
			}
		}
	}

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("live stream repair", *nzbName);
	BString<1024> infoName;
	infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	bool anyEligible = false;
	for (RepairTarget& target : targets)
	{
		anyEligible |= target.PatchEligible && target.NeedsRepair();
	}

	if (anyEligible && !donors.empty())
	{
		PrintMessage(Message::mkInfo, "Starting live stream repair for %s", *nzbName);

		StartWatchdog(targets);
		ComputePositionalRanks(destDir, targets, memberNames);
		ExecRepair(destDir, targets, donors, memberNames);

		bool anyHoles = false;
		for (RepairTarget& target : targets)
		{
			anyHoles |= target.PatchEligible && target.NeedsRepair();
		}
		if (anyHoles && !IsStopped())
		{
			RefreshLiveNames(targets, memberNames);
			ExecCrossPackRepair(destDir, targets, donors, memberNames);
		}
		StopWatchdog(nzbName, targets);
	}

	RepairCompletedLive(targets);
}

inline void StreamRepairController::RefreshLiveNames(std::vector<RepairTarget>& targets,
	std::vector<CString>& memberNames)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (!nzbInfo || nzbInfo->GetLiveRepairThread() != this)
	{
		return;
	}

	for (RepairTarget& target : targets)
	{
		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetId() == target.FileId)
			{
				target.Filename = completedFile.GetFilename();
				target.IsParFile = target.IsParFile || completedFile.GetParFile();
				break;
			}
		}
	}

	memberNames.clear();
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		memberNames.emplace_back(completedFile.GetFilename());
	}
}

inline void StreamRepairController::RefreshLiveTargetName(RepairTarget& target)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (!nzbInfo || nzbInfo->GetLiveRepairThread() != this)
	{
		return;
	}

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetId() == target.FileId)
		{
			target.Filename = completedFile.GetFilename();
			break;
		}
	}
}

inline void StreamRepairController::RepairCompletedLive(std::vector<RepairTarget>& targets)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (!nzbInfo)
	{
		return;
	}
	if (nzbInfo->GetLiveRepairThread() != this)
	{
		nzbInfo->SetDupeRecoveredBytes(nzbInfo->GetDupeRecoveredBytes() + m_recoveredBytes);
		return;
	}

	if (m_recoveredBytes > 0 || m_recoveredHoles > 0)
	{
		nzbInfo->SetDupeRecoveredBytes(nzbInfo->GetDupeRecoveredBytes() + m_recoveredBytes);
		nzbInfo->SetDupeRecoveredHoles(nzbInfo->GetDupeRecoveredHoles() + m_recoveredHoles);
	}

	for (RepairTarget& target : targets)
	{
		if (!target.PatchEligible)
		{
			continue;
		}
		for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
		{
			if (job.GetFileId() == target.FileId)
			{
				if (target.Recreated)
				{
					job.SetDecodedFileSize(target.DecodedFileSize);
					MarkRecreated(nzbInfo, target);
				}
				job.SetHoles(std::move(target.Holes));
				break;
			}
		}
	}

	nzbInfo->SetLiveRepairThread(nullptr);
	if (nzbInfo->GetPostInfo() && !nzbInfo->GetUnpackThread())
	{
		nzbInfo->GetPostInfo()->SetWorking(false);
	}

	downloadQueue->Save();
}

inline bool StreamRepairController::DonorDead(const DonorSource& donor, NzbInfo* donorNzb)
{
	auto it = m_donorDead.find(*donor.QueuedFilename);
	if (it != m_donorDead.end())
	{
		return it->second;
	}

	std::vector<FileInfo*> files;
	for (int pass = 0; pass < 2 && files.empty(); pass++)
	{
		for (FileInfo* fileInfo : donorNzb->GetFileList())
		{
			if (fileInfo->GetTotalArticles() > 0 && (pass == 1 || !fileInfo->GetParFile()))
			{
				files.push_back(fileInfo);
			}
		}
	}
	bool dead = false;
	std::vector<DupeProbe::Sample> samples = DupeProbe::SamplesOf(files);
	if ((int)samples.size() >= DupeProbe::MinArticles && !IsStopped())
	{
		m_checkingDonor++;
		DupeProbe::Verdict verdict = DupeProbe::Check(std::move(samples), DonorCheckSec);
		m_checkingDonor--;
		dead = verdict.Dead();
		if (dead)
		{
			PrintMessage(Message::mkInfo,
				"Skipping duplicate %s for stream repair: none of %i sampled articles exists on any server",
				*donor.InfoName, DupeProbe::SampleCount);
		}
	}
	m_donorDead[*donor.QueuedFilename] = dead;
	return dead;
}

inline void StreamRepairController::StartWatchdog(const std::vector<RepairTarget>& targets)
{
	m_fetcher.SetWaitCounter(&m_waitingForConnection);
	m_batchFetcher.SetWaitCounter(&m_waitingForConnection);
	int timeout = g_Options->GetDupeStreamTimeout();
	if (timeout <= 0)
	{
		return;
	}

	m_missingBytes = 0;
	for (const RepairTarget& target : targets)
	{
		m_missingBytes += target.DecodedFileSize > 0
			? DupeStreamRepair::TotalSize(target.Holes)
			: target.EncodedSize;
	}

	int upTimeSec = 0, downloadTimeSec = 0;
	int64 allBytes = 0;
	bool standBy = false;
	g_StatMeter->CalcTotalStat(&upTimeSec, &downloadTimeSec, &allBytes, &standBy);
	int64 downloadRate = downloadTimeSec > 0 ? allBytes / downloadTimeSec : 0;

	m_watchdog = std::thread([this, timeout, downloadRate]()
		{
			std::unique_lock<std::mutex> lock(m_watchdogMutex);
			int64 startProgress = m_progressBytes;
			int64 progress = startProgress;
			auto start = std::chrono::steady_clock::now();
			auto begun = start;
			auto lastProgress = start;
			auto lastTick = start;
			auto waiting = [this]() { return m_waitingForConnection > 0 || m_checkingDonor > 0; };
			while (!m_watchdogDone)
			{
				m_watchdogCond.wait_for(lock, std::chrono::milliseconds(500));
				if (m_watchdogDone)
				{
					break;
				}
				auto now = std::chrono::steady_clock::now();
				if (waiting())
				{
					start += now - lastTick;
				}
				lastTick = now;
				int64 sinceLastProgressSec =
					std::chrono::duration_cast<std::chrono::seconds>(now - lastProgress).count();
				int64 sinceBegunSec =
					std::chrono::duration_cast<std::chrono::seconds>(now - begun).count();
				int64 sinceStartSec =
					std::chrono::duration_cast<std::chrono::seconds>(now - start).count();
				double elapsedSec = std::chrono::duration<double>(now - start).count();
				int64 newProgress = progress;
				int resetLastProgress = 0;
				NzbgetRsBuf reasonBuf{nullptr, 0, 0};
				int shouldStop = nzbget_rs_stream_watchdog_eval_tick(
					m_progressBytes, progress, waiting() ? 1 : 0, m_materializing.load(),
					sinceLastProgressSec, sinceBegunSec, sinceStartSec, elapsedSec,
					timeout, MaxRunFactor, MaxConnectionWaitSec, SlowRepairFactor,
					downloadRate, startProgress, m_missingBytes,
					&newProgress, &resetLastProgress, &reasonBuf);
				progress = newProgress;
				if (resetLastProgress)
				{
					lastProgress = now;
				}
				if (shouldStop && reasonBuf.data && reasonBuf.len > 0)
				{
					m_slowReason.Set(reasonBuf.data, (int)reasonBuf.len);
					nzbget_rs_free(reasonBuf);
					break;
				}
				nzbget_rs_free(reasonBuf);
			}
			if (!m_watchdogDone)
			{
				m_timedOut = true;
				lock.unlock();
				Stop();
			}
		});
}

inline void StreamRepairController::StopWatchdog(const char* nzbName, const std::vector<RepairTarget>& targets)
{
	if (!m_watchdog.joinable())
	{
		return;
	}
	{
		std::lock_guard<std::mutex> lock(m_watchdogMutex);
		m_watchdogDone = true;
	}
	m_watchdogCond.notify_all();
	m_watchdog.join();
	if (m_timedOut)
	{
		int64 missing = 0;
		int unsized = 0;
		for (const RepairTarget& target : targets)
		{
			missing += DupeStreamRepair::TotalSize(target.Holes);
			unsized += target.DecodedFileSize <= 0 ? 1 : 0;
		}
		NzbgetRsBuf wholeFilesBuf = nzbget_rs_stream_format_whole_files_suffix(unsized);
		PrintMessage(Message::mkWarning,
			"Stream repair of %s stopped: %s (option DupeStreamTimeout); %.1f MB recovered, %.1f MB%s still missing",
			nzbName, *m_slowReason,
			nzbget_rs_dqh_stream_repair_bytes_to_mb(m_recoveredBytes),
			nzbget_rs_dqh_stream_repair_bytes_to_mb(missing),
			wholeFilesBuf.data ? wholeFilesBuf.data : "");
		nzbget_rs_free(wholeFilesBuf);
	}
}

inline void StreamRepairController::Stop()
{
	m_batchFetcher.Stop();
	m_fetcher.Stop();
	Thread::Stop();
}

inline void StreamRepairController::CollectTargets(NzbInfo* nzbInfo, std::vector<RepairTarget>& targets,
	std::vector<CString>& memberNames)
{
	m_hasParFiles = nzbInfo->GetParSize() > 0;

	NzbParameter* parameter = nzbInfo->GetParameters()->Find("*Unpack:Password");
	if (parameter)
	{
		m_targetPassword = parameter->GetValue();
	}

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		memberNames.emplace_back(completedFile.GetFilename());
	}

	for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
	{
		if (nzbget_rs_stream_is_par2_job_or_file(
			job.GetParFile() ? 1 : 0, job.GetFilename()) != 0)
		{
			continue;
		}
		RepairTarget target;
		target.FileId = job.GetFileId();
		target.Filename = job.GetFilename();
		target.DecodedFileSize = job.GetDecodedFileSize();
		target.FailedSize = job.GetFailedSize();
		target.MissedSize = job.GetMissedSize();
		target.FailedArticles = job.GetFailedArticles();
		target.IsParFile = job.GetParFile();
		target.Holes = *job.GetHoles();
		target.StepsHash = job.GetStepsHash();
		target.EncodedSize = job.GetFailedSize() + job.GetMissedSize();
		target.ArticleCount = job.GetFailedArticles();

		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetId() == target.FileId)
			{
				target.Filename = completedFile.GetFilename();
				target.IsParFile = target.IsParFile || completedFile.GetParFile();
				break;
			}
		}

		if (nzbget_rs_stream_is_par2_job_or_file(
			target.IsParFile ? 1 : 0, target.Filename))
		{
			continue;
		}
		targets.push_back(std::move(target));
	}
}

inline void StreamRepairController::ComputePositionalRanks(const char* destDir,
	std::vector<RepairTarget>& targets, const std::vector<CString>& memberNames)
{
	std::vector<const char*> names;
	std::vector<int64_t> sizes;
	names.reserve(memberNames.size());
	sizes.reserve(memberNames.size());
	for (const CString& memberName : memberNames)
	{
		NzbgetRsBuf memberPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			destDir, PATH_SEPARATOR, *memberName);
		BString<1024> memberPath;
		memberPath.Set(memberPathBuf.data ? memberPathBuf.data : "", (int)memberPathBuf.len);
		nzbget_rs_free(memberPathBuf);
		names.push_back(*memberName);
		sizes.push_back(FileSystem::FileSize(memberPath));
	}
	for (RepairTarget& target : targets)
	{
		int rank = -1;
		int windowSize = 0;
		nzbget_rs_stream_compute_positional_rank(
			names.data(), sizes.data(), names.size(),
			target.Filename, target.DecodedFileSize, &rank, &windowSize);
		target.PositionalRank = rank;
		target.PositionalWindow = windowSize;
	}
}

inline void StreamRepairController::CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo,
	std::vector<DonorSource>& donors)
{
	RawNzbList donorNzbs;

	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		if (queuedNzbInfo != nzbInfo &&
			queuedNzbInfo->GetKind() == NzbInfo::nkNzb &&
			queuedNzbInfo->GetDupeMode() != dmForce &&
			DupeCoordinator::SameNameOrKey(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			donorNzbs.push_back(queuedNzbInfo);
		}
	}

	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			historyInfo->GetNzbInfo()->GetDupeMode() != dmForce &&
			DupeCoordinator::SameNameOrKey(historyInfo->GetNzbInfo()->GetName(),
				historyInfo->GetNzbInfo()->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			donorNzbs.push_back(historyInfo->GetNzbInfo());
		}
	}

	if (!donorNzbs.empty())
	{
		std::vector<int32_t> dupeScores;
		std::vector<int32_t> donorIds;
		std::vector<size_t> order(donorNzbs.size());
		dupeScores.reserve(donorNzbs.size());
		donorIds.reserve(donorNzbs.size());
		for (NzbInfo* d : donorNzbs)
		{
			dupeScores.push_back(d->GetDupeScore());
			donorIds.push_back(d->GetId());
		}
		size_t n = nzbget_rs_stream_order_donor_indices(
			dupeScores.data(), donorIds.data(), donorNzbs.size(), order.data());
		RawNzbList sortedDonors;
		sortedDonors.reserve(n);
		for (size_t i = 0; i < n; i++)
		{
			sortedDonors.push_back(donorNzbs[order[i]]);
		}
		donorNzbs = std::move(sortedDonors);
	}

	for (NzbInfo* donorNzbInfo : donorNzbs)
	{
		if ((nzbInfo->GetFullContentHash() > 0 &&
				nzbInfo->GetFullContentHash() == donorNzbInfo->GetFullContentHash()) ||
			Util::EmptyStr(donorNzbInfo->GetQueuedFilename()))
		{
			continue;
		}
		{
			DonorSource donor;
			donor.QueuedFilename = donorNzbInfo->GetQueuedFilename();
			donor.InfoName = donorNzbInfo->GetName();
			NzbParameter* parameter = donorNzbInfo->GetParameters()->Find("*Unpack:Password");
			if (parameter)
			{
				donor.Password = parameter->GetValue();
			}
			donors.push_back(std::move(donor));
		}
	}
}

inline void StreamRepairController::ExecRepair(const char* destDir,
	std::vector<RepairTarget>& targets, std::vector<DonorSource>& donors,
	const std::vector<CString>& memberNames)
{
	int primaryConnections = 0;
	for (NewsServer* server : g_ServerPool->GetServers())
	{
		if (server->GetActive() && server->GetNormLevel() == 0)
		{
			primaryConnections += server->GetMaxConnections();
		}
	}
	m_batchFetcher.SetWorkerCount(m_liveMode ? 0 :
		DupeStreamRepair::StreamFetchWorkerCount(primaryConnections));
	std::set<std::string> triedPostings;

	for (DonorSource& donor : donors)
	{
		if (IsStopped())
		{
			break;
		}

		bool anyHoles = false;
		for (RepairTarget& target : targets)
		{
			anyHoles |= target.PatchEligible && target.NeedsRepair();
		}
		if (!anyHoles)
		{
			break;
		}

		std::unique_ptr<NzbInfo> donorNzb = DupeArticleFallback::ParseDonorNzb(donor.QueuedFilename);
		if (!donorNzb)
		{
			continue;
		}
		if (!triedPostings.insert(DupeStreamRepair::BuildDonorKey(
			donorNzb.get(), donor.Password)).second)
		{
			PrintMessage(Message::mkInfo,
				"Skipping another copy of duplicate posting %s", *donor.InfoName);
			continue;
		}
		if (DonorDead(donor, donorNzb.get()))
		{
			continue;
		}

		if (m_postInfo)
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(BString<1024>("Repairing from duplicate %s", *donor.InfoName));
			m_postInfo->SetStageProgress(0);
		}

		std::vector<const char*> memberNamePtrs(memberNames.size());
		for (size_t i = 0; i < memberNames.size(); i++)
		{
			memberNamePtrs[i] = *memberNames[i];
		}

		std::vector<const char*> targetFilenames(targets.size());
		std::vector<int64_t> targetEncodedSizes(targets.size());
		std::vector<int64_t> targetDecodedSizes(targets.size());
		std::vector<int> targetArticleCounts(targets.size());
		std::vector<int> targetPositionalRanks(targets.size());
		std::vector<int> targetPositionalWindows(targets.size());
		std::vector<uint64_t> targetStepsHashes(targets.size());
		std::vector<uint8_t> targetPatchEligible(targets.size());
		std::vector<size_t> targetHoleCounts(targets.size());
		std::vector<NzbgetRsByteRange> targetHolesFlat;
		for (size_t t = 0; t < targets.size(); t++)
		{
			targetFilenames[t] = *targets[t].Filename;
			targetEncodedSizes[t] = targets[t].EncodedSize;
			targetDecodedSizes[t] = targets[t].DecodedFileSize;
			targetArticleCounts[t] = targets[t].ArticleCount;
			targetPositionalRanks[t] = targets[t].PositionalRank;
			targetPositionalWindows[t] = targets[t].PositionalWindow;
			targetStepsHashes[t] = targets[t].StepsHash;
			targetPatchEligible[t] = targets[t].PatchEligible ? 1 : 0;
			targetHoleCounts[t] = targets[t].Holes.size();
			for (const StreamRange& h : targets[t].Holes)
			{
				targetHolesFlat.push_back({h.Offset, h.Size});
			}
		}

		std::vector<FileInfo*> donorFiles;
		std::vector<const char*> donorFileNames;
		std::vector<int64_t> donorFileSizes;
		std::vector<uint8_t> donorFileIsPar;
		std::vector<uint64_t> donorFileStepsHashes;
		std::vector<size_t> donorArticleCounts;
		std::vector<int32_t> donorArticleSizesFlat;
		std::vector<std::shared_ptr<std::vector<CString>>> donorGroupsCache;
		for (FileInfo* df : donorNzb->GetFileList())
		{
			donorFiles.push_back(df);
			donorFileNames.push_back(df->GetFilename());
			donorFileSizes.push_back(df->GetSize());
			donorFileIsPar.push_back(df->GetParFile() ? 1 : 0);
			donorArticleCounts.push_back(df->GetArticles()->size());
			for (const std::unique_ptr<ArticleInfo>& a : *df->GetArticles())
			{
				donorArticleSizesFlat.push_back(a->GetSize());
			}
			const std::vector<int32_t>& artSizes = donorArticleCounts.back() > 0
				? std::vector<int32_t>(
					donorArticleSizesFlat.end() - (ptrdiff_t)donorArticleCounts.back(),
					donorArticleSizesFlat.end())
				: std::vector<int32_t>{};
			donorFileStepsHashes.push_back(
				nzbget_rs_dupe_fallback_steps_hash(artSizes.data(), artSizes.size()));
			auto grp = std::make_shared<std::vector<CString>>();
			for (const CString& g : *df->GetGroups())
			{
				grp->emplace_back(*g);
			}
			donorGroupsCache.push_back(std::move(grp));
		}

		struct M1DonorCtx
		{
			StreamRepairController* Controller;
			std::vector<RepairTarget>* Targets;
			std::vector<FileInfo*>* DonorFiles;
			std::vector<std::shared_ptr<std::vector<CString>>>* DonorGroupsCache;
			ArticleFetcher::FetchedArticle CurrentFetched;
			std::exception_ptr Error;
		} donorCtx{this, &targets, &donorFiles, &donorGroupsCache, {}, nullptr};

		nzbget_rs_stream_repair_process_donor(
			destDir,
			m_liveMode ? 1 : 0,
			memberNamePtrs.data(),
			memberNamePtrs.size(),
			targetFilenames.data(),
			targetEncodedSizes.data(),
			targetDecodedSizes.data(),
			targetArticleCounts.data(),
			targetPositionalRanks.data(),
			targetPositionalWindows.data(),
			targetStepsHashes.data(),
			targetPatchEligible.data(),
			targetHoleCounts.data(),
			targetHolesFlat.data(),
			targets.size(),
			*donor.InfoName,
			donorFileNames.data(),
			donorFileSizes.data(),
			donorFileIsPar.data(),
			donorFileStepsHashes.data(),
			donorArticleCounts.data(),
			donorArticleSizesFlat.data(),
			donorFiles.size(),
			&donorCtx,
			[](void* rawCtx) -> int
			{
				return RepairMapCall<M1DonorCtx>(rawCtx, 1, [](M1DonorCtx* c)
				{
					return c->Controller->IsStopped() ? 1 : 0;
				});
			},
			[](void* rawCtx, size_t targetIdx) -> const char*
			{
				return RepairMapCall<M1DonorCtx>(rawCtx, static_cast<const char*>(nullptr), [=](M1DonorCtx* c) -> const char*
				{
					if (targetIdx >= c->Targets->size())
					{
						return nullptr;
					}
					c->Controller->RefreshLiveTargetName((*c->Targets)[targetIdx]);
					return *(*c->Targets)[targetIdx].Filename;
				});
			},
			[](void* rawCtx, int32_t memberIndex, const int32_t* partIndices, size_t count)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [=](M1DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return;
					}
					FileInfo* df = (*c->DonorFiles)[memberIndex];
					const auto& grp = (*c->DonorGroupsCache)[memberIndex];
					std::vector<ArticleBatchFetcher::Request> requests;
					requests.reserve(count);
					for (size_t i = 0; i < count; i++)
					{
						int32_t pi = partIndices[i];
						if (pi >= 0 && (size_t)pi < df->GetArticles()->size())
						{
							ArticleInfo* article = (*df->GetArticles())[pi].get();
							requests.push_back({article->GetMessageId(), grp});
						}
					}
					c->Controller->m_batchFetcher.Begin(std::move(requests));
				});
			},
			[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
				const uint8_t** outData, size_t* outDataLen) -> int
			{
				return RepairMapCall<M1DonorCtx>(rawCtx, 0, [=](M1DonorCtx* c)
				{
					c->CurrentFetched = ArticleFetcher::FetchedArticle();
					if (!c->Controller->m_batchFetcher.Next(c->CurrentFetched))
					{
						return 0;
					}
					*outSuccess = c->CurrentFetched.Success ? 1 : 0;
					*outOffset = c->CurrentFetched.Offset;
					*outFileSize = c->CurrentFetched.FileSize;
					*outData = reinterpret_cast<const uint8_t*>(c->CurrentFetched.Data.data());
					*outDataLen = c->CurrentFetched.Data.size();
					return 1;
				});
			},
			[](void* rawCtx)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [](M1DonorCtx* c)
				{
					c->Controller->m_batchFetcher.CancelRemaining();
				});
			},
			[](void* rawCtx, int64_t bytes)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [bytes](M1DonorCtx* c)
				{
					c->Controller->m_progressBytes += bytes;
				});
			},
			[](void* rawCtx, int64_t bytes)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [bytes](M1DonorCtx* c)
				{
					c->Controller->m_recoveredBytes += bytes;
				});
			},
			[](void* rawCtx, size_t targetIdx, const char* filename, int64_t decodedFileSize,
				int recreated, const NzbgetRsByteRange* holes, size_t holeCount)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [=](M1DonorCtx* c)
				{
					if (targetIdx >= c->Targets->size())
					{
						return;
					}
					RepairTarget& target = (*c->Targets)[targetIdx];
					if (filename && *filename)
					{
						target.Filename = filename;
					}
					target.DecodedFileSize = decodedFileSize;
					if (recreated != 0)
					{
						target.Recreated = true;
					}
					target.Holes.assign(
						reinterpret_cast<const StreamRange*>(holes),
						reinterpret_cast<const StreamRange*>(holes) + holeCount);
				});
			},
			[](void* rawCtx, int32_t kind, const char* text)
			{
				RepairMapCallVoid<M1DonorCtx>(rawCtx, [=](M1DonorCtx* c)
				{
					Message::EKind msgKind = kind == 3 ? Message::mkDetail :
						kind == 2 ? Message::mkError :
						kind == 1 ? Message::mkWarning : Message::mkInfo;
					c->Controller->PrintMessage(msgKind, "%s", text);
				});
			});
		if (donorCtx.Error)
		{
			std::rethrow_exception(donorCtx.Error);
		}
	}
}

inline void StreamRepairController::MarkRepaired(NzbInfo* nzbInfo, const RepairTarget& target)
{
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetId() == target.FileId &&
			completedFile.GetStatus() != CompletedFile::cfSuccess)
		{
			completedFile.SetStatus(CompletedFile::cfSuccess);
			g_DiskState->DiscardFile(target.FileId, true, true, true);
			break;
		}
	}
}

inline void StreamRepairController::MarkRecreated(NzbInfo* nzbInfo, const RepairTarget& target)
{
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetId() == target.FileId &&
			completedFile.GetStatus() == CompletedFile::cfFailure)
		{
			completedFile.SetStatus(CompletedFile::cfPartial);
			break;
		}
	}
}

inline void StreamRepairController::ReportRemainingHoles(std::vector<RepairTarget>& targets)
{
	bool interrupted = Interrupted();
	if (interrupted)
	{
		int unfinished = (int)std::count_if(targets.begin(), targets.end(),
			[](const RepairTarget& target)
			{
				return target.DecodedFileSize <= 0 || !target.Holes.empty();
			});
		m_holesRemain |= unfinished > 0;
		PrintMessage(Message::mkInfo, "Stream repair interrupted with %i file(s) still incomplete", unfinished);
	}

	const char* parDisposition = m_hasParFiles ? "left to par-repair" : "no par2 files to repair it";
	for (RepairTarget& target : targets)
	{
		int targetStatus = nzbget_rs_stream_classify_remaining_hole_target(
			interrupted ? 1 : 0, target.DecodedFileSize, target.Holes.empty() ? 1 : 0,
			target.IsParFile ? 1 : 0, target.FailedArticles, target.FailedSize, target.MissedSize,
			&m_recoveredArticles, &m_recoveredFailedSize, &m_recoveredFailedParSize);
		if (targetStatus == 1)
		{
			continue;
		}
		if (targetStatus == 2)
		{
			m_holesRemain = true;
			PrintMessage(Message::mkInfo,
				"Stream repair of %s: no duplicate could supply the file (%s)",
				*target.Filename, parDisposition);
		}
		else if (targetStatus == 3)
		{
			m_holesRemain = true;
			int64 missing = DupeStreamRepair::TotalSize(target.Holes);
			PrintMessage(Message::mkInfo,
				"Stream repair of %s: %.1f MB still missing after stream repair (%s)",
				*target.Filename, nzbget_rs_dqh_stream_repair_bytes_to_mb(missing),
				parDisposition);
			if (nzbget_rs_stream_is_incomplete_nzb_hole(missing, target.EncodedSize) != 0)
			{
				PrintMessage(Message::mkWarning,
					"%s is %.1f MB according to its articles, but the nzb's articles cover only "
					"%.1f MB of it: the nzb is incomplete",
					*target.Filename, nzbget_rs_dqh_stream_repair_bytes_to_mb(target.DecodedFileSize),
					nzbget_rs_dqh_stream_repair_bytes_to_mb(target.DecodedFileSize - missing));
			}
		}
	}
}

inline void StreamRepairController::ExecCrossPackRepair(const char* destDir,
	std::vector<RepairTarget>& targets, std::vector<DonorSource>& donors,
	const std::vector<CString>& memberNames)
{
	std::vector<const char*> memberNamePtrs(memberNames.size());
	for (size_t i = 0; i < memberNames.size(); i++)
	{
		memberNamePtrs[i] = *memberNames[i];
	}

	std::vector<const char*> targetFilenames(targets.size());
	std::vector<int64_t> targetDecodedSizes(targets.size());
	std::vector<uint8_t> targetPatchEligible(targets.size());
	std::vector<size_t> targetHoleCounts(targets.size());
	std::vector<NzbgetRsByteRange> targetHolesFlat;
	for (size_t t = 0; t < targets.size(); t++)
	{
		targetFilenames[t] = *targets[t].Filename;
		targetDecodedSizes[t] = targets[t].DecodedFileSize;
		targetPatchEligible[t] = targets[t].PatchEligible ? 1 : 0;
		targetHoleCounts[t] = targets[t].Holes.size();
		for (const StreamRange& h : targets[t].Holes)
		{
			targetHolesFlat.push_back({h.Offset, h.Size});
		}
	}

	struct InitCtx
	{
		StreamRepairController* Controller;
		std::exception_ptr Error;
	} initCtx{this, nullptr};

	NzbgetRsCrossPackSession* session = nzbget_rs_cross_pack_session_new(
		destDir,
		memberNamePtrs.data(),
		memberNamePtrs.size(),
		targetFilenames.data(),
		targetDecodedSizes.data(),
		targetPatchEligible.data(),
		targetHoleCounts.data(),
		targetHolesFlat.data(),
		targets.size(),
		m_targetPassword.Empty() ? nullptr : *m_targetPassword,
		&initCtx,
		[](void* rawCtx) -> int
		{
			return RepairMapCall<InitCtx>(rawCtx, 1, [](InitCtx* c)
			{
				return c->Controller->IsStopped() ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t kind, const char* text)
		{
			RepairMapCallVoid<InitCtx>(rawCtx, [=](InitCtx* c)
			{
				Message::EKind msgKind = kind == 2 ? Message::mkError :
					kind == 1 ? Message::mkWarning : Message::mkInfo;
				c->Controller->PrintMessage(msgKind, "%s", text);
			});
		},
#ifdef DEBUG
		[](void* rawCtx, const char* text)
		{
			RepairMapCallVoid<InitCtx>(rawCtx, [=](InitCtx*)
			{
				debug("%s", text ? text : "");
			});
		}
#else
		nullptr
#endif
	);
	if (initCtx.Error)
	{
		nzbget_rs_cross_pack_session_free(session);
		std::rethrow_exception(initCtx.Error);
	}
	if (!session)
	{
		return;
	}
	struct SessionGuard
	{
		NzbgetRsCrossPackSession* Ptr;
		~SessionGuard() { nzbget_rs_cross_pack_session_free(Ptr); }
	} sessionGuard{session};

	std::set<std::string> triedPostings;

	for (DonorSource& donor : donors)
	{
		if (IsStopped() || !nzbget_rs_cross_pack_session_has_inner_holes(session))
		{
			break;
		}

		std::unique_ptr<NzbInfo> donorNzb =
			DupeArticleFallback::ParseDonorNzb(donor.QueuedFilename);
		if (!donorNzb)
		{
			continue;
		}
		if (!triedPostings.insert(DupeStreamRepair::BuildDonorKey(
			donorNzb.get(), donor.Password)).second)
		{
			PrintMessage(Message::mkInfo,
				"Skipping another copy of duplicate posting %s", *donor.InfoName);
			continue;
		}
		if (DonorDead(donor, donorNzb.get()))
		{
			continue;
		}

		std::vector<FileInfo*> donorFiles;
		std::vector<const char*> donorFileNames;
		std::vector<int64_t> donorFileSizes;
		std::vector<size_t> donorArticleCounts;
		std::vector<int32_t> donorArticleSizesFlat;
		for (FileInfo* fileInfo : donorNzb->GetFileList())
		{
			donorFiles.push_back(fileInfo);
			donorFileNames.push_back(fileInfo->GetFilename());
			donorFileSizes.push_back(fileInfo->GetSize());
			size_t artCount = fileInfo->GetArticles() ? fileInfo->GetArticles()->size() : 0;
			donorArticleCounts.push_back(artCount);
			if (fileInfo->GetArticles())
			{
				for (ArticleInfo* article : fileInfo->GetArticles())
				{
					donorArticleSizesFlat.push_back(article->GetSize());
				}
			}
		}

		bool allowDecompress = !m_liveMode && g_Options->GetDupeStreamDecompress();

		struct DonorCtx
		{
			StreamRepairController* Controller;
			const std::vector<FileInfo*>* DonorFiles;
			std::vector<RepairTarget>* Targets;
			ArticleFetcher::FetchedArticle CurrentFetched;
			std::exception_ptr Error;
		} donorCtx{this, &donorFiles, &targets, {}, nullptr};

		nzbget_rs_cross_pack_session_process_donor(
			session,
			*donor.QueuedFilename,
			*donor.InfoName,
			donor.Password.Empty() ? "" : *donor.Password,
			donorFileNames.data(),
			donorFileSizes.data(),
			donorArticleCounts.data(),
			donorArticleSizesFlat.data(),
			donorFiles.size(),
			allowDecompress ? 1 : 0,
			&donorCtx,
			[](void* rawCtx) -> int
			{
				return RepairMapCall<DonorCtx>(rawCtx, 1, [](DonorCtx* c)
				{
					return c->Controller->IsStopped() ? 1 : 0;
				});
			},
			[](void* rawCtx, int32_t memberIndex, int32_t partIndex, int* outSuccess,
				int64_t* outOffset, int64_t* outFileSize, const uint8_t** outData, size_t* outDataLen) -> int
			{
				return RepairMapCall<DonorCtx>(rawCtx, 0, [=](DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return 0;
					}
					FileInfo* donorFile = (*c->DonorFiles)[memberIndex];
					if (!donorFile || !donorFile->GetArticles() || partIndex < 0 ||
						(size_t)partIndex >= donorFile->GetArticles()->size())
					{
						return 0;
					}
					ArticleInfo* article = (*donorFile->GetArticles())[partIndex].get();
					c->CurrentFetched = c->Controller->m_fetcher.Fetch(
						article->GetMessageId(), *donorFile->GetGroups());
					*outSuccess = c->CurrentFetched.Success ? 1 : 0;
					*outOffset = c->CurrentFetched.Offset;
					*outFileSize = c->CurrentFetched.FileSize;
					*outData = reinterpret_cast<const uint8_t*>(c->CurrentFetched.Data.data());
					*outDataLen = c->CurrentFetched.Data.size();
					return 1;
				});
			},
			[](void* rawCtx, int32_t memberIndex, const int32_t* partIndices, size_t count)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return;
					}
					FileInfo* donorFile = (*c->DonorFiles)[memberIndex];
					if (!donorFile || !donorFile->GetArticles())
					{
						return;
					}
					auto groups = std::make_shared<std::vector<CString>>();
					for (const CString& group : *donorFile->GetGroups())
					{
						groups->emplace_back(*group);
					}
					std::vector<ArticleBatchFetcher::Request> requests;
					requests.reserve(count);
					for (size_t i = 0; i < count; i++)
					{
						int partIndex = partIndices[i];
						if (partIndex >= 0 && (size_t)partIndex < donorFile->GetArticles()->size())
						{
							requests.push_back({
								(*donorFile->GetArticles())[partIndex]->GetMessageId(),
								groups
							});
						}
					}
					c->Controller->m_batchFetcher.Begin(std::move(requests));
				});
			},
			[](void* rawCtx, int* outSuccess, int64_t* outOffset, int64_t* outFileSize,
				const uint8_t** outData, size_t* outDataLen) -> int
			{
				return RepairMapCall<DonorCtx>(rawCtx, 0, [=](DonorCtx* c)
				{
					c->CurrentFetched = ArticleFetcher::FetchedArticle();
					if (!c->Controller->m_batchFetcher.Next(c->CurrentFetched))
					{
						return 0;
					}
					*outSuccess = c->CurrentFetched.Success ? 1 : 0;
					*outOffset = c->CurrentFetched.Offset;
					*outFileSize = c->CurrentFetched.FileSize;
					*outData = reinterpret_cast<const uint8_t*>(c->CurrentFetched.Data.data());
					*outDataLen = c->CurrentFetched.Data.size();
					return 1;
				});
			},
			[](void* rawCtx)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [](DonorCtx* c)
				{
					c->Controller->m_batchFetcher.CancelRemaining();
				});
			},
			[](void* rawCtx, int32_t memberIndex)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [memberIndex](DonorCtx* c)
				{
					if (memberIndex < 0 || (size_t)memberIndex >= c->DonorFiles->size())
					{
						return;
					}
					FileInfo* donorFile = (*c->DonorFiles)[memberIndex];
					if (!donorFile || !donorFile->GetArticles())
					{
						return;
					}
					auto donorGroups = std::make_shared<std::vector<CString>>();
					for (const CString& group : *donorFile->GetGroups())
					{
						donorGroups->emplace_back(*group);
					}
					std::vector<ArticleBatchFetcher::Request> requests;
					requests.reserve(donorFile->GetArticles()->size());
					for (std::unique_ptr<ArticleInfo>& article : *donorFile->GetArticles())
					{
						requests.push_back({article->GetMessageId(), donorGroups});
					}
					c->Controller->m_batchFetcher.Begin(std::move(requests));
				});
			},
			[](void* rawCtx, const char* label, int32_t stageProgress)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					if (c->Controller->m_postInfo)
					{
						GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
						c->Controller->m_postInfo->SetProgressLabel(label ? label : "");
						c->Controller->m_postInfo->SetStageProgress(stageProgress);
					}
				});
			},
			[](void* rawCtx, int32_t delta)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [delta](DonorCtx* c)
				{
					c->Controller->m_materializing += delta;
				});
			},
			[](void* rawCtx, int64_t bytes, int32_t holes)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [bytes, holes](DonorCtx* c)
				{
					c->Controller->m_recoveredBytes += bytes;
					c->Controller->m_progressBytes += bytes;
					c->Controller->m_recoveredHoles += holes;
				});
			},
			[](void* rawCtx, const size_t* updatedCounts, const NzbgetRsByteRange* updatedFlat, size_t)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					size_t cursor = 0;
					for (size_t t = 0; t < c->Targets->size(); t++)
					{
						size_t cnt = updatedCounts ? updatedCounts[t] : 0;
						(*c->Targets)[t].Holes.assign(
							reinterpret_cast<const StreamRange*>(updatedFlat + cursor),
							reinterpret_cast<const StreamRange*>(updatedFlat + cursor + cnt));
						cursor += cnt;
					}
				});
			},
			[](void* rawCtx, int32_t kind, const char* text)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx* c)
				{
					Message::EKind msgKind = kind == 2 ? Message::mkError :
						kind == 1 ? Message::mkWarning : Message::mkInfo;
					c->Controller->PrintMessage(msgKind, "%s", text);
				});
			},
			[](void* rawCtx, int32_t kind, const char* msg)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx*)
				{
					if (!msg) return;
					switch (kind)
					{
						case 0:
							info("%s", msg);
							break;
						case 1:
							error("%s", msg);
							break;
						case 2:
							warn("%s", msg);
							break;
						default:
							detail("%s", msg);
							break;
					}
				});
			},
#ifdef DEBUG
			[](void* rawCtx, const char* text)
			{
				RepairMapCallVoid<DonorCtx>(rawCtx, [=](DonorCtx*)
				{
					debug("%s", text ? text : "");
				});
			}
#else
			nullptr
#endif
		);
		if (donorCtx.Error)
		{
			std::rethrow_exception(donorCtx.Error);
		}
	}
}

inline void StreamRepairController::RepairCompleted()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();

	for (const RepairTarget& target : m_targets)
	{
		if (target.DecodedFileSize > 0 && target.Holes.empty())
		{
			MarkRepaired(nzbInfo, target);
		}
		else if (target.Recreated)
		{
			MarkRecreated(nzbInfo, target);
		}
	}

	if (m_recoveredArticles > 0 || m_recoveredBytes > 0 || m_recoveredHoles > 0)
	{
		nzbInfo->SetDupeRecoveredArticles(nzbInfo->GetDupeRecoveredArticles() + m_recoveredArticles);
		nzbInfo->SetDupeRecoveredBytes(nzbInfo->GetDupeRecoveredBytes() + m_recoveredBytes);
		nzbInfo->SetDupeRecoveredHoles(nzbInfo->GetDupeRecoveredHoles() + m_recoveredHoles);
	}

	if (m_recoveredFailedSize > 0)
	{
		int64 failedSize = nzbInfo->GetFailedSize();
		int64 successSize = nzbInfo->GetSuccessSize();
		int64 curFailedSize = nzbInfo->GetCurrentFailedSize();
		int64 curSuccessSize = nzbInfo->GetCurrentSuccessSize();
		int64 parFailedSize = nzbInfo->GetParFailedSize();
		int64 parSuccessSize = nzbInfo->GetParSuccessSize();
		int64 parCurFailedSize = nzbInfo->GetParCurrentFailedSize();
		int64 parCurSuccessSize = nzbInfo->GetParCurrentSuccessSize();
		nzbget_rs_stream_credit_repaired_sizes(
			m_recoveredFailedSize, m_recoveredFailedParSize,
			&failedSize, &successSize, &curFailedSize, &curSuccessSize,
			&parFailedSize, &parSuccessSize, &parCurFailedSize, &parCurSuccessSize);
		nzbInfo->SetFailedSize(failedSize);
		nzbInfo->SetSuccessSize(successSize);
		nzbInfo->SetCurrentFailedSize(curFailedSize);
		nzbInfo->SetCurrentSuccessSize(curSuccessSize);
		nzbInfo->SetParFailedSize(parFailedSize);
		nzbInfo->SetParSuccessSize(parSuccessSize);
		nzbInfo->SetParCurrentFailedSize(parCurFailedSize);
		nzbInfo->SetParCurrentSuccessSize(parCurSuccessSize);
	}

	for (auto it = nzbInfo->GetStreamRepairJobs()->begin();
		it != nzbInfo->GetStreamRepairJobs()->end(); )
	{
		if (!Interrupted())
		{
			it = nzbInfo->GetStreamRepairJobs()->erase(it);
			continue;
		}
		bool matched = false;
		for (const RepairTarget& target : m_targets)
		{
			if (target.FileId == it->GetFileId())
			{
				matched = true;
				if (target.Holes.empty() && target.DecodedFileSize > 0)
				{
					it = nzbInfo->GetStreamRepairJobs()->erase(it);
				}
				else
				{
					if (it->GetDecodedFileSize() <= 0 && target.DecodedFileSize > 0)
					{
						it->SetDecodedFileSize(target.DecodedFileSize);
					}
					it->SetHoles(target.Holes);
					++it;
				}
				break;
			}
		}
		if (!matched)
		{
			++it;
		}
	}

	bool recovered = m_recoveredBytes > 0;
	m_postInfo->SetStreamRepairDone(true);
	m_postInfo->SetStreamRepairRecovered(recovered);

	{
		int resetParRename = 0, setParStatus = 0, newParStatus = 0;
		int forceParFull = 0, setReqParCheckFalse = 0, checkMainPars = 0;
		nzbget_rs_stream_resolve_completion_par_action(
			m_recoveredBytes, m_holesRemain ? 1 : 0,
			(int)nzbInfo->GetParRenameStatus(), (int)NzbInfo::rsNone,
			(int)nzbInfo->GetParStatus(), (int)NzbInfo::psSkipped,
			(int)g_Options->GetParCheck(), (int)Options::pcManual,
			(int)NzbInfo::psManual, (int)NzbInfo::psNone,
			&resetParRename, &setParStatus, &newParStatus,
			&forceParFull, &setReqParCheckFalse, &checkMainPars);
		if (resetParRename)
		{
			nzbInfo->SetParRenameStatus(NzbInfo::rsNone);
		}
		if (setParStatus)
		{
			nzbInfo->SetParStatus((NzbInfo::EParStatus)newParStatus);
		}
		if (checkMainPars && ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
		{
			m_postInfo->SetRequestParCheck(true);
		}
		if (forceParFull)
		{
			m_postInfo->SetForceParFull(true);
		}
		if (setReqParCheckFalse)
		{
			m_postInfo->SetRequestParCheck(false);
		}
	}

	downloadQueue->Save();

	m_postInfo->SetWorking(false);
}

inline void StreamRepairController::AddMessage(Message::EKind kind, const char* text)
{
	if (m_liveMode)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo)
		{
			nzbInfo->AddMessage(kind, text);
		}
		return;
	}

	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}

class MoveController : public Thread, public ScriptController
{
public:
	void Run() override
	{
		BString<1024> nzbName;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			m_interDir = fs::u8path(m_postInfo->GetNzbInfo()->GetDestDir());
			m_destDir = fs::u8path(m_postInfo->GetNzbInfo()->GetFinalDir());
			if (m_destDir.empty())
			{
				m_destDir = fs::u8path(m_postInfo->GetNzbInfo()->BuildFinalDirName().Str());
			}
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("move", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(*infoName);
		PrintMessage(Message::mkInfo, "Moving completed files for %s", *nzbName);

		bool ok = MoveFiles();

		nzbget_rs_dqh_post_capitalize_first_ascii(*infoName);

		if (ok)
		{
			RemoveStaleHardlinks(*m_postInfo->GetNzbInfo(), m_destDir);
		}

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			if (m_postInfo && m_postInfo->GetNzbInfo())
			{
				int moveStatus = 0;
				int outcome = nzbget_rs_move_controller_decide_outcome(
					ok ? 1 : 0, IsStopped() ? 1 : 0, &moveStatus);
				if (outcome == 1)
				{
					PrintMessage(Message::mkInfo, "%s successful", *infoName);
					m_postInfo->GetNzbInfo()->SetDestDir(fs::u8string(m_destDir).c_str());
					m_postInfo->GetNzbInfo()->SetFinalDir("");
					m_postInfo->GetNzbInfo()->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
				}
				else if (outcome == 2)
				{
					PrintMessage(Message::mkInfo, "%s interrupted, resumes after the restart", *infoName);
				}
				else
				{
					PrintMessage(Message::mkError, "%s failed", *infoName);
					m_postInfo->GetNzbInfo()->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
				}
				m_postInfo->SetWorking(false);
			}
		}
	}

	static void StartJob(PostInfo* postInfo)
	{
		MoveController* moveController = new MoveController();
		moveController->m_postInfo = postInfo;
		moveController->SetAutoDestroy(false);

		postInfo->SetPostThread(moveController);

		moveController->Start();
	}

	static Message::EKind ToMessageKind(int kind)
	{
		switch (kind)
		{
		case 1:
			return Message::mkWarning;
		case 2:
			return Message::mkError;
		default:
			return Message::mkInfo;
		}
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

	bool MoveFiles()
	{
		const std::string interStr = fs::u8string(m_interDir);
		const std::string destStr = fs::u8string(m_destDir);
		return nzbget_rs_move_controller_move_files(
			interStr.c_str(),
			destStr.c_str(),
			this,
			[](void* rawCtx) -> int {
				return static_cast<MoveController*>(rawCtx)->IsStopped() ? 1 : 0;
			},
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<MoveController*>(rawCtx)->PrintMessage(
					ToMessageKind(kind), "%s", msg ? msg : "");
			},
			[](void* rawCtx, const char* oldRelName, const char* newRelName) {
				auto* self = static_cast<MoveController*>(rawCtx);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
				{
					self->m_postInfo->GetNzbInfo()->RenameCompletedFile(
						oldRelName ? oldRelName : "", newRelName ? newRelName : "");
				}
			}) != 0;
	}

	void RemoveStaleHardlinks(NzbInfo& nzbInfo, const fs::path& destDir)
	{
		const auto& hardLinkPath = nzbInfo.GetHardLinkPath();
		if (hardLinkPath.empty()) return;

		const std::string destStr = fs::u8string(destDir);
		nzbget_rs_move_controller_remove_stale_hardlinks(
			hardLinkPath.c_str(),
			destStr.c_str(),
			this,
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<MoveController*>(rawCtx)->PrintMessage(
					ToMessageKind(kind), "%s", msg ? msg : "");
			});
	}

private:
	PostInfo* m_postInfo = nullptr;
	fs::path m_interDir;
	fs::path m_destDir;
};

class CleanupController : public Thread, public ScriptController
{
public:
	void Run() override
	{
		BString<1024> nzbName;
		CString destDir;
		CString finalDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			destDir = m_postInfo->GetNzbInfo()->GetDestDir();
			finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("cleanup", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(*infoName);

		PrintMessage(Message::mkInfo, "Cleaning up %s", *nzbName);

		int cleanupStatus = 0;
		int cleanupOutcome = nzbget_rs_cleanup_controller_run(
			destDir ? *destDir : "",
			finalDir ? *finalDir : "",
			g_Options->GetExtCleanupDisk(),
			this,
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<CleanupController*>(rawCtx)->PrintMessage(
					MoveController::ToMessageKind(kind), "%s", msg ? msg : "");
			},
			&cleanupStatus);

		nzbget_rs_dqh_post_capitalize_first_ascii(*infoName);

		if (cleanupOutcome == 1)
		{
			PrintMessage(Message::mkInfo, "%s successful", *infoName);
		}
		else if (cleanupOutcome == 2)
		{
			PrintMessage(Message::mkInfo, "Nothing to cleanup for %s", *nzbName);
		}
		else
		{
			PrintMessage(Message::mkError, "%s failed", *infoName);
		}
		m_postInfo->GetNzbInfo()->SetCleanupStatus((NzbInfo::ECleanupStatus)cleanupStatus);

		m_postInfo->SetWorking(false);
	}

	static void StartJob(PostInfo* postInfo)
	{
		CleanupController* cleanupController = new CleanupController();
		cleanupController->m_postInfo = postInfo;
		cleanupController->SetAutoDestroy(false);

		postInfo->SetPostThread(cleanupController);

		cleanupController->Start();
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo;
	CString m_destDir;
	CString m_finalDir;
};

class RenameController : public Thread, public ScriptController
{
public:
	enum EJobKind
	{
		jkPar,
		jkRar
	};

	RenameController()
		: m_postInfo(nullptr)
		, m_kind(jkPar)
	{
		debug("Creating RenameController");
	}

	virtual void Run()
	{
		BString<1024> nzbName;
		CString destDir;
		CString finalDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			destDir = m_postInfo->GetNzbInfo()->GetDestDir();
			finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("rename", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(infoName);

		const char* archiveKindPrefix = nzbget_rs_rename_controller_archive_kind_prefix(
			(int)m_kind, (int)jkRar);
		PrintMessage(Message::mkInfo, "Checking renamed %sfiles for %s",
			archiveKindPrefix, *nzbName);

		ExecRename(destDir, finalDir, nzbName);

		int runOutcome = nzbget_rs_rename_controller_classify_run_outcome(
			IsStopped() ? 1 : 0, m_renamedCount);
		if (runOutcome == 1)
		{
			PrintMessage(Message::mkWarning, "Renaming cancelled for %s", *nzbName);
		}
		else if (runOutcome == 2)
		{
			PrintMessage(Message::mkInfo, "Successfully renamed %i %sfile(s) for %s",
				m_renamedCount, archiveKindPrefix, *nzbName);
		}
		else
		{
			PrintMessage(Message::mkInfo, "No renamed %sfiles found for %s",
				archiveKindPrefix, *nzbName);
		}

		RenameCompleted(nzbName);
	}

	static void StartJob(PostInfo* postInfo, EJobKind kind)
	{
		RenameController* renameController = new RenameController();
		renameController->m_postInfo = postInfo;
		renameController->m_kind = kind;
		renameController->SetAutoDestroy(false);

		postInfo->SetPostThread(renameController);

		renameController->Start();
	}

protected:
	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo;
	int m_renamedCount = 0;
	bool m_hasMissedFiles = false;
	bool m_hasDamagedParFiles = false;
	EJobKind m_kind;

	void UpdateProgress(const char* label, int stage)
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		m_postInfo->SetProgressLabel(label ? label : "");
		m_postInfo->SetStageProgress(stage);
	}

	void ExecRename(const char* destDir, const char* finalDir, const char* nzbName)
	{
		if (m_kind == jkPar)
		{
#ifndef DISABLE_PARCHECK
			int detectMissingInt = 0;
			bool useFinal = nzbget_rs_rename_controller_eval_par_config(
				(int)m_postInfo->GetNzbInfo()->GetUnpackStatus(),
				(int)NzbInfo::usSuccess,
				(int)NzbInfo::usNone,
				Util::EmptyStr(finalDir) ? 1 : 0,
				&detectMissingInt) != 0;
			int renamedCount = 0;
			int hasMissedFiles = 0;
			int hasDamagedParFiles = 0;
			nzbget_rs_par_renamer_execute(
				nzbName ? nzbName : "",
				(useFinal ? finalDir : destDir) ? (useFinal ? finalDir : destDir) : "",
				g_Options ? g_Options->GetParIgnoreExt() : "",
				detectMissingInt,
				PATH_SEPARATOR,
				this,
				+[](void* rawCtx) noexcept -> int
				{
					return static_cast<RenameController*>(rawCtx)->IsStopped() ? 1 : 0;
				},
				+[](void* rawCtx, const char* label, int stage) noexcept
				{
					static_cast<RenameController*>(rawCtx)->UpdateProgress(label, stage);
				},
				+[](void* rawCtx, int kind, const char* text) noexcept
				{
					auto* self = static_cast<RenameController*>(rawCtx);
					Message::EKind msgKind = kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError;
					self->PrintMessage(msgKind, "%s", text ? text : "");
				},
				+[](void* rawCtx, const char* name) noexcept
				{
					static_cast<RenameController*>(rawCtx)->m_postInfo->GetParredFiles()->push_back(name ? name : "");
				},
				+[](void* rawCtx, const char* oldRel, const char* newRel) noexcept
				{
					static_cast<RenameController*>(rawCtx)->RegisterRenamedFile(
						oldRel ? oldRel : "", newRel ? newRel : "");
				},
				&renamedCount,
				&hasMissedFiles,
				&hasDamagedParFiles);
			m_hasMissedFiles = hasMissedFiles != 0;
			m_hasDamagedParFiles = hasDamagedParFiles != 0;
#endif
		}
		else if (m_kind == jkRar)
		{
			const char* password = "";
			NzbParameter* parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
			if (parameter)
			{
				password = parameter->GetValue();
			}

			struct Context
			{
				RenameController* self;
				std::exception_ptr error;

				int Fail()
				{
					error = std::current_exception();
					return -1;
				}
			} context{this, nullptr};

			NzbgetRsRarRenamerRunEnv env{};
			env.ctx = &context;
			env.stopped = +[](void* ctx) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { return c.self->IsStopped() ? 1 : 0; } catch (...) { return c.Fail(); }
			};
			env.progress = +[](void* ctx, const char* label, int stage) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { c.self->UpdateProgress(label, stage); return 0; } catch (...) { return c.Fail(); }
			};
			env.message = +[](void* ctx, int kind, const char* text) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try
				{
					c.self->PrintMessage(
						kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError,
						"%s", text ? text : "");
					return 0;
				}
				catch (...) { return c.Fail(); }
			};
			env.registerRenamed = +[](void* ctx, const char* oldBase, const char* newBase) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { c.self->RegisterRenamedFile(oldBase, newBase); return 0; } catch (...) { return c.Fail(); }
			};

#ifndef DISABLE_TLS
			const int tlsEnabled = 1;
#else
			const int tlsEnabled = 0;
#endif
			int renamedCount = 0;
			nzbget_rs_rar_renamer_execute(
				&env, destDir ? destDir : "", nzbName ? nzbName : "", password,
				g_Options ? g_Options->GetUnpackIgnoreExt() : "", tlsEnabled, &renamedCount);
			if (context.error)
			{
				std::rethrow_exception(context.error);
			}
		}
	}

	void RenameCompleted(const char* nzbName)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int reqExtraPars = 0;
		int reqParCheck = 0;
#ifndef DISABLE_PARCHECK
		int hasDamagedParFiles = m_hasDamagedParFiles ? 1 : 0;
		int hasMissedFiles = m_hasMissedFiles ? 1 : 0;
#else
		int hasDamagedParFiles = 0;
		int hasMissedFiles = 0;
#endif
		int initialStatus = nzbget_rs_rename_controller_complete_action(
			(int)m_kind, m_renamedCount,
			hasDamagedParFiles,
			m_postInfo->GetNzbInfo()->GetRemainingParCount(),
			hasMissedFiles,
			(int)m_postInfo->GetNzbInfo()->GetParStatus(),
			&reqExtraPars, &reqParCheck);
		if (m_kind == jkPar)
		{
			m_postInfo->GetNzbInfo()->SetParRenameStatus((NzbInfo::EPostRenameStatus)initialStatus);
#ifndef DISABLE_PARCHECK
			if (reqExtraPars)
			{
				PrintMessage(Message::mkInfo, "Requesting extra par2-files for %s to perform par-rename", nzbName ? nzbName : "");
				downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupResume, nullptr);
				downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
				if (nzbget_rs_rename_controller_should_reset_par_status(
					m_postInfo->GetNzbInfo()->GetRemainingSize()) != 0)
				{
					m_postInfo->GetNzbInfo()->SetParRenameStatus(NzbInfo::rsNone);
				}
			}
#endif
		}
		else if (m_kind == jkRar)
		{
			m_postInfo->GetNzbInfo()->SetRarRenameStatus((NzbInfo::EPostRenameStatus)initialStatus);
		}
#ifndef DISABLE_PARCHECK
		if (reqParCheck)
		{
			PrintMessage(Message::mkInfo, "Requesting par-check/repair for %s to restore missing files ", nzbName ? nzbName : "");
			m_postInfo->SetRequestParCheck(true);
		}
#endif

		m_postInfo->SetWorking(false);
	}

	void RegisterRenamedFile(const char* oldFilename, const char* newFilename)
	{
		bool found;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			found = m_postInfo->GetNzbInfo()->RenameCompletedFile(oldFilename, newFilename);
		}
		if (!found)
		{
			PrintMessage(Message::mkWarning,
				"Could not find completed-file entry for %s while renaming to %s",
				oldFilename, newFilename);
		}
		++m_renamedCount;
	}
};

class PostRenamerController final : public Thread, public ScriptController
{
public:
	static void StartJob(PostInfo* postInfo, bool postUnpack)
	{
		PostRenamerController* controller = new (std::nothrow) PostRenamerController();
		if (!controller)
		{
			error("Failed to allocate memory for PostRenamerController");
			return;
		}
		controller->m_postInfo = postInfo;
		controller->m_postUnpack = postUnpack;
		controller->SetAutoDestroy(false);
		postInfo->SetPostThread(controller);
		controller->Start();
	}

	void Run() override
	{
		std::string metaName;
		std::string nzbName;
		std::string dstDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
			metaName = nzbInfo->GetMetaName();
			nzbName = nzbInfo->GetName() ? nzbInfo->GetName() : "";
			dstDir = nzbInfo->GetDestDir() ? nzbInfo->GetDestDir() : "";
		}

		int status = nzbget_rs_post_renamer_run(
			m_postUnpack ? 1 : 0,
			metaName.c_str(),
			nzbName.c_str(),
			dstDir.c_str(),
			g_Options->GetRenameIgnoreExt(),
			this,
			+[](void* rawCtx, const char* infoName) {
				static_cast<PostRenamerController*>(rawCtx)->SetInfoName(infoName ? infoName : "");
			},
			+[](void* rawCtx) -> int {
				return static_cast<PostRenamerController*>(rawCtx)->IsStopped() ? 1 : 0;
			},
			+[](void* rawCtx, int kind, const char* msg) {
				auto* self = static_cast<PostRenamerController*>(rawCtx);
				Message::EKind msgKind = kind == 1
					? Message::mkWarning
					: kind == 2
						? Message::mkError
						: Message::mkInfo;
				self->PrintMessage(msgKind, "%s", msg ? msg : "");
			},
			+[](void* rawCtx, const char* oldName, const char* newName) {
				auto* self = static_cast<PostRenamerController*>(rawCtx);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
				{
					self->m_postInfo->GetNzbInfo()->RenameCompletedFile(
						oldName ? oldName : "", newName ? newName : "");
				}
			});

		GuardedDownloadQueue guard = DownloadQueue::Guard();
		if (m_postUnpack)
		{
			m_postInfo->GetNzbInfo()->SetPostUnpackRenamingStatus(
				static_cast<NzbInfo::PostUnpackRenamingStatus>(status));
		}
		else
		{
			m_postInfo->GetNzbInfo()->SetPostDownloadRenamingStatus(
				static_cast<NzbInfo::PostDownloadRenamingStatus>(status));
		}
		m_postInfo->SetWorking(false);
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo = nullptr;
	bool m_postUnpack = false;
};

class PostScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run()
	{
		StringBuilder scriptCommaList;

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			size_t pcount = m_postInfo->GetNzbInfo()->GetParameters()->size();
			std::vector<const char*> pnames;
			std::vector<const char*> pvals;
			pnames.reserve(pcount);
			pvals.reserve(pcount);
			for (NzbParameter& parameter : m_postInfo->GetNzbInfo()->GetParameters())
			{
				pnames.push_back(parameter.GetName());
				pvals.push_back(parameter.GetValue());
			}
			NzbgetRsBuf listBuf = nzbget_rs_sus_build_enabled_script_comma_list(pnames.data(), pvals.data(), pcount);
			if (listBuf.data)
			{
				scriptCommaList.Append(listBuf.data);
			}
			nzbget_rs_free(listBuf);
			m_postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
		}

		ExecuteScriptList(scriptCommaList);

		m_postInfo->SetStage(PostInfo::ptFinished);
		m_postInfo->SetWorking(false);
	}

	virtual void Stop()
	{
		debug("Stopping post-process-script");
		Thread::Stop();
		Terminate();
	}

	static void StartJob(PostInfo* postInfo)
	{
		if (!postInfo)
		{
			return;
		}

		if (postInfo->GetNzbInfo() && postInfo->GetNzbInfo()->GetSkipScriptProcessing())
		{
			return;
		}

		PostScriptController* scriptController = new PostScriptController();
		scriptController->m_postInfo = postInfo;
		scriptController->SetAutoDestroy(false);
		scriptController->m_prefixLen = 0;

		postInfo->SetPostThread(scriptController);

		scriptController->Start();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		// if any script has requested par-check, do not execute other scripts
		if (!script->GetPostScript() || m_postInfo->GetRequestParCheck())
		{
			return;
		}

		m_script = std::move(script);

		PrintMessage(Message::mkInfo, "Executing post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());

		BString<1024> progressLabel("Executing post-process-script %s", m_script->GetName());
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(progressLabel);
		}

		SetArgs({m_script->GetEntry()});

		BString<1024> infoName("post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());
		SetInfoName(infoName);

		SetLogPrefix(m_script->GetDisplayName());
		m_prefixLen = static_cast<int>(strlen(m_script->GetDisplayName())) + 2; // 2 = strlen(": ");
		PrepareParams(m_script->GetName());

		int exitCode = Execute();

		infoName[0] = 'P';

		SetLogPrefix(nullptr);
		ScriptStatus::EStatus status = AnalyseExitCode(exitCode, infoName);

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->GetScriptStatuses()->emplace_back(m_script->GetName(), status);
		}
	}

	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		const char* msgText = text + m_prefixLen;

		NzbgetRsPostScriptCmd cmd{};
		int cmdKind = nzbget_rs_sus_parse_post_script_cmd(msgText, &cmd);
		if (cmdKind != 0)
		{
			debug("Command %s detected", msgText + 6);
			if (cmdKind == 1)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->SetFinalDir(msgText + cmd.val_offset);
			}
			else if (cmdKind == 2)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->SetDestDir(msgText + cmd.val_offset);
			}
			else if (cmdKind == 3)
			{
				std::string param(msgText + cmd.key_offset, cmd.key_len);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->GetParameters()->SetParameter(param.c_str(), msgText + cmd.val_offset);
			}
			else if (cmdKind == 4)
			{
				SetLogPrefix(nullptr);
				PrintMessage(Message::mkWarning, "Marking %s as bad", m_postInfo->GetNzbInfo()->GetName());
				SetLogPrefix(m_script->GetDisplayName());
				m_postInfo->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
			}
			else
			{
				m_postInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Invalid command \"%s\" received from %s", msgText, GetInfoName());
			}
		}
		else
		{
			m_postInfo->GetNzbInfo()->AddMessage(kind, text);
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(text);
		}

		if (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority())
		{
			time_t stageTime = m_postInfo->GetStageTime();
			time_t startTime = m_postInfo->GetStartTime();
			time_t waitTime = Util::CurrentTime();

			while (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority() && !IsStopped())
			{
				Util::Sleep(100);

				time_t delta = Util::CurrentTime() - waitTime;
				if (stageTime > 0)
				{
					m_postInfo->SetStageTime(stageTime + delta);
				}
				if (startTime > 0)
				{
					m_postInfo->SetStartTime(startTime + delta);
				}
			}
		}
	}

private:
	PostInfo* m_postInfo;
	int m_prefixLen;
	std::shared_ptr<const Extension::Script> m_script;

	void PrepareParams(const char* scriptName)
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		ResetEnv();

		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		BString<1024> status = nzbInfo->MakeTextStatus(true);
		std::vector<int> serverIds;
		std::vector<int> serverSuccess;
		std::vector<int> serverFailed;
		serverIds.reserve(nzbInfo->GetServerStats()->size());
		serverSuccess.reserve(nzbInfo->GetServerStats()->size());
		serverFailed.reserve(nzbInfo->GetServerStats()->size());
		for (ServerStat& serverStat : nzbInfo->GetServerStats())
		{
			serverIds.push_back(serverStat.GetServerId());
			serverSuccess.push_back(serverStat.GetSuccessArticles());
			serverFailed.push_back(serverStat.GetFailedArticles());
		}

		NzbgetRsPostScriptEnv env{};
		env.nzb_id = nzbInfo->GetId();
		env.nzb_name = nzbInfo->GetName();
		env.dest_dir = nzbInfo->GetDestDir();
		env.filename = nzbInfo->GetFilename();
		env.queued_filename = nzbInfo->GetQueuedFilename();
		env.url = nzbInfo->GetUrl();
		env.final_dir = nzbInfo->GetFinalDir();
		env.category = nzbInfo->GetCategory();
		env.health = nzbInfo->CalcHealth();
		env.critical_health = nzbInfo->CalcCriticalHealth(false);
		env.dupe_key = nzbInfo->GetDupeKey();
		env.dupe_score = nzbInfo->GetDupeScore();
		env.dupe_mode = static_cast<int>(nzbInfo->GetDupeMode());
		env.status = status;
		env.script_status = static_cast<int>(nzbInfo->GetScriptStatuses()->CalcTotalStatus());
		env.par_status = static_cast<int>(nzbInfo->GetParStatus());
		env.unpack_status = static_cast<int>(nzbInfo->GetUnpackStatus());
		env.delete_status = static_cast<int>(nzbInfo->GetDeleteStatus());
		env.mark_status = static_cast<int>(nzbInfo->GetMarkStatus());
		env.total_articles = nzbInfo->GetTotalArticles();
		env.success_articles = nzbInfo->GetSuccessArticles();
		env.failed_articles = nzbInfo->GetFailedArticles();
		env.server_ids = serverIds.data();
		env.server_success_articles = serverSuccess.data();
		env.server_failed_articles = serverFailed.data();
		env.server_count = serverIds.size();

		nzbget_rs_post_script_prepare_env(
			&env,
			this,
			[](void* ctx, const char* name, const char* value)
			{
				static_cast<PostScriptController*>(ctx)->SetEnvVar(name, value);
			},
			[](void* ctx, const char* name, int value)
			{
				static_cast<PostScriptController*>(ctx)->SetIntEnvVar(name, value);
			});

		PrepareEnvScript(nzbInfo->GetParameters(), scriptName);
	}

	ScriptStatus::EStatus AnalyseExitCode(int exitCode, const char* upInfoName)
	{
		// The ScriptStatus is accumulated for all scripts:
		// If any script has failed the status is "failure", etc.

#ifndef DISABLE_PARCHECK
		constexpr int parcheckEnabled = 1;
#else
		constexpr int parcheckEnabled = 0;
#endif
		int msgAction = 0;
		int reqParCheck = 0;
		int status = nzbget_rs_sus_analyse_post_script_exit_code(
			exitCode,
			static_cast<int>(m_postInfo->GetNzbInfo()->GetParStatus()),
			static_cast<int>(NzbInfo::psSkipped),
			parcheckEnabled,
			static_cast<int>(ScriptStatus::srNone),
			static_cast<int>(ScriptStatus::srFailure),
			static_cast<int>(ScriptStatus::srSuccess),
			&msgAction,
			&reqParCheck);
		switch (msgAction)
		{
			case 1:
				PrintMessage(Message::mkInfo, "%s successful", upInfoName);
				break;
			case 2:
				PrintMessage(Message::mkError, "%s failed", upInfoName);
				break;
			case 3:
				PrintMessage(Message::mkInfo, "%s skipped", upInfoName);
				break;
			case 4:
				PrintMessage(Message::mkError, "%s requested par-check/repair, but the collection was already checked", upInfoName);
				break;
			case 5:
				PrintMessage(Message::mkInfo, "%s requested par-check/repair", upInfoName);
				break;
			default:
				PrintMessage(Message::mkError, "%s failed (terminated with unknown status)", upInfoName);
				break;
		}
		if (reqParCheck)
		{
			m_postInfo->SetRequestParCheck(true);
			m_postInfo->SetForceRepair(true);
		}
		return static_cast<ScriptStatus::EStatus>(status);
	}
};

inline PrePostProcessor::PrePostProcessor()
{
	debug("Creating PrePostProcessor");

	DownloadQueue::Guard()->Attach(this);
}

inline void PrePostProcessor::Run()
{
	debug("Entering PrePostProcessor-loop");

	while (!DownloadQueue::IsLoaded())
	{
		Util::Sleep(20);
	}

	if (g_Options->GetServerMode())
	{
		SanitisePostQueue();
	}

	while (!IsStopped())
	{
		if (g_WorkState->GetTempPausePostprocess())
		{
			// Postprocess is paused: just wait and loop
			Util::Sleep(200);
			continue;
		}

		if (m_queuedJobs)
		{
			// check post-queue every 200 msec
			CheckPostQueue();
			Util::Sleep(200);
		}
		else
		{
			// Wait until we get the stop signal or more jobs in the queue
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait(lk, [&]{ return m_queuedJobs || IsStopped(); });
		}
	}

	WaitJobs();

	debug("Exiting PrePostProcessor-loop");
}

inline void PrePostProcessor::WaitJobs()
{
	debug("PrePostProcessor: waiting for jobs to complete");

	// wait 5 seconds until all post-processing jobs gracefully finish
	time_t waitStart = Util::CurrentTime();
	while (Util::CurrentTime() - waitStart < 5)
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			if (m_activeJobs.empty())
			{
				break;
			}
		}
		CheckPostQueue();
		Util::Sleep(200);
	}

	// kill remaining post-processing jobs; not safe but we can't wait any longer
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetPostThread())
			{
				Thread* thread = postJob->GetPostInfo()->GetPostThread();
				postJob->GetPostInfo()->SetPostThread(nullptr);
				warn("Terminating active post-process job for %s", postJob->GetName());
				thread->Kill();
				delete thread;
			}
		}
	}

	// wait 5 seconds until direct unpack and live repair threads gracefully finish
	waitStart = Util::CurrentTime();
	while (Util::CurrentTime() - waitStart < 5)
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			if (std::find_if(downloadQueue->GetQueue()->begin(),
				downloadQueue->GetQueue()->end(),
				[](const std::unique_ptr<NzbInfo>& nzbInfo)
				{
					return nzbInfo->GetUnpackThread() != nullptr ||
						nzbInfo->GetLiveRepairThread() != nullptr;
				}) == downloadQueue->GetQueue()->end())
			{
				break;
			}
		}
		Util::Sleep(200);
	}

	// disconnect remaining direct unpack and live repair jobs
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbInfo->SetUnpackThread(nullptr);
			nzbInfo->SetLiveRepairThread(nullptr);
		}
	}

	debug("PrePostProcessor: Jobs are completed");
}

inline void PrePostProcessor::Stop()
{
	Thread::Stop();

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);

		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetPostThread())
			{
				postJob->GetPostInfo()->GetPostThread()->Stop();
			}
		}

		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetUnpackThread())
			{
				((DirectUnpack*)nzbInfo->GetUnpackThread())->Stop(downloadQueue, nzbInfo);
			}
			if (nzbInfo->GetLiveRepairThread())
			{
				nzbInfo->GetLiveRepairThread()->Stop();
			}
		}
	}

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

inline bool PrePostProcessor::HasActiveUnpack(bool includeWaitingVolume)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	return HasActiveUnpack(downloadQueue, includeWaitingVolume);
}

inline bool PrePostProcessor::HasActiveUnpack(DownloadQueue* downloadQueue, bool includeWaitingVolume)
{
	// Check post-processing active jobs for UnpackController
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetStage() == PostInfo::ptUnpacking)
			{
				return true;
			}
		}
	}

	// Check direct unpack jobs: does a direct unpack own the unpack slot / actively extract?
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetUnpackThread())
		{
			DirectUnpack* directUnpack = (DirectUnpack*)nzbInfo->GetUnpackThread();
			if (includeWaitingVolume ||
				(directUnpack->IsUnpacking() && !directUnpack->IsWaitingVolume()))
			{
				return true;
			}
		}
	}

	return false;
}

/**
 * Reset the state of items after reloading from disk and
 * delete items which could not be resumed.
 * Also count the number of post-jobs.
 */
inline void PrePostProcessor::SanitisePostQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsRunning && !nzbInfo->GetUnpackThread())
		{
			nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
		}
		PostInfo* postInfo = nzbInfo->GetPostInfo();
		if (!postInfo && nzbInfo->GetKind() == NzbInfo::nkNzb &&
			!nzbInfo->GetCompletedFiles()->empty() &&
			nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
			nzbInfo->IsDownloadCompleted(true))
		{
			// its last file finished while the program stopped (a reload, a
			// shutdown): the event that queues it for post-processing was
			// dropped then, and only paused par2-files are left, so nothing
			// else would ever start it
			nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s completely downloaded", nzbInfo->GetName());
			g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
			NzbDownloaded(downloadQueue, nzbInfo);
			postInfo = nzbInfo->GetPostInfo();
			if (postInfo)
			{
				m_queuedJobs--;	// counted again below
			}
		}
		if (postInfo)
		{
			m_queuedJobs++;
			int nextStage = 0;
			int reqParCheck = 0;
			nzbget_rs_prepost_sanitise_post_stage(
				(int)postInfo->GetStage(),
				(int)PostInfo::ptExecutingScript,
				(int)PostInfo::ptFinished,
				(int)PostInfo::ptQueued,
				FileSystem::DirectoryExists(nzbInfo->GetDestDir()) ? 1 : 0,
				nzbInfo->GetUnpackTried() ? 1 : 0,
				(int)nzbInfo->GetParStatus(),
				(int)NzbInfo::psSkipped,
				&nextStage,
				&reqParCheck);
			postInfo->SetStage((PostInfo::EStage)nextStage);
			if (reqParCheck)
			{
				postInfo->SetRequestParCheck(true);
			}
			postInfo->SetWorking(false);
		}
	}
}

inline void PrePostProcessor::DownloadQueueUpdate(void* aspect)
{
	if (IsStopped())
	{
		return;
	}

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaNzbFound ||
		queueAspect->action == DownloadQueue::eaUrlFound)
	{
		NzbFound(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbAdded ||
		queueAspect->action == DownloadQueue::eaUrlAdded)
	{
		NzbAdded(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbNamed)
	{
		g_QueueScriptCoordinator->EnqueueScript(queueAspect->nzbInfo, QueueScriptCoordinator::qeNzbNamed);
		CheckDownloadCompleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbDeleted &&
		queueAspect->nzbInfo->GetDeleting() &&
		!queueAspect->nzbInfo->GetPostInfo() &&
		queueAspect->nzbInfo->GetFileList()->empty())
	{
		// the deleting of nzbs is usually handled via eaFileDeleted-event, but when deleting nzb without
		// any files left the eaFileDeleted-event is not fired and we need to process eaNzbDeleted-event instead
		queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s deleted from queue", queueAspect->nzbInfo->GetName());
		NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaUrlDeleted)
	{
		NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaUrlFailed)
	{
		NzbCompleted(queueAspect->downloadQueue, queueAspect->nzbInfo, true);
	}
	else if (queueAspect->action == DownloadQueue::eaFileCompleted ||
		queueAspect->action == DownloadQueue::eaFileDeleted)
	{
		if (queueAspect->action == DownloadQueue::eaFileCompleted)
		{
			FileDownloaded(queueAspect->downloadQueue, queueAspect->nzbInfo, queueAspect->fileInfo);
		}

#ifndef DISABLE_PARCHECK
		{
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			for (NzbInfo* postJob : m_activeJobs)
			{
				PostInfo* pjInfo = postJob ? postJob->GetPostInfo() : nullptr;
				if (postJob &&
					queueAspect->fileInfo->GetNzbInfo() == postJob &&
					pjInfo &&
					pjInfo->GetPostThread() &&
					pjInfo->GetStage() >= PostInfo::ptLoadingPars &&
					pjInfo->GetStage() <= PostInfo::ptVerifyingRepaired &&
					((RepairController*)pjInfo->GetPostThread())->AddPar(
						queueAspect->fileInfo, queueAspect->action == DownloadQueue::eaFileDeleted))
				{
					return;
				}
			}
		}
#endif

		int eventAction = nzbget_rs_prepost_classify_file_event(
			queueAspect->action == DownloadQueue::eaFileCompleted ? 1 : 0,
			queueAspect->action == DownloadQueue::eaFileDeleted ? 1 : 0,
			queueAspect->fileInfo->GetDupeDeleted() ? 1 : 0,
			(int)queueAspect->fileInfo->GetNzbInfo()->GetDeleteStatus(),
			(int)NzbInfo::dsNone,
			(int)NzbInfo::dsHealth,
			queueAspect->nzbInfo->GetPostInfo() ? 1 : 0,
			queueAspect->nzbInfo->IsDownloadCompleted(true) ? 1 : 0,
			queueAspect->nzbInfo->IsDownloadCompleted(false) ? 1 : 0);
		if (eventAction == 1)
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s completely downloaded", queueAspect->nzbInfo->GetName());
			g_QueueScriptCoordinator->EnqueueScript(queueAspect->nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
			NzbDownloaded(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
		else if (eventAction == 2)
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s deleted from queue", queueAspect->nzbInfo->GetName());
			NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
	}
}

inline void PrePostProcessor::NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetDupeCheck() && nzbInfo->GetDupeMode() != dmForce)
	{
		g_DupeCoordinator->NzbFound(downloadQueue, nzbInfo);
	}
}

inline void PrePostProcessor::NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetParCheck() != Options::pcForce)
	{
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
	}

	if (g_Options->GetReorderFiles() && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Reordering files for %s", nzbInfo->GetName());
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupSortFiles, nullptr);
	}

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsDupe ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsCopy ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsGood ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsScan)
	{
		NzbCompleted(downloadQueue, nzbInfo, false);
	}
	else
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbAdded);
	}
}

inline void PrePostProcessor::CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	// Direct rename can pause the last PAR volumes after the data files
	// finished. Their in-flight articles may still be draining; the last
	// such request does not complete a file, so check both transitions.
	if (nzbget_rs_prepost_should_complete_after_direct_rename(
		IsStopped() ? 1 : 0,
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsSuccess,
		nzbInfo->GetPostInfo() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		!nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
		nzbInfo->IsDownloadCompleted(true) ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0) != 0)
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s completely downloaded", nzbInfo->GetName());
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
		NzbDownloaded(downloadQueue, nzbInfo);
	}
}

inline void PrePostProcessor::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (!nzbInfo)
	{
		return;
	}

	int enqueueDeleted = 0;
	int setParSkipped = 0;
	int dlRoute = nzbget_rs_prepost_classify_nzb_downloaded_entry(
		nzbInfo->GetSkipScriptProcessing() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsHealth,
		(int)NzbInfo::dsBad,
		nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		(int)nzbInfo->GetParStatus(),
		(int)NzbInfo::psNone,
		(int)g_Options->GetParCheck(),
		(int)Options::pcAlways,
		(int)Options::pcForce,
		&enqueueDeleted,
		&setParSkipped);
	if (dlRoute == 1)
	{
		if (nzbInfo->GetLiveRepairThread())
		{
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}
		nzbInfo->SetCleanupDisk(true);
		DeleteCleanup(nzbInfo);
		NzbCompleted(downloadQueue, nzbInfo, true);
		return;
	}
	if (enqueueDeleted)
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}
	if (dlRoute == 2)
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Queueing %s for post-processing", nzbInfo->GetName());

		nzbInfo->EnterPostProcess();

		if (setParSkipped)
		{
			nzbInfo->SetParStatus(NzbInfo::psSkipped);
		}

		if (nzbInfo->GetUnpackThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(true);
			{
				std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
				m_activeJobs.push_back(nzbInfo);
			}
			((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDownloaded(downloadQueue, nzbInfo);
		}

		if (nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(true);
			if (!nzbInfo->GetUnpackThread())
			{
				std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
				m_activeJobs.push_back(nzbInfo);
			}
		}

		nzbInfo->SetChanged(true);
		downloadQueue->SaveChanged();

		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_queuedJobs++;
		m_waitCond.notify_all();
	}
	else
	{
		if (nzbInfo->GetUnpackThread())
		{
			((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDownloaded(downloadQueue, nzbInfo);
		}

		if (nzbInfo->GetLiveRepairThread())
		{
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}

		NzbCompleted(downloadQueue, nzbInfo, true);
	}
}

inline void PrePostProcessor::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (nzbInfo->GetUnpackThread())
	{
		((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDeleted(downloadQueue, nzbInfo);
	}

	if (nzbInfo->GetLiveRepairThread())
	{
		// detach FIRST: the live pass re-checks attachment at every locked
		// touchpoint and aborts without writing anything back
		Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
		nzbInfo->SetLiveRepairThread(nullptr);
		liveRepairThread->Stop();
	}

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
	}
	bool callDownloaded = nzbInfo->GetDeleteStatus() == NzbInfo::dsHealth ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsBad;
	nzbInfo->SetDeleting(false);

	DeleteCleanup(nzbInfo);

	if (callDownloaded)
	{
		NzbDownloaded(downloadQueue, nzbInfo);
	}
	else
	{
		NzbCompleted(downloadQueue, nzbInfo, true);
	}
}

inline void PrePostProcessor::NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool saveQueue)
{
	int addToHistoryInt = 0, callDupeCompletedInt = 0, enqueueDeletedScriptInt = 0, needSaveInt = 0;
	nzbget_rs_prepost_eval_nzb_completed(
		(int)nzbInfo->GetDupeHint(),
		(int)NzbInfo::dhRedownloadAuto,
		g_Options->GetKeepHistory(),
		nzbInfo->GetAvoidHistory() ? 1 : 0,
		g_Options->GetDupeCheck() ? 1 : 0,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce,
		DupeCoordinator::FailsOver(nzbInfo) ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)NzbInfo::dsHealth,
		(int)NzbInfo::dsBad,
		(int)NzbInfo::dsScan,
		(int)NzbInfo::dsCopy,
		&addToHistoryInt,
		&callDupeCompletedInt,
		&enqueueDeletedScriptInt,
		&needSaveInt);
	bool addToHistory = addToHistoryInt != 0;
	if (addToHistory)
	{
		g_HistoryCoordinator->AddToHistory(downloadQueue, nzbInfo);
	}
	nzbInfo->SetAvoidHistory(false);
	bool needSave = needSaveInt != 0;
	if (callDupeCompletedInt)
	{
		g_DupeCoordinator->NzbCompleted(downloadQueue, nzbInfo);
		needSave = true;
	}
	if (enqueueDeletedScriptInt)
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}

	if (!addToHistory)
	{
		g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
		ParDamage::Forget(nzbInfo->GetId());
		downloadQueue->GetQueue()->Remove(nzbInfo);
		// the removal must reach the disk: the saved queue may hold the download
		// with its files already flagged deleted, which a restart before the next
		// save brought back as a queued download without files (B42)
		needSave = true;
	}

	if (saveQueue && needSave)
	{
		downloadQueue->Save();
	}
}

inline void PrePostProcessor::DeleteCleanup(NzbInfo* nzbInfo)
{
	int cleanupMode = nzbget_rs_prepost_classify_delete_cleanup_mode(
		nzbInfo->GetCleanupDisk() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsDupe,
		nzbInfo->GetParking() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0);
	if (cleanupMode != 0)
	{
		if (cleanupMode == 1)
		{
			if (FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
			{
				CString errmsg;
				detail("Deleting dir %s", nzbInfo->GetDestDir());
				if (!FileSystem::DeleteDirectoryWithContent(nzbInfo->GetDestDir(), errmsg))
				{
					error("Could not delete directory %s: %s", nzbInfo->GetDestDir(), *errmsg);
				}
			}
			return;
		}

		// download was cancelled, deleting already downloaded files from disk
		for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, completedFile.GetFilename());
			BString<1024> fullFileName;
			fullFileName.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
			nzbget_rs_free(fullFnBuf);
			if (FileSystem::FileExists(fullFileName))
			{
				detail("Deleting file %s", completedFile.GetFilename());
				FileSystem::DeleteFile(fullFileName);
			}
		}

		// delete .out.tmp-files
		DirBrowser dir(nzbInfo->GetDestDir());
		while (const char* filename = dir.Next())
		{
			size_t fnLen = strlen(filename);
			if (fnLen >= 8 && !strcmp(filename + fnLen - 8, ".out.tmp"))
			{
				NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
				BString<1024> fullFilename;
				fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
				nzbget_rs_free(fullFnBuf);
				detail("Deleting file %s", filename);
				FileSystem::DeleteFile(fullFilename);
			}
		}

		// delete old directory (if empty)
		FileSystem::DeleteDirectory(nzbInfo->GetDestDir());

		// delete final directory (if empty)
		const auto finalDir = nzbInfo->BuildFinalDirName();
		if (nzbInfo->GetDestDir() != finalDir && FileSystem::DirectoryExists(finalDir))
		{
			FileSystem::DeleteDirectory(finalDir);
		}
	}
}

inline void PrePostProcessor::CheckRequestPar(DownloadQueue* downloadQueue)
{
#ifndef DISABLE_PARCHECK
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	for (NzbInfo* postJob : m_activeJobs)
	{
		PostInfo* postInfo = postJob->GetPostInfo();

		int newForceParFull = postInfo->GetForceParFull() ? 1 : 0;
		int reqAction = nzbget_rs_prepost_classify_request_par_action(
			postInfo->GetRequestParCheck() ? 1 : 0,
			(int)postInfo->GetNzbInfo()->GetParStatus(),
			(int)NzbInfo::psSkipped,
			postInfo->GetForceRepair() ? 1 : 0,
			postInfo->GetNzbInfo()->GetParFull() ? 1 : 0,
			(int)g_Options->GetParCheck(),
			(int)Options::pcManual,
			newForceParFull,
			postInfo->GetNzbInfo()->GetFileList()->empty() ? 1 : 0,
			&newForceParFull);
		if (reqAction == 1)
		{
			postInfo->SetForceParFull(newForceParFull != 0);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psNone);
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
			postInfo->SetStage(PostInfo::ptQueued);
			postInfo->SetWorking(false);
		}
		else if (reqAction == 2 || reqAction == 3)
		{
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psManual);

			if (reqAction == 2)
			{
				postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
					"Downloading all remaining files for manual par-check for %s", postInfo->GetNzbInfo()->GetName());
				downloadQueue->EditEntry(postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupResume, nullptr);
				postInfo->SetStage(PostInfo::ptFinished);
			}
			else
			{
				postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
					"There are no par-files remain for download for %s", postInfo->GetNzbInfo()->GetName());
			}
			postInfo->SetWorking(false);
		}
	}
#endif
}

inline void PrePostProcessor::CleanupJobs(DownloadQueue* downloadQueue)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	m_activeJobs.erase(std::remove_if(m_activeJobs.begin(), m_activeJobs.end(),
		[processor = this, downloadQueue](NzbInfo* postJob)
		{
			PostInfo* postInfo = postJob->GetPostInfo();
			bool hasRunningThread = postInfo->GetPostThread() && postInfo->GetPostThread()->IsRunning();
			int64_t curTime = (!postInfo->GetWorking() && !hasRunningThread && postInfo->GetStartTime() > 0)
				? (int64_t)Util::CurrentTime()
				: 0;
			int updateTotalSec = 0;
			int isCompleted = 0;
			int newTotalSec = postJob->GetPostTotalSec();
			if (nzbget_rs_prepost_eval_cleanup_job_state(
				postInfo->GetWorking() ? 1 : 0,
				hasRunningThread ? 1 : 0,
				(int)postInfo->GetStage(),
				(int)PostInfo::ptFinished,
				postInfo->GetDeleted() ? 1 : 0,
				(int64_t)postInfo->GetStartTime(),
				curTime,
				postJob->GetPostTotalSec(),
				&updateTotalSec,
				&isCompleted,
				&newTotalSec) != 0)
			{
				delete postInfo->GetPostThread();
				postInfo->SetPostThread(nullptr);

				postInfo->SetStageTime(0);
				postInfo->SetStageProgress(0);
				postInfo->SetFileProgress(0);
				postInfo->SetProgressLabel("");

				if (updateTotalSec != 0)
				{
					postJob->SetPostTotalSec(newTotalSec);
					postInfo->SetStartTime(0);
				}

				if (isCompleted != 0)
				{
					processor->JobCompleted(downloadQueue, postInfo);
				}
				else
				{
					postInfo->SetStage(PostInfo::ptQueued);
				}
				return true;
			}
			return false;
		}),
		m_activeJobs.end());
}

inline bool PrePostProcessor::CanRunMoreJobs(bool* allowPar)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	int allowParInt = 0;
	int canRun = nzbget_rs_prepost_can_run_more_jobs(
		stages.data(),
		stages.size(),
		(int)PostInfo::ptLoadingPars,
		(int)PostInfo::ptVerifyingRepaired,
		(int)PostInfo::ptRepairing,
		(int)g_Options->GetPostStrategy(),
		&allowParInt);
	*allowPar = allowParInt != 0;
	return canRun != 0;
}

inline NzbInfo* PrePostProcessor::PickNextJob(DownloadQueue* downloadQueue, bool allowPar)
{
	NzbInfo* nzbInfo = nullptr;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);

	bool hasActiveUnpack = HasActiveUnpack(downloadQueue);

	std::vector<NzbInfo*> items;
	std::vector<NzbgetRsPostJobCandidate> candidates;
	items.reserve(downloadQueue->GetQueue()->size());
	candidates.reserve(downloadQueue->GetQueue()->size());
	for (NzbInfo* nzbInfo1 : downloadQueue->GetQueue())
	{
		items.push_back(nzbInfo1);
		NzbgetRsPostJobCandidate c{};
		PostInfo* pi = nzbInfo1->GetPostInfo();
		c.has_post_info = pi ? 1 : 0;
		c.working = (pi && pi->GetWorking()) ? 1 : 0;
		c.direct_unpack_running = nzbInfo1->GetDirectUnpackStatus() == NzbInfo::nsRunning ? 1 : 0;
		c.has_live_repair_thread = nzbInfo1->GetLiveRepairThread() != nullptr ? 1 : 0;
		c.priority = nzbInfo1->GetPriority();
		c.force_priority = nzbInfo1->GetForcePriority() ? 1 : 0;
		c.need_par_check = (pi && pi->GetNeedParCheck()) ? 1 : 0;
		c.need_unpack = (pi && pi->GetNeedUnpack()) ? 1 : 0;
		if (pi && !pi->GetWorking() && !c.direct_unpack_running && !c.has_live_repair_thread)
		{
			c.has_queue_script_job = g_QueueScriptCoordinator->HasJob(nzbInfo1->GetId(), nullptr) ? 1 : 0;
			c.in_active_jobs = std::find(m_activeJobs.begin(), m_activeJobs.end(), nzbInfo1) != m_activeJobs.end() ? 1 : 0;
			c.download_completed = nzbInfo1->IsDownloadCompleted(true) ? 1 : 0;
		}
		candidates.push_back(c);
	}
	int picked = nzbget_rs_prepost_pick_next_post_job(
		candidates.data(),
		candidates.size(),
		allowPar ? 1 : 0,
		hasActiveUnpack ? 1 : 0,
		g_WorkState->GetPausePostProcess() ? 1 : 0);
	if (picked >= 0 && (size_t)picked < items.size())
	{
		nzbInfo = items[(size_t)picked];
	}

	return nzbInfo;
}

inline void PrePostProcessor::CheckPostQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	size_t countBefore;
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		countBefore = m_activeJobs.size();
	}
	CheckRequestPar(downloadQueue);
	CleanupJobs(downloadQueue);
	bool changed;
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		changed = m_activeJobs.size() != countBefore;
	}

	bool allowPar;
	while (CanRunMoreJobs(&allowPar) && !IsStopped())
	{
		NzbInfo* postJob = PickNextJob(downloadQueue, allowPar);
		if (!postJob)
		{
			break;
		}

		{
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			m_activeJobs.push_back(postJob);
		}

		PostInfo* postInfo = postJob->GetPostInfo();
		if (postInfo->GetStage() == PostInfo::ptQueued &&
			(!g_WorkState->GetPausePostProcess() || postInfo->GetNzbInfo()->GetForcePriority()))
		{
			StartJob(downloadQueue, postInfo, allowPar);
			CheckRequestPar(downloadQueue);
			CleanupJobs(downloadQueue);
			changed = true;
		}
	}

	if (changed)
	{
		downloadQueue->Save();
		UpdatePauseState();
	}

	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		Util::SetStandByMode(m_activeJobs.empty());
	}
}

#ifndef DISABLE_PARCHECK
// The collection's par2 files provably cannot cover its damage: the bytes
// still missing from the files they protect exceed all par2 data still
// intact. Health below critical says the same for the whole collection, but
// it also counts damage in files outside the par2 set and never drops for
// partly repaired files, so it only gates the closer look: the remaining
// holes of every stream-repair job whose file the main par2 file lists.
// Several par sets can't be judged from collection-wide sizes, so they are
// always left to par-check.
static inline bool ParCannotCover(NzbInfo* nzbInfo)
{
	int health = nzbInfo->CalcHealth();
	int criticalHealth = nzbInfo->CalcCriticalHealth(false);
	if (health == 0 || health >= criticalHealth)
	{
		return false;
	}

	ParParser::ParFileList mainPars;
	bool foundMainPars = ParParser::FindMainPars(nzbInfo->GetDestDir(), &mainPars);
	if (!foundMainPars || mainPars.size() != 1)
	{
		return false;
	}

	// FindMainPars reports whichever file of the set it meets first; the
	// description packets are cheapest to read from its smallest file (the
	// index, unless every volume was downloaded)
	std::string parPath;
	int64 parFileSize = -1;
	DirBrowser dir(nzbInfo->GetDestDir());
	while (const char* filename = dir.Next())
	{
		if (ParParser::SameParCollection(filename, mainPars[0].c_str(), true))
		{
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
			BString<1024> path;
			path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
			nzbget_rs_free(pathBuf);
			int64 size = FileSystem::FileSize(path);
			if (size >= 0 && (parFileSize < 0 || size < parFileSize))
			{
				parPath = *path;
				parFileSize = size;
			}
		}
	}

	std::vector<std::string> parredFiles;
	if (parFileSize < 0 || !ParParser::ListParredFiles(parPath.c_str(), parredFiles))
	{
		return false;
	}

	int64 protectedDamage = 0;
	for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
	{
		std::string filename = job.GetFilename();
		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetId() == job.GetFileId())
			{
				filename = completedFile.GetFilename();
				break;
			}
		}
		const char* baseName = FileSystem::BaseFileName(filename.c_str());
		if (std::any_of(parredFiles.begin(), parredFiles.end(),
			[baseName](const std::string& parredFile) { return nzbget_rs_dqh_bytes_eq_ignore_case(parredFile.c_str(), baseName) != 0; }))
		{
			// a whole-file job has no decoded geometry yet: its encoded size
			// is the closest measure of its damage
			if (job.GetDecodedFileSize() > 0)
			{
				protectedDamage += DupeStreamRepair::TotalSize(*job.GetHoles());
			}
			else
			{
				protectedDamage += job.GetFailedSize() + job.GetMissedSize();
			}
		}
	}

	int64 intactPar = std::max<int64>(0, nzbInfo->GetParSize() - nzbInfo->GetParCurrentFailedSize());
	return protectedDamage > intactPar;
}

// ParScan=dupe can still find missing blocks in the files of a duplicate
// already downloaded to disk, which the health estimate does not know about
static inline bool HasDupeScanSources(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetParScan() != Options::psDupe)
	{
		return false;
	}
	for (NzbInfo* dupeNzbInfo : g_DupeCoordinator->ListHistoryDupes(downloadQueue, nzbInfo))
	{
		if (!Util::EmptyStr(dupeNzbInfo->GetDestDir()) &&
			FileSystem::DirectoryExists(dupeNzbInfo->GetDestDir()))
		{
			return true;
		}
	}
	return false;
}
#endif

inline void PrePostProcessor::StartJob(DownloadQueue* downloadQueue, PostInfo* postInfo, bool allowPar)
{
	NzbInfo* nzbInfo = postInfo->GetNzbInfo();

	if (!postInfo->GetStartTime())
	{
		postInfo->SetStartTime(Util::CurrentTime());
	}
	postInfo->SetStageTime(Util::CurrentTime());
	postInfo->SetStageProgress(0);
	postInfo->SetFileProgress(0);
	postInfo->SetProgressLabel("");

	if (nzbInfo->GetParRenameStatus() == NzbInfo::rsNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		g_Options->GetParRename())
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptParRenaming);
		RenameController::StartJob(postInfo, RenameController::jkPar);
		return;
	}

	bool streamRepair = g_Options->GetDupeArticleFallback() >= Options::dafStream &&
		!nzbInfo->GetStreamRepairJobs()->empty() &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone;

	// stream repair is the last option (B78): it can take longer than downloading
	// a good backup whole, so with HealthCheck=dupe and a backup still untried in
	// history the download fails over instead (par2 still gets its turn)
	if (streamRepair && !postInfo->GetStreamRepairDone() &&
		g_Options->GetHealthCheck() == Options::hcDupe &&
		DupeCoordinator::FailsOver(nzbInfo) &&
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Not repairing %s from duplicates: a backup in history is tried first", nzbInfo->GetName());
		streamRepair = false;
	}

#ifndef DISABLE_PARCHECK
	// A par-check that cannot succeed still reads the whole collection (and
	// waits for every remaining par2-file): recover from duplicates first,
	// par-check afterwards only if that wrote anything
	if (streamRepair && !postInfo->GetStreamRepairDone() &&
		nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		g_Options->GetParCheck() == Options::pcAuto &&
		ParCannotCover(nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Par2 files of %s cannot cover its damage (health %.1f%%, critical %.1f%%), "
			"recovering from duplicates before par-check",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			nzbInfo->CalcCriticalHealth(false) / 10.0);
		postInfo->SetParCannotCover(true);
		EnterStage(downloadQueue, postInfo, PostInfo::ptStreamRepairing);
		StreamRepairController::StartJob(postInfo);
		return;
	}

	// Duplicates recovered nothing, so the damage is still beyond the par2
	// files: report the failure instead of verifying a collection that
	// cannot be repaired. Any byte ever recovered from duplicates (a live
	// pass, or a pass interrupted by a restart) may have brought the damage
	// within reach, so then par-check still runs.
	if (postInfo->GetParCannotCover() && postInfo->GetStreamRepairDone() &&
		!postInfo->GetStreamRepairRecovered() &&
		nzbInfo->GetDupeRecoveredBytes() <= 0 &&
		g_Options->GetParCheck() == Options::pcAuto &&
		nzbInfo->GetParStatus() <= NzbInfo::psNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		!HasDupeScanSources(downloadQueue, nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping par-check for %s: its par2 files cannot cover the damage "
			"(health %.1f%%, critical %.1f%%) and duplicates recovered nothing",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			nzbInfo->CalcCriticalHealth(false) / 10.0);
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}

	// Captured holes are a fallback, not evidence that donor data is needed.
	// Let the current PAR set verify and repair first, including auto mode
	// which initially marks the check as skipped until damage is known.
	if (streamRepair && nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		postInfo->SetRequestParCheck(true);
		return;
	}

	// damaged, and the par2-files downloaded so far all failed (a lost index file):
	// any par2-volume carries the set's description too, so the smallest one held
	// back is fetched, and the job waits for it (it isn't picked while files download)
	if ((nzbInfo->GetParStatus() == NzbInfo::psNone || nzbInfo->GetParStatus() == NzbInfo::psSkipped) &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		nzbInfo->CalcHealth() < 1000 &&
		!ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		FileInfo* smallest = nullptr;
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (fileInfo->GetParFile() && fileInfo->GetPaused() && !fileInfo->GetDeleted() &&
				(!smallest || fileInfo->GetSize() < smallest->GetSize()))
			{
				smallest = fileInfo;
			}
		}
		if (smallest)
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				"No par2-file of %s on disk to check it with: unpausing %s", nzbInfo->GetName(), smallest->GetFilename());
			smallest->SetPaused(false);
			smallest->SetExtraPriority(true);
			nzbInfo->SetChanged(true);
			downloadQueue->SaveChanged();
			return;
		}
	}

	if (nzbInfo->GetParStatus() == NzbInfo::psNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		if (ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
		{
			if (!allowPar)
			{
				postInfo->SetNeedParCheck(true);
				return;
			}

			EnterStage(downloadQueue, postInfo, PostInfo::ptLoadingPars);
			postInfo->SetNeedParCheck(false);
			RepairController::StartJob(postInfo);
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				"Nothing to par-check for %s", nzbInfo->GetName());
			nzbInfo->SetParStatus(NzbInfo::psSkipped);
		}
		return;
	}
	
	int skippedParAction = nzbget_rs_prepost_classify_skipped_par_health(
		(int)nzbInfo->GetParStatus(),
		(int)NzbInfo::psSkipped,
		(int)g_Options->GetParScan(),
		(int)Options::psDupe,
		nzbInfo->CalcHealth(),
		nzbInfo->CalcCriticalHealth(false),
		nzbInfo->GetFailedSize(),
		nzbInfo->GetParFailedSize());
	if ((skippedParAction == 1 || skippedParAction == 2) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		if (skippedParAction == 1)
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health 0%%", nzbInfo->GetName());
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health %.1f%% below critical %.1f%%",
				nzbInfo->GetName(),
				nzbInfo->CalcHealth() / 10.0,
				nzbInfo->CalcCriticalHealth(false) / 10.0);
		}
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}
	if (skippedParAction == 3 &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s with health %.1f%% needs par-check",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0);
		postInfo->SetRequestParCheck(true);
		return;
	}
#endif

	// Successful PAR sets remove the holes they actually protect. Remaining
	// jobs may belong to unprotected files, or to a set that could not repair.
	// Manual repair and sufficient parity with ParRepair=no remain user choices.
	if (streamRepair &&
		nzbInfo->GetParStatus() != NzbInfo::psRepairPossible &&
		nzbInfo->GetParStatus() != NzbInfo::psManual)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptStreamRepairing);
		StreamRepairController::StartJob(postInfo);
		return;
	}

	NzbParameter* unpackParameter = nzbInfo->GetParameters()->Find("*Unpack:");
	int wantUnpackInt = 0;
	int unpackInt = 0;
	bool startRarRename = nzbget_rs_prepost_eval_want_unpack_and_rar_rename(
		unpackParameter != nullptr ? 1 : 0,
		unpackParameter ? unpackParameter->GetValue() : "",
		(int)nzbInfo->GetUnpackStatus(),
		(int)NzbInfo::usNone,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)nzbInfo->GetRarRenameStatus(),
		(int)NzbInfo::rsNone,
		g_Options->GetRarRename() ? 1 : 0,
		&wantUnpackInt,
		&unpackInt) != 0;
	bool wantUnpack = wantUnpackInt != 0;
	bool unpack = unpackInt != 0;
	if (startRarRename)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptRarRenaming);
		RenameController::StartJob(postInfo, RenameController::jkRar);
		return;
	}

#ifndef DISABLE_PARCHECK
	if (nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		g_Options->GetParCheck() == Options::pcAuto &&
		!UnpackController::HasCompletedArchiveFiles(nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Requesting par-check for collection %s without archive files",
			nzbInfo->GetName());
		postInfo->SetRequestParCheck(true);
		return;
	}
#endif

	NzbgetRsPostStageDecision stageDec{};
	nzbget_rs_prepost_eval_post_stages(
		wantUnpack ? 1 : 0,
		(int)nzbInfo->GetUnpackStatus(),
		(int)nzbInfo->GetDeleteStatus(),
		(int)nzbInfo->GetParStatus(),
		(int)nzbInfo->GetCleanupStatus(),
		!Util::EmptyStr(g_Options->GetExtCleanupDisk()) ? 1 : 0,
		nzbInfo->CalcHealth(),
		(int)nzbInfo->GetMoveStatus(),
		nzbInfo->GetDestDir(),
		g_Options->GetInterDir(),
		PATH_SEPARATOR,
		(int)nzbInfo->GetDirectUnpackStatus(),
		g_Options->GetPostDownloadRename() ? 1 : 0,
		(int)nzbInfo->GetPostDownloadRenamingStatus(),
		!Util::EmptyStr(nzbInfo->GetDestDir()) ? 1 : 0,
		!Util::EmptyStr(nzbInfo->GetName()) ? 1 : 0,
		g_Options->GetRenameAfterUnpack() ? 1 : 0,
		(int)nzbInfo->GetPostUnpackRenamingStatus(),
		&stageDec);
	if (stageDec.skip_unpack_due_to_par)
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping unpack for %s due to %s", nzbInfo->GetName(),
			nzbInfo->GetParStatus() == NzbInfo::psManual ? "required par-repair" : "par-failure");
		nzbInfo->SetUnpackStatus(NzbInfo::usSkipped);
	}
	unpack = stageDec.unpack != 0;
	bool cleanup = stageDec.cleanup != 0;
	bool moveInter = stageDec.move_inter != 0;
	bool postDownloadRenaming = stageDec.post_download_renaming != 0;
	bool postUnpackRenaming = stageDec.post_unpack_renaming != 0;

	if (unpack)
	{
		if (HasActiveUnpack(downloadQueue))
		{
			postInfo->SetNeedUnpack(true);
			return;
		}
		postInfo->SetNeedUnpack(false);
		EnterStage(downloadQueue, postInfo, PostInfo::ptUnpacking);
		UnpackController::StartJob(postInfo);
	}
	else if (cleanup)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptCleaningUp);
		CleanupController::StartJob(postInfo);
	}
	else if (moveInter)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptMoving);
		MoveController::StartJob(postInfo);
	}
	else if (postDownloadRenaming)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptPostDownloadRenaming);
		PostRenamerController::StartJob(postInfo, false);
	}
	else if (postUnpackRenaming)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptPostUnpackRenaming);
		PostRenamerController::StartJob(postInfo, true);
	}
	else
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptExecutingScript);
		PostScriptController::StartJob(postInfo);
	}
}

inline void PrePostProcessor::EnterStage(DownloadQueue* downloadQueue, PostInfo* postInfo, PostInfo::EStage stage)
{
	postInfo->SetWorking(true);
	postInfo->SetStage(stage);
}

inline void PrePostProcessor::JobCompleted(DownloadQueue* downloadQueue, PostInfo* postInfo)
{
	NzbInfo* nzbInfo = postInfo->GetNzbInfo();

	nzbInfo->LeavePostProcess();

	if (nzbInfo->IsDownloadCompleted(true))
	{
		NzbCompleted(downloadQueue, nzbInfo, false);
	}

	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_queuedJobs--;
}

inline void PrePostProcessor::UpdatePauseState()
{
	bool needPause = false;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	needPause = nzbget_rs_prepost_calc_post_pause_state(
		stages.data(),
		stages.size(),
		g_Options->GetParPauseQueue() ? 1 : 0,
		g_Options->GetUnpackPauseQueue() ? 1 : 0,
		g_Options->GetScriptPauseQueue() ? 1 : 0) != 0;

	if (needPause && !g_WorkState->GetTempPauseDownload())
	{
		info("Pausing download before post-processing");
	}
	else if (!needPause && g_WorkState->GetTempPauseDownload())
	{
		info("Unpausing download after post-processing");
	}

	g_WorkState->SetTempPauseDownload(needPause);
}

inline bool PrePostProcessor::EditList(DownloadQueue* downloadQueue, IdList* idList,
	DownloadQueue::EEditAction action, const char* args)
{
	debug("Edit-command for post-processor received");
	switch (action)
	{
		case DownloadQueue::eaPostDelete:
			return PostQueueDelete(downloadQueue, idList);

		default:
			return false;
	}
}

inline bool PrePostProcessor::PostQueueDelete(DownloadQueue* downloadQueue, IdList* idList)
{
	bool ok = false;

	for (int id : *idList)
	{
		for (NzbInfo* nzbInfo: downloadQueue->GetQueue())
		{
			PostInfo* postInfo = nzbInfo->GetPostInfo();
			if (postInfo && nzbInfo->GetId() == id)
			{
				if (postInfo->GetWorking())
				{
					postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
						"Deleting active post-job %s", postInfo->GetNzbInfo()->GetName());
					postInfo->SetDeleted(true);
					if (postInfo->GetPostThread())
					{
						debug("Terminating post-process thread for %s", postInfo->GetNzbInfo()->GetName());
						postInfo->GetPostThread()->Stop();
						ok = true;
					}
					else if (postInfo->GetNzbInfo()->GetUnpackThread())
					{
						((DirectUnpack*)postInfo->GetNzbInfo()->GetUnpackThread())->NzbDeleted(downloadQueue, postInfo->GetNzbInfo());
						ok = true;
					}
					else if (postInfo->GetNzbInfo()->GetLiveRepairThread())
					{
						// the job is waiting for a live stream-repair pass:
						// detach first (the pass aborts at its next locked
						// touchpoint without writing back), stop its fetches
						// and release the job
						Thread* liveRepairThread = postInfo->GetNzbInfo()->GetLiveRepairThread();
						postInfo->GetNzbInfo()->SetLiveRepairThread(nullptr);
						liveRepairThread->Stop();
						postInfo->SetWorking(false);
						ok = true;
					}
					else
					{
						error("Internal error in PrePostProcessor::QueueDelete");
					}
				}
				else
				{
					postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
						"Deleting queued post-job %s", postInfo->GetNzbInfo()->GetName());

					JobCompleted(downloadQueue, postInfo);

					{
						std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
						m_activeJobs.erase(std::remove_if(m_activeJobs.begin(), m_activeJobs.end(),
							[postInfo](NzbInfo* postJob)
							{
								return postInfo == postJob->GetPostInfo();
							}),
							m_activeJobs.end());
					}

					ok = true;
				}
				break;
			}
		}
	}

	if (ok)
	{
		downloadQueue->Save();
	}

	return ok;
}

inline void PrePostProcessor::FileDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileInfo* fileInfo)
{
	if (!nzbInfo->GetPostInfo())
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeFileDownloaded);
	}

	int duAction = nzbget_rs_prepost_classify_file_downloaded_direct_unpack(
		g_Options->GetDirectUnpack() ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		(int)nzbInfo->GetDirectUnpackStatus(),
		(int)NzbInfo::nsNone,
		(int)NzbInfo::nsRunning,
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsRunning,
		nzbInfo->GetUnpackThread() != nullptr ? 1 : 0);
	if (duAction == 1)
	{
		bool allowPar;
		if (DirectUnpack::IsArchiveFilename(fileInfo->GetFilename()) &&
			CanRunMoreJobs(&allowPar) &&
			!HasActiveUnpack(downloadQueue))
		{
			NzbParameter* unpackParameter = nzbInfo->GetParameters()->Find("*Unpack:");
			bool wantUnpack = !unpackParameter || !strcasecmp(unpackParameter->GetValue(), "yes");
			if (wantUnpack && nzbInfo->GetFailedArticles() == 0)
			{
				DirectUnpack::StartJob(nzbInfo);
			}
		}
	}
	else if (duAction == 2)
	{
		((DirectUnpack*)nzbInfo->GetUnpackThread())->FileDownloaded(downloadQueue, fileInfo);
	}
}

#endif
