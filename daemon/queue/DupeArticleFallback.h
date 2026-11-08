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
#include <map>
#include <set>
#include "ParDamage.h"
#include "TwinCheck.h"
#include "DupeCoordinator.h"
#include "Options.h"
#include "DiskState.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include <fstream>
#include "nzbget_rs.h"

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
	char head[8] = {0};
	std::ifstream in(fs::u8path(fileInfo->GetOutputFilename()), std::ios::binary);
	if (!in.read(head, sizeof(head)))
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	int headKind = nzbget_rs_dupe_fallback_classify_first_head(
		reinterpret_cast<const uint8*>(head), sizeof(head));
	if (headKind == 0)
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	fileInfo->SetFirstContent(headKind == 1 ? FileInfo::fcPar2 : FileInfo::fcOther);
	return fileInfo->GetFirstContent();
}

#endif
