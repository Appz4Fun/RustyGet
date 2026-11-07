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


#ifndef DUPECOORDINATOR_H
#define DUPECOORDINATOR_H

#include "DownloadInfo.h"

class DupeCoordinator
{
public:
	enum EDupeStatus
	{
		dsNone = 0,
		dsQueued = 1,
		dsDownloading = 2,
		dsSuccess = 4,
		dsWarning = 8,
		dsFailure = 16
	};

	void NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void HistoryMark(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, NzbInfo::EMarkStatus markStatus);
	EDupeStatus GetDupeStatus(DownloadQueue* downloadQueue, const char* name, const char* dupeKey);
	RawNzbList ListHistoryDupes(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	static bool SameNameOrKey(const char* name1, const char* dupeKey1, const char* name2, const char* dupeKey2);
	/* The dupe-backup in history ReturnBestDupe would move to the queue for
	 * this title (nullptr if none, or if a good-duplicate already exists). */
	HistoryInfo* FindDupeBackup(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey);
	/* Whether a queued item at the given health (permille) and duplicate score
	 * should be abandoned early for a backup with the given score (option
	 * <HealthCheck> value "dupe"): the backup's score must not fall below the
	 * score the item still warrants at its remaining health. */
	static bool DupeFailoverWarranted(int itemScore, int health, int backupScore);
	/* a dupe tool's health check of a duplicate (parameter <name>, in percent,
	 * fractions allowed) in per-mille, clamped to 0..1000; -1 if not set or not a number */
	static int AlivePermille(NzbInfo* nzbInfo, const char* name = "DupeAlive");
	/* the download gets a duplicate in its place when it fails or turns out dead:
	 * DupeMode score; with HealthCheck=dupe a forced one too - it never fails
	 * outright while a viable duplicate waits (B46) */
	static bool FailsOver(NzbInfo* nzbInfo);
	/* With HealthCheck=dupe: a successful item of the same (non-empty) duplicate key
	 * whose files are still on disk, or nullptr (B70, B77). Within DownloadQueue-lock. */
	static NzbInfo* DownloadedOnDisk(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	/* the item's final (or destination) directory holds a file other than a side file */
	static bool FilesOnDisk(NzbInfo* nzbInfo);
	/* why no duplicate in history can take the download's place, for the log */
	std::string NoBackupReason(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);

private:
	void ReturnBestDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey);
	void HistoryCleanup(DownloadQueue* downloadQueue, HistoryInfo* markHistoryInfo);
};

extern DupeCoordinator* g_DupeCoordinator;

#include <algorithm>
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "HistoryCoordinator.h"
#include "QueueScript.h"
#include "nzbget_rs.h"

inline bool DupeCoordinator::SameNameOrKey(const char* name1, const char* dupeKey1,
	const char* name2, const char* dupeKey2)
{
	return nzbget_rs_dupecoord_same_name_or_key(name1, dupeKey1, name2, dupeKey2) != 0;
}

inline bool DupeCoordinator::FilesOnDisk(NzbInfo* nzbInfo)
{
	return nzbget_rs_dupecoord_files_on_disk(
		nzbInfo->GetFinalDir(),
		nzbInfo->GetDestDir(),
		g_Options->GetExtCleanupDisk()) != 0;
}

inline NzbInfo* DupeCoordinator::DownloadedOnDisk(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	const int healthCheck = (int)g_Options->GetHealthCheck();
	const char* targetDupeKey = nzbInfo->GetDupeKey();
	const char* extCleanupDisk = g_Options->GetExtCleanupDisk();
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() != HistoryInfo::hkNzb)
		{
			continue;
		}
		NzbInfo* item = historyInfo->GetNzbInfo();
		if (nzbget_rs_dupecoord_is_downloaded_on_disk_candidate(
			healthCheck,
			targetDupeKey,
			item == nzbInfo ? 1 : 0,
			(int)item->GetDeleteStatus(),
			item->IsDupeSuccess() ? 1 : 0,
			(int)item->GetMarkStatus(),
			item->GetDupeKey(),
			item->GetFinalDir(),
			item->GetDestDir(),
			extCleanupDisk) != 0)
		{
			return item;
		}
	}
	return nullptr;
}

inline void DupeCoordinator::NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Checking duplicates for %s", nzbInfo->GetName());

	// the release is already downloaded and on disk: another posting of it isn't
	// needed (B70: Knife Edge S02E01 - asked again after its success, a different
	// posting of the same release, scored higher, downloaded a second copy). Kept
	// in history as a backup, in case the download on disk is deleted or marked bad.
	if (nzbInfo->GetKind() == NzbInfo::nkNzb && nzbInfo->GetDupeMode() != dmForce)
	{
		if (NzbInfo* downloaded = DownloadedOnDisk(downloadQueue, nzbInfo))
		{
			nzbInfo->PrintMessage(Message::mkInfo, "Not downloading %s: %s is downloaded and on disk",
				nzbInfo->GetName(), downloaded->GetName());
			nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
			return;
		}
	}

	auto sameContentHash = [](uint32 full1, uint32 filtered1, uint32 full2, uint32 filtered2) -> bool
		{
			return (full1 > 0 && full1 == full2) || (filtered1 > 0 && filtered1 == filtered2);
		};

	// find duplicates in download queue with exactly same content
	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		bool sameContent = sameContentHash(
			nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
			queuedNzbInfo->GetFullContentHash(), queuedNzbInfo->GetFilteredContentHash());

		// if there is a duplicate with exactly same content (via hash-check)
		// in queue - the new item is skipped
		if (queuedNzbInfo != nzbInfo && sameContent && nzbInfo->GetKind() == NzbInfo::nkNzb)
		{
			NzbgetRsBuf msgBuf = nzbget_rs_dupecoord_format_dupe_skip_message(
				nzbInfo->GetName(), queuedNzbInfo->GetName(), 0, 1, 0);
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
	if (Util::EmptyStr(nzbInfo->GetDupeKey()) && nzbInfo->GetDupeScore() == 0)
	{
		for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
		{
			if (!strcmp(queuedNzbInfo->GetName(), nzbInfo->GetName()) &&
				(!Util::EmptyStr(queuedNzbInfo->GetDupeKey()) || queuedNzbInfo->GetDupeScore() != 0))
			{
				nzbInfo->SetDupeKey(queuedNzbInfo->GetDupeKey());
				nzbInfo->SetDupeScore(queuedNzbInfo->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing queue item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
		}
	}
	if (Util::EmptyStr(nzbInfo->GetDupeKey()) && nzbInfo->GetDupeScore() == 0)
	{
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
				!strcmp(historyInfo->GetNzbInfo()->GetName(), nzbInfo->GetName()) &&
				(!Util::EmptyStr(historyInfo->GetNzbInfo()->GetDupeKey()) || historyInfo->GetNzbInfo()->GetDupeScore() != 0))
			{
				nzbInfo->SetDupeKey(historyInfo->GetNzbInfo()->GetDupeKey());
				nzbInfo->SetDupeScore(historyInfo->GetNzbInfo()->GetDupeScore());
				info("Assigning dupekey %s and dupescore %i to %s from existing history item with the same name",
					 nzbInfo->GetDupeKey(), nzbInfo->GetDupeScore(), nzbInfo->GetName());
				break;
			}
			if (historyInfo->GetKind() == HistoryInfo::hkDup &&
				!strcmp(historyInfo->GetDupInfo()->GetName(), nzbInfo->GetName()) &&
				(!Util::EmptyStr(historyInfo->GetDupInfo()->GetDupeKey()) || historyInfo->GetDupInfo()->GetDupeScore() != 0))
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
			sameContentHash(
				nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
				historyInfo->GetNzbInfo()->GetFullContentHash(),
				historyInfo->GetNzbInfo()->GetFilteredContentHash()))
		{
			skip = true;
			sameContent = true;
			dupeName = historyInfo->GetNzbInfo()->GetName();
			break;
		}

		if (historyInfo->GetKind() == HistoryInfo::hkDup &&
			sameContentHash(
				nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
				historyInfo->GetDupInfo()->GetFullContentHash(),
				historyInfo->GetDupInfo()->GetFilteredContentHash()))
		{
			skip = true;
			sameContent = true;
			dupeName = historyInfo->GetDupInfo()->GetName();
			break;
		}

		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			historyInfo->GetNzbInfo()->GetDupeMode() != dmForce &&
			historyInfo->GetNzbInfo()->GetMarkStatus() == NzbInfo::ksGood &&
			SameNameOrKey(historyInfo->GetNzbInfo()->GetName(), historyInfo->GetNzbInfo()->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			skip = true;
			good = true;
			dupeName = historyInfo->GetNzbInfo()->GetName();
			break;
		}

		if (historyInfo->GetKind() == HistoryInfo::hkDup &&
			historyInfo->GetDupInfo()->GetDupeMode() != dmForce &&
			((historyInfo->GetDupInfo()->GetStatus() == DupInfo::dsGood) ||
			 (nzbInfo->GetDupeMode() == dmScore &&
			  historyInfo->GetDupInfo()->GetStatus() == DupInfo::dsSuccess &&
			  nzbInfo->GetDupeScore() <= historyInfo->GetDupInfo()->GetDupeScore())) &&
			SameNameOrKey(historyInfo->GetDupInfo()->GetName(), historyInfo->GetDupInfo()->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			skip = true;
			good = historyInfo->GetDupInfo()->GetStatus() == DupInfo::dsGood;
			dupeName = historyInfo->GetDupInfo()->GetName();
			break;
		}
	}

	// With HealthCheck=dupe (B43) the same content in history decides by what became
	// of it: only untried backups (and items the user deleted) - the nzb-file is
	// downloaded as sent, not skipped as a copy (a client re-sending it would get
	// nothing); a success - skipped, as before; a failure - skipped too (the same dead
	// posting), but the best viable backup is fetched in its place when nothing of the
	// key is queued, so the re-send still leads to a download
	bool returnBackup = false;
	if (skip && sameContent &&
		g_Options->GetHealthCheck() == Options::hcDupe &&
		nzbInfo->GetKind() == NzbInfo::nkNzb &&
		nzbInfo->GetDupeHint() == NzbInfo::dhNone)
	{
		bool succeeded = false;
		bool failed = false;
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				NzbInfo* item = historyInfo->GetNzbInfo();
				if (sameContentHash(nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
					item->GetFullContentHash(), item->GetFilteredContentHash()))
				{
					NzbInfo::EDeleteStatus del = item->GetDeleteStatus();
					if (del != NzbInfo::dsDupe && del != NzbInfo::dsCopy && del != NzbInfo::dsManual)
					{
						if (item->IsDupeSuccess())
						{
							succeeded = true;
						}
						else
						{
							failed = true;
						}
					}
				}
			}
			else if (historyInfo->GetKind() == HistoryInfo::hkDup)
			{
				DupInfo* dup = historyInfo->GetDupInfo();
				if (sameContentHash(nzbInfo->GetFullContentHash(), nzbInfo->GetFilteredContentHash(),
					dup->GetFullContentHash(), dup->GetFilteredContentHash()))
				{
					if (dup->GetStatus() == DupInfo::dsSuccess || dup->GetStatus() == DupInfo::dsGood)
					{
						succeeded = true;
					}
					else if (dup->GetStatus() == DupInfo::dsFailed || dup->GetStatus() == DupInfo::dsBad)
					{
						failed = true;
					}
				}
			}
		}
		if (!succeeded && !failed)
		{
			info("Adding %s: the same content waits in history only as an untried backup", nzbInfo->GetName());
			skip = false;
			sameContent = false;
		}
		else if (failed && !succeeded)
		{
			returnBackup = !std::any_of(downloadQueue->GetQueue()->begin(), downloadQueue->GetQueue()->end(),
				[nzbInfo](std::unique_ptr<NzbInfo>& queued)
				{
					return queued.get() != nzbInfo &&
						queued->GetDeleteStatus() == NzbInfo::dsNone &&
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
			if ((historyInfo->GetKind() == HistoryInfo::hkNzb ||
				 historyInfo->GetKind() == HistoryInfo::hkUrl) &&
				historyInfo->GetNzbInfo()->GetDupeMode() != dmForce &&
				SameNameOrKey(historyInfo->GetNzbInfo()->GetName(), historyInfo->GetNzbInfo()->GetDupeKey(),
					nzbInfo->GetName(), nzbInfo->GetDupeKey()) &&
				nzbInfo->GetDupeScore() <= historyInfo->GetNzbInfo()->GetDupeScore() &&
				historyInfo->GetNzbInfo()->IsDupeSuccess())
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
		NzbgetRsBuf msgBuf = nzbget_rs_dupecoord_format_dupe_skip_message(
			nzbInfo->GetName(), dupeName, 1, sameContent ? 1 : 0, good ? 1 : 0);
		std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
		nzbget_rs_free(msgBuf);

		if (nzbInfo->GetFeedId() && nzbInfo->GetDupeHint() == NzbInfo::dhNone)
		{
			nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
			warn("%s", message.c_str());
			g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
		}
		else
		{
			nzbInfo->SetDeleteStatus(sameContent ? NzbInfo::dsCopy : NzbInfo::dsGood);
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
			if (queuedNzbInfo != nzbInfo &&
				queuedNzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
				(queuedNzbInfo->GetKind() == NzbInfo::nkNzb ||
				 (queuedNzbInfo->GetKind() == NzbInfo::nkUrl && nzbInfo->GetKind() == NzbInfo::nkUrl)) &&
				queuedNzbInfo->GetDupeMode() != dmForce &&
				SameNameOrKey(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(),
					nzbInfo->GetName(), nzbInfo->GetDupeKey()))
			{
				// if queue has a duplicate with the same or higher score - the new item
				// is moved to history as dupe-backup. With HealthCheck=dupe so is one
				// arriving while a backup that failover returned downloads in good
				// health (B73: Las Azules S02E01 4404 lost its progress to a duplicate
				// a dupe tool appended, scored higher but no better): it is tried next
				// if that backup fails too
				bool keepReturned = g_Options->GetHealthCheck() == Options::hcDupe &&
					queuedNzbInfo->GetDupeHint() == NzbInfo::dhRedownloadAuto &&
					queuedNzbInfo->GetCurrentSuccessArticles() > 0 &&
					queuedNzbInfo->CalcHealth() >= queuedNzbInfo->CalcCriticalHealth(true);
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
					if (oldSize == newSize)
					{
						index++;
					}
					it = downloadQueue->GetQueue()->begin() + index;
					index--;
				}
			}
		}
	}
}

inline void DupeCoordinator::NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Processing duplicates for %s", nzbInfo->GetName());

	if (FailsOver(nzbInfo) && !nzbInfo->IsDupeSuccess())
	{
		ReturnBestDupe(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey());
	}

	// one copy is downloaded and on disk: the others of its key still downloading,
	// repairing or checking are stopped and kept as backups (B77: Sugar S01E01 - a
	// copy kept repairing for minutes after another one had succeeded, then failed)
	bool dupeHealth = g_Options->GetHealthCheck() == Options::hcDupe ||
		g_Options->GetHealthCheck() == Options::hcPark;
	if (dupeHealth && !Util::EmptyStr(nzbInfo->GetDupeKey()) &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		nzbInfo->IsDupeSuccess() && FilesOnDisk(nzbInfo))
	{
		DownloadQueue::EEditAction deleteAction = g_Options->GetHealthCheck() == Options::hcPark
			? DownloadQueue::eaGroupParkDelete
			: DownloadQueue::eaGroupDupeDelete;
		std::vector<std::pair<int, bool>> others;	// id, in post-processing
		for (NzbInfo* queued : downloadQueue->GetQueue())
		{
			if (queued != nzbInfo &&
				queued->GetKind() == NzbInfo::nkNzb &&
				!queued->GetDeleting() &&
				queued->GetDeleteStatus() == NzbInfo::dsNone &&
				queued->GetDupeMode() != dmForce &&
				!strcasecmp(queued->GetDupeKey(), nzbInfo->GetDupeKey()))
			{
				queued->PrintMessage(Message::mkInfo, "Stopping %s: %s is downloaded and on disk",
					queued->GetName(), nzbInfo->GetName());
				queued->SetDeleteStatus(NzbInfo::dsDupe);
				others.emplace_back(queued->GetId(), queued->GetPostInfo() != nullptr);
			}
		}
		for (const auto& other : others)
		{
			downloadQueue->EditEntry(other.first,
				other.second ? DownloadQueue::eaPostDelete : deleteAction,
				nullptr);
		}
	}
}

inline std::string DupeCoordinator::NoBackupReason(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			c.dupe_mode = (int)item->GetDupeMode();
			c.mark_status = (int)item->GetMarkStatus();
			c.delete_status = (int)item->GetDeleteStatus();
			c.alive_permille = AlivePermille(item);
			c.health = item->CalcHealth();
			c.critical_health = item->CalcCriticalHealth(true);
			c.same_name_or_key = (item != nzbInfo &&
				SameNameOrKey(item->GetName(), item->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey())) ? 1 : 0;
		}
		historyCandidates.push_back(c);
	}

	auto fileExistsCb = [](void* ctx, size_t idx) -> int {
		auto* items = static_cast<std::vector<HistoryInfo*>*>(ctx);
		return FileSystem::FileExists((*items)[idx]->GetNzbInfo()->GetQueuedFilename()) ? 1 : 0;
	};

	NzbgetRsBuf buf = nzbget_rs_dupecoord_no_backup_reason(
		FailsOver(nzbInfo) ? 1 : 0,
		(int)nzbInfo->GetDupeMode(),
		historyCandidates.data(),
		historyCandidates.size(),
		&historyItems,
		fileExistsCb);
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

inline bool DupeCoordinator::FailsOver(NzbInfo* nzbInfo)
{
	return nzbget_rs_dupecoord_fails_over(
		g_Options->GetDupeCheck() ? 1 : 0, (int)nzbInfo->GetDupeMode(), (int)g_Options->GetHealthCheck()) != 0;
}

inline void DupeCoordinator::ReturnBestDupe(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey)
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

inline HistoryInfo* DupeCoordinator::FindDupeBackup(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey)
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

inline bool DupeCoordinator::DupeFailoverWarranted(int itemScore, int health, int backupScore)
{
	return nzbget_rs_dupecoord_failover_warranted(itemScore, health, backupScore) != 0;
}

inline void DupeCoordinator::HistoryMark(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, NzbInfo::EMarkStatus markStatus)
{
	const char* markStatusName = markStatus == NzbInfo::ksBad ? "bad" :
		markStatus == NzbInfo::ksGood ? "good" :
		markStatus == NzbInfo::ksSuccess ? "success" : "NONE";
	info("Marking %s as %s", historyInfo->GetName(), markStatusName);

	if (historyInfo->GetKind() == HistoryInfo::hkNzb)
	{
		historyInfo->GetNzbInfo()->SetMarkStatus(markStatus);
		g_QueueScriptCoordinator->EnqueueScript(historyInfo->GetNzbInfo(), QueueScriptCoordinator::qeNzbMarked);
	}
	else if (historyInfo->GetKind() == HistoryInfo::hkDup)
	{
		historyInfo->GetDupInfo()->SetStatus(
			markStatus == NzbInfo::ksGood ? DupInfo::dsGood :
			markStatus == NzbInfo::ksSuccess ? DupInfo::dsSuccess :
			DupInfo::dsBad);
	}
	else
	{
		error("Could not mark %s as bad: history item has wrong type", historyInfo->GetName());
		return;
	}

	if (!g_Options->GetDupeCheck() ||
		(historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetDupeMode() == dmForce) ||
		(historyInfo->GetKind() == HistoryInfo::hkDup && historyInfo->GetDupInfo()->GetDupeMode() == dmForce))
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

inline void DupeCoordinator::HistoryCleanup(DownloadQueue* downloadQueue, HistoryInfo* markHistoryInfo)
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
		NzbInfo* nzbInfo = (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
			? historyInfo->GetNzbInfo()
			: nullptr;

		if (nzbInfo && nzbget_rs_dupecoord_should_cleanup_history_dupe(
			(int)historyInfo->GetKind(),
			(int)nzbInfo->GetDupeMode(),
			(int)nzbInfo->GetDeleteStatus(),
			historyInfo == markHistoryInfo ? 1 : 0,
			nzbInfo->GetName(),
			nzbInfo->GetDupeKey(),
			nzbName,
			dupeKey) != 0)
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

inline DupeCoordinator::EDupeStatus DupeCoordinator::GetDupeStatus(DownloadQueue* downloadQueue,
	const char* name, const char* dupeKey)
{
	int statuses = (int)dsNone;

	// find duplicates in download queue
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		statuses = nzbget_rs_dupecoord_accumulate_queue_dupe_status(
			statuses,
			name,
			dupeKey,
			nzbInfo->GetName(),
			nzbInfo->GetDupeKey(),
			nzbInfo->GetSuccessArticles(),
			nzbInfo->GetFailedArticles());
	}

	// find duplicates in history
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			statuses = nzbget_rs_dupecoord_accumulate_history_dupe_status(
				statuses,
				(int)HistoryInfo::hkNzb,
				name,
				dupeKey,
				item->GetName(),
				item->GetDupeKey(),
				item->MakeTextStatus(true),
				0);
		}
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			DupInfo* dup = historyInfo->GetDupInfo();
			statuses = nzbget_rs_dupecoord_accumulate_history_dupe_status(
				statuses,
				(int)HistoryInfo::hkDup,
				name,
				dupeKey,
				dup->GetName(),
				dup->GetDupeKey(),
				nullptr,
				(int)dup->GetStatus());
		}
	}

	return (EDupeStatus)statuses;
}

inline RawNzbList DupeCoordinator::ListHistoryDupes(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	RawNzbList dupeList;

	// find duplicates in history
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		NzbInfo* item = historyInfo->GetKind() == HistoryInfo::hkNzb ? historyInfo->GetNzbInfo() : nullptr;
		if (item && nzbget_rs_dupecoord_should_list_history_dupe(
			(int)nzbInfo->GetDupeMode(),
			(int)historyInfo->GetKind(),
			(int)item->GetDupeMode(),
			item->GetName(),
			item->GetDupeKey(),
			nzbInfo->GetName(),
			nzbInfo->GetDupeKey()) != 0)
		{
			dupeList.push_back(item);
		}
	}

	return dupeList;
}

inline int DupeCoordinator::AlivePermille(NzbInfo* nzbInfo, const char* name)
{
	NzbParameter* parameter = nzbInfo->GetParameters()->Find(name);
	if (!parameter || !parameter->GetValue())
	{
		return -1;
	}
	return nzbget_rs_dupecoord_parse_alive_permille(parameter->GetValue());
}

#endif
