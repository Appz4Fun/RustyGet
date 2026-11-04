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
#include "Repair.h"
#include "DupeCoordinator.h"
#include "ParParser.h"
#include "Options.h"
#include "WorkState.h"
#include "DiskState.h"
#include "Log.h"
#include "FileSystem.h"
#include <fstream>
#include <set>
#include <iterator>
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

#ifndef DISABLE_PARCHECK
bool RepairController::PostParChecker::RequestMorePars(int blockNeeded, int* blockFound)
{
	return m_owner->RequestMorePars(m_postInfo->GetNzbInfo(), GetParFilename(), blockNeeded, blockFound);
}

void RepairController::PostParChecker::UpdateProgress()
{
	m_owner->UpdateParCheckProgress();
}

void RepairController::PostParChecker::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, 1024, format, args);
	va_end(args);
	text[1024-1] = '\0';

	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}

void RepairController::PostParChecker::RegisterParredFile(const char* filename)
{
	m_postInfo->GetParredFiles()->push_back(filename);
}

void RepairController::PostParChecker::RegisterVerifiedFile(const char* filename)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
	StreamRepairJobList* jobs = nzbInfo->GetStreamRepairJobs();
	std::string verifiedPath = FileSystem::GetRealPath(filename).value_or(filename);
	size_t previousSize = jobs->size();
	jobs->erase(std::remove_if(jobs->begin(), jobs->end(),
		[nzbInfo, &verifiedPath](const StreamRepairJob& job)
		{
			std::string currentName = job.GetFilename();
			for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
			{
				if (completedFile.GetId() == job.GetFileId())
				{
					currentName = completedFile.GetFilename();
					break;
				}
			}
			std::string path = std::string(nzbInfo->GetDestDir()) + PATH_SEPARATOR + currentName;
			path = FileSystem::GetRealPath(path).value_or(path);
			return FileSystem::SameFilename(path.c_str(), verifiedPath.c_str());
		}), jobs->end());
	if (jobs->size() != previousSize)
	{
		nzbInfo->SetChanged(true);
	}
}

bool RepairController::PostParChecker::IsParredFile(const char* filename)
{
	for (CString& parredFile : m_postInfo->GetParredFiles())
	{
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_rrd_repair_ctrl_filename_eq_ignore_case(parredFile, filename) != 0)
#else
		if (!strcasecmp(parredFile, filename))
#endif
		{
			return true;
		}
	}
	return false;
}

ParChecker::EFileStatus RepairController::PostParChecker::FindFileCrc(const char* filename,
	uint32* crc, SegmentList* segments)
{
	// read under the queue lock and copied out: the par-check runs while other par
	// volumes complete (appending to the list) or get renamed; the file state is
	// loaded from disk after the lock is released
	int id = 0;
	CompletedFile::EStatus status;
	bool reprocess;
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		CompletedFile* completedFile = nullptr;
		for (CompletedFile& completedFile2 : *m_postInfo->GetNzbInfo()->GetCompletedFiles())
		{
#ifdef NZBGET_USE_RUST
			if (nzbget_rs_rrd_repair_ctrl_filename_eq_ignore_case(completedFile2.GetFilename(), filename) != 0)
#else
			if (!strcasecmp(completedFile2.GetFilename(), filename))
#endif
			{
				completedFile = &completedFile2;
				break;
			}
		}
		if (!completedFile)
		{
			return ParChecker::fsUnknown;
		}
		id = completedFile->GetId();
		*crc = completedFile->GetCrc();
		status = completedFile->GetStatus();
		reprocess = m_postInfo->GetNzbInfo()->GetReprocess();
	}

	debug("Found completed file: %s, CRC: %.8x, Status: %i", filename, *crc, (int)status);

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_rrd_repair_ctrl_should_load_partial_segments(
		(int)status, (int)CompletedFile::cfPartial, id, reprocess ? 1 : 0) != 0)
#else
	if (status == CompletedFile::cfPartial && id > 0 && !reprocess)
#endif
	{
		FileInfo tmpFileInfo(id);

		if (!g_DiskState->LoadFileState(&tmpFileInfo, nullptr, true))
		{
			return ParChecker::fsUnknown;
		}

		for (ArticleInfo* pa : tmpFileInfo.GetArticles())
		{
			segments->emplace_back(pa->GetStatus() == ArticleInfo::aiFinished,
				pa->GetSegmentOffset(), pa->GetSegmentSize(), pa->GetCrc());
		}
	}

#ifdef NZBGET_USE_RUST
	return (ParChecker::EFileStatus)nzbget_rs_dur_repair_ctrl_classify_file_crc_status(
		(int)status, reprocess ? 1 : 0, !segments->empty() ? 1 : 0,
		(int)CompletedFile::cfSuccess, (int)CompletedFile::cfFailure, (int)CompletedFile::cfPartial,
		(int)ParChecker::fsSuccess, (int)ParChecker::fsFailure, (int)ParChecker::fsPartial,
		(int)ParChecker::fsUnknown);
#else
	return status == CompletedFile::cfSuccess ? ParChecker::fsSuccess :
		status == CompletedFile::cfFailure && !reprocess ? ParChecker::fsFailure :
		status == CompletedFile::cfPartial && segments->size() > 0 && !reprocess ? ParChecker::fsPartial :
		ParChecker::fsUnknown;
#endif
}

const char* RepairController::PostParChecker::FindFileOrigname(const char* filename)
{
	// a copy (per thread, valid until its next call): the list's string can be
	// replaced by a rename while the caller reads it
	thread_local std::string origname;
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	for (CompletedFile& completedFile : *m_postInfo->GetNzbInfo()->GetCompletedFiles())
	{
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_rrd_repair_ctrl_filename_eq_ignore_case(completedFile.GetFilename(), filename) != 0)
#else
		if (!strcasecmp(completedFile.GetFilename(), filename))
#endif
		{
			if (!completedFile.GetOrigname())
			{
				return nullptr;
			}
			origname = completedFile.GetOrigname();
			return origname.c_str();
		}
	}

	return nullptr;
}

void RepairController::PostParChecker::RequestDupeSources(DupeSourceList* dupeSourceList)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	RawNzbList dupeList = g_DupeCoordinator->ListHistoryDupes(downloadQueue, m_postInfo->GetNzbInfo());

	if (!dupeList.empty())
	{
		PostDupeMatcher dupeMatcher(m_postInfo);
		PrintMessage(Message::mkInfo, "Checking %s for dupe scan usability", m_postInfo->GetNzbInfo()->GetName());
		bool sizeComparisonPossible = dupeMatcher.Prepare();
		for (NzbInfo* dupeNzbInfo : dupeList)
		{
			if (sizeComparisonPossible)
			{
				PrintMessage(Message::mkInfo, "Checking %s for dupe scan usability", FileSystem::BaseFileName(dupeNzbInfo->GetDestDir()));
			}
#ifdef NZBGET_USE_RUST
			bool useDupe = nzbget_rs_rrd_repair_ctrl_can_accept_dupe_without_content_match(
				sizeComparisonPossible ? 1 : 0) != 0 ||
				dupeMatcher.MatchDupeContent(dupeNzbInfo->GetDestDir());
#else
			bool useDupe = !sizeComparisonPossible || dupeMatcher.MatchDupeContent(dupeNzbInfo->GetDestDir());
#endif
			if (useDupe)
			{
				PrintMessage(Message::mkInfo, "Adding %s to dupe scan sources", FileSystem::BaseFileName(dupeNzbInfo->GetDestDir()));
				dupeSourceList->emplace_back(dupeNzbInfo->GetId(), dupeNzbInfo->GetDestDir());
			}
		}
		if (dupeSourceList->empty())
		{
			PrintMessage(Message::mkInfo, "No usable dupe scan sources found");
		}
	}
}

std::string RepairController::PostParChecker::FetchTwinFile(const TwinCheck::NzbEntry& entry, std::vector<char>* keep)
{
	std::vector<char> data;
	if (!TwinCheck::FetchEntry(m_twinFetcher, entry, 1024LL * 1024 * 1024, [this]() { return IsStopped(); }, data))
	{
		return "";
	}
	// a name of its own: the twin's par2-files may be named like ours
	std::string name = "_twin." + (entry.filename.empty() ? std::to_string(m_twinFiles.size()) + ".par2" : entry.filename);
	BString<1024> path("%s%c%s", GetDestDir(), PATH_SEPARATOR, name.c_str());
	{
		std::ofstream out(fs::u8path(*path), std::ios::binary | std::ios::trunc);
		out.write(data.data(), (std::streamsize)data.size());
		if (!out)
		{
			return "";
		}
	}
	m_twinFiles.push_back(*path);
	if (keep)
	{
		*keep = std::move(data);
	}
	return *path;
}

std::string RepairController::PostParChecker::RequestTwinIndex()
{
	// the files of our set, by the par2-file the check runs on
	std::vector<TwinCheck::FileSig> own;
	{
		std::ifstream in(fs::u8path(GetParFilename()), std::ios::binary);
		std::vector<char> data((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
		own = TwinCheck::ParsePar2(data.data(), data.size());
	}
	std::string ownPrint = TwinCheck::Fingerprint(own);
	if (ownPrint.empty())
	{
		return "";
	}

	// duplicates of its key whose fingerprint is ours, then those not fingerprinted yet
	struct Candidate
	{
		std::string name;
		std::string nzbFilename;
		bool known;
	};
	std::vector<Candidate> candidates;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		auto consider = [&](NzbInfo* other)
			{
#ifdef NZBGET_USE_RUST
				bool sameNameOrKey = other != nzbInfo && other->GetKind() == NzbInfo::nkNzb &&
					DupeCoordinator::SameNameOrKey(other->GetName(), other->GetDupeKey(),
						nzbInfo->GetName(), nzbInfo->GetDupeKey());
				NzbParameter* print = sameNameOrKey ? other->GetParameters()->Find(TwinCheck::FilesParam) : nullptr;
				if (!nzbget_rs_hsr_repair_ctrl_should_consider_twin_candidate(
					other == nzbInfo ? 1 : 0,
					(int)other->GetKind(),
					(int)NzbInfo::nkNzb,
					sameNameOrKey ? 1 : 0,
					other->GetQueuedFilename(),
					print != nullptr ? 1 : 0,
					(print && ownPrint == print->GetValue()) ? 1 : 0) ||
					!FileSystem::FileExists(other->GetQueuedFilename()))
				{
					return;
				}
#else
				if (other == nzbInfo || other->GetKind() != NzbInfo::nkNzb ||
					!DupeCoordinator::SameNameOrKey(other->GetName(), other->GetDupeKey(),
						nzbInfo->GetName(), nzbInfo->GetDupeKey()) ||
					strchr(other->GetQueuedFilename(), '|') || !FileSystem::FileExists(other->GetQueuedFilename()))
				{
					return;
				}
				NzbParameter* print = other->GetParameters()->Find(TwinCheck::FilesParam);
				if (print && ownPrint != print->GetValue())
				{
					return;	// an alt, or no par2
				}
#endif
				candidates.push_back({other->GetName(), other->GetQueuedFilename(), print != nullptr});
			};
		for (NzbInfo* queued : downloadQueue->GetQueue())
		{
			consider(queued);
		}
		for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				consider(historyInfo->GetNzbInfo());
			}
		}
	}
	std::stable_sort(candidates.begin(), candidates.end(),
		[](const Candidate& a, const Candidate& b) { return a.known > b.known; });

	std::set<std::string> ownFiles;
	for (const TwinCheck::FileSig& sig : own)
	{
		ownFiles.insert(sig.name + "/" + sig.md5);
	}

	for (const Candidate& candidate : candidates)
	{
		if (IsStopped())
		{
			break;
		}
		std::vector<TwinCheck::NzbEntry> entries = TwinCheck::ReadNzbEntries(candidate.nzbFilename.c_str());
		const TwinCheck::NzbEntry* index = nullptr;
		for (const TwinCheck::NzbEntry& entry : entries)
		{
			if (entry.par2 && (!index || entry.size < index->size))
			{
				index = &entry;
			}
		}
		if (!index || index->size > TwinCheck::MaxIndexSize)
		{
			continue;
		}

		std::vector<char> data;
		std::string path = FetchTwinFile(*index, &data);
		if (path.empty())
		{
			continue;
		}
		std::vector<TwinCheck::FileSig> sigs = TwinCheck::ParsePar2(data.data(), data.size());
		std::set<std::string> twinFiles;
		for (const TwinCheck::FileSig& sig : sigs)
		{
			twinFiles.insert(sig.name + "/" + sig.md5);
		}
		if (TwinCheck::Fingerprint(sigs) != ownPrint || twinFiles != ownFiles)
		{
			// another encode, or the same files under other names (par2 would rename
			// ours to them): not used
			PrintMessage(Message::mkDetail, "%s: its par2 set %s", candidate.name.c_str(),
				TwinCheck::Fingerprint(sigs) != ownPrint ? "describes other files" : "names the files otherwise");
			FileSystem::DeleteFile(path.c_str());
			m_twinFiles.pop_back();
			continue;
		}

		m_twinName = candidate.name;
		m_twinEntries = std::move(entries);
		m_twinIndex = index->filename;
		m_twinBlockSize = TwinCheck::BlockSize(data.data(), data.size());
		PrintMessage(Message::mkInfo, "Trying the par2 set of twin %s (blocks of %.1f MB)",
			m_twinName.c_str(), m_twinBlockSize / 1024.0 / 1024.0);
		return path;
	}
	return "";
}

std::vector<std::string> RepairController::PostParChecker::RequestTwinVolumes(int blockNeeded)
{
	// the smallest volumes that hold enough blocks: by their names, or by their sizes
	// (a recovery packet is a block and 68 bytes; nzb sizes are encoded)
	struct Volume
	{
		const TwinCheck::NzbEntry* entry;
		int blocks;
	};
	std::vector<Volume> volumes;
	for (const TwinCheck::NzbEntry& entry : m_twinEntries)
	{
		if (!entry.par2 || entry.filename == m_twinIndex)
		{
			continue;
		}
		int blocks = TwinCheck::VolumeBlocks(entry.filename);
		if (blocks < 0 && m_twinBlockSize > 0)
		{
#ifdef NZBGET_USE_RUST
			blocks = nzbget_rs_dur_repair_ctrl_estimate_twin_vol_blocks(entry.size, m_twinBlockSize);
#else
			blocks = (int)(entry.size * 100 / 102 / (int64)(m_twinBlockSize + 68));
#endif
		}
		if (blocks > 0)
		{
			volumes.push_back({&entry, blocks});
		}
	}
	std::sort(volumes.begin(), volumes.end(), [](const Volume& a, const Volume& b) { return a.blocks < b.blocks; });

#ifdef NZBGET_USE_RUST
	int available = 0;
	std::vector<int> allVolBlocks;
	allVolBlocks.reserve(volumes.size());
	for (const Volume& volume : volumes)
	{
		allVolBlocks.push_back(volume.blocks);
	}
	bool hasEnoughTwinBlocks = nzbget_rs_hsr_repair_ctrl_sum_and_check_twin_blocks(
		allVolBlocks.data(), allVolBlocks.size(), blockNeeded, &available) != 0;
	std::vector<std::string> paths;
	if (!hasEnoughTwinBlocks)
#else
	int available = 0;
	for (const Volume& volume : volumes)
	{
		available += volume.blocks;
	}
	std::vector<std::string> paths;
	if (available < blockNeeded)
#endif
	{
		PrintMessage(Message::mkInfo, "Twin %s holds %i par-block(s), %i needed: not fetched",
			m_twinName.c_str(), available, blockNeeded);
		return paths;
	}

	// the fewest bytes: the smallest volume that covers what's left, else the
	// biggest one and go on
	int left = blockNeeded;
	while (left > 0 && !volumes.empty() && !IsStopped())
	{
#ifdef NZBGET_USE_RUST
		std::vector<int> volBlocks;
		volBlocks.reserve(volumes.size());
		for (const Volume& v : volumes)
		{
			volBlocks.push_back(v.blocks);
		}
		ptrdiff_t idx = nzbget_rs_repair_pick_twin_volume(volBlocks.data(), volBlocks.size(), left);
		if (idx < 0 || (size_t)idx >= volumes.size())
		{
			break;
		}
		auto it = volumes.begin() + idx;
#else
		auto it = std::find_if(volumes.begin(), volumes.end(), [left](const Volume& v) { return v.blocks >= left; });
		if (it == volumes.end())
		{
			it = volumes.end() - 1;
		}
#endif
		PrintMessage(Message::mkInfo, "Fetching %s of twin %s for par-repair (%i block(s))",
			it->entry->filename.c_str(), m_twinName.c_str(), it->blocks);
		std::string path = FetchTwinFile(*it->entry);
		if (!path.empty())
		{
			paths.push_back(path);
			left -= it->blocks;
		}
		volumes.erase(it);
	}
	return paths;
}

void RepairController::PostParChecker::DeleteTwinFiles()
{
	for (const std::string& path : m_twinFiles)
	{
		FileSystem::DeleteFile(path.c_str());
	}
	m_twinFiles.clear();
	m_twinEntries.clear();
	m_twinName.clear();
	m_twinIndex.clear();
	m_twinBlockSize = 0;
}

void RepairController::PostParChecker::StatDupeSources(DupeSourceList* dupeSourceList)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	int totalExtraParBlocks = 0;
	for (DupeSource& dupeSource : dupeSourceList)
	{
		if (dupeSource.GetUsedBlocks() > 0)
		{
			for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
			{
				if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
					historyInfo->GetNzbInfo()->GetId() == dupeSource.GetId())
				{
#ifdef NZBGET_USE_RUST
					int newHistExtra = 0;
					int dummyTotal = totalExtraParBlocks;
					nzbget_rs_hsr_repair_ctrl_calc_dupe_source_extra_blocks(
						historyInfo->GetNzbInfo()->GetExtraParBlocks(),
						dupeSource.GetUsedBlocks(),
						totalExtraParBlocks,
						&newHistExtra,
						&dummyTotal);
					historyInfo->GetNzbInfo()->SetExtraParBlocks(newHistExtra);
#else
					historyInfo->GetNzbInfo()->SetExtraParBlocks(historyInfo->GetNzbInfo()->GetExtraParBlocks() - dupeSource.GetUsedBlocks());
#endif
				}
			}
		}
#ifdef NZBGET_USE_RUST
		int dummyHist = 0;
		nzbget_rs_hsr_repair_ctrl_calc_dupe_source_extra_blocks(
			0, dupeSource.GetUsedBlocks(), totalExtraParBlocks, &dummyHist, &totalExtraParBlocks);
#else
		totalExtraParBlocks += dupeSource.GetUsedBlocks();
#endif
	}

	m_postInfo->GetNzbInfo()->SetExtraParBlocks(m_postInfo->GetNzbInfo()->GetExtraParBlocks() + totalExtraParBlocks);
}


void RepairController::PostDupeMatcher::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, 1024, format, args);
	va_end(args);
	text[1024-1] = '\0';

	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}

#endif

RepairController::RepairController()
{
	debug("Creating RepairController");

#ifndef DISABLE_PARCHECK
	m_parChecker.m_owner = this;
#endif
}

void RepairController::Stop()
{
	debug("Stopping RepairController");
	Thread::Stop();
#ifndef DISABLE_PARCHECK
	m_parChecker.Cancel();
#endif
}

#ifndef DISABLE_PARCHECK

void RepairController::StartJob(PostInfo* postInfo)
{
	RepairController* repairController = new RepairController();
	repairController->m_postInfo = postInfo;
	repairController->SetAutoDestroy(false);

	postInfo->SetPostThread(repairController);

	repairController->Start();
}

void RepairController::Run()
{
	BString<1024> nzbName;
	CString destDir;
	bool dupeRepaired = false;
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		nzbName = m_postInfo->GetNzbInfo()->GetName();
		destDir = m_postInfo->GetNzbInfo()->GetDestDir();
		dupeRepaired = m_postInfo->GetNzbInfo()->GetDupeRecoveredBytes() > 0;
	}

	m_parChecker.SetPostInfo(m_postInfo);
	m_parChecker.SetDestDir(destDir);
	m_parChecker.SetNzbName(nzbName);
	m_parChecker.SetParTime(Util::CurrentTime());
	m_parChecker.SetDownloadSec(m_postInfo->GetNzbInfo()->GetDownloadSec());
	// stream repair rewrites file contents without updating the download-time
	// CRCs and segment states ParQuick judges files by: once it has written
	// into the collection (live or in post-processing, before or after a
	// restart), only a full verification sees the repaired bytes
#ifdef NZBGET_USE_RUST
	m_parChecker.SetParQuick(nzbget_rs_hsr_repair_ctrl_should_use_par_quick(
		g_Options->GetParQuick() ? 1 : 0,
		m_postInfo->GetForceParFull() ? 1 : 0,
		dupeRepaired ? 1 : 0) != 0);
#else
	m_parChecker.SetParQuick(g_Options->GetParQuick() && !m_postInfo->GetForceParFull() &&
		!dupeRepaired);
#endif
	m_parChecker.SetForceRepair(m_postInfo->GetForceRepair());

	m_parChecker.PrintMessage(Message::mkInfo, "Checking pars for %s", *nzbName);

	m_parChecker.Execute();
}

/**
 * DownloadQueue must be locked prior to call of this function.
 */
bool RepairController::AddPar(FileInfo* fileInfo, bool deleted)
{
	bool sameCollection = fileInfo->GetNzbInfo() == m_parChecker.GetPostInfo()->GetNzbInfo();
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_rrd_repair_ctrl_should_add_par_file(sameCollection ? 1 : 0, deleted ? 1 : 0) != 0)
#else
	if (sameCollection && !deleted)
#endif
	{
		BString<1024> fullFilename("%s%c%s", fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, fileInfo->GetFilename());
		m_parChecker.AddParFile(fullFilename);
	}
	else
	{
		m_parChecker.QueueChanged();
	}
	return sameCollection;
}

void RepairController::ParCheckCompleted()
{
	m_parChecker.DeleteTwinFiles();
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	PostInfo* postInfo = m_parChecker.GetPostInfo();

	// Update ParStatus (accumulate result)
#ifdef NZBGET_USE_RUST
	int setParRepaired = 0, parRepairedVal = 0;
	NzbInfo::EParStatus newParStatus = (NzbInfo::EParStatus)nzbget_rs_dur_repair_ctrl_resolve_par_status(
		(int)m_parChecker.GetStatus(),
		(int)postInfo->GetNzbInfo()->GetParStatus(),
		(int)ParChecker::psRepaired, (int)ParChecker::psRepairNotNeeded, (int)ParChecker::psRepairPossible,
		(int)NzbInfo::psSkipped, (int)NzbInfo::psSuccess, (int)NzbInfo::psRepairPossible, (int)NzbInfo::psFailure,
		&setParRepaired, &parRepairedVal);
	postInfo->GetNzbInfo()->SetParStatus(newParStatus);
	if (setParRepaired)
	{
		postInfo->SetParRepaired(parRepairedVal != 0);
	}
#else
	if ((m_parChecker.GetStatus() == ParChecker::psRepaired ||
		m_parChecker.GetStatus() == ParChecker::psRepairNotNeeded) &&
		postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped)
	{
		postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psSuccess);
		postInfo->SetParRepaired(m_parChecker.GetStatus() == ParChecker::psRepaired);
	}
	else if (m_parChecker.GetStatus() == ParChecker::psRepairPossible &&
		postInfo->GetNzbInfo()->GetParStatus() != NzbInfo::psFailure)
	{
		postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psRepairPossible);
	}
	else
	{
		postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psFailure);
	}
#endif

#ifdef NZBGET_USE_RUST
	int waitTime = 0, parSec = 0;
	nzbget_rs_fuq_repair_ctrl_calc_par_check_times(
		postInfo->GetNzbInfo()->GetDownloadSec(),
		m_parChecker.GetDownloadSec(),
		(int64_t)Util::CurrentTime(),
		(int64_t)m_parChecker.GetParTime(),
		&waitTime, &parSec);
	postInfo->SetStartTime(postInfo->GetStartTime() + (time_t)waitTime);
	postInfo->GetNzbInfo()->SetParSec(postInfo->GetNzbInfo()->GetParSec() + parSec);
#else
	int waitTime = postInfo->GetNzbInfo()->GetDownloadSec() - m_parChecker.GetDownloadSec();
	postInfo->SetStartTime(postInfo->GetStartTime() + (time_t)waitTime);
	int parSec = (int)(Util::CurrentTime() - m_parChecker.GetParTime()) - waitTime;
	postInfo->GetNzbInfo()->SetParSec(postInfo->GetNzbInfo()->GetParSec() + parSec);
#endif

	postInfo->GetNzbInfo()->SetParFull(m_parChecker.GetParFull());

	postInfo->SetWorking(false);
}

/**
* Unpause par2-files
* returns true, if the files with required number of blocks were unpaused,
* or false if there are no more files in queue for this collection or not enough blocks.
* special case: returns true if there are any unpaused par2-files in the queue regardless
* of the amount of blocks; this is to keep par-checker wait for download completion.
*/
bool RepairController::RequestMorePars(NzbInfo* nzbInfo, const char* parFilename, int blockNeeded, int* blockFoundOut)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	Blocks availableBlocks;
	Blocks selectedBlocks;
	int blockFound = 0;
	int curBlockFound = 0;

	FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, true, true, &curBlockFound);
	blockFound += curBlockFound;
	if (blockFound < blockNeeded)
	{
		FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, true, false, &curBlockFound);
		blockFound += curBlockFound;
	}
	if (blockFound < blockNeeded)
	{
		FindPars(downloadQueue, nzbInfo, parFilename, availableBlocks, false, false, &curBlockFound);
		blockFound += curBlockFound;
	}

	std::sort(availableBlocks.begin(), availableBlocks.end(),
		[](const BlockInfo& block1, const BlockInfo& block2)
		{
			return block1.m_blockCount < block2.m_blockCount;
		});

	if (blockFound >= blockNeeded)
	{
#ifdef NZBGET_USE_RUST
		std::vector<int> sortedCounts;
		sortedCounts.reserve(availableBlocks.size());
		for (const BlockInfo& b : availableBlocks)
		{
			sortedCounts.push_back(b.m_blockCount);
		}
		std::vector<size_t> selectedIdx(availableBlocks.size());
		int remNeeded = blockNeeded;
		size_t selCount = nzbget_rs_repair_select_more_pars(
			sortedCounts.data(), sortedCounts.size(), blockNeeded,
			selectedIdx.data(), selectedIdx.size(), &remNeeded);
		blockNeeded = remNeeded;
		for (size_t i = 0; i < selCount && i < selectedIdx.size(); ++i)
		{
			selectedBlocks.push_back(availableBlocks[selectedIdx[i]]);
		}
#else
		// collect as much blocks as needed
		for (Blocks::iterator it = availableBlocks.begin(); blockNeeded > 0 && it != availableBlocks.end(); ++it)
		{
			BlockInfo& blockInfo = *it;
			selectedBlocks.push_front(blockInfo);
			blockNeeded -= blockInfo.m_blockCount;
		}

		// discarding superfluous blocks
		for (Blocks::iterator it = selectedBlocks.begin(); it != selectedBlocks.end(); )
		{
			BlockInfo& blockInfo = *it;
			if (blockNeeded + blockInfo.m_blockCount <= 0)
			{
				blockNeeded += blockInfo.m_blockCount;
				it = selectedBlocks.erase(it);
			}
			else
			{
				++it;
			}
		}
#endif

		// unpause files with blocks
		for (BlockInfo& blockInfo : selectedBlocks)
		{
			if (blockInfo.m_fileInfo->GetPaused())
			{
				m_parChecker.PrintMessage(Message::mkInfo, "Unpausing %s%c%s for par-recovery", nzbInfo->GetName(), PATH_SEPARATOR, blockInfo.m_fileInfo->GetFilename());
				blockInfo.m_fileInfo->SetPaused(false);
				blockInfo.m_fileInfo->SetExtraPriority(true);
			}
		}
	}

	bool hasUnpausedParFiles = false;
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (fileInfo->GetParFile() && !fileInfo->GetPaused())
		{
			hasUnpausedParFiles = true;
			break;
		}
	}

	if (blockFoundOut)
	{
		*blockFoundOut = blockFound;
	}

#ifdef NZBGET_USE_RUST
	bool ok = nzbget_rs_rrd_repair_ctrl_is_request_more_pars_ok(
		blockNeeded, hasUnpausedParFiles ? 1 : 0) != 0;
#else
	bool ok = blockNeeded <= 0 || hasUnpausedParFiles;
#endif

	return ok;
}

void RepairController::FindPars(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* parFilename,
	Blocks& blocks, bool strictParName, bool exactParName, int* blockFound)
{
	*blockFound = 0;

	// extract base name from m_szParFilename (trim .par2-extension and possible .vol-part)
	char* baseParFilename = FileSystem::BaseFileName(parFilename);
	int mainBaseLen = 0;
	if (!ParParser::ParseParFilename(baseParFilename, true, &mainBaseLen, nullptr))
	{
		// should not happen
		nzbInfo->PrintMessage(Message::mkError, "Internal error: could not parse filename %s", baseParFilename);
		return;
	}
	BString<1024> mainBaseFilename;
	mainBaseFilename.Set(baseParFilename, mainBaseLen);
	for (char* p = mainBaseFilename; *p; p++) *p = tolower(*p); // convert string to lowercase

	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		int blockCount = 0;
		bool parsed = ParParser::ParseParFilename(fileInfo->GetFilename(), fileInfo->GetFilenameConfirmed(), nullptr, &blockCount);
		if (parsed && blockCount < 0)
		{
			// a volume named without its block count ("vol-06"): estimated from
			// its size (a recovery packet is a block and 68 bytes)
			int64 blockSize = m_parChecker.GetBlockSize();
#ifdef NZBGET_USE_RUST
			blockCount = nzbget_rs_dur_repair_ctrl_estimate_par_blocks_from_size(
				fileInfo->GetSize(), blockSize);
#else
			blockCount = blockSize > 0 ? (int)std::max<int64>(1, fileInfo->GetSize() / (blockSize + 68)) : 1;
#endif
		}
		if (parsed && blockCount > 0)
		{
			bool useFile = true;

			if (exactParName)
			{
				useFile = ParParser::SameParCollection(fileInfo->GetFilename(),
					FileSystem::BaseFileName(parFilename), fileInfo->GetFilenameConfirmed());
			}
			else if (strictParName)
			{
				// the pFileInfo->GetFilename() may be not confirmed and may contain
				// additional texts if Subject could not be parsed correctly

#ifdef NZBGET_USE_RUST
				useFile = nzbget_rs_dur_repair_ctrl_match_strict_par_name(
					fileInfo->GetFilename(), mainBaseFilename) != 0;
#else
				BString<1024> loFileName = fileInfo->GetFilename();
				for (char* p = loFileName; *p; p++) *p = tolower(*p); // convert string to lowercase

				BString<1024> candidateFileName("%s.par2", *mainBaseFilename);
				if (!strstr(loFileName, candidateFileName))
				{
					candidateFileName.Format("%s.vol", *mainBaseFilename);
					useFile = strstr(loFileName, candidateFileName);
				}
#endif
			}

			bool alreadyAdded = false;
			// check if file is not in the list already
			if (useFile)
			{
				for (BlockInfo& blockInfo : blocks)
				{
					if (blockInfo.m_fileInfo == fileInfo)
					{
						alreadyAdded = true;
						break;
					}
				}
			}

			// if it is a par2-file with blocks and it was from the same NZB-request
			// and it belongs to the same file collection (same base name),
			// then OK, we can use it
			if (useFile && !alreadyAdded)
			{
				blocks.emplace_back(fileInfo, blockCount);
				*blockFound += blockCount;
			}
		}
	}
}

void RepairController::UpdateParCheckProgress()
{
	PostInfo* postInfo;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		postInfo = m_parChecker.GetPostInfo();
		if (m_parChecker.GetFileProgress() == 0)
		{
			postInfo->SetProgressLabel(m_parChecker.GetProgressLabel());
		}
		postInfo->SetFileProgress(m_parChecker.GetFileProgress());
		postInfo->SetStageProgress(m_parChecker.GetStageProgress());
		PostInfo::EStage StageKind[] = {PostInfo::ptLoadingPars, PostInfo::ptVerifyingSources, PostInfo::ptRepairing, PostInfo::ptVerifyingRepaired};
		PostInfo::EStage stage = StageKind[m_parChecker.GetStage()];
		time_t current = Util::CurrentTime();

#ifdef NZBGET_USE_RUST
		int setRepairTime = 0;
		int newRepairSec = postInfo->GetNzbInfo()->GetRepairSec();
		if (nzbget_rs_hsr_repair_ctrl_eval_stage_transition(
			(int)postInfo->GetStage(),
			(int)stage,
			(int)PostInfo::ptRepairing,
			(int)PostInfo::ptVerifyingRepaired,
			(int64)current,
			(int64)m_parChecker.GetRepairTime(),
			newRepairSec,
			&setRepairTime,
			&newRepairSec) != 0)
		{
			postInfo->SetStage(stage);
			postInfo->SetStageTime(current);
			if (setRepairTime)
			{
				m_parChecker.SetRepairTime(current);
			}
			else if (postInfo->GetStage() == PostInfo::ptVerifyingRepaired)
			{
				postInfo->GetNzbInfo()->SetRepairSec(newRepairSec);
			}
		}
#else
		if (postInfo->GetStage() != stage)
		{
			postInfo->SetStage(stage);
			postInfo->SetStageTime(current);
			if (postInfo->GetStage() == PostInfo::ptRepairing)
			{
				m_parChecker.SetRepairTime(current);
			}
			else if (postInfo->GetStage() == PostInfo::ptVerifyingRepaired)
			{
				int repairSec = (int)(current - m_parChecker.GetRepairTime());
				postInfo->GetNzbInfo()->SetRepairSec(postInfo->GetNzbInfo()->GetRepairSec() + repairSec);
			}
		}
#endif

		bool parCancel = false;
		if (!IsStopped())
		{
#ifdef NZBGET_USE_RUST
			int estimatedRepairTime = 0;
			if (nzbget_rs_repair_check_par_time_limit(
				g_Options->GetParTimeLimit(),
				m_parChecker.GetStage() == PostParChecker::ptRepairing ? 1 : 0,
				(int64)(current - postInfo->GetStageTime()),
				postInfo->GetStageProgress(),
				&estimatedRepairTime) != 0)
			{
				debug("Estimated repair time %i seconds", estimatedRepairTime);
				m_parChecker.PrintMessage(Message::mkWarning, "Cancelling par-repair for %s, estimated repair time (%i minutes) exceeds allowed repair time", m_parChecker.GetInfoName(), estimatedRepairTime / 60);
				parCancel = true;
			}
#else
			if ((g_Options->GetParTimeLimit() > 0) &&
				m_parChecker.GetStage() == PostParChecker::ptRepairing &&
				((g_Options->GetParTimeLimit() > 5 && current - postInfo->GetStageTime() > 5 * 60) ||
					(g_Options->GetParTimeLimit() <= 5 && current - postInfo->GetStageTime() > 1 * 60)))
			{
				// first five (or one) minutes elapsed, now can check the estimated time: of
				// the repair stage, whose progress this is (from the job's start, loading
				// pars and verification counted against the limit, contrary to the option)
				int estimatedRepairTime = (int)((current - postInfo->GetStageTime()) * 1000 /
					(postInfo->GetStageProgress() > 0 ? postInfo->GetStageProgress() : 1));
				if (estimatedRepairTime > g_Options->GetParTimeLimit() * 60)
				{
					debug("Estimated repair time %i seconds", estimatedRepairTime);
					m_parChecker.PrintMessage(Message::mkWarning, "Cancelling par-repair for %s, estimated repair time (%i minutes) exceeds allowed repair time", m_parChecker.GetInfoName(), estimatedRepairTime / 60);
					parCancel = true;
				}
			}
#endif
		}

		if (parCancel)
		{
			Stop();
		}
	}

	CheckPauseState(postInfo);
}

void RepairController::CheckPauseState(PostInfo* postInfo)
{
	if (g_WorkState->GetPausePostProcess() && !postInfo->GetNzbInfo()->GetForcePriority())
	{
		time_t stageTime = postInfo->GetStageTime();
		time_t startTime = postInfo->GetStartTime();
		time_t parTime = m_parChecker.GetParTime();
		time_t repairTime = m_parChecker.GetRepairTime();
		time_t waitTime = Util::CurrentTime();

		// wait until Post-processor is unpaused
#ifdef NZBGET_USE_RUST
		while (nzbget_rs_rrd_repair_ctrl_is_post_process_paused(
			g_WorkState->GetPausePostProcess() ? 1 : 0,
			postInfo->GetNzbInfo()->GetForcePriority() ? 1 : 0,
			IsStopped() ? 1 : 0) != 0)
		{
			Util::Sleep(50);

			int64 newStage = 0, newStart = 0, newPar = 0, newRepair = 0;
			nzbget_rs_rrd_repair_ctrl_calc_paused_timestamps(
				(int64)Util::CurrentTime(), (int64)waitTime,
				(int64)stageTime, (int64)startTime, (int64)parTime, (int64)repairTime,
				&newStage, &newStart, &newPar, &newRepair);
			if (stageTime > 0)
			{
				postInfo->SetStageTime((time_t)newStage);
			}
			if (startTime > 0)
			{
				postInfo->SetStartTime((time_t)newStart);
			}
			if (parTime > 0)
			{
				m_parChecker.SetParTime((time_t)newPar);
			}
			if (repairTime > 0)
			{
				m_parChecker.SetRepairTime((time_t)newRepair);
			}
		}
#else
		while (g_WorkState->GetPausePostProcess() && !postInfo->GetNzbInfo()->GetForcePriority() && !IsStopped())
		{
			Util::Sleep(50);

			// update time stamps

			time_t delta = Util::CurrentTime() - waitTime;

			if (stageTime > 0)
			{
				postInfo->SetStageTime(stageTime + delta);
			}
			if (startTime > 0)
			{
				postInfo->SetStartTime(startTime + delta);
			}
			if (parTime > 0)
			{
				m_parChecker.SetParTime(parTime + delta);
			}
			if (repairTime > 0)
			{
				m_parChecker.SetRepairTime(repairTime + delta);
			}
		}
#endif
	}
}

#endif
