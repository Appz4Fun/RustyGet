/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "DownloadInfo.h"
#include "QueueEditor.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "QueueCoordinator.h"
#include "PrePostProcessor.h"
#include "HistoryCoordinator.h"
#include "UrlCoordinator.h"
#include "ParParser.h"
#include "DupeSearch.h"
#include "nzbget_rs.h"

const int MAX_ID = 1000000000;

class GroupSorter
{
public:
	GroupSorter(NzbList* nzbList, QueueEditor::ItemList* sortItemList) :
		m_nzbList(nzbList), m_sortItemList(sortItemList), m_sortCriteria(scName), m_sortOrder(soAscending) {}
	bool Execute(const char* sort);
	bool operator()(const std::unique_ptr<NzbInfo>& refNzbInfo1, const std::unique_ptr<NzbInfo>& refNzbInfo2) const;

private:
	enum ESortCriteria
	{
		scName,
		scSize,
		scRemainingSize,
		scAge,
		scCategory,
		scPriority
	};

	enum ESortOrder
	{
		soAscending,
		soDescending,
		soAuto
	};

	NzbList* m_nzbList;
	QueueEditor::ItemList* m_sortItemList;
	ESortCriteria m_sortCriteria;
	ESortOrder m_sortOrder;
};

bool GroupSorter::Execute(const char* sort)
{
	int32_t rsCriteria = 0, rsOrder = 0;
	if (!nzbget_rs_qhp_parse_sort_spec(sort, &rsCriteria, &rsOrder))
	{
		error("Could not sort groups: incorrect sort order (%s)", sort);
		return false;
	}
	m_sortCriteria = (ESortCriteria)rsCriteria;
	m_sortOrder = (ESortOrder)rsOrder;

	RawNzbList tempList;
	for (NzbInfo* nzbInfo : m_nzbList)
	{
		tempList.push_back(nzbInfo);
	}

	ESortOrder origSortOrder = m_sortOrder;
	m_sortOrder = (ESortOrder)nzbget_rs_stq_queue_editor_resolve_auto_sort_order(
		(int)m_sortOrder, (int)soAuto, (int)soDescending,
		(int)m_sortCriteria, (int)scPriority);

	std::stable_sort(m_nzbList->begin(), m_nzbList->end(), *this);

	if (origSortOrder == soAuto &&
		std::equal(tempList.begin(), tempList.end(), m_nzbList->begin(),
		[](NzbInfo* nzbInfo1, std::unique_ptr<NzbInfo>& nzbInfo2)
		{
			return nzbInfo1 == nzbInfo2.get();
		}))
	{
		m_sortOrder = (ESortOrder)nzbget_rs_stq_queue_editor_flip_sort_order(
			(int)m_sortOrder, (int)soDescending, (int)soAscending);
		std::stable_sort(m_nzbList->begin(), m_nzbList->end(), *this);
	} 

	return true;
}

bool GroupSorter::operator()(const std::unique_ptr<NzbInfo>& refNzbInfo1, const std::unique_ptr<NzbInfo>& refNzbInfo2) const
{
	NzbInfo* nzbInfo1 = refNzbInfo1.get();
	NzbInfo* nzbInfo2 = refNzbInfo2.get();

	// if list of ID is empty - sort all items
	bool initialSortState = m_sortItemList->empty();
	bool sortItem1 = initialSortState;
	bool sortItem2 = initialSortState;

	for (QueueEditor::EditItem& item : m_sortItemList)
	{
		sortItem1 |= item.m_nzbInfo == nzbInfo1;
		sortItem2 |= item.m_nzbInfo == nzbInfo2;
	}

	if (!sortItem1 || !sortItem2)
	{
		return false;
	}

	bool ret = false;

	ret = nzbget_rs_qci_group_sorter_compare_values(
		(int)m_sortCriteria,
		(int)m_sortOrder,
		(int)soDescending,
		strcmp(nzbInfo1->GetName(), nzbInfo2->GetName()),
		nzbInfo1->GetSize(),
		nzbInfo2->GetSize(),
		nzbInfo1->GetRemainingSize() - nzbInfo1->GetPausedSize(),
		nzbInfo2->GetRemainingSize() - nzbInfo2->GetPausedSize(),
		(int64)nzbInfo1->GetMinTime(),
		(int64)nzbInfo2->GetMinTime(),
		strcmp(nzbInfo1->GetCategory(), nzbInfo2->GetCategory()),
		nzbInfo1->GetPriority(),
		nzbInfo2->GetPriority()) != 0;

	return ret;
}


FileInfo* QueueEditor::FindFileInfo(int id)
{
	for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
	{
		FileInfo* fileInfo = nzbInfo->GetFileList()->Find(id);
		if (fileInfo)
		{
			return fileInfo;
		}
	}
	return nullptr;
}

void QueueEditor::PauseUnpauseEntry(FileInfo* fileInfo, bool pause)
{
	fileInfo->SetPaused(pause);
}

void QueueEditor::DeleteEntry(FileInfo* fileInfo)
{
	if (!fileInfo->GetDeleted())
	{
		fileInfo->GetNzbInfo()->PrintMessage(
			fileInfo->GetNzbInfo()->GetDeleting() ? Message::mkDetail : Message::mkInfo,
			"Deleting file %s from download queue", fileInfo->GetFilename());
		g_QueueCoordinator->DeleteQueueEntry(m_downloadQueue, fileInfo);
	}
}

void QueueEditor::MoveEntry(FileInfo* fileInfo, int offset)
{
	int entry = 0;
	for (FileInfo* fileInfo2 : fileInfo->GetNzbInfo()->GetFileList())
	{
		if (fileInfo2 == fileInfo)
		{
			break;
		}
		entry++;
	}

	int size = (int)fileInfo->GetNzbInfo()->GetFileList()->size();
	int newEntry = 0;
	if (nzbget_rs_ccm_queue_editor_clamp_move_target(entry, offset, size, &newEntry) != 0)
	{
		std::unique_ptr<FileInfo> movedFileInfo = std::move(*(fileInfo->GetNzbInfo()->GetFileList()->begin() + entry));
		fileInfo->GetNzbInfo()->GetFileList()->erase(fileInfo->GetNzbInfo()->GetFileList()->begin() + entry);
		fileInfo->GetNzbInfo()->GetFileList()->insert(fileInfo->GetNzbInfo()->GetFileList()->begin() + newEntry, std::move(movedFileInfo));
	}
}

void QueueEditor::MoveGroup(NzbInfo* nzbInfo, int offset)
{
	int entry = 0;
	for (NzbInfo* nzbInfo2 : m_downloadQueue->GetQueue())
	{
		if (nzbInfo2 == nzbInfo)
		{
			break;
		}
		entry++;
	}

	int size = (int)m_downloadQueue->GetQueue()->size();
	int newEntry = 0;
	if (nzbget_rs_ccm_queue_editor_clamp_move_target(entry, offset, size, &newEntry) != 0)
	{
		std::unique_ptr<NzbInfo> movedNzbInfo = std::move(*(m_downloadQueue->GetQueue()->begin() + entry));
		m_downloadQueue->GetQueue()->erase(m_downloadQueue->GetQueue()->begin() + entry);
		m_downloadQueue->GetQueue()->insert(m_downloadQueue->GetQueue()->begin() + newEntry, std::move(movedNzbInfo));
	}
}

bool QueueEditor::EditEntry(DownloadQueue* downloadQueue, int ID, DownloadQueue::EEditAction action, const char* args)
{
	m_downloadQueue = downloadQueue;
	IdList cIdList;
	cIdList.push_back(ID);
	return InternEditList(nullptr, &cIdList, action, args);
}

bool QueueEditor::EditList(DownloadQueue* downloadQueue, IdList* idList, NameList* nameList, DownloadQueue::EMatchMode matchMode,
	DownloadQueue::EEditAction action, const char* args)
{
	if (nzbget_rs_pup_queue_editor_is_history_or_post_without_ids(
		(int)action, (int)DownloadQueue::eaPostDelete,
		(int)DownloadQueue::eaHistoryDelete, (int)DownloadQueue::eaHistorySetName,
		idList != nullptr ? 1 : 0) != 0)
	{
		// by name (no ids) these never had a way to match and used the missing
		// id list; take names for the queue only
		return false;
	}

	if (action == DownloadQueue::eaPostDelete)
	{
		return g_PrePostProcessor->EditList(downloadQueue, idList, action, args);
	}
	else if (DownloadQueue::eaHistoryDelete <= action && action <= DownloadQueue::eaHistorySetName)
	{
		return g_HistoryCoordinator->EditList(downloadQueue, idList, action, args);
	}

	m_downloadQueue = downloadQueue;
	bool ok = true;

	std::unique_ptr<IdList> nameIdList;

	if (nameList)
	{
		nameIdList = std::make_unique<IdList>();
		idList = nameIdList.get();
		ok = BuildIdListFromNameList(idList, nameList, matchMode, action);
	}

	ok = ok && (InternEditList(nullptr, idList, action, args) || matchMode == DownloadQueue::mmRegEx);

	m_downloadQueue->Save();

	return ok;
}

bool QueueEditor::InternEditList(ItemList* itemList,
	IdList* idList, DownloadQueue::EEditAction action, const char* args)
{
	// an id given twice was edited twice: a merge or file delete used the item it had
	// just freed
	IdList uniqueIds;
	if (idList)
	{
		for (int id : *idList)
		{
			if (std::find(uniqueIds.begin(), uniqueIds.end(), id) == uniqueIds.end())
			{
				uniqueIds.push_back(id);
			}
		}
		idList = &uniqueIds;
	}

	ItemList workItems;
	if (!itemList)
	{
		itemList = &workItems;
		int offset = args && (action == DownloadQueue::eaFileMoveOffset ||
			action == DownloadQueue::eaGroupMoveOffset) ? atoi(args) : 0;
		PrepareList(itemList, idList, action, offset);
	}

	switch (action)
	{
		case DownloadQueue::eaFilePauseAllPars:
		case DownloadQueue::eaFilePauseExtraPars:
		{
			// PauseParsInGroups takes the items off the list: the result is
			// whether there were any (an empty list answered false after pausing)
			bool any = !itemList->empty();
			PauseParsInGroups(itemList, action == DownloadQueue::eaFilePauseExtraPars);
			return any;
		}

		case DownloadQueue::eaGroupMerge:
			return MergeGroups(itemList);

		case DownloadQueue::eaGroupSort:
			return SortGroups(itemList, args);

		case DownloadQueue::eaGroupMoveAfter:
		case DownloadQueue::eaGroupMoveBefore:
			return MoveGroupsTo(itemList, idList, action == DownloadQueue::eaGroupMoveBefore, args);

		case DownloadQueue::eaFileSplit:
			return SplitGroup(itemList, args);

		case DownloadQueue::eaFileReorder:
			ReorderFiles(itemList);
			break;

		default:
			for (EditItem& item : itemList)
			{
				switch (action)
				{
					case DownloadQueue::eaFilePause:
						PauseUnpauseEntry(item.m_fileInfo, true);
						break;

					case DownloadQueue::eaFileResume:
						PauseUnpauseEntry(item.m_fileInfo, false);
						break;

					case DownloadQueue::eaFileMoveOffset:
					case DownloadQueue::eaFileMoveTop:
					case DownloadQueue::eaFileMoveBottom:
						MoveEntry(item.m_fileInfo, item.m_offset);
						break;

					case DownloadQueue::eaFileDelete:
						DeleteEntry(item.m_fileInfo);
						break;

					case DownloadQueue::eaGroupSetPriority:
						SetNzbPriority(item.m_nzbInfo, args);
						break;

					case DownloadQueue::eaGroupSetCategory:
					case DownloadQueue::eaGroupApplyCategory:
						SetNzbCategory(item.m_nzbInfo, args, action == DownloadQueue::eaGroupApplyCategory);
						break;

					case DownloadQueue::eaGroupSetName:
						SetNzbName(item.m_nzbInfo, args);
						break;

					case DownloadQueue::eaGroupSetDupeKey:
					case DownloadQueue::eaGroupSetDupeScore:
					case DownloadQueue::eaGroupSetDupeMode:
						SetNzbDupeParam(item.m_nzbInfo, action, args);
						break;

					case DownloadQueue::eaGroupSetParameter:
						SetNzbParameter(item.m_nzbInfo, args);
						break;

					case DownloadQueue::eaGroupMoveTop:
					case DownloadQueue::eaGroupMoveBottom:
					case DownloadQueue::eaGroupMoveOffset:
						MoveGroup(item.m_nzbInfo, item.m_offset);
						break;

					case DownloadQueue::eaGroupPause:
					case DownloadQueue::eaGroupResume:
					case DownloadQueue::eaGroupPauseAllPars:
					case DownloadQueue::eaGroupPauseExtraPars:
						EditGroup(item.m_nzbInfo, action, args);
						break;

					case DownloadQueue::eaGroupDelete:
					case DownloadQueue::eaGroupParkDelete:
					case DownloadQueue::eaGroupDupeDelete:
					case DownloadQueue::eaGroupFinalDelete:
						if (item.m_nzbInfo->GetKind() == NzbInfo::nkUrl)
						{
							DeleteUrl(item.m_nzbInfo, action);
						}
						else
						{
							EditGroup(item.m_nzbInfo, action, args);
						}
						break;

					case DownloadQueue::eaGroupSortFiles:
						SortGroupFiles(item.m_nzbInfo);
						break;

					default:
						// suppress compiler warning "enumeration not handled in switch"
						break;
				}
			}
	}

	return itemList->size() > 0;
}

void QueueEditor::PrepareList(ItemList* itemList, IdList* idList,
	DownloadQueue::EEditAction action, int offset)
{
	if (!itemList || !idList)
	{
		return;
	}

	offset = nzbget_rs_stq_queue_editor_resolve_prepare_list_offset(
		(int)action, (int)DownloadQueue::eaFileMoveTop, (int)DownloadQueue::eaGroupMoveTop,
		(int)DownloadQueue::eaFileMoveBottom, (int)DownloadQueue::eaGroupMoveBottom,
		MAX_ID, offset);

	itemList->reserve(idList->size());
	if ((offset != 0) &&
		(action == DownloadQueue::eaFileMoveOffset || action == DownloadQueue::eaFileMoveTop || action == DownloadQueue::eaFileMoveBottom))
	{
		// add IDs to list in order they currently have in download queue
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			int nrEntries = (int)nzbInfo->GetFileList()->size();
			int lastDestPos = -1;
			int start, end, step;
			nzbget_rs_stq_queue_editor_calc_move_sweep_bounds(
				nrEntries, offset, 0, &start, &end, &step);
			for (int index = start; index != end; index += step)
			{
				std::unique_ptr<FileInfo>& fileInfo = nzbInfo->GetFileList()->at(index);
				IdList::iterator it2 = std::find(idList->begin(), idList->end(), fileInfo->GetId());
				if (it2 != idList->end())
				{
					int workOffset = 0;
					nzbget_rs_qhp_clamp_move_offset(index, nrEntries, offset, lastDestPos, &workOffset, &lastDestPos);
					itemList->emplace_back(fileInfo.get(), nullptr, workOffset);
				}
			}
		}
	}
	else if (((offset != 0) &&
		(action == DownloadQueue::eaGroupMoveOffset || action == DownloadQueue::eaGroupMoveTop || action == DownloadQueue::eaGroupMoveBottom)) ||
		action == DownloadQueue::eaGroupMoveBefore || action == DownloadQueue::eaGroupMoveAfter)
	{
		// add IDs to list in order they currently have in download queue
		int nrEntries = (int)m_downloadQueue->GetQueue()->size();
		int lastDestPos = -1;
		int start, end, step;
		nzbget_rs_stq_queue_editor_calc_move_sweep_bounds(
			nrEntries, offset, 1, &start, &end, &step);
		for (int index = start; index != end; index += step)
		{
			std::unique_ptr<NzbInfo>& nzbInfo = m_downloadQueue->GetQueue()->at(index);
			IdList::iterator it2 = std::find(idList->begin(), idList->end(), nzbInfo->GetId());
			if (it2 != idList->end())
			{
				int workOffset = 0;
				nzbget_rs_qhp_clamp_move_offset(index, nrEntries, offset, lastDestPos, &workOffset, &lastDestPos);
				itemList->emplace_back(nullptr, nzbInfo.get(), workOffset);
			}
		}
	}
	else if (action < DownloadQueue::eaGroupMoveOffset)
	{
		// check ID range
		int maxId = 0;
		int minId = MAX_ID;
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				int ID = fileInfo->GetId();
				nzbget_rs_uqs_queue_editor_update_min_max_id(ID, minId, maxId, &minId, &maxId);
			}
		}

		//add IDs to list in order they were transmitted in command
		for (int id : *idList)
		{
			if (nzbget_rs_uqs_queue_editor_is_id_in_bounds(id, minId, maxId) != 0)
			{
				FileInfo* fileInfo = FindFileInfo(id);
				if (fileInfo)
				{
					itemList->emplace_back(fileInfo, nullptr, offset);
				}
			}
		}
	}
	else
	{
		// check ID range
		int maxId = 0;
		int minId = MAX_ID;
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			int ID = nzbInfo->GetId();
			nzbget_rs_uqs_queue_editor_update_min_max_id(ID, minId, maxId, &minId, &maxId);
		}

		//add IDs to list in order they were transmitted in command
		for (int id : *idList)
		{
			if (nzbget_rs_uqs_queue_editor_is_id_in_bounds(id, minId, maxId) != 0)
			{
				for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
				{
					if (id == nzbInfo->GetId())
					{
						itemList->emplace_back(nullptr, nzbInfo, offset);
					}
				}
			}
		}
	}
}

bool QueueEditor::BuildIdListFromNameList(IdList* idList, NameList* nameList, DownloadQueue::EMatchMode matchMode, DownloadQueue::EEditAction action)
{
#ifndef HAVE_REGEX_H
	if (matchMode == DownloadQueue::mmRegEx)
	{
		return false;
	}
#endif

	std::set<int> uniqueIds;

	for (CString& name : nameList)
	{
		std::unique_ptr<RegEx> regEx;
		if (matchMode == DownloadQueue::mmRegEx)
		{
			regEx = std::make_unique<RegEx>(name);
			if (!regEx->IsValid())
			{
				return false;
			}
		}

		bool found = false;

		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (action < DownloadQueue::eaGroupMoveOffset)
				{
					// file action
					BString<1024> filename("%s/%s", fileInfo->GetNzbInfo()->GetName(), FileSystem::BaseFileName(fileInfo->GetFilename()));
					if (nzbget_rs_uqs_queue_editor_matches_name_filter(
						regEx != nullptr ? 1 : 0,
						regEx ? (regEx->Match(filename) ? 1 : 0) : 0,
						filename,
						name,
						uniqueIds.find(fileInfo->GetId()) != uniqueIds.end() ? 1 : 0) != 0)
					{
						uniqueIds.insert(fileInfo->GetId());
						idList->push_back(fileInfo->GetId());
						found = true;
					}
				}
			}

			if (action >= DownloadQueue::eaGroupMoveOffset)
			{
				// group action
				const char *filename = nzbInfo->GetName();
				if (nzbget_rs_uqs_queue_editor_matches_name_filter(
					regEx != nullptr ? 1 : 0,
					regEx ? (regEx->Match(filename) ? 1 : 0) : 0,
					filename,
					name,
					uniqueIds.find(nzbInfo->GetId()) != uniqueIds.end() ? 1 : 0) != 0)
				{
					uniqueIds.insert(nzbInfo->GetId());
					idList->push_back(nzbInfo->GetId());
					found = true;
				}
			}
		}

		if (!found && (matchMode == DownloadQueue::mmName))
		{
			return false;
		}
	}

	return true;
}

bool QueueEditor::EditGroup(NzbInfo* nzbInfo, DownloadQueue::EEditAction action, const char* args)
{
	ItemList itemList;
	bool allPaused = true;
	int id = nzbInfo->GetId();

	// collecting files belonging to group
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		itemList.emplace_back(fileInfo, nullptr, 0);
		allPaused &= fileInfo->GetPaused();
	}

	bool isGroupDel = nzbget_rs_uqs_queue_editor_is_group_delete_action(
		(int)action,
		(int)DownloadQueue::eaGroupDelete,
		(int)DownloadQueue::eaGroupParkDelete,
		(int)DownloadQueue::eaGroupDupeDelete,
		(int)DownloadQueue::eaGroupFinalDelete) != 0;
	if (isGroupDel)
	{
		nzbInfo->SetDeleting(true);
		int parking = 0, avoidHistory = 0, setDupeStatus = 0, cleanupDisk = 0;
		nzbget_rs_ccm_queue_editor_eval_group_delete_flags(
			(int)action,
			(int)DownloadQueue::eaGroupParkDelete,
			(int)DownloadQueue::eaGroupDupeDelete,
			(int)DownloadQueue::eaGroupFinalDelete,
			g_Options->GetKeepHistory(),
			nzbInfo->GetUnpackCleanedUpDisk() ? 1 : 0,
			nzbInfo->GetCurrentSuccessArticles(),
			&parking, &avoidHistory, &setDupeStatus, &cleanupDisk);
		nzbInfo->SetParking(parking != 0);
		nzbInfo->SetAvoidHistory(avoidHistory != 0);
		nzbInfo->SetDeletePaused(allPaused);
		if (setDupeStatus)
		{
			nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
		}
		nzbInfo->SetCleanupDisk(cleanupDisk != 0);
	}

	DownloadQueue::EEditAction GroupToFileMap[] = {
		(DownloadQueue::EEditAction)0,
		DownloadQueue::eaFileMoveOffset,
		DownloadQueue::eaFileMoveTop,
		DownloadQueue::eaFileMoveBottom,
		DownloadQueue::eaFilePause,
		DownloadQueue::eaFileResume,
		DownloadQueue::eaFileDelete,
		DownloadQueue::eaFilePauseAllPars,
		DownloadQueue::eaFilePauseExtraPars,
		DownloadQueue::eaFileReorder,
		DownloadQueue::eaFileSplit,
		DownloadQueue::eaFileMoveOffset,
		DownloadQueue::eaFileMoveTop,
		DownloadQueue::eaFileMoveBottom,
		(DownloadQueue::EEditAction)0,
		(DownloadQueue::EEditAction)0,
		DownloadQueue::eaFilePause,
		DownloadQueue::eaFileResume,
		DownloadQueue::eaFileDelete,
		DownloadQueue::eaFileDelete,
		DownloadQueue::eaFileDelete,
		DownloadQueue::eaFileDelete,
		DownloadQueue::eaFilePauseAllPars,
		DownloadQueue::eaFilePauseExtraPars,
		(DownloadQueue::EEditAction)0,
		(DownloadQueue::EEditAction)0,
		(DownloadQueue::EEditAction)0,
		(DownloadQueue::EEditAction)0 };

	bool ok = InternEditList(&itemList, nullptr, GroupToFileMap[action], args);

	if (isGroupDel &&
		// NZBInfo could have been destroyed already
		m_downloadQueue->GetQueue()->Find(id))
	{
		DownloadQueue::Aspect deleteAspect = { DownloadQueue::eaNzbDeleted, m_downloadQueue, nzbInfo, nullptr };
		m_downloadQueue->Notify(&deleteAspect);
	}

	return ok;
}

void QueueEditor::PauseParsInGroups(ItemList* itemList, bool extraParsOnly)
{
	while (true)
	{
		RawFileList GroupFileList;
		FileInfo* firstFileInfo = nullptr;

		for (ItemList::iterator it = itemList->begin(); it != itemList->end(); )
		{
			EditItem& item = *it;
			if (!firstFileInfo ||
				(firstFileInfo->GetNzbInfo() == item.m_fileInfo->GetNzbInfo()))
			{
				GroupFileList.push_back(item.m_fileInfo);
				if (!firstFileInfo)
				{
					firstFileInfo = item.m_fileInfo;
				}
				itemList->erase(it);
				it = itemList->begin();
				continue;
			}
			++it;
		}

		if (!GroupFileList.empty())
		{
			PausePars(&GroupFileList, extraParsOnly);
		}
		else
		{
			break;
		}
	}
}

/**
* If the parameter "bExtraParsOnly" is set to "false", then we pause all par2-files.
* If the parameter "bExtraParsOnly" is set to "true", we use the following strategy:
* At first we find all par-files, which do not have "vol" in their names, then we pause
* all vols and do not affect all just-pars.
* In a case, if there are no just-pars, but only vols, we find the smallest vol-file
* and do not affect it, but pause all other pars.
*/
void QueueEditor::PausePars(RawFileList* fileList, bool extraParsOnly)
{
	debug("QueueEditor: Pausing pars");

	size_t count = fileList->size();
	std::vector<uint8_t> rsIsPar;
	std::vector<uint8_t> rsHasVol;
	std::vector<int64_t> rsSizes;
	rsIsPar.reserve(count);
	rsHasVol.reserve(count);
	rsSizes.reserve(count);
	std::vector<uint8_t> rsOutPause(count, 0);
	for (FileInfo* fileInfo : fileList)
	{
		rsIsPar.push_back(fileInfo->GetParFile() ? 1 : 0);
		BString<1024> loFileName = fileInfo->GetFilename();
		for (char* p = loFileName; *p; p++) *p = tolower(*p);
		rsHasVol.push_back(strstr(loFileName, ".vol") != nullptr ? 1 : 0);
		rsSizes.push_back(fileInfo->GetSize());
	}
	nzbget_rs_qhp_select_pars_to_pause(
		rsIsPar.empty() ? nullptr : rsIsPar.data(),
		rsHasVol.empty() ? nullptr : rsHasVol.data(),
		rsSizes.empty() ? nullptr : rsSizes.data(),
		count,
		extraParsOnly ? 1 : 0,
		rsOutPause.empty() ? nullptr : rsOutPause.data()
	);
	for (size_t i = 0; i < count; i++)
	{
		if (rsOutPause[i])
		{
			(*fileList)[i]->SetPaused(true);
		}
	}
}

void QueueEditor::SetNzbPriority(NzbInfo* nzbInfo, const char* priority)
{
	debug("Setting priority %s for %s", priority, nzbInfo->GetName());

	int priorityVal = atoi(priority);
	nzbInfo->SetPriority(priorityVal);
}

void QueueEditor::SetNzbCategory(NzbInfo* nzbInfo, const char* category, bool applyParams)
{
	debug("QueueEditor: setting category '%s' for '%s'", category, nzbInfo->GetName());

	bool oldUnpack = g_Options->GetUnpack();
	const char* oldExtensions = g_Options->GetExtensions();
	if (applyParams && !Util::EmptyStr(nzbInfo->GetCategory()))
	{
		Options::Category* categoryObj = g_Options->FindCategory(nzbInfo->GetCategory(), false);
		if (categoryObj)
		{
			oldUnpack = categoryObj->GetUnpack();
			if (!Util::EmptyStr(categoryObj->GetExtensions()))
			{
				oldExtensions = categoryObj->GetExtensions();
			}
		}
	}

	g_QueueCoordinator->SetQueueEntryCategory(m_downloadQueue, nzbInfo, category);

	if (!applyParams)
	{
		return;
	}

	bool newUnpack = g_Options->GetUnpack();
	const char* newExtensions = g_Options->GetExtensions();
	if (!Util::EmptyStr(nzbInfo->GetCategory()))
	{
		Options::Category* categoryObj = g_Options->FindCategory(nzbInfo->GetCategory(), false);
		if (categoryObj)
		{
			newUnpack = categoryObj->GetUnpack();
			if (!Util::EmptyStr(categoryObj->GetExtensions()))
			{
				newExtensions = categoryObj->GetExtensions();
			}
		}
	}

	if (oldUnpack != newUnpack)
	{
		nzbInfo->GetParameters()->SetParameter("*Unpack:", newUnpack ? "yes" : "no");
	}

	NzbgetRsBuf extDiff = nzbget_rs_qci_queue_editor_diff_category_extensions(oldExtensions, newExtensions);
	if (extDiff.data && extDiff.len > 0)
	{
		std::string_view lines(extDiff.data, extDiff.len);
		size_t pos = 0;
		while (pos < lines.size())
		{
			size_t nl = lines.find('\n', pos);
			if (nl == std::string_view::npos) break;
			std::string_view line = lines.substr(pos, nl - pos);
			size_t tab = line.find('\t');
			if (tab != std::string_view::npos)
			{
				std::string paramName(line.substr(0, tab));
				std::string paramVal(line.substr(tab + 1));
				nzbInfo->GetParameters()->SetParameter(paramName.c_str(), paramVal.c_str());
			}
			pos = nl + 1;
		}
	}
	nzbget_rs_free(extDiff);
}

void QueueEditor::SetNzbName(NzbInfo* nzbInfo, const char* name)
{
	debug("QueueEditor: renaming '%s' to '%s'", nzbInfo->GetName(), name);

    std::string previous = nzbInfo->GetName();
    g_QueueCoordinator->SetQueueEntryName(m_downloadQueue, nzbInfo, name);
    if (previous != nzbInfo->GetName() && g_DupeSearch)
        g_DupeSearch->NotifyNameChanged(nzbInfo->GetId());
}

bool QueueEditor::MergeGroups(ItemList* itemList)
{
	if (itemList->size() == 0)
	{
		return false;
	}

	bool ok = true;

	EditItem& destItem = itemList->front();

	for (EditItem& item : itemList) 
	{
		if (item.m_nzbInfo != destItem.m_nzbInfo)
		{
			debug("merge %s to %s", item.m_nzbInfo->GetFilename(), destItem.m_nzbInfo->GetFilename());
			// (MergeQueueEntries returns true on success: the result was inverted)
			if (!g_QueueCoordinator->MergeQueueEntries(m_downloadQueue, destItem.m_nzbInfo, item.m_nzbInfo))
			{
				ok = false;
			}
		}
	}

	return ok;
}

bool QueueEditor::SplitGroup(ItemList* itemList, const char* name)
{
	if (itemList->size() == 0)
	{
		return false;
	}

	RawFileList fileList;

	for (EditItem& item : itemList)
	{
		fileList.push_back(item.m_fileInfo);
	}

	NzbInfo* newNzbInfo = nullptr;
	bool ok = g_QueueCoordinator->SplitQueueEntries(m_downloadQueue, &fileList, name, &newNzbInfo);

	return ok;
}

bool QueueEditor::SortGroups(ItemList* itemList, const char* sort)
{
	AlignGroups(itemList);
	if (!sort)
	{
		return false;
	}
	GroupSorter sorter(m_downloadQueue->GetQueue(), itemList);
	return sorter.Execute(sort);
}

void QueueEditor::AlignGroups(ItemList* itemList)
{
	NzbList* nzbList = m_downloadQueue->GetQueue();
	NzbInfo* lastNzbInfo = nullptr;
	uint32 lastNum = 0;
	uint32 num = 0;
	while (num < nzbList->size())
	{
		std::unique_ptr<NzbInfo>& nzbInfo = nzbList->at(num);

		bool selected = false;
		for (QueueEditor::EditItem& item : itemList)
		{
			if (item.m_nzbInfo == nzbInfo.get())
			{
				selected = true;
				break;
			}
		}

		if (selected)
		{
			uint32 newLastNum = lastNum;
			if (nzbget_rs_stq_queue_editor_eval_align_group_step(
				selected ? 1 : 0, lastNzbInfo != nullptr ? 1 : 0,
				num, lastNum, &newLastNum) != 0)
			{
				std::unique_ptr<NzbInfo> movedNzbInfo = std::move(*(nzbList->begin() + num));
				nzbList->erase(nzbList->begin() + num);
				nzbList->insert(nzbList->begin() + lastNum + 1, std::move(movedNzbInfo));
			}
			lastNum = newLastNum;
			lastNzbInfo = nzbInfo.get();
		}
		num++;
	}
}

bool QueueEditor::ItemListContainsItem(ItemList* itemList, int id)
{
	return std::find_if(itemList->begin(), itemList->end(),
		[id](const EditItem& item)
		{
			return item.m_nzbInfo->GetId() == id;
		}) != itemList->end();
}

bool QueueEditor::MoveGroupsTo(ItemList* itemList, IdList* idList, bool before, const char* args)
{
	if (itemList->size() == 0 || Util::EmptyStr(args))
	{
		return false;
	}

	int targetId = atoi(args);
	int offset = 0;

	// a target that isn't queued: the offset below counted to the end of the
	// queue, moved the groups to the bottom and reported success
	if (!m_downloadQueue->GetQueue()->Find(targetId))
	{
		return false;
	}

	// check if target is in list of moved items
	if (ItemListContainsItem(itemList, targetId))
	{
		// find the next item to use as target-before
		bool found = false;
		bool targetSet = false;

		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			if (found)
			{
				if (!ItemListContainsItem(itemList, nzbInfo->GetId()))
				{
					targetId = nzbInfo->GetId();
					before = true;
					targetSet = true;
					break;
				}
			}
			else if (targetId == nzbInfo->GetId())
			{
				found = true;
			}
		}

		if (!targetSet)
		{
			// there are no next item; move to the bottom then
			offset = MAX_ID;
		}
	}

	AlignGroups(itemList);

	if (offset == 0)
	{
		// calculate offset between first moving item and target
		int moveId = itemList->at(0).m_nzbInfo->GetId();
		std::vector<int> queueIds;
		queueIds.reserve(m_downloadQueue->GetQueue()->size());
		for (NzbInfo* nzbInfo : m_downloadQueue->GetQueue())
		{
			queueIds.push_back(nzbInfo->GetId());
		}
		offset = nzbget_rs_ccm_queue_editor_calc_move_groups_to_offset(
			queueIds.data(), queueIds.size(), targetId, moveId,
			before ? 1 : 0, itemList->size());
	}

	return InternEditList(nullptr, idList, DownloadQueue::eaGroupMoveOffset,
		CString::FormatStr("%i", offset));
}

void QueueEditor::ReorderFiles(ItemList* itemList)
{
	if (itemList->size() == 0)
	{
		return;
	}

	EditItem& firstItem = itemList->front();
	NzbInfo* nzbInfo = firstItem.m_fileInfo->GetNzbInfo();
	uint32 insertPos = 0;

	// now can reorder
	for (EditItem& item : itemList)
	{
		FileInfo* fileInfo = item.m_fileInfo;

		// move file item
		FileList::iterator it2 = nzbInfo->GetFileList()->Find(fileInfo);
		if (it2 != nzbInfo->GetFileList()->end())
		{
			std::unique_ptr<FileInfo> movedFileInfo = std::move(*it2);
			nzbInfo->GetFileList()->erase(it2);
			nzbInfo->GetFileList()->insert(nzbInfo->GetFileList()->begin() + insertPos, std::move(movedFileInfo));
			insertPos++;
		}
	}
}

void QueueEditor::SetNzbParameter(NzbInfo* nzbInfo, const char* paramString)
{
	debug("QueueEditor: setting nzb parameter '%s' for '%s'", paramString, FileSystem::BaseFileName(nzbInfo->GetFilename()));

	size_t nameLen = 0, valOff = 0;
	if (paramString && nzbget_rs_tpr_split_param_assignment(paramString, &nameLen, &valOff) != 0)
	{
		std::string name(paramString, nameLen);
		nzbInfo->GetParameters()->SetParameter(name.c_str(), paramString + valOff);
	}
	else
	{
		error("Could not set nzb parameter for %s: invalid argument: %s", nzbInfo->GetName(), paramString);
	}
}

void QueueEditor::SetNzbDupeParam(NzbInfo* nzbInfo, DownloadQueue::EEditAction action, const char* text)
{
	debug("QueueEditor: setting dupe parameter %i='%s' for '%s'", (int)action, text, nzbInfo->GetName());

	switch (action)
	{
		case DownloadQueue::eaGroupSetDupeKey:
            nzbInfo->GetParameters()->SetParameter(DupeSearch::OwnedKeyParam, nullptr);
			nzbInfo->SetDupeKey(text);
			break;

		case DownloadQueue::eaGroupSetDupeScore:
			nzbInfo->SetDupeScore(atoi(text));
			break;

		case DownloadQueue::eaGroupSetDupeMode:
			{
				int parsedMode = 0;
				if (!nzbget_rs_qhp_parse_dupe_mode(text, &parsedMode))
				{
					error("Could not set duplicate mode for %s: incorrect mode (%s)", nzbInfo->GetName(), text);
					return;
				}
				nzbInfo->SetDupeMode((EDupeMode)parsedMode);
				break;
			}

		default:
			// suppress compiler warning
			break;
	}
}

void QueueEditor::SortGroupFiles(NzbInfo* nzbInfo)
{
	debug("QueueEditor: sorting inner files for '%s'", nzbInfo->GetName());

	std::sort(nzbInfo->GetFileList()->begin(), nzbInfo->GetFileList()->end(),
		[](const std::unique_ptr<FileInfo>& fileInfo1, const std::unique_ptr<FileInfo>& fileInfo2)
		{
			bool samePar = fileInfo1->GetParFile() && fileInfo2->GetParFile() &&
				ParParser::SameParCollection(fileInfo1->GetFilename(), fileInfo2->GetFilename(),
					fileInfo1->GetFilenameConfirmed() && fileInfo2->GetFilenameConfirmed());
			return nzbget_rs_qhp_compare_inner_files(
				fileInfo1->GetFilename(),
				fileInfo1->GetParFile() ? 1 : 0,
				fileInfo1->GetSize(),
				fileInfo2->GetFilename(),
				fileInfo2->GetParFile() ? 1 : 0,
				fileInfo2->GetSize(),
				samePar ? 1 : 0
			) != 0;
		});
}

bool QueueEditor::DeleteUrl(NzbInfo* nzbInfo, DownloadQueue::EEditAction action)
{
	return g_UrlCoordinator->DeleteQueueEntry(m_downloadQueue, nzbInfo, action == DownloadQueue::eaGroupFinalDelete);
}
