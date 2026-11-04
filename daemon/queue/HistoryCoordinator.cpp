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


#include "nzbget.h"

#include <set>
#include "HistoryCoordinator.h"
#include "Options.h"
#include "Log.h"
#include "QueueCoordinator.h"
#include "DiskState.h"
#include "Util.h"
#include "FileSystem.h"
#include "NzbFile.h"
#include "DupeCoordinator.h"
#include "ParParser.h"
#include "PrePostProcessor.h"
#include "DupeCoordinator.h"
#include "ServerPool.h"
#include "DupeProbe.h"
#include "ParDamage.h"
#include "DupeSearch.h"
#include "nzbget_rs.h"

/**
 * Removes old entries from (recent) history
 */
void HistoryCoordinator::ServiceWork()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	// in 64 bits: past 24,855 days the int product went negative, and every item was
	// hidden as too old
	time_t minTime = (time_t)nzbget_rs_pup_history_coord_calc_min_time(
		(int64)Util::CurrentTime(), g_Options->GetKeepHistory());
	bool changed = false;
	int index = 0;

	// traversing in a reverse order to delete items in order they were added to history
	// (just to produce the log-messages in a more logical order)
	for (HistoryList::reverse_iterator it = downloadQueue->GetHistory()->rbegin(); it != downloadQueue->GetHistory()->rend(); )
	{
		HistoryInfo* historyInfo = (*it).get();
		int action = nzbget_rs_hsr_history_coord_classify_service_work_item(
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

void HistoryCoordinator::DeleteDiskFiles(NzbInfo* nzbInfo)
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
	CString filename = nzbInfo->GetQueuedFilename();
	char* end = filename - 1;
	while (end)
	{
		char* name1 = end + 1;
		end = strchr(name1, '|');
		if (end) *end = '\0';

		if (FileSystem::FileExists(name1))
		{
			info("Deleting file %s", name1);
			FileSystem::DeleteFile(name1);
		}
	}
}

void HistoryCoordinator::AddToHistory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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
		if (nzbget_rs_pup_history_coord_should_count_parked_file(
			(int)completedFile.GetStatus(), (int)CompletedFile::cfNone, (int)CompletedFile::cfPartial,
			&completedFile == &*nzbInfo->GetCompletedFiles()->rbegin() ? 1 : 0) != 0)
		{
			nzbInfo->PrintMessage(Message::mkDetail, "Parking file %s", completedFile.GetFilename());
			nzbInfo->SetParkedFileCount(nzbInfo->GetParkedFileCount() + 1);
		}
	}

	nzbInfo->GetFileList()->clear();
	nzbInfo->SetRemainingParCount(0);
	nzbInfo->SetParking(false);

	{
		int newRename = (int)nzbInfo->GetDirectRenameStatus();
		int newUnpack = (int)nzbInfo->GetDirectUnpackStatus();
		nzbget_rs_hsr_history_coord_resolve_post_history_direct_statuses(
			newRename, (int)NzbInfo::tsRunning, (int)NzbInfo::tsFailure,
			newUnpack, (int)NzbInfo::nsRunning, (int)NzbInfo::nsNone,
			&newRename, &newUnpack);
		nzbInfo->SetDirectRenameStatus((NzbInfo::EDirectRenameStatus)newRename);
		nzbInfo->SetDirectUnpackStatus((NzbInfo::EDirectUnpackStatus)newUnpack);
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
void HistoryCoordinator::RecheckFailedArticles(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (nzbget_rs_pup_history_coord_should_recheck_failed_articles(
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
	if (nzbget_rs_pur_history_coord_should_skip_recheck_for_dupe_backup(
		g_Options->GetDupeCheck() ? 1 : 0,
		(int)nzbInfo->GetDupeMode(),
		(int)dmScore) != 0 &&
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()))
	{
		return;
	}
	// once per download
	nzbInfo->GetParameters()->SetParameter("*DupeRecheck", "1");

	std::vector<DupeProbe::Sample> failed;
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		int isPartialInt = 0;
		if (nzbget_rs_hsr_history_coord_should_sample_completed_file(
			(int)completedFile.GetStatus(),
			(int)CompletedFile::cfPartial,
			(int)CompletedFile::cfFailure,
			completedFile.GetId(),
			&isPartialInt) == 0)
		{
			continue;
		}
		bool partial = isPartialInt != 0;
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
			if (nzbget_rs_hsr_history_coord_should_sample_article(
				partial ? 1 : 0,
				(int)article->GetStatus(),
				(int)ArticleInfo::aiFailed,
				article->GetMessageId()) != 0)
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

void HistoryCoordinator::HistoryHide(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, int rindex)
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

void HistoryCoordinator::PrepareEdit(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action)
{
	// First pass: when marking multiple items - mark them bad without performing the mark-logic,
	// this will later (on second step) avoid moving other items to download queue, if they are marked bad too.
	if (action == DownloadQueue::eaHistoryMarkBad)
	{
		for (int id : *idList)
		{
			HistoryInfo* historyInfo = downloadQueue->GetHistory()->Find(id);
			if (historyInfo && historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				historyInfo->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
			}
		}
	}
}

bool HistoryCoordinator::EditList(DownloadQueue* downloadQueue, IdList* idList,
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

void HistoryCoordinator::HistoryDelete(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool final)
{
	info("Deleting %s from history", historyInfo->GetName());

	if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		DeleteDiskFiles(historyInfo->GetNzbInfo());
	}

	bool isNzb = historyInfo->GetKind() == HistoryInfo::hkNzb;
	if (isNzb && nzbget_rs_pup_history_coord_should_delete_dest_dir(
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

	if (nzbget_rs_pup_history_coord_should_erase_on_delete(
		final ? 1 : 0, g_Options->GetDupeCheck() ? 1 : 0,
		historyInfo->GetKind() == HistoryInfo::hkUrl ? 1 : 0) != 0)
	{
		downloadQueue->GetHistory()->erase(itHistory);
	}
	else
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			// replace history element
			int rindex = nzbget_rs_tpr_history_coord_reverse_delete_index(
				(int)downloadQueue->GetHistory()->size(),
				(int)(itHistory - downloadQueue->GetHistory()->begin()));
			HistoryHide(downloadQueue, historyInfo, rindex);
		}
	}
}

void HistoryCoordinator::MoveToQueue(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo, bool reprocess)
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
	nzbget_rs_hsr_history_coord_calc_move_to_queue_reset(
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

void HistoryCoordinator::HistoryRedownload(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool restorePauseState)
{
	if (historyInfo->GetKind() == HistoryInfo::hkUrl)
	{
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		historyInfo->DiscardNzbInfo();
		nzbInfo->SetUrlStatus(NzbInfo::lsNone);
		nzbInfo->SetDeleteStatus(NzbInfo::dsNone);
		nzbInfo->SetDupeHint((NzbInfo::EDupeHint)nzbget_rs_hsr_history_coord_resolve_url_redownload_dupe_hint(
			(int)nzbInfo->GetDupeHint(), (int)NzbInfo::dhNone, (int)NzbInfo::dhRedownloadManual));
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
	bool paused = nzbget_rs_tpr_history_coord_should_restore_paused_on_redownload(
		restorePauseState ? 1 : 0, nzbInfo->GetDeletePaused() ? 1 : 0) != 0;

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

void HistoryCoordinator::HistoryReturn(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo)
{
	int parkedCount = historyInfo->GetKind() == HistoryInfo::hkNzb ?
		historyInfo->GetNzbInfo()->GetParkedFileCount() : 0;
	int returnAction = nzbget_rs_hsr_history_coord_classify_return_action(
		(int)historyInfo->GetKind(), (int)HistoryInfo::hkUrl, (int)HistoryInfo::hkNzb, parkedCount);
	if (returnAction == 1)
	{
		HistoryRedownload(downloadQueue, itHistory, historyInfo, false);
	}
	else if (returnAction == 2)
	{
		warn("Could not download remaining files for %s: history item does not have any files left for download", historyInfo->GetName());
	}
	else if (returnAction == 3)
	{
		HistoryRetry(downloadQueue, itHistory, historyInfo, false, false);
	}
	else
	{
		error("Could not download remaining files for %s: history item has wrong type", historyInfo->GetName());
	}
}

void HistoryCoordinator::HistoryProcess(DownloadQueue* downloadQueue, HistoryList::iterator itHistory, HistoryInfo* historyInfo)
{
	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not post-process again %s: history item has wrong type", historyInfo->GetName());
		return;
	}

	HistoryRetry(downloadQueue, itHistory, historyInfo, false, true);
}

void HistoryCoordinator::HistoryRetry(DownloadQueue* downloadQueue, HistoryList::iterator itHistory,
	HistoryInfo* historyInfo, bool resetFailed, bool reprocess)
{
	if (historyInfo->GetKind() != HistoryInfo::hkNzb)
	{
		error("Could not %s for %s: history item has wrong type",
			(resetFailed ? "retry failed articles" : "download remaining files"),
			historyInfo->GetName());
		return;
	}

	NzbInfo* nzbInfo = historyInfo->GetNzbInfo();

	if (!FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
	{
		error("Could not %s %s: destination directory %s doesn't exist",
			(resetFailed ? "retry failed articles for" : reprocess ? "post-process again" : "download remaining files for"),
			historyInfo->GetName(), nzbInfo->GetDestDir());
		return;
	}

	nzbInfo->PrintMessage(Message::mkInfo, "%s %s",
		(resetFailed ? "Retrying failed articles for" : reprocess ? "Post-processing again" : "Downloading remaining files for"),
		nzbInfo->GetName());

	// the files that download again: their repair jobs describe the old attempt
	std::set<int> requeued;

	// move failed completed files to (parked) file list
	for (CompletedFileList::iterator it = nzbInfo->GetCompletedFiles()->begin(); it != nzbInfo->GetCompletedFiles()->end(); )
	{
		CompletedFile& completedFile = *it;
		if (nzbget_rs_pup_history_coord_can_attempt_requeue_file(
			(int)completedFile.GetStatus(), (int)CompletedFile::cfSuccess,
			(int)CompletedFile::cfFailure, resetFailed ? 1 : 0, completedFile.GetId()) != 0)
		{
			std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>();
			fileInfo->SetId(completedFile.GetId());
			bool fileLoaded = g_DiskState->LoadFile(fileInfo.get(), true, true);
			bool stateLoaded = fileLoaded &&
				completedFile.GetStatus() == CompletedFile::cfPartial &&
				g_DiskState->LoadFileState(fileInfo.get(), g_ServerPool->GetServers(), true);
			if (fileLoaded &&
				nzbget_rs_hsr_history_coord_eval_requeue_loaded_file(
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
				bool temporaryName = nzbget_rs_pup_history_coord_is_temporary_output_name(
					recordedName.c_str()) != 0;
				if (nzbget_rs_pur_history_coord_should_keep_recorded_filename(
					temporaryName ? 1 : 0,
					Util::EmptyStr(fileInfo->GetFilename()) ? 1 : 0) != 0)
				{
					fileInfo->SetFilename(recordedName.c_str());
				}
				fileInfo->SetNzbInfo(nzbInfo);

				BString<1024> outputFilename("%s%c%s", nzbInfo->GetDestDir(), PATH_SEPARATOR, recordedName.c_str());

				int64 outFileSize = fileInfo->GetSuccessArticles() == 0 ? 0 : FileSystem::FileSize(outputFilename);
				if (nzbget_rs_pur_history_coord_should_delete_empty_or_zero_success_output(
					fileInfo->GetSuccessArticles(), outFileSize) != 0)
				{
					FileSystem::DeleteFile(outputFilename);
				}

				if (fileInfo->GetSuccessArticles() > 0)
				{
					if (FileSystem::FileExists(outputFilename))
					{
						fileInfo->SetPartialState(FileInfo::psCompleted);
					}
					else if (!reprocess)
					{
						nzbInfo->PrintMessage(Message::mkWarning, "File %s could not be found on disk, downloading again", fileInfo->GetFilename());
						fileInfo->SetPartialState(FileInfo::psNone);
					}
				}

				// A partial file may contain only entries missing from the NZB.
				// They have no message IDs to retry. Keep its completed record and
				// statistics intact instead of queueing an all-finished article list.
				ArticleList* articles = fileInfo->GetArticles();
				bool noRetryableArticles = false;
				if (fileInfo->GetPartialState() == FileInfo::psCompleted)
				{
					std::vector<int> artStatuses;
					artStatuses.reserve(articles->size());
					for (const std::unique_ptr<ArticleInfo>& article : *articles)
					{
						artStatuses.push_back((int)article->GetStatus());
					}
					noRetryableArticles = nzbget_rs_hsr_history_coord_has_retryable_article(
						artStatuses.data(),
						artStatuses.size(),
						resetFailed ? 1 : 0,
						(int)ArticleInfo::aiUndefined,
						(int)ArticleInfo::aiFailed) == 0;
				}
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
	if (nzbget_rs_pur_history_coord_should_pause_health_on_remaining_retry(
		resetFailed ? 1 : 0,
		reprocess ? 1 : 0) != 0)
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

void HistoryCoordinator::ResetArticles(FileInfo* fileInfo, bool allFailed, bool resetFailed)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();

	int64 fileFailedSize = fileInfo->GetFailedSize();
	int fileFailedArticles = fileInfo->GetFailedArticles();
	int64 fileRemainingSize = fileInfo->GetRemainingSize();
	int fileCompletedArticles = fileInfo->GetCompletedArticles();
	int64 nzbFailedSize = nzbInfo->GetFailedSize();
	int64 nzbSuccessSize = nzbInfo->GetSuccessSize();
	int nzbFailedArticles = nzbInfo->GetFailedArticles();
	int nzbSuccessArticles = nzbInfo->GetSuccessArticles();
	int64 nzbParFailedSize = nzbInfo->GetParFailedSize();
	int64 nzbParSuccessSize = nzbInfo->GetParSuccessSize();
	nzbget_rs_hsr_history_coord_reset_articles_header(
		allFailed ? 1 : 0,
		fileInfo->GetParFile() ? 1 : 0,
		fileInfo->GetSize(),
		fileInfo->GetMissedSize(),
		fileInfo->GetTotalArticles(),
		fileInfo->GetMissedArticles(),
		fileFailedSize,
		fileFailedArticles,
		fileRemainingSize,
		fileCompletedArticles,
		fileInfo->GetSuccessSize(),
		fileInfo->GetSuccessArticles(),
		nzbFailedSize,
		nzbSuccessSize,
		nzbFailedArticles,
		nzbSuccessArticles,
		nzbParFailedSize,
		nzbParSuccessSize,
		&fileFailedSize,
		&fileFailedArticles,
		&fileRemainingSize,
		&fileCompletedArticles,
		&nzbFailedSize,
		&nzbSuccessSize,
		&nzbFailedArticles,
		&nzbSuccessArticles,
		&nzbParFailedSize,
		&nzbParSuccessSize);
	if (allFailed)
	{
		fileInfo->SetFailedSize(fileFailedSize);
		fileInfo->SetFailedArticles(fileFailedArticles);
		fileInfo->SetRemainingSize(fileRemainingSize);
		fileInfo->SetCompletedArticles(fileCompletedArticles);
	}
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

	for (ArticleInfo* pa : fileInfo->GetArticles())
	{
		if (nzbget_rs_qhp_should_reset_article(
			(int)pa->GetStatus(),
			(int)fileInfo->GetPartialState(),
			allFailed ? 1 : 0,
			resetFailed ? 1 : 0
		) != 0)
		{
			int compArt = fileInfo->GetCompletedArticles();
			int64 remSize = fileInfo->GetRemainingSize();
			int failArt = fileInfo->GetFailedArticles();
			int64 failSize = fileInfo->GetFailedSize();
			int succArt = fileInfo->GetSuccessArticles();
			int64 succSize = fileInfo->GetSuccessSize();
			nzbget_rs_hsr_history_coord_apply_article_reset_delta(
				(int)pa->GetStatus(),
				(int)ArticleInfo::aiFailed,
				(int)ArticleInfo::aiUndefined,
				(int)ArticleInfo::aiFinished,
				pa->GetSize(),
				compArt,
				remSize,
				failArt,
				failSize,
				succArt,
				succSize,
				&compArt,
				&remSize,
				&failArt,
				&failSize,
				&succArt,
				&succSize);
			fileInfo->SetCompletedArticles(compArt);
			fileInfo->SetRemainingSize(remSize);
			fileInfo->SetFailedArticles(failArt);
			fileInfo->SetFailedSize(failSize);
			fileInfo->SetSuccessArticles(succArt);
			fileInfo->SetSuccessSize(succSize);

			pa->SetStatus(ArticleInfo::aiUndefined);
			pa->SetCrc(0);
			pa->SetSegmentOffset(0);
			pa->SetSegmentSize(0);
			pa->SetDupeExpectedOffset(-1);
			pa->SetDupeExpectedEnd(-1);
		}
	}
}

bool HistoryCoordinator::HistorySetParameter(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting post-process-parameter '%s' for '%s'", text, historyInfo->GetName());

	if (nzbget_rs_pur_history_coord_is_nzb_or_url_kind(
		(int)historyInfo->GetKind(),
		(int)HistoryInfo::hkNzb,
		(int)HistoryInfo::hkUrl) == 0)
	{
		error("Could not set post-process-parameter for %s: history item has wrong type", historyInfo->GetName());
		return false;
	}

	CString str = text;
	char* value = strchr(str, '=');
	if (value)
	{
		*value = '\0';
		value++;
		historyInfo->GetNzbInfo()->GetParameters()->SetParameter(str, value);
	}
	else
	{
		error("Could not set post-process-parameter for %s: invalid argument: %s", historyInfo->GetNzbInfo()->GetName(), text);
	}

	return true;
}

bool HistoryCoordinator::HistorySetCategory(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting category '%s' for '%s'", text, historyInfo->GetName());

	if (nzbget_rs_pur_history_coord_is_nzb_or_url_kind(
		(int)historyInfo->GetKind(),
		(int)HistoryInfo::hkNzb,
		(int)HistoryInfo::hkUrl) == 0)
	{
		error("Could not set category for %s: history item has wrong type", historyInfo->GetName());
		return false;
	}

	historyInfo->GetNzbInfo()->SetCategory(text);

	return true;
}

bool HistoryCoordinator::HistorySetName(HistoryInfo* historyInfo, const char* text)
{
	debug("Setting name '%s' for '%s'", text, historyInfo->GetName());

	if (Util::EmptyStr(text))
	{
		error("Could not rename %s. The new name cannot be empty", historyInfo->GetName());
		return false;
	}

	if (nzbget_rs_pur_history_coord_is_nzb_or_url_kind(
		(int)historyInfo->GetKind(),
		(int)HistoryInfo::hkNzb,
		(int)HistoryInfo::hkUrl) != 0)
	{
		std::string previous = historyInfo->GetNzbInfo()->GetName();
		historyInfo->GetNzbInfo()->SetName(text);
		if (nzbget_rs_pur_history_coord_should_notify_dupe_search_name_changed(
			(int)historyInfo->GetKind(),
			(int)HistoryInfo::hkNzb,
			g_DupeSearch != nullptr ? 1 : 0,
			previous != historyInfo->GetNzbInfo()->GetName() ? 1 : 0) != 0)
            g_DupeSearch->NotifyNameChanged(historyInfo->GetNzbInfo()->GetId());
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		historyInfo->GetDupInfo()->SetName(text);
	}

	return true;
}

void HistoryCoordinator::HistorySetDupeParam(HistoryInfo* historyInfo, DownloadQueue::EEditAction action, const char* text)
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

	if (nzbget_rs_pur_history_coord_is_nzb_or_url_kind(
		(int)historyInfo->GetKind(),
		(int)HistoryInfo::hkNzb,
		(int)HistoryInfo::hkUrl) != 0)
	{
		switch (action)
		{
			case DownloadQueue::eaHistorySetDupeKey:
                historyInfo->GetNzbInfo()->GetParameters()->SetParameter(DupeSearch::OwnedKeyParam, nullptr);
				historyInfo->GetNzbInfo()->SetDupeKey(text);
				break;

			case DownloadQueue::eaHistorySetDupeScore:
				historyInfo->GetNzbInfo()->SetDupeScore(atoi(text));
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
				else if (nzbget_rs_pur_history_coord_is_valid_dupe_backup_delete_status(
					(int)historyInfo->GetNzbInfo()->GetDeleteStatus(),
					(int)NzbInfo::dsDupe,
					(int)NzbInfo::dsManual) == 0)
				{
					error("Could not set duplicate parameter for %s: history item has wrong delete status", historyInfo->GetName());
					return;
				}
				historyInfo->GetNzbInfo()->SetDeleteStatus(
					(NzbInfo::EDeleteStatus)nzbget_rs_pup_history_coord_parse_dupe_backup_flag(
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
				historyInfo->GetDupInfo()->SetDupeScore(atoi(text));
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

void HistoryCoordinator::Redownload(DownloadQueue* downloadQueue, HistoryInfo* historyInfo)
{
	HistoryList::iterator it = downloadQueue->GetHistory()->Find(historyInfo);
	HistoryRedownload(downloadQueue, it, historyInfo, true);
}
