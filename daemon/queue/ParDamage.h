/*
 *  This file is part of nzbget. See <https://nzbget.com>.
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


#ifndef PARDAMAGE_H
#define PARDAMAGE_H

#include <algorithm>
#include <set>
#include <string>
#include <vector>
#include "DownloadInfo.h"
#include "nzbget_rs.h"

class DupeStreamRepair
{
public:
	static constexpr int StreamFetchWorkers = 4;
	static constexpr int MaxStreamFetchWorkers = 16;
	static int StreamFetchWorkerCount(int primaryConnections)
	{
		return std::clamp(primaryConnections / 4, StreamFetchWorkers, MaxStreamFetchWorkers);
	}

	static StreamRangeList ComputeHoles(FileInfo* fileInfo);
	static int64 TotalSize(const StreamRangeList& ranges);
	static bool BuildRepairJob(FileInfo* fileInfo, const char* diskBasename);
	static std::string BuildDonorKey(NzbInfo* donorNzb, const char* password);
};

/*
 * The par2 blocks a download's failed articles spoil, against the recovery
 * blocks its par2-files hold. Health counts articles, but par2 repairs whole
 * blocks: one lost article of 0.7 MB spoils a block of 15-25 MB, so a download
 * at health 98% can have most of its blocks damaged and fail par-repair after
 * downloading in full. Kept in memory only (all calls under the queue guard):
 * after a restart the files finished before it count as undamaged.
 */
class ParDamage
{
public:
	struct Verdict
	{
		bool known = false;			// a damaged file matched a par2 set with its block size
		uint64 blockSize = 0;
		int damagedBlocks = 0;		// in the data files finished so far
		int recoveryBlocks = 0;		// in all par2 volumes of the download, counted whole
		int64 doneBytes = 0;		// data finished so far
		int64 totalBytes = 0;		// data of the download
		int projectedBlocks = 0;	// damagedBlocks at the rate so far, for all the data

		/* the blocks already spoiled are more than all its par2-files can repair */
		bool Certain() const { return known && damagedBlocks > recoveryBlocks; }
		/* at the rate so far, a fifth or more of the data done, the damage will be
		 * well past what par2 repairs (by a quarter and two blocks) */
		bool Projected() const
		{
			return known && doneBytes * 5 >= totalBytes &&
				(int64)projectedBlocks * 4 > (int64)recoveryBlocks * 5 + 8;
		}
	};

	/* a file of the download is finished (its article list still exists) */
	static void FileCompleted(NzbInfo* nzbInfo, FileInfo* fileInfo, const std::string& filename);

	static Verdict Judge(NzbInfo* nzbInfo);

	/* true the first time it's asked for a download, false after */
	static bool FirstReport(int nzbId)
	{
		return nzbget_rs_par_damage_first_report(nzbId) != 0;
	}

	static void Forget(int nzbId)
	{
		nzbget_rs_par_damage_forget(nzbId);
		nzbget_rs_streaming_par_forget(nzbId);
	}

	/* distinct blocks of <blockSize> the ranges touch */
	static int BlocksTouched(const StreamRangeList& holes, uint64 blockSize)
	{
		static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange) &&
			offsetof(StreamRange, Offset) == offsetof(NzbgetRsByteRange, offset) &&
			offsetof(StreamRange, Size) == offsetof(NzbgetRsByteRange, size),
			"StreamRange layout must match NzbgetRsByteRange");
		return nzbget_rs_par_damage_blocks_touched(
			holes.empty() ? nullptr : reinterpret_cast<const NzbgetRsByteRange*>(holes.data()),
			holes.size(), blockSize);
	}
};

#include "DupeArticleFallback.h"
#include "Options.h"
#include "ParParser.h"

inline StreamRangeList DupeStreamRepair::ComputeHoles(FileInfo* fileInfo)
{
	int64 fileSize = fileInfo->GetDecodedFileSize();
	if (fileSize <= 0)
	{
		return StreamRangeList();
	}

	std::vector<NzbgetRsByteRange> covered;
	covered.reserve(fileInfo->GetArticles()->size());
	for (ArticleInfo* article : fileInfo->GetArticles())
	{
		if (article->GetStatus() == ArticleInfo::aiFinished && article->GetSegmentSize() > 0)
		{
			covered.push_back({article->GetSegmentOffset(), article->GetSegmentSize()});
		}
	}
	std::vector<NzbgetRsByteRange> rsHoles(covered.size() + 1);
	size_t count = nzbget_rs_stream_compute_holes(
		covered.data(), covered.size(), fileSize, rsHoles.data(), rsHoles.size());
	StreamRangeList holes;
	holes.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		holes.push_back({rsHoles[i].offset, rsHoles[i].size});
	}
	return holes;
}

inline int64 DupeStreamRepair::TotalSize(const StreamRangeList& ranges)
{
	return nzbget_rs_stream_total_size(
		reinterpret_cast<const NzbgetRsByteRange*>(ranges.data()), ranges.size());
}

inline bool DupeStreamRepair::BuildRepairJob(FileInfo* fileInfo, const char* diskBasename)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	int jobMode = nzbget_rs_stream_eval_build_repair_job(
		(int)g_Options->GetDupeArticleFallback(),
		(int)Options::dafStream,
		g_Options->GetRawArticle() ? 1 : 0,
		DupeArticleFallback::IsParFile(fileInfo) ? 1 : 0,
		diskBasename,
		nzbInfo ? 1 : 0,
		nzbInfo ? (int)nzbInfo->GetDeleteStatus() : 0,
		(int)NzbInfo::dsNone,
		(int)NzbInfo::dsHealth,
		nzbInfo && nzbInfo->GetDeleting() ? 1 : 0,
		nzbInfo && nzbInfo->GetParking() ? 1 : 0,
		nzbInfo && nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
		fileInfo->GetArticles()->empty() ? 1 : 0,
		fileInfo->GetSuccessArticles(),
		fileInfo->GetSize(),
		fileInfo->GetDecodedFileSize());
	if (jobMode == 0)
	{
		return false;
	}

	if (jobMode == 1)
	{
		nzbInfo->GetStreamRepairJobs()->emplace_back(fileInfo->GetId(), diskBasename,
			0, fileInfo->GetFailedSize(), fileInfo->GetMissedSize(),
			fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles(), fileInfo->GetParFile(),
			StreamRangeList(), DupeArticleFallback::ArticleSizeStepsHash(fileInfo));
		return true;
	}

	StreamRangeList holes = ComputeHoles(fileInfo);
	if (holes.empty())
	{
		return false;
	}

	nzbInfo->GetStreamRepairJobs()->emplace_back(fileInfo->GetId(), diskBasename,
		fileInfo->GetDecodedFileSize(), fileInfo->GetFailedSize(), fileInfo->GetMissedSize(),
		fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles(), fileInfo->GetParFile(),
		std::move(holes), DupeArticleFallback::ArticleSizeStepsHash(fileInfo));

	return true;
}

inline std::string DupeStreamRepair::BuildDonorKey(NzbInfo* donorNzb, const char* password)
{
	size_t fileCount = donorNzb->GetFileList()->size();
	std::vector<const char*> fileNames;
	std::vector<int64_t> fileSizes;
	std::vector<uint8_t> fileIsPar;
	std::vector<size_t> fileGroupCounts;
	std::vector<const char*> groups;
	std::vector<size_t> fileArticleCounts;
	std::vector<int32_t> artPartNumbers;
	std::vector<int32_t> artSizes;
	std::vector<const char*> artMessageIds;
	fileNames.reserve(fileCount);
	fileSizes.reserve(fileCount);
	fileIsPar.reserve(fileCount);
	fileGroupCounts.reserve(fileCount);
	fileArticleCounts.reserve(fileCount);
	for (FileInfo* file : donorNzb->GetFileList())
	{
		fileNames.push_back(file->GetFilename());
		fileSizes.push_back(file->GetSize());
		fileIsPar.push_back(file->GetParFile() ? 1 : 0);
		fileGroupCounts.push_back(file->GetGroups()->size());
		for (const CString& group : *file->GetGroups())
		{
			groups.push_back(*group);
		}
		fileArticleCounts.push_back(file->GetArticles()->size());
		for (const std::unique_ptr<ArticleInfo>& article : *file->GetArticles())
		{
			artPartNumbers.push_back(article->GetPartNumber());
			artSizes.push_back(article->GetSize());
			artMessageIds.push_back(article->GetMessageId());
		}
	}
	NzbgetRsBuf buf = nzbget_rs_stream_build_donor_key(
		password,
		fileNames.data(),
		fileSizes.data(),
		fileIsPar.data(),
		fileGroupCounts.data(),
		fileCount,
		groups.data(),
		groups.size(),
		fileArticleCounts.data(),
		artPartNumbers.data(),
		artSizes.data(),
		artMessageIds.data(),
		artMessageIds.size());
	std::string key(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return key;
}

inline void ParDamage::FileCompleted(NzbInfo* nzbInfo, FileInfo* fileInfo, const std::string& filename)
{
	static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange) &&
		offsetof(StreamRange, Offset) == offsetof(NzbgetRsByteRange, offset) &&
		offsetof(StreamRange, Size) == offsetof(NzbgetRsByteRange, size),
		"StreamRange layout must match NzbgetRsByteRange");
	bool isPar = fileInfo->GetParFile() || DupeArticleFallback::IsParFile(fileInfo);
	int failedPlusMissed = fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles();
	int64 decodedSize = fileInfo->GetDecodedFileSize();
	StreamRangeList holes;
	if (!isPar && failedPlusMissed > 0 && decodedSize > 0)
	{
		holes = DupeStreamRepair::ComputeHoles(fileInfo);
	}
	nzbget_rs_par_damage_file_completed(
		nzbInfo->GetId(),
		isPar ? 1 : 0,
		failedPlusMissed,
		decodedSize,
		fileInfo->GetSuccessArticles(),
		fileInfo->GetSize(),
		filename.c_str(),
		holes.empty() ? nullptr : reinterpret_cast<const NzbgetRsByteRange*>(holes.data()),
		holes.size());

	if (isPar && failedPlusMissed == 0 && !filename.empty())
	{
		if (nzbget_rs_streaming_par_has_session(nzbInfo->GetId()) == 0)
		{
			if (nzbget_rs_streaming_par_start(nzbInfo->GetId(), filename.c_str(), nzbInfo->GetDestDir()) != 0)
			{
				for (FileInfo* qFile : nzbInfo->GetFileList())
				{
					if (!qFile || qFile == fileInfo || qFile->GetDeleted())
					{
						continue;
					}
					int baseLen = 0;
					int blocks = 0;
					if (ParParser::ParseParFilename(qFile->GetFilename(), qFile->GetFilenameConfirmed(), &baseLen, &blocks) &&
						blocks != 0 &&
						ParParser::SameParCollection(
							fileInfo->GetFilename(),
							qFile->GetFilename(),
							fileInfo->GetFilenameConfirmed() && qFile->GetFilenameConfirmed()))
					{
						nzbget_rs_streaming_par_register_queued_volume(
							nzbInfo->GetId(),
							static_cast<uint64_t>(qFile->GetId()),
							blocks > 0 ? blocks : 1,
							qFile->GetPaused() ? 1 : 0);
					}
				}
			}
		}
		else
		{
			nzbget_rs_streaming_par_on_recovery_volume_arrived(
				nzbInfo->GetId(),
				filename.c_str(),
				static_cast<uint64_t>(fileInfo->GetId()),
				1);
		}
	}
}

inline ParDamage::Verdict ParDamage::Judge(NzbInfo* nzbInfo)
{
	Verdict verdict;
	if (!nzbget_rs_par_damage_has_damage(nzbInfo->GetId()))
	{
		return verdict;
	}

	std::vector<NzbgetRsParDamageQueuedFile> queued;
	queued.reserve(nzbInfo->GetFileList()->size());
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		queued.push_back({
			fileInfo->GetFilename(),
			fileInfo->GetFilenameConfirmed() ? 1 : 0,
			fileInfo->GetSize()
		});
	}

	std::vector<NzbgetRsParDamageCompletedFile> completed;
	completed.reserve(nzbInfo->GetCompletedFiles()->size());
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		completed.push_back({
			completedFile.GetFilename(),
			completedFile.GetStatus() != CompletedFile::cfFailure ? 1 : 0
		});
	}

	NzbgetRsParDamageVerdict rsVerdict{};
	nzbget_rs_par_damage_judge(
		nzbInfo->GetId(),
		nzbInfo->GetDestDir(),
		nzbInfo->GetSize(),
		nzbInfo->GetParSize(),
		queued.empty() ? nullptr : queued.data(),
		queued.size(),
		completed.empty() ? nullptr : completed.data(),
		completed.size(),
		&rsVerdict);

	verdict.known = rsVerdict.known != 0;
	verdict.blockSize = rsVerdict.block_size;
	verdict.damagedBlocks = rsVerdict.damaged_blocks;
	verdict.recoveryBlocks = rsVerdict.recovery_blocks;
	verdict.doneBytes = rsVerdict.done_bytes;
	verdict.totalBytes = rsVerdict.total_bytes;
	verdict.projectedBlocks = rsVerdict.projected_blocks;
	return verdict;
}

#endif
