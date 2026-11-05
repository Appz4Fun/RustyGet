/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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

#include <algorithm>
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "NzbFile.h"
#include "HistoryCoordinator.h"
#include "DupeCoordinator.h"
#include "QueueScript.h"
#include "QueueCoordinator.h"
#include "nzbget_rs.h"

bool DupeCoordinator::SameNameOrKey(const char* name1, const char* dupeKey1,
	const char* name2, const char* dupeKey2)
{
	return nzbget_rs_dupecoord_same_name_or_key(name1, dupeKey1, name2, dupeKey2) != 0;
}

/**
  Check if the title was already downloaded or is already queued:
  - if there is a duplicate with exactly same content (via hash-check)
	in queue or in history - the new item is skipped;
  - if there is a duplicate marked as good in history - the new item is skipped;
  - if there is a duplicate with success-status in dup-history but
	there are no duplicates in recent history - the new item is skipped;
  - if queue has a duplicate with the same or higher score - the new item
	is moved to history as dupe-backup;
  - if queue has a duplicate with lower score - the existing item is moved
	to history as dupe-backup (unless it is in post-processing stage) and
	the new item is added to queue;
  - if queue doesn't have duplicates - the new item is added to queue.
*/
bool DupeCoordinator::FilesOnDisk(NzbInfo* nzbInfo)
{
	const char* dir = !Util::EmptyStr(nzbInfo->GetFinalDir()) ? nzbInfo->GetFinalDir() : nzbInfo->GetDestDir();
	if (Util::EmptyStr(dir) || !FileSystem::DirectoryExists(dir))
	{
		return false;
	}
	DirBrowser browser(dir);
	while (const char* filename = browser.Next())
	{
		BString<1024> path("%s%c%s", dir, PATH_SEPARATOR, filename);
		if (!QueueCoordinator::SideFile(filename) && !FileSystem::DirectoryExists(path) &&
			FileSystem::FileSize(path) > 0)
		{
			return true;
		}
	}
	return false;
}

NzbInfo* DupeCoordinator::DownloadedOnDisk(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	bool dupeHealth = nzbget_rs_qcf_dupecoord_is_dupe_health_mode(
		(int)g_Options->GetHealthCheck(),
		(int)Options::hcDupe,
		(int)Options::hcPark) != 0;
	if (!dupeHealth || Util::EmptyStr(nzbInfo->GetDupeKey()))
	{
		return nullptr;
	}
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() != HistoryInfo::hkNzb)
		{
			continue;
		}
		NzbInfo* item = historyInfo->GetNzbInfo();
		// a success the user deleted (or marked bad) from disk no longer counts
		if (nzbget_rs_dcf_dupecoord_is_downloaded_on_disk_candidate(
				item == nzbInfo ? 1 : 0,
				(int)item->GetDeleteStatus(),
				(int)NzbInfo::dsNone,
				item->IsDupeSuccess() ? 1 : 0,
				(int)item->GetMarkStatus(),
				(int)NzbInfo::ksBad,
				item->GetDupeKey(),
				nzbInfo->GetDupeKey()) != 0 &&
			FilesOnDisk(item))
		{
			return item;
		}
	}
	return nullptr;
}

void DupeCoordinator::NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Checking duplicates for %s", nzbInfo->GetName());

	// the release is already downloaded and on disk: another posting of it isn't
	// needed (B70: Knife Edge S02E01 - asked again after its success, a different
	// posting of the same release, scored higher, downloaded a second copy). Kept
	// in history as a backup, in case the download on disk is deleted or marked bad.
	if (nzbget_rs_dqh_dupecoord_can_check_downloaded_on_disk(
		(int)nzbInfo->GetKind(),
		(int)NzbInfo::nkNzb,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce) != 0)
	{
		if (NzbInfo* downloaded = DownloadedOnDisk(downloadQueue, nzbInfo))
		{
			nzbInfo->PrintMessage(Message::mkInfo, "Not downloading %s: %s is downloaded and on disk",
				nzbInfo->GetName(), downloaded->GetName());
			nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
			return;
		}
	}

	// find duplicates in download queue with exactly same content
	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		bool sameContent = nzbget_rs_cdn_dupecoord_same_content_hash(
			nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
			queuedNzbInfo->GetFullContentHash(), queuedNzbInfo->GetFilteredContentHash()) != 0;

		// if there is a duplicate with exactly same content (via hash-check)
		// in queue - the new item is skipped
		if (nzbget_rs_dqh_dupecoord_is_queued_same_content_dupe(
			queuedNzbInfo == nzbInfo ? 1 : 0,
			sameContent ? 1 : 0,
			(int)nzbInfo->GetKind(),
			(int)NzbInfo::nkNzb) != 0)
		{
			NzbgetRsBuf msgBuf = nzbget_rs_dqh_dupecoord_format_queued_dupe_skip_message(
				nzbInfo->GetName(), queuedNzbInfo->GetName());
			std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
			nzbget_rs_free(msgBuf);

			if (nzbInfo->GetFeedId())
			{
				warn("%s", message.c_str());
				// Flag saying QueueCoordinator to skip nzb-file
				nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
				g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
			}
			else
			{
				nzbInfo->SetDeleteStatus(NzbInfo::dsCopy);
				nzbInfo->AddMessage(Message::mkWarning, message.c_str());
			}

			return;
		}
	}

	// if download has empty dupekey and empty dupescore - check if download queue
	// or history have an item with the same name and non empty dupekey or dupescore and
	// take these properties from this item
	if (nzbget_rs_dqh_dupecoord_needs_dupe_prop_inheritance(
		Util::EmptyStr(nzbInfo->GetDupeKey()) ? 1 : 0,
		nzbInfo->GetDupeScore()) != 0)
	{
		for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dcf_dupecoord_should_inherit_dupe_props(
				queuedNzbInfo->GetName(), nzbInfo->GetName(),
				queuedNzbInfo->GetDupeKey(), queuedNzbInfo->GetDupeScore()) != 0)
			{
				nzbInfo->SetDupeKey(queuedNzbInfo->GetDupeKey());
				nzbInfo->SetDupeScore(queuedNzbInfo->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing queue item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
		}
	}
	if (nzbget_rs_dqh_dupecoord_needs_dupe_prop_inheritance(
		Util::EmptyStr(nzbInfo->GetDupeKey()) ? 1 : 0,
		nzbInfo->GetDupeScore()) != 0)
	{
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
				nzbget_rs_dcf_dupecoord_should_inherit_dupe_props(
					historyInfo->GetNzbInfo()->GetName(), nzbInfo->GetName(),
					historyInfo->GetNzbInfo()->GetDupeKey(), historyInfo->GetNzbInfo()->GetDupeScore()) != 0)
			{
				nzbInfo->SetDupeKey(historyInfo->GetNzbInfo()->GetDupeKey());
				nzbInfo->SetDupeScore(historyInfo->GetNzbInfo()->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing history item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
			if (historyInfo->GetKind() == HistoryInfo::hkDup &&
				nzbget_rs_dcf_dupecoord_should_inherit_dupe_props(
					historyInfo->GetDupInfo()->GetName(), nzbInfo->GetName(),
					historyInfo->GetDupInfo()->GetDupeKey(), historyInfo->GetDupInfo()->GetDupeScore()) != 0)
			{
				nzbInfo->SetDupeKey(historyInfo->GetDupInfo()->GetDupeKey());
				nzbInfo->SetDupeScore(historyInfo->GetDupInfo()->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing history item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
		}
	}

	// find duplicates in history

	bool skip = false;
	bool good = false;
	bool sameContent = false;
	const char* dupeName = nullptr;

	// find duplicates in history having exactly same content
	// also: nzb-files having duplicates marked as good are skipped
	// also (only in score mode): nzb-files having success-duplicates in dup-history but not having duplicates in recent history are skipped
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			nzbget_rs_cdn_dupecoord_same_content_hash(
				nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
				historyInfo->GetNzbInfo()->GetFullContentHash(),
				historyInfo->GetNzbInfo()->GetFilteredContentHash()) != 0)
		{
			skip = true;
			sameContent = true;
			dupeName = historyInfo->GetNzbInfo()->GetName();
			break;
		}

		if (historyInfo->GetKind() == HistoryInfo::hkDup &&
			nzbget_rs_cdn_dupecoord_same_content_hash(
				nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
				historyInfo->GetDupInfo()->GetFullContentHash(),
				historyInfo->GetDupInfo()->GetFilteredContentHash()) != 0)
		{
			skip = true;
			sameContent = true;
			dupeName = historyInfo->GetDupInfo()->GetName();
			break;
		}

		if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			bool isHkNzb = historyInfo->GetKind() == HistoryInfo::hkNzb;
			const char* hName = isHkNzb ? historyInfo->GetNzbInfo()->GetName() : historyInfo->GetDupInfo()->GetName();
			const char* hKey = isHkNzb ? historyInfo->GetNzbInfo()->GetDupeKey() : historyInfo->GetDupInfo()->GetDupeKey();
			int hDupeMode = isHkNzb ? (int)historyInfo->GetNzbInfo()->GetDupeMode() : (int)historyInfo->GetDupInfo()->GetDupeMode();
			int hMarkStatus = isHkNzb ? (int)historyInfo->GetNzbInfo()->GetMarkStatus() : 0;
			int hDupStatus = isHkNzb ? 0 : (int)historyInfo->GetDupInfo()->GetStatus();
			int hDupeScore = isHkNzb ? historyInfo->GetNzbInfo()->GetDupeScore() : historyInfo->GetDupInfo()->GetDupeScore();
			int outGood = 0;
			if (nzbget_rs_pnd_dupecoord_check_history_good_or_score_skip(
				isHkNzb ? 1 : 0, isHkNzb ? 0 : 1, hDupeMode, (int)dmForce,
				hMarkStatus, (int)NzbInfo::ksGood, hDupStatus, (int)DupInfo::dsGood,
				(int)DupInfo::dsSuccess, (int)nzbInfo->GetDupeMode(), (int)dmScore,
				nzbInfo->GetDupeScore(), hDupeScore,
				SameNameOrKey(hName, hKey, nzbInfo->GetName(), nzbInfo->GetDupeKey()) ? 1 : 0,
				&outGood) != 0)
			{
				skip = true;
				good = outGood != 0;
				dupeName = hName;
				break;
			}
		}
	}

	// With HealthCheck=dupe (B43) the same content in history decides by what became
	// of it: only untried backups (and items the user deleted) - the nzb-file is
	// downloaded as sent, not skipped as a copy (a client re-sending it would get
	// nothing); a success - skipped, as before; a failure - skipped too (the same dead
	// posting), but the best viable backup is fetched in its place when nothing of the
	// key is queued, so the re-send still leads to a download
	bool returnBackup = false;
	if (nzbget_rs_dqh_dupecoord_should_inspect_same_content_history(
		skip ? 1 : 0,
		sameContent ? 1 : 0,
		(int)g_Options->GetHealthCheck(),
		(int)Options::hcDupe,
		(int)nzbInfo->GetKind(),
		(int)NzbInfo::nkNzb,
		(int)nzbInfo->GetDupeHint(),
		(int)NzbInfo::dhNone) != 0)
	{
		auto same = [nzbInfo](uint32 fullHash, uint32 filteredHash)
			{
				return nzbget_rs_cdn_dupecoord_same_content_hash(
					nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
					fullHash, filteredHash) != 0;
			};
		bool succeeded = false;
		bool failed = false;
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			bool isHkNzb = historyInfo->GetKind() == HistoryInfo::hkNzb;
			bool isHkDup = historyInfo->GetKind() == HistoryInfo::hkDup;
			if (isHkNzb || isHkDup)
			{
				uint32 fullH = isHkNzb ? historyInfo->GetNzbInfo()->GetFullContentHash() : historyInfo->GetDupInfo()->GetFullContentHash();
				uint32 filtH = isHkNzb ? historyInfo->GetNzbInfo()->GetFilteredContentHash() : historyInfo->GetDupInfo()->GetFilteredContentHash();
				int delStat = isHkNzb ? (int)historyInfo->GetNzbInfo()->GetDeleteStatus() : 0;
				bool isDupeSucc = isHkNzb ? historyInfo->GetNzbInfo()->IsDupeSuccess() : false;
				int dupStat = isHkDup ? (int)historyInfo->GetDupInfo()->GetStatus() : 0;
				int itemFailed = 0;
				int itemSucc = nzbget_rs_pnd_dupecoord_classify_same_content_history_item(
					isHkNzb ? 1 : 0, isHkDup ? 1 : 0, same(fullH, filtH) ? 1 : 0,
					delStat, (int)NzbInfo::dsDupe, (int)NzbInfo::dsCopy, (int)NzbInfo::dsManual,
					isDupeSucc ? 1 : 0, dupStat, (int)DupInfo::dsSuccess, (int)DupInfo::dsGood,
					(int)DupInfo::dsFailed, (int)DupInfo::dsBad, &itemFailed);
				succeeded |= itemSucc != 0;
				failed |= itemFailed != 0;
			}
		}
		int clearSkipAndSameContent = 0;
		int checkReturnBackup = 0;
		nzbget_rs_qcf_dupecoord_same_content_history_outcome(
			succeeded ? 1 : 0,
			failed ? 1 : 0,
			&clearSkipAndSameContent,
			&checkReturnBackup);
		if (clearSkipAndSameContent)
		{
			info("Adding %s: the same content waits in history only as an untried backup", nzbInfo->GetName());
			skip = false;
			sameContent = false;
		}
		else if (checkReturnBackup)
		{
			returnBackup = !std::any_of(downloadQueue->GetQueue()->begin(), downloadQueue->GetQueue()->end(),
				[nzbInfo](std::unique_ptr<NzbInfo>& queued)
				{
					return queued.get() != nzbInfo && queued->GetDeleteStatus() == NzbInfo::dsNone &&
						SameNameOrKey(queued->GetName(), queued->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey());
				});
		}
	}

	if (!sameContent && nzbInfo->GetDupeHint() != NzbInfo::dhNone)
	{
		// dupe check when "download again" URLs: checking same content only
		return;
	}

	if (!sameContent && !good && nzbInfo->GetDupeMode() == dmScore)
	{
		// nzb-files having success-duplicates in recent history (with different content) are added to history for backup
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			bool isNzbOrUrl = historyInfo->GetKind() == HistoryInfo::hkNzb ||
				historyInfo->GetKind() == HistoryInfo::hkUrl;
			NzbInfo* hItem = isNzbOrUrl ? historyInfo->GetNzbInfo() : nullptr;
			if (hItem && nzbget_rs_dcf_dupecoord_is_recent_history_score_duplicate(
				(int)historyInfo->GetKind(),
				(int)HistoryInfo::hkNzb,
				(int)HistoryInfo::hkUrl,
				(int)hItem->GetDupeMode(),
				(int)dmForce,
				SameNameOrKey(hItem->GetName(), hItem->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey()) ? 1 : 0,
				nzbInfo->GetDupeScore(),
				hItem->GetDupeScore(),
				hItem->IsDupeSuccess() ? 1 : 0) != 0)
			{
				// Flag saying QueueCoordinator to skip nzb-file
				nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
				info("Collection %s is a duplicate to %s", nzbInfo->GetName(), historyInfo->GetNzbInfo()->GetName());
				return;
			}
		}
	}

	if (skip)
	{
		NzbgetRsBuf msgBuf = nzbget_rs_dqh_dupecoord_format_history_dupe_skip_message(
			nzbInfo->GetName(), dupeName, sameContent ? 1 : 0, good ? 1 : 0);
		std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
		nzbget_rs_free(msgBuf);

		int deleteDisk = 0;
		int delStatus = nzbget_rs_pnd_dupecoord_resolve_skip_action(
			nzbInfo->GetFeedId(), (int)nzbInfo->GetDupeHint(), (int)NzbInfo::dhNone,
			sameContent ? 1 : 0, (int)NzbInfo::dsManual, (int)NzbInfo::dsCopy,
			(int)NzbInfo::dsGood, &deleteDisk);
		nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)delStatus);
		if (deleteDisk != 0)
		{
			warn("%s", message.c_str());
			g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
		}
		else
		{
			nzbInfo->AddMessage(Message::mkWarning, message.c_str());
		}

		if (returnBackup)
		{
			info("%s is the same posting as a failed download: fetching the best backup instead", nzbInfo->GetName());
			ReturnBestDupe(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey());
		}

		return;
	}

	// find duplicates in download queue and post-queue and handle both items according to their scores:
	// only one item remains in queue and another one is moved to history as dupe-backup
	if (nzbInfo->GetDupeMode() == dmScore)
	{
		// find duplicates in download queue
		int index = 0;
		for (NzbList::iterator it = downloadQueue->GetQueue()->begin(); it != downloadQueue->GetQueue()->end(); index++)
		{
			NzbInfo* queuedNzbInfo = (*it++).get();
			if (nzbget_rs_dqh_dupecoord_is_queue_score_dupe_candidate(
				queuedNzbInfo == nzbInfo ? 1 : 0,
				(int)queuedNzbInfo->GetDeleteStatus(),
				(int)NzbInfo::dsNone,
				(int)queuedNzbInfo->GetKind(),
				(int)nzbInfo->GetKind(),
				(int)NzbInfo::nkNzb,
				(int)NzbInfo::nkUrl,
				(int)queuedNzbInfo->GetDupeMode(),
				(int)dmForce) != 0 &&
				SameNameOrKey(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(),
					nzbInfo->GetName(), nzbInfo->GetDupeKey()))
			{
				// if queue has a duplicate with the same or higher score - the new item
				// is moved to history as dupe-backup. With HealthCheck=dupe so is one
				// arriving while a backup that failover returned downloads in good
				// health (B73: Las Azules S02E01 4404 lost its progress to a duplicate
				// a dupe tool appended, scored higher but no better): it is tried next
				// if that backup fails too
				bool keepReturned = nzbget_rs_cdn_dupecoord_should_keep_returned_dupe(
					(int)g_Options->GetHealthCheck(),
					(int)Options::hcDupe,
					(int)queuedNzbInfo->GetDupeHint(),
					(int)NzbInfo::dhRedownloadAuto,
					queuedNzbInfo->GetCurrentSuccessArticles(),
					queuedNzbInfo->CalcHealth(),
					queuedNzbInfo->CalcCriticalHealth(true)) != 0;
				if (nzbInfo->GetDupeScore() <= queuedNzbInfo->GetDupeScore() || keepReturned)
				{
					// Flag saying QueueCoordinator to skip nzb-file
					nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
					info("Collection %s is a duplicate to %s", nzbInfo->GetName(), queuedNzbInfo->GetName());
					return;
				}

				// if queue has a duplicate with lower score - the existing item is moved
				// to history as dupe-backup (unless it is in post-processing stage) and
				// the new item is added to queue (unless it is in post-processing stage)
				if (!queuedNzbInfo->GetPostInfo())
				{
					// the existing queue item is moved to history as dupe-backup
					info("Moving collection %s with lower duplicate score to history", queuedNzbInfo->GetName());
					queuedNzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
					int oldSize = downloadQueue->GetQueue()->size();
					downloadQueue->EditEntry(queuedNzbInfo->GetId(),
						DownloadQueue::eaGroupDelete, nullptr);
					int newSize = downloadQueue->GetQueue()->size();
					index = nzbget_rs_dcf_dupecoord_calc_post_delete_index(index, oldSize, newSize);
					it = downloadQueue->GetQueue()->begin() + index;
					index--;
				}
			}
		}
	}
}

/**
  - if download of an item fails and there are duplicates in history -
	return the best duplicate from history to queue for download;
  - if download of an item completes successfully - nothing extra needs to be done;
*/
void DupeCoordinator::NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Processing duplicates for %s", nzbInfo->GetName());

	if (FailsOver(nzbInfo) && !nzbInfo->IsDupeSuccess())
	{
		ReturnBestDupe(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey());
	}

	// one copy is downloaded and on disk: the others of its key still downloading,
	// repairing or checking are stopped and kept as backups (B77: Sugar S01E01 - a
	// copy kept repairing for minutes after another one had succeeded, then failed)
	bool dupeHealth = nzbget_rs_qcf_dupecoord_is_dupe_health_mode(
		(int)g_Options->GetHealthCheck(),
		(int)Options::hcDupe,
		(int)Options::hcPark) != 0;
	int deleteActionInt = 0;
	if (nzbget_rs_dqh_dupecoord_eval_completed_stop_others(
		dupeHealth ? 1 : 0,
		Util::EmptyStr(nzbInfo->GetDupeKey()) ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		nzbInfo->IsDupeSuccess() ? 1 : 0,
		(int)g_Options->GetHealthCheck(),
		(int)Options::hcPark,
		(int)DownloadQueue::eaGroupParkDelete,
		(int)DownloadQueue::eaGroupDupeDelete,
		&deleteActionInt) != 0 && FilesOnDisk(nzbInfo))
	{
		DownloadQueue::EEditAction deleteAction = (DownloadQueue::EEditAction)deleteActionInt;
		std::vector<std::pair<int, bool>> others;	// id, in post-processing
		for (NzbInfo* queued : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dcf_dupecoord_should_stop_other_queued_dupe(
				queued == nzbInfo ? 1 : 0,
				(int)queued->GetKind(),
				(int)NzbInfo::nkNzb,
				queued->GetDeleting() ? 1 : 0,
				(int)queued->GetDeleteStatus(),
				(int)NzbInfo::dsNone,
				(int)queued->GetDupeMode(),
				(int)dmForce,
				queued->GetDupeKey(),
				nzbInfo->GetDupeKey()) != 0)
			{
				queued->PrintMessage(Message::mkInfo, "Stopping %s: %s is downloaded and on disk",
					queued->GetName(), nzbInfo->GetName());
				queued->SetDeleteStatus(NzbInfo::dsDupe);
				others.emplace_back(queued->GetId(), queued->GetPostInfo() != nullptr);
			}
		}
		for (const auto& other : others)
		{
			downloadQueue->EditEntry(other.first, other.second ? DownloadQueue::eaPostDelete :
				deleteAction, nullptr);
		}
	}
}

std::string DupeCoordinator::NoBackupReason(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (!FailsOver(nzbInfo))
	{
		return nzbget_rs_dqh_dupecoord_format_non_failover_reason(
			(int)nzbInfo->GetDupeMode(), (int)dmForce);
	}
	int total = 0, notBackup = 0, forced = 0, noFile = 0, unhealthy = 0, bad = 0, dead = 0;
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() != HistoryInfo::hkNzb ||
			!SameNameOrKey(historyInfo->GetNzbInfo()->GetName(), historyInfo->GetNzbInfo()->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()) || historyInfo->GetNzbInfo() == nzbInfo)
		{
			continue;
		}
		NzbInfo* item = historyInfo->GetNzbInfo();
		total++;
		int alive = AlivePermille(item);
		bool fileExists = (item->GetDeleteStatus() == NzbInfo::dsDupe || item->GetDeleteStatus() == NzbInfo::dsCopy) &&
			item->GetDupeMode() != dmForce ? FileSystem::FileExists(item->GetQueuedFilename()) : false;
		int bucket = nzbget_rs_dupecoord_classify_no_backup_item(
			(int)item->GetDeleteStatus(), (int)item->GetDupeMode(), fileExists ? 1 : 0,
			item->CalcHealth(), item->CalcCriticalHealth(true), (int)item->GetMarkStatus(), alive);
		if (bucket == 1) notBackup++;
		else if (bucket == 2) forced++;
		else if (bucket == 3) noFile++;
		else if (bucket == 4) unhealthy++;
		else if (bucket == 5) bad++;
		else if (bucket == 6) dead++;
	}
	NzbgetRsBuf buf = nzbget_rs_dupecoord_format_no_backup_reason(
		total, notBackup, forced, noFile, unhealthy, bad, dead);
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

bool DupeCoordinator::FailsOver(NzbInfo* nzbInfo)
{
	return nzbget_rs_dupecoord_fails_over(
		g_Options->GetDupeCheck() ? 1 : 0, (int)nzbInfo->GetDupeMode(), (int)g_Options->GetHealthCheck()) != 0;
}

/**
 Returns the best duplicate from history to download queue.
*/
void DupeCoordinator::ReturnBestDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey)
{
	HistoryInfo* historyDupe = FindDupeBackup(downloadQueue, nzbInfo, nzbName, dupeKey);

	// move that dupe-backup from history to download queue
	if (historyDupe)
	{
		info("Found duplicate %s for %s", historyDupe->GetNzbInfo()->GetName(), nzbName);
		historyDupe->GetNzbInfo()->SetDupeHint(NzbInfo::dhRedownloadAuto);
		g_HistoryCoordinator->Redownload(downloadQueue, historyDupe);
	}
}

HistoryInfo* DupeCoordinator::FindDupeBackup(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey)
{
	std::vector<HistoryInfo*> historyItems;
	std::vector<NzbgetRsDupeHistoryCandidate> historyCandidates;
	historyItems.reserve(downloadQueue->GetHistory()->size());
	historyCandidates.reserve(downloadQueue->GetHistory()->size());
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		historyItems.push_back(historyInfo);
		NzbgetRsDupeHistoryCandidate c{};
		c.kind = (int)historyInfo->GetKind();
		if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			c.dupe_mode = (int)item->GetDupeMode();
			c.dupe_score = item->GetDupeScore();
			c.is_dupe_success = item->IsDupeSuccess() ? 1 : 0;
			c.mark_status = (int)item->GetMarkStatus();
			c.delete_status = (int)item->GetDeleteStatus();
			c.full_content_hash = item->GetFullContentHash();
			c.alive_permille = AlivePermille(item);
			c.health = item->CalcHealth();
			c.critical_health = item->CalcCriticalHealth(true);
			c.same_name_or_key = SameNameOrKey(item->GetName(), item->GetDupeKey(), nzbName, dupeKey) ? 1 : 0;
		}
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			DupInfo* dup = historyInfo->GetDupInfo();
			c.dupe_mode = (int)dup->GetDupeMode();
			c.dupe_score = dup->GetDupeScore();
			c.dup_status = (int)dup->GetStatus();
			c.same_name_or_key = SameNameOrKey(dup->GetName(), dup->GetDupeKey(), nzbName, dupeKey) ? 1 : 0;
		}
		historyCandidates.push_back(c);
	}

	std::vector<NzbgetRsDupeQueueCandidate> queueCandidates;
	queueCandidates.reserve(downloadQueue->GetQueue()->size());
	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		NzbgetRsDupeQueueCandidate q{};
		q.is_self = (queuedNzbInfo == nzbInfo) ? 1 : 0;
		q.is_nzb_kind = (queuedNzbInfo->GetKind() == NzbInfo::nkNzb) ? 1 : 0;
		q.dupe_mode = (int)queuedNzbInfo->GetDupeMode();
		q.dupe_score = queuedNzbInfo->GetDupeScore();
		q.same_name_or_key = SameNameOrKey(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(), nzbName, dupeKey) ? 1 : 0;
		queueCandidates.push_back(q);
	}

	auto fileExistsCb = [](void* ctx, size_t idx) -> int {
		auto* items = static_cast<std::vector<HistoryInfo*>*>(ctx);
		return FileSystem::FileExists((*items)[idx]->GetNzbInfo()->GetQueuedFilename()) ? 1 : 0;
	};

	int winner = nzbget_rs_dupecoord_select_dupe_backup(
		nzbInfo != nullptr ? 1 : 0,
		nzbInfo ? nzbInfo->GetFullContentHash() : 0,
		g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0,
		historyCandidates.data(),
		historyCandidates.size(),
		queueCandidates.data(),
		queueCandidates.size(),
		&historyItems,
		fileExistsCb);

	return winner >= 0 && (size_t)winner < historyItems.size() ? historyItems[(size_t)winner] : nullptr;
}

bool DupeCoordinator::DupeFailoverWarranted(int itemScore, int health, int backupScore)
{
	return nzbget_rs_dupecoord_failover_warranted(itemScore, health, backupScore) != 0;
}

void DupeCoordinator::HistoryMark(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, NzbInfo::EMarkStatus markStatus)
{
	info("Marking %s as %s", historyInfo->GetName(),
		nzbget_rs_dqh_dupecoord_mark_status_label((int)markStatus));

	if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		historyInfo->GetNzbInfo()->SetMarkStatus(markStatus);
		g_QueueScriptCoordinator->EnqueueScript(historyInfo->GetNzbInfo(), QueueScriptCoordinator::qeNzbMarked);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		historyInfo->GetDupInfo()->SetStatus(
			(DupInfo::EStatus)nzbget_rs_dcf_dupecoord_map_mark_to_dup_status(
				(int)markStatus,
				(int)NzbInfo::ksGood,
				(int)NzbInfo::ksSuccess,
				(int)DupInfo::dsGood,
				(int)DupInfo::dsSuccess,
				(int)DupInfo::dsBad));
	}
	else
	{
		error("Could not mark %s as bad: history item has wrong type", historyInfo->GetName());
		return;
	}

	int itemDupeMode = historyInfo->GetKind() == HistoryInfo::hkNzb ? (int)historyInfo->GetNzbInfo()->GetDupeMode() :
		historyInfo->GetKind() == HistoryInfo::hkDup ? (int)historyInfo->GetDupInfo()->GetDupeMode() : 0;
	if (nzbget_rs_dqh_dupecoord_should_skip_mark_action(
		g_Options->GetDupeCheck() ? 1 : 0,
		(int)historyInfo->GetKind(),
		(int)HistoryInfo::hkNzb,
		(int)HistoryInfo::hkDup,
		itemDupeMode,
		(int)dmForce) != 0)
	{
		return;
	}

	if (markStatus == NzbInfo::ksGood)
	{
		// mark as good
		// moving all duplicates from history to dup-history
		HistoryCleanup(downloadQueue, historyInfo);
	}
	else if (markStatus == NzbInfo::ksBad)
	{
		// mark as bad
		const char* dupeKey = historyInfo->GetKind() == HistoryInfo::hkNzb ? historyInfo->GetNzbInfo()->GetDupeKey() :
			historyInfo->GetKind() == HistoryInfo::hkDup ? historyInfo->GetDupInfo()->GetDupeKey() :
			nullptr;
		ReturnBestDupe(downloadQueue, nullptr, historyInfo->GetName(), dupeKey);
	}
}

void DupeCoordinator::HistoryCleanup(DownloadQueue* downloadQueue, HistoryInfo* markHistoryInfo)
{
	const char* dupeKey = markHistoryInfo->GetKind() == HistoryInfo::hkNzb ? markHistoryInfo->GetNzbInfo()->GetDupeKey() :
		markHistoryInfo->GetKind() == HistoryInfo::hkDup ? markHistoryInfo->GetDupInfo()->GetDupeKey() :
		nullptr;
	const char* nzbName = markHistoryInfo->GetKind() == HistoryInfo::hkNzb ? markHistoryInfo->GetNzbInfo()->GetName() :
		markHistoryInfo->GetKind() == HistoryInfo::hkDup ? markHistoryInfo->GetDupInfo()->GetName() :
		nullptr;
	bool changed = false;
	int index = 0;

	// traversing in a reverse order to delete items in order they were added to history
	// (just to produce the log-messages in a more logical order)
	for (HistoryList::reverse_iterator it = downloadQueue->GetHistory()->rbegin(); it != downloadQueue->GetHistory()->rend(); )
	{
		HistoryInfo* historyInfo = (*it).get();

		bool isNzbOrUrl = historyInfo->GetKind() == HistoryInfo::hkNzb ||
			historyInfo->GetKind() == HistoryInfo::hkUrl;
		if (isNzbOrUrl && nzbget_rs_dcf_dupecoord_should_hide_history_dupe(
			(int)historyInfo->GetKind(),
			(int)HistoryInfo::hkNzb,
			(int)HistoryInfo::hkUrl,
			(int)historyInfo->GetNzbInfo()->GetDupeMode(),
			(int)dmForce,
			(int)historyInfo->GetNzbInfo()->GetDeleteStatus(),
			(int)NzbInfo::dsDupe,
			historyInfo == markHistoryInfo ? 1 : 0,
			SameNameOrKey(historyInfo->GetNzbInfo()->GetName(), historyInfo->GetNzbInfo()->GetDupeKey(), nzbName, dupeKey) ? 1 : 0) != 0)
		{
			g_HistoryCoordinator->HistoryHide(downloadQueue, historyInfo, index);
			index++;
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

DupeCoordinator::EDupeStatus DupeCoordinator::GetDupeStatus(DownloadQueue* downloadQueue,
	const char* name, const char* dupeKey)
{
	EDupeStatus statuses = dsNone;

	// find duplicates in download queue
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		statuses = (EDupeStatus)nzbget_rs_pnd_dupecoord_accum_queue_dupe_status(
			(int)statuses,
			SameNameOrKey(name, dupeKey, nzbInfo->GetName(), nzbInfo->GetDupeKey()) ? 1 : 0,
			nzbInfo->GetSuccessArticles(),
			nzbInfo->GetFailedArticles(),
			(int)dsDownloading,
			(int)dsQueued);
	}

	// find duplicates in history
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			SameNameOrKey(name, dupeKey, historyInfo->GetNzbInfo()->GetName(), historyInfo->GetNzbInfo()->GetDupeKey()))
		{
			const char* textStatus = historyInfo->GetNzbInfo()->MakeTextStatus(true);
			statuses = (EDupeStatus)(statuses | nzbget_rs_dupecoord_status_from_text(textStatus));
		}

		if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			statuses = (EDupeStatus)nzbget_rs_pnd_dupecoord_accum_dup_history_status(
				(int)statuses,
				SameNameOrKey(name, dupeKey, historyInfo->GetDupInfo()->GetName(), historyInfo->GetDupInfo()->GetDupeKey()) ? 1 : 0,
				(int)historyInfo->GetDupInfo()->GetStatus(),
				(int)DupInfo::dsSuccess, (int)DupInfo::dsGood,
				(int)DupInfo::dsFailed, (int)DupInfo::dsBad,
				(int)dsSuccess, (int)dsFailure);
		}
	}

	return statuses;
}

RawNzbList DupeCoordinator::ListHistoryDupes(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	RawNzbList dupeList;

	if (nzbInfo->GetDupeMode() == dmForce)
	{
		return dupeList;
	}

	// find duplicates in history
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			nzbget_rs_dqh_dupe_fallback_is_history_donor_eligible(
				(int)historyInfo->GetKind(),
				(int)HistoryInfo::hkNzb,
				(int)historyInfo->GetNzbInfo()->GetDupeMode(),
				(int)dmForce) != 0 &&
			SameNameOrKey(historyInfo->GetNzbInfo()->GetName(), historyInfo->GetNzbInfo()->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			dupeList.push_back(historyInfo->GetNzbInfo());
		}
	}

	return dupeList;
}

int DupeCoordinator::AlivePermille(NzbInfo* nzbInfo, const char* name)
{
	NzbParameter* parameter = nzbInfo->GetParameters()->Find(name);
	if (!parameter || !parameter->GetValue())
	{
		return -1;
	}
	return nzbget_rs_dupecoord_parse_alive_permille(parameter->GetValue());
}
