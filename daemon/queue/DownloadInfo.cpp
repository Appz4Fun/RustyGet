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
#include "nzbget_rs.h"

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
	else if (!emptyVal)
	{
		if (pos != end())
		{
			pos->SetValue(value);
		}
		else
		{
			emplace_back(name, value);
		}
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
	std::vector<int32_t> statuses;
	statuses.reserve(size());
	for (ScriptStatus& scriptStatus : this)
	{
		statuses.push_back((int32_t)scriptStatus.GetStatus());
	}
	return (ScriptStatus::EStatus)nzbget_rs_downloadinfo_script_calc_total_status(
		statuses.empty() ? nullptr : statuses.data(), statuses.size());
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

	int newSuccess = serverStat->GetSuccessArticles();
	int newFailed = serverStat->GetFailedArticles();
	nzbget_rs_downloadinfo_server_stat_apply_op(
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
	if (m_idMax < m_id)
	{
		m_idMax = m_id;
	}
}

void NzbInfo::ResetGenId(bool max)
{
	if (max)
	{
		m_idGen = m_idMax;
	}
	else
	{
		m_idGen = 0;
		m_idMax = 0;
	}
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
			nzbNicename.Format("nzb_%i", GetId());
		}
		SetName(nzbNicename);
	}
}

CString NzbInfo::MakeNiceNzbName(const char* nzbFilename, bool removeExt)
{
	NzbgetRsBuf buf = nzbget_rs_downloadinfo_make_nice_nzb_name(nzbFilename, removeExt ? 1 : 0);
	std::string out(buf.data ? buf.data : "nzb", buf.len);
	nzbget_rs_free(buf);
	return out.c_str();
}

CString NzbInfo::MakeNiceUrlName(const char* urlStr, const char* nzbFilename)
{
	NzbgetRsBuf buf = nzbget_rs_downloadinfo_make_nice_url_name(urlStr, nzbFilename);
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out.c_str();
}

void NzbInfo::BuildDestDirName()
{
	if (Util::EmptyStr(g_Options->GetInterDir()))
	{
		m_destDir = BuildFinalDirName();
	}
	else
	{
		NzbgetRsBuf destBuf = nzbget_rs_downloadinfo_build_dest_dir_name(
			g_Options->GetInterDir(), nullptr, nullptr, nullptr, 0, GetName(), GetId());
		m_destDir = destBuf.data ? destBuf.data : "";
		nzbget_rs_free(destBuf);
	}
}

CString NzbInfo::BuildFinalDirName()
{
	const char* categoryDestDir = nullptr;
	if (!m_category.Empty())
	{
		Options::Category* category = g_Options->FindCategory(m_category, false);
		if (category && !Util::EmptyStr(category->GetDestDir()))
		{
			categoryDestDir = category->GetDestDir();
		}
	}

	NzbgetRsBuf finalBuf = nzbget_rs_downloadinfo_build_final_dir_name(
		g_Options->GetDestDir(),
		categoryDestDir,
		m_category,
		g_Options->GetAppendCategoryDir() ? 1 : 0,
		GetName(),
		GetId());
	CString finalDir = finalBuf.data ? finalBuf.data : "";
	nzbget_rs_free(finalBuf);
	return finalDir;
}

int NzbInfo::CalcHealth()
{
	return nzbget_rs_qhp_calc_health(m_size, m_parSize, m_currentFailedSize, m_parCurrentFailedSize);
}

bool NzbInfo::RenameCompletedFile(const char* oldName, const char* newAbsoluteOrRelativePath)
{
	return nzbget_rs_qhp_rename_completed_file(
		m_completedFiles.size(),
		oldName,
		newAbsoluteOrRelativePath,
		&m_completedFiles,
		[](void* rawCtx, size_t idx, const char** outFilename, const char** outOrigname) {
			auto& list = *static_cast<CompletedFileList*>(rawCtx);
			*outFilename = list[idx].GetFilename();
			*outOrigname = list[idx].GetOrigname();
		},
		[](void* rawCtx, size_t idx, const char* newFilename, const char* newOrigname) {
			auto& list = *static_cast<CompletedFileList*>(rawCtx);
			if (newOrigname)
			{
				list[idx].SetOrigname(newOrigname);
			}
			list[idx].SetFilename(newFilename);
		}) != 0;
}

int NzbInfo::CalcCriticalHealth(bool allowEstimation)
{
	return nzbget_rs_qhp_calc_critical_health(
		m_size, m_parSize, m_parCurrentFailedSize, allowEstimation ? 1 : 0);
}

void NzbInfo::UpdateMinMaxTime()
{
	std::vector<int64_t> times;
	times.reserve(m_fileList.size());
	for (FileInfo* fileInfo : &m_fileList)
	{
		times.push_back((int64_t)fileInfo->GetTime());
	}
	int64_t minTime = 0, maxTime = 0;
	nzbget_rs_downloadinfo_update_min_max_time(
		times.empty() ? nullptr : times.data(), times.size(), &minTime, &maxTime);
	m_minTime = (time_t)minTime;
	m_maxTime = (time_t)maxTime;
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

	while (m_messages.size() > (uint32)g_Options->GetLogBuffer())
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
	int outSec = m_downloadSec;
	int64 outStartTime = (int64)m_downloadStartTime;
	int outStartSec = m_downloadStartSec;
	int outChanged = 0;
	nzbget_rs_downloadinfo_calc_active_downloads_sec(
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
	m_activeDownloads = activeDownloads;
}

bool NzbInfo::IsDupeSuccess()
{
	return nzbget_rs_downloadinfo_is_dupe_success(
		(int)m_markStatus, (int)m_deleteStatus, (int)m_parStatus,
		(int)m_unpackStatus, (int)m_urlStatus, CalcHealth(),
		CalcCriticalHealth(true), CalcCriticalHealth(false),
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0) != 0;
}

const char* NzbInfo::MakeTextStatus(bool ignoreScriptStatus)
{
	int health = m_kind == NzbInfo::nkNzb ? CalcHealth() : 0;
	int criticalHealth = m_kind == NzbInfo::nkNzb ? CalcCriticalHealth(false) : 0;
	ScriptStatus::EStatus scriptStatus = (m_kind == NzbInfo::nkNzb && !ignoreScriptStatus)
		? m_scriptStatuses.CalcTotalStatus()
		: ScriptStatus::srSuccess;
	return nzbget_rs_downloadinfo_make_text_status(
		(int)m_kind, (int)m_markStatus, (int)m_deleteStatus, (int)m_parStatus,
		(int)m_unpackStatus, (int)m_moveStatus, (int)m_urlStatus,
		(int)scriptStatus, health, criticalHealth);
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
		nzbget_rs_downloadinfo_update_current_stats_step(
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

		m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
	}
}

void NzbInfo::UpdateCompletedStats(FileInfo* fileInfo)
{
	nzbget_rs_downloadinfo_update_completed_stats(
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

	m_serverStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
}

void NzbInfo::UpdateDeletedStats(FileInfo* fileInfo)
{
	nzbget_rs_downloadinfo_update_deleted_stats(
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

	m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
}

bool NzbInfo::IsDownloadCompleted(bool ignorePausedPars)
{
	return nzbget_rs_downloadinfo_is_download_completed(
		m_activeDownloads,
		ignorePausedPars ? 1 : 0,
		m_fileList.size(),
		&m_fileList,
		[](void* rawCtx, size_t idx, int* outPaused, int* outParFile, int* outDeleted) {
			auto* list = static_cast<FileList*>(rawCtx);
			FileInfo* fi = (*list)[idx].get();
			*outPaused = fi->GetPaused() ? 1 : 0;
			*outParFile = fi->GetParFile() ? 1 : 0;
			*outDeleted = fi->GetDeleted() ? 1 : 0;
		}) != 0;
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
	if (m_idMax < m_id)
	{
		m_idMax = m_id;
	}
}

void FileInfo::ResetGenId(bool max)
{
	if (max)
	{
		m_idGen = m_idMax;
	}
	else
	{
		m_idGen = 0;
		m_idMax = 0;
	}
}

void FileInfo::SetPaused(bool paused)
{
	if (m_paused != paused && m_nzbInfo)
	{
		m_nzbInfo->SetPausedFileCount(m_nzbInfo->GetPausedFileCount() + (paused ? 1 : -1));
		m_nzbInfo->SetPausedSize(m_nzbInfo->GetPausedSize() + (paused ? m_remainingSize : -m_remainingSize));
	}
	m_paused = paused;
}

void FileInfo::SetExtraPriority(bool extraPriority)
{
	if (m_extraPriority != extraPriority && m_nzbInfo)
	{
		m_nzbInfo->SetExtraPriority(m_nzbInfo->GetExtraPriority() + (extraPriority ? 1 : -1));
	}
	m_extraPriority = extraPriority;
}

void FileInfo::MakeValidFilename()
{
	m_filename = FileSystem::MakeValidFilename(m_filename.c_str()).Str();
}

void FileInfo::SetActiveDownloads(int activeDownloads)
{
	m_activeDownloads = activeDownloads;

	if (m_activeDownloads > 0 && !m_outputFileMutex)
	{
		m_outputFileMutex = std::make_unique<Mutex>();
	}
	else if (m_activeDownloads == 0 && m_outputFileMutex)
	{
		m_outputFileMutex.reset();
	}
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
	if (FileInfo::m_idMax < m_id)
	{
		FileInfo::m_idMax = m_id;
	}
}

bool CompletedFile::SameFilename(const char* name) const
{
	return nzbget_rs_downloadinfo_completed_same_filename(m_filename.c_str(), name) != 0;
}

void DupInfo::SetId(int id)
{
	m_id = id;
	if (NzbInfo::m_idMax < m_id)
	{
		NzbInfo::m_idMax = m_id;
	}
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
			if (!fileInfo->GetPaused() && !fileInfo->GetDeleted())
			{
				remainingSize += fileInfo->GetRemainingSize();
				if (nzbInfo->GetForcePriority())
				{
					remainingForcedSize += fileInfo->GetRemainingSize();
				}
			}
		}
	}

	*remaining = remainingSize;

	if (remainingForced)
	{
		*remainingForced = remainingForcedSize;
	}
}
