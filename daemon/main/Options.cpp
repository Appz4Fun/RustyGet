/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
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

#include "Util.h"
#include "Options.h"
#include "nzbget_rs.h"
#include "Log.h"
#include "FileSystem.h"

const char* PossibleConfigLocations[] =
	{
		"~/.nzbget",
		"~/.config/nzbget.conf",
		"~/.config/nzbget/nzbget.conf",
		"/etc/nzbget.conf",
		"/usr/etc/nzbget.conf",
		"/usr/local/etc/nzbget.conf",
		"/opt/etc/nzbget.conf",
		"~/usr/etc/nzbget.conf",
		nullptr
	};

void Options::OptEntry::SetValue(const char* value)
{
	m_value = value;
	if (!m_defValue)
	{
		m_defValue = value;
	}
}

bool Options::OptEntry::Restricted()
{
	// rust/src/options.rs
	return nzbget_rs_option_restricted(m_name, (char)-1 < 0) != 0;
}

Options::OptEntry* Options::OptEntries::FindOption(const char* name)
{
	if (!name)
	{
		return nullptr;
	}

	for (OptEntry& optEntry : *this)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(optEntry.GetName(), name) != 0)
		{
			return &optEntry;
		}
	}

	return nullptr;
}

Options::Category* Options::Categories::FindCategory(const char* name, bool searchAliases)
{
	if (!name)
	{
		return nullptr;
	}

	for (Category& category : *this)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(category.GetName(), name) != 0)
		{
			return &category;
		}
	}

	if (searchAliases)
	{
		for (Category& category : *this)
		{
			std::vector<const char*> aliasPtrs;
			aliasPtrs.reserve(category.GetAliases()->size());
			for (const CString& alias : *category.GetAliases())
			{
				aliasPtrs.push_back(alias ? *alias : "");
			}
			if (nzbget_rs_options_category_alias_list_matches(
				aliasPtrs.data(), aliasPtrs.size(), name, (char)-1 < 0) != 0)
			{
				return &category;
			}
		}
	}

	return nullptr;
}

Options::Extender::~Extender() = default;

Options::Options(const char* exeName, const char* configFilename, bool noConfig,
	CmdOptList* commandLineOptions, Extender* extender)
{
	Init(exeName, configFilename, noConfig, commandLineOptions, false, extender);
}

Options::Options(CmdOptList* commandLineOptions, Extender* extender)
{
	Init("nzbget/nzbget", nullptr, true, commandLineOptions, true, extender);
}

void Options::Init(const char* exeName, const char* configFilename, bool noConfig,
	CmdOptList* commandLineOptions, bool noDiskAccess, Extender* extender)
{
	g_Options = this;
	m_extender = extender;
	m_noDiskAccess = noDiskAccess;
	m_noConfig = noConfig;
	m_configFilename = configFilename;

	SetOption(CONFIGFILE.data(), "");

	CString filename;
	if (m_noDiskAccess)
	{
		filename = exeName;
	}
	else
	{
		filename = FileSystem::GetExeFileName(exeName);
	}
	FileSystem::NormalizePathSeparators(filename);
	SetOption(APPBIN.data(), filename);
	NzbgetRsBuf appDirBuf = nzbget_rs_dqh_fs_extract_parent_dir_path(filename, PATH_SEPARATOR);
	filename.Set(appDirBuf.data ? appDirBuf.data : "", (int)appDirBuf.len);
	nzbget_rs_free(appDirBuf);
	SetOption(APPDIR.data(), filename);
	m_appDir = *filename;
	m_appDirPath = fs::u8path(*m_appDir);

	SetOption(APPVERSION.data(), Util::VersionRevision());

	InitDefaults();

	InitOptFile();
	if (m_fatalError)
	{
		return;
	}

	if (commandLineOptions)
	{
		InitCommandLineOptions(commandLineOptions);
	}

	if (!m_configFilename && !noConfig)
	{
		printf("No configuration-file found\n");
		printf("Please use option \"-c\" or put configuration-file in one of the following locations:\n");
		int p = 0;
		while (const char* filename = PossibleConfigLocations[p++])
		{
			printf("%s\n", filename);
		}
		m_fatalError = true;
		return;
	}

	ConvertOldOptions(&m_optEntries);

	CheckDirs();
	InitOptions();
	CheckOptions();

	InitServers();
	InitCategories();
	InitScheduler();
	InitFeeds();
}

Options::~Options()
{
	g_Options = nullptr;
}

NzbgetRsOptionsHost Options::MakeHost()
{
	NzbgetRsOptionsHost host{};
	host.ctx = this;
	host.find_entry = [](void* ctx, const char* name, const char** outVal, const char** outDefVal, int* outLineNo) -> int
	{
		auto* self = static_cast<Options*>(ctx);
		OptEntry* optEntry = self->FindOption(name);
		if (!optEntry)
		{
			return 0;
		}
		if (outVal) *outVal = optEntry->GetValue();
		if (outDefVal) *outDefVal = optEntry->GetDefValue();
		if (outLineNo) *outLineNo = optEntry->GetLineNo();
		return 1;
	};
	host.set_entry_raw_value = [](void* ctx, const char* name, const char* value)
	{
		auto* self = static_cast<Options*>(ctx);
		if (OptEntry* optEntry = self->FindOption(name))
		{
			optEntry->SetValue(value);
		}
	};
	host.set_option = [](void* ctx, const char* name, const char* value)
	{
		static_cast<Options*>(ctx)->SetOption(name, value);
	};
	host.set_config_line = [](void* ctx, int line)
	{
		static_cast<Options*>(ctx)->m_configLine = line;
	};
	host.get_config_line = [](void* ctx) -> int
	{
		return static_cast<Options*>(ctx)->m_configLine;
	};
	host.config_error = [](void* ctx, const char* msg)
	{
		static_cast<Options*>(ctx)->ConfigError("%s", msg);
	};
	host.config_warn = [](void* ctx, const char* msg)
	{
		static_cast<Options*>(ctx)->ConfigWarn("%s", msg);
	};
	host.file_exists = [](void* /*ctx*/, const char* path) -> int
	{
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	host.ensure_directory = [](void* ctx, const char* dir, char* errBuf, size_t errCap) -> int
	{
		auto* self = static_cast<Options*>(ctx);
		CString errmsg;
		if (!FileSystem::ForceDirectories(dir, errmsg))
		{
			if (errBuf && errCap > 0)
			{
				snprintf(errBuf, errCap, "%s", errmsg ? *errmsg : "");
			}
			return 0;
		}
		if (getuid() == 0 || geteuid() == 0)
		{
			struct passwd* pwd = getpwnam(self->GetOption(DAEMONUSERNAME.data()));
			if (pwd != nullptr)
			{
				chown(dir, pwd->pw_uid, pwd->pw_gid);
			}
		}
		return 1;
	};
	host.add_news_server = [](void* ctx, int id, int active, const char* name, const char* host,
		int port, int ipVersion, const char* user, const char* pass, int joinGroup, int tls,
		const char* cipher, int maxConnections, int retention, int level, int group, int optional,
		uint32_t certVerifLevel)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddNewsServer(id, active != 0, name, host, port, ipVersion,
				user, pass, joinGroup != 0, tls != 0, cipher, maxConnections, retention,
				level, group, optional != 0, certVerifLevel);
		}
	};
	host.add_feed = [](void* ctx, int id, const char* name, const char* url, int interval,
		const char* filter, int backlog, int pauseNzb, const char* category, int categorySource,
		int priority, const char* extensions, uint32_t certVerifLevel)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddFeed(id, name, url, interval, filter, backlog != 0, pauseNzb != 0,
				category, static_cast<FeedInfo::CategorySource>(categorySource), priority,
				extensions, certVerifLevel);
		}
	};
	host.add_task = [](void* ctx, int id, int hours, int minutes, int weekDaysBits,
		int command, const char* param)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddTask(id, hours, minutes, weekDaysBits,
				static_cast<ESchedulerCommand>(command), param);
		}
	};
	host.add_category = [](void* ctx, const char* name, const char* destDir, int unpack,
		const char* extensions, const char* const* aliases, size_t aliasesLen)
	{
		auto* self = static_cast<Options*>(ctx);
		self->m_categories.emplace_back(name, destDir, unpack != 0, extensions);
		Category& category = self->m_categories.back();
		for (size_t i = 0; i < aliasesLen; ++i)
		{
			category.GetAliases()->push_back(aliases[i]);
		}
	};
	return host;
}

NzbgetRsParsedOptions Options::ExportParsedScalars() const
{
	NzbgetRsParsedOptions s{};
	s.download_rate = m_downloadRate;
	s.article_timeout = m_articleTimeout;
	s.article_read_chunk_size = m_articleReadChunkSize;
	s.url_timeout = m_urlTimeout;
	s.remote_timeout = m_remoteTimeout;
	s.article_retries = m_articleRetries;
	s.article_interval = m_articleInterval;
	s.url_retries = m_urlRetries;
	s.url_interval = m_urlInterval;
	s.control_port = m_controlPort;
	s.secure_port = m_securePort;
	s.url_connections = m_urlConnections;
	s.log_buffer = m_logBuffer;
	s.rotate_log = m_rotateLog;
	s.umask = m_umask;
	s.update_interval = m_updateInterval;
	s.write_buffer = m_writeBuffer;
	s.nzb_dir_interval = m_nzbDirInterval;
	s.nzb_dir_file_age = m_nzbDirFileAge;
	s.nzb_dir_archive_scan = m_nzbDirArchiveScan ? 1 : 0;
	s.nzb_dir_archive_action = (int)m_nzbDirArchiveAction;
	s.disk_space = m_diskSpace;
	s.par_time_limit = m_parTimeLimit;
	s.keep_history = m_keepHistory;
	s.feed_history = m_feedHistory;
	s.time_correction = m_timeCorrection;
	s.propagation_delay = m_propagationDelay;
	s.article_cache = m_articleCache;
	s.memory_pressure_interval = m_memoryPressureInterval;
	s.event_interval = m_eventInterval;
	s.par_buffer = m_parBuffer;
	s.par_threads = m_parThreads;
	s.monthly_quota = m_monthlyQuota;
	s.quota_start_day = m_quotaStartDay;
	s.daily_quota = m_dailyQuota;
	s.system_health_check = m_systemHealthCheck ? 1 : 0;
	s.nzb_log = m_nzbLog ? 1 : 0;
	s.append_category_dir = m_appendCategoryDir ? 1 : 0;
	s.continue_partial = m_continuePartial ? 1 : 0;
	s.flush_queue = m_flushQueue ? 1 : 0;
	s.dupe_check = m_dupeCheck ? 1 : 0;
	s.par_repair = m_parRepair ? 1 : 0;
	s.par_quick = m_parQuick ? 1 : 0;
	s.par_rename = m_parRename ? 1 : 0;
	s.rar_rename = m_rarRename ? 1 : 0;
	s.direct_rename = m_directRename ? 1 : 0;
	s.hard_linking = m_hardLinking ? 1 : 0;
	s.curses_nzb_name = m_cursesNzbName ? 1 : 0;
	s.curses_time = m_cursesTime ? 1 : 0;
	s.curses_group = m_cursesGroup ? 1 : 0;
	s.crc_check = m_crcCheck ? 1 : 0;
	s.direct_write = m_directWrite ? 1 : 0;
	s.raw_article = m_rawArticle ? 1 : 0;
	s.skip_write = m_skipWrite ? 1 : 0;
	s.crash_trace = m_crashTrace ? 1 : 0;
	s.crash_dump = m_crashDump ? 1 : 0;
	s.par_pause_queue = m_parPauseQueue ? 1 : 0;
	s.script_pause_queue = m_scriptPauseQueue ? 1 : 0;
	s.nzb_cleanup_disk = m_nzbCleanupDisk ? 1 : 0;
	s.form_auth = m_formAuth ? 1 : 0;
	s.json_p = m_jsonP ? 1 : 0;
	s.secure_control = m_secureControl ? 1 : 0;
	s.unpack = m_unpack ? 1 : 0;
	s.direct_unpack = m_directUnpack ? 1 : 0;
	s.use_temp_unpack_dir = m_useTempUnpackDir ? 1 : 0;
	s.unpack_cleanup_disk = m_unpackCleanupDisk ? 1 : 0;
	s.unpack_pause_queue = m_unpackPauseQueue ? 1 : 0;
	s.unpack_mutex_enforced = m_unpackMutexEnforced ? 1 : 0;
	s.url_force = m_urlForce ? 1 : 0;
	s.cert_check = m_certCheck ? 1 : 0;
	s.reorder_files = m_reorderFiles ? 1 : 0;
	s.post_download_rename = m_postDownloadRename ? 1 : 0;
	s.rename_after_unpack = m_renameAfterUnpack ? 1 : 0;
	s.output_mode = (int)m_outputMode;
	s.par_check = (int)m_parCheck;
	s.par_scan = (int)m_parScan;
	s.dupe_article_fallback = (int)m_dupeArticleFallback;
	s.dupe_stream_decompress = m_dupeStreamDecompress ? 1 : 0;
	s.dupe_stream_timeout = m_dupeStreamTimeout;
	s.dupe_search = m_dupeSearch ? 1 : 0;
	s.dupe_search_dry_run = m_dupeSearchDryRun ? 1 : 0;
	s.dupe_search_delay = m_dupeSearchDelay;
	s.dupe_search_max_donors = m_dupeSearchMaxDonors;
	s.dupe_health_percent = m_dupeHealthPercent;
	s.dupe_health_min = m_dupeHealthMin;
	s.dupe_health_max = m_dupeHealthMax;
	s.dupe_body_checks = m_dupeBodyChecks;
	s.dupe_health_budget = m_dupeHealthBudget;
	s.dupe_min_alive = m_dupeMinAlive;
	s.dupe_fast_donors = m_dupeFastDonors;
	s.post_strategy = (int)m_postStrategy;
	s.file_naming = (int)m_fileNaming;
	s.health_check = (int)m_healthCheck;
	s.info_target = (int)m_infoTarget;
	s.warning_target = (int)m_warningTarget;
	s.error_target = (int)m_errorTarget;
	s.debug_target = (int)m_debugTarget;
	s.detail_target = (int)m_detailTarget;
	s.write_log = (int)m_writeLog;
	s.tls = m_tls ? 1 : 0;
	return s;
}

void Options::ApplyParsedScalars(const NzbgetRsParsedOptions& s)
{
	m_downloadRate = s.download_rate;
	m_articleTimeout = s.article_timeout;
	m_articleReadChunkSize = s.article_read_chunk_size;
	m_urlTimeout = s.url_timeout;
	m_remoteTimeout = s.remote_timeout;
	m_articleRetries = s.article_retries;
	m_articleInterval = s.article_interval;
	m_urlRetries = s.url_retries;
	m_urlInterval = s.url_interval;
	m_controlPort = s.control_port;
	m_securePort = s.secure_port;
	m_urlConnections = s.url_connections;
	m_logBuffer = s.log_buffer;
	m_rotateLog = s.rotate_log;
	m_umask = s.umask;
	m_updateInterval = s.update_interval;
	m_writeBuffer = s.write_buffer;
	m_nzbDirInterval = s.nzb_dir_interval;
	m_nzbDirFileAge = s.nzb_dir_file_age;
	m_nzbDirArchiveScan = s.nzb_dir_archive_scan != 0;
	m_nzbDirArchiveAction = (ENzbDirArchiveAction)s.nzb_dir_archive_action;
	m_diskSpace = s.disk_space;
	m_parTimeLimit = s.par_time_limit;
	m_keepHistory = s.keep_history;
	m_feedHistory = s.feed_history;
	m_timeCorrection = s.time_correction;
	m_propagationDelay = s.propagation_delay;
	m_articleCache = s.article_cache;
	m_memoryPressureInterval = s.memory_pressure_interval;
	m_eventInterval = s.event_interval;
	m_parBuffer = s.par_buffer;
	m_parThreads = s.par_threads;
	m_monthlyQuota = s.monthly_quota;
	m_quotaStartDay = s.quota_start_day;
	m_dailyQuota = s.daily_quota;
	m_systemHealthCheck = s.system_health_check != 0;
	m_nzbLog = s.nzb_log != 0;
	m_appendCategoryDir = s.append_category_dir != 0;
	m_continuePartial = s.continue_partial != 0;
	m_flushQueue = s.flush_queue != 0;
	m_dupeCheck = s.dupe_check != 0;
	m_parRepair = s.par_repair != 0;
	m_parQuick = s.par_quick != 0;
	m_parRename = s.par_rename != 0;
	m_rarRename = s.rar_rename != 0;
	m_directRename = s.direct_rename != 0;
	m_hardLinking = s.hard_linking != 0;
	m_cursesNzbName = s.curses_nzb_name != 0;
	m_cursesTime = s.curses_time != 0;
	m_cursesGroup = s.curses_group != 0;
	m_crcCheck = s.crc_check != 0;
	m_directWrite = s.direct_write != 0;
	m_rawArticle = s.raw_article != 0;
	m_skipWrite = s.skip_write != 0;
	m_crashTrace = s.crash_trace != 0;
	m_crashDump = s.crash_dump != 0;
	m_parPauseQueue = s.par_pause_queue != 0;
	m_scriptPauseQueue = s.script_pause_queue != 0;
	m_nzbCleanupDisk = s.nzb_cleanup_disk != 0;
	m_formAuth = s.form_auth != 0;
	m_jsonP = s.json_p != 0;
	m_secureControl = s.secure_control != 0;
	m_unpack = s.unpack != 0;
	m_directUnpack = s.direct_unpack != 0;
	m_useTempUnpackDir = s.use_temp_unpack_dir != 0;
	m_unpackCleanupDisk = s.unpack_cleanup_disk != 0;
	m_unpackPauseQueue = s.unpack_pause_queue != 0;
	m_unpackMutexEnforced = s.unpack_mutex_enforced != 0;
	m_urlForce = s.url_force != 0;
	m_certCheck = s.cert_check != 0;
	m_reorderFiles = s.reorder_files != 0;
	m_postDownloadRename = s.post_download_rename != 0;
	m_renameAfterUnpack = s.rename_after_unpack != 0;
	m_outputMode = (EOutputMode)s.output_mode;
	m_parCheck = (EParCheck)s.par_check;
	m_parScan = (EParScan)s.par_scan;
	m_dupeArticleFallback = (EDupeArticleFallback)s.dupe_article_fallback;
	m_dupeStreamDecompress = s.dupe_stream_decompress != 0;
	m_dupeStreamTimeout = s.dupe_stream_timeout;
	m_dupeSearch = s.dupe_search != 0;
	m_dupeSearchDryRun = s.dupe_search_dry_run != 0;
	m_dupeSearchDelay = s.dupe_search_delay;
	m_dupeSearchMaxDonors = s.dupe_search_max_donors;
	m_dupeHealthPercent = s.dupe_health_percent;
	m_dupeHealthMin = s.dupe_health_min;
	m_dupeHealthMax = s.dupe_health_max;
	m_dupeBodyChecks = s.dupe_body_checks;
	m_dupeHealthBudget = s.dupe_health_budget;
	m_dupeMinAlive = s.dupe_min_alive;
	m_dupeFastDonors = s.dupe_fast_donors;
	m_postStrategy = (EPostStrategy)s.post_strategy;
	m_fileNaming = (EFileNaming)s.file_naming;
	m_healthCheck = (EHealthCheck)s.health_check;
	m_infoTarget = (EMessageTarget)s.info_target;
	m_warningTarget = (EMessageTarget)s.warning_target;
	m_errorTarget = (EMessageTarget)s.error_target;
	m_debugTarget = (EMessageTarget)s.debug_target;
	m_detailTarget = (EMessageTarget)s.detail_target;
	m_writeLog = (EWriteLog)s.write_log;
	m_tls = s.tls != 0;
}

void Options::ConfigError(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	printf("%s(%i): %s\n", m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>", m_configLine, tmp2);
	error("%s(%i): %s", m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>", m_configLine, tmp2);

	m_configErrors = true;
}

void Options::ConfigWarn(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	const char* filename = m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>";
	printf("%s(%i): %s\n", filename, m_configLine, tmp2);
	warn("%s(%i): %s", filename, m_configLine, tmp2);
}

void Options::InitDefaults()
{
#ifdef DISABLE_CURSES
	int disableCurses = 1;
#else
	int disableCurses = 0;
#endif
	nzbget_rs_options_init_defaults(
		disableCurses,
		[](void* ctx, const char* name, const char* value)
		{
			static_cast<Options*>(ctx)->SetOption(name, value);
		},
		this);
}

void Options::InitOptFile()
{
	if (!m_configFilename && !m_noConfig)
	{
		// search for config file in default locations
		// look in the exe-directory first
		BString<1024> filename;
		{
			NzbgetRsBuf cfBuf = nzbget_rs_tpr_options_format_default_conf_path(*m_appDir, '/');
			filename.Set(cfBuf.data ? cfBuf.data : "", (int)cfBuf.len);
			nzbget_rs_free(cfBuf);
		}

		if (FileSystem::FileExists(filename))
		{
			m_configFilename = filename;
		}
		else
		{
			int p = 0;
			while (const char* altfilename = PossibleConfigLocations[p++])
			{
				// substitute HOME-variable
				filename = FileSystem::ExpandHomePath(altfilename);

				if (FileSystem::FileExists(filename))
				{
					m_configFilename = *filename;
					break;
				}
			}
		}
	}

	if (m_configFilename)
	{
		// normalize path in filename
		CString filename = FileSystem::ExpandFileName(m_configFilename);

		// substitute HOME-variable
		filename = FileSystem::ExpandHomePath(filename);

		m_configFilename = *filename;
		m_configFilePath = fs::u8path(*m_configFilename);

		SetOption(CONFIGFILE.data(), m_configFilename);

		LoadConfigFile();
	}
}

void Options::CheckDir(CString& dir, const char* optionName,
	const char* parentDir, bool allowEmpty, bool create)
{
	NzbgetRsOptionsHost host = MakeHost();
	int hasDir = 0;
	NzbgetRsBuf buf = nzbget_rs_options_check_dir(
		&host, m_noDiskAccess ? 1 : 0, optionName, parentDir,
		allowEmpty ? 1 : 0, create ? 1 : 0, &hasDir);
	if (hasDir)
	{
		dir = buf.data ? buf.data : "";
	}
	else
	{
		dir = nullptr;
	}
	nzbget_rs_free(buf);
}

// ScriptDir may list several folders: each is made absolute and created on
// its own (as one path, "a;b" created junk folders such as "a;")
void Options::CheckScriptDir()
{
	NzbgetRsOptionsHost host = MakeHost();
	int hasDir = 0;
	NzbgetRsBuf buf = nzbget_rs_options_check_script_dir(
		&host, m_noDiskAccess ? 1 : 0, m_mainDir, &hasDir);
	if (hasDir)
	{
		m_scriptDir = buf.data ? buf.data : "";
	}
	else
	{
		m_scriptDir = nullptr;
	}
	nzbget_rs_free(buf);
}

void Options::CheckDirs()
{
	m_mainDir = GetOption(MAINDIR.data());

	CheckDir(m_destDir, DESTDIR.data(), m_mainDir, false, true);
	CheckDir(m_interDir, INTERDIR.data(), m_mainDir, true, true);
	CheckDir(m_tempDir, TEMPDIR.data(), m_mainDir, false, true);
	CheckDir(m_queueDir, QUEUEDIR.data(), m_mainDir, false, true);
	CheckDir(m_webDir, WEBDIR.data(), nullptr, true, false);
	CheckScriptDir();
	CheckDir(m_nzbDir, NZBDIR.data(), m_mainDir, false, true);

	m_mainDirPath = fs::u8path(*m_mainDir);
	m_destDirPath = fs::u8path(*m_destDir);
	m_interDirPath = fs::u8path(*m_interDir);
	m_tempDirPath = fs::u8path(*m_tempDir);
	m_queueDirPath = fs::u8path(*m_queueDir);
	m_webDirPath = fs::u8path(*m_webDir);
	m_nzbDirPath = fs::u8path(*m_nzbDir);

	Tokenizer tokDir(g_Options->GetScriptDir(), ",;");
	while (const char* scriptDir = tokDir.Next())
	{
		fs::path path;
		path = fs::u8path(scriptDir);
		m_scriptDirPaths.push_back(std::move(path));
	}
}

void Options::InitOptions()
{
	m_requiredDir = GetOption(REQUIREDDIR.data());

	m_configTemplate		= GetOption(CONFIGTEMPLATE.data());
	m_scriptOrder			= GetOption(SCRIPTORDER.data());
	m_extensions			= GetOption(EXTENSIONS.data());
	m_controlIp				= GetOption(CONTROLIP.data());
	m_controlUsername		= GetOption(CONTROLUSERNAME.data());
	m_controlPassword		= GetOption(CONTROLPASSWORD.data());
	m_restrictedUsername	= GetOption(RESTRICTEDUSERNAME.data());
	m_restrictedPassword	= GetOption(RESTRICTEDPASSWORD.data());
	m_addUsername			= GetOption(ADDUSERNAME.data());
	m_addPassword			= GetOption(ADDPASSWORD.data());
	m_secureCert			= GetOption(SECURECERT.data());
	m_secureKey				= GetOption(SECUREKEY.data());
	m_certStore				= GetOption(CERTSTORE.data());
	m_authorizedIp			= GetOption(AUTHORIZEDIP.data());
	m_lockFile				= GetOption(LOCKFILE.data());
	m_daemonUsername		= GetOption(DAEMONUSERNAME.data());
	m_logFile				= GetOption(LOGFILE.data());
	m_unrarCmd				= GetOption(UNRARCMD.data());
	m_sevenZipCmd			= GetOption(SEVENZIPCMD.data());
	m_unpackPassFile		= GetOption(UNPACKPASSFILE.data());
	m_extCleanupDisk		= GetOption(EXTCLEANUPDISK.data());
	m_parIgnoreExt			= GetOption(PARIGNOREEXT.data());
	m_unpackIgnoreExt		= GetOption(UNPACKIGNOREEXT.data());

	m_configTemplatePath = fs::u8path(*m_configTemplate);
	m_secureCertPath = fs::u8path(*m_secureCert);
	m_secureKeyPath = fs::u8path(*m_secureKey);
	m_certStorePath = fs::u8path(*m_certStore);
	m_lockFilePath = fs::u8path(*m_lockFile);
	m_logFilePath = fs::u8path(*m_logFile);
	m_unpackPassFilePath = fs::u8path(*m_unpackPassFile);
	SetToolPathOption(m_unrarPath, *m_unrarCmd);
	SetToolPathOption(m_sevenZipPath, *m_sevenZipCmd);

	const char* shellOverride = GetOption(SHELLOVERRIDE.data());
	m_shellOverride	= shellOverride ? shellOverride : "";

	m_renameIgnoreExt 	    = GetOption(RENAMEIGNOREEXT.data());

	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars{};
	int hasMemPath = 0, hasHlExt = 0, hasDupeUrl = 0, hasDupeKey = 0, warnDupeNoUrl = 0;
	NzbgetRsBuf memPathBuf{}, hlExtBuf{}, dupeUrlBuf{}, dupeKeyBuf{};
	nzbget_rs_options_init_scalars(
		&host, &scalars,
		&hasMemPath, &memPathBuf,
		&hasHlExt, &hlExtBuf,
		&hasDupeUrl, &dupeUrlBuf,
		&hasDupeKey, &dupeKeyBuf,
		&warnDupeNoUrl);
	m_memoryPressurePath = hasMemPath ? (memPathBuf.data ? memPathBuf.data : "") : nullptr;
	m_hardLinkingIgnoreExt = hasHlExt ? (hlExtBuf.data ? hlExtBuf.data : "") : nullptr;
	m_dupeSearchUrl = hasDupeUrl ? (dupeUrlBuf.data ? dupeUrlBuf.data : "") : nullptr;
	m_dupeSearchApiKey = hasDupeKey ? (dupeKeyBuf.data ? dupeKeyBuf.data : "") : nullptr;
	nzbget_rs_free(memPathBuf);
	nzbget_rs_free(hlExtBuf);
	nzbget_rs_free(dupeUrlBuf);
	nzbget_rs_free(dupeKeyBuf);
	ApplyParsedScalars(scalars);

	if (warnDupeNoUrl)
	{
		warn("Option <%s> is enabled but option <%s> is empty: no duplicates will be searched",
			DUPESEARCH.data(), DUPESEARCHURL.data());
	}
}

void Options::SetOption(const char* optname, const char* value)
{
	OptEntry* optEntry = FindOption(optname);
	if (!optEntry)
	{
		m_optEntries.emplace_back(optname, nullptr);
		optEntry = &m_optEntries.back();
	}

	CString curvalue;

	if (nzbget_rs_dqh_options_should_expand_home_value(value, m_noDiskAccess ? 1 : 0) != 0)
	{
		curvalue = FileSystem::ExpandHomePath(value);
	}
	else
	{
		curvalue = value;
	}

	optEntry->SetLineNo(m_configLine);

	if (curvalue)
	{
		NzbgetRsBuf expanded = nzbget_rs_options_expand_variables(
			curvalue,
			[](void* ctx, const char* name) -> const char*
			{
				return static_cast<Options*>(ctx)->GetOption(name);
			},
			this);
		curvalue.Set(expanded.data ? expanded.data : "", (int)expanded.len);
		nzbget_rs_free(expanded);
	}

	optEntry->SetValue(curvalue);
}

void Options::SetToolPathOption(fs::path& pathOpt, std::string_view value)
{
	if (value.empty()) return;

	NzbgetRsBuf resolved = nzbget_rs_options_resolve_tool_path(value.data(), value.size());
	std::string_view sv(resolved.data ? resolved.data : "", resolved.len);
	fs::path directPath = fs::u8path(sv);
	nzbget_rs_free(resolved);
	pathOpt.swap(directPath);
}

Options::OptEntry* Options::FindOption(const char* optname)
{
	OptEntry* optEntry = m_optEntries.FindOption(optname);

	// normalize option name in option list; for example "server1.joingroup" -> "Server1.JoinGroup"
	if (optEntry && nzbget_rs_dqh_bytes_equal(optEntry->GetName(), optname) == 0)
	{
		optEntry->SetName(optname);
	}

	return optEntry;
}

const char* Options::GetOption(const char* optname)
{
	OptEntry* optEntry = FindOption(optname);
	if (optEntry)
	{
		m_configLine = nzbget_rs_dqh_options_update_config_line_on_get(
			optEntry->GetLineNo(), m_configLine);
		return optEntry->GetValue();
	}
	return nullptr;
}

void Options::InitServers()
{
	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars = ExportParsedScalars();
#ifdef DISABLE_TLS
	int disableTls = 1;
#else
	int disableTls = 0;
#endif
	nzbget_rs_options_init_servers(&host, disableTls, &scalars);
	ApplyParsedScalars(scalars);
}

void Options::InitCategories()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_categories(&host, m_noDiskAccess ? 1 : 0, m_destDir);
}

void Options::InitFeeds()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_feeds(&host);
}

void Options::InitScheduler()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_scheduler(&host);
}

void Options::CreateSchedulerTask(int id, const char* time, const char* weekDays,
	ESchedulerCommand command, const char* param)
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_create_scheduler_task(&host, id, time, weekDays, (int)command, param);
}

void Options::LoadConfigFile()
{
	SetOption(CONFIGFILE.data(), m_configFilename);

	DiskFile infile;

	if (!infile.Open(m_configFilename, DiskFile::omRead))
	{
		ConfigError("Could not open file %s", *m_configFilename);
		m_fatalError = true;
		return;
	}

	m_configLine = 0;
	int bufLen = (int)FileSystem::FileSize(m_configFilename) + 1;
	CharBuffer buf(bufLen);

	int line = 0;
	while (infile.ReadLine(buf, buf.Size() - 1))
	{
		m_configLine = ++line;

		if (nzbget_rs_rco_options_trim_and_check_config_line(buf))
		{
			continue;
		}

		SetOptionString(buf);
	}

	infile.Close();

	m_configLine = 0;
}

void Options::InitCommandLineOptions(CmdOptList* commandLineOptions)
{
	for (const char* option : *commandLineOptions)
	{
		SetOptionString(option);
	}
}

bool Options::SetOptionString(const char* option)
{
	CString optname;
	CString optvalue;

	if (!SplitOptionString(option, optname, optvalue))
	{
		ConfigError("Invalid option \"%s\"", option);
		return false;
	}

	bool ok = ValidateOptionName(optname, optvalue);
	if (ok)
	{
		SetOption(optname, optvalue);
	}
	else
	{
		ConfigError("Invalid option \"%s\"", *optname);
	}

	return ok;
}

/*
 * Splits option string into name and value;
 * Converts old names and values if necessary;
 * Returns true if the option string has name and value;
 */
bool Options::SplitOptionString(const char* option, CString& optName, CString& optValue)
{
	NzbgetRsBuf nameBuf{}, valBuf{};
	if (!nzbget_rs_options_split_and_convert(option, &nameBuf, &valBuf))
	{
		return false;
	}

	optName.Set(nameBuf.data ? nameBuf.data : "", (int)nameBuf.len);
	optValue.Set(valBuf.data ? valBuf.data : "", (int)valBuf.len);
	nzbget_rs_free(nameBuf);
	nzbget_rs_free(valBuf);

	return true;
}

bool Options::ValidateOptionName(const char* optname, const char* optvalue)
{
	// rust/src/options.rs: the name's class; the messages are logged here
	auto predefined = [](void* ctx, const char* name) -> int
	{
		return static_cast<Options*>(ctx)->GetOption(name) != nullptr;
	};
	switch (nzbget_rs_validate_option_name(optname, predefined, this))
	{
		case 1:
			return true;

		case 2:
			ConfigWarn("Option \"%s\" is obsolete, ignored", optname);
			return true;

		case 3:
			if (!Util::EmptyStr(optvalue))
			{
				ConfigError("Option \"%s\" is obsolete, ignored, use \"%s\" and \"%s\" instead",
					optname, SCRIPTDIR.data(), EXTENSIONS.data());
			}
			return true;

		case 4:
			ConfigWarn("Option \"%s\" is obsolete, ignored, use \"%s\" instead", optname, WRITELOG.data());
			return true;

		default:
			return false;
	}
}

void Options::CheckOptions()
{
	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars = ExportParsedScalars();
#ifdef DISABLE_PARCHECK
	int disableParcheck = 1;
#else
	int disableParcheck = 0;
#endif
#ifdef DISABLE_CURSES
	int disableCurses = 1;
#else
	int disableCurses = 0;
#endif
#ifdef DISABLE_TLS
	int disableTls = 1;
#else
	int disableTls = 0;
#endif
#ifdef HAVE_X509_CHECK_HOST
	int haveX509CheckHost = 1;
#else
	int haveX509CheckHost = 0;
#endif
	int ctChanged = 0;
	NzbgetRsBuf ctBuf = nzbget_rs_options_check_options(
		&host, &scalars,
		m_noDiskAccess ? 1 : 0,
		sizeof(void*) == 4 ? 1 : 0,
		disableParcheck, disableCurses, disableTls, haveX509CheckHost,
		m_certStore.Empty() ? 1 : 0,
		m_webDir ? *m_webDir : "",
		m_configTemplate ? *m_configTemplate : nullptr,
		m_unpackPassFile ? *m_unpackPassFile : nullptr,
		&ctChanged);
	if (ctChanged)
	{
		m_configTemplate.Set(ctBuf.data ? ctBuf.data : "", (int)ctBuf.len);
	}
	nzbget_rs_free(ctBuf);
	ApplyParsedScalars(scalars);
}

void Options::ConvertOldOptions(OptEntries* optEntries)
{
	if (!optEntries) return;
	std::vector<const char*> names;
	std::vector<const char*> values;
	names.reserve(optEntries->size());
	values.reserve(optEntries->size());
	for (const OptEntry& opt : *optEntries)
	{
		names.push_back(opt.GetName() ? opt.GetName() : "");
		values.push_back(opt.GetValue() ? opt.GetValue() : "");
	}
	nzbget_rs_options_convert_old_options(
		names.data(),
		values.data(),
		names.size(),
		optEntries,
		[](void* ctx, size_t idx, const char* name, const char* value)
		{
			auto* entries = static_cast<OptEntries*>(ctx);
			if (idx < entries->size())
			{
				(*entries)[idx].m_value = value;
			}
			else
			{
				entries->emplace_back(name, value);
			}
		});
}
