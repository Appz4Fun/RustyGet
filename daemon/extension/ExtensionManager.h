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
#include "Options.h"
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
#include "Log.h"

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
#include "Options.h"
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

#include "DownloadInfo.h"
#include "Log.h"

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

#include "DownloadInfo.h"

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
#include "Options.h"
#include "Log.h"
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
