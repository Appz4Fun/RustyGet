/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2012-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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

#include "nzbget.h"
#include "Unpack.h"

#include "UrlCoordinator.h"
#include "Options.h"
#include "WorkState.h"
#include "WebDownloader.h"
#include "Util.h"
#include "FileSystem.h"
#include "NzbFile.h"
#include "Scanner.h"
#include "DiskState.h"
#include "QueueScript.h"
#include "nzbget_rs.h"

void UrlDownloader::ProcessHeader(const char* line)
{
	WebDownloader::ProcessHeader(line);

	// rust/src/urlcoord.rs; most headers aren't X-DNZB ones
	if (nzbget_rs_dqh_url_dl_has_dnzb_prefix(line) == 0)
	{
		return;
	}

	size_t start, length;
	NzbgetRsBuf name{}, value{};
	switch (nzbget_rs_url_dnzb_header(line, &start, &length, &name, &value))
	{
		case 1:
			// a length of 0 would take the rest of the line
			m_category = length > 0 ? CString(line + start, (int)length) : CString("");
			debug("Category: %s", *m_category);
			break;

		case 2:
			debug("X-DNZB: %s", name.data);
			debug("Value: %s", value.data);
			m_nzbInfo->GetParameters()->SetParameter(name.data, value.data);
			nzbget_rs_free(name);
			nzbget_rs_free(value);
			break;
	}
}

UrlCoordinator::UrlCoordinator()
{
	m_downloadQueueObserver.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);
}

UrlCoordinator::~UrlCoordinator()
{
	debug("Destroying UrlCoordinator");

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		delete urlDownloader;
	}
	m_activeDownloads.clear();

	debug("UrlCoordinator destroyed");
}

void UrlCoordinator::Run()
{
	debug("Entering UrlCoordinator-loop");

	while (!DownloadQueue::IsLoaded())
	{
		Util::Sleep(20);
	}

	time_t lastReset = 0;
	while (!IsStopped())
	{
		bool downloadStarted = false;

		{
			NzbInfo* nzbInfo = nullptr;
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			if ((int)m_activeDownloads.size() < g_Options->GetUrlConnections())
			{
				nzbInfo = GetNextUrl(downloadQueue);
				if (nzbget_rs_fuq_url_coord_should_start_download(
					nzbInfo != nullptr ? 1 : 0,
					g_WorkState->GetPauseDownload() ? 1 : 0,
					g_Options->GetUrlForce() ? 1 : 0) != 0)
				{
					StartUrlDownload(nzbInfo);
					downloadStarted = true;
				}
			}
			m_hasMoreJobs = nzbget_rs_uqs_url_coord_has_more_jobs(
				m_activeDownloads.empty() ? 1 : 0, nzbInfo != nullptr ? 1 : 0) != 0;
		}

		if (lastReset != Util::CurrentTime())
		{
			// this code should not be called too often, once per second is OK
			ResetHangingDownloads();
			lastReset = Util::CurrentTime();
		}

		if (!m_hasMoreJobs && !IsStopped())
		{
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait(lk, [&] { return m_hasMoreJobs || IsStopped(); });
		}
		else
		{
			int sleepInterval = nzbget_rs_fuq_url_coord_calc_sleep_interval(downloadStarted ? 1 : 0);
			Util::Sleep(sleepInterval);
		}
	}

	WaitJobs();

	debug("Exiting UrlCoordinator-loop");
}

void UrlCoordinator::WaitJobs()
{
	// waiting for downloads
	debug("UrlCoordinator: waiting for Downloads to complete");

	while (true)
	{
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			if (m_activeDownloads.empty())
			{
				break;
			}
		}
		Util::Sleep(100);
		ResetHangingDownloads();
	}

	debug("UrlCoordinator: Downloads are completed");
}

void UrlCoordinator::Stop()
{
	Thread::Stop();

	debug("Stopping UrlDownloads");
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		for (UrlDownloader* urlDownloader : m_activeDownloads)
		{
			urlDownloader->Stop();
		}
	}
	debug("UrlDownloads are notified");

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

void UrlCoordinator::DownloadQueueUpdate(Subject*, void* aspect)
{
	debug("Notification from download queue received");

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (nzbget_rs_ucf_url_coord_is_wake_action(
		(int)queueAspect->action,
		(int)DownloadQueue::eaUrlAdded,
		(int)DownloadQueue::eaUrlReturned) != 0)
	{
		// Resume Run()
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_hasMoreJobs = true;
		m_waitCond.notify_all();
	}
}

void UrlCoordinator::ResetHangingDownloads()
{
	if (g_Options->GetUrlTimeout() == 0)
	{
		return;
	}

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	time_t tm = Util::CurrentTime();

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		if (nzbget_rs_sus_is_hanging_url_download(
			static_cast<int64>(tm),
			static_cast<int64>(urlDownloader->GetLastUpdateTime()),
			g_Options->GetUrlTimeout(),
			static_cast<int>(urlDownloader->GetStatus()),
			static_cast<int>(UrlDownloader::adRunning)))
		{
			error("Cancelling hanging url download %s", urlDownloader->GetInfoName());
			urlDownloader->Stop();
		}
	}
}

void UrlCoordinator::LogDebugInfo()
{
	info("   ---------- UrlCoordinator");

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	info("    Active Downloads: %i", (int)m_activeDownloads.size());
	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		urlDownloader->LogDebugInfo();
	}
}

/*
 * Returns next URL for download.
 */
NzbInfo* UrlCoordinator::GetNextUrl(DownloadQueue* downloadQueue)
{
	size_t n = downloadQueue->GetQueue()->size();
	if (n == 0)
	{
		return nullptr;
	}
	std::vector<NzbInfo*> items;
	std::vector<int> kinds;
	std::vector<int> urlStatuses;
	std::vector<int> deleteStatuses;
	std::vector<int> priorities;
	items.reserve(n);
	kinds.reserve(n);
	urlStatuses.reserve(n);
	deleteStatuses.reserve(n);
	priorities.reserve(n);
	for (NzbInfo* nzbInfo1 : downloadQueue->GetQueue())
	{
		items.push_back(nzbInfo1);
		kinds.push_back(static_cast<int>(nzbInfo1->GetKind()));
		urlStatuses.push_back(static_cast<int>(nzbInfo1->GetUrlStatus()));
		deleteStatuses.push_back(static_cast<int>(nzbInfo1->GetDeleteStatus()));
		priorities.push_back(nzbInfo1->GetPriority());
	}
	int idx = nzbget_rs_sus_select_next_url(
		kinds.data(),
		urlStatuses.data(),
		deleteStatuses.data(),
		priorities.data(),
		n,
		static_cast<int>(NzbInfo::nkUrl),
		static_cast<int>(NzbInfo::lsNone),
		static_cast<int>(NzbInfo::dsNone));
	return idx >= 0 && static_cast<size_t>(idx) < items.size() ? items[idx] : nullptr;
}

void UrlCoordinator::StartUrlDownload(NzbInfo* nzbInfo)
{
	debug("Starting new UrlDownloader");

	UrlDownloader* urlDownloader = new UrlDownloader();
	urlDownloader->SetAutoDestroy(true);
	urlDownloader->Attach(this);
	urlDownloader->SetNzbInfo(nzbInfo);
	urlDownloader->SetUrl(nzbInfo->GetUrl());
	urlDownloader->SetForce(g_Options->GetUrlForce());
	urlDownloader->SetInfoName(nzbInfo->MakeNiceUrlName(nzbInfo->GetUrl(), nzbInfo->GetFilename()));
	NzbgetRsBuf outPathBuf = nzbget_rs_dqh_url_coord_format_temp_url_path(
		g_Options->GetTempDir(), PATH_SEPARATOR, nzbInfo->GetId());
	urlDownloader->SetOutputFilename(outPathBuf.data ? outPathBuf.data : "");
	nzbget_rs_free(outPathBuf);

	nzbInfo->SetActiveDownloads(1);
	nzbInfo->SetUrlStatus(NzbInfo::lsRunning);

	m_activeDownloads.push_back(urlDownloader);
	urlDownloader->Start();
}

void UrlCoordinator::Update(Subject* caller, void*)
{
	debug("Notification from UrlDownloader received");

	UrlDownloader* urlDownloader = (UrlDownloader*)caller;
	if (nzbget_rs_fuq_url_coord_is_completed_status(
		(int)urlDownloader->GetStatus(),
		(int)WebDownloader::adFinished,
		(int)WebDownloader::adFailed,
		(int)WebDownloader::adRetry) != 0)
	{
		UrlCompleted(urlDownloader);
	}
}

void UrlCoordinator::UrlCompleted(UrlDownloader* urlDownloader)
{
	debug("URL downloaded");

	NzbInfo* nzbInfo = urlDownloader->GetNzbInfo();

	const char* origname;
	if (urlDownloader->GetOriginalFilename())
	{
		origname = urlDownloader->GetOriginalFilename();
	}
	else
	{
		origname = FileSystem::BaseFileName(nzbInfo->GetUrl());

		// TODO: decode URL escaping
	}

	CString filename = FileSystem::MakeValidFilename(origname);

	debug("Filename: [%s]", *filename);

	bool retry;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		// remove downloader from downloader list
		m_activeDownloads.erase(std::find(m_activeDownloads.begin(), m_activeDownloads.end(), urlDownloader));

		// rust/src/urlcoord.rs
		int urlStatus = nzbInfo->GetUrlStatus();
		int deleteStatus = nzbInfo->GetDeleteStatus();
		int again = 0;
		nzbget_rs_url_completed(urlDownloader->GetStatus(), nzbInfo->GetDeleting(), &urlStatus, &deleteStatus, &again);
		retry = again != 0;
		if (nzbInfo->GetDeleting())
		{
			nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)deleteStatus);
			nzbInfo->SetDeleting(false);
		}
		nzbInfo->SetUrlStatus((NzbInfo::EUrlStatus)urlStatus);

		if (!retry)
		{
			DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlCompleted, downloadQueue, nzbInfo, nullptr};
			downloadQueue->Notify(&aspect);
		}
	}

	if (retry)
	{
		nzbInfo->SetActiveDownloads(0);
		return;
	}

	if (nzbInfo->GetUrlStatus() == NzbInfo::lsFinished)
	{
		const char* name = nzbget_rs_uqs_url_coord_use_primary_str(nzbInfo->GetFilename()) != 0
			? nzbInfo->GetFilename() : *filename;
		const char* category = nzbget_rs_uqs_url_coord_use_primary_str(nzbInfo->GetCategory()) != 0
			? nzbInfo->GetCategory() : urlDownloader->GetCategory();

		Scanner::EAddStatus addStatus;
		if (Unpack::IsArchive(name))
		{
			addStatus = g_Scanner->AddArchive(
				name,
				category,
				nzbInfo->GetAutoCategory(),
				nzbInfo->GetPriority(),
				nzbInfo->GetDupeKey(),
				nzbInfo->GetDupeScore(),
				nzbInfo->GetDupeMode(),
				nzbInfo->GetParameters(),
				false,
				nzbInfo->GetAddUrlPaused(),
				nzbInfo,
				urlDownloader->GetOutputFilename()
			);
		}
		else 
		{
			addStatus = g_Scanner->AddExternalFile(
				name,
				category,
				nzbInfo->GetAutoCategory(),
				nzbInfo->GetPriority(),
				nzbInfo->GetDupeKey(),
				nzbInfo->GetDupeScore(),
				nzbInfo->GetDupeMode(),
				nzbInfo->GetParameters(),
				false,
				nzbInfo->GetAddUrlPaused(),
				nzbInfo,
				urlDownloader->GetOutputFilename(),
				nullptr,
				0,
				nullptr
			);
		}

		if (addStatus == Scanner::asSuccess)
		{
			// if scanner has successfully added nzb-file to queue, our nzbInfo is
			// already removed from queue and destroyed
			return;
		}

		nzbInfo->SetUrlStatus((NzbInfo::EUrlStatus)nzbget_rs_fuq_url_coord_resolve_scan_fail_status(
			(int)addStatus, (int)Scanner::asFailed,
			(int)NzbInfo::lsScanFailed, (int)NzbInfo::lsScanSkipped));
	}

	// the rest of function is only for failed URLs or for failed scans

	g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeUrlCompleted);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		nzbInfo->SetActiveDownloads(0);

		DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlFailed, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);
	}
}

bool UrlCoordinator::DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory)
{
	if (nzbInfo->GetActiveDownloads() > 0)
	{
		info("Deleting active URL %s", nzbInfo->GetName());
		nzbInfo->SetDeleting(true);
		nzbInfo->SetAvoidHistory(avoidHistory);

		for (UrlDownloader* urlDownloader : m_activeDownloads)
		{
			if (urlDownloader->GetNzbInfo() == nzbInfo)
			{
				urlDownloader->Stop();
				return true;
			}
		}

		return false;
	}

	info("Deleting URL %s", nzbInfo->GetName());

	nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)nzbget_rs_ucf_url_coord_resolve_delete_status(
		(int)nzbInfo->GetDeleteStatus(), (int)NzbInfo::dsNone, (int)NzbInfo::dsManual));
	nzbInfo->SetUrlStatus(NzbInfo::lsNone);

	DownloadQueue::Aspect deletedAspect = {DownloadQueue::eaUrlDeleted, downloadQueue, nzbInfo, nullptr};
	downloadQueue->Notify(&deletedAspect);

	return true;
}

void UrlCoordinator::AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst)
{
	debug("Adding URL to queue");

	NzbInfo* addedNzb = nzbInfo.get();

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	DownloadQueue::Aspect foundAspect = {DownloadQueue::eaUrlFound, downloadQueue, addedNzb, nullptr};
	downloadQueue->Notify(&foundAspect);

	if (addedNzb->GetDeleteStatus() != NzbInfo::dsManual)
	{
		downloadQueue->GetQueue()->Add(std::move(nzbInfo), addFirst);

		DownloadQueue::Aspect addedAspect = {DownloadQueue::eaUrlAdded, downloadQueue, addedNzb, nullptr};
		downloadQueue->Notify(&addedAspect);
	}

	downloadQueue->Save();
}
