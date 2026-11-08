/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
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


#ifndef DUPESTREAMREPAIR_H
#define DUPESTREAMREPAIR_H

#include <algorithm>
#include <set>
#include <string>
#include <vector>
#include "DownloadInfo.h"

/*
 * Byte-range ("stream level") recovery of directly posted media files from
 * duplicate postings of the same content that were segmented differently
 * (see option <DupeArticleFallback> value "stream").
 *
 * This class holds the pure geometry and eligibility logic: computing the
 * missing byte ranges of a file from its downloaded articles, estimating the
 * decoded byte ranges of a donor's articles from its nzb-declared encoded
 * sizes, and selecting donor articles for content probes and hole patches.
 * The post-processing stage that fetches and writes bytes is
 * StreamRepairController (daemon/postprocess/StreamRepair.h).
 */
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

#include "nzbget_rs.h"
#include "DupeArticleFallback.h"
#include "Options.h"

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

#endif
