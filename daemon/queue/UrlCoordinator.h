/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2012-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef URLCOORDINATOR_H
#define URLCOORDINATOR_H

#include "NString.h"
#include "Log.h"
#include "Thread.h"
#include "WebDownloader.h"
#include "DownloadInfo.h"
#include "Observer.h"

class UrlDownloader;

class UrlCoordinator : public Thread, public Observer, public Debuggable
{
public:
	UrlCoordinator();
	~UrlCoordinator() override;
	void Run() override;
	void Stop() override;
	void Update(Subject* caller, void* aspect) override;

	// Editing the queue
	void AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst);
	bool HasMoreJobs() const { return m_hasMoreJobs; }
	bool DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory);

protected:
	void LogDebugInfo() override;

private:
	using ActiveDownloads = std::list<UrlDownloader*>;

	class DownloadQueueObserver final : public Observer
	{
	public:
		UrlCoordinator* m_owner;
		void Update(Subject* caller, void* aspect) override { m_owner->DownloadQueueUpdate(caller, aspect); }
	};

	ActiveDownloads m_activeDownloads;
	std::atomic<bool> m_hasMoreJobs{true};
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	DownloadQueueObserver m_downloadQueueObserver;

	NzbInfo* GetNextUrl(DownloadQueue* downloadQueue);
	void StartUrlDownload(NzbInfo* nzbInfo);
	void UrlCompleted(UrlDownloader* urlDownloader);
	void ResetHangingDownloads();
	void WaitJobs();
	void DownloadQueueUpdate(Subject* caller, void* aspect);
};

extern UrlCoordinator* g_UrlCoordinator;

class UrlDownloader : public WebDownloader
{
public:
	void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
	NzbInfo* GetNzbInfo() { return m_nzbInfo; }
	const char* GetCategory() const { return m_category; }

protected:
	void ProcessHeader(const char* line) override;

private:
	NzbInfo* m_nzbInfo;
	CString m_category;
};

#include <algorithm>
#include "Options.h"
#include "WorkState.h"
#include "Util.h"
#include "FileSystem.h"
#include "NzbFile.h"
#include "Scanner.h"
#include "DiskState.h"
#include "QueueScript.h"
#include "nzbget_rs.h"

inline void UrlDownloader::ProcessHeader(const char* line)
{
	WebDownloader::ProcessHeader(line);

	size_t start, length;
	NzbgetRsBuf name{}, value{};
	switch (nzbget_rs_url_dnzb_header(line, &start, &length, &name, &value))
	{
		case 1:
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

inline UrlCoordinator::UrlCoordinator()
{
	m_downloadQueueObserver.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);
}

inline UrlCoordinator::~UrlCoordinator()
{
	debug("Destroying UrlCoordinator");

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		delete urlDownloader;
	}
	m_activeDownloads.clear();

	debug("UrlCoordinator destroyed");
}

inline void UrlCoordinator::Run()
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
				if (nzbInfo && (!g_WorkState->GetPauseDownload() || g_Options->GetUrlForce()))
				{
					StartUrlDownload(nzbInfo);
					downloadStarted = true;
				}
			}
			m_hasMoreJobs = !m_activeDownloads.empty() || nzbInfo != nullptr;
		}

		if (lastReset != Util::CurrentTime())
		{
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
			Util::Sleep(downloadStarted ? 0 : 100);
		}
	}

	WaitJobs();

	debug("Exiting UrlCoordinator-loop");
}

inline void UrlCoordinator::WaitJobs()
{
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

inline void UrlCoordinator::Stop()
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

	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

inline void UrlCoordinator::DownloadQueueUpdate(Subject*, void* aspect)
{
	debug("Notification from download queue received");

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaUrlAdded ||
		queueAspect->action == DownloadQueue::eaUrlReturned)
	{
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_hasMoreJobs = true;
		m_waitCond.notify_all();
	}
}

inline void UrlCoordinator::ResetHangingDownloads()
{
	if (g_Options->GetUrlTimeout() == 0)
	{
		return;
	}

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	time_t tm = Util::CurrentTime();

	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		if (nzbget_rs_url_coord_is_hanging_download(
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

inline void UrlCoordinator::LogDebugInfo()
{
	info("   ---------- UrlCoordinator");

	GuardedDownloadQueue guard = DownloadQueue::Guard();
	info("    Active Downloads: %i", (int)m_activeDownloads.size());
	for (UrlDownloader* urlDownloader : m_activeDownloads)
	{
		urlDownloader->LogDebugInfo();
	}
}

inline NzbInfo* UrlCoordinator::GetNextUrl(DownloadQueue* downloadQueue)
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
	int idx = nzbget_rs_url_coord_select_next_url(
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

inline void UrlCoordinator::StartUrlDownload(NzbInfo* nzbInfo)
{
	debug("Starting new UrlDownloader");

	UrlDownloader* urlDownloader = new UrlDownloader();
	urlDownloader->SetAutoDestroy(true);
	urlDownloader->Attach(this);
	urlDownloader->SetNzbInfo(nzbInfo);
	urlDownloader->SetUrl(nzbInfo->GetUrl());
	urlDownloader->SetForce(g_Options->GetUrlForce());
	urlDownloader->SetInfoName(nzbInfo->MakeNiceUrlName(nzbInfo->GetUrl(), nzbInfo->GetFilename()));
	NzbgetRsBuf outPathBuf = nzbget_rs_url_coord_format_temp_path(
		g_Options->GetTempDir(), PATH_SEPARATOR, nzbInfo->GetId());
	urlDownloader->SetOutputFilename(outPathBuf.data ? outPathBuf.data : "");
	nzbget_rs_free(outPathBuf);

	nzbInfo->SetActiveDownloads(1);
	nzbInfo->SetUrlStatus(NzbInfo::lsRunning);

	m_activeDownloads.push_back(urlDownloader);
	urlDownloader->Start();
}

inline void UrlCoordinator::Update(Subject* caller, void*)
{
	debug("Notification from UrlDownloader received");

	UrlDownloader* urlDownloader = (UrlDownloader*)caller;
	if (urlDownloader->GetStatus() == WebDownloader::adFinished ||
		urlDownloader->GetStatus() == WebDownloader::adFailed ||
		urlDownloader->GetStatus() == WebDownloader::adRetry)
	{
		UrlCompleted(urlDownloader);
	}
}

inline void UrlCoordinator::UrlCompleted(UrlDownloader* urlDownloader)
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
	}

	CString filename = FileSystem::MakeValidFilename(origname);

	debug("Filename: [%s]", *filename);

	bool retry;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		m_activeDownloads.erase(std::find(m_activeDownloads.begin(), m_activeDownloads.end(), urlDownloader));

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
		const char* name = !Util::EmptyStr(nzbInfo->GetFilename())
			? nzbInfo->GetFilename() : *filename;
		const char* category = !Util::EmptyStr(nzbInfo->GetCategory())
			? nzbInfo->GetCategory() : urlDownloader->GetCategory();

		Scanner::EAddStatus addStatus;
		if (nzbget_rs_unpack_can_extract_with_zipfast(name) != 0)
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
			return;
		}

		nzbInfo->SetUrlStatus(addStatus == Scanner::asFailed
			? NzbInfo::lsScanFailed : NzbInfo::lsScanSkipped);
	}

	g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeUrlCompleted);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		nzbInfo->SetActiveDownloads(0);

		DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlFailed, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);
	}
}

inline bool UrlCoordinator::DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory)
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

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
	}
	nzbInfo->SetUrlStatus(NzbInfo::lsNone);

	DownloadQueue::Aspect deletedAspect = {DownloadQueue::eaUrlDeleted, downloadQueue, nzbInfo, nullptr};
	downloadQueue->Notify(&deletedAspect);

	return true;
}

inline void UrlCoordinator::AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst)
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

#endif
