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
	// donor articles picked around holes are expanded by this many parts on
	// each side to absorb the encoded-to-decoded estimation error
	static constexpr int PatchMarginParts = 2;
	// number of donor articles fetched to verify content identity against
	// already-downloaded regions before anything is written
	static constexpr int ProbeCount = 2;
	// a repost misses articles of its own: a probe the donor cannot supply
	// is inconclusive, so it is replaced by another eligible article, up to
	// this many probe fetches per donor member
	static constexpr int MaxProbeFetches = 8;
	// a probe must byte-compare at least this much overlap to count as proof
	static constexpr int64 MinProbeCompareBytes = 16 * 1024;
	// how many donor members are probed per target member before giving up
	// (bounds wasted fetches when pairing heuristics fail on obfuscated sets)
	static constexpr int MaxDonorCandidates = 4;
	// nzb-declared encoded sizes of two postings of one file differ by their
	// subject lines only (as DupeArticleFallback::TotalSizeToleranceDiv)
	static constexpr int WholeFileSizeToleranceDiv = 64;
	// a duplicate whose same-size members cannot be told apart by name or
	// article sizes (obfuscated volumes on both sides) is probed member by
	// member up to this many, until one is byte-identical: a dupe is the
	// same release, so the matching member is in there somewhere
	static constexpr int MaxSiblingCandidates = 32;
	// concurrent donor-article fetch workers for the post-processing repair
	// pass (each holds one pool connection while fetching): a quarter of the
	// primary servers' connections, within these bounds. A heavily damaged
	// file can need most of a duplicate (or, with DupeStreamDecompress, all
	// of a duplicate archive), which a handful of connections fetches many
	// times slower than the download itself ran; memory stays bounded by
	// ArticleBatchFetcher's window. The live pass always fetches serially
	// (0 workers) - it must not compete with the active download
	static constexpr int StreamFetchWorkers = 4;
	// bounds worst-case memory: every worker may hold one article in flight
	// on top of the batch fetcher's buffered window
	static constexpr int MaxStreamFetchWorkers = 16;
	static int StreamFetchWorkerCount(int primaryConnections)
	{
		return std::clamp(primaryConnections / 4, StreamFetchWorkers, MaxStreamFetchWorkers);
	}
	// M4 (option <DupeStreamDecompress>): materialization cap on a donor
	// archive's total decoded size - generous, since the donor inner size
	// ~ target size and the archive is usually smaller, but a hard bound so
	// a huge donor is never extracted for a small target
	static constexpr int64 MaxDecompressBytes = 16LL * 1024 * 1024 * 1024;

	static StreamRangeList ComputeHoles(FileInfo* fileInfo);
	static int64 TotalSize(const StreamRangeList& ranges);
	static StreamRangeList EstimateDonorRanges(FileInfo* donorFile, int64 decodedFileSize);

	/* Captures a stream-repair job on the owning NzbInfo for a data file
	 * that completed with missing byte ranges - or without a single article:
	 * such a job has no decoded size and no holes yet (the whole file is the
	 * hole; its size is learned from the first donor article). diskBasename
	 * is the file's on-disk name at completion. PAR2 files are excluded.
	 * Must be called within DownloadQueue-lock. */
	static bool BuildRepairJob(FileInfo* fileInfo, const char* diskBasename);

	/* The one donor member that can stand in for a file no article of which
	 * was downloaded (so nothing exists to compare bytes against): among the
	 * donor's data files with the target's encoded size (the article count
	 * may differ), the unique size-step fingerprint match, else the unique
	 * exact name, else the unique volume suffix. Size-only pairing is
	 * deliberately not offered. Members in claimed are skipped; with donorSet
	 * only members of that archive set (VolumeSetKey) are considered. */
	static FileInfo* SelectWholeFileDonor(const char* targetFilename, int64 targetEncodedSize,
		int targetArticleCount, uint64 targetStepsHash, NzbInfo* donorNzb,
		const std::set<FileInfo*>* claimed = nullptr, const std::string& donorSet = "");

	/* The name a file shares with every other volume of its archive set, its
	 * volume number replaced: "Rel.part03.rar" and "Rel.part1.rar" ->
	 * "rel.part#.rar", "Rel.r07" and "Rel.rar" -> "rel.r#", "Rel.7z.002" ->
	 * "rel.7z.#", "Rel.z01" and "Rel.zip" -> "rel.z#". Empty for a file that
	 * is no archive volume (an .nfo, a sample, a bare media file). */
	static std::string VolumeSetKey(const char* filename);

	/* Donor files of one duplicate collection, ordered most-likely-identical
	 * first for the given target member: exact name match, same suffix key
	 * when it identifies exactly one donor member (volume schemes; shared
	 * extension keys are skipped), the positionalRank-th size-window member
	 * by donor filename order when positionalWindow matches the donor window size
	 * (rank < 0 skips), then ascending encoded-size distance; deduplicated
	 * and capped. PAR2 targets and donors are excluded, and so are the donor
	 * members in claimed (already proven identical to another target). Every
	 * candidate still has to pass probe verification. */
	static std::vector<FileInfo*> SelectDonorCandidates(const char* targetFilename,
		int64 targetDecodedFileSize, int positionalRank, int positionalWindow,
		NzbInfo* donorNzb, int maxCandidates, uint64 targetStepsHash = 0,
		const std::set<FileInfo*>* claimed = nullptr);

	/* Exact, in-memory identity of a parsed donor's repair inputs. Includes
	 * groups, archive names, article geometry and password so saved copies of
	 * one posting can be skipped without hiding useful alternate metadata.
	 * Contains the password: never persist or log this key. */
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

inline StreamRangeList DupeStreamRepair::EstimateDonorRanges(FileInfo* donorFile, int64 decodedFileSize)
{
	std::vector<int> sizes;
	sizes.reserve(donorFile->GetArticles()->size());
	for (ArticleInfo* article : donorFile->GetArticles())
	{
		sizes.push_back(article->GetSize());
	}
	std::vector<NzbgetRsByteRange> rsRanges(sizes.size());
	size_t count = nzbget_rs_stream_estimate_donor_ranges(
		sizes.data(), sizes.size(), decodedFileSize, rsRanges.data());
	StreamRangeList ranges;
	ranges.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		ranges.push_back({rsRanges[i].offset, rsRanges[i].size});
	}
	return ranges;
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

inline FileInfo* DupeStreamRepair::SelectWholeFileDonor(const char* targetFilename, int64 targetEncodedSize,
	int targetArticleCount, uint64 targetStepsHash, NzbInfo* donorNzb,
	const std::set<FileInfo*>* claimed, const std::string& donorSet)
{
	std::vector<FileInfo*> files;
	std::vector<const char*> names;
	std::vector<int64_t> sizes;
	std::vector<uint8_t> isPar;
	std::vector<uint8_t> hasArticles;
	std::vector<uint8_t> isClaimed;
	std::vector<uint64_t> stepsHashes;
	size_t n = donorNzb->GetFileList()->size();
	files.reserve(n);
	names.reserve(n);
	sizes.reserve(n);
	isPar.reserve(n);
	hasArticles.reserve(n);
	isClaimed.reserve(n);
	stepsHashes.reserve(n);
	for (FileInfo* donorFile : donorNzb->GetFileList())
	{
		files.push_back(donorFile);
		names.push_back(donorFile->GetFilename());
		sizes.push_back(donorFile->GetSize());
		isPar.push_back(DupeArticleFallback::IsParFile(donorFile) ? 1 : 0);
		hasArticles.push_back(!donorFile->GetArticles()->empty() ? 1 : 0);
		isClaimed.push_back((claimed && claimed->count(donorFile)) ? 1 : 0);
		stepsHashes.push_back(targetStepsHash != 0 ? DupeArticleFallback::ArticleSizeStepsHash(donorFile) : 0);
	}
	int idx = nzbget_rs_stream_select_whole_file_donor(
		targetFilename, targetEncodedSize, targetArticleCount, targetStepsHash,
		donorSet.c_str(), names.data(), sizes.data(), isPar.data(),
		hasArticles.data(), isClaimed.data(), stepsHashes.data(), files.size());
	return (idx >= 0 && (size_t)idx < files.size()) ? files[idx] : nullptr;
}

inline std::string DupeStreamRepair::VolumeSetKey(const char* filename)
{
	NzbgetRsBuf buf = nzbget_rs_stream_volume_set_key(filename);
	std::string res(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return res;
}

inline std::vector<FileInfo*> DupeStreamRepair::SelectDonorCandidates(const char* targetFilename,
	int64 targetDecodedFileSize, int positionalRank, int positionalWindow,
	NzbInfo* donorNzb, int maxCandidates, uint64 targetStepsHash,
	const std::set<FileInfo*>* claimed)
{
	std::vector<FileInfo*> files;
	std::vector<const char*> names;
	std::vector<int64_t> sizes;
	std::vector<uint8_t> isPar;
	std::vector<uint8_t> hasArticles;
	std::vector<uint8_t> isClaimed;
	std::vector<uint64_t> stepsHashes;
	size_t n = donorNzb->GetFileList()->size();
	files.reserve(n);
	names.reserve(n);
	sizes.reserve(n);
	isPar.reserve(n);
	hasArticles.reserve(n);
	isClaimed.reserve(n);
	stepsHashes.reserve(n);
	for (FileInfo* donorFile : donorNzb->GetFileList())
	{
		files.push_back(donorFile);
		names.push_back(donorFile->GetFilename());
		sizes.push_back(donorFile->GetSize());
		isPar.push_back(DupeArticleFallback::IsParFile(donorFile) ? 1 : 0);
		hasArticles.push_back(!donorFile->GetArticles()->empty() ? 1 : 0);
		isClaimed.push_back((claimed && claimed->count(donorFile)) ? 1 : 0);
		stepsHashes.push_back(targetStepsHash != 0 ? DupeArticleFallback::ArticleSizeStepsHash(donorFile) : 0);
	}
	std::vector<int32_t> outIndices(files.size());
	size_t count = nzbget_rs_stream_select_donor_candidates(
		targetFilename, targetDecodedFileSize, positionalRank, positionalWindow,
		maxCandidates, targetStepsHash, names.data(), sizes.data(),
		isPar.data(), hasArticles.data(), isClaimed.data(), stepsHashes.data(),
		files.size(), outIndices.data(), outIndices.size());
	std::vector<FileInfo*> candidates;
	candidates.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		int32_t idx = outIndices[i];
		if (idx >= 0 && (size_t)idx < files.size())
		{
			candidates.push_back(files[idx]);
		}
	}
	return candidates;
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
