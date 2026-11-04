/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
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


#include "nzbget.h"
#include "DownloadInfo.h"
#include "DiskState.h"
#include "Options.h"
#include "Util.h"
#include "FileSystem.h"
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

std::atomic<int> FileInfo::m_idGen{0};
int FileInfo::m_idMax = 0;
std::atomic<int> NzbInfo::m_idGen{0};
int NzbInfo::m_idMax = 0;
DownloadQueue* DownloadQueue::g_DownloadQueue = nullptr;
std::atomic<bool> DownloadQueue::g_Loaded{false};

void NzbParameterList::SetParameter(const char* name, const char* value)
{
	bool emptyVal = Util::EmptyStr(value);

	iterator pos = std::find_if(begin(), end(),
		[name](NzbParameter& parameter)
		{
			return !strcasecmp(parameter.GetName(), name);
		});

	if (emptyVal && pos != end())
	{
		erase(pos);
	}
	else if (pos != end())
	{
		pos->SetValue(value);
	}
	else if (!emptyVal)
	{
		emplace_back(name, value);
	}
}

NzbParameter* NzbParameterList::Find(const char* name)
{
	for (NzbParameter& parameter : this)
	{
		if (!strcasecmp(parameter.GetName(), name))
		{
			return &parameter;
		}
	}

	return nullptr;
}

void NzbParameterList::CopyFrom(NzbParameterList* sourceParameters)
{
	for (NzbParameter& parameter : sourceParameters)
	{
		SetParameter(parameter.GetName(), parameter.GetValue());
	}
}


ScriptStatus::EStatus ScriptStatusList::CalcTotalStatus()
{
#ifdef NZBGET_USE_RUST
	std::vector<int32_t> statuses;
	statuses.reserve(size());
	for (ScriptStatus& scriptStatus : this)
	{
		statuses.push_back((int32_t)scriptStatus.GetStatus());
	}
	return (ScriptStatus::EStatus)nzbget_rs_qhp_script_calc_total_status(
		statuses.empty() ? nullptr : statuses.data(), statuses.size());
#else
	ScriptStatus::EStatus status = ScriptStatus::srNone;

	for (ScriptStatus& scriptStatus : this)
	{
		// Failure-Status overrides Success-Status
		if ((scriptStatus.GetStatus() == ScriptStatus::srSuccess && status == ScriptStatus::srNone) ||
			(scriptStatus.GetStatus() == ScriptStatus::srFailure))
		{
			status = scriptStatus.GetStatus();
		}
	}

	return status;
#endif
}


void ServerStatList::StatOp(int serverId, int successArticles, int failedArticles, EStatOperation statOperation)
{
	ServerStat* serverStat = nullptr;
	for (ServerStat& serverStat1 : this)
	{
		if (serverStat1.GetServerId() == serverId)
		{
			serverStat = &serverStat1;
			break;
		}
	}

	if (!serverStat)
	{
		emplace_back(serverId);
		serverStat = &back();
	}

#ifdef NZBGET_USE_RUST
	int newSuccess = serverStat->GetSuccessArticles();
	int newFailed = serverStat->GetFailedArticles();
	nzbget_rs_qci_server_stat_apply_op(
		newSuccess,
		newFailed,
		successArticles,
		failedArticles,
		(int)statOperation,
		(int)soSet,
		(int)soAdd,
		(int)soSubtract,
		&newSuccess,
		&newFailed);
	serverStat->SetSuccessArticles(newSuccess);
	serverStat->SetFailedArticles(newFailed);
#else
	switch (statOperation)
	{
		case soSet:
			serverStat->SetSuccessArticles(successArticles);
			serverStat->SetFailedArticles(failedArticles);
			break;

		case soAdd:
			serverStat->SetSuccessArticles(serverStat->GetSuccessArticles() + successArticles);
			serverStat->SetFailedArticles(serverStat->GetFailedArticles() + failedArticles);
			break;

		case soSubtract:
			serverStat->SetSuccessArticles(serverStat->GetSuccessArticles() - successArticles);
			serverStat->SetFailedArticles(serverStat->GetFailedArticles() - failedArticles);
			break;
	}
#endif
}

void ServerStatList::ListOp(ServerStatList* serverStats, EStatOperation statOperation)
{
	for (ServerStat& serverStat : serverStats)
	{
		StatOp(serverStat.GetServerId(), serverStat.GetSuccessArticles(), serverStat.GetFailedArticles(), statOperation);
	}
}


void NzbInfo::SetId(int id)
{
	m_id = id;
#ifdef NZBGET_USE_RUST
	m_idMax = nzbget_rs_dps_dl_info_update_max_id(m_id, m_idMax);
#else
	if (m_idMax < m_id)
	{
		m_idMax = m_id;
	}
#endif
}

void NzbInfo::ResetGenId(bool max)
{
#ifdef NZBGET_USE_RUST
	int idGen = 0;
	nzbget_rs_dps_dl_info_reset_gen_id(max ? 1 : 0, m_idMax, &idGen, &m_idMax);
	m_idGen = idGen;
#else
	if (max)
	{
		m_idGen = m_idMax;
	}
	else
	{
		m_idGen = 0;
		m_idMax = 0;
	}
#endif
}

int NzbInfo::GenerateId()
{
	return ++m_idGen;
}

void NzbInfo::SetUrl(const char* url)
{
	m_url = url;

	if (!m_name)
	{
		CString nzbNicename = MakeNiceUrlName(url, m_filename);
		SetName(nzbNicename);
	}
}

void NzbInfo::SetFilename(const char* filename)
{
	bool hadFilename = !Util::EmptyStr(m_filename);
	m_filename = filename;

	if ((!m_name || !hadFilename) && !Util::EmptyStr(filename))
	{
		CString nzbNicename = MakeNiceNzbName(m_filename, true);
		if (nzbNicename.Empty())
		{
			nzbNicename.Format("nzb-%i", GetId());
		}
		SetName(nzbNicename);
	}
}

CString NzbInfo::MakeNiceNzbName(const char * nzbFilename, bool removeExt)
{
	// only a full path drops its folder: other "/" belong to the name (indexers
	// post "[hash] [16/22] - Title", which became "22] - Title"); the sanitizing
	// below makes them safe
#ifdef NZBGET_USE_RUST
	bool absolutePath = nzbget_rs_pup_scanner_is_absolute_nzb_name(nzbFilename) != 0;
#else
	bool absolutePath = nzbFilename[0] == '/' || nzbFilename[0] == '\\' ||
		(isalpha((unsigned char)nzbFilename[0]) && nzbFilename[1] == ':' &&
		 (nzbFilename[2] == '\\' || nzbFilename[2] == '/'));
#endif
	std::string nicename = absolutePath ? FileSystem::BaseFileName(nzbFilename) : nzbFilename;
#ifdef NZBGET_USE_RUST
	size_t strippedLen = nzbget_rs_pup_nzb_info_strip_nzb_ext_len(
		nicename.c_str(), removeExt ? 1 : 0);
	nicename.resize(strippedLen);
#else
	if (removeExt)
	{
		// wipe out ".nzb"
		auto dotpos = nicename.rfind('.');
		if (dotpos != std::string::npos && !strcasecmp(nicename.c_str() + dotpos, ".nzb"))
		{
			nicename.erase(dotpos);
		}
	}
#endif
	std::string validname = FileSystem::SanitizePathSegment(nicename);
	if (validname.empty())
	{
		validname = "nzb";
	}
	return validname.c_str();
}

CString NzbInfo::MakeNiceUrlName(const char* urlStr, const char* nzbFilename)
{
	CString urlNicename;
	URL url(urlStr);

	if (!Util::EmptyStr(nzbFilename))
	{
		CString nzbNicename = MakeNiceNzbName(nzbFilename, true);
		urlNicename.Format("%s @ %s", *nzbNicename, url.GetHost());
	}
	else if (url.IsValid())
	{
		urlNicename.Format("%s%s", url.GetHost(), url.GetResource());
	}
	else
	{
		urlNicename = urlStr;
	}

	return urlNicename;
}

// The name as a directory name: a file system takes 255 bytes for a name, and
// a longer one couldn't be created - the download's files failed to write and
// counted as failed articles. Cut on a whole UTF-8 character, leaving room for
// "reserve" more bytes; a name starting with "." gets "_" in front (hidden).
static std::string DirNameOf(const char* name, size_t reserve)
{
#ifdef NZBGET_USE_RUST
	NzbgetRsBuf buf = nzbget_rs_qhp_dir_name_of(name, reserve);
	std::string dirName(buf.data ? buf.data : "nzb");
	nzbget_rs_free(buf);
	return dirName;
#else
	std::string dirName = Util::EmptyStr(name) ? "nzb" : name;
	if (dirName[0] == '.')
	{
		dirName.insert(0, "_");
	}
	size_t maxLen = 255 - reserve;
	if (dirName.size() > maxLen)
	{
		size_t cut = maxLen;
		while (cut > 0 && ((unsigned char)dirName[cut] & 0xC0) == 0x80)
		{
			cut--;
		}
		dirName.resize(cut);
		// no trailing dot or space (not allowed on Windows, see MakeValidFilename)
		while (!dirName.empty() && (dirName.back() == '.' || dirName.back() == ' '))
		{
			dirName.pop_back();
		}
	}
	return dirName;
#endif
}

void NzbInfo::BuildDestDirName()
{
	if (Util::EmptyStr(g_Options->GetInterDir()))
	{
		m_destDir = BuildFinalDirName();
	}
	else
	{
		BString<100> idSuffix(".#%i", GetId());
		m_destDir.Format("%s%c%s%s", g_Options->GetInterDir(), PATH_SEPARATOR,
			DirNameOf(GetName(), strlen(idSuffix)).c_str(), *idSuffix);
	}
}

CString NzbInfo::BuildFinalDirName()
{
	CString finalDir = g_Options->GetDestDir();
	bool useCategory = !m_category.Empty();

	if (useCategory)
	{
		Options::Category* category = g_Options->FindCategory(m_category, false);
		if (category && !Util::EmptyStr(category->GetDestDir()))
		{
			finalDir = category->GetDestDir();
			useCategory = false;
		}
	}

	if (g_Options->GetAppendCategoryDir() && useCategory)
	{
		std::string categoryDir = FileSystem::SanitizeRelativePath(*m_category);
		if (!categoryDir.empty())
		{
			// we can't format with "finalDir.Format" because one of the parameter is "finalDir" itself.
			finalDir = CString::FormatStr("%s%c%s", *finalDir, PATH_SEPARATOR, categoryDir.c_str());
		}
	}

	if (Util::EmptyStr(GetName()))
	{
		finalDir.AppendFmt("%c%s-%i", PATH_SEPARATOR, "nzb", GetId());
	}
	else
	{
		finalDir.AppendFmt("%c%s", PATH_SEPARATOR, DirNameOf(GetName(), 0).c_str());
	}

	return finalDir;
}

int NzbInfo::CalcHealth()
{
#ifdef NZBGET_USE_RUST
	return nzbget_rs_qhp_calc_health(m_size, m_parSize, m_currentFailedSize, m_parCurrentFailedSize);
#else
	if (m_currentFailedSize == 0 || m_size == m_parSize)
	{
		return 1000;
	}

	int health = (int)((m_size - m_parSize -
		(m_currentFailedSize - m_parCurrentFailedSize)) * 1000 / (m_size - m_parSize));

	if (health == 1000 && m_currentFailedSize - m_parCurrentFailedSize > 0)
	{
		health = 999;
	}

	return health;
#endif
}

bool NzbInfo::RenameCompletedFile(const char* oldName, const char* newAbsoluteOrRelativePath)
{
	if (Util::EmptyStr(oldName) || Util::EmptyStr(newAbsoluteOrRelativePath))
	{
		return false;
	}

	for (CompletedFile& cf : m_completedFiles)
	{
		if (cf.SameFilename(oldName))
		{
			if (Util::EmptyStr(cf.GetOrigname()))
			{
				cf.SetOrigname(cf.GetFilename());
			}

			cf.SetFilename(newAbsoluteOrRelativePath);
			return true;
		}
	}
	return false;
}

int NzbInfo::CalcCriticalHealth(bool allowEstimation)
{
#ifdef NZBGET_USE_RUST
	return nzbget_rs_qhp_calc_critical_health(
		m_size, m_parSize, m_parCurrentFailedSize, allowEstimation ? 1 : 0);
#else
	if (m_size == 0)
	{
		return 1000;
	}

	if (m_size == m_parSize)
	{
		return 0;
	}

	int64 goodParSize = m_parSize - m_parCurrentFailedSize;
	int criticalHealth = (int)((m_size - goodParSize*2) * 1000 / (m_size - goodParSize));

	if (goodParSize*2 > m_size)
	{
		criticalHealth = 0;
	}
	else if (criticalHealth == 1000 && m_parSize > 0)
	{
		criticalHealth = 999;
	}

	if (criticalHealth == 1000 && allowEstimation)
	{
		// using empirical critical health 85%, to avoid false alarms for downloads with renamed par-files
		criticalHealth = 850;
	}

	return criticalHealth;
#endif
}

void NzbInfo::UpdateMinMaxTime()
{
#ifdef NZBGET_USE_RUST
	std::vector<int64_t> times;
	times.reserve(m_fileList.size());
	for (FileInfo* fileInfo : &m_fileList)
	{
		times.push_back((int64_t)fileInfo->GetTime());
	}
	int64_t minTime = 0, maxTime = 0;
	nzbget_rs_qhp_update_min_max_time(
		times.empty() ? nullptr : times.data(), times.size(), &minTime, &maxTime);
	m_minTime = (time_t)minTime;
	m_maxTime = (time_t)maxTime;
#else
	m_minTime = 0;
	m_maxTime = 0;

	bool first = true;
	for (FileInfo* fileInfo : &m_fileList)
	{
		if (first)
		{
			m_minTime = fileInfo->GetTime();
			m_maxTime = fileInfo->GetTime();
			first = false;
		}
		if (fileInfo->GetTime() > 0)
		{
			if (fileInfo->GetTime() < m_minTime)
			{
				m_minTime = fileInfo->GetTime();
			}
			if (fileInfo->GetTime() > m_maxTime)
			{
				m_maxTime = fileInfo->GetTime();
			}
		}
	}
#endif
}

void NzbInfo::AddMessage(Message::EKind kind, const char * text, bool print)
{
	if (print)
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

	Guard guard(m_logMutex);

	m_messages.emplace_back(++m_idMessageGen, kind, Util::CurrentTime(), text);

	if (g_Options->GetServerMode() && g_Options->GetNzbLog())
	{
		g_DiskState->AppendNzbMessage(m_id, kind, text);
		m_messageCount.fetch_add(1, std::memory_order_relaxed);
	}

#ifdef NZBGET_USE_RUST
	while (nzbget_rs_dds_dl_info_should_trim_message_buffer(
		m_messages.size(), g_Options->GetLogBuffer()) != 0)
#else
	while (m_messages.size() > (uint32)g_Options->GetLogBuffer())
#endif
	{
		m_messages.pop_front();
	}

	m_cachedMessageCount = m_messages.size();
}

void NzbInfo::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, format);
	vsnprintf(tmp2, 1024, format, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	AddMessage(kind, tmp2);
}

void NzbInfo::ClearMessages()
{
	Guard guard(m_logMutex);
	m_messages.clear();
	m_cachedMessageCount = 0;
}

void NzbInfo::MoveFileList(NzbInfo* srcNzbInfo)
{
	m_fileList = std::move(*srcNzbInfo->GetFileList());
	for (FileInfo* fileInfo : &m_fileList)
	{
		fileInfo->SetNzbInfo(this);
	}

	SetFullContentHash(srcNzbInfo->GetFullContentHash());
	SetFilteredContentHash(srcNzbInfo->GetFilteredContentHash());

	SetFileCount(srcNzbInfo->GetFileCount());
	SetPausedFileCount(srcNzbInfo->GetPausedFileCount());
	SetRemainingParCount(srcNzbInfo->GetRemainingParCount());

	SetSize(srcNzbInfo->GetSize());
	SetRemainingSize(srcNzbInfo->GetRemainingSize());
	SetPausedSize(srcNzbInfo->GetPausedSize());
	SetSuccessSize(srcNzbInfo->GetSuccessSize());
	SetCurrentSuccessSize(srcNzbInfo->GetCurrentSuccessSize());
	SetFailedSize(srcNzbInfo->GetFailedSize());
	SetCurrentFailedSize(srcNzbInfo->GetCurrentFailedSize());

	SetParSize(srcNzbInfo->GetParSize());
	SetParSuccessSize(srcNzbInfo->GetParSuccessSize());
	SetParCurrentSuccessSize(srcNzbInfo->GetParCurrentSuccessSize());
	SetParFailedSize(srcNzbInfo->GetParFailedSize());
	SetParCurrentFailedSize(srcNzbInfo->GetParCurrentFailedSize());

	SetTotalArticles(srcNzbInfo->GetTotalArticles());
	SetSuccessArticles(srcNzbInfo->GetSuccessArticles());
	SetFailedArticles(srcNzbInfo->GetFailedArticles());
	SetCurrentSuccessArticles(srcNzbInfo->GetSuccessArticles());
	SetCurrentFailedArticles(srcNzbInfo->GetFailedArticles());

	SetMinTime(srcNzbInfo->GetMinTime());
	SetMaxTime(srcNzbInfo->GetMaxTime());
}

void NzbInfo::EnterPostProcess()
{
	m_postInfo = std::make_unique<PostInfo>();
	m_postInfo->SetNzbInfo(this);
}

void NzbInfo::LeavePostProcess()
{
	m_postInfo.reset();
	m_unpackTried = false;
	m_lastUnpackStatus = 0;
	m_extractedArchives.clear();
	ClearMessages();
}

void NzbInfo::SetActiveDownloads(int activeDownloads)
{
#ifdef NZBGET_USE_RUST
	int outSec = m_downloadSec;
	int64 outStartTime = (int64)m_downloadStartTime;
	int outStartSec = m_downloadStartSec;
	int outChanged = 0;
	nzbget_rs_pup_nzb_info_calc_active_downloads_sec(
		m_activeDownloads, activeDownloads, m_kind == NzbInfo::nkNzb ? 1 : 0,
		(int64)Util::CurrentTime(), m_downloadSec, (int64)m_downloadStartTime,
		m_downloadStartSec, &outSec, &outStartTime, &outStartSec, &outChanged);
	m_downloadSec = outSec;
	m_downloadStartTime = (time_t)outStartTime;
	m_downloadStartSec = outStartSec;
	if (outChanged)
	{
		m_changed = true;
	}
#else
	if (((m_activeDownloads == 0 && activeDownloads > 0) ||
		 (m_activeDownloads > 0 && activeDownloads == 0)) &&
		m_kind == NzbInfo::nkNzb)
	{
		if (activeDownloads > 0)
		{
			m_downloadStartTime = Util::CurrentTime();
			m_downloadStartSec = m_downloadSec;
		}
		else
		{
			m_downloadSec = m_downloadStartSec + (int)(Util::CurrentTime() - m_downloadStartTime);
			m_downloadStartTime = 0;
			m_changed = true;
		}
	}
	else if (activeDownloads > 0)
	{
		m_downloadSec = m_downloadStartSec + (int)(Util::CurrentTime() - m_downloadStartTime);
		m_changed = true;
	}
#endif
	m_activeDownloads = activeDownloads;
}

bool NzbInfo::IsDupeSuccess()
{
#ifdef NZBGET_USE_RUST
	return nzbget_rs_qhp_is_dupe_success(
		(int)m_markStatus, (int)m_deleteStatus, (int)m_parStatus,
		(int)m_unpackStatus, (int)m_urlStatus, CalcHealth(),
		CalcCriticalHealth(true), CalcCriticalHealth(false),
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0) != 0;
#else
	bool failure =
		m_markStatus != NzbInfo::ksSuccess &&
		m_markStatus != NzbInfo::ksGood &&
		(m_deleteStatus != NzbInfo::dsNone ||
		m_markStatus == NzbInfo::ksBad ||
		m_parStatus == NzbInfo::psFailure ||
		m_unpackStatus == NzbInfo::usFailure ||
		m_unpackStatus == NzbInfo::usPassword ||
		m_urlStatus == NzbInfo::lsFailed ||
		m_urlStatus == NzbInfo::lsScanSkipped ||
		m_urlStatus == NzbInfo::lsScanFailed ||
		(m_parStatus == NzbInfo::psSkipped &&
		 m_unpackStatus == NzbInfo::usSkipped &&
		 CalcHealth() < CalcCriticalHealth(true)) ||
		// with HealthCheck=dupe as the status reads it (FAILURE/HEALTH): a download
		// with neither par2 nor archives that lost articles failed too, and its
		// backups are tried (B79: before, it counted as a success)
		(g_Options->GetHealthCheck() == Options::hcDupe &&
		 (m_parStatus == NzbInfo::psNone || m_parStatus == NzbInfo::psSkipped) &&
		 (m_unpackStatus == NzbInfo::usNone || m_unpackStatus == NzbInfo::usSkipped) &&
		 CalcHealth() < CalcCriticalHealth(false)));
	return !failure;
#endif
}

const char* NzbInfo::MakeTextStatus(bool ignoreScriptStatus)
{
#ifdef NZBGET_USE_RUST
	int health = m_kind == NzbInfo::nkNzb ? CalcHealth() : 0;
	int criticalHealth = m_kind == NzbInfo::nkNzb ? CalcCriticalHealth(false) : 0;
	ScriptStatus::EStatus scriptStatus = (m_kind == NzbInfo::nkNzb && !ignoreScriptStatus)
		? m_scriptStatuses.CalcTotalStatus()
		: ScriptStatus::srSuccess;
	return nzbget_rs_qhp_make_text_status(
		(int)m_kind, (int)m_markStatus, (int)m_deleteStatus, (int)m_parStatus,
		(int)m_unpackStatus, (int)m_moveStatus, (int)m_urlStatus,
		(int)scriptStatus, health, criticalHealth);
#else
	const char* status = "FAILURE/INTERNAL_ERROR";

	if (m_kind == NzbInfo::nkNzb)
	{
		int health = CalcHealth();
		int criticalHealth = CalcCriticalHealth(false);
		ScriptStatus::EStatus scriptStatus = ignoreScriptStatus ? ScriptStatus::srSuccess : m_scriptStatuses.CalcTotalStatus();

		if (m_markStatus == NzbInfo::ksBad)
		{
			status = "FAILURE/BAD";
		}
		else if (m_markStatus == NzbInfo::ksGood)
		{
			status = "SUCCESS/GOOD";
		}
		else if (m_markStatus == NzbInfo::ksSuccess)
		{
			status = "SUCCESS/MARK";
		}
		else if (m_deleteStatus == NzbInfo::dsHealth)
		{
			status = "FAILURE/HEALTH";
		}
		else if (m_deleteStatus == NzbInfo::dsManual)
		{
			status = "DELETED/MANUAL";
		}
		else if (m_deleteStatus == NzbInfo::dsDupe)
		{
			status = "DELETED/DUPE";
		}
		else if (m_deleteStatus == NzbInfo::dsBad)
		{
			status = "FAILURE/BAD";
		}
		else if (m_deleteStatus == NzbInfo::dsGood)
		{
			status = "DELETED/GOOD";
		}
		else if (m_deleteStatus == NzbInfo::dsCopy)
		{
			status = "DELETED/COPY";
		}
		else if (m_deleteStatus == NzbInfo::dsScan)
		{
			status = "FAILURE/SCAN";
		}
		else if (m_parStatus == NzbInfo::psFailure)
		{
			status = "FAILURE/PAR";
		}
		else if (m_unpackStatus == NzbInfo::usFailure)
		{
			status = "FAILURE/UNPACK";
		}
		else if (m_moveStatus == NzbInfo::msFailure)
		{
			status = "FAILURE/MOVE";
		}
		else if (m_parStatus == NzbInfo::psManual)
		{
			status = "WARNING/DAMAGED";
		}
		else if (m_parStatus == NzbInfo::psRepairPossible)
		{
			status = "WARNING/REPAIRABLE";
		}
		else if ((m_parStatus == NzbInfo::psNone || m_parStatus == NzbInfo::psSkipped) &&
				 (m_unpackStatus == NzbInfo::usNone || m_unpackStatus == NzbInfo::usSkipped) &&
				 health < criticalHealth)
		{
			status = "FAILURE/HEALTH";
		}
		else if ((m_parStatus == NzbInfo::psNone || m_parStatus == NzbInfo::psSkipped) &&
				 (m_unpackStatus == NzbInfo::usNone || m_unpackStatus == NzbInfo::usSkipped) &&
				 health < 1000 && health >= criticalHealth)
		{
			status = "WARNING/HEALTH";
		}
		else if ((m_parStatus == NzbInfo::psNone || m_parStatus == NzbInfo::psSkipped) &&
				 (m_unpackStatus == NzbInfo::usNone || m_unpackStatus == NzbInfo::usSkipped) &&
				 scriptStatus != ScriptStatus::srFailure && health == 1000)
		{
			status = "SUCCESS/HEALTH";
		}
		else if (m_unpackStatus == NzbInfo::usSpace)
		{
			status = "WARNING/SPACE";
		}
		else if (m_unpackStatus == NzbInfo::usPassword)
		{
			status = "WARNING/PASSWORD";
		}
		else if ((m_unpackStatus == NzbInfo::usSuccess ||
				  ((m_unpackStatus == NzbInfo::usNone || m_unpackStatus == NzbInfo::usSkipped) &&
				   m_parStatus == NzbInfo::psSuccess)) &&
				 scriptStatus == ScriptStatus::srSuccess)
		{
			status = "SUCCESS/ALL";
		}
		else if (m_unpackStatus == NzbInfo::usSuccess && scriptStatus == ScriptStatus::srNone)
		{
			status = "SUCCESS/UNPACK";
		}
		else if (m_parStatus == NzbInfo::psSuccess && scriptStatus == ScriptStatus::srNone)
		{
			status = "SUCCESS/PAR";
		}
		else if (scriptStatus == ScriptStatus::srFailure)
		{
			status = "WARNING/SCRIPT";
		}
	}
	else if (m_kind == NzbInfo::nkUrl)
	{
		if (m_deleteStatus == NzbInfo::dsManual)
		{
			status = "DELETED/MANUAL";
		}
		else if (m_deleteStatus == NzbInfo::dsDupe)
		{
			status = "DELETED/DUPE";
		}
		else if (m_deleteStatus == NzbInfo::dsGood)
		{
			status = "DELETED/GOOD";
		}
		else
		{
			const char* urlStatusName[] = { "FAILURE/INTERNAL_ERROR", "FAILURE/INTERNAL_ERROR", "FAILURE/INTERNAL_ERROR",
				"FAILURE/FETCH", "FAILURE/INTERNAL_ERROR", "WARNING/SKIPPED", "FAILURE/SCAN" };
			status = urlStatusName[m_urlStatus];
		}
	}

	return status;
#endif
}

void NzbInfo::UpdateCurrentStats()
{
	m_pausedFileCount = 0;
	m_remainingParCount = 0;
	m_remainingSize = 0;
	m_pausedSize = 0;
	m_currentSuccessArticles = m_successArticles;
	m_currentFailedArticles = m_failedArticles;
	m_currentSuccessSize = m_successSize;
	m_currentFailedSize = m_failedSize;
	m_parCurrentSuccessSize = m_parSuccessSize;
	m_parCurrentFailedSize = m_parFailedSize;
	m_extraPriority = 0;

	m_currentServerStats.ListOp(&m_serverStats, ServerStatList::soSet);

	for (FileInfo* fileInfo : &m_fileList)
	{
#ifdef NZBGET_USE_RUST
		NzbgetRsDpsCurrentStatsAcc acc{
			m_remainingSize,
			m_currentSuccessSize,
			m_currentFailedSize,
			m_pausedSize,
			m_parCurrentSuccessSize,
			m_parCurrentFailedSize,
			m_currentSuccessArticles,
			m_currentFailedArticles,
			m_extraPriority,
			m_pausedFileCount,
			m_remainingParCount,
			0
		};
		nzbget_rs_dps_dl_info_update_current_stats_step(
			&acc,
			fileInfo->GetRemainingSize(),
			fileInfo->GetSuccessArticles(),
			fileInfo->GetFailedArticles(),
			fileInfo->GetSuccessSize(),
			fileInfo->GetFailedSize(),
			fileInfo->GetExtraPriority() ? 1 : 0,
			fileInfo->GetPaused() ? 1 : 0,
			fileInfo->GetParFile() ? 1 : 0);
		m_remainingSize = acc.remaining_size;
		m_currentSuccessArticles = acc.current_success_articles;
		m_currentFailedArticles = acc.current_failed_articles;
		m_currentSuccessSize = acc.current_success_size;
		m_currentFailedSize = acc.current_failed_size;
		m_extraPriority = acc.extra_priority;
		m_pausedFileCount = acc.paused_file_count;
		m_pausedSize = acc.paused_size;
		m_remainingParCount = acc.remaining_par_count;
		m_parCurrentSuccessSize = acc.par_current_success_size;
		m_parCurrentFailedSize = acc.par_current_failed_size;
#else
		m_remainingSize += fileInfo->GetRemainingSize();
		m_currentSuccessArticles += fileInfo->GetSuccessArticles();
		m_currentFailedArticles += fileInfo->GetFailedArticles();
		m_currentSuccessSize += fileInfo->GetSuccessSize();
		m_currentFailedSize += fileInfo->GetFailedSize();
		m_extraPriority += fileInfo->GetExtraPriority() ? 1 : 0;

		if (fileInfo->GetPaused())
		{
			m_pausedFileCount++;
			m_pausedSize += fileInfo->GetRemainingSize();
		}
		if (fileInfo->GetParFile())
		{
			m_remainingParCount++;
			m_parCurrentSuccessSize += fileInfo->GetSuccessSize();
			m_parCurrentFailedSize += fileInfo->GetFailedSize();
		}
#endif

		m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
	}
}

void NzbInfo::UpdateCompletedStats(FileInfo* fileInfo)
{
#ifdef NZBGET_USE_RUST
	nzbget_rs_qci_nzb_info_update_completed_stats(
		fileInfo->GetSuccessSize(),
		fileInfo->GetFailedSize(),
		fileInfo->GetFailedArticles(),
		fileInfo->GetSuccessArticles(),
		fileInfo->GetExtraPriority() ? 1 : 0,
		fileInfo->GetParFile() ? 1 : 0,
		fileInfo->GetPaused() ? 1 : 0,
		&m_successSize,
		&m_failedSize,
		&m_failedArticles,
		&m_successArticles,
		&m_extraPriority,
		&m_parSuccessSize,
		&m_parFailedSize,
		&m_remainingParCount,
		&m_pausedFileCount);
#else
	m_successSize += fileInfo->GetSuccessSize();
	m_failedSize += fileInfo->GetFailedSize();
	m_failedArticles += fileInfo->GetFailedArticles();
	m_successArticles += fileInfo->GetSuccessArticles();
	m_extraPriority -= fileInfo->GetExtraPriority() ? 1 : 0;

	if (fileInfo->GetParFile())
	{
		m_parSuccessSize += fileInfo->GetSuccessSize();
		m_parFailedSize += fileInfo->GetFailedSize();
		m_remainingParCount--;
	}

	if (fileInfo->GetPaused())
	{
		m_pausedFileCount--;
	}
#endif

	m_serverStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
}

void NzbInfo::UpdateDeletedStats(FileInfo* fileInfo)
{
#ifdef NZBGET_USE_RUST
	nzbget_rs_qci_nzb_info_update_deleted_stats(
		fileInfo->GetSize(),
		fileInfo->GetSuccessSize(),
		fileInfo->GetFailedSize(),
		fileInfo->GetMissedSize(),
		fileInfo->GetTotalArticles(),
		fileInfo->GetSuccessArticles(),
		fileInfo->GetFailedArticles(),
		fileInfo->GetMissedArticles(),
		fileInfo->GetRemainingSize(),
		fileInfo->GetExtraPriority() ? 1 : 0,
		fileInfo->GetParFile() ? 1 : 0,
		fileInfo->GetPaused() ? 1 : 0,
		&m_fileCount,
		&m_size,
		&m_currentSuccessSize,
		&m_failedSize,
		&m_failedArticles,
		&m_currentFailedSize,
		&m_totalArticles,
		&m_currentSuccessArticles,
		&m_currentFailedArticles,
		&m_remainingSize,
		&m_extraPriority,
		&m_remainingParCount,
		&m_parSize,
		&m_parCurrentSuccessSize,
		&m_parFailedSize,
		&m_parCurrentFailedSize,
		&m_pausedFileCount,
		&m_pausedSize);
#else
	m_fileCount--;
	m_size -= fileInfo->GetSize();
	m_currentSuccessSize -= fileInfo->GetSuccessSize();
	m_failedSize -= fileInfo->GetMissedSize();
	m_failedArticles -= fileInfo->GetMissedArticles();
	m_currentFailedSize -= fileInfo->GetFailedSize() + fileInfo->GetMissedSize();
	m_totalArticles -= fileInfo->GetTotalArticles();
	m_currentSuccessArticles -= fileInfo->GetSuccessArticles();
	m_currentFailedArticles -= fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles();
	m_remainingSize -= fileInfo->GetRemainingSize();
	m_extraPriority -= fileInfo->GetExtraPriority() ? 1 : 0;

	if (fileInfo->GetParFile())
	{
		m_remainingParCount--;
		m_parSize -= fileInfo->GetSize();
		m_parCurrentSuccessSize -= fileInfo->GetSuccessSize();
		m_parFailedSize -= fileInfo->GetMissedSize();
		m_parCurrentFailedSize -= fileInfo->GetFailedSize() + fileInfo->GetMissedSize();
	}

	if (fileInfo->GetPaused())
	{
		m_pausedFileCount--;
		m_pausedSize -= fileInfo->GetRemainingSize();
	}
#endif

	m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
}

bool NzbInfo::IsDownloadCompleted(bool ignorePausedPars)
{
	if (m_activeDownloads)
	{
		return false;
	}

	for (FileInfo* fileInfo : &m_fileList)
	{
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_pup_file_info_blocks_download_completion(
			fileInfo->GetPaused() ? 1 : 0, ignorePausedPars ? 1 : 0,
			fileInfo->GetParFile() ? 1 : 0, fileInfo->GetDeleted() ? 1 : 0) != 0)
#else
		if ((!fileInfo->GetPaused() || !ignorePausedPars || !fileInfo->GetParFile()) &&
			!fileInfo->GetDeleted())
#endif
		{
			return false;
		}
	}

	return true;
}

void ArticleInfo::AttachSegment(std::unique_ptr<SegmentData> content, int64 offset, int size)
{
	m_segmentContent = std::move(content);
	m_segmentOffset = offset;
	m_segmentSize = size;
}

void ArticleInfo::DiscardSegment()
{
	m_segmentContent.reset();
}


void FileInfo::SetId(int id)
{
	m_id = id;
#ifdef NZBGET_USE_RUST
	m_idMax = nzbget_rs_dps_dl_info_update_max_id(m_id, m_idMax);
#else
	if (m_idMax < m_id)
	{
		m_idMax = m_id;
	}
#endif
}

void FileInfo::ResetGenId(bool max)
{
#ifdef NZBGET_USE_RUST
	int idGen = 0;
	nzbget_rs_dps_dl_info_reset_gen_id(max ? 1 : 0, m_idMax, &idGen, &m_idMax);
	m_idGen = idGen;
#else
	if (max)
	{
		m_idGen = m_idMax;
	}
	else
	{
		m_idGen = 0;
		m_idMax = 0;
	}
#endif
}

void FileInfo::SetPaused(bool paused)
{
#ifdef NZBGET_USE_RUST
	int newPausedCount = m_nzbInfo ? m_nzbInfo->GetPausedFileCount() : 0;
	int64 newPausedSize = m_nzbInfo ? m_nzbInfo->GetPausedSize() : 0;
	if (nzbget_rs_qci_file_info_calc_paused_delta(
		m_paused ? 1 : 0,
		paused ? 1 : 0,
		m_nzbInfo ? 1 : 0,
		newPausedCount,
		newPausedSize,
		m_remainingSize,
		&newPausedCount,
		&newPausedSize) != 0)
	{
		m_nzbInfo->SetPausedFileCount(newPausedCount);
		m_nzbInfo->SetPausedSize(newPausedSize);
	}
#else
	if (m_paused != paused && m_nzbInfo)
	{
		m_nzbInfo->SetPausedFileCount(m_nzbInfo->GetPausedFileCount() + (paused ? 1 : -1));
		m_nzbInfo->SetPausedSize(m_nzbInfo->GetPausedSize() + (paused ? m_remainingSize : - m_remainingSize));
	}
#endif
	m_paused = paused;
}

void FileInfo::SetExtraPriority(bool extraPriority)
{
#ifdef NZBGET_USE_RUST
	int newExtra = m_nzbInfo ? m_nzbInfo->GetExtraPriority() : 0;
	if (nzbget_rs_qci_file_info_calc_extra_priority_delta(
		m_extraPriority ? 1 : 0,
		extraPriority ? 1 : 0,
		m_nzbInfo ? 1 : 0,
		newExtra,
		&newExtra) != 0)
	{
		m_nzbInfo->SetExtraPriority(newExtra);
	}
#else
	if (m_extraPriority != extraPriority && m_nzbInfo)
	{
		m_nzbInfo->SetExtraPriority(m_nzbInfo->GetExtraPriority() + (extraPriority ? 1 : -1));
	}
#endif
	m_extraPriority = extraPriority;
}

void FileInfo::MakeValidFilename()
{
	m_filename = FileSystem::MakeValidFilename(m_filename.c_str()).Str();
}

void FileInfo::SetActiveDownloads(int activeDownloads)
{
	m_activeDownloads = activeDownloads;

#ifdef NZBGET_USE_RUST
	int mutexAction = nzbget_rs_dds_file_info_eval_output_mutex_action(
		m_activeDownloads, m_outputFileMutex ? 1 : 0);
	if (mutexAction == 1)
	{
		m_outputFileMutex = std::make_unique<Mutex>();
	}
	else if (mutexAction == 2)
	{
		m_outputFileMutex.reset();
	}
#else
	if (m_activeDownloads > 0 && !m_outputFileMutex)
	{
		m_outputFileMutex = std::make_unique<Mutex>();
	}
	else if (m_activeDownloads == 0)
	{
		m_outputFileMutex.reset();
	}
#endif
}

bool FileInfo::IsHardLinked()
{
	return !m_hardLinkPath.empty();
}


CompletedFile::CompletedFile(int id, std::string filename, std::string origname, EStatus status,
	uint32 crc, bool parFile, std::string hash16k, std::string parSetId) 
	: m_id(id)
	, m_filename(std::move(filename))
	, m_origname(std::move(origname))
	, m_status(status)
	, m_crc(crc)
	, m_parFile(parFile)
	, m_hash16k(std::move(hash16k))
	, m_parSetId(std::move(parSetId))
{
#ifdef NZBGET_USE_RUST
	FileInfo::m_idMax = nzbget_rs_dps_dl_info_update_max_id(m_id, FileInfo::m_idMax);
#else
	if (FileInfo::m_idMax < m_id)
	{
		FileInfo::m_idMax = m_id;
	}
#endif
}

bool CompletedFile::SameFilename(const char* name) const
{
#ifdef NZBGET_USE_RUST
	return nzbget_rs_qhp_completed_same_filename(m_filename.c_str(), name) != 0;
#else
	if (!name) return false;

	std::string_view s1 = m_filename;
	std::string_view s2 = name;
	if (s1.size() != s2.size())
		return false;

	return std::equal(s1.begin(), s1.end(), s2.begin(), [](char c1, char c2)
	{
		if (c1 == '\\') c1 = '/';
		if (c2 == '\\') c2 = '/';
		return std::tolower(static_cast<unsigned char>(c1)) == std::tolower(static_cast<unsigned char>(c2));
	});
#endif
}

void DupInfo::SetId(int id)
{
	m_id = id;
#ifdef NZBGET_USE_RUST
	NzbInfo::m_idMax = nzbget_rs_dps_dl_info_update_max_id(m_id, NzbInfo::m_idMax);
#else
	if (NzbInfo::m_idMax < m_id)
	{
		NzbInfo::m_idMax = m_id;
	}
#endif
}


HistoryInfo::~HistoryInfo()
{
	if ((m_kind == hkNzb || m_kind == hkUrl) && m_info)
	{
		delete (NzbInfo*)m_info;
	}
	else if (m_kind == hkDup && m_info)
	{
		delete (DupInfo*)m_info;
	}
}

int HistoryInfo::GetId()
{
	if ((m_kind == hkNzb || m_kind == hkUrl))
	{
		return ((NzbInfo*)m_info)->GetId();
	}
	else // if (m_eKind == hkDup)
	{
		return ((DupInfo*)m_info)->GetId();
	}
}

const char* HistoryInfo::GetName()
{
	if (m_kind == hkNzb || m_kind == hkUrl)
	{
		return GetNzbInfo()->GetName();
	}
	else if (m_kind == hkDup)
	{
		return GetDupInfo()->GetName();
	}
	else
	{
		return "<unknown>";
	}
}


void DownloadQueue::CalcRemainingSize(int64* remaining, int64* remainingForced)
{
	int64 remainingSize = 0;
	int64 remainingForcedSize = 0;

	for (NzbInfo* nzbInfo : &m_queue)
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
#ifdef NZBGET_USE_RUST
			nzbget_rs_dps_dl_queue_calc_remaining_size_step(
				fileInfo->GetPaused() ? 1 : 0,
				fileInfo->GetDeleted() ? 1 : 0,
				nzbInfo->GetForcePriority() ? 1 : 0,
				fileInfo->GetRemainingSize(),
				remainingSize,
				remainingForcedSize,
				&remainingSize,
				&remainingForcedSize);
#else
			if (!fileInfo->GetPaused() && !fileInfo->GetDeleted())
			{
				remainingSize += fileInfo->GetRemainingSize();
				if (nzbInfo->GetForcePriority())
				{
					remainingForcedSize += fileInfo->GetRemainingSize();
				}
			}
#endif
		}
	}

	*remaining = remainingSize;

	if (remainingForced)
	{
		*remainingForced = remainingForcedSize;
	}
}
