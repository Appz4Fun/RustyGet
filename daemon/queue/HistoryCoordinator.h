/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef HISTORYCOORDINATOR_H
#define HISTORYCOORDINATOR_H

#include "DownloadInfo.h"
#include "Log.h"

class HistoryCoordinator : public Service
{
public:
	void AddToHistory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void RecheckFailedArticles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action, const char* args);
	void DeleteDiskFiles(NzbInfo* nzbInfo);
	void HistoryHide(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, int rindex);
	void Redownload(DownloadQueue* downloadQueue, HistoryInfo* historyInfo);

protected:
	virtual int ServiceInterval() { return 60 * 60; }
	virtual void ServiceWork();

private:
	void HistoryDelete(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool final);
	void HistoryReturn(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo);
	void HistoryProcess(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo);
	void HistoryRedownload(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool restorePauseState);
	void HistoryRetry(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool resetFailed, bool reprocess);
	bool HistorySetParameter(HistoryInfo* historyInfo, const char* text);
	void HistorySetDupeParam(HistoryInfo* historyInfo, DownloadQueue::EEditAction action, const char* text);
	bool HistorySetCategory(HistoryInfo* historyInfo, const char* text);
	bool HistorySetName(HistoryInfo* historyInfo, const char* text);
	void MoveToQueue(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool reprocess);
	void PrepareEdit(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action);
	void ResetArticles(FileInfo* fileInfo, bool allFailed, bool resetFailed);
};

extern HistoryCoordinator* g_HistoryCoordinator;

#include <set>
#include "Options.h"
#include "Log.h"
#include "QueueCoordinator.h"
#include "DiskState.h"
#include "Util.h"
#include "FileSystem.h"
#include "DupeCoordinator.h"
#include "PrePostProcessor.h"
#include "ServerPool.h"
#include "DupeArticleFallback.h"
#include "DupeSearch.h"
#include "nzbget_rs.h"

/**
 * Removes old entries from (recent) history
 */
inline void HistoryCoordinator::ServiceWork()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	// in 64 bits: past 24,855 days the int product went negative, and every item was
	// hidden as too old
	time_t minTime = (time_t)nzbget_rs_history_coord_calc_min_time(
		(int64)Util::CurrentTime(), g_Options->GetKeepHistory());
	bool changed = false;
	int index = 0;

	// traversing in a reverse order to delete items in order they were added to history
	// (just to produce the log-messages in a more logical order)
	for (HistoryList::reverse_iterator it = downloadQueue->GetHistory()->rbegin(); it != downloadQueue->GetHistory()->rend(); )
	{
		HistoryInfo* historyInfo = (*it).get();
		int action = nzbget_rs_history_coord_classify_service_work_item(
			(int)historyInfo->GetKind(),
			(int)HistoryInfo::hkDup,
			(int)HistoryInfo::hkNzb,
			(int64)historyInfo->GetTime(),
			(int64)minTime,
			g_Options->GetDupeCheck() ? 1 : 0);
		if (action != 0)
		{
			if (action == 1)
			{
				HistoryHide(downloadQueue, historyInfo, index);
				index++;
			}
			else
			{
				if (action == 2)
				{
					DeleteDiskFiles(historyInfo->GetNzbInfo());
				}
				info("Collection %s removed from history", historyInfo->GetName());

				downloadQueue->GetHistory()->erase(downloadQueue->GetHistory()->end() - 1 - index);
			}

			it = downloadQueue->GetHistory()->rbegin() + index;
			changed = true;
		}
		else
		{
			++it;
			index++;
		}
	}

	if (changed)
	{
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

inline void HistoryCoordinator::DeleteDiskFiles(NzbInfo* nzbInfo)
{
	if (g_Options->GetServerMode())
	{
		// delete parked files
		g_DiskState->DiscardFiles(nzbInfo);
	}
	nzbInfo->GetFileList()->clear();

	// delete nzb-file
	if (!g_Options->GetNzbCleanupDisk())
	{
		return;
	}

	// QueuedFile may contain one filename or several filenames separated
	// with "|"-character (for merged groups)
	const char* filename = nzbInfo->GetQueuedFilename() ? nzbInfo->GetQueuedFilename() : "";
	Tokenizer tok(filename, "|");
	while (const char* name1 = tok.Next())
	{
		if (FileSystem::FileExists(name1))
		{
			info("Deleting file %s", name1);
			FileSystem::DeleteFile(name1);
		}
	}
}

inline void HistoryCoordinator::AddToHistory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	// catch-all for every path into history: a still-attached live
	// stream-repair thread (option <DupeArticleFallback> value "live") could
	// never find this collection by id again (the pass searches the QUEUE),
	// so it would self-destruct and leave this slot a dangling pointer
	if (nzbInfo->GetLiveRepairThread())
	{
		Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
		nzbInfo->SetLiveRepairThread(nullptr);
		liveRepairThread->Stop();
	}

	std::unique_ptr<NzbInfo> oldNzbInfo = downloadQueue->GetQueue()->Remove(nzbInfo);
	std::unique_ptr<HistoryInfo> historyInfo = std::make_unique<HistoryInfo>(std::move(oldNzbInfo));
	historyInfo->SetTime(Util::CurrentTime());
	downloadQueue->GetHistory()->Add(std::move(historyInfo), true);
	downloadQueue->HistoryChanged();

	// park remaining files
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		nzbInfo->UpdateCompletedStats(fileInfo);
		nzbInfo->GetCompletedFiles()->emplace_back(
			fileInfo->GetId(),
			fileInfo->GetFilename(),
			fileInfo->GetOrigname() ? fileInfo->GetOrigname() : "",
			CompletedFile::cfNone,
			0,
			fileInfo->GetParFile(),
			fileInfo->GetHash16k() ? fileInfo->GetHash16k() : "",
			fileInfo->GetParSetId() ? fileInfo->GetParSetId() : ""
		);
	}

	// Cleaning up parked files if par-check was successful or unpack was successful or
	// health is 100% (if unpack and par-check were not performed) or if deleted
	bool cleanupParkedFiles = nzbget_rs_qhp_should_cleanup_parked_files(
		(int32_t)nzbInfo->GetParStatus(),
		(int32_t)nzbInfo->GetUnpackStatus(),
		nzbInfo->GetFailedSize(),
		nzbInfo->GetParFailedSize(),
		(int32_t)nzbInfo->GetDeleteStatus(),
		nzbInfo->GetParking() ? 1 : 0,
		nzbInfo->GetUnpackCleanedUpDisk() ? 1 : 0
	) != 0;

	if (cleanupParkedFiles)
	{
		g_DiskState->DiscardFiles(nzbInfo, false);
		nzbInfo->GetCompletedFiles()->clear();
	}

	nzbInfo->SetParkedFileCount(0);
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		bool isLast = &completedFile == &*nzbInfo->GetCompletedFiles()->rbegin();
		if (nzbget_rs_history_coord_should_park_completed_file(
			(int)completedFile.GetStatus(),
			(int)CompletedFile::cfNone,
			(int)CompletedFile::cfPartial,
			isLast ? 1 : 0) != 0)
		{
			nzbInfo->PrintMessage(Message::mkDetail, "Parking file %s", completedFile.GetFilename());
			nzbInfo->SetParkedFileCount(nzbInfo->GetParkedFileCount() + 1);
		}
	}

	nzbInfo->GetFileList()->clear();
	nzbInfo->SetRemainingParCount(0);
	nzbInfo->SetParking(false);

	if (nzbInfo->GetDirectRenameStatus() == NzbInfo::tsRunning)
	{
		nzbInfo->SetDirectRenameStatus(NzbInfo::tsFailure);
	}
	if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsRunning)
	{
		nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
	}

	nzbInfo->SetDupeHint(NzbInfo::dhNone);

	nzbInfo->PrintMessage(Message::mkInfo, "Collection %s added to history", nzbInfo->GetName());

	RecheckFailedArticles(downloadQueue, nzbInfo);
	ParDamage::Forget(nzbInfo->GetId());
}

/*
 * A download that failed with articles missing gets its failed articles asked of
 * the servers once (DupeProbe::StartRecheck): when most of them exist after all,
 * a timeout or a dropped connection lost them, and they are retried. Only where
 * duplicate handling judges downloads by their article results (option
 * <DupeArticleFallback>, HealthCheck=dupe), only from files whose state was kept,
 * and not while a duplicate downloads in its place.
 */
inline void HistoryCoordinator::RecheckFailedArticles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (nzbget_rs_history_coord_should_recheck_failed_articles(
		g_Options->GetDupeArticleFallback() == Options::dafNone ? 1 : 0,
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0,
		nzbInfo->GetKind() == NzbInfo::nkNzb ? 1 : 0,
		nzbInfo->GetFailedArticles(),
		nzbInfo->MakeTextStatus(true),
		nzbInfo->GetParameters()->Find("*DupeRecheck") != nullptr ? 1 : 0) == 0)
	{
		return;
	}
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (DupeCoordinator::SameNameOrKey(queued->GetName(), queued->GetDupeKey(),
			nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			return;	// a duplicate downloads in its place
		}
	}
	// a duplicate waits in history: nzbget fetches it in its place (B39)
	if (g_Options->GetDupeCheck() &&
		nzbInfo->GetDupeMode() == dmScore &&
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()))
	{
		return;
	}
	// once per download
	nzbInfo->GetParameters()->SetParameter("*DupeRecheck", "1");

	std::vector<DupeProbe::Sample> failed;
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if ((completedFile.GetStatus() != CompletedFile::cfPartial &&
			 completedFile.GetStatus() != CompletedFile::cfFailure) ||
			completedFile.GetId() <= 0)
		{
			continue;
		}
		bool partial = completedFile.GetStatus() == CompletedFile::cfPartial;
		FileInfo fileInfo(completedFile.GetId());
		if (!g_DiskState->LoadFile(&fileInfo, false, true) ||
			(partial && !g_DiskState->LoadFileState(&fileInfo, g_ServerPool->GetServers(), true)))
		{
			continue;
		}
		auto groups = std::make_shared<std::vector<CString>>();
		for (const CString& group : *fileInfo.GetGroups())
		{
			groups->emplace_back(*group);
		}
		for (ArticleInfo* article : fileInfo.GetArticles())
		{
			if ((!partial || article->GetStatus() == ArticleInfo::aiFailed) &&
				!Util::EmptyStr(article->GetMessageId()))
			{
				DupeProbe::Sample sample;
				sample.MessageId = article->GetMessageId();
				sample.Groups = groups;
				failed.push_back(std::move(sample));
			}
		}
	}
	if (failed.size() < 2)
	{
		return;
	}

	std::vector<DupeProbe::Sample> samples;
	for (size_t index : DupeProbe::SampleIndexes(failed.size(), DupeProbe::RecheckSampleCount))
	{
		samples.push_back(std::move(failed[index]));	// the indexes are distinct
	}
	DupeProbe::StartRecheck(nzbInfo->GetId(), std::move(samples));
}

inline void HistoryCoordinator::HistoryHide(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, int rindex)
{
	// replace history element
	std::unique_ptr<DupInfo> dupInfo = std::make_unique<DupInfo>();
	dupInfo->SetId(historyInfo->GetNzbInfo()->GetId());
	dupInfo->SetName(historyInfo->GetNzbInfo()->GetName());
	dupInfo->SetDupeKey(historyInfo->GetNzbInfo()->GetDupeKey());
	dupInfo->SetDupeScore(historyInfo->GetNzbInfo()->GetDupeScore());
	dupInfo->SetDupeMode(historyInfo->GetNzbInfo()->GetDupeMode());
	dupInfo->SetSize(historyInfo->GetNzbInfo()->GetSize());
	dupInfo->SetFullContentHash(historyInfo->GetNzbInfo()->GetFullContentHash());
	dupInfo->SetFilteredContentHash(historyInfo->GetNzbInfo()->GetFilteredContentHash());

	dupInfo->SetStatus((DupInfo::EStatus)nzbget_rs_qhp_classify_dup_status(
		(int32_t)historyInfo->GetNzbInfo()->GetMarkStatus(),
		(int32_t)historyInfo->GetNzbInfo()->GetDeleteStatus(),
		historyInfo->GetNzbInfo()->IsDupeSuccess() ? 1 : 0
	));

	std::unique_ptr<HistoryInfo> newHistoryInfo = std::make_unique<HistoryInfo>(std::move(dupInfo));
	newHistoryInfo->SetTime(historyInfo->GetTime());

	DeleteDiskFiles(historyInfo->GetNzbInfo());

	info("Collection %s removed from history", historyInfo->GetName());

	(*downloadQueue->GetHistory())[downloadQueue->GetHistory()->size() - 1 - rindex] = std::move(newHistoryInfo);
}

inline void HistoryCoordinator::PrepareEdit(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action)
{
	// First pass: when marking multiple items - mark them bad without performing the mark-logic,
	// this will later (on second step) avoid moving other items to download queue, if they are marked bad too.
	for (int id : *idList)
	{
		HistoryInfo* historyInfo = downloadQueue->GetHistory()->Find(id);
		if (action == DownloadQueue::eaHistoryMarkBad &&
			historyInfo && historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			historyInfo->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
		}
	}
}

inline bool HistoryCoordinator::EditList(DownloadQueue* downloadQueue, IdList* idList,
	DownloadQueue::EEditAction action, const char* args)
{
	bool ok = false;
	PrepareEdit(downloadQueue, idList, action);

	for (int id : *idList)
	{
		for (HistoryList::iterator itHistory = downloadQueue->GetHistory()->begin(); itHistory != downloadQueue->GetHistory()->end(); ++itHistory)
		{
			HistoryInfo* historyInfo = (*itHistory).get();
			if (historyInfo->GetId() == id)
			{
				// one item's edit failing doesn't undo (or leave unsaved) the
				// others: the list counts as edited when any item was
				bool done = true;

				switch (action)
				{
					case DownloadQueue::eaHistoryDelete:
					case DownloadQueue::eaHistoryFinalDelete:
						HistoryDelete(downloadQueue, itHistory, historyInfo, action == DownloadQueue::eaHistoryFinalDelete);
						break;

					case DownloadQueue::eaHistoryReturn:
						HistoryReturn(downloadQueue, itHistory, historyInfo);
						break;

					case DownloadQueue::eaHistoryProcess:
						HistoryProcess(downloadQueue, itHistory, historyInfo);
						break;

					case DownloadQueue::eaHistoryRedownload:
						HistoryRedownload(downloadQueue, itHistory, historyInfo, false);
						break;

					case DownloadQueue::eaHistoryRetryFailed:
						HistoryRetry(downloadQueue, itHistory, historyInfo, true, false);
						break;

					case DownloadQueue::eaHistorySetParameter:
						done = HistorySetParameter(historyInfo, args);
						break;

 					case DownloadQueue::eaHistorySetCategory:
						done = HistorySetCategory(historyInfo, args);
						break;

 					case DownloadQueue::eaHistorySetName:
						done = HistorySetName(historyInfo, args);
						break;

					case DownloadQueue::eaHistorySetDupeKey:
					case DownloadQueue::eaHistorySetDupeScore:
					case DownloadQueue::eaHistorySetDupeMode:
					case DownloadQueue::eaHistorySetDupeBackup:
						HistorySetDupeParam(historyInfo, action, args);
						break;

					case DownloadQueue::eaHistoryMarkBad:
						g_DupeCoordinator->HistoryMark(downloadQueue, historyInfo, NzbInfo::ksBad);
						break;

					case DownloadQueue::eaHistoryMarkGood:
						g_DupeCoordinator->HistoryMark(downloadQueue, historyInfo, NzbInfo::ksGood);
						break;

					case DownloadQueue::eaHistoryMarkSuccess:
						g_DupeCoordinator->HistoryMark(downloadQueue, historyInfo, NzbInfo::ksSuccess);
						break;

					default:
						// nothing, just to avoid compiler warning
						break;
				}

				ok = ok || done;
				break;
			}
		}
	}

	if (ok)
	{
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}

	return ok;
}

inline void HistoryCoordinator::HistoryDelete(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool final)
{
	info("Deleting %s from history", historyInfo->GetName());

	if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		DeleteDiskFiles(historyInfo->GetNzbInfo());
	}

	bool isNzb = historyInfo->GetKind() == HistoryInfo::hkNzb;
	if (isNzb && nzbget_rs_history_coord_should_delete_dest_dir(
		1,
		(int)historyInfo->GetNzbInfo()->GetDeleteStatus(), (int)NzbInfo::dsNone,
		(int)historyInfo->GetNzbInfo()->GetParStatus(), (int)NzbInfo::psFailure,
		(int)historyInfo->GetNzbInfo()->GetUnpackStatus(), (int)NzbInfo::usFailure,
		(int)NzbInfo::usPassword,
		FileSystem::DirectoryExists(historyInfo->GetNzbInfo()->GetDestDir()) ? 1 : 0) != 0)
	{
		info("Deleting %s", historyInfo->GetNzbInfo()->GetDestDir());
		CString errmsg;
		if (!FileSystem::DeleteDirectoryWithContent(historyInfo->GetNzbInfo()->GetDestDir(), errmsg))
		{
			error("Could not delete directory %s: %s", historyInfo->GetNzbInfo()->GetDestDir(), *errmsg);
		}
	}

	if (final || !g_Options->GetDupeCheck() || historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		downloadQueue->GetHistory()->erase(itHistory);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		// replace history element
		int rindex = (int)downloadQueue->GetHistory()->size() - 1 - (int)(itHistory - downloadQueue->GetHistory()->begin());
		HistoryHide(downloadQueue, historyInfo, rindex);
	}
}

inline void HistoryCoordinator::MoveToQueue(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool reprocess)
{
	debug("Returning %s from history back to download queue", historyInfo->GetName());
	CString nicename = historyInfo->GetName();
	NzbInfo* nzbInfo = historyInfo->GetNzbInfo();

	// unpark files
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		nzbInfo->PrintMessage(Message::mkDetail, "Unparking file %s", fileInfo->GetFilename());
	}

	downloadQueue->GetQueue()->Add(std::unique_ptr<NzbInfo>(nzbInfo), true);
	historyInfo->DiscardNzbInfo();

	// reset postprocessing status variables
	bool hasMainPars = !nzbInfo->GetUnpackCleanedUpDisk() &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr);
	int newPostTotalSec = nzbInfo->GetPostTotalSec();
	int newMoveStatus = (int)nzbInfo->GetMoveStatus();
	nzbget_rs_history_coord_calc_move_to_queue_reset(
		nzbInfo->GetUnpackCleanedUpDisk() ? 1 : 0,
		nzbInfo->GetPostTotalSec(),
		nzbInfo->GetUnpackSec(),
		hasMainPars ? 1 : 0,
		nzbInfo->GetParSec(),
		(int)nzbInfo->GetMoveStatus(),
		(int)NzbInfo::msFailure,
		(int)NzbInfo::msNone,
		&newPostTotalSec,
		&newMoveStatus);
	if (!nzbInfo->GetUnpackCleanedUpDisk())
	{
		nzbInfo->SetUnpackStatus(NzbInfo::usNone);
		nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
		nzbInfo->SetParRepaired(false);
		nzbInfo->SetUnpackTried(false);
		nzbInfo->SetLastUnpackStatus(0);
		nzbInfo->GetExtractedArchives()->clear();
		nzbInfo->SetCleanupStatus(NzbInfo::csNone);
		nzbInfo->SetParRenameStatus(NzbInfo::rsNone);
		nzbInfo->SetRarRenameStatus(NzbInfo::rsNone);
		nzbInfo->SetPostTotalSec(newPostTotalSec);
		nzbInfo->SetUnpackSec(0);

		if (hasMainPars)
		{
			nzbInfo->SetParStatus(NzbInfo::psNone);
			nzbInfo->SetParSec(0);
			nzbInfo->SetRepairSec(0);
			nzbInfo->SetParFull(false);
		}
	}
	nzbInfo->SetDeleteStatus(NzbInfo::dsNone);
	nzbInfo->SetDeletePaused(false);
	nzbInfo->SetDupeFailoverChecked(-1);
	nzbInfo->SetDeadPickProbed(false);
	nzbInfo->SetDupeAttemptedArticles(0);
	nzbInfo->SetDupeUnsourcedArticles(0);
	nzbInfo->SetDupeFailedRun(0);
	nzbInfo->SetDupeParDeferState(NzbInfo::dpNone);
	nzbInfo->SetMarkStatus(NzbInfo::ksNone);
	nzbInfo->GetScriptStatuses()->clear();
	nzbInfo->SetParkedFileCount(0);
	nzbInfo->SetMoveStatus((NzbInfo::EMoveStatus)newMoveStatus);
	nzbInfo->SetReprocess(reprocess);
	nzbInfo->SetFinalDir("");

	downloadQueue->GetHistory()->erase(itHistory);
	// the object "pHistoryInfo" is released few lines later, after the call to "NZBDownloaded"
	nzbInfo->PrintMessage(Message::mkInfo, "%s returned from history back to download queue", *nicename);

	if (reprocess)
	{
		// start postprocessing
		debug("Restarting postprocessing for %s", *nicename);
		g_PrePostProcessor->NzbDownloaded(downloadQueue, nzbInfo);
		DownloadQueue::Aspect aspect = {DownloadQueue::eaNzbReturned, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);
	}
}

inline void HistoryCoordinator::HistoryRedownload(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool restorePauseState)
{
	if (historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		historyInfo->DiscardNzbInfo();
		nzbInfo->SetUrlStatus(NzbInfo::lsNone);
		nzbInfo->SetDeleteStatus(NzbInfo::dsNone);
		if (nzbInfo->GetDupeHint() == NzbInfo::dhNone)
		{
			nzbInfo->SetDupeHint(NzbInfo::dhRedownloadManual);
		}
		downloadQueue->GetQueue()->Add(std::unique_ptr<NzbInfo>(nzbInfo), true);
		downloadQueue->GetHistory()->erase(itHistory);

		DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlReturned, downloadQueue, nzbInfo, nullptr};
		downloadQueue->Notify(&aspect);

		return;
	}

	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not download again %s: history item has wrong type", historyInfo->GetName());
		return;
	}

	NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
	bool paused = restorePauseState && nzbInfo->GetDeletePaused();

	if (!FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
	{
		error("Could not download again %s: could not find source nzb-file %s",
			nzbInfo->GetName(), nzbInfo->GetQueuedFilename());
		return;
	}

	NzbFile nzbFile(nzbInfo->GetQueuedFilename(), "");
	if (!nzbFile.Parse())
	{
		error("Could not download again %s: could not parse nzb-file", nzbInfo->GetName());
		return;
	}

	info("Downloading again %s", nzbInfo->GetName());

	std::unique_ptr<NzbInfo> newNzbInfo = nzbFile.DetachNzbInfo();

	for (FileInfo* fileInfo : newNzbInfo->GetFileList())
	{
		fileInfo->SetPaused(paused);
	}

	if (FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
	{
		detail("Deleting %s", nzbInfo->GetDestDir());
		CString errmsg;
		if (!FileSystem::DeleteDirectoryWithContent(nzbInfo->GetDestDir(), errmsg))
		{
			error("Could not delete directory %s: %s", nzbInfo->GetDestDir(), *errmsg);
		}
	}

	nzbInfo->BuildDestDirName();
	if (FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
	{
		detail("Deleting %s", nzbInfo->GetDestDir());
		CString errmsg;
		if (!FileSystem::DeleteDirectoryWithContent(nzbInfo->GetDestDir(), errmsg))
		{
			error("Could not delete directory %s: %s", nzbInfo->GetDestDir(), *errmsg);
		}
	}

	g_DiskState->DiscardFiles(nzbInfo);

	// reset status fields (which are not reset by "MoveToQueue")
	nzbInfo->SetMoveStatus(NzbInfo::msNone);
	nzbInfo->SetUnpackCleanedUpDisk(false);
	nzbInfo->SetParStatus(NzbInfo::psNone);
	nzbInfo->SetParRenameStatus(NzbInfo::rsNone);
	nzbInfo->SetRarRenameStatus(NzbInfo::rsNone);
	nzbInfo->SetDirectRenameStatus(NzbInfo::tsNone);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
	nzbInfo->SetParRepaired(false);
	nzbInfo->SetUnpackTried(false);
	nzbInfo->SetLastUnpackStatus(0);
	nzbInfo->GetExtractedArchives()->clear();
	nzbInfo->SetHealthPaused(false);
	nzbInfo->SetDownloadedSize(0);
	nzbInfo->SetDownloadSec(0);
	nzbInfo->SetPostTotalSec(0);
	nzbInfo->SetParSec(0);
	nzbInfo->SetRepairSec(0);
	nzbInfo->SetUnpackSec(0);
	nzbInfo->SetExtraParBlocks(0);
	nzbInfo->SetAllFirst(false);
	nzbInfo->SetWaitingPar(false);
	nzbInfo->SetLoadingPar(false);
	nzbInfo->GetCompletedFiles()->clear();
	// stream-repair jobs captured for the previous download attempt are stale
	nzbInfo->GetStreamRepairJobs()->clear();
	// and so are its duplicate recoveries: the failover gates weigh them against
	// this attempt's article counts, which start at 0 - a stale count made a
	// download that arrives look dead (failed over to the next backup), or a dead
	// one look borrowed enough to be spared. Blocked donors stay blocked: they
	// were proven to hold other bytes
	nzbInfo->SetDupeRecoveredArticles(0);
	nzbInfo->SetDupeRecoveredBytes(0);
	nzbInfo->SetDupeRecoveredHoles(0);
	nzbInfo->SetDupeParDeferState(NzbInfo::dpNone);
	nzbInfo->SetDupeNoParNoted(false);
	nzbInfo->GetServerStats()->clear();
	nzbInfo->GetCurrentServerStats()->clear();

	nzbInfo->MoveFileList(newNzbInfo.get());

	g_QueueCoordinator->CheckDupeFileInfos(nzbInfo);

	MoveToQueue(downloadQueue, itHistory, historyInfo, false);

	g_PrePostProcessor->NzbAdded(downloadQueue, nzbInfo);

	DownloadQueue::Aspect aspect = {DownloadQueue::eaNzbReturned, downloadQueue, nzbInfo, nullptr};
	downloadQueue->Notify(&aspect);
}

inline void HistoryCoordinator::HistoryReturn(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo)
{
	if (historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		HistoryRedownload(downloadQueue, itHistory, historyInfo, false);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetParkedFileCount() == 0)
	{
		warn("Could not download remaining files for %s: history item does not have any files left for download", historyInfo->GetName());
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		HistoryRetry(downloadQueue, itHistory, historyInfo, false, false);
	}
	else
	{
		error("Could not download remaining files for %s: history item has wrong type", historyInfo->GetName());
	}
}

inline void HistoryCoordinator::HistoryProcess(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo)
{
	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not post-process again %s: history item has wrong type", historyInfo->GetName());
		return;
	}

	HistoryRetry(downloadQueue, itHistory, historyInfo, false, true);
}

inline void HistoryCoordinator::HistoryRetry(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool resetFailed, bool reprocess)
{
	const char* wrongTypeVerb = nullptr;
	const char* missingDirVerb = nullptr;
	const char* startMsgVerb = nullptr;
	nzbget_rs_history_coord_retry_action_labels(
		resetFailed ? 1 : 0, reprocess ? 1 : 0,
		&wrongTypeVerb, &missingDirVerb, &startMsgVerb);

	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not %s for %s: history item has wrong type",
			wrongTypeVerb,
			historyInfo->GetName());
		return;
	}

	NzbInfo* nzbInfo = historyInfo->GetNzbInfo();

	if (!FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
	{
		error("Could not %s %s: destination directory %s doesn't exist",
			missingDirVerb,
			historyInfo->GetName(), nzbInfo->GetDestDir());
		return;
	}

	nzbInfo->PrintMessage(Message::mkInfo, "%s %s",
		startMsgVerb,
		nzbInfo->GetName());

	// the files that download again: their repair jobs describe the old attempt
	std::set<int> requeued;

	// move failed completed files to (parked) file list
	for (CompletedFileList::iterator it = nzbInfo->GetCompletedFiles()->begin(); it != nzbInfo->GetCompletedFiles()->end(); )
	{
		CompletedFile& completedFile = *it;
		if (completedFile.GetStatus() != CompletedFile::cfSuccess &&
			(completedFile.GetStatus() != CompletedFile::cfFailure || resetFailed) &&
			completedFile.GetId() > 0)
		{
			std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>();
			fileInfo->SetId(completedFile.GetId());
			bool fileLoaded = g_DiskState->LoadFile(fileInfo.get(), true, true);
			bool stateLoaded = fileLoaded &&
				completedFile.GetStatus() == CompletedFile::cfPartial &&
				g_DiskState->LoadFileState(fileInfo.get(), g_ServerPool->GetServers(), true);
			if (fileLoaded &&
				nzbget_rs_history_coord_eval_requeue_loaded_file(
					(int)completedFile.GetStatus(),
					(int)CompletedFile::cfNone,
					(int)CompletedFile::cfFailure,
					(int)CompletedFile::cfPartial,
					resetFailed ? 1 : 0,
					stateLoaded ? 1 : 0,
					fileInfo->GetRemainingSize()) != 0)
			{
				// a file parked while it downloaded is recorded by its temporary output
				// name ("<id>.out.tmp"): its data is there, but it keeps the name its
				// nzb-file gives it, which a recreation from a duplicate pairs by
				std::string recordedName = completedFile.GetFilename();
				ArticleList* articles = fileInfo->GetArticles();
				std::vector<int> artStatuses;
				artStatuses.reserve(articles->size());
				for (const std::unique_ptr<ArticleInfo>& article : *articles)
				{
					artStatuses.push_back((int)article->GetStatus());
				}

				auto fileSizeCb = [](void*, const char* path) -> int64_t {
					return FileSystem::FileSize(path);
				};
				auto deleteFileCb = [](void*, const char* path) {
					FileSystem::DeleteFile(path);
				};
				auto fileExistsCb = [](void*, const char* path) -> int {
					return FileSystem::FileExists(path) ? 1 : 0;
				};

				int setRecordedFilename = 0;
				int newPartialState = (int)fileInfo->GetPartialState();
				int warnMissingOnDisk = 0;
				int noRetryableArticles = 0;
				nzbget_rs_history_coord_prepare_requeued_loaded_file(
					nzbInfo->GetDestDir(),
					PATH_SEPARATOR,
					recordedName.c_str(),
					fileInfo->GetFilename() ? fileInfo->GetFilename() : "",
					fileInfo->GetSuccessArticles(),
					(int)fileInfo->GetPartialState(),
					(int)FileInfo::psNone,
					(int)FileInfo::psCompleted,
					reprocess ? 1 : 0,
					resetFailed ? 1 : 0,
					artStatuses.data(),
					artStatuses.size(),
					(int)ArticleInfo::aiUndefined,
					(int)ArticleInfo::aiFailed,
					nullptr,
					fileSizeCb,
					deleteFileCb,
					fileExistsCb,
					&setRecordedFilename,
					&newPartialState,
					&warnMissingOnDisk,
					&noRetryableArticles);

				if (setRecordedFilename)
				{
					fileInfo->SetFilename(recordedName.c_str());
				}
				fileInfo->SetNzbInfo(nzbInfo);
				if (warnMissingOnDisk)
				{
					nzbInfo->PrintMessage(Message::mkWarning, "File %s could not be found on disk, downloading again", fileInfo->GetFilename());
				}
				fileInfo->SetPartialState((FileInfo::EPartialState)newPartialState);

				// A partial file may contain only entries missing from the NZB.
				// They have no message IDs to retry. Keep its completed record and
				// statistics intact instead of queueing an all-finished article list.
				if (noRetryableArticles)
				{
					++it;
					continue;
				}

				ResetArticles(fileInfo.get(), completedFile.GetStatus() == CompletedFile::cfFailure, resetFailed);

				g_DiskState->DiscardFile(fileInfo->GetId(), false, true, fileInfo->GetPartialState() != FileInfo::psCompleted);
				if (fileInfo->GetPartialState() == FileInfo::psCompleted)
				{
					g_DiskState->SaveFileState(fileInfo.get(), true);
				}
				fileInfo->GetArticles()->clear();

				requeued.insert(fileInfo->GetId());
				nzbInfo->GetFileList()->Add(std::move(fileInfo), false);

				it = nzbInfo->GetCompletedFiles()->erase(it);
				continue;
			}
		}
		++it;
	}

	// the repair jobs of files that download again are stale (a new one is built
	// when the file completes); the others keep their holes for post-processing:
	// a finished file that isn't downloaded again would otherwise never be repaired
	StreamRepairJobList* jobs = nzbInfo->GetStreamRepairJobs();
	jobs->erase(std::remove_if(jobs->begin(), jobs->end(),
		[&requeued](const StreamRepairJob& job) { return requeued.count(job.GetFileId()) > 0; }), jobs->end());

	nzbInfo->UpdateCurrentStats();
	if (!resetFailed && !reprocess)
	{
		// The user explicitly requested the remaining files; do not park the
		// download again because of the health failure being overridden.
		nzbInfo->SetHealthPaused(true);
	}

	// With no download work left there will be no article-completion event to
	// start post-processing. Paused spare PAR files do not block that transition.
	MoveToQueue(downloadQueue, itHistory, historyInfo, reprocess || nzbInfo->IsDownloadCompleted(true));

	if (g_Options->GetParCheck() != Options::pcForce)
	{
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
	}
}

inline void HistoryCoordinator::ResetArticles(FileInfo* fileInfo, bool allFailed, bool resetFailed)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	ArticleList* articles = fileInfo->GetArticles();
	std::vector<int> artStatuses;
	std::vector<int> artSizes;
	std::vector<uint8_t> resetFlags(articles->size(), 0);
	artStatuses.reserve(articles->size());
	artSizes.reserve(articles->size());
	for (ArticleInfo* pa : articles)
	{
		artStatuses.push_back((int)pa->GetStatus());
		artSizes.push_back(pa->GetSize());
	}

	int64 fileFailedSize = fileInfo->GetFailedSize();
	int fileFailedArticles = fileInfo->GetFailedArticles();
	int64 fileRemainingSize = fileInfo->GetRemainingSize();
	int fileCompletedArticles = fileInfo->GetCompletedArticles();
	int64 fileSuccessSize = fileInfo->GetSuccessSize();
	int fileSuccessArticles = fileInfo->GetSuccessArticles();
	int64 nzbFailedSize = nzbInfo->GetFailedSize();
	int64 nzbSuccessSize = nzbInfo->GetSuccessSize();
	int nzbFailedArticles = nzbInfo->GetFailedArticles();
	int nzbSuccessArticles = nzbInfo->GetSuccessArticles();
	int64 nzbParFailedSize = nzbInfo->GetParFailedSize();
	int64 nzbParSuccessSize = nzbInfo->GetParSuccessSize();

	nzbget_rs_history_coord_reset_articles_batch(
		allFailed ? 1 : 0,
		resetFailed ? 1 : 0,
		fileInfo->GetParFile() ? 1 : 0,
		(int)fileInfo->GetPartialState(),
		fileInfo->GetSize(),
		fileInfo->GetMissedSize(),
		fileInfo->GetTotalArticles(),
		fileInfo->GetMissedArticles(),
		&fileFailedSize,
		&fileFailedArticles,
		&fileRemainingSize,
		&fileCompletedArticles,
		&fileSuccessSize,
		&fileSuccessArticles,
		&nzbFailedSize,
		&nzbSuccessSize,
		&nzbFailedArticles,
		&nzbSuccessArticles,
		&nzbParFailedSize,
		&nzbParSuccessSize,
		artStatuses.data(),
		artSizes.data(),
		resetFlags.data(),
		articles->size());

	nzbInfo->GetServerStats()->ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
	nzbInfo->SetFailedSize(nzbFailedSize);
	nzbInfo->SetSuccessSize(nzbSuccessSize);
	nzbInfo->SetFailedArticles(nzbFailedArticles);
	nzbInfo->SetSuccessArticles(nzbSuccessArticles);
	if (fileInfo->GetParFile())
	{
		nzbInfo->SetParFailedSize(nzbParFailedSize);
		nzbInfo->SetParSuccessSize(nzbParSuccessSize);
	}
	fileInfo->SetFailedSize(fileFailedSize);
	fileInfo->SetFailedArticles(fileFailedArticles);
	fileInfo->SetRemainingSize(fileRemainingSize);
	fileInfo->SetCompletedArticles(fileCompletedArticles);
	fileInfo->SetSuccessSize(fileSuccessSize);
	fileInfo->SetSuccessArticles(fileSuccessArticles);

	for (size_t i = 0; i < articles->size(); ++i)
	{
		if (resetFlags[i])
		{
			ArticleInfo* pa = (*articles)[i].get();
			pa->SetStatus((ArticleInfo::EStatus)artStatuses[i]);
			pa->SetCrc(0);
			pa->SetSegmentOffset(0);
			pa->SetSegmentSize(0);
			pa->SetDupeExpectedOffset(-1);
			pa->SetDupeExpectedEnd(-1);
		}
	}
}

inline bool HistoryCoordinator::HistorySetParameter(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting post-process-parameter '%s' for '%s'", text, historyInfo->GetName());

	if (historyInfo->GetKind() != HistoryInfo::hkNzb && historyInfo->GetKind() != HistoryInfo::hkUrl)
	{
		error("Could not set post-process-parameter for %s: history item has wrong type", historyInfo->GetName());
		return false;
	}

	const char* eq = text ? strchr(text, '=') : nullptr;
	if (eq)
	{
		std::string name(text, (size_t)(eq - text));
		historyInfo->GetNzbInfo()->GetParameters()->SetParameter(name.c_str(), eq + 1);
	}
	else
	{
		error("Could not set post-process-parameter for %s: invalid argument: %s", historyInfo->GetNzbInfo()->GetName(), text);
	}

	return true;
}

inline bool HistoryCoordinator::HistorySetCategory(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting category '%s' for '%s'", text, historyInfo->GetName());

	if (historyInfo->GetKind() != HistoryInfo::hkNzb && historyInfo->GetKind() != HistoryInfo::hkUrl)
	{
		error("Could not set category for %s: history item has wrong type", historyInfo->GetName());
		return false;
	}

	historyInfo->GetNzbInfo()->SetCategory(text);

	return true;
}

inline bool HistoryCoordinator::HistorySetName(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting name '%s' for '%s'", text, historyInfo->GetName());

	if (Util::EmptyStr(text))
	{
		error("Could not rename %s. The new name cannot be empty", historyInfo->GetName());
		return false;
	}

	if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		std::string previous = historyInfo->GetNzbInfo()->GetName();
		historyInfo->GetNzbInfo()->SetName(text);
		if (historyInfo->GetKind() == HistoryInfo::hkNzb && g_DupeSearch && previous != historyInfo->GetNzbInfo()->GetName())
            g_DupeSearch->NotifyNameChanged(historyInfo->GetNzbInfo()->GetId());
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		historyInfo->GetDupInfo()->SetName(text);
	}

	return true;
}

inline void HistoryCoordinator::HistorySetDupeParam(HistoryInfo* historyInfo, DownloadQueue::EEditAction action, const char* text)
{
	debug("Setting dupe-parameter '%i'='%s' for '%s'", (int)action, text, historyInfo->GetName());

	EDupeMode mode = dmScore;
	if (action == DownloadQueue::eaHistorySetDupeMode)
	{
		int parsedMode = 0;
		if (!nzbget_rs_qhp_parse_dupe_mode(text, &parsedMode))
		{
			error("Could not set duplicate mode for %s: incorrect mode (%s)", historyInfo->GetName(), text);
			return;
		}
		mode = (EDupeMode)parsedMode;
	}

	if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		switch (action)
		{
			case DownloadQueue::eaHistorySetDupeKey:
                historyInfo->GetNzbInfo()->GetParameters()->SetParameter(DupeSearch::OwnedKeyParam, nullptr);
				historyInfo->GetNzbInfo()->SetDupeKey(text);
				break;

			case DownloadQueue::eaHistorySetDupeScore:
				historyInfo->GetNzbInfo()->SetDupeScore(text ? atoi(text) : 0);
				break;

			case DownloadQueue::eaHistorySetDupeMode:
				historyInfo->GetNzbInfo()->SetDupeMode(mode);
				break;

			case DownloadQueue::eaHistorySetDupeBackup:
				if (historyInfo->GetKind() == HistoryInfo::hkUrl)
				{
					error("Could not set duplicate parameter for %s: history item has wrong type", historyInfo->GetName());
					return;
				}
				else if (historyInfo->GetNzbInfo()->GetDeleteStatus() != NzbInfo::dsDupe &&
					historyInfo->GetNzbInfo()->GetDeleteStatus() != NzbInfo::dsManual)
				{
					error("Could not set duplicate parameter for %s: history item has wrong delete status", historyInfo->GetName());
					return;
				}
				historyInfo->GetNzbInfo()->SetDeleteStatus(
					(NzbInfo::EDeleteStatus)nzbget_rs_history_coord_parse_dupe_backup_flag(
						text, (int)NzbInfo::dsDupe, (int)NzbInfo::dsManual));
				break;

			default:
				// suppress compiler warning
				break;
		}
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		switch (action)
		{
			case DownloadQueue::eaHistorySetDupeKey:
				historyInfo->GetDupInfo()->SetDupeKey(text);
				break;

			case DownloadQueue::eaHistorySetDupeScore:
				historyInfo->GetDupInfo()->SetDupeScore(text ? atoi(text) : 0);
				break;

			case DownloadQueue::eaHistorySetDupeMode:
				historyInfo->GetDupInfo()->SetDupeMode(mode);
				break;

			case DownloadQueue::eaHistorySetDupeBackup:
				error("Could not set duplicate parameter for %s: history item has wrong type", historyInfo->GetName());
				return;

			default:
				// suppress compiler warning
				break;
		}
	}
}

inline void HistoryCoordinator::Redownload(DownloadQueue* downloadQueue, HistoryInfo* historyInfo)
{
	HistoryList::iterator it = downloadQueue->GetHistory()->Find(historyInfo);
	HistoryRedownload(downloadQueue, it, historyInfo, true);
}

#endif
