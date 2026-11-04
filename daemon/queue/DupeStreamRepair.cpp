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


#include "nzbget.h"

#include <algorithm>
#include <array>
#include <bit>
#include <cstdlib>
#include <cstddef>
#include <type_traits>
#include <limits>
#include "nzbget_rs.h"
#include "DupeStreamRepair.h"
#include "DupeArticleFallback.h"
#include "FileSystem.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"

bool DupeStreamRepair::IsStreamEligible(const char* filename)
{
	return nzbget_rs_stream_is_eligible(filename) != 0;
}

StreamRangeList DupeStreamRepair::ComputeHoles(FileInfo* fileInfo)
{
	int64 fileSize = fileInfo->GetDecodedFileSize();
	if (fileSize <= 0)
	{
		return StreamRangeList();
	}

	// ranges of successfully decoded articles; failed articles carry no
	// offset/size (their yEnc headers never arrived) and default to 0/0
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

int64 DupeStreamRepair::TotalSize(const StreamRangeList& ranges)
{
	return nzbget_rs_stream_total_size(
		reinterpret_cast<const NzbgetRsByteRange*>(ranges.data()), ranges.size());
}

bool DupeStreamRepair::IntersectsAny(const StreamRange& range, const StreamRangeList& ranges)
{
	NzbgetRsByteRange r{range.Offset, range.Size};
	return nzbget_rs_stream_intersects_any(
		r, reinterpret_cast<const NzbgetRsByteRange*>(ranges.data()), ranges.size()) != 0;
}

StreamRangeList DupeStreamRepair::EstimateDonorRanges(FileInfo* donorFile, int64 decodedFileSize)
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

std::vector<int> DupeStreamRepair::SelectPatchParts(const StreamRangeList& donorRanges,
	const StreamRangeList& holes, int marginParts)
{
	static_assert(std::is_standard_layout<StreamRange>::value && sizeof(StreamRange) == 2 * sizeof(int64_t) &&
		alignof(StreamRange) == alignof(int64_t) && offsetof(StreamRange, Offset) == 0 &&
		offsetof(StreamRange, Size) == sizeof(int64_t), "StreamRange C ABI layout");
	static_assert(std::is_same<int, int32_t>::value, "patch index C ABI layout");
	if (donorRanges.empty() || holes.empty() || marginParts < 0)
	{
		return {};
	}
	std::vector<int> parts(donorRanges.size());
	parts.resize(nzbget_rs_stream_patch_parts(donorRanges.data(), donorRanges.size(),
		holes.data(), holes.size(), marginParts, parts.data()));
	return parts;
}

std::vector<int> DupeStreamRepair::SelectProbeParts(const StreamRangeList& donorRanges,
	const StreamRangeList& holes, int probeCount)
{
	if (probeCount <= 0 || donorRanges.empty())
	{
		return {};
	}
	std::vector<int> probes(donorRanges.size());
	size_t count = nzbget_rs_stream_select_probe_parts(
		reinterpret_cast<const NzbgetRsByteRange*>(donorRanges.data()), donorRanges.size(),
		reinterpret_cast<const NzbgetRsByteRange*>(holes.data()), holes.size(),
		probeCount, probes.data(), probes.size());
	probes.resize(count);
	return probes;
}

void DupeStreamRepair::SubtractCovered(StreamRangeList& ranges, const StreamRange& covered)
{
	std::vector<NzbgetRsByteRange> out(ranges.size() * 2 + 1);
	NzbgetRsByteRange cov{covered.Offset, covered.Size};
	size_t count = nzbget_rs_stream_subtract_covered(
		reinterpret_cast<const NzbgetRsByteRange*>(ranges.data()), ranges.size(),
		cov, out.data(), out.size());
	ranges.clear();
	ranges.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		ranges.push_back({out[i].offset, out[i].size});
	}
}

inline int64 DupeStreamRepair::RequiredCompareFloorAvailable(const StreamRange* ranges, size_t count,
	const StreamRangeList& donorRanges, const std::vector<int>& probeParts)
{
	int64 achievable = 0;
	const StreamRange* end = ranges + count;
	for (int partIndex : probeParts)
	{
		const auto& donor = donorRanges[partIndex];
		int64 bytes = donor.Size;
		auto start = count <= 8 ? ranges : std::lower_bound(ranges, end, donor.Offset,
			[](const auto& hole, int64 offset) { return hole.End() <= offset; });
		for (auto hole = start; hole != end && hole->Offset < donor.End(); ++hole)
		{
			if (hole->End() <= donor.Offset) continue;
			bytes -= std::max<int64>(0, std::min(donor.End(), hole->End()) - std::max(donor.Offset, hole->Offset));
		}
		achievable += bytes;
	}
	return achievable;
}

int64 DupeStreamRepair::RequiredCompareFloorExact(const StreamRangeList& holes,
	const StreamRangeList& donorRanges, const std::vector<int>& probeParts)
{
	int64 achievable = 0;
	StreamRangeList present, remaining;
	for (int partIndex : probeParts)
	{
		// Reset contents while retaining both capacities across ordered probes.
		present.clear();
		present.push_back(donorRanges[partIndex]);
		for (const auto& hole : holes)
		{
			remaining.clear();
			for (const auto& range : present)
			{
				if (!RangesIntersect(range, hole)) remaining.push_back(range);
				else
				{
					if (range.Offset < hole.Offset) remaining.push_back({range.Offset, hole.Offset - range.Offset});
					if (hole.End() < range.End()) remaining.push_back({hole.End(), range.End() - hole.End()});
				}
			}
			present.swap(remaining);
		}
		achievable += TotalSize(present);
	}
	return achievable;
}

int64 DupeStreamRepair::RequiredCompareFloorUnordered(const StreamRangeList& holes,
	const StreamRangeList& donorRanges, const std::vector<int>& probeParts)
{
	// Eight is the existing linear-scan limit. Every scratch element is
	// constructed; compaction writes only within the populated prefix.
	std::array<StreamRange, 8> small{};
	StreamRangeList large;
	StreamRange* ranges;
	if (holes.size() <= small.size())
	{
		std::copy(holes.begin(), holes.end(), small.begin());
		ranges = small.data();
	}
	else
	{
		large = holes;
		ranges = large.data();
	}
	std::sort(ranges, ranges + holes.size(), [](auto& left, auto& right) { return left.Offset < right.Offset; });
	size_t count = 0;
	for (size_t i = 0; i < holes.size(); ++i)
	{
		auto hole = ranges[i];
		if (count && hole.Offset <= ranges[count - 1].End())
		{
			auto& last = ranges[count - 1];
			last.Size = std::max(last.End(), hole.End()) - last.Offset;
		}
		else ranges[count++] = hole;
	}
	return RequiredCompareFloorAvailable(ranges, count, donorRanges, probeParts);
}

int64 DupeStreamRepair::RequiredCompareFloor(int64 decodedFileSize, const StreamRangeList& holes,
	const StreamRangeList& donorRanges, const std::vector<int>& probeParts)
{
	static_assert(MinProbeCompareBytes == 16 * 1024, "Rust stream compare floor cap");
	static_assert(std::is_standard_layout<StreamRange>::value, "StreamRange C ABI layout");
	static_assert(sizeof(StreamRange) == sizeof(NzbgetRsStreamCompareRange), "StreamRange C ABI size");
	static_assert(alignof(StreamRange) == alignof(NzbgetRsStreamCompareRange), "StreamRange C ABI alignment");
	static_assert(offsetof(StreamRange, Offset) == offsetof(NzbgetRsStreamCompareRange, offset), "StreamRange offset");
	static_assert(offsetof(StreamRange, Size) == offsetof(NzbgetRsStreamCompareRange, size), "StreamRange size");
	static_assert(sizeof(int) == sizeof(int32_t), "Probe index C ABI size");
	// No probes need only the original base calculation; avoid FFI overhead.
	if (probeParts.empty())
	{
		return BaseCompareFloor(decodedFileSize - TotalSize(holes));
	}
	// No holes need only the raw donor-size sum, including reversed sizes.
	if (holes.empty())
	{
		int64 achievable = 0;
		for (int partIndex : probeParts)
		{
			if ((unsigned)partIndex >= donorRanges.size()) std::abort();
			achievable += donorRanges[partIndex].Size;
		}
		int64 base = BaseCompareFloor(decodedFileSize);
		return achievable >= 64 ? std::min(base, achievable) : base;
	}
	// Prospectively fixed from the measured native large-input win. Small
	// inputs retain native C++ accounting; this is a hybrid, not an all-Rust port.
	if (holes.size() >= 4096)
	{
		return nzbget_rs_stream_compare_floor(decodedFileSize,
			reinterpret_cast<const NzbgetRsStreamCompareRange*>(holes.data()), holes.size(),
			reinterpret_cast<const NzbgetRsStreamCompareRange*>(donorRanges.data()), donorRanges.size(),
			probeParts.data(), probeParts.size());
	}
	// Classification must not evaluate overflowing signed endpoints which the
	// original subtraction can leave unused after an earlier hole consumes it.
	auto wrappedEnd = [](const StreamRange& range)
	{
		return std::bit_cast<int64>((uint64_t)range.Offset + (uint64_t)range.Size);
	};
	int64 sum = 0, previous = std::numeric_limits<int64>::min();
	bool ordered = true, positive = true;
	for (const auto& hole : holes)
	{
		sum += hole.Size;
		int64 end = wrappedEnd(hole);
		positive &= hole.Size >= 0;
		positive &= end >= hole.Offset;
		ordered &= hole.Offset >= previous;
		previous = end;
	}
	int64 base = BaseCompareFloor(decodedFileSize - sum);
	for (int partIndex : probeParts)
	{
		if (partIndex < 0 || (size_t)partIndex >= donorRanges.size()) std::abort();
		positive &= donorRanges[partIndex].Size >= 0;
		positive &= wrappedEnd(donorRanges[partIndex]) >= donorRanges[partIndex].Offset;
	}
	int64 achievable = !positive ? RequiredCompareFloorExact(holes, donorRanges, probeParts) :
		!ordered ? RequiredCompareFloorUnordered(holes, donorRanges, probeParts) :
		RequiredCompareFloorAvailable(holes.data(), holes.size(), donorRanges, probeParts);
	return achievable >= 64 ? std::min(base, achievable) : base;
}

bool DupeStreamRepair::BuildRepairJob(FileInfo* fileInfo, const char* diskBasename)
{
	if (nzbget_rs_dcf_stream_repair_should_skip_build_job(
		(int)g_Options->GetDupeArticleFallback(),
		(int)Options::dafStream,
		g_Options->GetRawArticle() ? 1 : 0,
		DupeArticleFallback::IsParFile(fileInfo) ? 1 : 0,
		diskBasename) != 0)
	{
		return false;
	}

	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	// a health cancel keeps the files for "Download remaining files": the file
	// whose last article set it off still takes its job
	if (!nzbget_rs_cdn_stream_repair_is_nzb_eligible_for_job(
		nzbInfo ? 1 : 0,
		nzbInfo ? (int)nzbInfo->GetDeleteStatus() : 0,
		(int)NzbInfo::dsNone,
		(int)NzbInfo::dsHealth,
		nzbInfo && nzbInfo->GetDeleting() ? 1 : 0,
		nzbInfo && nzbInfo->GetParking() ? 1 : 0,
		nzbInfo && nzbInfo->GetPostInfo() != nullptr ? 1 : 0))
	{
		// the PostInfo check: files completing AFTER post-processing started
		// (par-check unpauses par2 volumes mid-repair) must not re-arm the
		// already-drained job list for a second stream-repair pass
		return false;
	}

	if (Util::EmptyStr(diskBasename) || fileInfo->GetArticles()->empty())
	{
		return false;
	}

	// A file none of whose articles arrived has no decoded size and nothing
	// on disk: the whole file is its hole. It can only be recreated from a
	// donor member identified structurally and proven on the collection's
	// other files (see StreamRepairController::RepairWholeFile).
	if (fileInfo->GetSuccessArticles() == 0)
	{
		if (fileInfo->GetSize() <= 0)
		{
			return false;
		}
		nzbInfo->GetStreamRepairJobs()->emplace_back(fileInfo->GetId(), diskBasename,
			0, fileInfo->GetFailedSize(), fileInfo->GetMissedSize(),
			fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles(), fileInfo->GetParFile(),
			StreamRangeList(), DupeArticleFallback::ArticleSizeStepsHash(fileInfo));
		return true;
	}

	// Data files qualify regardless of container: identity is decided by
	// probe byte-compares. Parity is excluded above because partial byte
	// matches cannot establish that another posting belongs to the PAR set.
	if (fileInfo->GetDecodedFileSize() <= 0)
	{
		return false;
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

FileInfo* DupeStreamRepair::SelectWholeFileDonor(const char* targetFilename, int64 targetEncodedSize,
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

std::string DupeStreamRepair::VolumeSetKey(const char* filename)
{
	NzbgetRsBuf buf = nzbget_rs_stream_volume_set_key(filename);
	std::string res(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return res;
}

bool DupeStreamRepair::DecodedSizePlausible(int64 decodedFileSize, int64 encodedSize)
{
	return nzbget_rs_stream_decoded_size_plausible(decodedFileSize, encodedSize) != 0;
}

std::string DupeStreamRepair::VolumeKey(const char* filename)
{
	NzbgetRsBuf buf = nzbget_rs_stream_volume_key(filename);
	std::string res(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return res;
}

std::string DupeStreamRepair::SuffixKey(const char* filename)
{
	NzbgetRsBuf buf = nzbget_rs_stream_suffix_key(filename);
	std::string res(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return res;
}

std::vector<FileInfo*> DupeStreamRepair::SelectDonorCandidates(const char* targetFilename,
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

std::string DupeStreamRepair::SelectExtractedInner(const char* dir, int64 innerSize, const char* innerName)
{
	int foundSymlink = 0;
	NzbgetRsBuf buf = nzbget_rs_stream_select_extracted_inner(dir, innerSize, innerName, &foundSymlink);
	if (foundSymlink)
	{
		info("Stream repair: archive contains link");
	}
	std::string res(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return res;
}

bool DupeStreamRepair::ExceedsDecompressCap(int64 totalBytes, int64 addBytes,
	int64 totalExtent, int64 addExtent)
{
	return nzbget_rs_stream_exceeds_decompress_cap(
		totalBytes, addBytes, totalExtent, addExtent) != 0;
}

std::string DupeStreamRepair::BuildDonorKey(NzbInfo* donorNzb, const char* password)
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
