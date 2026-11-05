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
#include <map>
#include <set>
#include "DupeArticleFallback.h"
#include "ParDamage.h"
#include "TwinCheck.h"
#include "DupeCoordinator.h"
#include "NzbFile.h"
#include "Options.h"
#include "DiskState.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include <fstream>
#include "nzbget_rs.h"

bool DupeArticleFallback::IsParFile(FileInfo* fileInfo)
{
	return nzbget_rs_cdn_dupe_fallback_is_par_file(
		fileInfo->GetParFile() ? 1 : 0,
		(int)fileInfo->GetFirstContent(),
		(int)FileInfo::fcPar2,
		fileInfo->GetFilenameConfirmed() ? 1 : 0,
		fileInfo->GetFilename()) != 0;
}

bool DupeArticleFallback::HasPar2(NzbInfo* nzbInfo)
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
		if (nzbget_rs_dcf_dupe_fallback_completed_file_is_par2(
			completedFile.GetParFile() ? 1 : 0, completedFile.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}

bool DupeArticleFallback::ShouldDeferToPar(NzbInfo* nzbInfo)
{
	int headerVerdict = nzbget_rs_pnd_dupe_fallback_should_defer_to_par_header(
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
		if (nzbget_rs_dcf_dupe_fallback_completed_file_is_par2(
			fileInfo.GetParFile() ? 1 : 0, fileInfo.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}

bool DupeArticleFallback::ParCannotCover(NzbInfo* nzbInfo, bool withMargin)
{
	return nzbget_rs_dupe_fallback_par_cannot_cover(
		nzbInfo->GetParSize(), nzbInfo->GetParCurrentFailedSize(),
		nzbInfo->GetCurrentFailedSize(), nzbInfo->GetCurrentSuccessSize(),
		nzbInfo->GetParCurrentSuccessSize(), nzbInfo->GetSize(),
		nzbInfo->GetCurrentSuccessArticles(), nzbInfo->GetCurrentFailedArticles(),
		withMargin ? 1 : 0) != 0;
}

bool DupeArticleFallback::TryFallback(DownloadQueue* downloadQueue, FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	if (nzbget_rs_dqh_dupe_fallback_should_skip_try_entry(
		(int)g_Options->GetDupeArticleFallback(),
		(int)Options::dafNone,
		g_Options->GetRawArticle() ? 1 : 0,
		IsParFile(fileInfo) ? 1 : 0) != 0)
	{
		return false;
	}

	// a par2-file with a random name looks like any data file: what its first
	// article holds tells (that article itself may be borrowed: it's analyzed).
	// Until that's known only a name without an extension waits: one with an
	// extension of its own isn't a par2-file posing as data, and making every
	// file wait for its first article lost the articles failing before it
	FileInfo::EFirstContent content = FirstContent(fileInfo);
	if (nzbget_rs_cdn_dupe_fallback_should_skip_first_content(
		(int)content, (int)FileInfo::fcPar2, (int)FileInfo::fcUnknown,
		articleInfo->GetPartNumber(),
		HasExtension(fileInfo->GetFilename()) ? 1 : 0) != 0)
	{
		return false;
	}

	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();
	if (nzbget_rs_cdn_dupe_fallback_is_nzb_or_file_ineligible(
		fileInfo->GetDeleted() ? 1 : 0,
		nzbInfo->GetDeleting() ? 1 : 0,
		nzbInfo->GetParking() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce) != 0)
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
		bool parDefer = nzbget_rs_dqh_dupe_fallback_can_defer_to_par(
			(int)g_Options->GetParCheck(),
			(int)Options::pcManual,
			g_Options->GetParCheck() != Options::pcManual && ShouldDeferToPar(nzbInfo) ? 1 : 0) != 0;
		ParDamage::Verdict damage = parDefer ? ParDamage::Judge(nzbInfo) : ParDamage::Verdict();
		// par2 repairs whole blocks: the blocks the failed articles spoiled may
		// be more than its volumes hold long before the bytes are (ParDamage);
		// borrowing then starts, and the par-damage failover waits for it
		int liftDeferState = 0;
		int logLifted = 0;
		defer = nzbget_rs_cdn_dupe_fallback_eval_par_defer(
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
	if (nzbget_rs_dcf_dupe_fallback_should_abort_deferred_try(
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
	while (nzbget_rs_dqh_dupe_fallback_should_skip_blocked_donor_round(
		round,
		sources.size(),
		sourceDonors.size(),
		round >= 0 && (size_t)round < sourceDonors.size() &&
			nzbInfo->GetDupeBlockedDonors()->count(sourceDonors[round]) ? 1 : 0) != 0)
	{
		round++;
	}
	if (round >= (int)sources.size())
	{
		if (defer)
		{
			// say once per collection which way the par-first rule went, so a
			// download that borrowed nothing can be told apart from one that tried
			if (nzbget_rs_dqh_dupe_fallback_should_log_par_defer(
				articleInfo->GetDupeProactive() ? 1 : 0,
				(int)nzbInfo->GetDupeParDeferState(),
				(int)NzbInfo::dpNone,
				downloadQueue != nullptr ? 1 : 0) != 0 &&
				!CollectDonors(downloadQueue, nzbInfo).empty())
			{
				nzbInfo->SetDupeParDeferState(NzbInfo::dpDeferred);
				nzbInfo->PrintMessage(Message::mkInfo,
					"Deferring duplicate recovery for %s to par-check (its par2 files may cover the damage)",
					nzbInfo->GetName());
			}
			if (nzbget_rs_dqh_dupe_fallback_should_restore_original_msg_id(
				pinnedRound,
				Util::EmptyStr(articleInfo->GetDupeOriginalMessageId()) ? 1 : 0) != 0)
			{
				articleInfo->SetMessageId(articleInfo->GetDupeOriginalMessageId());
			}
			return false;
		}
		if (nzbget_rs_dcf_dupe_fallback_is_reactive_first_round(
			pinnedRound, articleInfo->GetDupeProactive() ? 1 : 0) != 0)
		{
			// no duplicate carries this article's file at all (an article asked of a
			// duplicate first, after the cutover, hasn't failed: counted, every fresh
			// article of a file whose lead was barred added one, and the download
			// failed over early)
			nzbInfo->SetDupeUnsourcedArticles(nzbInfo->GetDupeUnsourcedArticles() + 1);
		}
		else if (!Util::EmptyStr(articleInfo->GetDupeOriginalMessageId()))
		{
			// every source was tried: the article is the release's own again
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
	if (nzbget_rs_dcf_dupe_fallback_is_reactive_first_round(
		pinnedRound, articleInfo->GetDupeProactive() ? 1 : 0) != 0)
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

std::vector<CString> DupeArticleFallback::OrderSources(const std::vector<CString>& donorCandidates,
	bool cutover, const char* primaryMessageId)
{
	std::vector<CString> sources;
	bool appendPrimary = nzbget_rs_dqh_dupe_fallback_should_append_primary_on_order(
		cutover ? 1 : 0, donorCandidates.size()) != 0;
	sources.reserve(donorCandidates.size() + (appendPrimary ? 1 : 0));
	for (const CString& candidate : donorCandidates)
	{
		sources.emplace_back((const char*)candidate);
	}
	if (appendPrimary)
	{
		// cut over: lead with the top donor, then the remaining donors, then the
		// primary as the final fallback. The file cut over precisely because the
		// primary is missing many articles, so on a lead-donor miss the other
		// duplicates are better bets than paying a (likely failing) full server
		// sweep on the primary; the primary still backstops parts every duplicate
		// misses. NOTE: proactive fetches must never count as "recovered"
		// regardless of round (see QueueCoordinator::ArticleCompleted) - with the
		// primary last, no donor round proves the primary was missing the part
		sources.emplace_back(primaryMessageId);
	}
	return sources;
}

void DupeArticleFallback::RotateToLead(RawNzbList& donors, int leadNzbId)
{
	std::vector<int32_t> donorIds;
	donorIds.reserve(donors.size());
	for (NzbInfo* donor : donors)
	{
		donorIds.push_back(donor->GetId());
	}
	ptrdiff_t idx = nzbget_rs_dqh_dupe_fallback_find_lead_index(
		donorIds.data(), donorIds.size(), leadNzbId);
	if (nzbget_rs_dqh_dupe_fallback_should_rotate_donors(idx, donors.size()) != 0)
	{
		std::rotate(donors.begin(), donors.begin() + idx, donors.end());
	}
}

std::vector<ArticleInfo*> DupeArticleFallback::BorrowedPar2Mismatches(FileInfo* fileInfo, const char* path,
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

void DupeArticleFallback::FinishPin(FileInfo* fileInfo, ArticleInfo* articleInfo,
	const std::vector<CString>& candidates, const std::vector<int>& contributors,
	bool cutover, const char* primaryMessageId)
{
	int leadSnapshot = 0;
	int nextLead = 0;
	int donorCount = 0;
	int newFileLead = fileInfo->GetDupeLeadDonorId();
	nzbget_rs_pnd_dupe_fallback_compute_pin_lead_state(
		contributors.empty() ? nullptr : contributors.data(),
		contributors.size(),
		fileInfo->GetDupeLeadDonorId(),
		&leadSnapshot, &nextLead, &donorCount, &newFileLead);
	articleInfo->SetDupeLeadSnapshot(leadSnapshot);
	articleInfo->SetDupeNextLead(nextLead);
	articleInfo->SetDupeDonorCount(donorCount);
	fileInfo->SetDupeLeadDonorId(newFileLead);

	*articleInfo->GetDupeSources() = OrderSources(candidates, cutover, primaryMessageId);
	// OrderSources keeps the candidates' order (the primary goes last)
	*articleInfo->GetDupeSourceDonors() = contributors;
}

bool DupeArticleFallback::RegisterLeadFailure(FileInfo* fileInfo, ArticleInfo* articleInfo)
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

void DupeArticleFallback::RegisterLeadSuccess(FileInfo* fileInfo, ArticleInfo* articleInfo)
{
	if (!nzbget_rs_dqh_dupe_fallback_can_register_lead_success(
		articleInfo->GetDupeFallbackRound(),
		articleInfo->GetDupeLeadSnapshot(),
		fileInfo->GetDupeLeadDonorId()))
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

void DupeArticleFallback::VacateGhostLead(FileInfo* fileInfo, const RawNzbList& donors)
{
	// a lead gone from queue and history, or barred after its articles failed the
	// par2 check (B40), is vacated: a barred one was passed over when sources were
	// pinned, so no article's lead matched it again and the lead rotation froze
	int leadNzbId = fileInfo->GetDupeLeadDonorId();
	bool barred = fileInfo->GetNzbInfo() && fileInfo->GetNzbInfo()->GetDupeBlockedDonors()->count(leadNzbId);
	bool inDonors = (leadNzbId != 0 && !barred) &&
		std::find_if(donors.begin(), donors.end(),
			[leadNzbId](NzbInfo* donor) { return donor->GetId() == leadNzbId; }) != donors.end();
	if (nzbget_rs_pnd_dupe_fallback_should_vacate_ghost_lead(
		leadNzbId, barred ? 1 : 0, inDonors ? 1 : 0) == 0)
	{
		return;
	}

	fileInfo->SetDupeLeadDonorId(0);
	fileInfo->SetDupeLeadFailures(0);
	fileInfo->SetDupeLeadSwitches(0);
}

CString DupeArticleFallback::NzbFilenameOf(FileInfo* fileInfo)
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
			const char* messageId = nzbget_rs_dqh_diskstate_select_original_or_current_msg_id(
				article->GetDupeOriginalMessageId(), article->GetMessageId());
			ArticleInfo* ownArticle = (*ownArticles)[i].get();
			if (nzbget_rs_dcf_dupe_fallback_own_article_matches(
				ownArticle->GetPartNumber(), article->GetPartNumber(),
				ownArticle->GetMessageId(), messageId) != 0)
			{
				return CString(ownFile->GetFilename());
			}
		}
	}
	return CString();
}

RawNzbList DupeArticleFallback::CollectDonors(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	RawNzbList donors;

	for (NzbInfo* queuedNzbInfo : downloadQueue->GetQueue())
	{
		if (nzbget_rs_dqh_dupe_fallback_is_queued_donor_eligible(
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
			nzbget_rs_dqh_dupe_fallback_is_history_donor_eligible(
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
			if (nzbget_rs_dqh_dupe_fallback_is_same_full_content_hash(
				nzbInfo->GetFullContentHash(),
				donor->GetFullContentHash()) != 0)
			{
				continue;
			}
			std::string donorIdentity = TwinCheck::Identity(donor);
			std::string kind = !donorIdentity.empty() ?
				TwinCheck::Kind(nzbInfo->GetId(), ownIdentity.c_str(), donor->GetId(), donorIdentity.c_str()) : "";
			if (nzbget_rs_dqh_dupe_fallback_is_twin_kind(
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
			return nzbget_rs_pnd_dupe_fallback_compare_donors(
				twin1 ? 1 : 0, donor1->GetDupeScore(), donor1->GetId(),
				twin2 ? 1 : 0, donor2->GetDupeScore(), donor2->GetId()) != 0;
		});

	return donors;
}

void DupeArticleFallback::PinSources(DownloadQueue* downloadQueue, FileInfo* fileInfo,
	ArticleInfo* articleInfo, bool twinOnly)
{
	NzbInfo* nzbInfo = fileInfo->GetNzbInfo();

	RawNzbList donors = CollectDonors(downloadQueue, nzbInfo);
	VacateGhostLead(fileInfo, donors);
	if (nzbget_rs_dqh_dupe_fallback_needs_twin_lead_check(
		twinOnly ? 1 : 0, fileInfo->GetDupeLeadDonorId()) != 0)
	{
		std::string ownIdentity = TwinCheck::Identity(nzbInfo);
		int leadId = fileInfo->GetDupeLeadDonorId();
		auto it = std::find_if(donors.begin(), donors.end(),
			[leadId](NzbInfo* donor) { return donor->GetId() == leadId; });
		std::string leadIdentity = it != donors.end() ? TwinCheck::Identity(*it) : "";
		std::string leadKind = (!ownIdentity.empty() && !leadIdentity.empty()) ?
			TwinCheck::Kind(nzbInfo->GetId(), ownIdentity.c_str(), leadId, leadIdentity.c_str()) : "";
		if (nzbget_rs_dqh_dupe_fallback_should_reset_non_twin_lead(
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
		if (nzbget_rs_dqh_dupe_fallback_is_same_full_content_hash(
			nzbInfo->GetFullContentHash(),
			donorNzbInfo->GetFullContentHash()) != 0)
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

	if (nzbget_rs_dqh_dupe_fallback_should_log_no_source(
		twinOnly ? 1 : 0,
		candidates.empty() ? 1 : 0,
		donors.empty() ? 1 : 0,
		fileInfo->GetDupeNoSourceLogged() ? 1 : 0) != 0)
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

void DupeArticleFallback::AppendDonorCandidate(std::vector<CString>& candidates,
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
		[messageId](const CString& candidate) { return nzbget_rs_dqh_bytes_equal(candidate, messageId) != 0; }) != candidates.end();
	if (!duplicate)
	{
		candidates.emplace_back(messageId);
		contributors.push_back(donorNzbId);
	}
}

/*
 * Pure candidate builder over already-live donors, used by unit tests. The
	 * production path (PinSources) processes donors one at a time for
	 * cache-safety; both share AppendDonorCandidate so the logic stays in sync.
	 *
 * The candidate list must not depend on the current state of the article:
 * it is frozen on first fallback, so later donor insertion, removal, or lead
 * rotation cannot shift the round-to-source mapping.
 */
std::vector<CString> DupeArticleFallback::BuildCandidateMessageIds(
	const std::vector<NzbInfo*>& parsedDonors, FileInfo* targetFile, int partNumber)
{
	std::vector<CString> candidates;
	std::vector<int> contributors;

	for (NzbInfo* parsedDonor : parsedDonors)
	{
		AppendDonorCandidate(candidates, contributors, parsedDonor->GetId(),
			parsedDonor, targetFile, partNumber);
	}

	return candidates;
}

std::unique_ptr<NzbInfo> DupeArticleFallback::ParseDonorNzb(const char* queuedFilename)
{
	if (nzbget_rs_dqh_dupe_fallback_can_parse_donor_nzb(
		Util::EmptyStr(queuedFilename) ? 1 : 0,
		!Util::EmptyStr(queuedFilename) && FileSystem::FileExists(queuedFilename) ? 1 : 0) == 0)
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

NzbInfo* DupeArticleFallback::GetParsedDonor(NzbInfo* donorNzbInfo)
{
	int donorId = donorNzbInfo->GetId();

	// a duplicate whose nzb-file couldn't be read is asked again after a while: the
	// failure may have been passing (review item 9)
	auto bad = m_badDonors.find(donorId);
	if (bad != m_badDonors.end() && nzbget_rs_dcf_dupe_fallback_is_bad_donor_in_cooldown(
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

	if (nzbget_rs_qcf_dupe_fallback_should_evict_parsed_donor_cache(
		m_parsedDonors.size(), MaxCachedDonors) != 0)
	{
		m_parsedDonors.erase(m_parsedDonors.begin());
	}

	// store the parsed collection in the cache and return a pointer to the
	// owned object (the cache keeps it alive for the daemon's lifetime)
	std::unique_ptr<NzbInfo>& cachedDonor = m_parsedDonors[donorId];
	cachedDonor = std::move(parsedNzb);

	return cachedDonor.get();
}

FileInfo* DupeArticleFallback::MatchDonorFile(FileInfo* targetFile, NzbInfo* donorNzb,
	const char* targetNzbFilename, int donorNzbId, const char* donorPrint, bool twinOnly)
{
	if (IsParFile(targetFile))
	{
		return nullptr;
	}

	// by content first, when the par2 file lists of both postings are known
	// (twin.rs): a donor file with the target's MD5 and length is the same bytes
	// under any name. "-": the donor's complete list has no file with these bytes;
	// "": not known (incomplete or missing lists, several candidates)
	NzbInfo* own = targetFile->GetNzbInfo();
	std::string ownIdentity = own ? TwinCheck::Identity(own) : "";
	if (twinOnly)
	{
		std::string kind = (donorNzbId && !ownIdentity.empty() && !Util::EmptyStr(donorPrint)) ?
			TwinCheck::Kind(own->GetId(), ownIdentity.c_str(), donorNzbId, donorPrint) : "";
		if (!donorNzbId || !nzbget_rs_dqh_dupe_fallback_is_twin_kind(
			ownIdentity.c_str(), donorPrint, kind.c_str()))
		{
			return nullptr;
		}
	}
	std::string byContent = nzbget_rs_dqh_dupe_fallback_can_query_content_match(
		donorNzbId,
		ownIdentity.empty() ? 1 : 0,
		Util::EmptyStr(donorPrint) ? 1 : 0) != 0 ?
		TwinCheck::MatchByContent(own->GetId(), ownIdentity.c_str(), targetFile->GetFilename(),
			targetNzbFilename, donorNzbId, donorPrint) : "";
	if (nzbget_rs_dqh_dupe_fallback_is_content_match_rejected(
		byContent.c_str(),
		twinOnly ? 1 : 0) != 0)
	{
		return nullptr;
	}
	if (!byContent.empty())
	{
		// the donor's file of that name (exact case first): it holds our bytes, so
		// if its articles don't fit ours, no other file of the donor is the one.
		// A name the donor's nzb doesn't have (it names the file otherwise), or
		// has twice, leaves it to the matching below
		std::vector<FileInfo*> exact;
		std::vector<FileInfo*> folded;
		for (FileInfo* donorFile : donorNzb->GetFileList())
		{
			if (IsParFile(donorFile))
			{
				continue;
			}
			int nameMatchKind = nzbget_rs_dqh_dupe_fallback_classify_content_name_match(
				donorFile->GetFilename(), byContent.c_str());
			if (nameMatchKind == 1)
			{
				exact.push_back(donorFile);
			}
			else if (nameMatchKind == 2)
			{
				folded.push_back(donorFile);
			}
		}
		int matchKind = nzbget_rs_dcf_dupe_fallback_select_named_content_match(
			exact.size(), folded.size());
		FileInfo* named = matchKind == 1 ? exact[0] : matchKind == 2 ? folded[0] : nullptr;
		if (named || twinOnly)
		{
			return named && StructureMatches(targetFile, named) ? named : nullptr;
		}
	}

	std::vector<FileInfo*> structuralMatches;
	FileInfo* filenameMatch = nullptr;
	bool filenameAmbiguous = false;

	for (FileInfo* donorFile : donorNzb->GetFileList())
	{
		bool isPar = IsParFile(donorFile);
		if (nzbget_rs_dqh_dupe_fallback_is_donor_file_struct_candidate(
			isPar ? 1 : 0,
			!isPar && StructureMatches(targetFile, donorFile) ? 1 : 0) == 0)
		{
			continue;
		}

		if (nzbget_rs_dqh_dupe_fallback_is_donor_filename_match(
			targetFile->GetFilename(),
			targetNzbFilename,
			donorFile->GetFilename()) != 0)
		{
			if (filenameMatch)
			{
				filenameAmbiguous = true;
			}
			else
			{
				filenameMatch = donorFile;
			}
			continue;
		}

		structuralMatches.push_back(donorFile);
	}

	// An exact filename is preferred only when it is unique.  Choosing the
	// first exact match is nondeterministic for multi-file duplicate NZBs.
	int structOrNameAction = nzbget_rs_qcf_dupe_fallback_resolve_structural_or_filename_match(
		filenameAmbiguous ? 1 : 0,
		filenameMatch != nullptr ? 1 : 0,
		structuralMatches.size());
	if (structOrNameAction == -1)
	{
		return nullptr;
	}
	if (structOrNameAction == 1)
	{
		return filenameMatch;
	}
	if (structOrNameAction == 2)
	{
		return structuralMatches[0];
	}

	// Equal-size volumes of an obfuscated repost all match structurally;
	// the identical member is the only one whose article sizes step exactly
	// like the target's.
	FileInfo* stepMatch = nullptr;
	for (FileInfo* donorFile : structuralMatches)
	{
		if (ArticleSizeStepsMatch(targetFile, donorFile))
		{
			if (stepMatch)
			{
				return nullptr;
			}
			stepMatch = donorFile;
		}
	}
	return stepMatch;
}

uint64 DupeArticleFallback::ArticleSizeStepsHash(FileInfo* fileInfo)
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

bool DupeArticleFallback::ArticleSizeStepsMatch(FileInfo* targetFile, FileInfo* donorFile)
{
	// An NZB lists the encoded size of each article including its headers.
	// Two postings of the same bytes with the same segmentation differ there
	// only by a per-file constant (their subject lines differ), so the steps
	// between consecutive article sizes are equal. Encoded sizes vary with
	// the content (yEnc escaping), which makes the step sequence a strong
	// identity fingerprint once a file has enough articles.
	ArticleList* targetArticles = targetFile->GetArticles();
	ArticleList* donorArticles = donorFile->GetArticles();
	if ((int)targetArticles->size() < MinStepFingerprintArticles ||
		targetArticles->size() != donorArticles->size())
	{
		return false;
	}

	std::vector<int> targetSizes(targetArticles->size());
	std::vector<int> donorSizes(donorArticles->size());
	for (size_t i = 0; i < targetArticles->size(); i++)
	{
		targetSizes[i] = (*targetArticles)[i]->GetSize();
		donorSizes[i] = (*donorArticles)[i]->GetSize();
	}
	return nzbget_rs_dupe_fallback_steps_match(
		targetSizes.data(), targetSizes.size(),
		donorSizes.data(), donorSizes.size()) != 0;
}


bool DupeArticleFallback::StructureMatches(FileInfo* targetFile, FileInfo* donorFile)
{
	ArticleList* targetArticles = targetFile->GetArticles();
	ArticleList* donorArticles = donorFile->GetArticles();

	if (targetArticles->empty())
	{
		return false;
	}

	std::vector<int> targetParts(targetArticles->size());
	std::vector<int> targetSizes(targetArticles->size());
	for (size_t i = 0; i < targetArticles->size(); i++)
	{
		targetParts[i] = (*targetArticles)[i]->GetPartNumber();
		targetSizes[i] = (*targetArticles)[i]->GetSize();
	}
	std::vector<int> donorParts(donorArticles->size());
	std::vector<int> donorSizes(donorArticles->size());
	for (size_t i = 0; i < donorArticles->size(); i++)
	{
		donorParts[i] = (*donorArticles)[i]->GetPartNumber();
		donorSizes[i] = (*donorArticles)[i]->GetSize();
	}
	return nzbget_rs_dupe_fallback_structure_matches(
		targetFile->GetSubject(), targetFile->GetTotalArticles(), targetFile->GetSize(),
		targetParts.data(), targetSizes.data(), targetParts.size(),
		donorFile->GetSubject(), donorFile->GetTotalArticles(), donorFile->GetSize(),
		donorParts.empty() ? nullptr : donorParts.data(),
		donorSizes.empty() ? nullptr : donorSizes.data(),
		donorParts.size()) != 0;
}

const char* DupeArticleFallback::FindDonorMessageId(FileInfo* donorFile, int partNumber)
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

bool DupeArticleFallback::SizesMatch(int64 size1, int64 size2, int div)
{
	return nzbget_rs_dupe_fallback_sizes_match(size1, size2, div) != 0;
}

static int FindArticleIndex(ArticleList* articles, ArticleInfo* articleInfo)
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

int64 DupeArticleFallback::ExpectedSegmentOffset(FileInfo* fileInfo, ArticleInfo* articleInfo)
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

int64 DupeArticleFallback::ExpectedSegmentEnd(FileInfo* fileInfo, ArticleInfo* articleInfo)
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

bool DupeArticleFallback::SegmentAligned(FileInfo* fileInfo, ArticleInfo* articleInfo)
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

ArticleInfo* DupeArticleFallback::FirstUntiledArticle(FileInfo* fileInfo)
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

bool DupeArticleFallback::HasDonorArticles(FileInfo* fileInfo)
{
	ArticleList* articles = fileInfo->GetArticles();
	std::vector<int32_t> rounds(articles->size());
	for (size_t i = 0; i < articles->size(); i++)
	{
		rounds[i] = (*articles)[i]->GetDupeFallbackRound();
	}
	return nzbget_rs_dqh_dupe_fallback_has_donor_articles(rounds.data(), rounds.size()) != 0;
}

bool DupeArticleFallback::MergeDecodedFileSize(FileInfo* fileInfo, int64 articleFileSize)
{
	int64 decodedFileSize = fileInfo->GetDecodedFileSize();
	bool conflict = nzbget_rs_dupe_fallback_merge_decoded_size(&decodedFileSize, articleFileSize) != 0;
	fileInfo->SetDecodedFileSize(decodedFileSize);
	return conflict;
}

std::vector<DupeArticleFallback::Par2Desc> DupeArticleFallback::ListPar2Files(const char* dir)
{
	std::vector<Par2Desc> list;
	nzbget_rs_dupe_fallback_list_par2_files(
		dir, &list,
		[](void* ctx, const char* name, size_t nameLen, uint64_t length, uint64_t blockSize)
		{
			auto* out = static_cast<std::vector<Par2Desc>*>(ctx);
			Par2Desc desc;
			desc.name.assign(name, nameLen);
			desc.length = length;
			desc.blockSize = blockSize;
			out->push_back(std::move(desc));
		});
	return list;
}

FileInfo::EFirstContent DupeArticleFallback::FirstContent(FileInfo* fileInfo)
{
	if (fileInfo->GetFirstContent() != FileInfo::fcUnknown)
	{
		return fileInfo->GetFirstContent();
	}

	// after a restart: the first article was written already, its bytes tell
	ArticleList* articles = fileInfo->GetArticles();
	if (!nzbget_rs_dqh_dupe_fallback_can_read_first_content_from_disk(
		!articles->empty() ? 1 : 0,
		!articles->empty() ? articles->front()->GetPartNumber() : 0,
		!articles->empty() ? (int)articles->front()->GetStatus() : 0,
		(int)ArticleInfo::aiFinished,
		fileInfo->GetOutputFilename().c_str()))
	{
		return FileInfo::fcUnknown;
	}
	char head[8] = {0};
	std::ifstream in(fs::u8path(fileInfo->GetOutputFilename()), std::ios::binary);
	if (!in.read(head, sizeof(head)))
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	int headKind = nzbget_rs_cdn_dupe_fallback_classify_first_head(
		reinterpret_cast<const uint8*>(head), sizeof(head));
	if (headKind == 0)
	{
		return FileInfo::fcUnknown;	// not on disk yet (the article cache)
	}
	fileInfo->SetFirstContent(headKind == 1 ? FileInfo::fcPar2 : FileInfo::fcOther);
	return fileInfo->GetFirstContent();
}

bool DupeArticleFallback::HasExtension(const char* filename)
{
	return nzbget_rs_dupe_fallback_has_extension(filename) != 0;
}
