/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2008-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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


#ifndef SCHEDULER_H
#define SCHEDULER_H
#include "Options.h"
#include "Log.h"
#include "ServerPool.h"
#include "FeedCoordinator.h"
#include "ExtensionManager.h"
#include "nzbget_rs.h"

class SchedulerScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run()
	{
		if (m_externalProcess)
		{
			ExecuteExternalProcess();
		}
		else
		{
			ExecuteScriptList(m_script);
		}
	}

	static void StartScript(const char* param, bool externalProcess, int taskId)
	{
		std::vector<CString> argv;
		if (externalProcess && (argv = Util::SplitCommandLine(param)).empty())
		{
			error("Could not execute scheduled process-script, failed to parse command line: %s", param);
			return;
		}

		SchedulerScriptController* scriptController = new SchedulerScriptController();

		scriptController->m_externalProcess = externalProcess;
		scriptController->m_script = param;
		scriptController->m_taskId = taskId;

		if (externalProcess)
		{
			scriptController->SetArgs(std::move(argv));
		}

		scriptController->SetAutoDestroy(true);

		scriptController->Start();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		if (!script->GetSchedulerScript())
		{
			return;
		}

		BString<1024> taskName;
		if (m_taskId > 0)
		{
			taskName.Format(" for Task%i", m_taskId);
		}
		else
		{
			taskName = "";
		}

		PrintMessage(Message::mkInfo, "Executing scheduler-script %s%s", script->GetName(), *taskName);

		SetArgs({script->GetEntry()});

		BString<1024> infoName("scheduler-script %s%s", script->GetName(), *taskName);
		SetInfoName(infoName);

		SetLogPrefix(script->GetDisplayName());
		PrepareParams(script->GetName());

		Execute();

		SetLogPrefix(nullptr);
	}

private:
	CString m_script;
	bool m_externalProcess;
	int m_taskId;

	void PrepareParams(const char* scriptName)
	{
		ResetEnv();

		SetIntEnvVar("NZBSP_TASKID", m_taskId);

		PrepareEnvScript(nullptr, scriptName);
	}

	void ExecuteExternalProcess()
	{
		info("Executing scheduled process-script %s for Task%i", FileSystem::BaseFileName(GetScript()), m_taskId);

		BString<1024> infoName("scheduled process-script %s for Task%i", FileSystem::BaseFileName(GetScript()), m_taskId);
		SetInfoName(infoName);

		BString<1024> logPrefix = FileSystem::BaseFileName(GetScript());
		if (char* ext = strrchr(logPrefix, '.')) *ext = '\0';
		SetLogPrefix(logPrefix);

		Execute();
	}
};

class Scheduler : public Service
{
public:
	enum ECommand
	{
		scPauseDownload,
		scUnpauseDownload,
		scPausePostProcess,
		scUnpausePostProcess,
		scDownloadRate,
		scExtensions,
		scProcess,
		scPauseScan,
		scUnpauseScan,
		scActivateServer,
		scDeactivateServer,
		scFetchFeed
	};

	class Task
	{
	public:
		Task(int id, int hours, int minutes, int weekDaysBits, ECommand command, const char* param)
			: m_id(id),
			  m_hours(hours),
			  m_minutes(minutes),
			  m_weekDaysBits(weekDaysBits),
			  m_command(command),
			  m_param(param ? param : "")
		{
		}
		friend class Scheduler;
		static const int STARTUP_TASK = -1;
		int GetId() const { return m_id; }
		int GetHours() const { return m_hours; }
		int GetMinutes() const { return m_minutes; }
		int GetWeeDaysBits() const { return m_weekDaysBits; }
		ECommand GetCommand() const { return m_command; }
		const std::string& GetParam() const { return m_param; }

	private:
		const int m_id;
		const int m_hours;
		const int m_minutes;
		const int m_weekDaysBits;
		const ECommand m_command;
		const std::string m_param;
		time_t m_lastExecuted = 0;
	};

	using TaskList = std::vector<std::unique_ptr<Task>>;

	const TaskList& GetTasks() const { return m_taskList; }
	void AddTask(std::unique_ptr<Task> task)
	{
		Guard guard(m_taskListMutex);
		m_taskList.push_back(std::move(task));
	}

protected:
	int ServiceInterval() override { return m_serviceInterval; }
	void ServiceWork() override
	{
		debug("Scheduler service work");

		if (!DownloadQueue::IsLoaded())
		{
			return;
		}

		debug("Scheduler service work: doing work");

		if (!m_firstChecked)
		{
			FirstCheck();
			m_firstChecked = true;
			return;
		}

		m_executeProcess = true;
		CheckTasks();
		CheckScheduledResume();
		ScheduleNextWork();
	}

private:
	using ServerStatusList = std::vector<bool>;

	TaskList m_taskList;
	Mutex m_taskListMutex;
	time_t m_lastCheck = 0;
	bool m_downloadRateChanged;
	bool m_executeProcess;
	bool m_pauseDownloadChanged;
	bool m_pausePostProcessChanged;
	bool m_pauseScanChanged;
	bool m_serverChanged;
	ServerStatusList m_serverStatusList;
	bool m_firstChecked = false;
	int m_serviceInterval = 1;

	void ExecuteTask(Task* task)
	{
#ifdef DEBUG
		debug("Executing scheduled command: %s",
			nzbget_rs_dqh_scheduler_command_name((int)task->m_command));
#endif

		bool executeProcess = nzbget_rs_sfw_scheduler_should_execute_process(
			m_executeProcess ? 1 : 0, task->m_hours, Task::STARTUP_TASK) != 0;

		switch (task->m_command)
		{
			case scDownloadRate:
				if (!task->m_param.empty())
				{
					g_WorkState->SetSpeedLimit(
						nzbget_rs_sfw_scheduler_calc_speed_limit_bytes(
							nzbget_rs_dqh_parse_c_atoi(task->m_param.c_str())));
					m_downloadRateChanged = true;
				}
				break;

			case scPauseDownload:
			case scUnpauseDownload:
				g_WorkState->SetPauseDownload(task->m_command == scPauseDownload);
				m_pauseDownloadChanged = true;
				break;

			case scPausePostProcess:
			case scUnpausePostProcess:
				g_WorkState->SetPausePostProcess(task->m_command == scPausePostProcess);
				m_pausePostProcessChanged = true;
				break;

			case scPauseScan:
			case scUnpauseScan:
				g_WorkState->SetPauseScan(task->m_command == scPauseScan);
				m_pauseScanChanged = true;
				break;

			case scExtensions:
			case scProcess:
				if (executeProcess)
				{
					SchedulerScriptController::StartScript(task->m_param.c_str(), task->m_command == scProcess, task->m_id);
				}
				break;

			case scActivateServer:
			case scDeactivateServer:
				EditServer(task->m_command == scActivateServer, task->m_param.c_str());
				break;

			case scFetchFeed:
				if (executeProcess)
				{
					FetchFeed(task->m_param.c_str());
					break;
				}
		}
	}

	void CheckTasks()
	{
		PrepareLog();

		{
			Guard guard(m_taskListMutex);

			time_t current = Util::CurrentTime();

			std::vector<NzbgetRsSchedTask> tasks;
			tasks.reserve(m_taskList.size());
			for (Task* task : &m_taskList)
			{
				tasks.push_back({task->m_hours, task->m_minutes, task->m_weekDaysBits,
					static_cast<long long>(task->m_lastExecuted)});
			}
			std::vector<size_t> due(m_taskList.size() * 9);
			long long lastCheck = m_lastCheck;
			int reset = 0;
			// StatMeter can change the atomic offset between these two readings.
			// Preserve the original readings and reuse them if the buffer grows.
			int currentOffset = tasks.empty() ? 0 : g_WorkState->GetLocalTimeOffset();
			int lastCheckOffset = tasks.empty() ? 0 : g_WorkState->GetLocalTimeOffset();
			auto calendar = [](long long value, NzbgetRsSchedTm* result)
			{
				time_t time = static_cast<time_t>(value);
				tm fields{};
				gmtime_r(&time, &fields);
				*result = {static_cast<long long>(fields.tm_year) + 1900, fields.tm_mon,
					fields.tm_mday, fields.tm_hour, fields.tm_min, fields.tm_sec, fields.tm_wday};
			};
			size_t dueCount;
			while (true)
			{
				dueCount = nzbget_rs_scheduler_check(tasks.data(), tasks.size(), &lastCheck, current,
					currentOffset, lastCheckOffset, due.data(), due.size(), &reset, calendar);
				if (dueCount <= due.size())
				{
					break;
				}
				// Leap corrections in libc's calendar can exceed the usual count * 9.
				// An undersized output leaves all input state unchanged for this retry.
				due.resize(dueCount);
			}

			if (reset)
			{
				debug("Reset scheduled tasks (detected clock change greater than 90 minutes or negative)");
				m_executeProcess = false;
			}
			for (size_t i = 0; i < tasks.size(); i++)
			{
				m_taskList[i]->m_lastExecuted = static_cast<time_t>(tasks[i].lastExecuted);
			}
			for (size_t i = 0; i < dueCount; i++)
			{
				ExecuteTask(m_taskList[due[i]].get());
			}
			m_lastCheck = static_cast<time_t>(lastCheck);
		}

		PrintLog();
	}

	void PrepareLog()
	{
		m_downloadRateChanged = false;
		m_pauseDownloadChanged = false;
		m_pausePostProcessChanged = false;
		m_pauseScanChanged = false;
		m_serverChanged = false;
	}

	void PrintLog()
	{
		if (m_downloadRateChanged)
		{
			info("Scheduler: setting download rate to %i KB/s",
				nzbget_rs_dqh_scheduler_bytes_to_kb(g_WorkState->GetSpeedLimit()));
		}
		if (m_pauseDownloadChanged)
		{
			info("Scheduler: %s download",
				nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPauseDownload() ? 1 : 0));
		}
		if (m_pausePostProcessChanged)
		{
			info("Scheduler: %s post-processing",
				nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPausePostProcess() ? 1 : 0));
		}
		if (m_pauseScanChanged)
		{
			info("Scheduler: %s scan",
				nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPauseScan() ? 1 : 0));
		}
		if (m_serverChanged)
		{
			int index = 0;
			for (NewsServer* server : g_ServerPool->GetServers())
			{
				if (server->GetActive() != m_serverStatusList[index])
				{
					info("Scheduler: %s %s",
						nzbget_rs_dqh_scheduler_server_action_str(server->GetActive() ? 1 : 0),
						server->GetName());
				}
				index++;
			}
			g_ServerPool->Changed();
		}
	}

	void EditServer(bool active, const char* serverList)
	{
		Tokenizer tok(serverList, ",;");
		while (const char* serverRef = tok.Next())
		{
			int id = nzbget_rs_dqh_parse_c_atoi(serverRef);
			for (NewsServer* server : g_ServerPool->GetServers())
			{
				if (nzbget_rs_sfw_scheduler_matches_server_ref(
					id, server->GetId(), server->GetName(), serverRef) != 0)
				{
					if (!m_serverChanged)
					{
						// store old server status for logging
						m_serverStatusList.clear();
						m_serverStatusList.reserve(g_ServerPool->GetServers()->size());
						for (NewsServer* server2 : g_ServerPool->GetServers())
						{
							m_serverStatusList.push_back(server2->GetActive());
						}
					}
					m_serverChanged = true;
					server->SetActive(active);
					break;
				}
			}
		}
	}

	void FetchFeed(const char* feedList)
	{
		Tokenizer tok(feedList, ",;");
		while (const char* feedRef = tok.Next())
		{
			int id = nzbget_rs_dqh_parse_c_atoi(feedRef);
			for (FeedInfo* feed : g_FeedCoordinator->GetFeeds())
			{
				int targetFeedId = 0;
				if (nzbget_rs_sfw_scheduler_matches_feed_ref(
					id, feed->GetId(), feed->GetName(), feedRef, &targetFeedId) != 0)
				{
					g_FeedCoordinator->FetchFeed(targetFeedId);
					break;
				}
			}
		}
	}

	void CheckScheduledResume()
	{
		time_t resumeTime = g_WorkState->GetResumeTime();
		time_t currentTime = Util::CurrentTime();
		if (nzbget_rs_sfw_scheduler_should_autoresume((int64_t)resumeTime, (int64_t)currentTime) != 0)
		{
			info("Autoresume");
			g_WorkState->SetResumeTime(0);
			g_WorkState->SetPauseDownload(false);
			g_WorkState->SetPausePostProcess(false);
			g_WorkState->SetPauseScan(false);
		}
	}

	void FirstCheck()
	{
		{
			Guard guard(m_taskListMutex);

			std::sort(m_taskList.begin(), m_taskList.end(),
				[](const std::unique_ptr<Task>& task1, const std::unique_ptr<Task>& task2)
			{
				return nzbget_rs_sfw_scheduler_task_less(
					task1->m_hours, task1->m_minutes, task2->m_hours, task2->m_minutes) != 0;
			});
		}

		// check all tasks for the last week
		CheckTasks();
	}

	void ScheduleNextWork()
	{
		m_serviceInterval = nzbget_rs_scheduler_calc_next_work_interval(
			static_cast<int64_t>(Util::CurrentTime()));
	}
};

#ifndef SYSTEM_HEALTH_H
#define SYSTEM_HEALTH_H

#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace SystemHealth
{
class Service
{
public:
	Service(const Options& options, const Servers& servers, const ::Feeds& feeds,
			const ::Scheduler::TaskList& tasks, const ::Log& log)
		: m_options(options), m_servers(servers), m_feeds(feeds), m_tasks(tasks), m_log(log)
	{
	}

	void DiagnoseAndLog() const
	{
		NzbgetRsBuf buf = RunDiagnose(0);
		nzbget_rs_free(buf);
	}

	NzbgetRsBuf DiagnoseSerialized(bool isJson) const
	{
		return RunDiagnose(isJson ? 1 : 2);
	}

private:
	NzbgetRsBuf RunDiagnose(int formatMode) const
	{
		std::string mainDir = m_options.GetMainDirPath().string();
		std::string destDir = m_options.GetDestDirPath().string();
		std::string interDir = m_options.GetInterDirPath().string();
		std::string nzbDir = m_options.GetNzbDirPath().string();
		std::string queueDir = m_options.GetQueueDirPath().string();
		std::string webDir = m_options.GetWebDirPath().string();
		std::string tempDir = m_options.GetTempDirPath().string();
		std::string configTemplate = m_options.GetConfigTemplatePath().string();
		std::string configFile = m_options.GetConfigFilePath().string();
		std::string logFile = m_log.GetLogFilePath().string();
		std::string optionsLogFile = m_options.GetLogFilePath().string();
		std::string_view certStore = m_options.GetCertStore();
		std::string certStorePath = m_options.GetCertStorePath().string();
		std::string lockFile = m_options.GetLockFilePath().string();
		std::string unrarPath = m_options.GetUnrarPath().string();
		std::string sevenZipPath = m_options.GetSevenZipPath().string();

		const auto& scriptDirPaths = m_options.GetScriptDirPaths();
		std::vector<std::string> scriptDirStrs;
		scriptDirStrs.reserve(scriptDirPaths.size());
		std::vector<const char*> scriptDirPtrs;
		std::vector<size_t> scriptDirLens;
		scriptDirPtrs.reserve(scriptDirPaths.size());
		scriptDirLens.reserve(scriptDirPaths.size());
		for (const auto& dir : scriptDirPaths)
		{
			scriptDirStrs.push_back(dir.string());
			scriptDirPtrs.push_back(scriptDirStrs.back().data());
			scriptDirLens.push_back(scriptDirStrs.back().size());
		}

		std::vector<NzbgetRsHealthServerInput> servers;
		servers.reserve(m_servers.size());
		for (const auto& server : m_servers)
		{
			std::string_view name = server->GetName();
			std::string_view host = server->GetHost();
			std::string_view user = server->GetUser();
			std::string_view pass = server->GetPassword();
			std::string_view cipher = server->GetCipher();
			servers.push_back(NzbgetRsHealthServerInput{
				server->GetId(),
				server->GetActive() ? 1 : 0,
				name.data(), name.size(),
				server->GetLevel(),
				server->GetOptional() ? 1 : 0,
				server->GetGroup(),
				host.data(), host.size(),
				server->GetPort(),
				user.data(), user.size(),
				pass.data(), pass.size(),
				server->GetTls() ? 1 : 0,
				cipher.data(), cipher.size(),
				server->GetMaxConnections(),
				server->GetRetention(),
				static_cast<uint32_t>(server->GetCertVerificationLevel()),
				server->GetIpVersion()
			});
		}

		std::vector<NzbgetRsHealthTaskInput> tasks;
		tasks.reserve(m_tasks.size());
		for (const auto& task : m_tasks)
		{
			tasks.push_back(NzbgetRsHealthTaskInput{
				task->GetId(),
				task->GetHours(),
				task->GetMinutes(),
				task->GetWeeDaysBits()
			});
		}

		std::vector<NzbgetRsHealthCategoryInput> categories;
		if (const auto* cats = m_options.GetCategories())
		{
			categories.reserve(cats->size());
			for (const auto& cat : *cats)
			{
				std::string_view name = cat.GetName();
				categories.push_back(NzbgetRsHealthCategoryInput{
					name.data(), name.size(),
					cat.GetUnpack() ? 1 : 0
				});
			}
		}

		std::vector<NzbgetRsHealthFeedInput> feeds;
		feeds.reserve(m_feeds.size());
		for (const auto& feed : m_feeds)
		{
			std::string_view name = feed->GetName();
			std::string_view url = feed->GetUrl();
			feeds.push_back(NzbgetRsHealthFeedInput{
				feed->GetId(),
				name.data(), name.size(),
				url.data(), url.size(),
				feed->GetInterval(),
				static_cast<uint32_t>(feed->GetCertVerificationLevel())
			});
		}

		std::string_view controlIp = m_options.GetControlIp();
		std::string_view controlUser = m_options.GetControlUsername();
		std::string_view controlPass = m_options.GetControlPassword();
		std::string_view restrictedUser = m_options.GetRestrictedUsername();
		std::string_view restrictedPass = m_options.GetRestrictedPassword();
		std::string_view addUser = m_options.GetAddUsername();
		std::string_view addPass = m_options.GetAddPassword();
		std::string_view secureCert = m_options.GetSecureCert();
		std::string_view secureKey = m_options.GetSecureKey();
		std::string_view daemonUser = m_options.GetDaemonUsername();
		std::string_view extCleanup = m_options.GetExtCleanupDisk();
		std::string_view unpackPassFile = m_options.GetUnpackPassFile();
		std::string_view unpackIgnoreExt = m_options.GetUnpackIgnoreExt();

		NzbgetRsSystemHealthConfig cfg{};
		cfg.main_dir = mainDir.data();
		cfg.main_dir_len = mainDir.size();
		cfg.dest_dir = destDir.data();
		cfg.dest_dir_len = destDir.size();
		cfg.inter_dir = interDir.data();
		cfg.inter_dir_len = interDir.size();
		cfg.nzb_dir = nzbDir.data();
		cfg.nzb_dir_len = nzbDir.size();
		cfg.queue_dir = queueDir.data();
		cfg.queue_dir_len = queueDir.size();
		cfg.web_dir = webDir.data();
		cfg.web_dir_len = webDir.size();
		cfg.temp_dir = tempDir.data();
		cfg.temp_dir_len = tempDir.size();
		cfg.script_dir_ptrs = scriptDirPtrs.data();
		cfg.script_dir_lens = scriptDirLens.data();
		cfg.script_dir_count = scriptDirPaths.size();
		cfg.config_template = configTemplate.data();
		cfg.config_template_len = configTemplate.size();
		cfg.log_file = logFile.data();
		cfg.log_file_len = logFile.size();
		cfg.options_log_file = optionsLogFile.data();
		cfg.options_log_file_len = optionsLogFile.size();
		cfg.cert_store = certStore.data();
		cfg.cert_store_len = certStore.size();
		cfg.cert_store_path = certStorePath.data();
		cfg.cert_store_path_len = certStorePath.size();
		cfg.config_file = configFile.data();
		cfg.config_file_len = configFile.size();
		cfg.lock_file = lockFile.data();
		cfg.lock_file_len = lockFile.size();
		cfg.write_log = static_cast<int>(m_options.GetWriteLog());
		cfg.cert_check = m_options.GetCertCheck() ? 1 : 0;
		cfg.daemon_mode = m_options.GetDaemonMode() ? 1 : 0;
		cfg.servers = servers.data();
		cfg.server_count = servers.size();
		cfg.output_mode = static_cast<int>(m_options.GetOutputMode());
		cfg.update_interval = m_options.GetUpdateInterval();
		cfg.curses_nzb_name = m_options.GetCursesNzbName() ? 1 : 0;
		cfg.curses_group = m_options.GetCursesGroup() ? 1 : 0;
		cfg.curses_time = m_options.GetCursesTime() ? 1 : 0;
		cfg.rotate_log = m_options.GetRotateLog();
		cfg.log_buffer = m_options.GetLogBuffer();
		cfg.crash_dump = m_options.GetCrashDump() ? 1 : 0;
		cfg.time_correction = m_options.GetTimeCorrection();
		cfg.article_retries = m_options.GetArticleRetries();
		cfg.hc_dupe = m_options.GetHealthCheck() == Options::hcDupe ? 1 : 0;
		cfg.dupe_search = m_options.GetDupeSearch() ? 1 : 0;
		cfg.article_interval = m_options.GetArticleInterval();
		cfg.article_timeout = m_options.GetArticleTimeout();
		cfg.article_read_chunk_size = m_options.GetArticleReadChunkSize();
		cfg.url_retries = m_options.GetUrlRetries();
		cfg.url_interval = m_options.GetUrlInterval();
		cfg.url_timeout = m_options.GetUrlTimeout();
		cfg.url_connections = m_options.GetUrlConnections();
		cfg.remote_timeout = m_options.GetRemoteTimeout();
		cfg.download_rate = m_options.GetDownloadRate();
		cfg.monthly_quota = m_options.GetMonthlyQuota();
		cfg.quota_start_day = m_options.GetQuotaStartDay();
		cfg.daily_quota = m_options.GetDailyQuota();
		cfg.flush_queue = m_options.GetFlushQueue() ? 1 : 0;
		cfg.continue_partial = m_options.GetContinuePartial() ? 1 : 0;
		cfg.propagation_delay = m_options.GetPropagationDelay();
		cfg.article_cache = m_options.GetArticleCache();
		cfg.direct_write = m_options.GetDirectWrite() ? 1 : 0;
		cfg.write_buffer = m_options.GetWriteBuffer();
		cfg.file_naming = static_cast<int>(m_options.GetFileNaming());
		cfg.post_strategy = static_cast<int>(m_options.GetPostStrategy());
		cfg.disk_space = m_options.GetDiskSpace();
		cfg.nzb_cleanup_disk = m_options.GetNzbCleanupDisk() ? 1 : 0;
		cfg.keep_history = m_options.GetKeepHistory();
		cfg.feed_history = m_options.GetFeedHistory();
		cfg.skip_write = m_options.GetSkipWrite() ? 1 : 0;
		cfg.raw_article = m_options.GetRawArticle() ? 1 : 0;
		cfg.control_ip = controlIp.data();
		cfg.control_ip_len = controlIp.size();
		cfg.control_port = m_options.GetControlPort();
		cfg.control_username = controlUser.data();
		cfg.control_username_len = controlUser.size();
		cfg.control_password = controlPass.data();
		cfg.control_password_len = controlPass.size();
		cfg.restricted_username = restrictedUser.data();
		cfg.restricted_username_len = restrictedUser.size();
		cfg.restricted_password = restrictedPass.data();
		cfg.restricted_password_len = restrictedPass.size();
		cfg.add_username = addUser.data();
		cfg.add_username_len = addUser.size();
		cfg.add_password = addPass.data();
		cfg.add_password_len = addPass.size();
		cfg.form_auth = m_options.GetFormAuth() ? 1 : 0;
		cfg.secure_control = m_options.GetSecureControl() ? 1 : 0;
		cfg.secure_cert = secureCert.data();
		cfg.secure_cert_len = secureCert.size();
		cfg.secure_key = secureKey.data();
		cfg.secure_key_len = secureKey.size();
		cfg.secure_port = m_options.GetSecurePort();
		cfg.cert_store_path_empty = m_options.GetCertStorePath().empty() ? 1 : 0;
		cfg.daemon_username = daemonUser.data();
		cfg.daemon_username_len = daemonUser.size();
		cfg.umask = m_options.GetUMask();
		cfg.append_category_dir = m_options.GetAppendCategoryDir() ? 1 : 0;
		cfg.nzb_dir_interval = m_options.GetNzbDirInterval();
		cfg.nzb_dir_file_age = m_options.GetNzbDirFileAge();
		cfg.dupe_check = m_options.GetDupeCheck() ? 1 : 0;
		cfg.health_check = static_cast<int>(m_options.GetHealthCheck());
		cfg.crc_check = m_options.GetCrcCheck() ? 1 : 0;
		cfg.par_check = static_cast<int>(m_options.GetParCheck());
		cfg.par_repair = m_options.GetParRepair() ? 1 : 0;
		cfg.par_quick = m_options.GetParQuick() ? 1 : 0;
		cfg.par_buffer = m_options.GetParBuffer();
		cfg.par_threads = m_options.GetParThreads();
		cfg.hw_threads = std::thread::hardware_concurrency();
		cfg.par_rename = m_options.GetParRename() ? 1 : 0;
		cfg.rar_rename = m_options.GetRarRename() ? 1 : 0;
		cfg.direct_rename = m_options.GetDirectRename() ? 1 : 0;
		cfg.hard_linking = m_options.GetHardLinking() ? 1 : 0;
		cfg.par_time_limit = m_options.GetParTimeLimit();
		cfg.unpack = m_options.GetUnpack() ? 1 : 0;
		cfg.direct_unpack = m_options.GetDirectUnpack() ? 1 : 0;
		cfg.unpack_pause_queue = m_options.GetUnpackPauseQueue() ? 1 : 0;
		cfg.unrar_path = unrarPath.data();
		cfg.unrar_path_len = unrarPath.size();
		cfg.seven_zip_path = sevenZipPath.data();
		cfg.seven_zip_path_len = sevenZipPath.size();
		cfg.ext_cleanup_disk = extCleanup.data();
		cfg.ext_cleanup_disk_len = extCleanup.size();
		cfg.unpack_pass_file = unpackPassFile.data();
		cfg.unpack_pass_file_len = unpackPassFile.size();
		cfg.unpack_ignore_ext = unpackIgnoreExt.data();
		cfg.unpack_ignore_ext_len = unpackIgnoreExt.size();
		cfg.tasks = tasks.data();
		cfg.task_count = tasks.size();
		cfg.categories = categories.data();
		cfg.category_count = categories.size();
		cfg.feeds = feeds.data();
		cfg.feed_count = feeds.size();

		auto logCb = [](void*, int sev, const char* msg)
		{
			if (!msg) return;
			if (sev == 2)
			{
				error("%s", msg);
			}
			else if (sev == 1)
			{
				warn("%s", msg);
			}
		};

		return nzbget_rs_system_health_diagnose(&cfg, formatMode, nullptr, logCb);
	}

	const Options& m_options;
	const Servers& m_servers;
	const ::Feeds& m_feeds;
	const ::Scheduler::TaskList& m_tasks;
	const ::Log& m_log;
};
}  // namespace SystemHealth

extern SystemHealth::Service* g_SystemHealth;

#endif

#ifndef MAINTENANCE_H
#define MAINTENANCE_H

#include "ScriptController.h"

extern void ExitProc();
extern int g_ArgumentCount;
extern char* (*g_Arguments)[];

class UpdateScriptController;

class UpdateInfoScriptController : public ScriptController
{
public:
	static void ExecuteScript(const char* script, CString& updateInfo)
	{
		detail("Executing update-info-script %s", FileSystem::BaseFileName(script));

		UpdateInfoScriptController scriptController;
		scriptController.SetArgs({script});

		const char* baseScript = FileSystem::BaseFileName(script);
		{
			NzbgetRsBuf infoBuf = nzbget_rs_dqh_maintenance_format_script_info_name(1, baseScript);
			scriptController.SetInfoName(infoBuf.data ? infoBuf.data : "");
			nzbget_rs_free(infoBuf);
		}

		size_t prefixLen = 0;
		{
			NzbgetRsBuf lpBuf = nzbget_rs_dqh_maintenance_strip_ext_and_calc_prefix_len(
				baseScript, 1023, &prefixLen);
			scriptController.SetLogPrefix(lpBuf.data ? lpBuf.data : "");
			nzbget_rs_free(lpBuf);
		}
		scriptController.m_prefixLen = (int)prefixLen;

		scriptController.Execute();

		if (!scriptController.m_updateInfo.Empty())
		{
			updateInfo = scriptController.m_updateInfo;
		}
	}

private:
	int m_prefixLen;
	StringBuilder m_updateInfo;

protected:
	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		text = text + m_prefixLen;

		int infoKind = nzbget_rs_feh_maintenance_classify_update_info_message(text);
		if (infoKind != 0)
		{
			debug("Command %s detected", text + 6);
			if (infoKind == 1)
			{
				m_updateInfo.Append(text + 6 + 12);
			}
			else
			{
				error("Invalid command \"%s\" received from %s", text, GetInfoName());
			}
		}
		else
		{
			ScriptController::AddMessage(kind, text);
		}
	}
};

class Maintenance
{
public:
	enum EBranch
	{
		brStable,
		brTesting,
		brDevel
	};

	~Maintenance();

	void AddMessage(Message::EKind kind, time_t time, const char* text)
	{
		if (time == 0)
		{
			time = Util::CurrentTime();
		}

		Guard guard(m_logMutex);
		m_messages.emplace_back(++m_idMessageGen, kind, time, text);
	}

	GuardedMessageList GuardMessages() { return GuardedMessageList(&m_messages, &m_logMutex); }
	bool StartUpdate(EBranch branch);

	void ResetUpdateController()
	{
		Guard guard(m_controllerMutex);
		m_updateScriptController = nullptr;
	}

	bool CheckUpdates(CString& updateInfo)
	{
		CString updateInfoScript;
		if (!ReadPackageInfoStr("update-info-script", updateInfoScript))
		{
			return false;
		}

		UpdateInfoScriptController::ExecuteScript(updateInfoScript, updateInfo);

		return nzbget_rs_dqh_maintenance_has_update_info(updateInfo.Length()) != 0;
	}

	static bool VerifySignature(const char* inFilename, const char* sigFilename, const char* pubKeyFilename)
	{
#ifndef DISABLE_TLS
		uint8_t inHash[32]{};
		uint8_t signature[256]{};
		NzbgetRsBuf pubKeyBuf{};
		if (!nzbget_rs_maintenance_prepare_signature_inputs(
			inFilename, sigFilename, pubKeyFilename, inHash, signature, &pubKeyBuf))
		{
			nzbget_rs_free(pubKeyBuf);
			return false;
		}
		BIO* mem = BIO_new_mem_buf(pubKeyBuf.data, static_cast<int>(pubKeyBuf.len));
		EVP_PKEY* pubKey = PEM_read_bio_PUBKEY(mem, nullptr, nullptr, nullptr);
		BIO_free(mem);
		nzbget_rs_free(pubKeyBuf);
		if (!pubKey)
		{
			return false;
		}
		EVP_PKEY_CTX* pctx = EVP_PKEY_CTX_new(pubKey, nullptr);
		if (!pctx)
		{
			EVP_PKEY_free(pubKey);
			return false;
		}
		bool ok = EVP_PKEY_verify_init(pctx) == 1 &&
			EVP_PKEY_CTX_set_rsa_padding(pctx, RSA_PKCS1_PADDING) > 0 &&
			EVP_PKEY_CTX_set_signature_md(pctx, EVP_sha256()) > 0 &&
			EVP_PKEY_verify(pctx, signature, sizeof(signature), inHash, sizeof(inHash)) == 1;
		EVP_PKEY_CTX_free(pctx);
		EVP_PKEY_free(pubKey);
		return ok;
#else
		(void)inFilename;
		(void)sigFilename;
		(void)pubKeyFilename;
		return false;
#endif
	}

private:
	MessageList m_messages;
	Mutex m_logMutex;
	Mutex m_controllerMutex;
	int m_idMessageGen = 0;
	UpdateScriptController* m_updateScriptController = nullptr;
	CString m_updateScript;

	bool ReadPackageInfoStr(const char* key, CString& value)
	{
		int errKind = 0;
		NzbgetRsBuf fnBuf{};
		NzbgetRsBuf valBuf = nzbget_rs_maintenance_read_package_info_str(
			g_Options->GetWebDir(), PATH_SEPARATOR, key, &errKind, &fnBuf);
		if (errKind != 0)
		{
			const char* fn = fnBuf.data ? fnBuf.data : "";
			if (errKind == 1)
			{
				error("Could not load file %s", fn);
			}
			else
			{
				error("Could not parse file %s", fn);
			}
			nzbget_rs_free(fnBuf);
			nzbget_rs_free(valBuf);
			return false;
		}
		value.Set(valBuf.data ? valBuf.data : "", static_cast<int>(valBuf.len));
		nzbget_rs_free(fnBuf);
		nzbget_rs_free(valBuf);
		return true;
	}
};

extern Maintenance* g_Maintenance;

class UpdateScriptController : public Thread, public ScriptController
{
public:
	virtual void Run()
	{
		UnregisterRunningScript();

		m_prefixLen = 0;
		PrintMessage(Message::mkInfo, "Executing update-script %s", GetScript());

		{
			NzbgetRsBuf infoBuf = nzbget_rs_dqh_maintenance_format_script_info_name(
				0, FileSystem::BaseFileName(GetScript()));
			SetInfoName(infoBuf.data ? infoBuf.data : "");
			nzbget_rs_free(infoBuf);
		}

		SetEnvVar("NZBUP_BRANCH", nzbget_rs_dqh_maintenance_branch_name((int)m_branch));

		SetEnvVar("NZBUP_RUNMODE", nzbget_rs_dqh_maintenance_run_mode_name(g_Options->GetDaemonMode() ? 1 : 0));

		for (int i = 0; i < g_ArgumentCount; i++)
		{
			NzbgetRsBuf envBuf = nzbget_rs_dqh_maintenance_format_cmdline_env_name(i);
			SetEnvVar(envBuf.data ? envBuf.data : "", (*g_Arguments)[i]);
			nzbget_rs_free(envBuf);
		}

		int pid = (int)getpid();

		{
			NzbgetRsBuf pidBuf = nzbget_rs_dqh_conn_format_port_str(pid);
			SetEnvVar("NZBUP_PROCESSID", pidBuf.data ? pidBuf.data : "");
			nzbget_rs_free(pidBuf);
		}

		const char* baseScript = FileSystem::BaseFileName(GetScript());
		size_t prefixLen = 0;
		{
			NzbgetRsBuf lpBuf = nzbget_rs_dqh_maintenance_strip_ext_and_calc_prefix_len(
				baseScript, 99, &prefixLen);
			SetLogPrefix(lpBuf.data ? lpBuf.data : "");
			nzbget_rs_free(lpBuf);
		}
		m_prefixLen = (int)prefixLen;

		Execute();

		g_Maintenance->ResetUpdateController();
	}

	void SetBranch(Maintenance::EBranch branch) { m_branch = branch; }

protected:
	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		text = text + m_prefixLen;

		int msgKind = nzbget_rs_feh_maintenance_classify_update_message(text);
		if (msgKind != 0)
		{
			debug("Command %s detected", text + 6);
			if (msgKind == 1)
			{
				Detach();
				ExitProc();
			}
			else
			{
				error("Invalid command \"%s\" received", text);
			}
		}
		else
		{
			g_Maintenance->AddMessage(kind, Util::CurrentTime(), text);
			ScriptController::AddMessage(kind, text);
		}
	}

private:
	Maintenance::EBranch m_branch;
	int m_prefixLen;
};

inline Maintenance::~Maintenance()
{
	bool waitScript = false;
	{
		Guard guard(m_controllerMutex);
		if (m_updateScriptController)
		{
			m_updateScriptController->Detach();
			waitScript = true;
		}
	}

	if (waitScript)
	{
		while (m_updateScriptController)
		{
			Util::Sleep(20);
		}
	}
}

inline bool Maintenance::StartUpdate(EBranch branch)
{
	Guard guard(m_controllerMutex);

	if (m_updateScriptController)
	{
		error("Could not start update-script: update-script is already running");
		return false;
	}

	if (!ReadPackageInfoStr("install-script", m_updateScript))
	{
		return false;
	}

	{
		NzbgetRsBuf scriptPathBuf = nzbget_rs_dqh_maintenance_resolve_script_path(
			g_Options->GetAppDir(), PATH_SEPARATOR, *m_updateScript);
		m_updateScript.Set(scriptPathBuf.data ? scriptPathBuf.data : "", (int)scriptPathBuf.len);
		nzbget_rs_free(scriptPathBuf);
	}

	{
		Guard logGuard(m_logMutex);
		m_messages.clear();
	}

	m_updateScriptController = new UpdateScriptController();
	m_updateScriptController->SetArgs({*m_updateScript});
	m_updateScriptController->SetBranch(branch);
	m_updateScriptController->SetAutoDestroy(true);

	m_updateScriptController->Start();

	return true;
}

#endif

#endif
