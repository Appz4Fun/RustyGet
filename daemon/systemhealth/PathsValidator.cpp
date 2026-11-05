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
#include "Options.h"
#include "Status.h"
#include "Validators.h"
#include "PathsValidator.h"
#include "nzbget_rs.h"

namespace SystemHealth::Paths
{
namespace
{
Status ValidateRequiredWritableDir(std::string_view optName, const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_required_writable_dir(
		optName.data(), optName.size(), p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}
}  // namespace

PathsValidator::PathsValidator(const Options& options, const ::Log& log)
	: m_options(options)
	, m_log(log)
{
}

SectionReport PathsValidator::Validate() const
{
	SectionReport report;
	report.name = GetName();

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
		&report.options,
		&SectionValidator::AppendOptionCallback);

	return report;
}

Status MainDirValidator::Validate() const { return Validate(m_options.GetMainDirPath()); }

Status MainDirValidator::Validate(const fs::path& path)
{
	return ValidateRequiredWritableDir(Options::MAINDIR, path);
}

Status DestDirValidator::Validate() const
{
	return Validate(m_options.GetDestDirPath())
		.And([&]() {
			return UniquePath(GetName(), m_options.GetDestDirPath(),
							  {{Options::MAINDIR, m_options.GetMainDirPath()}});
		});
}

Status DestDirValidator::Validate(const fs::path& path)
{
	return ValidateRequiredWritableDir(Options::DESTDIR, path);
}

Status InterDirValidator::Validate() const
{
	return Validate(m_options.GetInterDirPath())
		.And([&]() {
			return UniquePath(GetName(), m_options.GetInterDirPath(),
							  {{Options::MAINDIR, m_options.GetMainDirPath()},
							   {Options::DESTDIR, m_options.GetDestDirPath()}});
		});
}

Status InterDirValidator::Validate(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_inter_dir_path(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status NzbDirValidator::Validate() const
{
	return Validate(m_options.GetNzbDirPath())
		.And([&]() {
			return UniquePath(GetName(), m_options.GetNzbDirPath(),
							  {{Options::MAINDIR, m_options.GetMainDirPath()},
							   {Options::DESTDIR, m_options.GetDestDirPath()},
							   {Options::INTERDIR, m_options.GetInterDirPath()}});
		});
}

Status NzbDirValidator::Validate(const fs::path& path)
{
	return ValidateRequiredWritableDir(Options::NZBDIR, path);
}

Status QueueDirValidator::Validate() const
{
	return Validate(m_options.GetQueueDirPath())
		.And([&]() {
			return UniquePath(GetName(), m_options.GetQueueDirPath(),
							  {{Options::MAINDIR, m_options.GetMainDirPath()},
							   {Options::DESTDIR, m_options.GetDestDirPath()},
							   {Options::INTERDIR, m_options.GetInterDirPath()},
							   {Options::NZBDIR, m_options.GetNzbDirPath()}});
		});
}

Status QueueDirValidator::Validate(const fs::path& path)
{
	return ValidateRequiredWritableDir(Options::QUEUEDIR, path);
}

Status WebDirValidator::Validate() const
{
	const auto& path = m_options.GetWebDirPath();
	return Validate(path).And([&]() {
		if (path.empty()) return Status::Ok();
		return UniquePath(GetName(), m_options.GetWebDirPath(),
						  {{Options::MAINDIR, m_options.GetMainDirPath()},
						   {Options::DESTDIR, m_options.GetDestDirPath()},
						   {Options::INTERDIR, m_options.GetInterDirPath()},
						   {Options::NZBDIR, m_options.GetNzbDirPath()},
						   {Options::QUEUEDIR, m_options.GetQueueDirPath()}});
	});
}

Status WebDirValidator::Validate(const fs::path& path)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_web_dir_path(p.data(), p.size(), &msg);
	return Status::FromRust(sev, msg);
}

Status TempDirValidator::Validate() const
{
	return Validate(m_options.GetTempDirPath())
		.And([&]() {
			return UniquePath(GetName(), m_options.GetTempDirPath(),
							  {{Options::MAINDIR, m_options.GetMainDirPath()},
							   {Options::DESTDIR, m_options.GetDestDirPath()},
							   {Options::INTERDIR, m_options.GetInterDirPath()},
							   {Options::NZBDIR, m_options.GetNzbDirPath()},
							   {Options::QUEUEDIR, m_options.GetQueueDirPath()},
							   {Options::WEBDIR, m_options.GetWebDirPath()}});
		});
}

Status TempDirValidator::Validate(const fs::path& path)
{
	return ValidateRequiredWritableDir(Options::TEMPDIR, path);
}

Status ScriptDirValidator::Validate() const
{
	const auto& paths = m_options.GetScriptDirPaths();
	if (paths.empty())
	{
		return Status::Warning("'" + std::string(Options::SCRIPTDIR) +
							 "' is empty. Extensions cannot be found or installed");
	}

	for (const auto& dir : paths)
	{
		auto status = Validate(dir).And([&]() {
			return UniquePath(GetName(), dir,
							  {{Options::MAINDIR, m_options.GetMainDirPath()},
							   {Options::DESTDIR, m_options.GetDestDirPath()},
							   {Options::INTERDIR, m_options.GetInterDirPath()},
							   {Options::NZBDIR, m_options.GetNzbDirPath()},
							   {Options::QUEUEDIR, m_options.GetQueueDirPath()},
							   {Options::WEBDIR, m_options.GetWebDirPath()},
							   {Options::TEMPDIR, m_options.GetTempDirPath()}});
		});
		if (!status.IsOk())
		{
			return status;
		}
	}

	return Status::Ok();
}

Status ScriptDirValidator::Validate(const fs::path& path)
{
	return ValidateRequiredWritableDir(Options::SCRIPTDIR, path);
}

Status ConfigTemplateValidator::Validate() const { return Status::Ok(); }

Status LogFileValidator::Validate() const
{
	return Validate(m_log.GetLogFilePath(), m_options.GetWriteLog())
		.And([&]() {
			return UniquePath(GetName(), m_log.GetLogFilePath(),
							  {{Options::CONFIGTEMPLATE, m_options.GetConfigTemplatePath()},
							   {Options::CONFIGFILE, m_options.GetConfigFilePath()}});
		});
}

Status LogFileValidator::Validate(const fs::path& path, Options::EWriteLog writeLog)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_log_file_path(
		p.data(), p.size(),
		static_cast<int>(writeLog),
		&msg);
	return Status::FromRust(sev, msg);
}

Status CertStoreValidator::Validate() const
{
	return Validate(m_options.GetCertStore(), m_options.GetCertCheck())
		.And([&]() {
			return UniquePath(GetName(), m_options.GetCertStorePath(),
							  {{Options::MAINDIR, m_options.GetMainDirPath()},
							   {Options::DESTDIR, m_options.GetDestDirPath()},
							   {Options::INTERDIR, m_options.GetInterDirPath()},
							   {Options::NZBDIR, m_options.GetNzbDirPath()},
							   {Options::QUEUEDIR, m_options.GetQueueDirPath()},
							   {Options::WEBDIR, m_options.GetWebDirPath()},
							   {Options::TEMPDIR, m_options.GetTempDirPath()},
							   {Options::CONFIGTEMPLATE, m_options.GetConfigTemplatePath()},
							   {Options::LOGFILE, m_log.GetLogFilePath()},
							   {Options::CONFIGFILE, m_options.GetConfigFilePath()}});
		});
}

Status CertStoreValidator::Validate(const fs::path& path, bool certCheck)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_cert_store_path(p.data(), p.size(), certCheck ? 1 : 0, &msg);
	return Status::FromRust(sev, msg);
}

Status RequiredDirValidator::Validate() const { return Status::Ok(); }

Status LockFileValidator::Validate() const
{
	return Validate(m_options.GetLockFilePath(), m_options.GetDaemonMode())
		.And([&]() {
			return UniquePath(GetName(), m_options.GetLockFilePath(),
							  {{Options::MAINDIR, m_options.GetMainDirPath()},
							   {Options::CONFIGTEMPLATE, m_options.GetConfigTemplatePath()},
							   {Options::LOGFILE, m_log.GetLogFilePath()},
							   {Options::CERTSTORE, m_options.GetCertStorePath()},
							   {Options::CONFIGFILE, m_options.GetConfigFilePath()}});
		});
}

Status LockFileValidator::Validate(const fs::path& path, bool daemonMode)
{
	std::string p = path.string();
	NzbgetRsBuf msg{nullptr, 0, 0};
	int sev = nzbget_rs_health_validate_lock_file_path(p.data(), p.size(), daemonMode ? 1 : 0, &msg);
	return Status::FromRust(sev, msg);
}
}  // namespace SystemHealth::Paths
