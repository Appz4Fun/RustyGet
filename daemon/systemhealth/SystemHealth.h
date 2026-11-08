/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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

#ifndef SYSTEM_HEALTH_H
#define SYSTEM_HEALTH_H

#include <string>
#include <string_view>
#include <thread>
#include <vector>
#include "Options.h"
#include "NewsServer.h"
#include "FeedInfo.h"
#include "Scheduler.h"
#include "Log.h"
#include "nzbget_rs.h"

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
