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

#include "ScriptController.h"
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
#include "FileSystem.h"
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
