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

#ifndef SYSTEM_HEALTH_SERVICE_H
#define SYSTEM_HEALTH_SERVICE_H

#include <chrono>
#include <ctime>
#include <memory>
#include <mutex>
#include <string>
#include <string_view>
#include <utility>
#include <vector>
#include "Options.h"
#include "NewsServer.h"
#include "FeedInfo.h"
#include "Scheduler.h"
#include "Json.h"
#include "Xml.h"
#include "nzbget_rs.h"

namespace SystemHealth
{
enum class Severity
{
	Ok,
	Info,
	Warning,
	Error
};

class Status final
{
public:
	Status() = delete;

	static Status Ok() { return Status(Severity::Ok, ""); }
	static Status Info(std::string message) { return Status(Severity::Info, std::move(message)); }
	static Status Warning(std::string message)
	{
		return Status(Severity::Warning, std::move(message));
	}
	static Status Error(std::string message) { return Status(Severity::Error, std::move(message)); }
	static Status FromRaw(Severity severity, std::string message)
	{
		return Status(severity, std::move(message));
	}
	static Status FromRust(int severity, NzbgetRsBuf msg)
	{
		std::string text;
		if (msg.data && msg.len > 0)
		{
			text.assign(msg.data, msg.len);
		}
		nzbget_rs_free(msg);
		Severity sev = Severity::Ok;
		if (severity == 1) sev = Severity::Info;
		else if (severity == 2) sev = Severity::Warning;
		else if (severity == 3) sev = Severity::Error;
		return Status(sev, std::move(text));
	}

	bool IsOk() const { return m_severity == Severity::Ok; }
	bool IsInfo() const { return m_severity == Severity::Info; }
	bool IsWarning() const { return m_severity == Severity::Warning; }
	bool IsError() const { return m_severity == Severity::Error; }

	Severity GetSeverity() const { return m_severity; }
	const std::string& GetMessage() const { return m_message; }

private:
	Status(Severity severity, std::string message)
		: m_message(std::move(message)), m_severity(severity)
	{
	}

	std::string m_message;
	Severity m_severity = Severity::Ok;
};

struct OptionStatus
{
	std::string name;
	Status status;
};

struct SubsectionReport
{
	std::string name;
	std::vector<OptionStatus> options;
};

struct SectionReport
{
	std::string name;
	std::vector<Status> issues;
	std::vector<OptionStatus> options;
	std::vector<SubsectionReport> subsections;
};

using Timestamp = std::chrono::system_clock::time_point;

class Alert final
{
public:
	Alert() = delete;
	Alert(Severity severity, std::string category, std::string source, std::string message);

	Severity GetSeverity() const { return m_severity; }
	const std::string& GetCategory() const { return m_category; }
	const std::string& GetSource() const { return m_source; }
	const std::string& GetMessage() const { return m_message; }
	const Timestamp& GetTimestamp() const { return m_timestamp; }

private:
	Severity m_severity;
	std::string m_category;
	std::string m_source;
	std::string m_message;
	Timestamp m_timestamp;
};

struct HealthReport
{
	std::vector<Alert> alerts;
	std::vector<SectionReport> sections;
};

class Service
{
public:
	Service(const Options& options, const Servers& servers, const Feeds& feeds,
			const ::Scheduler::TaskList& tasks, const Log& log);
	HealthReport Diagnose() const;

	void ReportAlert(Alert alert);

private:
	const Options& m_options;
	const Servers& m_servers;
	const Feeds& m_feeds;
	const ::Scheduler::TaskList& m_tasks;
	const Log& m_log;

	std::vector<Alert> m_alerts;
	mutable std::mutex m_mutex;
};

std::string_view SeverityToStr(Severity severity);

void Log(const HealthReport& report);
void Log(const SectionReport& sectionReport);
void Log(const Alert& alert);

Json::JsonObject ToJson(const Status& status);
Json::JsonObject ToJson(const OptionStatus& status);
Json::JsonObject ToJson(const SubsectionReport& report);
Json::JsonObject ToJson(const SectionReport& report);
Json::JsonObject ToJson(const Alert& alert);
Xml::XmlNodePtr ToXml(const Status& status);
Xml::XmlNodePtr ToXml(const OptionStatus& status);
Xml::XmlNodePtr ToXml(const SubsectionReport& report);
Xml::XmlNodePtr ToXml(const SectionReport& report);
Xml::XmlNodePtr ToXml(const Alert& alert);

std::string ToJsonStr(const HealthReport& report);
std::string ToXmlStr(const HealthReport& report);

}  // namespace SystemHealth

extern SystemHealth::Service* g_SystemHealth;

#endif
