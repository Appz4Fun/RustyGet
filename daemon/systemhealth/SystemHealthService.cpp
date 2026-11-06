/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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

#include "nzbget.h"

#include <thread>
#include "ExtensionManager.h"
#include "SystemHealthService.h"
#include "Log.h"
#include "Json.h"
#include "Xml.h"
#include "nzbget_rs.h"

namespace SystemHealth
{
namespace
{
void AppendOptionCallback(
	void* ctx,
	const char* opt_ptr,
	size_t opt_len,
	int severity,
	const char* msg_ptr,
	size_t msg_len)
{
	auto* options = static_cast<std::vector<OptionStatus>*>(ctx);
	Severity sev = Severity::Ok;
	if (severity == 1) sev = Severity::Info;
	else if (severity == 2) sev = Severity::Warning;
	else if (severity == 3) sev = Severity::Error;
	if (sev != Severity::Ok)
	{
		options->push_back({
			std::string(opt_ptr ? opt_ptr : "", opt_len),
			Status::FromRaw(sev, std::string(msg_ptr ? msg_ptr : "", msg_len))
		});
	}
}

void AppendIssueCallback(
	void* ctx,
	const char*,
	size_t,
	int severity,
	const char* msg_ptr,
	size_t msg_len)
{
	auto* issues = static_cast<std::vector<Status>*>(ctx);
	Severity sev = Severity::Ok;
	if (severity == 1) sev = Severity::Info;
	else if (severity == 2) sev = Severity::Warning;
	else if (severity == 3) sev = Severity::Error;
	if (sev != Severity::Ok)
	{
		issues->push_back(Status::FromRaw(sev, std::string(msg_ptr ? msg_ptr : "", msg_len)));
	}
}
}  // namespace

std::string_view SeverityToStr(Severity severity)
{
	return nzbget_rs_health_severity_to_str(static_cast<int>(severity));
}

Alert::Alert(Severity severity, std::string category, std::string source, std::string message)
	: m_severity(severity)
	, m_category(std::move(category))
	, m_source(std::move(source))
	, m_message(std::move(message))
	, m_timestamp{std::chrono::system_clock::now()}
{}

Service::Service(const Options& options, const Servers& servers, const ::Feeds& feeds,
				 const ::Scheduler::TaskList& tasks, const ::Log& log)
	: m_options(options), m_servers(servers), m_feeds(feeds), m_tasks(tasks), m_log(log)
{
}

HealthReport Service::Diagnose() const
{
	HealthReport report;
	report.sections.reserve(14);

	{
		std::lock_guard<std::mutex> lock(m_mutex);
		report.alerts = m_alerts;
	}

	// 1. Paths
	{
		SectionReport sec;
		sec.name = "Paths";
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
		std::string_view certStore = m_options.GetCertStore();
		std::string certStorePath = m_options.GetCertStorePath().string();
		std::string lockFile = m_options.GetLockFilePath().string();

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

		nzbget_rs_health_validate_paths_section(
			mainDir.data(), mainDir.size(),
			destDir.data(), destDir.size(),
			interDir.data(), interDir.size(),
			nzbDir.data(), nzbDir.size(),
			queueDir.data(), queueDir.size(),
			webDir.data(), webDir.size(),
			tempDir.data(), tempDir.size(),
			scriptDirPtrs.data(), scriptDirLens.data(), scriptDirPaths.size(),
			configTemplate.data(), configTemplate.size(),
			logFile.data(), logFile.size(),
			certStore.data(), certStore.size(),
			certStorePath.data(), certStorePath.size(),
			configFile.data(), configFile.size(),
			lockFile.data(), lockFile.size(),
			static_cast<int>(m_options.GetWriteLog()),
			m_options.GetCertCheck() ? 1 : 0,
			m_options.GetDaemonMode() ? 1 : 0,
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 2. NewsServers
	{
		SectionReport sec;
		sec.name = "NewsServers";
		sec.subsections.reserve(m_servers.size());
		std::vector<const char*> hostPtrs;
		std::vector<size_t> hostLens;
		std::vector<uint8_t> actives;
		std::vector<int32_t> maxConns;
		std::vector<int32_t> levels;
		hostPtrs.reserve(m_servers.size());
		hostLens.reserve(m_servers.size());
		actives.reserve(m_servers.size());
		maxConns.reserve(m_servers.size());
		levels.reserve(m_servers.size());

		for (const auto& server : m_servers)
		{
			SubsectionReport sub;
			sub.name = "Server" + std::to_string(server->GetId());
			std::string_view name = server->GetName();
			std::string_view host = server->GetHost();
			std::string_view user = server->GetUser();
			std::string_view pass = server->GetPassword();
			std::string_view cipher = server->GetCipher();

			nzbget_rs_health_validate_news_server_section(
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
				server->GetIpVersion(),
				(g_Options && g_Options->GetDupeSearch()) ? 1 : 0,
				&sub.options,
				&AppendOptionCallback);
			sec.subsections.push_back(std::move(sub));

			hostPtrs.push_back(host.data());
			hostLens.push_back(host.size());
			actives.push_back(server->GetActive() ? 1 : 0);
			maxConns.push_back(server->GetMaxConnections());
			levels.push_back(server->GetLevel());
		}

		nzbget_rs_health_validate_news_servers_group(
			hostPtrs.data(),
			hostLens.data(),
			actives.data(),
			maxConns.data(),
			levels.data(),
			m_servers.size(),
			&sec.issues,
			&AppendIssueCallback);
		report.sections.push_back(std::move(sec));
	}

	// 3. Display
	{
		SectionReport sec;
		sec.name = "Display";
		nzbget_rs_health_validate_display_section(
			static_cast<int>(m_options.GetOutputMode()),
			m_options.GetUpdateInterval(),
			m_options.GetCursesNzbName() ? 1 : 0,
			m_options.GetCursesGroup() ? 1 : 0,
			m_options.GetCursesTime() ? 1 : 0,
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 4. Logging
	{
		SectionReport sec;
		sec.name = "Logging";
		std::string logFile = m_options.GetLogFilePath().string();
		nzbget_rs_health_validate_logging_section(
			static_cast<int>(m_options.GetWriteLog()),
			logFile.data(), logFile.size(),
			m_options.GetRotateLog(),
			m_options.GetLogBuffer(),
			m_options.GetCrashDump() ? 1 : 0,
			m_options.GetTimeCorrection(),
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 5. ExtensionScripts
	{
		SectionReport sec;
		sec.name = "ExtensionScripts";
		std::string_view extensions = m_options.GetExtensions();
		std::string_view shellOverride = m_options.GetShellOverride();

		std::vector<ExtensionManager::ExtensionPtr> extHolders;
		std::vector<const char*> extNamePtrs;
		std::vector<size_t> extNameLens;
		std::vector<const char*> extEntryPtrs;
		std::vector<size_t> extEntryLens;
		if (g_ExtensionManager)
		{
			g_ExtensionManager->ForEach([&](const ExtensionManager::ExtensionPtr ext)
			{
				extHolders.push_back(ext);
				std::string_view name = ext->GetName();
				std::string_view entry = ext->GetEntry();
				extNamePtrs.push_back(name.data());
				extNameLens.push_back(name.size());
				extEntryPtrs.push_back(entry.data());
				extEntryLens.push_back(entry.size());
			});
		}

		nzbget_rs_health_validate_extension_scripts_section(
			extensions.data(), extensions.size(),
			extNamePtrs.data(), extNameLens.data(),
			extEntryPtrs.data(), extEntryLens.data(),
			extNamePtrs.size(),
			shellOverride.data(), shellOverride.size(),
			m_options.GetEventInterval(),
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 6. Connection
	{
		SectionReport sec;
		sec.name = "Connection";
		nzbget_rs_health_validate_connection_section(
			m_options.GetArticleRetries(),
			m_options.GetHealthCheck() == Options::hcDupe ? 1 : 0,
			m_options.GetDupeSearch() ? 1 : 0,
			m_options.GetArticleInterval(),
			m_options.GetArticleTimeout(),
			m_options.GetArticleReadChunkSize(),
			m_options.GetUrlRetries(),
			m_options.GetUrlInterval(),
			m_options.GetUrlTimeout(),
			m_options.GetUrlConnections(),
			m_options.GetRemoteTimeout(),
			m_options.GetDownloadRate(),
			m_options.GetMonthlyQuota(),
			m_options.GetQuotaStartDay(),
			m_options.GetDailyQuota(),
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 7. DownloadQueue
	{
		SectionReport sec;
		sec.name = "DownloadQueue";
		nzbget_rs_health_validate_download_queue_section(
			m_options.GetFlushQueue() ? 1 : 0,
			m_options.GetContinuePartial() ? 1 : 0,
			m_options.GetPropagationDelay(),
			m_options.GetArticleCache(),
			m_options.GetDirectWrite() ? 1 : 0,
			m_options.GetWriteBuffer(),
			static_cast<int>(m_options.GetFileNaming()),
			static_cast<int>(m_options.GetPostStrategy()),
			m_options.GetDiskSpace(),
			m_options.GetNzbCleanupDisk() ? 1 : 0,
			m_options.GetKeepHistory(),
			m_options.GetFeedHistory(),
			m_options.GetSkipWrite() ? 1 : 0,
			m_options.GetRawArticle() ? 1 : 0,
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 8. Security
	{
		SectionReport sec;
		sec.name = "Security";
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

		nzbget_rs_health_validate_security_section(
			controlIp.data(), controlIp.size(),
			m_options.GetControlPort(),
			controlUser.data(), controlUser.size(),
			controlPass.data(), controlPass.size(),
			restrictedUser.data(), restrictedUser.size(),
			restrictedPass.data(), restrictedPass.size(),
			addUser.data(), addUser.size(),
			addPass.data(), addPass.size(),
			m_options.GetFormAuth() ? 1 : 0,
			m_options.GetSecureControl() ? 1 : 0,
			secureCert.data(), secureCert.size(),
			secureKey.data(), secureKey.size(),
			m_options.GetSecurePort(),
			m_options.GetCertCheck() ? 1 : 0,
			m_options.GetCertStorePath().empty() ? 1 : 0,
			m_options.GetDaemonMode() ? 1 : 0,
			daemonUser.data(), daemonUser.size(),
			m_options.GetUMask(),
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 9. IncomingNzbs
	{
		SectionReport sec;
		sec.name = "IncomingNzbs";
		nzbget_rs_health_validate_incoming_nzb_section(
			m_options.GetAppendCategoryDir() ? 1 : 0,
			m_options.GetNzbDirInterval(),
			m_options.GetNzbDirFileAge(),
			m_options.GetDupeCheck() ? 1 : 0,
			m_options.GetDupeSearch() ? 1 : 0,
			static_cast<int>(m_options.GetHealthCheck()),
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 10. CheckAndRepair
	{
		SectionReport sec;
		sec.name = "CheckAndRepair";
		const size_t hwThreads = std::thread::hardware_concurrency();
		nzbget_rs_health_validate_check_and_repair_section(
			m_options.GetCrcCheck() ? 1 : 0,
			static_cast<int>(m_options.GetParCheck()),
			m_options.GetParRepair() ? 1 : 0,
			m_options.GetParQuick() ? 1 : 0,
			m_options.GetParBuffer(),
			m_options.GetParThreads(),
			hwThreads,
			m_options.GetParRename() ? 1 : 0,
			m_options.GetRarRename() ? 1 : 0,
			m_options.GetDirectRename() ? 1 : 0,
			m_options.GetHardLinking() ? 1 : 0,
			static_cast<int>(m_options.GetHealthCheck()),
			m_options.GetParTimeLimit(),
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 11. Unpack
	{
		SectionReport sec;
		sec.name = "Unpack";
		std::string unrarPath = m_options.GetUnrarPath().string();
		std::string sevenZipPath = m_options.GetSevenZipPath().string();
		std::string_view extCleanup = m_options.GetExtCleanupDisk();
		std::string_view unpackPassFile = m_options.GetUnpackPassFile();
		std::string_view unpackIgnoreExt = m_options.GetUnpackIgnoreExt();

		nzbget_rs_health_validate_unpack_section(
			m_options.GetUnpack() ? 1 : 0,
			m_options.GetDirectUnpack() ? 1 : 0,
			m_options.GetUnpackPauseQueue() ? 1 : 0,
			unrarPath.data(), unrarPath.size(),
			sevenZipPath.data(), sevenZipPath.size(),
			extCleanup.data(), extCleanup.size(),
			unpackPassFile.data(), unpackPassFile.size(),
			unpackIgnoreExt.data(), unpackIgnoreExt.size(),
			&sec.options,
			&AppendOptionCallback);
		report.sections.push_back(std::move(sec));
	}

	// 12. Scheduler
	{
		SectionReport sec;
		sec.name = "Scheduler";
		sec.subsections.reserve(m_tasks.size());
		for (const auto& task : m_tasks)
		{
			SubsectionReport sub;
			sub.name = "Task" + std::to_string(task->GetId());
			nzbget_rs_health_validate_scheduler_task_section(
				task->GetHours(),
				task->GetMinutes(),
				task->GetWeeDaysBits(),
				&sub.options,
				&AppendOptionCallback);
			sec.subsections.push_back(std::move(sub));
		}
		report.sections.push_back(std::move(sec));
	}

	// 13. Categories
	{
		SectionReport sec;
		sec.name = "Categories";
		const auto* categories = m_options.GetCategories();
		if (categories)
		{
			sec.subsections.reserve(categories->size());
			std::vector<const char*> ptrs(categories->size());
			std::vector<size_t> lens(categories->size());
			for (size_t i = 0; i < categories->size(); ++i)
			{
				const auto& cat = (*categories)[i];
				std::string_view name = cat.GetName();
				ptrs[i] = name.data();
				lens[i] = name.size();

				SubsectionReport sub;
				sub.name = "Category" + std::to_string(i + 1);
				nzbget_rs_health_validate_category_section(
					name.data(), name.size(),
					m_options.GetUnpack() ? 1 : 0,
					cat.GetUnpack() ? 1 : 0,
					&sub.options,
					&AppendOptionCallback);
				sec.subsections.push_back(std::move(sub));
			}
			if (!categories->empty())
			{
				NzbgetRsBuf msg{nullptr, 0, 0};
				int sev = nzbget_rs_health_validate_duplicate_category_names(
					ptrs.data(), lens.data(), categories->size(), &msg);
				Status dup = Status::FromRust(sev, msg);
				if (!dup.IsOk())
				{
					sec.issues.push_back(std::move(dup));
				}
			}
		}
		report.sections.push_back(std::move(sec));
	}

	// 14. Feeds
	{
		SectionReport sec;
		sec.name = "Feeds";
		sec.subsections.reserve(m_feeds.size());
		std::vector<const char*> namePtrs(m_feeds.size());
		std::vector<size_t> nameLens(m_feeds.size());
		std::vector<const char*> urlPtrs(m_feeds.size());
		std::vector<size_t> urlLens(m_feeds.size());
		for (size_t i = 0; i < m_feeds.size(); ++i)
		{
			const auto& feed = m_feeds[i];
			std::string_view name = feed->GetName();
			std::string_view url = feed->GetUrl();
			namePtrs[i] = name.data();
			nameLens[i] = name.size();
			urlPtrs[i] = url.data();
			urlLens[i] = url.size();

			SubsectionReport sub;
			sub.name = "Feed" + std::to_string(feed->GetId());
			nzbget_rs_health_validate_feed_section(
				name.data(), name.size(),
				url.data(), url.size(),
				feed->GetInterval(),
				static_cast<uint32_t>(feed->GetCertVerificationLevel()),
				&sub.options,
				&AppendOptionCallback);
			sec.subsections.push_back(std::move(sub));
		}
		if (!m_feeds.empty())
		{
			NzbgetRsBuf msg{nullptr, 0, 0};
			int sev = nzbget_rs_health_validate_duplicate_feed_urls(
				namePtrs.data(), nameLens.data(),
				urlPtrs.data(), urlLens.data(),
				m_feeds.size(), &msg);
			Status dup = Status::FromRust(sev, msg);
			if (!dup.IsOk())
			{
				sec.issues.push_back(std::move(dup));
			}
		}
		report.sections.push_back(std::move(sec));
	}

	return report;
}

void Service::ReportAlert(Alert alert)
{
	std::lock_guard<std::mutex> lock(m_mutex);

	auto it = std::find_if(
		m_alerts.begin(), m_alerts.end(), [&](const Alert& existing)
		{
			return existing.GetSource() == alert.GetSource() &&
				   existing.GetCategory() == alert.GetCategory();
		});

	if (alert.GetSeverity() == Severity::Ok)
	{
		if (it != m_alerts.end())
		{
			m_alerts.erase(it);
		}
		return;
	}

	if (it != m_alerts.end())
	{
		*it = std::move(alert);
	}
	else
	{
		m_alerts.push_back(std::move(alert));
	}
}

void Log(const HealthReport& report)
{
	for (const auto& alert : report.alerts) Log(alert);
	for (const auto& sectionReport : report.sections) Log(sectionReport);
}

void Log(const SectionReport& report)
{
	for (const auto& issue : report.issues)
	{
		if (issue.IsError())
			error("%s", issue.GetMessage().c_str());
		else if (issue.IsWarning())
			warn("%s", issue.GetMessage().c_str());
	}

	for (const auto& [optName, status] : report.options)
	{
		if (status.IsError())
			error("[%s][%s]: %s", report.name.c_str(), optName.c_str(), status.GetMessage().c_str());
		else if (status.IsWarning())
			warn("[%s][%s]: %s", report.name.c_str(), optName.c_str(), status.GetMessage().c_str());
	}

	for (const auto& section : report.subsections)
	{
		for (const auto& [optName, status] : section.options)
		{
			if (status.IsError())
				error("[%s][%s.%s]: %s", report.name.c_str(), section.name.c_str(), optName.c_str(),
					  status.GetMessage().c_str());
			else if (status.IsWarning())
				warn("[%s][%s.%s]: %s", report.name.c_str(), section.name.c_str(), optName.c_str(),
					 status.GetMessage().c_str());
		}
	}
}

void Log(const Alert& alert)
{
	if (alert.GetSeverity() == Severity::Error)
		error("[%s][%s]: %s", alert.GetCategory().c_str(), alert.GetSource().c_str(),
			  alert.GetMessage().c_str());
	else if (alert.GetSeverity() == Severity::Warning)
		warn("[%s][%s]: %s", alert.GetCategory().c_str(), alert.GetSource().c_str(),
			 alert.GetMessage().c_str());
}

Json::JsonObject ToJson(const Status& status)
{
	Json::JsonObject json;
	json["Severity"] = SeverityToStr(status.GetSeverity());
	json["Message"] = status.GetMessage();
	return json;
}

Json::JsonObject ToJson(const OptionStatus& status)
{
	Json::JsonObject json;
	json["Name"] = status.name;
	json["Severity"] = SeverityToStr(status.status.GetSeverity());
	json["Message"] = status.status.GetMessage();
	return json;
}

Json::JsonObject ToJson(const SubsectionReport& report)
{
	Json::JsonObject json;
	Json::JsonArray optionsArrayJson;

	for (const auto& option : report.options)
	{
		optionsArrayJson.push_back(ToJson(option));
	}

	json["Name"] = report.name;
	json["Options"] = std::move(optionsArrayJson);

	return json;
}

Json::JsonObject ToJson(const SectionReport& report)
{
	Json::JsonObject json;
	Json::JsonArray issuesArrayJson;
	Json::JsonArray optionsArrayJson;
	Json::JsonArray sectionsArrayJson;

	for (const auto& issue : report.issues)
	{
		issuesArrayJson.push_back(ToJson(issue));
	}

	for (const auto& option : report.options)
	{
		optionsArrayJson.push_back(ToJson(option));
	}

	for (const auto& section : report.subsections)
	{
		sectionsArrayJson.push_back(ToJson(section));
	}

	json["Name"] = report.name;
	json["Issues"] = std::move(issuesArrayJson);
	json["Options"] = std::move(optionsArrayJson);
	json["Subsections"] = std::move(sectionsArrayJson);

	return json;
}

Json::JsonObject ToJson(const Alert& alert)
{
	Json::JsonObject alertJson;
	alertJson["Source"] = alert.GetSource();
	alertJson["Category"] = alert.GetCategory();
	alertJson["Severity"] = SeverityToStr(alert.GetSeverity());
	alertJson["Message"] = alert.GetMessage();
	alertJson["Timestamp"] = alert.GetTimestamp().time_since_epoch().count();

	return alertJson;
}

std::string ToJsonStr(const HealthReport& report)
{
	Json::JsonObject reportJson;
	Json::JsonArray sectionsArrayJson;
	Json::JsonArray alertsArrayJson;

	for (const auto& alert : report.alerts)
	{
		alertsArrayJson.push_back(ToJson(alert));
	}

	for (const auto& section : report.sections)
	{
		sectionsArrayJson.push_back(ToJson(section));
	}

	reportJson["Alerts"] = std::move(alertsArrayJson);
	reportJson["Sections"] = std::move(sectionsArrayJson);

	return Json::serialize(reportJson);
}

Xml::XmlNodePtr ToXml(const Status& status)
{
	std::string_view severity = SeverityToStr(status.GetSeverity());
	const std::string& message = status.GetMessage();

	xmlNodePtr node = Xml::CreateStructNode();
	Xml::AddNewNode(node, "Severity", "string", severity.data());
	Xml::AddNewNode(node, "Message", "string", message.c_str());

	return node;
}

Xml::XmlNodePtr ToXml(const OptionStatus& status)
{
	xmlNodePtr structNode = Xml::CreateStructNode();

	const auto severity = SeverityToStr(status.status.GetSeverity());
	Xml::AddNewNode(structNode, "Name", "string", status.name.c_str());
	Xml::AddNewNode(structNode, "Severity", "string", severity.data());
	Xml::AddNewNode(structNode, "Message", "string", status.status.GetMessage().c_str());

	return structNode->parent;
}

Xml::XmlNodePtr ToXml(const SubsectionReport& report)
{
	xmlNodePtr structNode = Xml::CreateStructNode();

	Xml::AddNewNode(structNode, "Name", "string", report.name.c_str());

	std::vector<xmlNodePtr> optionsNodes;
	for (const auto& option : report.options)
	{
		optionsNodes.push_back(ToXml(option));
	}
	Xml::AddArrayNode(structNode, "Options", optionsNodes);

	return structNode->parent;
}

Xml::XmlNodePtr ToXml(const SectionReport& report)
{
	xmlNodePtr structNode = Xml::CreateStructNode();

	Xml::AddNewNode(structNode, "Name", "string", report.name.c_str());

	std::vector<xmlNodePtr> issueNodes;
	for (const auto& issue : report.issues)
	{
		xmlNodePtr sNode = Xml::CreateStructNode();
		const auto severity = SeverityToStr(issue.GetSeverity());
		Xml::AddNewNode(sNode, "Severity", "string", severity.data());
		Xml::AddNewNode(sNode, "Message", "string", issue.GetMessage().c_str());
		issueNodes.push_back(sNode->parent);
	}
	Xml::AddArrayNode(structNode, "Issues", issueNodes);

	std::vector<xmlNodePtr> optionNodes;
	for (const auto& option : report.options)
	{
		optionNodes.push_back(ToXml(option));
	}
	Xml::AddArrayNode(structNode, "Options", optionNodes);

	std::vector<xmlNodePtr> subNodes;
	for (const auto& sub : report.subsections)
	{
		subNodes.push_back(ToXml(sub));
	}
	Xml::AddArrayNode(structNode, "Subsections", subNodes);

	return structNode->parent;
}

Xml::XmlNodePtr ToXml(const Alert& alert)
{
	xmlNodePtr structNode = Xml::CreateStructNode();

	Xml::AddNewNode(structNode, "Source", "string", alert.GetSource().c_str());
	Xml::AddNewNode(structNode, "Category", "string", alert.GetCategory().c_str());
	const auto severity = SeverityToStr(alert.GetSeverity());
	Xml::AddNewNode(structNode, "Severity", "string", severity.data());
	Xml::AddNewNode(structNode, "Message", "string", alert.GetMessage().c_str());

	auto ticks = std::to_string(alert.GetTimestamp().time_since_epoch().count());
	Xml::AddNewNode(structNode, "Timestamp", "i8", ticks.c_str());

	return structNode->parent;
}

std::string ToXmlStr(const HealthReport& report)
{
	xmlDocPtr doc = xmlNewDoc(BAD_CAST "1.0");
	xmlNodePtr structNode = Xml::CreateStructNode();
	xmlDocSetRootElement(doc, structNode->parent);

	std::vector<xmlNodePtr> alertNodes;
	for (const auto& alert : report.alerts)
	{
		alertNodes.push_back(ToXml(alert));
	}
	Xml::AddArrayNode(structNode, "Alerts", alertNodes);

	std::vector<xmlNodePtr> sectionNodes;
	for (const auto& section : report.sections)
	{
		sectionNodes.push_back(ToXml(section));
	}
	Xml::AddArrayNode(structNode, "Sections", sectionNodes);

	std::string result = Xml::Serialize(structNode->parent);

	xmlFreeDoc(doc);

	return result;
}

}  // namespace SystemHealth
