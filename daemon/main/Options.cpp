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

const char* BoolNames[] = { "yes", "no", "true", "false", "1", "0", "on", "off", "enable", "disable", "enabled", "disabled" };
const int BoolValues[] = { 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0 };
const int BoolCount = 12;

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

	for (OptEntry& optEntry : this)
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

	for (Category& category : this)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(category.GetName(), name) != 0)
		{
			return &category;
		}
	}

	if (searchAliases)
	{
		for (Category& category : this)
		{
			for (CString& alias : category.GetAliases())
			{
				WildMask mask(alias);
				if (mask.Match(name))
				{
					return &category;
				}
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

void Options::LocateOptionSrcPos(const char *optionName)
{
	OptEntry* optEntry = FindOption(optionName);
	if (optEntry)
	{
		m_configLine = optEntry->GetLineNo();
	}
	else
	{
		m_configLine = 0;
	}
}

void Options::InitDefaults()
{
	SetOption(MAINDIR.data(), "~/downloads");
	SetOption(WEBDIR.data(), "");
	SetOption(CONFIGTEMPLATE.data(), "");
	SetOption(TEMPDIR.data(), "${MainDir}/tmp");
	SetOption(DESTDIR.data(), "${MainDir}/dst");
	SetOption(INTERDIR.data(), "");
	SetOption(QUEUEDIR.data(), "${MainDir}/queue");
	SetOption(NZBDIR.data(), "${MainDir}/nzb");
	SetOption(LOCKFILE.data(), "${MainDir}/nzbget.lock");
	SetOption(LOGFILE.data(), "${MainDir}/nzbget.log");
	SetOption(SCRIPTDIR.data(), "${MainDir}/scripts");
	SetOption(REQUIREDDIR.data(), "");
	SetOption(WRITELOG.data(), "append");
	SetOption(ROTATELOG.data(), "3");
	SetOption(APPENDCATEGORYDIR.data(), "yes");
#ifdef DISABLE_CURSES
	SetOption(OUTPUTMODE.data(), "color");
#else
	SetOption(OUTPUTMODE.data(), "curses");
#endif
	SetOption(DUPECHECK.data(), "yes");
	SetOption(DUPEARTICLEFALLBACK.data(), "no");
	SetOption(DUPESTREAMDECOMPRESS.data(), "no");
	SetOption(DUPESTREAMTIMEOUT.data(), "60");
	SetOption(DUPESEARCH.data(), "no");
	SetOption(DUPESEARCHDRYRUN.data(), "no");
	SetOption(DUPESEARCHURL.data(), "");
	SetOption(DUPESEARCHAPIKEY.data(), "");
	SetOption(DUPESEARCHDELAY.data(), "20");
	SetOption(DUPESEARCHMAXDONORS.data(), "0");
	SetOption(DUPEHEALTHPERCENT.data(), "5");
	SetOption(DUPEHEALTHMIN.data(), "50");
	SetOption(DUPEHEALTHMAX.data(), "1000");
	SetOption(DUPEBODYCHECKS.data(), "20");
	SetOption(DUPEHEALTHBUDGET.data(), "120");
	SetOption(DUPEMINALIVE.data(), "50");
	SetOption(DUPEFASTDONORS.data(), "5");
	SetOption(DOWNLOADRATE.data(), "0");
	SetOption(CONTROLIP.data(), "0.0.0.0");
	SetOption(CONTROLUSERNAME.data(), "nzbget");
	SetOption(CONTROLPASSWORD.data(), "tegbzn6789");
	SetOption(RESTRICTEDUSERNAME.data(), "");
	SetOption(RESTRICTEDPASSWORD.data(), "");
	SetOption(ADDUSERNAME.data(), "");
	SetOption(ADDPASSWORD.data(), "");
	SetOption(CONTROLPORT.data(), "6789");
	SetOption(FORMAUTH.data(), "no");
	SetOption(JSONP.data(), "no");
	SetOption(SECURECONTROL.data(), "no");
	SetOption(SECUREPORT.data(), "6791");
	SetOption(SECURECERT.data(), "");
	SetOption(SECUREKEY.data(), "");
	SetOption(CERTSTORE.data(), "");
	SetOption(CERTCHECK.data(), "no");
	SetOption(AUTHORIZEDIP.data(), "");
	SetOption(ARTICLETIMEOUT.data(), "60");
	SetOption(ARTICLEREADCHUNKSIZE.data(), "4");
	SetOption(URLTIMEOUT.data(), "60");
	SetOption(REMOTETIMEOUT.data(), "90");
	SetOption(FLUSHQUEUE.data(), "yes");
	SetOption(SYSTEMHEALTHCHECK.data(), "yes");
	SetOption(NZBLOG.data(), "yes");
	SetOption(RAWARTICLE.data(), "no");
	SetOption(SKIPWRITE.data(), "no");
	SetOption(ARTICLERETRIES.data(), "3");
	SetOption(ARTICLEINTERVAL.data(), "10");
	SetOption(URLRETRIES.data(), "3");
	SetOption(URLINTERVAL.data(), "10");
	SetOption(CONTINUEPARTIAL.data(), "no");
	SetOption(URLCONNECTIONS.data(), "4");
	SetOption(LOGBUFFER.data(), "1000");
	SetOption(INFOTARGET.data(), "both");
	SetOption(WARNINGTARGET.data(), "both");
	SetOption(ERRORTARGET.data(), "both");
	SetOption(DEBUGTARGET.data(), "none");
	SetOption(DETAILTARGET.data(), "both");
	SetOption(PARCHECK.data(), "auto");
	SetOption(PARREPAIR.data(), "yes");
	SetOption(PARSCAN.data(), "extended");
	SetOption(PARQUICK.data(), "yes");
	SetOption(POSTSTRATEGY.data(), "sequential");
	SetOption(FILENAMING.data(), "article");
	SetOption(POSTDOWNLOADRENAME.data(), "yes");
	SetOption(RENAMEAFTERUNPACK.data(), "yes");
	SetOption(RENAMEIGNOREEXT.data(), "");
	SetOption(PARRENAME.data(), "yes");
	SetOption(PARBUFFER.data(), "16");
	SetOption(PARTHREADS.data(), "0");
	SetOption(RARRENAME.data(), "yes");
	SetOption(HEALTHCHECK.data(), "none");
	SetOption(DIRECTRENAME.data(), "no");
	SetOption(HARDLINKING.data(), "no");
	SetOption(HARDLINKINGIGNOREEXT.data(), ".zip, .7z, .rar, *.7z.###, *.r##");
	SetOption(SCRIPTORDER.data(), "");
	SetOption(EXTENSIONS.data(), "");
	SetOption(DAEMONUSERNAME.data(), "root");
	SetOption(UMASK.data(), "1000");
	SetOption(UPDATEINTERVAL.data(), "200");
	SetOption(CURSESNZBNAME.data(), "yes");
	SetOption(CURSESTIME.data(), "no");
	SetOption(CURSESGROUP.data(), "no");
	SetOption(CRCCHECK.data(), "yes");
	SetOption(DIRECTWRITE.data(), "yes");
	SetOption(WRITEBUFFER.data(), "0");
	SetOption(NZBDIRINTERVAL.data(), "5");
	SetOption(NZBDIRFILEAGE.data(), "60");
	SetOption(NZBDIRARCHIVESCAN.data(), "yes");
	SetOption(NZBDIRARCHIVEACTION.data(), "delete");
	SetOption(DISKSPACE.data(), "250");
	SetOption(CRASHTRACE.data(), "no");
	SetOption(CRASHDUMP.data(), "no");
	SetOption(PARPAUSEQUEUE.data(), "no");
	SetOption(SCRIPTPAUSEQUEUE.data(), "no");
	SetOption(NZBCLEANUPDISK.data(), "no");
	SetOption(PARTIMELIMIT.data(), "0");
	SetOption(KEEPHISTORY.data(), "7");
	SetOption(UNPACK.data(), "no");
	SetOption(DIRECTUNPACK.data(), "no");
	SetOption(USETEMPUNPACKDIR.data(), "yes");
	SetOption(UNPACKCLEANUPDISK.data(), "no");
	SetOption(UNRARCMD.data(), "unrar");
	SetOption(SEVENZIPCMD.data(), "7z");
	SetOption(UNPACKPASSFILE.data(), "");
	SetOption(UNPACKPAUSEQUEUE.data(), "no");
	SetOption(EXTCLEANUPDISK.data(), "");
	SetOption(PARIGNOREEXT.data(), "");
	SetOption(UNPACKIGNOREEXT.data(), "");
	SetOption(FEEDHISTORY.data(), "7");
	SetOption(URLFORCE.data(), "yes");
	SetOption(TIMECORRECTION.data(), "0");
	SetOption(PROPAGATIONDELAY.data(), "0");
	SetOption(ARTICLECACHE.data(), "0");
	SetOption(MEMORYPRESSUREINTERVAL.data(), "500");
	SetOption(MEMORYPRESSUREPATH.data(), "");
	SetOption(EVENTINTERVAL.data(), "0");
	SetOption(SHELLOVERRIDE.data(), "");
	SetOption(MONTHLYQUOTA.data(), "0");
	SetOption(QUOTASTARTDAY.data(), "1");
	SetOption(DAILYQUOTA.data(), "0");
	SetOption(REORDERFILES.data(), "no");
	SetOption(UPDATECHECK.data(), "none");
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
	const char* tempdir = GetOption(optionName);

	if (m_noDiskAccess)
	{
		dir = tempdir;
		return;
	}

	if (Util::EmptyStr(tempdir))
	{
		if (!allowEmpty)
		{
			ConfigError("Invalid value for option \"%s\": <empty>", optionName);
		}
		dir = "";
		return;
	}

	dir = tempdir;
	FileSystem::NormalizePathSeparators((char*)dir);
	size_t strippedLen = nzbget_rs_tpr_options_strip_trailing_sep(dir, PATH_SEPARATOR);
	if (strippedLen < (size_t)dir.Length())
	{
		// remove trailing slash
		dir[(int)strippedLen] = '\0';
	}

	int joinMode = nzbget_rs_rol_options_check_dir_join_mode(
		dir, parentDir, PATH_SEPARATOR, ALT_PATH_SEPARATOR);
	if (joinMode != 0)
	{
		BString<1024> usedir2;
		{
			NzbgetRsBuf joinBuf = nzbget_rs_tpr_options_format_joined_dir(
				parentDir, PATH_SEPARATOR, *dir, joinMode);
			usedir2.Set(joinBuf.data ? joinBuf.data : "", (int)joinBuf.len);
			nzbget_rs_free(joinBuf);
		}

		FileSystem::NormalizePathSeparators((char*)usedir2);
		dir = usedir2;
		SetOption(optionName, usedir2);
	}

	// Ensure the dir is created
	CString errmsg;
	if (create)
	{
		if (!FileSystem::ForceDirectories(dir, errmsg))
		{
			ConfigError("Invalid value for option \"%s\" (%s): %s", optionName, *dir, *errmsg);
			return;
		}

		if (getuid() == 0 || geteuid() == 0)
		{
			struct passwd* pwd = getpwnam(GetOption(DAEMONUSERNAME.data()));
			if (pwd != nullptr)
			{
				chown(dir, pwd->pw_uid, pwd->pw_gid);
			}
		}
	}
}

// ScriptDir may list several folders: each is made absolute and created on
// its own (as one path, "a;b" created junk folders such as "a;")
void Options::CheckScriptDir()
{
	const char* value = GetOption(SCRIPTDIR.data());
	if (m_noDiskAccess || Util::EmptyStr(value) || !strpbrk(value, ",;"))
	{
		CheckDir(m_scriptDir, SCRIPTDIR.data(), m_mainDir, true, true);
		return;
	}

	CString list = value;
	CString result;
	Tokenizer tok(list, ",;");
	while (const char* item = tok.Next())
	{
		SetOption(SCRIPTDIR.data(), item);
		CString dir;
		CheckDir(dir, SCRIPTDIR.data(), m_mainDir, true, true);
		if (!dir.Empty())
		{
			if (!result.Empty())
			{
				result.Append(";");
			}
			result.Append(dir);
		}
	}

	SetOption(SCRIPTDIR.data(), result);
	m_scriptDir = *result;
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

	m_downloadRate			= nzbget_rs_tpr_webdl_kb_to_bytes(ParseIntValue(DOWNLOADRATE.data(), 10));
	m_articleTimeout		= ParseIntValue(ARTICLETIMEOUT.data(), 10);
	m_articleReadChunkSize  = nzbget_rs_tpr_webdl_kb_to_bytes(ParseIntValue(ARTICLEREADCHUNKSIZE.data(), 10));
	m_urlTimeout			= ParseIntValue(URLTIMEOUT.data(), 10);
	m_remoteTimeout			= ParseIntValue(REMOTETIMEOUT.data(), 10);
	m_articleRetries		= ParseIntValue(ARTICLERETRIES.data(), 10);
	m_articleInterval		= ParseIntValue(ARTICLEINTERVAL.data(), 10);
	m_urlRetries			= ParseIntValue(URLRETRIES.data(), 10);
	m_urlInterval			= ParseIntValue(URLINTERVAL.data(), 10);
	m_controlPort			= ParseIntValue(CONTROLPORT.data(), 10);
	m_securePort			= ParseIntValue(SECUREPORT.data(), 10);
	m_urlConnections		= ParseIntValue(URLCONNECTIONS.data(), 10);
	m_logBuffer				= ParseIntValue(LOGBUFFER.data(), 10);
	m_rotateLog				= ParseIntValue(ROTATELOG.data(), 10);
	m_umask					= ParseIntValue(UMASK.data(), 8);
	m_updateInterval		= ParseIntValue(UPDATEINTERVAL.data(), 10);
	m_writeBuffer			= ParseIntValue(WRITEBUFFER.data(), 10);
	m_nzbDirInterval		= ParseIntValue(NZBDIRINTERVAL.data(), 10);
	m_nzbDirFileAge			= ParseIntValue(NZBDIRFILEAGE.data(), 10);
	m_nzbDirArchiveScan		= (bool)ParseEnumValue(NZBDIRARCHIVESCAN.data(), BoolCount, BoolNames, BoolValues);
	const char* ArchiveActions[] = { "move", "delete" };
	const int ArchiveActionValues[] = { ENzbDirArchiveAction::Move, ENzbDirArchiveAction::Delete };
	const int ArchiveActionsCount = 2;
	m_nzbDirArchiveAction 	= (ENzbDirArchiveAction)ParseEnumValue(NZBDIRARCHIVEACTION.data(), ArchiveActionsCount, ArchiveActions, ArchiveActionValues);
	m_diskSpace				= ParseIntValue(DISKSPACE.data(), 10);
	m_parTimeLimit			= ParseIntValue(PARTIMELIMIT.data(), 10);
	m_keepHistory			= ParseIntValue(KEEPHISTORY.data(), 10);
	m_feedHistory			= ParseIntValue(FEEDHISTORY.data(), 10);
	m_timeCorrection = nzbget_rs_fuq_options_normalize_time_correction(
		ParseIntValue(TIMECORRECTION.data(), 10));
	m_propagationDelay		= nzbget_rs_tpr_options_minutes_to_seconds_i32(ParseIntValue(PROPAGATIONDELAY.data(), 10));
	m_articleCache			= ParseIntValue(ARTICLECACHE.data(), 10);
	m_memoryPressurePath	= GetOption(MEMORYPRESSUREPATH.data());
	int memoryPressureInterval = ParseIntValue(MEMORYPRESSUREINTERVAL.data(), 10);
	m_memoryPressureInterval = memoryPressureInterval > 0 ? memoryPressureInterval : 500;
	m_eventInterval			= ParseIntValue(EVENTINTERVAL.data(), 10);
	m_parBuffer				= ParseIntValue(PARBUFFER.data(), 10);
	m_parThreads			= ParseIntValue(PARTHREADS.data(), 10);
	m_monthlyQuota			= ParseIntValue(MONTHLYQUOTA.data(), 10);
	m_quotaStartDay			= ParseIntValue(QUOTASTARTDAY.data(), 10);
	m_dailyQuota			= ParseIntValue(DAILYQUOTA.data(), 10);

	m_systemHealthCheck		= (bool)ParseEnumValue(SYSTEMHEALTHCHECK.data(), BoolCount, BoolNames, BoolValues);
	m_nzbLog				= (bool)ParseEnumValue(NZBLOG.data(), BoolCount, BoolNames, BoolValues);
	m_appendCategoryDir		= (bool)ParseEnumValue(APPENDCATEGORYDIR.data(), BoolCount, BoolNames, BoolValues);
	m_continuePartial		= (bool)ParseEnumValue(CONTINUEPARTIAL.data(), BoolCount, BoolNames, BoolValues);
	m_flushQueue			= (bool)ParseEnumValue(FLUSHQUEUE.data(), BoolCount, BoolNames, BoolValues);
	m_dupeCheck				= (bool)ParseEnumValue(DUPECHECK.data(), BoolCount, BoolNames, BoolValues);
	m_parRepair				= (bool)ParseEnumValue(PARREPAIR.data(), BoolCount, BoolNames, BoolValues);
	m_parQuick				= (bool)ParseEnumValue(PARQUICK.data(), BoolCount, BoolNames, BoolValues);
	m_parRename				= (bool)ParseEnumValue(PARRENAME.data(), BoolCount, BoolNames, BoolValues);
	m_rarRename				= (bool)ParseEnumValue(RARRENAME.data(), BoolCount, BoolNames, BoolValues);
	m_directRename			= (bool)ParseEnumValue(DIRECTRENAME.data(), BoolCount, BoolNames, BoolValues);
	m_hardLinking			= (bool)ParseEnumValue(HARDLINKING.data(), BoolCount, BoolNames, BoolValues);
	m_hardLinkingIgnoreExt = GetOption(HARDLINKINGIGNOREEXT.data());
	m_cursesNzbName			= (bool)ParseEnumValue(CURSESNZBNAME.data(), BoolCount, BoolNames, BoolValues);
	m_cursesTime			= (bool)ParseEnumValue(CURSESTIME.data(), BoolCount, BoolNames, BoolValues);
	m_cursesGroup			= (bool)ParseEnumValue(CURSESGROUP.data(), BoolCount, BoolNames, BoolValues);
	m_crcCheck				= (bool)ParseEnumValue(CRCCHECK.data(), BoolCount, BoolNames, BoolValues);
	m_directWrite			= (bool)ParseEnumValue(DIRECTWRITE.data(), BoolCount, BoolNames, BoolValues);
	m_rawArticle			= (bool)ParseEnumValue(RAWARTICLE.data(), BoolCount, BoolNames, BoolValues);
	m_skipWrite				= (bool)ParseEnumValue(SKIPWRITE.data(), BoolCount, BoolNames, BoolValues);
	m_crashTrace			= (bool)ParseEnumValue(CRASHTRACE.data(), BoolCount, BoolNames, BoolValues);
	m_crashDump				= (bool)ParseEnumValue(CRASHDUMP.data(), BoolCount, BoolNames, BoolValues);
	m_parPauseQueue			= (bool)ParseEnumValue(PARPAUSEQUEUE.data(), BoolCount, BoolNames, BoolValues);
	m_scriptPauseQueue		= (bool)ParseEnumValue(SCRIPTPAUSEQUEUE.data(), BoolCount, BoolNames, BoolValues);
	m_nzbCleanupDisk		= (bool)ParseEnumValue(NZBCLEANUPDISK.data(), BoolCount, BoolNames, BoolValues);
	m_formAuth				= (bool)ParseEnumValue(FORMAUTH.data(), BoolCount, BoolNames, BoolValues);
	m_jsonP					= (bool)ParseEnumValue(JSONP.data(), BoolCount, BoolNames, BoolValues);
	m_secureControl			= (bool)ParseEnumValue(SECURECONTROL.data(), BoolCount, BoolNames, BoolValues);
	m_unpack				= (bool)ParseEnumValue(UNPACK.data(), BoolCount, BoolNames, BoolValues);
	m_directUnpack			= (bool)ParseEnumValue(DIRECTUNPACK.data(), BoolCount, BoolNames, BoolValues);
	m_useTempUnpackDir		= (bool)ParseEnumValue(USETEMPUNPACKDIR.data(), BoolCount, BoolNames, BoolValues);
	m_unpackCleanupDisk		= (bool)ParseEnumValue(UNPACKCLEANUPDISK.data(), BoolCount, BoolNames, BoolValues);
	m_unpackPauseQueue		= (bool)ParseEnumValue(UNPACKPAUSEQUEUE.data(), BoolCount, BoolNames, BoolValues);
	m_urlForce				= (bool)ParseEnumValue(URLFORCE.data(), BoolCount, BoolNames, BoolValues);
	m_certCheck				= (bool)ParseEnumValue(CERTCHECK.data(), BoolCount, BoolNames, BoolValues);
	m_reorderFiles			= (bool)ParseEnumValue(REORDERFILES.data(), BoolCount, BoolNames, BoolValues);
	m_postDownloadRename    = (bool)ParseEnumValue(POSTDOWNLOADRENAME.data(), BoolCount, BoolNames, BoolValues);
	m_renameAfterUnpack     = (bool)ParseEnumValue(RENAMEAFTERUNPACK.data(), BoolCount, BoolNames, BoolValues);

	const char* OutputModeNames[] = { "loggable", "logable", "log", "colored", "color", "ncurses", "curses" };
	const int OutputModeValues[] = { omLoggable, omLoggable, omLoggable, omColored, omColored, omNCurses, omNCurses };
	const int OutputModeCount = 7;
	m_outputMode = (EOutputMode)ParseEnumValue(OUTPUTMODE.data(), OutputModeCount, OutputModeNames, OutputModeValues);

	const char* ParCheckNames[] = { "auto", "always", "force", "manual" };
	const int ParCheckValues[] = { pcAuto, pcAlways, pcForce, pcManual };
	const int ParCheckCount = (int)std::size(ParCheckNames);
	m_parCheck = (EParCheck)ParseEnumValue(PARCHECK.data(), ParCheckCount, ParCheckNames, ParCheckValues);

	const char* ParScanNames[] = { "limited", "extended", "full", "dupe" };
	const int ParScanValues[] = { psLimited, psExtended, psFull, psDupe };
	const int ParScanCount = 4;
	m_parScan = (EParScan)ParseEnumValue(PARSCAN.data(), ParScanCount, ParScanNames, ParScanValues);

	// "yes" is accepted for compatibility with the former boolean option and
	// is normalized to "article" by ParseEnumValue (first name with same value)
	const char* DupeArticleFallbackNames[] = { "no", "article", "stream", "live", "yes" };
	const int DupeArticleFallbackValues[] = { dafNone, dafArticle, dafStream, dafLive, dafArticle };
	const int DupeArticleFallbackCount = 5;
	m_dupeArticleFallback = (EDupeArticleFallback)ParseEnumValue(DUPEARTICLEFALLBACK.data(),
		DupeArticleFallbackCount, DupeArticleFallbackNames, DupeArticleFallbackValues);
	m_dupeStreamDecompress	= (bool)ParseEnumValue(DUPESTREAMDECOMPRESS.data(), BoolCount, BoolNames, BoolValues);
	int rawStreamTimeout = ParseIntValue(DUPESTREAMTIMEOUT.data(), 10);
	m_dupeSearch			= (bool)ParseEnumValue(DUPESEARCH.data(), BoolCount, BoolNames, BoolValues);
	m_dupeSearchDryRun		= (bool)ParseEnumValue(DUPESEARCHDRYRUN.data(), BoolCount, BoolNames, BoolValues);
	m_dupeSearchUrl			= GetOption(DUPESEARCHURL.data());
	m_dupeSearchApiKey		= GetOption(DUPESEARCHAPIKEY.data());
	int rawSearchDelay = ParseIntValue(DUPESEARCHDELAY.data(), 10);
	int rawSearchMaxDonors = ParseIntValue(DUPESEARCHMAXDONORS.data(), 10);
	int rawHealthPercent = ParseIntValue(DUPEHEALTHPERCENT.data(), 10);
	int rawHealthMin = ParseIntValue(DUPEHEALTHMIN.data(), 10);
	int rawHealthMax = ParseIntValue(DUPEHEALTHMAX.data(), 10);
	int rawBodyChecks = ParseIntValue(DUPEBODYCHECKS.data(), 10);
	int rawHealthBudget = ParseIntValue(DUPEHEALTHBUDGET.data(), 10);
	int rawMinAlive = ParseIntValue(DUPEMINALIVE.data(), 10);
	int rawFastDonors = ParseIntValue(DUPEFASTDONORS.data(), 10);
	nzbget_rs_rol_options_normalize_dupe_and_mem_options(
		memoryPressureInterval, rawStreamTimeout, rawSearchDelay,
		rawSearchMaxDonors, rawHealthPercent, rawHealthMin, rawHealthMax,
		rawBodyChecks, rawHealthBudget, rawMinAlive, rawFastDonors,
		&m_memoryPressureInterval, &m_dupeStreamTimeout, &m_dupeSearchDelay,
		&m_dupeSearchMaxDonors, &m_dupeHealthPercent, &m_dupeHealthMin,
		&m_dupeHealthMax, &m_dupeBodyChecks, &m_dupeHealthBudget,
		&m_dupeMinAlive, &m_dupeFastDonors);
	if (m_dupeSearch && Util::EmptyStr(m_dupeSearchUrl))
	{
		warn("Option <%s> is enabled but option <%s> is empty: no duplicates will be searched",
			DUPESEARCH.data(), DUPESEARCHURL.data());
		m_dupeSearch = false;
	}

	const char* PostStrategyNames[] = { "sequential", "balanced", "aggressive", "rocket" };
	const int PostStrategyValues[] = { ppSequential, ppBalanced, ppAggressive, ppRocket };
	const int PostStrategyCount = 4;
	m_postStrategy = (EPostStrategy)ParseEnumValue(POSTSTRATEGY.data(), PostStrategyCount, PostStrategyNames, PostStrategyValues);

	const char* FileNamingNames[] = { "auto", "article", "nzb" };
	const int FileNamingValues[] = { nfAuto, nfArticle, nfNzb };
	const int FileNamingCount = (int)std::size(FileNamingNames);
	m_fileNaming = (EFileNaming)ParseEnumValue(FILENAMING.data(), FileNamingCount, FileNamingNames, FileNamingValues);

	const char* HealthCheckNames[] = { "pause", "delete", "park", "none", "dupe" };
	const int HealthCheckValues[] = { hcPause, hcDelete, hcPark, hcNone, hcDupe };
	const int HealthCheckCount = 5;
	m_healthCheck = (EHealthCheck)ParseEnumValue(HEALTHCHECK.data(), HealthCheckCount, HealthCheckNames, HealthCheckValues);

	const char* TargetNames[] = { "screen", "log", "both", "none" };
	const int TargetValues[] = { mtScreen, mtLog, mtBoth, mtNone };
	const int TargetCount = 4;
	m_infoTarget = (EMessageTarget)ParseEnumValue(INFOTARGET.data(), TargetCount, TargetNames, TargetValues);
	m_warningTarget = (EMessageTarget)ParseEnumValue(WARNINGTARGET.data(), TargetCount, TargetNames, TargetValues);
	m_errorTarget = (EMessageTarget)ParseEnumValue(ERRORTARGET.data(), TargetCount, TargetNames, TargetValues);
	m_debugTarget = (EMessageTarget)ParseEnumValue(DEBUGTARGET.data(), TargetCount, TargetNames, TargetValues);
	m_detailTarget = (EMessageTarget)ParseEnumValue(DETAILTARGET.data(), TargetCount, TargetNames, TargetValues);

	const char* WriteLogNames[] = { "none", "append", "reset", "rotate" };
	const int WriteLogValues[] = { wlNone, wlAppend, wlReset, wlRotate };
	const int WriteLogCount = 4;
	m_writeLog = (EWriteLog)ParseEnumValue(WRITELOG.data(), WriteLogCount, WriteLogNames, WriteLogValues);
}

int Options::ParseEnumValue(const char* OptName, int argc, const char * argn[], const int argv[])
{
	OptEntry* optEntry = FindOption(OptName);
	if (!optEntry)
	{
		ConfigError("Undefined value for option \"%s\"", OptName);
		return argv[0];
	}

	int parsedVal = argv[0];
	int normIdx = -1;
	if (nzbget_rs_options_parse_enum_value(
		optEntry->GetValue(), optEntry->GetDefValue(), argc, argn, argv, &parsedVal, &normIdx))
	{
		if (normIdx >= 0 && normIdx < argc)
		{
			optEntry->SetValue(argn[normIdx]);
		}
		return parsedVal;
	}

	m_configLine = optEntry->GetLineNo();
	ConfigError("Invalid value for option \"%s\": \"%s\"", OptName, optEntry->GetValue());
	if (normIdx >= 0 && normIdx < argc)
	{
		optEntry->SetValue(argn[normIdx]);
	}
	return parsedVal;
}

int Options::ParseIntValue(const char* OptName, int base)
{
	OptEntry* optEntry = FindOption(OptName);
	if (!optEntry)
	{
		ConfigError("Undefined value for option \"%s\"", OptName);
		return 0;
	}

	int val = 0;
	if (!nzbget_rs_options_parse_int_value(
		optEntry->GetValue(), optEntry->GetDefValue(), base, &val))
	{
		m_configLine = optEntry->GetLineNo();
		ConfigError("Invalid value for option \"%s\": \"%s\"", OptName, optEntry->GetValue());
		optEntry->SetValue(optEntry->GetDefValue());
	}

	return val;
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

	// expand variables
	size_t dollarPos = 0;
	size_t varLen = 0;
	while (nzbget_rs_rco_options_find_var_reference(curvalue, &dollarPos, &varLen))
	{
		BString<100> variable;
		variable.Set((const char*)curvalue + dollarPos + 2, (int)varLen);
		const char* varvalue = GetOption(variable);
		if (varvalue)
		{
			curvalue.Replace((int)dollarPos, 2 + (int)varLen + 1, varvalue);
		}
		else
		{
			break;
		}
	}

	optEntry->SetValue(curvalue);
}

void Options::SetToolPathOption(fs::path& pathOpt, std::string_view value)
{
	if (value.empty()) return;

	fs::path directPath = fs::u8path(value);

	fs::error_code ec;
	if (fs::exists(directPath, ec) && fs::is_regular_file(directPath, ec))
	{
		pathOpt.swap(directPath);
		return;
	}

	ec.clear();

	// Clean the input: "'C:\\Program Files\\unrar' -x" -> "C:\\Program Files\\unrar"
	const auto args = Util::SplitCommandLine(value.data());
	const auto tool = !args.empty() ? *args[0] : std::string(value);
	directPath = fs::u8path(tool);

	if (directPath.has_parent_path())
	{
		if (fs::exists(directPath, ec) && fs::is_regular_file(directPath, ec))
		{
			pathOpt.swap(directPath);
			return;
		}
	}

	auto res = Util::ResolvePathFromEnv(tool);
	if (res)
	{
		pathOpt.swap(*res);
		return;
	}

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
		if (optEntry->GetLineNo() > 0)
		{
			m_configLine = optEntry->GetLineNo();
		}
		return optEntry->GetValue();
	}
	return nullptr;
}

namespace
{
BString<100> IndexedOptName(const char* prefix, int n, const char* field)
{
	NzbgetRsBuf buf = nzbget_rs_dqh_options_format_indexed_opt_name(prefix, n, field);
	BString<100> res;
	res.Set(buf.data ? buf.data : "", (int)buf.len);
	nzbget_rs_free(buf);
	return res;
}
}

void Options::InitServers()
{
	int n = 1;
	while (true)
	{
		BString<100> activeOpt = IndexedOptName("Server", n, "Active");
		const char* nactive = GetOption(activeOpt);
		bool active = true;
		if (nactive)
		{
			active = (bool)ParseEnumValue(activeOpt, BoolCount, BoolNames, BoolValues);
		}

		const char* nname = GetOption(IndexedOptName("Server", n, "Name"));
		const char* nlevel = GetOption(IndexedOptName("Server", n, "Level"));
		const char* ngroup = GetOption(IndexedOptName("Server", n, "Group"));
		const char* nhost = GetOption(IndexedOptName("Server", n, "Host"));
		const char* nport = GetOption(IndexedOptName("Server", n, "Port"));
		const char* nusername = GetOption(IndexedOptName("Server", n, "Username"));
		const char* npassword = GetOption(IndexedOptName("Server", n, "Password"));

		BString<100> joinGroupOpt = IndexedOptName("Server", n, "JoinGroup");
		const char* njoingroup = GetOption(joinGroupOpt);
		bool joinGroup = false;
		if (njoingroup)
		{
			joinGroup = (bool)ParseEnumValue(joinGroupOpt, BoolCount, BoolNames, BoolValues);
		}

		BString<100> optionalOpt = IndexedOptName("Server", n, "Optional");
		const char* noptional = GetOption(optionalOpt);
		bool optional = false;
		if (noptional)
		{
			optional = (bool)ParseEnumValue(optionalOpt, BoolCount, BoolNames, BoolValues);
		}

		BString<100> tlsOpt = IndexedOptName("Server", n, "Encryption");
		const char* ntls = GetOption(tlsOpt);
		bool tls = false;
		if (ntls)
		{
			tls = (bool)ParseEnumValue(tlsOpt, BoolCount, BoolNames, BoolValues);
#ifdef DISABLE_TLS
			if (tls)
			{
				ConfigError("Invalid value for option \"%s\": program was compiled without TLS/SSL-support",
					*tlsOpt);
				tls = false;
			}
#endif
			m_tls |= tls;
		}

		BString<100> certVerifOpt = IndexedOptName("Server", n, "CertVerification");
		const char* ncertveriflevel = GetOption(certVerifOpt);
		int certveriflevel = ECertVerifLevel::cvStrict;
		if (ncertveriflevel)
		{
			const char* CertVerifNames[] = { "none", "minimal", "strict" };
			const int CertVerifValues[] = { ECertVerifLevel::cvNone, ECertVerifLevel::cvMinimal, ECertVerifLevel::cvStrict };
			const int CertVerifCount = ECertVerifLevel::Count;
			certveriflevel = ParseEnumValue(certVerifOpt, CertVerifCount, CertVerifNames, CertVerifValues);
		}

		BString<100> ipVerOpt = IndexedOptName("Server", n, "IpVersion");
		const char* nipversion = GetOption(ipVerOpt);
		int ipversion = 0;
		if (nipversion)
		{
			const char* IpVersionNames[] = {"auto", "ipv4", "ipv6"};
			const int IpVersionValues[] = {0, 4, 6};
			const int IpVersionCount = 3;
			ipversion = ParseEnumValue(ipVerOpt, IpVersionCount, IpVersionNames, IpVersionValues);
		}

		const char* ncipher = GetOption(IndexedOptName("Server", n, "Cipher"));
		const char* nconnections = GetOption(IndexedOptName("Server", n, "Connections"));
		const char* nretention = GetOption(IndexedOptName("Server", n, "Retention"));

		bool definition = nactive || nname || nlevel || ngroup || nhost || nport || noptional ||
			nusername || npassword || nconnections || njoingroup || ntls || ncipher || nretention;
		int serverDefState = nzbget_rs_rol_options_check_server_def(
			definition ? 1 : 0, nhost != nullptr ? 1 : 0,
			nport != nullptr ? 1 : 0, nconnections != nullptr ? 1 : 0);
		if (serverDefState == 0)
		{
			break;
		}
		bool completed = serverDefState == 1;

		if (completed)
		{
			if (m_extender)
			{
				m_extender->AddNewsServer(n, active, nname,
					nhost,
					nport ? atoi(nport) : 119,
					ipversion,
					nusername, npassword,
					joinGroup, tls, ncipher,
					nconnections ? atoi(nconnections) : 1,
					nretention ? atoi(nretention) : 0,
					nlevel ? atoi(nlevel) : 0,
					ngroup ? atoi(ngroup) : 0,
					optional, certveriflevel);
			}
		}
		else
		{
			ConfigError("Server definition not complete for \"Server%i\"", n);
		}

		n++;
	}
}

void Options::InitCategories()
{
	int n = 1;
	while (true)
	{
		const char* nname = GetOption(IndexedOptName("Category", n, "Name"));
		BString<100> destDirOpt = IndexedOptName("Category", n, "DestDir");
		const char* ndestdir = GetOption(destDirOpt);

		BString<100> unpackOpt = IndexedOptName("Category", n, "Unpack");
		const char* nunpack = GetOption(unpackOpt);
		bool unpack = true;
		if (nunpack)
		{
			unpack = (bool)ParseEnumValue(unpackOpt, BoolCount, BoolNames, BoolValues);
		}

		const char* nextensions = GetOption(IndexedOptName("Category", n, "Extensions"));
		const char* naliases = GetOption(IndexedOptName("Category", n, "Aliases"));

		bool definition = nname || ndestdir || nunpack || nextensions || naliases;
		int catDefState = nzbget_rs_rol_options_check_category_or_feed_def(
			definition ? 1 : 0, (nname && strlen(nname) > 0) ? 1 : 0);
		if (catDefState == 0)
		{
			break;
		}
		bool completed = catDefState == 1;

		if (completed)
		{
			CString destDir;
			if (ndestdir && ndestdir[0] != '\0')
			{
				CheckDir(destDir, destDirOpt, m_destDir, false, false);
			}

			m_categories.emplace_back(nname, destDir, unpack, nextensions);
			Category& category = m_categories.back();

			// split Aliases into tokens and create items for each token
			if (naliases)
			{
				Tokenizer tok(naliases, ",;");
				while (const char* aliasName = tok.Next())
				{
					category.GetAliases()->push_back(aliasName);
				}
			}
		}
		else
		{
			ConfigError("Category definition not complete for \"Category%i\"", n);
		}

		n++;
	}
}

void Options::InitFeeds()
{
	int n = 1;
	while (true)
	{
		const char* nname = GetOption(IndexedOptName("Feed", n, "Name"));
		const char* nurl = GetOption(IndexedOptName("Feed", n, "URL"));
		const char* nfilter = GetOption(IndexedOptName("Feed", n, "Filter"));
		const char* ncategory = GetOption(IndexedOptName("Feed", n, "Category"));
		const char* nextensions = GetOption(IndexedOptName("Feed", n, "Extensions"));

		BString<100> backlogOpt = IndexedOptName("Feed", n, "Backlog");
		const char* nbacklog = GetOption(backlogOpt);
		bool backlog = true;
		if (nbacklog)
		{
			backlog = (bool)ParseEnumValue(backlogOpt, BoolCount, BoolNames, BoolValues);
		}

		BString<100> pauseNzbOpt = IndexedOptName("Feed", n, "PauseNzb");
		const char* npausenzb = GetOption(pauseNzbOpt);
		bool pauseNzb = false;
		if (npausenzb)
		{
			pauseNzb = (bool)ParseEnumValue(pauseNzbOpt, BoolCount, BoolNames, BoolValues);
		}

		const char* ncategorySource = GetOption(IndexedOptName("Feed", n, "CategorySource"));
		const auto categorySource = ParseCategorySource(ncategorySource);

		const char* ninterval = GetOption(IndexedOptName("Feed", n, "Interval"));
		const char* npriority = GetOption(IndexedOptName("Feed", n, "Priority"));

		BString<100> certVerifOpt = IndexedOptName("Feed", n, "CertVerification");
		const char* ncertveriflevel = GetOption(certVerifOpt);
		int certveriflevel = ECertVerifLevel::cvStrict;
		if (ncertveriflevel)
		{
			const char* CertVerifNames[] = { "none", "minimal", "strict" };
			const int CertVerifValues[] = { ECertVerifLevel::cvNone, ECertVerifLevel::cvMinimal, ECertVerifLevel::cvStrict };
			const int CertVerifCount = ECertVerifLevel::Count;
			certveriflevel = ParseEnumValue(certVerifOpt, CertVerifCount, CertVerifNames, CertVerifValues);
		}

		bool definition = nname || nurl || nfilter || ncategory || nbacklog || npausenzb ||
			ninterval || npriority || nextensions;
		int feedDefState = nzbget_rs_rol_options_check_category_or_feed_def(
			definition ? 1 : 0, nurl != nullptr ? 1 : 0);
		if (feedDefState == 0)
		{
			break;
		}
		bool completed = feedDefState == 1;

		if (completed)
		{
			if (m_extender)
			{
			m_extender->AddFeed(
				n,
				nname,
				nurl,
				ninterval ? atoi(ninterval) : 0,
				nfilter,
				backlog,
				pauseNzb,
				ncategory,
				categorySource,
				npriority ? atoi(npriority) : 0,
				nextensions,
				certveriflevel
			);
			}
		}
		else
		{
			ConfigError("Feed definition not complete for \"Feed%i\"", n);
		}

		n++;
	}
}

void Options::InitScheduler()
{
	for (int n = 1; ; n++)
	{
		const char* time = GetOption(IndexedOptName("Task", n, "Time"));
		const char* weekDays = GetOption(IndexedOptName("Task", n, "WeekDays"));
		BString<100> cmdOpt = IndexedOptName("Task", n, "Command");
		const char* command = GetOption(cmdOpt);
		const char* downloadRate = GetOption(IndexedOptName("Task", n, "DownloadRate"));
		const char* process = GetOption(IndexedOptName("Task", n, "Process"));
		const char* param = GetOption(IndexedOptName("Task", n, "Param"));

		int paramSource = 0, definition = 0, completed = 0;
		nzbget_rs_tpr_options_resolve_scheduler_task_state(
			time != nullptr ? 1 : 0, weekDays != nullptr ? 1 : 0,
			command != nullptr ? 1 : 0, downloadRate != nullptr ? 1 : 0,
			param != nullptr ? 1 : 0,
			Util::EmptyStr(param) ? 1 : 0,
			Util::EmptyStr(process) ? 1 : 0,
			Util::EmptyStr(downloadRate) ? 1 : 0,
			&paramSource, &definition, &completed);
		if (paramSource == 1)
		{
			param = process;
		}
		else if (paramSource == 2)
		{
			param = downloadRate;
		}

		if (!definition)
		{
			break;
		}

		if (!completed)
		{
			ConfigError("Task definition not complete for \"Task%i\"", n);
			continue;
		}

		const char* CommandNames[] = { "pausedownload", "pause", "unpausedownload", "resumedownload", "unpause", "resume",
			"pausepostprocess", "unpausepostprocess", "resumepostprocess", "pausepost", "unpausepost", "resumepost",
			"downloadrate", "setdownloadrate", "rate", "speed", "script", "process", "pausescan", "unpausescan", "resumescan",
			"activateserver", "activateservers", "deactivateserver", "deactivateservers", "fetchfeed", "fetchfeeds" };
		const int CommandValues[] = { scPauseDownload, scPauseDownload, scUnpauseDownload,
			scUnpauseDownload, scUnpauseDownload, scUnpauseDownload,
			scPausePostProcess, scUnpausePostProcess, scUnpausePostProcess,
			scPausePostProcess, scUnpausePostProcess, scUnpausePostProcess,
			scDownloadRate, scDownloadRate, scDownloadRate, scDownloadRate,
			scScript, scProcess, scPauseScan, scUnpauseScan, scUnpauseScan,
			scActivateServer, scActivateServer, scDeactivateServer,
			scDeactivateServer, scFetchFeed, scFetchFeed };
		const int CommandCount = 27;
		bool knownCommand = nzbget_rs_rco_options_lookup_scheduler_command(command) >= 0;
		ESchedulerCommand taskCommand = (ESchedulerCommand)ParseEnumValue(
			cmdOpt, CommandCount, CommandNames, CommandValues);
		if (!knownCommand)
		{
			continue;
		}

		bool splitCmdEmpty = (param && strlen(param) > 0 && taskCommand == scProcess)
			? Util::SplitCommandLine(param).empty() : false;
		int paramCheck = nzbget_rs_rco_options_validate_scheduler_param(
			(int)taskCommand, param, splitCmdEmpty ? 1 : 0);
		if (paramCheck == 1)
		{
			ConfigError("Invalid value for option \"Task%i.Param\"", n);
			continue;
		}
		if (paramCheck == 2)
		{
			ConfigError("Invalid value for option \"Task%i.Param\": \"%s\"", n, downloadRate);
			continue;
		}
		if (paramCheck == 3)
		{
			ConfigError("Task definition not complete for \"Task%i\". Option \"Task%i.Param\" is missing", n, n);
			continue;
		}

		CreateSchedulerTask(n, time, weekDays, taskCommand, param);
	}
}

void Options::CreateSchedulerTask(int id, const char* time, const char* weekDays,
	ESchedulerCommand command, const char* param)
{
	if (!id)
	{
		m_configLine = 0;
	}

	int weekDaysVal = 0;
	if (weekDays && !ParseWeekDays(weekDays, &weekDaysVal))
	{
		ConfigError("Invalid value for option \"Task%i.WeekDays\": \"%s\"", id, weekDays);
		return;
	}

	int hours, minutes;
	Tokenizer tok(time, ";,");
	while (const char* oneTime = tok.Next())
	{
		if (!ParseTime(oneTime, &hours, &minutes))
		{
			ConfigError("Invalid value for option \"Task%i.Time\": \"%s\"", id, oneTime);
			return;
		}

		if (m_extender)
		{
			if (hours == -2)
			{
				for (int everyHour = 0; everyHour < 24; everyHour++)
				{
					m_extender->AddTask(id, everyHour, minutes, weekDaysVal, command, param);
				}
			}
			else
			{
				m_extender->AddTask(id, hours, minutes, weekDaysVal, command, param);
			}
		}
	}
}

bool Options::ParseTime(const char* time, int* hours, int* minutes)
{
	// rust/src/options.rs
	return nzbget_rs_parse_time(time, hours, minutes) != 0;
}

bool Options::ParseWeekDays(const char* weekDays, int* weekDaysBits)
{
	// rust/src/options.rs
	return nzbget_rs_parse_week_days(weekDays, weekDaysBits) != 0;
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
	size_t nameLen = 0, valOffset = 0;
	if (!nzbget_rs_tpr_options_split_option_string(option, &nameLen, &valOffset))
	{
		return false;
	}

	optName.Set(option, (int)nameLen);
	optValue.Set(option + valOffset);

	ConvertOldOption(optName, optValue);

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
			if (optvalue && strlen(optvalue) > 0)
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

void Options::ConvertOldOption(CString& option, CString& value)
{
	// rust/src/options.rs
	NzbgetRsBuf newOption, newValue;
	nzbget_rs_convert_old_option(option, value, &newOption, &newValue);
	// Decide before changing option: callers may pass the same CString twice.
	// An unchanged null value must also remain null.
	bool valueChanged = value ? nzbget_rs_dqh_bytes_equal(value, newValue.data) == 0 : newValue.len != 0;
	if (!option || nzbget_rs_dqh_bytes_equal(option, newOption.data) == 0)
	{
		option = newOption.data;
	}
	if (valueChanged)
	{
		value = newValue.data;
	}
	nzbget_rs_free(newOption);
	nzbget_rs_free(newValue);
}

void Options::CheckOptions()
{
#ifdef DISABLE_PARCHECK
	if (m_parCheck != pcManual)
	{
		LocateOptionSrcPos(PARCHECK.data());
		ConfigError("Invalid value for option \"%s\": program was compiled without parcheck-support", PARCHECK.data());
	}
	if (m_parRename)
	{
		LocateOptionSrcPos(PARRENAME.data());
		ConfigError("Invalid value for option \"%s\": program was compiled without parcheck-support", PARRENAME.data());
	}
	if (m_directRename)
	{
		LocateOptionSrcPos(DIRECTRENAME.data());
		ConfigError("Invalid value for option \"%s\": program was compiled without parcheck-support", DIRECTRENAME.data());
	}
#endif

#ifdef DISABLE_CURSES
	if (m_outputMode == omNCurses)
	{
		LocateOptionSrcPos(OUTPUTMODE.data());
		ConfigError("Invalid value for option \"%s\": program was compiled without curses-support", OUTPUTMODE.data());
	}
#endif

#ifdef DISABLE_TLS
	if (m_secureControl)
	{
		LocateOptionSrcPos(SECURECONTROL.data());
		ConfigError("Invalid value for option \"%s\": program was compiled without TLS/SSL-support", SECURECONTROL.data());
	}

	if (m_certCheck)
	{
		LocateOptionSrcPos(CERTCHECK.data());
		ConfigError("Invalid value for option \"%s\": program was compiled without TLS/SSL-support", CERTCHECK.data());
	}
#endif

#ifndef DISABLE_TLS
#ifndef HAVE_X509_CHECK_HOST
	if (m_certCheck)
	{
		LocateOptionSrcPos(CERTCHECK.data());
		ConfigWarn("TLS certificate verification (option \"%s\") is limited because the program "
			"was compiled with older OpenSSL version not supporting hostname validation (at least OpenSSL 1.0.2d is required)", CERTCHECK.data());
	}
#endif
#endif

	if (m_certCheck && m_certStore.Empty())
	{
		LocateOptionSrcPos(CERTCHECK.data());
		ConfigError("Option \"%s\" requires proper configuration of option \"%s\"", CERTCHECK.data(), CERTSTORE.data());
		m_certCheck = false;
	}

	int newDirectWrite = m_directWrite ? 1 : 0;
	int newDirectRename = m_directRename ? 1 : 0;
	nzbget_rs_rol_options_reconcile_write_flags(
		m_rawArticle ? 1 : 0, m_skipWrite ? 1 : 0,
		newDirectWrite, newDirectRename,
		&newDirectWrite, &newDirectRename);
	m_directWrite = newDirectWrite != 0;
	m_directRename = newDirectRename != 0;

	// if option "ConfigTemplate" is not set, use "WebDir" as default location for template
	// (for compatibility with versions 9 and 10).
	if (m_configTemplate.Empty() && !m_noDiskAccess && !m_webDir.Empty())
	{
		NzbgetRsBuf ctBuf = nzbget_rs_tpr_options_format_default_conf_path(*m_webDir, PATH_SEPARATOR);
		m_configTemplate.Set(ctBuf.data ? ctBuf.data : "", (int)ctBuf.len);
		nzbget_rs_free(ctBuf);
		if (!FileSystem::FileExists(m_configTemplate))
		{
			m_configTemplate = "";
		}
	}

	int origArticleCache = m_articleCache;
	int origParBuffer = m_parBuffer;
	int memErr = nzbget_rs_fuq_options_clamp_32bit_memory_limits(
		sizeof(void*) == 4 ? 1 : 0, m_articleCache, m_parBuffer,
		&m_articleCache, &m_parBuffer);
	if (memErr == 1)
	{
		ConfigError("Invalid value for option \"ArticleCache\": %i. Changed to 1900", origArticleCache);
	}
	else if (memErr == 2)
	{
		ConfigError("Invalid value for option \"ParBuffer\": %i. Changed to 1900", origParBuffer);
	}
	else if (memErr == 3)
	{
		if (origArticleCache > 1900)
		{
			ConfigError("Invalid value for option \"ArticleCache\": %i. Changed to 1900", origArticleCache);
		}
		else if (origParBuffer > 1900)
		{
			ConfigError("Invalid value for option \"ParBuffer\": %i. Changed to 1900", origParBuffer);
		}
		ConfigError("Options \"ArticleCache\" and \"ParBuffer\" in total cannot use more than 1900MB of memory in 32-Bit mode. Changed to 1500 and 400");
	}

	if (!m_unpackPassFile.Empty() && !FileSystem::FileExists(m_unpackPassFile))
	{
		ConfigError("Invalid value for option \"UnpackPassFile\": %s. File not found", *m_unpackPassFile);
	}

	// Configuration Conflict Reconciliation (R7)
	// Rule 7.1: ServerX.Encryption=no with ServerX.Cipher configured
	for (int n = 1; ; n++)
	{
		BString<100> encOpt = IndexedOptName("Server", n, "Encryption");
		BString<100> cipherOpt = IndexedOptName("Server", n, "Cipher");
		const char* ntls = GetOption(encOpt);
		const char* ncipher = GetOption(cipherOpt);
		if (!ntls && !ncipher && !GetOption(IndexedOptName("Server", n, "Host")) && !GetOption(IndexedOptName("Server", n, "Name")))
		{
			break;
		}

		bool tls = false;
		if (ntls)
		{
			tls = (bool)ParseEnumValue(encOpt, BoolCount, BoolNames, BoolValues);
		}

		if (nzbget_rs_roc_options_server_cipher_without_tls(tls ? 1 : 0, ncipher))
		{
			LocateOptionSrcPos(cipherOpt);
			ConfigWarn("Option %s ('%s') is ignored because %s is disabled for Server%i.",
				*cipherOpt, ncipher, *encOpt, n);
			SetOption(cipherOpt, "");
		}
	}

	int disableDupeSearch = 0;
	int enableFlushQueue = 0;
	int enforceUnpackMutex = 0;
	nzbget_rs_roc_options_reconcile_conflict_rules(
		m_dupeSearch ? 1 : 0, (int)m_healthCheck, (int)hcPark,
		m_flushQueue ? 1 : 0, m_directWrite ? 1 : 0,
		m_unpackPauseQueue ? 1 : 0, m_directUnpack ? 1 : 0,
		&disableDupeSearch, &enableFlushQueue, &enforceUnpackMutex);

	if (disableDupeSearch)
	{
		LocateOptionSrcPos(DUPESEARCH.data());
		ConfigWarn("Option 'DupeSearch' automatically disabled: 'HealthCheck' is set to 'park', which suspends failed downloads without triggering duplicate donor resolution.");
		m_dupeSearch = false;
		SetOption(DUPESEARCH.data(), "no");
	}

	if (enableFlushQueue)
	{
		LocateOptionSrcPos(FLUSHQUEUE.data());
		ConfigWarn("Option 'FlushQueue=no' is incompatible with 'DirectWrite=yes'. Automatically enabling 'FlushQueue=yes' to ensure queue crash-recovery and prevent duplicate file corruption.");
		m_flushQueue = true;
		SetOption(FLUSHQUEUE.data(), "yes");
	}

	if (enforceUnpackMutex)
	{
		LocateOptionSrcPos(DIRECTUNPACK.data());
		ConfigWarn("Option 'DirectUnpack=yes' with 'UnpackPauseQueue=no' can cause severe disk thrashing. Unpack concurrency will be serialized across jobs to protect I/O throughput.");
		m_unpackMutexEnforced = true;
		SetOption("_UnpackMutexEnforced", "yes");
	}
}

void Options::ConvertOldOptions(OptEntries* optEntries)
{
	MergeOldScriptOption(optEntries, SCANSCRIPT.data(), true);
	MergeOldScriptOption(optEntries, QUEUESCRIPT.data(), true);
	MergeOldScriptOption(optEntries, FEEDSCRIPT.data(), false);
}

void Options::MergeOldScriptOption(OptEntries* optEntries, const char* optname, bool mergeCategories)
{
	OptEntry* optEntry = optEntries->FindOption(optname);
	if (!optEntry || Util::EmptyStr(optEntry->GetValue()))
	{
		return;
	}

	OptEntry* extensionsOpt = optEntries->FindOption(EXTENSIONS.data());
	if (!extensionsOpt)
	{
		optEntries->emplace_back(EXTENSIONS.data(), "");
		extensionsOpt = optEntries->FindOption(EXTENSIONS.data());
	}

	const char* scriptList = optEntry->GetValue();

	Tokenizer tok(scriptList, ",;");
	while (const char* scriptName = tok.Next())
	{
		// merge into global "Extensions"
		if (!HasScript(extensionsOpt->m_value, scriptName))
		{
			if (!extensionsOpt->m_value.Empty())
			{
				extensionsOpt->m_value.Append(",");
			}
			extensionsOpt->m_value.Append(scriptName);
		}

		// merge into categories' "Extensions" (if not empty)
		if (mergeCategories)
		{
			for (OptEntry& opt : optEntries)
			{
				if (nzbget_rs_options_is_category_extensions_opt(opt.GetName()) != 0)
				{
					if (!opt.m_value.Empty() && !HasScript(opt.m_value, scriptName))
					{
						opt.m_value.Append(",");
						opt.m_value.Append(scriptName);
					}
				}
			}
		}
	}
}

bool Options::HasScript(const char* scriptList, const char* scriptName)
{
	// rust/src/options.rs
	return nzbget_rs_has_script(scriptList, scriptName) != 0;
}

FeedInfo::CategorySource Options::ParseCategorySource(const char* value)
{
	// rust/src/options.rs
	return static_cast<FeedInfo::CategorySource>(nzbget_rs_parse_category_source(value));
}
