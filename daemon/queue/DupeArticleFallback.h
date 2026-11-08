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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef DUPEARTICLEFALLBACK_H
#define DUPEARTICLEFALLBACK_H

#include <map>
#include <set>
#include <vector>
#include "DownloadInfo.h"

/*
 * Recovers articles missing on all news servers from duplicate collections
 * (same title or dupe-key) found in download queue or history. The failed
 * article is requeued with the message-id of the equivalent article of the
 * duplicate; residual damage from slightly different segment boundaries is
 * left to par-repair. See option <DupeArticleFallback>.
 *
 * Once a file has recovered enough articles from duplicates, it "cuts over":
 * subsequent articles lead with the duplicate instead of failing on the
 * primary first (which would waste a full server sweep per article). The
 * primary remains a revert source in case the duplicate is the one missing
 * that particular article.
 *
 * The duplicate a file leads with rotates as well, independently of the
 * cutover: whenever duplicates are tried at all, the file's lead duplicate is
 * tried first, and when that lead misses enough articles in a row it is
 * demoted and the next duplicate takes over the lead for fresh articles, so a
 * holed lead does not cost a wasted fetch per article when other duplicates
 * could serve them. The rotation is bounded: once every duplicate has led
 * without a single lead success it stops (the duplicates are equally holed
 * and rotating - and logging - would be pure noise) until a lead success
 * resets it.
 *
 * An article's source order is PINNED at its first fallback (PinSources): the
 * donors known at that moment, rotated to the file's lead, primary revert
 * inserted when cut over. The fallback round indexes into that pinned list,
 * so neither a lead rotation nor a queue/history change while the article is
 * in flight can shift the round->source mapping under it (which would skip or
 * repeat sources). The lead is identified by donor nzb-id, never by list
 * position: a lead-round result only counts towards demotion while the pinned
 * slot-0 donor is still the file's lead, so a burst of stale failures cannot
 * cascade-demote donors that were never tried, and a donor's streak can never
 * be charged to another donor even when candidate lists differ per part.
 *
 * All methods must be called within DownloadQueue-lock.
 */
class DupeArticleFallback
{
public:
	// after this many articles of a file are recovered from duplicates, lead
	// with the duplicate for the file's remaining articles (cutover)
	static constexpr int CutoverThreshold = 3;
	// after this many consecutive articles missed by the lead duplicate, the
	// next duplicate becomes the lead for the file's fresh articles
	static constexpr int LeadDemoteThreshold = 3;

	bool TryFallback(DownloadQueue* downloadQueue, FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* PAR2 identity is not established by filenames, sizes, or partial byte
	 * matches. Recognized parity must never be a duplicate target or donor. */
	static bool IsParFile(FileInfo* fileInfo);
	/* what the file's first article holds, read from disk if not known yet */
	static FileInfo::EFirstContent FirstContent(FileInfo* fileInfo);
	/* does the collection have par2 files (the only check a borrowed article's bytes get) */
	static bool HasPar2(NzbInfo* nzbInfo);

	/* Defer download-time donor recovery while parity is being discovered or
	 * a known PAR set has not failed ordinary repair. Must be called within
	 * DownloadQueue-lock, like TryFallback. */
	static bool ShouldDeferToPar(NzbInfo* nzbInfo);

	/* True when the collection's damaged data already exceeds all of its par2
	 * recovery data, or is projected to (B59): once ProjectionSample articles
	 * were tried, the failed share of the data tried so far, applied to all of
	 * its data, plus ParEdgeMargin (left out when withMargin is false: the
	 * 40-in-a-row rule), exceeds the par2 recovery data. Par-check
	 * can't be counted on then; download-time article fallback stops deferring.
	 * Must be called within DownloadQueue-lock. */
	static bool ParCannotCover(NzbInfo* nzbInfo, bool withMargin = true);
	// articles tried before the damage of a download is projected
	static constexpr int ProjectionSample = 200;
	// damage within this share of the data (per mille) of all that par2 can repair
	// is not left to par-check: the projection is rough and par2 needs whole blocks
	static constexpr int ParEdgeMargin = 50;
	// the largest par2 block size the check of borrowed articles reads (256 MiB)
	static constexpr int64 MaxPar2BlockSize = 256LL * 1024 * 1024;

	/* Finds the file of the duplicate collection which corresponds to the target
	 * data file: preferably by filename, otherwise by unambiguous structural identity
	 * (article count and sizes). Returns nullptr if no or multiple candidates. */
	static FileInfo* MatchDonorFile(FileInfo* targetFile, NzbInfo* donorNzb,
		const char* targetNzbFilename = nullptr, int donorNzbId = 0, const char* donorPrint = nullptr,
		bool twinOnly = false);
	/* The step fingerprint as a 64-bit hash (0 = the file has too few or too
	 * uniform articles to be identified this way), for pairing after the
	 * target's article list is gone. */
	static uint64 ArticleSizeStepsHash(FileInfo* fileInfo);
	static const char* FindDonorMessageId(FileInfo* donorFile, int partNumber);

	/* Rotates the donor list so the donor with the given nzb-id comes first
	 * (order among the others preserved); no-op when the id is 0 or absent. */
	static void RotateToLead(RawNzbList& donors, int leadNzbId);

	/* Pure pinning step: derives the article's lead bookkeeping (slot-0 donor,
	 * next-lead donor, distinct donor count) from the collected candidates and
	 * their contributing donor nzb-ids, lazily fixes the file's lead to the
	 * slot-0 donor when still undecided, and stores the ordered sources on the
	 * article. candidates/contributors run parallel (one contributor id per
	 * candidate, the donor that FIRST offered that message-id). */
	static void FinishPin(FileInfo* fileInfo, ArticleInfo* articleInfo,
		const std::vector<CString>& candidates, const std::vector<int>& contributors,
		bool cutover, const char* primaryMessageId);

	/* Counts a failed fetch of the article's pinned lead donor towards the
	 * file's consecutive lead-miss streak. Only a lead-round (round 1) result
	 * counts, and only while the pinned slot-0 donor is still the file's
	 * lead. Returns true when the streak reached LeadDemoteThreshold and the
	 * lead was rotated to the article's next donor (requires another donor
	 * and an unexhausted rotation budget; the caller logs the switch). Also
	 * called when a provisionally-accepted lead article is demoted later
	 * (QueueCoordinator::DemoteFinishedArticle): that fetch was in truth a
	 * lead miss and must not leave its bogus streak reset standing. */
	static bool RegisterLeadFailure(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Ends the file's lead-miss streak after a successful fetch from the
	 * current lead (same gating as RegisterLeadFailure). The rotation budget
	 * is re-armed only when the success is verifiable (the article's decoded
	 * boundaries were pinned by finished neighbours): an article accepted
	 * provisionally may be demoted again later, and its bogus success must
	 * not bypass the rotation bound. */
	static void RegisterLeadSuccess(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* A lead donor deleted from queue/history would freeze the lead
	 * accounting for good: nzb-ids are never reused, so its id could never
	 * match a collected donor again and the snapshot gate would block
	 * RegisterLeadFailure/Success for every fresh pin. Treat such a lead as
	 * vacated - the next pin re-decides the lead and the accounting starts
	 * clean. */
	static void VacateGhostLead(FileInfo* fileInfo, const RawNzbList& donors);

	/* Parses a collection's retained queued .nzb from disk into a standalone
	 * NzbInfo (with article lists loaded back in server mode). Returns nullptr
	 * if the file is missing or unparseable. Unlike GetParsedDonor this does
	 * not use or fill the per-instance donor cache; safe to call without the
	 * DownloadQueue lock. */
	static std::unique_ptr<NzbInfo> ParseDonorNzb(const char* queuedFilename);

	/* Decoded byte offset the article must begin at, derived from currently
	 * known geometry: 0 for the first article of the file, the end of an
	 * already-finished preceding article otherwise. -1 = not derivable yet.
	 * Guards substituted donor articles against decoded-boundary drift: the
	 * structural gate compares posted (encoded) sizes only, so a donor with
	 * the same article count and total but non-uniform segmentation can pass
	 * it while its decoded boundaries drift at interior articles. */
	static int64 ExpectedSegmentOffset(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Decoded byte offset the article must end at (exclusive): the begin of an
	 * already-finished following article, or the decoded file size for the last
	 * article of the file. -1 = not derivable yet. */
	static int64 ExpectedSegmentEnd(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Verifies the decoded byte range recorded for a finished article
	 * (SegmentOffset/SegmentSize) exactly abuts the currently known geometry
	 * (see ExpectedSegmentOffset/ExpectedSegmentEnd). A substituted article
	 * failing this check would leave a zero-filled gap and/or overwrite a
	 * neighbour's bytes, so it must not count as a successful download. */
	/* B40: the borrowed articles of the completed file <path> whose bytes don't match the
	 * block checksums of its par2 set (the par2 files already in the download's
	 * directory); none if no par2 data describes the file. Reads the disk: call
	 * without the queue lock, on a file no downloader writes any more */
	static std::vector<ArticleInfo*> BorrowedPar2Mismatches(FileInfo* fileInfo, const char* path,
		const char* destDir, const char* filename);
	static bool SegmentAligned(FileInfo* fileInfo, ArticleInfo* articleInfo);

	/* Whole-file decoded-boundary check, independent of the (non-persisted)
	 * fallback round. Returns the first finished article whose decoded range
	 * breaks the contiguous tiling of [0, DecodedFileSize) - the gap/overlap a
	 * mis-placed donor article leaves - or nullptr when the finished articles
	 * tile exactly OR the geometry cannot be judged (DecodedFileSize unknown /
	 * non-yEnc, an article without recorded placement, or an unfinished
	 * article). Meaningful only for a fully-downloaded yEnc file; used at
	 * completion to catch a provisionally-accepted drifted donor that survived
	 * a restart (reloaded as a plain finished article, so the round-gated
	 * checks no longer recognise it). Only meaningful for files that took donor
	 * bytes (see HasDonorArticles): a poster's own geometry may be unusual
	 * without being wrong, and such files must be written as without this
	 * feature. */
	static ArticleInfo* FirstUntiledArticle(FileInfo* fileInfo);

	/* True when any article of the file was substituted from a duplicate (its
	 * fallback round survives a restart through the staged flag of the file
	 * state). Files without donor articles are never judged by the geometry
	 * checks. */
	static bool HasDonorArticles(FileInfo* fileInfo);

	/* Folds the "=ybegin size=" of a successfully decoded article into the
	 * file's DecodedFileSize. Some posters randomise that field per article
	 * (the "=ypart" ranges stay correct); once two articles disagree the size
	 * is marked unknown (-1) for good, which turns off every size-based check
	 * for the file. Returns true only on the call that detects the
	 * disagreement. */
	static bool MergeDecodedFileSize(FileInfo* fileInfo, int64 articleFileSize);

private:
	// total declared size must match within 1/64 (~1.6%); the slack absorbs
	// differences in yEnc overhead (header line lengths, filenames)
	static constexpr int TotalSizeToleranceDiv = 64;
	// per-article declared size must match within 1/16 (~6%)
	static constexpr int PartSizeToleranceDiv = 16;
	// keep enough donors parsed to cover a whole dupe-set without re-parsing per
	// article; must comfortably exceed the number of duplicates of one release
	static constexpr int MaxCachedDonors = 16;
	// articles a file needs before its size steps are trusted as a fingerprint
	static constexpr int MinStepFingerprintArticles = 8;

	std::map<int, std::unique_ptr<NzbInfo>> m_parsedDonors;
	std::map<int, time_t> m_badDonors;	// donor id -> when its nzb-file couldn't be read
	static constexpr int BadDonorRetrySec = 600;
	static void AppendDonorCandidate(std::vector<CString>& candidates,
		std::vector<int>& contributors, int donorNzbId,
		NzbInfo* parsedDonor, FileInfo* targetFile, int partNumber,
		const char* targetNzbFilename = nullptr, const char* donorPrint = nullptr,
		bool twinOnly = false);
	static RawNzbList CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	// the subject filename of fileInfo in its own collection's nzb-file
	CString NzbFilenameOf(FileInfo* fileInfo);
	void PinSources(DownloadQueue* downloadQueue, FileInfo* fileInfo, ArticleInfo* articleInfo,
		bool twinOnly = false);
	NzbInfo* GetParsedDonor(NzbInfo* donorNzb);
};

#include <algorithm>
#include <atomic>
#include <climits>
#include <functional>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <vector>
#include "ServerPool.h"
#include "Options.h"
#include "DiskState.h"
#include "Log.h"
#include "Util.h"
#include "Util.h"
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

		bool Certain() const { return known && damagedBlocks > recoveryBlocks; }
		bool Projected() const
		{
			return known && doneBytes * 5 >= totalBytes &&
				(int64)projectedBlocks * 4 > (int64)recoveryBlocks * 5 + 8;
		}
	};

	static void FileCompleted(NzbInfo* nzbInfo, FileInfo* fileInfo, const std::string& filename);

	static Verdict Judge(NzbInfo* nzbInfo);

	static bool FirstReport(int nzbId)
	{
		return nzbget_rs_par_damage_first_report(nzbId) != 0;
	}

	static void Forget(int nzbId)
	{
		nzbget_rs_par_damage_forget(nzbId);
		nzbget_rs_streaming_par_forget(nzbId);
	}

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

class TwinCheck : public Service
{
public:
	static constexpr const char* FilesParam = "DupeFiles";	// fingerprint, or "none"
	static constexpr const char* KindParam = "DupeKind";	// "twin" or "alt" (of the primary)
	static constexpr const char* SampledParam = "DupeSampled";		// "twin", "alt" or "none"
	static constexpr const char* SampledOfParam = "DupeSampledOf";	// the primary's id
	static constexpr int SampleCount = 6;
	static constexpr int64 MaxIndexSize = 16LL * 1024 * 1024;
	static constexpr int MaxRunning = 2;

	struct FileSig
	{
		std::string name;
		uint64 length = 0;
		std::string md5;		// hex, the whole file
		std::string hash16k;	// hex, its first 16 KB
	};

	struct NzbEntry
	{
		std::string subject;
		std::string filename;	// the quoted name in the subject, or ""
		std::vector<CString> groups;
		std::vector<std::pair<int64, std::string>> segments;	// bytes, message-id with brackets
		int64 size = 0;
		bool par2 = false;		// its name says .par2
	};
	static std::vector<NzbEntry> ReadNzbEntries(const char* filename);
	static bool FetchEntry(ArticleFetcher& fetcher, const NzbEntry& entry, int64 maxSize,
		const std::function<bool()>& stopped, std::vector<char>& data);
	static uint64 BlockSize(const char* data, size_t size);
	static int VolumeBlocks(const std::string& filename);

	static std::vector<FileSig> ParsePar2(const char* data, size_t size);
	static std::string Fingerprint(const std::vector<FileSig>& sigs);

	static std::string Posting(NzbInfo* nzbInfo);
	static std::string Identity(NzbInfo* nzbInfo);
	static std::string PutPar2(int nzbId, const char* posting, const char* data, size_t size,
		bool* complete = nullptr);
	static int HasSigs(int nzbId, const char* identity);	// 0 none, 1 partial, 2 complete
	static void Prune(const std::string& live);
	static std::string Kind(int primaryId, const char* primaryPrint, int dupeId, const char* dupePrint);
	static std::string MatchByContent(int ownId, const char* ownPrint, const char* target, const char* targetAlt,
		int donorId, const char* donorPrint);
	static bool Available()
	{
		return true;
	}

	static void StopAll();
	static void WaitAll();
	static void Reset();

	TwinCheck();
	~TwinCheck() override;

	static void Nudge();
	static void NudgeLocked();

protected:
	int ServiceInterval() override;
	void ServiceWork() override;

private:
	class DownloadQueueObserver final : public Observer
	{
	public:
		TwinCheck* m_owner;
		void Update(Subject* caller, void* aspect) override;
	};

	DownloadQueueObserver m_observer;
};

#include "HistoryCoordinator.h"

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

namespace TwinCheckDetail
{

inline std::string RsString(NzbgetRsBuf buf)
{
	std::string text(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return text;
}

inline bool OpenStore()
{
	static std::string openedPath;
	static std::mutex mutex;
	if (!g_Options || Util::EmptyStr(g_Options->GetQueueDir()))
	{
		return false;
	}
	std::string path = RsString(nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "twincheck"));
	std::lock_guard<std::mutex> guard(mutex);
	if (openedPath != path)
	{
		nzbget_rs_twin_open(path.c_str());
		openedPath = std::move(path);
	}
	return true;
}

class TwinCheckJob;

inline Mutex g_mutex;
inline std::set<TwinCheckJob*> g_jobs;
inline std::set<int> g_checking;
inline std::atomic<int> g_jobCount{0};
inline bool g_stopping = false;
inline std::map<int, int> g_failedAttempts;
inline constexpr int MaxAttempts = 3;
inline TwinCheck* g_instance = nullptr;
inline std::atomic<bool> g_nudged{false};

inline int Attempts(int nzbId)
{
	Guard guard(g_mutex);
	auto it = g_failedAttempts.find(nzbId);
	return it != g_failedAttempts.end() ? it->second : 0;
}

class TwinCheckJob : public Thread
{
public:
	TwinCheckJob(int nzbId, std::string nzbFilename, std::string posting) :
		m_nzbId(nzbId), m_nzbFilename(std::move(nzbFilename)), m_posting(std::move(posting)) {}
	TwinCheckJob(int nzbId, std::string nzbFilename, int primaryId, std::string primaryNzbFilename) :
		m_nzbId(nzbId), m_nzbFilename(std::move(nzbFilename)), m_primaryId(primaryId),
		m_primaryNzbFilename(std::move(primaryNzbFilename)) {}

	void Cancel() { m_fetcher.Stop(); }
	int GetNzbId() const { return m_nzbId; }

protected:
	void Run() override;

private:
	int m_nzbId;
	std::string m_nzbFilename;
	std::string m_posting;
	int m_primaryId = 0;
	std::string m_primaryNzbFilename;
	ArticleFetcher m_fetcher;
	bool m_stored = false;

	void Sample();
	void Store(const char* name, const char* value, const char* name2 = nullptr, const char* value2 = nullptr);
	bool IndexData(std::vector<char>& data, bool& fetched);
};

inline bool TwinCheckJob::IndexData(std::vector<char>& data, bool& fetched)
{
	fetched = false;
	std::vector<TwinCheck::NzbEntry> entries = TwinCheck::ReadNzbEntries(m_nzbFilename.c_str());

	std::vector<uint8_t> entryPar2(entries.size());
	std::vector<int64_t> entrySizes(entries.size());
	for (size_t i = 0; i < entries.size(); i++)
	{
		entryPar2[i] = entries[i].par2 ? 1 : 0;
		entrySizes[i] = entries[i].size;
	}
	ptrdiff_t idx = nzbget_rs_twin_select_min_par2_index(
		entryPar2.data(), entrySizes.data(), entries.size(), TwinCheck::MaxIndexSize);
	if (idx < 0 || (size_t)idx >= entries.size())
	{
		return false;
	}

	fetched = true;
	return TwinCheck::FetchEntry(m_fetcher, entries[(size_t)idx], TwinCheck::MaxIndexSize, [this]() { return IsStopped(); }, data);
}

inline void TwinCheckJob::Run()
{
	struct Unregister
	{
		TwinCheckJob* job;
		~Unregister()
		{
			Guard guard(g_mutex);
			g_jobs.erase(job);
			g_checking.erase(job->GetNzbId());
			g_jobCount--;
			if (!g_stopping && job->m_stored)
			{
				TwinCheck::NudgeLocked();
			}
		}
	} unregister{this};

	if (m_primaryId)
	{
		Sample();
		return;
	}

	std::vector<char> data;
	bool fetched = false;
	bool readable = IndexData(data, fetched);
	if (IsStopped())
	{
		return;
	}
	bool complete = false;
	std::string fingerprint = readable ?
		TwinCheck::PutPar2(m_nzbId, m_posting.c_str(), data.data(), data.size(), &complete) : "";
	bool known = !fingerprint.empty();
	if (fetched && (!known || !complete))
	{
		Guard guard(g_mutex);
		if (++g_failedAttempts[m_nzbId] < MaxAttempts)
		{
			return;
		}
	}
	if (!known)
	{
		fingerprint = fetched ? "lost" : "none";
	}
	int files = known ? nzbget_rs_twin_parse_fingerprint_files(fingerprint.c_str()) : 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = nullptr;
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued->GetId() == m_nzbId)
		{
			nzbInfo = queued;
		}
	}
	for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
	{
		if (!nzbInfo && historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == m_nzbId)
		{
			nzbInfo = historyInfo->GetNzbInfo();
		}
	}
	if (nzbInfo)
	{
		nzbInfo->GetParameters()->SetParameter(TwinCheck::FilesParam, fingerprint.c_str());
		m_stored = true;
		nzbInfo->PrintMessage(Message::mkDetail, "Fingerprint of %s: %s (%i file(s) in its par2 set)",
			nzbInfo->GetName(), fingerprint.c_str(), files);
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

inline void TwinCheckJob::Store(const char* name, const char* value, const char* name2, const char* value2)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = nullptr;
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued->GetId() == m_nzbId)
		{
			nzbInfo = queued;
		}
	}
	for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
	{
		if (!nzbInfo && historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == m_nzbId)
		{
			nzbInfo = historyInfo->GetNzbInfo();
		}
	}
	if (nzbInfo)
	{
		nzbInfo->GetParameters()->SetParameter(name, value);
		if (name2)
		{
			nzbInfo->GetParameters()->SetParameter(name2, value2);
		}
		m_stored = true;
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

inline void TwinCheckJob::Sample()
{
	auto dataFiles = [](const std::string& nzbFilename)
		{
			std::vector<TwinCheck::NzbEntry> files;
			for (TwinCheck::NzbEntry& entry : TwinCheck::ReadNzbEntries(nzbFilename.c_str()))
			{
				if (!entry.par2)
				{
					files.push_back(std::move(entry));
				}
			}
			std::sort(files.begin(), files.end(), [](const TwinCheck::NzbEntry& a, const TwinCheck::NzbEntry& b)
				{
					return nzbget_rs_twin_compare_entry_names(
						a.filename.c_str(), a.subject.c_str(),
						b.filename.c_str(), b.subject.c_str()) != 0;
				});
			return files;
		};
	std::vector<TwinCheck::NzbEntry> primary = dataFiles(m_primaryNzbFilename);
	std::vector<TwinCheck::NzbEntry> dupe = dataFiles(m_nzbFilename);
	std::string primaryId = std::to_string(m_primaryId);

	std::vector<size_t> pSegCounts;
	std::vector<int64> pSegBytes;
	pSegCounts.reserve(primary.size());
	for (const TwinCheck::NzbEntry& e : primary)
	{
		pSegCounts.push_back(e.segments.size());
		for (const auto& seg : e.segments)
		{
			pSegBytes.push_back(seg.first);
		}
	}
	std::vector<size_t> dSegCounts;
	std::vector<int64> dSegBytes;
	dSegCounts.reserve(dupe.size());
	for (const TwinCheck::NzbEntry& e : dupe)
	{
		dSegCounts.push_back(e.segments.size());
		for (const auto& seg : e.segments)
		{
			dSegBytes.push_back(seg.first);
		}
	}
	std::vector<size_t> sampleFileIdx(TwinCheck::SampleCount, 0);
	std::vector<size_t> sampleSegIdx(TwinCheck::SampleCount, 0);
	if (!nzbget_rs_twin_sample_indices(
		pSegCounts.data(), pSegBytes.data(), pSegBytes.size(), primary.size(),
		dSegCounts.data(), dSegBytes.data(), dSegBytes.size(), dupe.size(),
		(size_t)TwinCheck::SampleCount,
		sampleFileIdx.data(), sampleSegIdx.data()))
	{
		Store(TwinCheck::SampledParam, "none", TwinCheck::SampledOfParam, primaryId.c_str());
		return;
	}

	int compared = 0;
	int differ = 0;
	for (int n = 0; n < TwinCheck::SampleCount && !IsStopped(); n++)
	{
		const TwinCheck::NzbEntry& a = primary[sampleFileIdx[(size_t)n]];
		const TwinCheck::NzbEntry& b = dupe[sampleFileIdx[(size_t)n]];
		size_t segIdx = sampleSegIdx[(size_t)n];
		ArticleFetcher::FetchedArticle partA = m_fetcher.Fetch(a.segments[segIdx].second.c_str(), a.groups);
		ArticleFetcher::FetchedArticle partB = m_fetcher.Fetch(b.segments[segIdx].second.c_str(), b.groups);
		nzbget_rs_twin_compare_sampled_pair(
			partA.Success ? 1 : 0,
			partB.Success ? 1 : 0,
			partA.Offset,
			partB.Offset,
			partA.Data == partB.Data ? 1 : 0,
			compared,
			differ,
			&compared,
			&differ);
	}
	const char* label = "";
	int verdict = nzbget_rs_twin_eval_sample_verdict(
		IsStopped() ? 1 : 0, compared, differ, TwinCheck::SampleCount, &label);
	if (verdict == 0)
	{
		return;
	}
	Store(TwinCheck::SampledParam, label,
		TwinCheck::SampledOfParam, primaryId.c_str());
}

struct Item
{
	NzbInfo* nzbInfo;
	bool queued;
	int rank;
};

} // namespace TwinCheckDetail

inline std::vector<TwinCheck::NzbEntry> TwinCheck::ReadNzbEntries(const char* filename)
{
	std::vector<NzbEntry> entries;
	nzbget_rs_twin_visit_nzb_entries(
		filename,
		&entries,
		[](void* ctx, int64_t size, int32_t par2,
		   const char* fnamePtr, size_t fnameLen,
		   const char* subjPtr, size_t subjLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			NzbEntry entry;
			entry.size = size;
			entry.par2 = par2 != 0;
			entry.filename.assign(fnamePtr, fnameLen);
			entry.subject.assign(subjPtr, subjLen);
			out->push_back(std::move(entry));
		},
		[](void* ctx, const char* groupPtr, size_t groupLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			if (!out->empty())
			{
				out->back().groups.emplace_back(std::string(groupPtr, groupLen).c_str());
			}
		},
		[](void* ctx, int64_t bytes, const char* idPtr, size_t idLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			if (!out->empty())
			{
				out->back().segments.emplace_back(bytes, std::string(idPtr, idLen));
			}
		});
	return entries;
}

inline uint64 TwinCheck::BlockSize(const char* data, size_t size)
{
	return nzbget_rs_par2_block_size((const unsigned char*)data, size);
}

inline int TwinCheck::VolumeBlocks(const std::string& filename)
{
	return nzbget_rs_par2_volume_blocks(filename.c_str());
}

inline std::vector<TwinCheck::FileSig> TwinCheck::ParsePar2(const char* data, size_t size)
{
	std::vector<FileSig> sigs;
	nzbget_rs_twin_visit_par2_sigs(
		(const uint8_t*)data, size, &sigs,
		[](void* ctx, uint64_t length, const char* md5Ptr, size_t md5Len,
		   const char* namePtr, size_t nameLen) noexcept
		{
			auto* out = static_cast<std::vector<FileSig>*>(ctx);
			FileSig sig;
			sig.length = length;
			sig.md5.assign(md5Ptr, md5Len);
			sig.name.assign(namePtr, nameLen);
			out->push_back(std::move(sig));
		});
	return sigs;
}

inline std::string TwinCheck::Posting(NzbInfo* nzbInfo)
{
	return std::to_string((uint32_t)nzbInfo->GetFullContentHash());
}

inline std::string TwinCheck::Identity(NzbInfo* nzbInfo)
{
	NzbParameter* print = nzbInfo->GetParameters()->Find(FilesParam);
	if (!print)
	{
		return "";
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_format_identity(
		print->GetValue(), Posting(nzbInfo).c_str()));
}

inline std::string TwinCheck::PutPar2(int nzbId, const char* posting, const char* data, size_t size,
	bool* complete)
{
	if (complete)
	{
		*complete = false;
	}
	if (!data || !size || !TwinCheckDetail::OpenStore())
	{
		return "";
	}
	std::string result = TwinCheckDetail::RsString(nzbget_rs_twin_put_par2(nzbId, posting ? posting : "",
		(const unsigned char*)data, size));
	int32_t completeFlag = 0;
	std::string fp = TwinCheckDetail::RsString(nzbget_rs_twin_split_put_par2_result(
		result.c_str(), &completeFlag));
	if (complete)
	{
		*complete = completeFlag != 0;
	}
	return fp;
}

inline int TwinCheck::HasSigs(int nzbId, const char* print)
{
	return TwinCheckDetail::OpenStore() ? nzbget_rs_twin_has(nzbId, print) : 0;
}

inline void TwinCheck::Prune(const std::string& live)
{
	if (TwinCheckDetail::OpenStore())
	{
		nzbget_rs_twin_prune(live.c_str());
	}
}

inline std::string TwinCheck::Kind(int primaryId, const char* primaryPrint, int dupeId, const char* dupePrint)
{
	if (!TwinCheckDetail::OpenStore())
	{
		return "";
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_kind(primaryId, primaryPrint, dupeId, dupePrint));
}

inline std::string TwinCheck::MatchByContent(int ownId, const char* ownPrint, const char* target, const char* targetAlt,
	int donorId, const char* donorPrint)
{
	if (!TwinCheckDetail::OpenStore())
	{
		return "";
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_match(ownId, ownPrint, target, targetAlt, donorId, donorPrint));
}

inline std::string TwinCheck::Fingerprint(const std::vector<FileSig>& sigs)
{
	std::vector<const char*> md5s;
	std::vector<unsigned long long> lengths;
	md5s.reserve(sigs.size());
	lengths.reserve(sigs.size());
	for (const FileSig& sig : sigs)
	{
		md5s.push_back(sig.md5.c_str());
		lengths.push_back(sig.length);
	}
	return TwinCheckDetail::RsString(nzbget_rs_twin_fingerprint(md5s.data(), lengths.data(), sigs.size()));
}

inline bool TwinCheck::FetchEntry(ArticleFetcher& fetcher, const NzbEntry& entry, int64 maxSize,
	const std::function<bool()>& stopped, std::vector<char>& data)
{
	data.clear();
	for (const auto& segment : entry.segments)
	{
		if (stopped())
		{
			return false;
		}
		ArticleFetcher::FetchedArticle part = fetcher.Fetch(segment.second.c_str(), entry.groups);
		size_t newDataLen = data.size();
		if (!nzbget_rs_twin_is_fetched_entry_part_valid(
			part.Success ? 1 : 0, part.Offset, part.Data.size(),
			maxSize, data.size(), &newDataLen))
		{
			continue;
		}
		if (newDataLen > data.size())
		{
			data.resize(newDataLen);
		}
		std::copy(part.Data.begin(), part.Data.end(), data.begin() + (size_t)part.Offset);
	}
	return !data.empty();
}

inline void TwinCheck::ServiceWork()
{
	using namespace TwinCheckDetail;
	g_nudged = false;
	if (!Available() || !g_Options->GetDupeCheck())
	{
		return;
	}

	struct Job
	{
		int nzbId;
		std::string nzbFilename;
		bool hinted;
		int primaryId = 0;
		std::string primaryNzbFilename;
		std::string posting;
		int rank = 0;
	};
	std::vector<Job> jobs;
	std::string live;
	static time_t lastPrune = 0;
	bool prune = Util::CurrentTime() - lastPrune >= 60 * 60 && g_jobCount == 0;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (prune)
		{
			lastPrune = Util::CurrentTime();
			auto add = [&live](NzbInfo* nzbInfo)
				{
					std::string id = Identity(nzbInfo);
					if (!id.empty())
					{
						live += std::to_string(nzbInfo->GetId()) + "\t" + id + "\n";
					}
				};
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				add(nzbInfo);
			}
			for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
			{
				if (historyInfo->GetKind() == HistoryInfo::hkNzb)
				{
					add(historyInfo->GetNzbInfo());
				}
			}
			if (live.empty())
			{
				live = "\n";
			}
		}
		bool changed = false;

		std::map<std::string, std::vector<Item>> keys;
		auto keyOf = [](NzbInfo* nzbInfo)
			{
				std::string k = nzbInfo->GetDupeKey() ? nzbInfo->GetDupeKey() : "";
				for (char& c : k)
				{
					c = (char)tolower((unsigned char)c);
				}
				return k;
			};
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetKind() == NzbInfo::nkNzb && !keyOf(nzbInfo).empty())
			{
				keys[keyOf(nzbInfo)].push_back({nzbInfo, true, 0});
			}
		}
		int rank = 0;
		for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
		{
			rank++;
			if (historyInfo->GetKind() == HistoryInfo::hkNzb && !keyOf(historyInfo->GetNzbInfo()).empty())
			{
				keys[keyOf(historyInfo->GetNzbInfo())].push_back({historyInfo->GetNzbInfo(), false, rank});
			}
		}

		auto isDupe = [](const Item& item)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				return nzbget_rs_twin_is_dupe_item(
					item.queued ? 1 : 0,
					nzbInfo->GetRemainingSize(),
					nzbInfo->GetPausedSize(),
					(int)nzbInfo->GetDeleteStatus(),
					(int)NzbInfo::dsDupe,
					(int)NzbInfo::dsCopy) != 0;
			};
		auto fingerprintOf = [](NzbInfo* nzbInfo)
			{
				NzbParameter* files = nzbInfo->GetParameters()->Find(FilesParam);
				return std::string(files ? files->GetValue() : "");
			};

		for (auto& entry : keys)
		{
			std::vector<Item>& items = entry.second;
			items.erase(std::remove_if(items.begin(), items.end(),
				[](const Item& item)
				{
					return strchr(item.nzbInfo->GetQueuedFilename(), '|') != nullptr;
				}),
				items.end());
			if (items.size() < 2)
			{
				continue;
			}
			size_t firstJob = jobs.size();
			int keyRank = INT_MAX;
			for (Item& item : items)
			{
				keyRank = std::min(keyRank, item.rank);
			}

			for (Item& item : items)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				std::string print = fingerprintOf(nzbInfo);
				bool listless = nzbget_rs_twin_is_print_usable(print.c_str()) != 0 &&
					HasSigs(nzbInfo->GetId(), Identity(nzbInfo).c_str()) < 2 &&
					Attempts(nzbInfo->GetId()) < MaxAttempts;
				bool isMerged = strchr(nzbInfo->GetQueuedFilename(), '|') != nullptr;
				if ((print.empty() || listless) && !isMerged &&
					FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
				{
					bool hinted = nzbInfo->GetParameters()->Find("DupeSameSize") != nullptr ||
						nzbInfo->GetParameters()->Find("DupeSameInner") != nullptr;
					Job job{nzbInfo->GetId(), nzbInfo->GetQueuedFilename(), hinted};
					job.posting = Posting(nzbInfo);
					jobs.push_back(std::move(job));
				}
			}

			NzbInfo* primary = nullptr;
			{
				std::vector<uint8_t> itemQueued;
				std::vector<uint8_t> itemIsDupe;
				std::vector<int> itemScores;
				itemQueued.reserve(items.size());
				itemIsDupe.reserve(items.size());
				itemScores.reserve(items.size());
				for (Item& item : items)
				{
					itemQueued.push_back(item.queued ? 1 : 0);
					itemIsDupe.push_back(isDupe(item) ? 1 : 0);
					itemScores.push_back(item.nzbInfo->GetDupeScore());
				}
				int primaryIdx = nzbget_rs_twin_select_primary(
					itemQueued.data(), itemIsDupe.data(), itemScores.data(), items.size());
				if (primaryIdx >= 0 && (size_t)primaryIdx < items.size())
				{
					primary = items[(size_t)primaryIdx].nzbInfo;
				}
			}

			std::string primaryPrint = primary ? fingerprintOf(primary) : "";
			bool primaryOk = primary &&
				strchr(primary->GetQueuedFilename(), '|') == nullptr &&
				FileSystem::FileExists(primary->GetQueuedFilename());
			std::string primaryId = primary ? std::to_string(primary->GetId()) : "";
			for (Item& item : items)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				std::string print = fingerprintOf(nzbInfo);
				bool byPar2 = nzbget_rs_twin_is_print_usable(primaryPrint.c_str()) != 0 &&
					nzbget_rs_twin_is_print_usable(print.c_str()) != 0;
				NzbParameter* sampled = nzbInfo->GetParameters()->Find(SampledParam);
				NzbParameter* sampledOf = nzbInfo->GetParameters()->Find(SampledOfParam);
				int sampledNowInt = 0;
				bool sampleable = nzbget_rs_twin_eval_sampleable(
					primaryOk ? 1 : 0,
					nzbInfo == primary ? 1 : 0,
					isDupe(item) ? 1 : 0,
					print.empty() ? 1 : 0,
					primaryPrint.empty() ? 1 : 0,
					byPar2 ? 1 : 0,
					sampled != nullptr ? 1 : 0,
					sampledOf ? sampledOf->GetValue() : nullptr,
					primaryId.c_str(),
					&sampledNowInt) != 0;
				bool sampledNow = sampledNowInt != 0;
				bool isMerged = strchr(nzbInfo->GetQueuedFilename(), '|') != nullptr;
				if (sampleable && !sampledNow && !isMerged &&
					FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
				{
					jobs.push_back({nzbInfo->GetId(), nzbInfo->GetQueuedFilename(), false,
						primary->GetId(), primary->GetQueuedFilename()});
				}
				std::string byFiles;
				bool listed = false;
				if (nzbInfo != primary && isDupe(item) && byPar2)
				{
					byFiles = Kind(primary->GetId(), Identity(primary).c_str(), nzbInfo->GetId(),
						Identity(nzbInfo).c_str());
					listed = HasSigs(primary->GetId(), Identity(primary).c_str()) > 0 ||
						HasSigs(nzbInfo->GetId(), Identity(nzbInfo).c_str()) > 0;
				}
				std::string kindResolved = RsString(nzbget_rs_twin_resolve_kind(
					nzbInfo == primary ? 1 : 0,
					isDupe(item) ? 1 : 0,
					byPar2 ? 1 : 0,
					byFiles.c_str(),
					listed ? 1 : 0,
					print == primaryPrint ? 1 : 0,
					sampleable ? 1 : 0,
					sampledNow ? 1 : 0,
					sampled ? sampled->GetValue() : ""));
				const char* kind = kindResolved.c_str();
				NzbParameter* old = nzbInfo->GetParameters()->Find(KindParam);
				if (nzbget_rs_dqh_bytes_equal(old ? old->GetValue() : "", kind) == 0)
				{
					nzbInfo->GetParameters()->SetParameter(KindParam, kind);
					changed = true;
					if (*kind)
					{
						std::string msg = RsString(nzbget_rs_twin_format_kind_message(
							nzbInfo->GetName(), primary->GetName(), kind, byPar2 ? 1 : 0));
						nzbInfo->PrintMessage(Message::mkInfo, "%s", msg.c_str());
					}
				}
			}

			for (size_t i = firstJob; i < jobs.size(); i++)
			{
				jobs[i].rank = keyRank;
			}
		}

		if (changed)
		{
			downloadQueue->HistoryChanged();
			downloadQueue->Save();
		}
	}

	if (!live.empty())
	{
		Guard guard(g_mutex);
		if (g_jobs.empty())
		{
			Prune(live);
		}
		else
		{
			lastPrune = 0;
		}
	}

	std::stable_sort(jobs.begin(), jobs.end(), [](const Job& a, const Job& b)
		{
			return a.rank != b.rank ? a.rank < b.rank : a.hinted > b.hinted;
		});

	Guard guard(g_mutex);
	for (Job& job : jobs)
	{
		if (g_stopping || (int)g_jobs.size() >= MaxRunning)
		{
			break;
		}
		if (g_checking.count(job.nzbId))
		{
			continue;
		}
		TwinCheckJob* thread = job.primaryId ?
			new TwinCheckJob(job.nzbId, job.nzbFilename, job.primaryId, job.primaryNzbFilename) :
			new TwinCheckJob(job.nzbId, job.nzbFilename, job.posting);
		g_jobs.insert(thread);
		g_checking.insert(job.nzbId);
		g_jobCount++;
		thread->SetAutoDestroy(true);
		thread->Start();
	}
}

inline void TwinCheck::StopAll()
{
	using namespace TwinCheckDetail;
	Guard guard(g_mutex);
	g_stopping = true;
	for (TwinCheckJob* job : g_jobs)
	{
		job->Stop();
		job->Cancel();
	}
}

inline void TwinCheck::WaitAll()
{
	while (TwinCheckDetail::g_jobCount > 0)
	{
		Util::Sleep(20);
	}
}

inline void TwinCheck::Reset()
{
	Guard guard(TwinCheckDetail::g_mutex);
	TwinCheckDetail::g_stopping = false;
}

inline TwinCheck::TwinCheck()
{
	Reset();
	{
		Guard guard(TwinCheckDetail::g_mutex);
		TwinCheckDetail::g_instance = this;
	}
	m_observer.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_observer);
}

inline TwinCheck::~TwinCheck()
{
	DownloadQueue::Guard()->Detach(&m_observer);
	Guard guard(TwinCheckDetail::g_mutex);
	TwinCheckDetail::g_instance = nullptr;
}

inline int TwinCheck::ServiceInterval()
{
	return TwinCheckDetail::g_nudged ? 1000 : 15000;
}

inline void TwinCheck::Nudge()
{
	Guard guard(TwinCheckDetail::g_mutex);
	NudgeLocked();
}

inline void TwinCheck::NudgeLocked()
{
	TwinCheckDetail::g_nudged = true;
	if (TwinCheckDetail::g_instance)
	{
		TwinCheckDetail::g_instance->WakeUp();
	}
}

inline void TwinCheck::DownloadQueueObserver::Update(Subject*, void* aspect)
{
	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaNzbAdded &&
		queueAspect->nzbInfo && !Util::EmptyStr(queueAspect->nzbInfo->GetDupeKey()))
	{
		TwinCheckDetail::g_nudged = true;
		m_owner->WakeUp();
	}
}

inline bool DupeArticleFallback::IsParFile(FileInfo* fileInfo)
{
	return nzbget_rs_dupe_fallback_is_par_file(
		fileInfo->GetParFile() ? 1 : 0,
		(int)fileInfo->GetFirstContent(),
		(int)FileInfo::fcPar2,
		fileInfo->GetFilenameConfirmed() ? 1 : 0,
		fileInfo->GetFilename()) != 0;
}

inline bool DupeArticleFallback::HasPar2(NzbInfo* nzbInfo)
{
	if (nzbInfo->GetParSize() > 0)
	{
		return true;
	}
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (IsParFile(fileInfo))
		{
			return true;
		}
	}
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_dupe_fallback_completed_file_is_par2(
			completedFile.GetParFile() ? 1 : 0, completedFile.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}

inline bool DupeArticleFallback::ShouldDeferToPar(NzbInfo* nzbInfo)
{
	int headerVerdict = nzbget_rs_dupe_fallback_should_defer_to_par_header(
		nzbInfo ? 1 : 0,
		nzbInfo ? (int)nzbInfo->GetDirectRenameStatus() : 0,
		(int)NzbInfo::tsRunning,
		(nzbInfo && nzbInfo->GetAllFirst()) ? 1 : 0,
		nzbInfo ? (int)nzbInfo->GetParStatus() : 0,
		(int)NzbInfo::psFailure,
		nzbInfo ? nzbInfo->GetParSize() : 0);
	if (headerVerdict != 0)
	{
		return headerVerdict > 0;
	}
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (!fileInfo->GetDeleted() && IsParFile(fileInfo))
		{
			return true;
		}
	}
	for (CompletedFile& fileInfo : nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_dupe_fallback_completed_file_is_par2(
			fileInfo.GetParFile() ? 1 : 0, fileInfo.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}

inline bool DupeArticleFallback::ParCannotCover(NzbInfo* nzbInfo, bool withMargin)
{
	return nzbget_rs_dupe_fallback_par_cannot_cover(
		nzbInfo->GetParSize(), nzbInfo->GetParCurrentFailedSize(),
		nzbInfo->GetCurrentFailedSize(), nzbInfo->GetCurrentSuccessSize(),
		nzbInfo->GetParCurrentSuccessSize(), nzbInfo->GetSize(),
		nzbInfo->GetCurrentSuccessArticles(), nzbInfo->GetCurrentFailedArticles(),
		withMargin ? 1 : 0) != 0;
}

inline bool DupeArticleFallback::TryFallback(DownloadQueue* downloadQueue, FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	if (g_Options->GetDupeArticleFallback() == Options::dafNone ||
		g_Options->GetRawArticle() ||
		IsParFile(fileInfo))
	{
		return false;
	}

	// a par2-file with a random name looks like any data file: what its first
	// article holds tells (that article itself may be borrowed: it's analyzed).
	// Until that's known only a name without an extension waits: one with an
	// extension of its own isn't a par2-file posing as data, and making every
	// file wait for its first article lost the articles failing before it
	FileInfo::EFirstContent content = FirstContent(fileInfo);
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	if (nzbget_rs_dupe_fallback_check_try_preconditions(
		(int)g_Options->GetDupeArticleFallback(),
		(int)Options::dafNone,
		g_Options->GetRawArticle() ? 1 : 0,
		IsParFile(fileInfo) ? 1 : 0,
		(int)content,
		(int)FileInfo::fcPar2,
		(int)FileInfo::fcUnknown,
		articleInfo->GetPartNumber(),
		fileInfo->GetFilename(),
		fileInfo->GetDeleted() ? 1 : 0,
		nzbInfo->GetDeleting() ? 1 : 0,
		nzbInfo->GetParking() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce) == 0)
	{
		return false;
	}

	// A borrowed article proves only that it arrived intact (its yEnc crc), not that
	// it is this file's: a duplicate of the same size and article layout but other
	// bytes (another encode) filled the holes, and the download ended SUCCESS with
	// the wrong bytes. Its par2 files are what checks borrowed bytes (BorrowedPar2-
	// Mismatches, par-check): without any, holes are left to stream repair, which
	// proves a duplicate's bytes before it copies them
	if (!HasPar2(nzbInfo))
	{
		if (!nzbInfo->GetDupeNoParNoted())
		{
			nzbInfo->SetDupeNoParNoted(true);
			nzbInfo->PrintMessage(Message::mkInfo,
				"Not borrowing articles of %s from duplicates: it has no par2 files to check them",
				nzbInfo->GetName());
		}
		return false;
	}

	bool defer = false;
	if (g_Options->GetDupeArticleFallback() >= Options::dafStream)
	{
		// The first article identifies its file (the 16k hash direct-rename
		// and par-rename go by, the archive headers) - losing it costs far
		// more than the single donor fetch that can recover it.
		// Once the damage outgrows every recovery byte the collection has,
		// single articles are borrowed again (whole-file stream recovery
		// keeps waiting for par-check, which knows which files it protects).
		// Once lifted, the wait stays lifted: a projection can fall back below the
		// par2 data (articles fetched from a lead duplicate count as arrived).
		// A par-check that runs only when asked for (ParCheck=manual) repairs
		// nothing by itself, and the stream repair of post-processing waits for
		// it too: a hole left to it stayed, while the "article" mode had filled
		// it. Then the articles are borrowed as in that mode.
		bool parDefer = g_Options->GetParCheck() != Options::pcManual && ShouldDeferToPar(nzbInfo);
		ParDamage::Verdict damage = parDefer ? ParDamage::Judge(nzbInfo) : ParDamage::Verdict();
		// par2 repairs whole blocks: the blocks the failed articles spoiled may
		// be more than its volumes hold long before the bytes are (ParDamage);
		// borrowing then starts, and the par-damage failover waits for it
		int liftDeferState = 0;
		int logLifted = 0;
		defer = nzbget_rs_dupe_fallback_eval_par_defer(
			parDefer ? 1 : 0,
			articleInfo->GetPartNumber(),
			(int)nzbInfo->GetDupeParDeferState(),
			(int)NzbInfo::dpDeferred,
			(int)NzbInfo::dpLifted,
			ParCannotCover(nzbInfo) ? 1 : 0,
			damage.Certain() ? 1 : 0,
			damage.Projected() ? 1 : 0,
			&liftDeferState,
			&logLifted) != 0;
		if (liftDeferState)
		{
			if (logLifted)
			{
				nzbInfo->PrintMessage(Message::mkInfo,
					"Damage of %s exceeds its par2 recovery data (or will, judged by the articles tried so far), "
					"recovering missing articles from duplicates",
					nzbInfo->GetName());
			}
			nzbInfo->SetDupeParDeferState(NzbInfo::dpLifted);
		}
	}

	int round = articleInfo->GetDupeFallbackRound();
	if (nzbget_rs_dupe_fallback_should_abort_deferred_try(
		defer ? 1 : 0, round, downloadQueue ? 1 : 0,
		articleInfo->GetDupeSourceDonors()->empty() ? 1 : 0) != 0)
	{
		return false;
	}

	// preserve the article's own (primary) message-id before the first
	// substitution, so it can be used as a revert source when cut over
	if (Util::EmptyStr(articleInfo->GetDupeOriginalMessageId()))
	{
		articleInfo->SetDupeOriginalMessageId(articleInfo->GetMessageId());
	}

	if (round == 0)
	{
		// the article's source order is decided once, at its first fallback:
		// the donors known now, rotated to the file's current lead, pinned on
		// the article so no later queue/history change or lead rotation can
		// shift the round->source mapping under it (which would skip or
		// repeat sources). The trade-off is deliberate: a duplicate appearing
		// later serves fresh articles, not articles already mid-fallback.
		// While deferring to par-check, only proven twins of this primary whose
		// par2 file list carries the exact same file bytes are pinned.
		PinSources(downloadQueue, fileInfo, articleInfo, defer);
	}
	else if (round == 1 && RegisterLeadFailure(fileInfo, articleInfo))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Switching lead duplicate collection for %s (the lead duplicate is missing many articles)",
			fileInfo->GetFilename());
	}

	std::vector<CString>& sources = *articleInfo->GetDupeSources();
	// sources of a duplicate whose articles failed the par2 checksums are passed over (B40)
	std::vector<int>& sourceDonors = *articleInfo->GetDupeSourceDonors();
	int pinnedRound = round;
	while (round >= 0 && (size_t)round < sources.size() && (size_t)round < sourceDonors.size() &&
		nzbInfo->GetDupeBlockedDonors()->count(sourceDonors[round]))
	{
		round++;
	}
	if (round >= (int)sources.size())
	{
		int checkLogParDefer = 0;
		int incUnsourced = 0;
		int restoreOrigMsgId = 0;
		nzbget_rs_dupe_fallback_resolve_exhausted_sources(
			defer ? 1 : 0,
			pinnedRound,
			articleInfo->GetDupeProactive() ? 1 : 0,
			(int)nzbInfo->GetDupeParDeferState(),
			(int)NzbInfo::dpNone,
			downloadQueue != nullptr ? 1 : 0,
			Util::EmptyStr(articleInfo->GetDupeOriginalMessageId()) ? 1 : 0,
			&checkLogParDefer,
			&incUnsourced,
			&restoreOrigMsgId);
		if (checkLogParDefer && !CollectDonors(downloadQueue, nzbInfo).empty())
		{
			nzbInfo->SetDupeParDeferState(NzbInfo::dpDeferred);
			nzbInfo->PrintMessage(Message::mkInfo,
				"Deferring duplicate recovery for %s to par-check (its par2 files may cover the damage)",
				nzbInfo->GetName());
		}
		if (incUnsourced)
		{
			nzbInfo->SetDupeUnsourcedArticles(nzbInfo->GetDupeUnsourcedArticles() + 1);
		}
		if (restoreOrigMsgId)
		{
			articleInfo->SetMessageId(articleInfo->GetDupeOriginalMessageId());
		}
		return false;
	}

	// count this article once, the first time a duplicate source is tried for it
	// (the denominator of the per-file recovered/attempted completion summary).
	// No per-attempt log line: it would flood large files and, worse, read as a
	// success when it is only an attempt - the recovery is counted on success.
	// A duplicate asked first after the cutover isn't an attempt on a missing article:
	// counted, it read as "recovered 11 of 2853 missing" for a file missing ~20
	if (pinnedRound == 0 && !articleInfo->GetDupeProactive())
	{
		fileInfo->SetDupeAttemptedArticles(fileInfo->GetDupeAttemptedArticles() + 1);
		nzbInfo->SetDupeAttemptedArticles(nzbInfo->GetDupeAttemptedArticles() + 1);
	}

	// pin the decoded byte range this article must occupy, as far as it is
	// already known from finished neighbour articles, so a donor article with
	// drifted decoded boundaries is rejected before its bytes are written
	articleInfo->SetDupeExpectedOffset(ExpectedSegmentOffset(fileInfo, articleInfo));
	articleInfo->SetDupeExpectedEnd(ExpectedSegmentEnd(fileInfo, articleInfo));

	articleInfo->SetMessageId(sources[round]);
	articleInfo->SetDupeFallbackRound(round + 1);

	return true;
}

inline void DupeArticleFallback::RotateToLead(RawNzbList& donors, int leadNzbId)
{
	std::vector<int32_t> donorIds;
	donorIds.reserve(donors.size());
	for (NzbInfo* donor : donors)
	{
		donorIds.push_back(donor->GetId());
	}
	size_t idx = nzbget_rs_dupe_fallback_find_rotate_offset(
		donorIds.data(), donorIds.size(), leadNzbId);
	if (idx > 0 && idx < donors.size())
	{
		std::rotate(donors.begin(), donors.begin() + idx, donors.end());
	}
}

inline std::vector<ArticleInfo*> DupeArticleFallback::BorrowedPar2Mismatches(FileInfo* fileInfo, const char* path,
	const char* destDir, const char* filename)
{
	ArticleList* articles = fileInfo->GetArticles();
	size_t n = articles->size();
	if (n == 0)
	{
		return {};
	}
	std::vector<int32_t> statuses(n);
	std::vector<int32_t> donorIds(n);
	std::vector<int32_t> fallbackRounds(n);
	std::vector<int64_t> segmentOffsets(n);
	std::vector<int32_t> segmentSizes(n);
	for (size_t i = 0; i < n; i++)
	{
		ArticleInfo* a = (*articles)[i].get();
		statuses[i] = (int32_t)a->GetStatus();
		donorIds[i] = a->GetDupeDonorId();
		fallbackRounds[i] = a->GetDupeFallbackRound();
		segmentOffsets[i] = a->GetSegmentOffset();
		segmentSizes[i] = a->GetSegmentSize();
	}
	std::vector<int32_t> outIndices(n);
	size_t count = nzbget_rs_dupe_fallback_borrowed_par2_mismatches(
		path, destDir, filename, statuses.data(), donorIds.data(),
		fallbackRounds.data(), segmentOffsets.data(), segmentSizes.data(),
		n,
		[](const uint8_t* data, size_t len) -> uint32_t
		{
			Crc32 crc;
			crc.Append(const_cast<uchar*>(data), (uint32)len);
			return crc.Finish();
		},
		outIndices.data(), outIndices.size());
	std::vector<ArticleInfo*> mismatches;
	mismatches.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		int32_t idx = outIndices[i];
		if (idx >= 0 && (size_t)idx < n)
		{
			mismatches.push_back((*articles)[idx].get());
		}
	}
	return mismatches;
}

inline void DupeArticleFallback::FinishPin(FileInfo* fileInfo, ArticleInfo* articleInfo,
	const std::vector<CString>& candidates, const std::vector<int>& contributors,
	bool cutover, const char* primaryMessageId)
{
	int leadSnapshot = 0;
	int nextLead = 0;
	int donorCount = 0;
	int newFileLead = fileInfo->GetDupeLeadDonorId();
	nzbget_rs_dupe_fallback_compute_pin_lead_state(
		contributors.empty() ? nullptr : contributors.data(),
		contributors.size(),
		fileInfo->GetDupeLeadDonorId(),
		&leadSnapshot, &nextLead, &donorCount, &newFileLead);
	articleInfo->SetDupeLeadSnapshot(leadSnapshot);
	articleInfo->SetDupeNextLead(nextLead);
	articleInfo->SetDupeDonorCount(donorCount);
	fileInfo->SetDupeLeadDonorId(newFileLead);

	std::vector<CString>& sources = *articleInfo->GetDupeSources();
	sources.clear();
	bool appendPrimary = cutover && !candidates.empty();
	sources.reserve(candidates.size() + (appendPrimary ? 1 : 0));
	for (const CString& candidate : candidates)
	{
		sources.emplace_back((const char*)candidate);
	}
	if (appendPrimary)
	{
		sources.emplace_back(primaryMessageId);
	}
	*articleInfo->GetDupeSourceDonors() = contributors;
}

inline bool DupeArticleFallback::RegisterLeadFailure(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	int leadDonorId = fileInfo->GetDupeLeadDonorId();
	int leadFailures = fileInfo->GetDupeLeadFailures();
	int leadSwitches = fileInfo->GetDupeLeadSwitches();
	bool rotated = nzbget_rs_dupe_fallback_register_lead_failure(
		articleInfo->GetDupeFallbackRound(), articleInfo->GetDupeLeadSnapshot(),
		articleInfo->GetDupeDonorCount(), articleInfo->GetDupeNextLead(),
		&leadDonorId, &leadFailures, &leadSwitches) != 0;
	fileInfo->SetDupeLeadDonorId(leadDonorId);
	fileInfo->SetDupeLeadFailures(leadFailures);
	fileInfo->SetDupeLeadSwitches(leadSwitches);
	return rotated;
}

inline void DupeArticleFallback::RegisterLeadSuccess(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	if (articleInfo->GetDupeFallbackRound() != 1 ||
		articleInfo->GetDupeLeadSnapshot() == 0 ||
		articleInfo->GetDupeLeadSnapshot() != fileInfo->GetDupeLeadDonorId())
	{
		return;
	}
	int leadFailures = fileInfo->GetDupeLeadFailures();
	int leadSwitches = fileInfo->GetDupeLeadSwitches();
	nzbget_rs_dupe_fallback_register_lead_success(
		articleInfo->GetDupeFallbackRound(), articleInfo->GetDupeLeadSnapshot(),
		fileInfo->GetDupeLeadDonorId(),
		ExpectedSegmentOffset(fileInfo, articleInfo),
		ExpectedSegmentEnd(fileInfo, articleInfo),
		&leadFailures, &leadSwitches);
	fileInfo->SetDupeLeadFailures(leadFailures);
	fileInfo->SetDupeLeadSwitches(leadSwitches);
}

inline void DupeArticleFallback::VacateGhostLead(FileInfo* fileInfo, const RawNzbList& donors)
{
	// a lead gone from queue and history, or barred after its articles failed the
	// par2 check (B40), is vacated: a barred one was passed over when sources were
	// pinned, so no article's lead matched it again and the lead rotation froze
	int leadNzbId = fileInfo->GetDupeLeadDonorId();
	bool barred = fileInfo->GetNzbInfo() && fileInfo->GetNzbInfo()->GetDupeBlockedDonors()->count(leadNzbId);
	std::vector<int32_t> donorIds;
	donorIds.reserve(donors.size());
	for (NzbInfo* donor : donors)
	{
		donorIds.push_back(donor->GetId());
	}
	if (nzbget_rs_dupe_fallback_should_vacate_ghost_lead(
		leadNzbId, barred ? 1 : 0,
		donorIds.empty() ? nullptr : donorIds.data(),
		donorIds.size()) == 0)
	{
		return;
	}

	fileInfo->SetDupeLeadDonorId(0);
	fileInfo->SetDupeLeadFailures(0);
	fileInfo->SetDupeLeadSwitches(0);
}

inline CString DupeArticleFallback::NzbFilenameOf(FileInfo* fileInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	if (articles->empty())
	{
		return CString();
	}

	// the collection's own nzb-file is parsed (and cached) like a donor's
	NzbInfo* ownNzb = GetParsedDonor(fileInfo->GetNzbInfo());
	if (!ownNzb)
	{
		return CString();
	}

	// The own entry is found by an article message-id. Substituted articles
	// carry a duplicate's id (their original id is not kept across an
	// unload/reload of the article list), so any article whose id is still
	// the posting's own identifies the file.
	for (FileInfo* ownFile : ownNzb->GetFileList())
	{
		ArticleList* ownArticles = ownFile->GetArticles();
		if (ownArticles->size() != articles->size())
		{
			continue;
		}
		for (size_t i = 0; i < articles->size(); i++)
		{
			ArticleInfo* article = (*articles)[i].get();
			ArticleInfo* ownArticle = (*ownArticles)[i].get();
			if (nzbget_rs_dupe_fallback_own_article_matches(
				ownArticle->GetPartNumber(), article->GetPartNumber(),
				ownArticle->GetMessageId(),
				article->GetDupeOriginalMessageId(),
				article->GetMessageId()) != 0)
			{
				return CString(ownFile->GetFilename());
			}
		}
	}
	return CString();
}

inline RawNzbList DupeArticleFallback::CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	RawNzbList donors;

	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		if (nzbget_rs_dupe_fallback_is_donor_eligible(
			queuedNzbInfo == nzbInfo ? 1 : 0,
			(int)queuedNzbInfo->GetKind(),
			(int)NzbInfo::nkNzb,
			(int)queuedNzbInfo->GetDupeMode(),
			(int)dmForce) != 0 &&
			DupeCoordinator::SameNameOrKey(queuedNzbInfo->GetName(), queuedNzbInfo->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			donors.push_back(queuedNzbInfo);
		}
	}

	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
			nzbget_rs_dupe_fallback_is_donor_eligible(
				0,
				(int)historyInfo->GetKind(),
				(int)HistoryInfo::hkNzb,
				(int)historyInfo->GetNzbInfo()->GetDupeMode(),
				(int)dmForce) != 0 &&
			DupeCoordinator::SameNameOrKey(historyInfo->GetNzbInfo()->GetName(),
				historyInfo->GetNzbInfo()->GetDupeKey(), nzbInfo->GetName(), nzbInfo->GetDupeKey()))
		{
			donors.push_back(historyInfo->GetNzbInfo());
		}
	}

	std::string ownIdentity = TwinCheck::Identity(nzbInfo);
	std::set<const NzbInfo*> twinDonors;
	if (!ownIdentity.empty())
	{
		for (NzbInfo* donor : donors)
		{
			if (nzbInfo->GetFullContentHash() > 0 &&
				nzbInfo->GetFullContentHash() == donor->GetFullContentHash())
			{
				continue;
			}
			std::string donorIdentity = TwinCheck::Identity(donor);
			std::string kind = !donorIdentity.empty() ?
				TwinCheck::Kind(nzbInfo->GetId(), ownIdentity.c_str(), donor->GetId(), donorIdentity.c_str()) : "";
			if (nzbget_rs_dupe_fallback_is_twin_kind(
				ownIdentity.c_str(), donorIdentity.c_str(), kind.c_str()) != 0)
			{
				twinDonors.insert(donor);
			}
		}
	}

	std::sort(donors.begin(), donors.end(),
		[&twinDonors](NzbInfo* donor1, NzbInfo* donor2)
		{
			bool twin1 = twinDonors.count(donor1) > 0;
			bool twin2 = twinDonors.count(donor2) > 0;
			return nzbget_rs_dupe_fallback_compare_donors(
				twin1 ? 1 : 0, donor1->GetDupeScore(), donor1->GetId(),
				twin2 ? 1 : 0, donor2->GetDupeScore(), donor2->GetId()) != 0;
		});

	return donors;
}

inline void DupeArticleFallback::PinSources(DownloadQueue* downloadQueue, FileInfo* fileInfo,
	ArticleInfo* articleInfo, bool twinOnly)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();

	RawNzbList donors = CollectDonors(downloadQueue, nzbInfo);
	VacateGhostLead(fileInfo, donors);
	if (twinOnly && fileInfo->GetDupeLeadDonorId() != 0)
	{
		std::string ownIdentity = TwinCheck::Identity(nzbInfo);
		int leadId = fileInfo->GetDupeLeadDonorId();
		auto it = std::find_if(donors.begin(), donors.end(),
			[leadId](NzbInfo* donor) { return donor->GetId() == leadId; });
		std::string leadIdentity = it != donors.end() ? TwinCheck::Identity(*it) : "";
		std::string leadKind = (!ownIdentity.empty() && !leadIdentity.empty()) ?
			TwinCheck::Kind(nzbInfo->GetId(), ownIdentity.c_str(), leadId, leadIdentity.c_str()) : "";
		if (nzbget_rs_dupe_fallback_should_reset_non_twin_lead(
			twinOnly ? 1 : 0, leadId, ownIdentity.c_str(), leadIdentity.c_str(), leadKind.c_str()) != 0)
		{
			fileInfo->SetDupeLeadDonorId(0);
			fileInfo->SetDupeLeadFailures(0);
			fileInfo->SetDupeLeadSwitches(0);
		}
	}
	RotateToLead(donors, fileInfo->GetDupeLeadDonorId());

	// the name the target's own nzb-file gives this file: the downloaded name
	// may have been replaced by an (obfuscated) name from the article itself,
	// while duplicate nzb-files still carry the subject names
	CString targetNzbFilename = donors.empty() ? CString() : NzbFilenameOf(fileInfo);

	std::vector<CString> candidates;
	std::vector<int> contributors;
	int samePosting = 0;
	int unparsed = 0;

	for (NzbInfo* donorNzbInfo : donors)
	{
		// one whose articles failed the par2 checksums of this download is out (B40)
		if (nzbInfo->GetDupeBlockedDonors()->count(donorNzbInfo->GetId()))
		{
			continue;
		}

		// an exact copy of the same posting shares the message-ids and cannot help
		if (nzbInfo->GetFullContentHash() > 0 &&
			nzbInfo->GetFullContentHash() == donorNzbInfo->GetFullContentHash())
		{
			samePosting++;
			continue;
		}

		// Extract this donor's candidate immediately: GetParsedDonor may evict a
		// previously-parsed donor from the bounded cache, so a parsed-donor pointer
		// must never be held across another GetParsedDonor call (use-after-free).
		NzbInfo* parsedDonor = GetParsedDonor(donorNzbInfo);
		if (parsedDonor)
		{
			// the donor's par2 fingerprint: its file list is matched by content (twin.rs)
			std::string donorIdentity = TwinCheck::Identity(donorNzbInfo);
			AppendDonorCandidate(candidates, contributors, donorNzbInfo->GetId(),
				parsedDonor, fileInfo, articleInfo->GetPartNumber(), targetNzbFilename,
				donorIdentity.c_str(), twinOnly);
		}
		else
		{
			unparsed++;
		}
	}

	if (!twinOnly && candidates.empty() && !donors.empty() && !fileInfo->GetDupeNoSourceLogged())
	{
		// the silent "did not even try" case: say why no duplicate could help,
		// once per file (it was once per missing article: dozens a second)
		fileInfo->SetDupeNoSourceLogged(true);
		nzbInfo->PrintMessage(Message::mkDetail,
			"No duplicate source for %s [%i]: %i duplicate(s), %i same posting, "
			"%i without readable nzb-file, none with a matching file (nzb name: %s); "
			"not repeated for its other articles",
			fileInfo->GetFilename(), articleInfo->GetPartNumber(), (int)donors.size(),
			samePosting, unparsed, targetNzbFilename.Empty() ? "unknown" : *targetNzbFilename);
	}

	FinishPin(fileInfo, articleInfo, candidates, contributors,
		fileInfo->GetDupeCutover(), articleInfo->GetDupeOriginalMessageId());
}

inline void DupeArticleFallback::AppendDonorCandidate(std::vector<CString>& candidates,
	std::vector<int>& contributors, int donorNzbId,
	NzbInfo* parsedDonor, FileInfo* targetFile, int partNumber, const char* targetNzbFilename,
	const char* donorPrint, bool twinOnly)
{
	FileInfo* donorFile = MatchDonorFile(targetFile, parsedDonor, targetNzbFilename,
		donorNzbId, donorPrint, twinOnly);
	if (!donorFile)
	{
		return;
	}

	const char* messageId = FindDonorMessageId(donorFile, partNumber);
	if (!messageId)
	{
		return;
	}

	bool duplicate = std::find_if(candidates.begin(), candidates.end(),
		[messageId](const CString& candidate) { return candidate == messageId; }) != candidates.end();
	if (!duplicate)
	{
		candidates.emplace_back(messageId);
		contributors.push_back(donorNzbId);
	}
}

inline std::unique_ptr<NzbInfo> DupeArticleFallback::ParseDonorNzb(const char* queuedFilename)
{
	if (Util::EmptyStr(queuedFilename) || !FileSystem::FileExists(queuedFilename))
	{
		return nullptr;
	}

	NzbFile nzbFile(queuedFilename, "");
	if (!nzbFile.Parse())
	{
		detail("Could not parse duplicate nzb-file %s", queuedFilename);
		return nullptr;
	}

	std::unique_ptr<NzbInfo> parsedNzb = nzbFile.DetachNzbInfo();

	if (g_Options->GetServerMode())
	{
		// in server mode the parser offloads article lists to disk-state and
		// clears them from memory; load them back (the donor is used in memory
		// only) and remove the disk-state files written as parse side effect
		for (FileInfo* fileInfo : parsedNzb->GetFileList())
		{
			g_DiskState->LoadArticles(fileInfo);
		}
		g_DiskState->DiscardFiles(parsedNzb.get(), false);
	}

	return parsedNzb;
}

inline NzbInfo* DupeArticleFallback::GetParsedDonor(NzbInfo* donorNzbInfo)
{
	int donorId = donorNzbInfo->GetId();

	// a duplicate whose nzb-file couldn't be read is asked again after a while: the
	// failure may have been passing (review item 9)
	auto bad = m_badDonors.find(donorId);
	if (bad != m_badDonors.end() && nzbget_rs_dupe_fallback_is_bad_donor_in_cooldown(
		(int64)Util::CurrentTime(), (int64)bad->second, BadDonorRetrySec) != 0)
	{
		return nullptr;
	}

	auto it = m_parsedDonors.find(donorId);
	if (it != m_parsedDonors.end())
	{
		return it->second.get();
	}

	// duplicates in history don't keep their article lists in memory; the
	// retained source nzb-file is parsed again instead (like "Download again")
	std::unique_ptr<NzbInfo> parsedNzb = ParseDonorNzb(donorNzbInfo->GetQueuedFilename());
	if (!parsedNzb)
	{
		m_badDonors[donorId] = Util::CurrentTime();
		return nullptr;
	}

	if ((int)m_parsedDonors.size() >= MaxCachedDonors)
	{
		m_parsedDonors.erase(m_parsedDonors.begin());
	}

	// store the parsed collection in the cache and return a pointer to the
	// owned object (the cache keeps it alive for the daemon's lifetime)
	std::unique_ptr<NzbInfo>& cachedDonor = m_parsedDonors[donorId];
	cachedDonor = std::move(parsedNzb);

	return cachedDonor.get();
}

inline FileInfo* DupeArticleFallback::MatchDonorFile(FileInfo* targetFile, NzbInfo* donorNzb,
	const char* targetNzbFilename, int donorNzbId, const char* donorPrint, bool twinOnly)
{
	if (IsParFile(targetFile))
	{
		return nullptr;
	}

	NzbInfo* own = targetFile->GetNzbInfo();
	std::string ownIdentity = own ? TwinCheck::Identity(own) : "";
	bool twinAllowed = false;
	if (twinOnly)
	{
		std::string kind = (donorNzbId && !ownIdentity.empty() && !Util::EmptyStr(donorPrint)) ?
			TwinCheck::Kind(own->GetId(), ownIdentity.c_str(), donorNzbId, donorPrint) : "";
		twinAllowed = donorNzbId && nzbget_rs_dupe_fallback_is_twin_kind(
			ownIdentity.c_str(), donorPrint, kind.c_str()) != 0;
		if (!twinAllowed)
		{
			return nullptr;
		}
	}
	std::string byContent = (donorNzbId && !ownIdentity.empty() && !Util::EmptyStr(donorPrint)) ?
		TwinCheck::MatchByContent(own->GetId(), ownIdentity.c_str(), targetFile->GetFilename(),
			targetNzbFilename, donorNzbId, donorPrint) : "";

	ArticleList* targetArticles = targetFile->GetArticles();
	std::vector<int32_t> targetParts(targetArticles->size());
	std::vector<int32_t> targetSizes(targetArticles->size());
	for (size_t i = 0; i < targetArticles->size(); i++)
	{
		targetParts[i] = (*targetArticles)[i]->GetPartNumber();
		targetSizes[i] = (*targetArticles)[i]->GetSize();
	}
	NzbgetRsDonorFileSpec targetSpec{};
	targetSpec.filename = targetFile->GetFilename();
	targetSpec.subject = targetFile->GetSubject();
	targetSpec.total_articles = targetFile->GetTotalArticles();
	targetSpec.size = targetFile->GetSize();
	targetSpec.is_par_file = 0;
	targetSpec.parts = targetParts.empty() ? nullptr : targetParts.data();
	targetSpec.sizes = targetSizes.empty() ? nullptr : targetSizes.data();
	targetSpec.article_count = targetParts.size();

	FileList* fileList = donorNzb->GetFileList();
	size_t n = fileList->size();
	std::vector<std::vector<int32_t>> donorPartsBuf(n);
	std::vector<std::vector<int32_t>> donorSizesBuf(n);
	std::vector<NzbgetRsDonorFileSpec> donorSpecs(n);
	for (size_t i = 0; i < n; i++)
	{
		FileInfo* df = (*fileList)[i].get();
		ArticleList* da = df->GetArticles();
		donorPartsBuf[i].resize(da->size());
		donorSizesBuf[i].resize(da->size());
		for (size_t j = 0; j < da->size(); j++)
		{
			donorPartsBuf[i][j] = (*da)[j]->GetPartNumber();
			donorSizesBuf[i][j] = (*da)[j]->GetSize();
		}
		donorSpecs[i].filename = df->GetFilename();
		donorSpecs[i].subject = df->GetSubject();
		donorSpecs[i].total_articles = df->GetTotalArticles();
		donorSpecs[i].size = df->GetSize();
		donorSpecs[i].is_par_file = IsParFile(df) ? 1 : 0;
		donorSpecs[i].parts = donorPartsBuf[i].empty() ? nullptr : donorPartsBuf[i].data();
		donorSpecs[i].sizes = donorSizesBuf[i].empty() ? nullptr : donorSizesBuf[i].data();
		donorSpecs[i].article_count = donorPartsBuf[i].size();
	}

	ptrdiff_t idx = nzbget_rs_dupe_fallback_match_donor_file(
		&targetSpec,
		targetNzbFilename,
		byContent.c_str(),
		twinAllowed ? 1 : 0,
		twinOnly ? 1 : 0,
		donorSpecs.empty() ? nullptr : donorSpecs.data(),
		donorSpecs.size());
	return (idx >= 0 && (size_t)idx < n) ? (*fileList)[(size_t)idx].get() : nullptr;
}

inline uint64 DupeArticleFallback::ArticleSizeStepsHash(FileInfo* fileInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	if ((int)articles->size() < MinStepFingerprintArticles)
	{
		return 0;
	}

	std::vector<int> sizes(articles->size());
	for (size_t i = 0; i < articles->size(); i++)
	{
		sizes[i] = (*articles)[i]->GetSize();
	}
	return nzbget_rs_dupe_fallback_steps_hash(sizes.data(), sizes.size());
}

inline const char* DupeArticleFallback::FindDonorMessageId(FileInfo* donorFile, int partNumber)
{
	for (ArticleInfo* article : donorFile->GetArticles())
	{
		if (article->GetPartNumber() == partNumber && !Util::EmptyStr(article->GetMessageId()))
		{
			return article->GetMessageId();
		}
	}

	return nullptr;
}

static inline int FindArticleIndex(ArticleList* articles, ArticleInfo* articleInfo)
{
	for (size_t i = 0; i < articles->size(); i++)
	{
		if ((*articles)[i].get() == articleInfo)
		{
			return (int)i;
		}
	}
	return -1;
}

inline int64 DupeArticleFallback::ExpectedSegmentOffset(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	int index = FindArticleIndex(articles, articleInfo);
	if (index < 0)
	{
		return -1;
	}

	ArticleInfo* prev = index > 0 ? (*articles)[index - 1].get() : nullptr;
	return nzbget_rs_dupe_fallback_expected_offset(
		index, articleInfo->GetPartNumber(),
		prev ? prev->GetPartNumber() : 0,
		prev ? (int)prev->GetStatus() : 0,
		prev ? prev->GetSegmentOffset() : 0,
		prev ? prev->GetSegmentSize() : 0);
}

inline int64 DupeArticleFallback::ExpectedSegmentEnd(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	int index = FindArticleIndex(articles, articleInfo);
	if (index < 0)
	{
		return -1;
	}

	ArticleInfo* next = (index + 1 < (int)articles->size()) ? (*articles)[index + 1].get() : nullptr;
	return nzbget_rs_dupe_fallback_expected_end(
		index, articles->size(), articleInfo->GetPartNumber(),
		fileInfo->GetDecodedFileSize(),
		next ? next->GetPartNumber() : 0,
		next ? (int)next->GetStatus() : 0,
		next ? next->GetSegmentOffset() : 0,
		next ? next->GetSegmentSize() : 0);
}

inline bool DupeArticleFallback::SegmentAligned(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	int64 begin = articleInfo->GetSegmentOffset();
	int size = articleInfo->GetSegmentSize();
	if (size <= 0)
	{
		return true;
	}
	return nzbget_rs_dupe_fallback_segment_aligned(
		begin, size,
		ExpectedSegmentOffset(fileInfo, articleInfo),
		ExpectedSegmentEnd(fileInfo, articleInfo)) != 0;
}

inline ArticleInfo* DupeArticleFallback::FirstUntiledArticle(FileInfo* fileInfo)
{
	// DecodedFileSize is only set from a decoded yEnc article; when it is 0 the
	// file is non-yEnc (e.g. uuencode, whose articles all record offset 0) or
	// its geometry is not yet known - either way the tiling cannot be judged.
	// After a restart it is repopulated from the first yEnc article that
	// completes, so it is available again by the time a reloaded file finishes.
	int64 decodedSize = fileInfo->GetDecodedFileSize();
	if (decodedSize <= 0)
	{
		return nullptr;
	}

	ArticleList* articles = fileInfo->GetArticles();
	std::vector<NzbgetRsArticleGeom> geom(articles->size());
	for (size_t i = 0; i < articles->size(); i++)
	{
		ArticleInfo* a = (*articles)[i].get();
		geom[i].part_number = a->GetPartNumber();
		geom[i].status = (int32_t)a->GetStatus();
		geom[i].segment_offset = a->GetSegmentOffset();
		geom[i].segment_size = a->GetSegmentSize();
		geom[i].fallback_round = a->GetDupeFallbackRound();
	}
	ptrdiff_t idx = nzbget_rs_dupe_fallback_first_untiled(
		geom.empty() ? nullptr : geom.data(), geom.size(), decodedSize);
	return (idx >= 0 && (size_t)idx < articles->size()) ? (*articles)[(size_t)idx].get() : nullptr;
}

inline bool DupeArticleFallback::HasDonorArticles(FileInfo* fileInfo)
{
	for (ArticleInfo* article : fileInfo->GetArticles())
	{
		if (article->GetDupeFallbackRound() > 0)
		{
			return true;
		}
	}
	return false;
}

inline bool DupeArticleFallback::MergeDecodedFileSize(FileInfo* fileInfo, int64 articleFileSize)
{
	int64 decodedFileSize = fileInfo->GetDecodedFileSize();
	bool conflict = nzbget_rs_dupe_fallback_merge_decoded_size(&decodedFileSize, articleFileSize) != 0;
	fileInfo->SetDecodedFileSize(decodedFileSize);
	return conflict;
}

inline FileInfo::EFirstContent DupeArticleFallback::FirstContent(FileInfo* fileInfo)
{
	if (fileInfo->GetFirstContent() != FileInfo::fcUnknown)
	{
		return fileInfo->GetFirstContent();
	}

	// after a restart: the first article was written already, its bytes tell
	ArticleList* articles = fileInfo->GetArticles();
	if (articles->empty() ||
		articles->front()->GetPartNumber() != 1 ||
		articles->front()->GetStatus() != ArticleInfo::aiFinished ||
		fileInfo->GetOutputFilename().empty())
	{
		return FileInfo::fcUnknown;
	}
	uint8 head[8] = {0};
	if (!nzbget_rs_fs_read_first_8_bytes(fileInfo->GetOutputFilename().c_str(), head))
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	int headKind = nzbget_rs_dupe_fallback_classify_first_head(head, sizeof(head));
	if (headKind == 0)
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	fileInfo->SetFirstContent(headKind == 1 ? FileInfo::fcPar2 : FileInfo::fcOther);
	return fileInfo->GetFirstContent();
}

#endif
