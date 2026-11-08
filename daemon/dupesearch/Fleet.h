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


#ifndef FLEET_H
#define FLEET_H

#include <string>
#include <vector>

/*
 * A fleet: several nzb-files a client believes are copies of one release,
 * sent at once (JSON-RPC method "appendfleet"). Nothing downloads before
 * their postings were checked on the configured news servers: the wholest
 * is queued, the others are kept in history as backups, already scored in
 * the order failover should try them. Same postings are checked once. One
 * fleet of a key at a time: a second waits for the first. With a download of
 * the key running, a fleet only adds backups, below it; a posting the key
 * holds already isn't added again. A member is dead below option DupeMinAlive.
 */
class Fleet
{
public:
	struct Member
	{
		std::string name;		// the nzb-file's name
		std::string data;		// its content (fetched first when url is set)
		std::string url;
	};
	struct Request
	{
		std::string dupeKey;
		std::string category;
		int priority = 0;
		int timeoutSec = DefaultTimeoutSec;
		std::vector<Member> members;
	};
	struct Entry
	{
		int nzbId = 0;
		std::string name;
		int rank = 0;
		int alive = -1;			// percent of the sampled articles found, -1 unknown
		std::string status;		// QUEUED, BACKUP, SAME_POSTING, DEAD, ERROR, SKIPPED, COPY
		int sameAs = 0;			// SAME_POSTING: the NZBID of the item it is a copy of (0: not added)
		int sameAsRank = 0;		// SAME_POSTING: the rank of the fleet member it is a copy of
		int member = -1;		// its index in the request
		std::string reason;
	};
	struct Result
	{
		int chosen = 0;			// the download now queued, 0 for none
		bool complete = true;	// every posting was checked within the time limit
		std::string reason;		// ALREADY_QUEUED (chosen: the key's running download); when chosen is 0:
							// ALREADY_DOWNLOADED, ALL_DEAD, NO_USABLE_MEMBERS, NO_MEMBERS, SHUTDOWN, NOT_QUEUED,
							// KEY_BUSY (another fleet of the key held it past the time limit),
							// INCOMPLETE (none found alive, but the time limit cut the check short)
		std::vector<Entry> members;	// in rank order
	};

	static constexpr int DefaultTimeoutSec = 45;
	static constexpr int MaxTimeoutSec = 120;
	static constexpr int MaxMembers = 50;
	// a parameter of every item a fleet adds: its rank in the fleet. A queue extension
	// (NZBPR_DupeFleet) can tell that nzbget ranked the key's copies already: ranking
	// them again tied two live backups at one score
	static constexpr const char* FleetParam = "DupeFleet";

	/* checks, ranks and adds the members; blocks for up to request.timeoutSec */
	static Result Append(Request request);
};

#include <algorithm>
#include <map>
#include <chrono>
#include <mutex>
#include <thread>
#include "DonorHealth.h"
#include "DupeCoordinator.h"
#include "DupeSearch.h"
#include "Scanner.h"
#include "DownloadInfo.h"
#include "Options.h"
#include "Log.h"
#include "WorkState.h"
#include "nzbget_rs.h"

namespace FleetDetail
{
	inline constexpr size_t MaxNzbBytes = 64 * 1024 * 1024;
	inline constexpr int MaxParallel = 16;
	inline constexpr int FleetSampleMax = 200;
	inline constexpr long long FinishReserveMs = 2000;

	struct Candidate
	{
		size_t index;
		NzbSummary summary;
		DonorHealth::Health health;
		bool checked = false;
		int alive = -1;
		bool dead = false;
		int sameAs = -1;
		int existingId = 0;
		std::string error;
	};

	inline std::timed_mutex g_checkMutex;
	inline std::mutex g_keyLocksMutex;
	inline std::map<std::string, std::shared_ptr<std::timed_mutex>> g_keyLocks;

	inline std::shared_ptr<std::timed_mutex> KeyLock(const std::string& key)
	{
		std::lock_guard<std::mutex> guard(g_keyLocksMutex);
		std::shared_ptr<std::timed_mutex>& lock = g_keyLocks[DupeUtil::Lower(key)];
		if (!lock)
		{
			lock = std::make_shared<std::timed_mutex>();
		}
		return lock;
	}
}

inline Fleet::Result Fleet::Append(Request request)
{
	using namespace FleetDetail;
	Result result;
	request.timeoutSec = nzbget_rs_dqh_fleet_clamp_timeout_sec(
		request.timeoutSec, (int)MaxTimeoutSec);
	if (request.members.empty())
	{
		result.reason = "NO_MEMBERS";
		return result;
	}
	if (request.dupeKey.empty())
	{
		request.dupeKey = DupeSearch::MakeDupeKey(request.members[0].name);
	}
	long long deadlineMs = DonorHealth::NowMs() + request.timeoutSec * 1000LL;
	std::shared_ptr<std::timed_mutex> keyLock = KeyLock(request.dupeKey);
	std::unique_lock<std::timed_mutex> keyGuard(*keyLock, std::defer_lock);
	if (!keyGuard.try_lock_for(std::chrono::milliseconds(std::max(0LL,
		deadlineMs - DonorHealth::NowMs() - FinishReserveMs))))
	{
		result.reason = "KEY_BUSY";
		result.complete = false;
		return result;
	}

	std::vector<HttpGet::Reply> fetched(request.members.size());
	{
		std::vector<std::thread> fetchers;
		for (size_t i = 0; i < request.members.size(); i++)
		{
			if (!request.members[i].url.empty())
			{
				fetchers.emplace_back([&, i]()
					{
						fetched[i] = HttpGet::Fetch(request.members[i].url, "fleet member " + request.members[i].name,
							MaxNzbBytes, deadlineMs - FinishReserveMs);
					});
			}
		}
		for (std::thread& fetcher : fetchers)
		{
			fetcher.join();
		}
	}

	std::vector<Candidate> candidates(request.members.size());
	for (size_t i = 0; i < request.members.size(); i++)
	{
		Member& member = request.members[i];
		Candidate& candidate = candidates[i];
		candidate.index = i;
		if (!member.url.empty())
		{
			HttpGet::Reply& reply = fetched[i];
			if (!reply.ok)
			{
				NzbgetRsBuf errBuf = nzbget_rs_dqh_fleet_format_fetch_error(reply.status);
				candidate.error.assign(errBuf.data ? errBuf.data : "", errBuf.len);
				nzbget_rs_free(errBuf);
				continue;
			}
			member.data = std::move(reply.body);
		}
		if (!NzbReader::Parse(member.data, candidate.summary) || candidate.summary.messageIds.empty())
		{
			candidate.error = "the nzb-file is unreadable";
			continue;
		}
		for (size_t j = 0; j < i; j++)
		{
			if (candidates[j].error.empty() && candidates[j].sameAs < 0 &&
				Posting::SamePosting(candidates[j].summary.messageIds, candidate.summary.messageIds))
			{
				candidate.sameAs = (int)j;
				break;
			}
		}
	}

	long long top = DupeSearch::BasePickScore;
	int runningId = 0;
	int runningScore = 0;
	int lowest = 0;
	bool any = false;
	std::vector<std::pair<int, std::string>> existing;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		auto consider = [&](NzbInfo* nzbInfo, bool queued)
			{
				if (nzbInfo->GetKind() != NzbInfo::nkNzb ||
					!nzbget_rs_dqh_bytes_eq_ignore_case(nzbInfo->GetDupeKey(), request.dupeKey.c_str()))
				{
					return;
				}
				int64_t newTop = top;
				nzbget_rs_dqh_fleet_update_key_score_bounds(
					top, lowest, any ? 1 : 0, queued ? 1 : 0,
					nzbInfo->GetDeleting() ? 1 : 0,
					(int)nzbInfo->GetDeleteStatus(),
					(int)NzbInfo::dsNone,
					runningId, runningScore,
					nzbInfo->GetId(), nzbInfo->GetDupeScore(),
					&newTop, &lowest, &runningId, &runningScore);
				top = newTop;
				any = true;
				std::string names = nzbInfo->GetQueuedFilename() ? nzbInfo->GetQueuedFilename() : "";
				struct PipeCtx
				{
					std::vector<std::pair<int, std::string>>& existing;
					int id;
				} pipeCtx{existing, nzbInfo->GetId()};
				nzbget_rs_dqh_fleet_visit_pipe_names(
					names.data(), names.size(), &pipeCtx,
					[](void* ctx, const char* partPtr, size_t partLen)
					{
						auto* c = static_cast<PipeCtx*>(ctx);
						c->existing.emplace_back(c->id, std::string(partPtr, partLen));
					});
			};
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			consider(nzbInfo, true);
		}
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb)
			{
				consider(historyInfo->GetNzbInfo(), false);
			}
		}
	}
	for (const auto& item : existing)
	{
		NzbSummary summary;
		if (!NzbReader::Parse(DupeUtil::ReadAll(item.second), summary))
		{
			continue;
		}
		for (Candidate& candidate : candidates)
		{
			if (candidate.error.empty() && candidate.sameAs < 0 && !candidate.existingId &&
				Posting::SamePosting(summary.messageIds, candidate.summary.messageIds))
			{
				candidate.existingId = item.first;
			}
		}
	}
	{
		std::vector<Candidate*> ranked;
		for (Candidate& candidate : candidates)
		{
			ranked.push_back(&candidate);
		}
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			NzbInfo* item = historyInfo->GetKind() == HistoryInfo::hkNzb ? historyInfo->GetNzbInfo() : nullptr;
			if (!item || nzbget_rs_dqh_fleet_is_already_downloaded_history_item(
				item != nullptr ? 1 : 0,
				(int)item->GetDeleteStatus(),
				(int)NzbInfo::dsNone,
				item->IsDupeSuccess() ? 1 : 0,
				(int)item->GetMarkStatus(),
				(int)NzbInfo::ksBad) == 0 ||
				!ReleaseName::Readable(item->GetName()))
			{
				continue;
			}
			bool same = std::any_of(ranked.begin(), ranked.end(), [&](const Candidate* c)
				{
					const std::string& name = request.members[c->index].name;
					return c->error.empty() && ReleaseName::Readable(name) &&
						ReleaseName::SameRelease(name, item->GetName());
				});
			if (same && DupeCoordinator::FilesOnDisk(item))
			{
				result.reason = "ALREADY_DOWNLOADED";
				result.chosen = 0;
				int rank = 0;
				for (Candidate* candidate : ranked)
				{
					Entry entry;
					entry.name = request.members[candidate->index].name;
					entry.rank = ++rank;
					entry.alive = candidate->alive;
					entry.status = candidate->error.empty() ? "SKIPPED" : "ERROR";
					entry.reason = candidate->error.empty() ? "downloaded as " + std::string(item->GetName()) : candidate->error;
					result.members.push_back(entry);
				}
				info("Fleet for %s: %s is downloaded already", request.dupeKey.c_str(), item->GetName());
				return result;
			}
		}
	}

	std::vector<DonorHealth::Posting> postings;
	for (Candidate& candidate : candidates)
	{
		if (candidate.error.empty() && candidate.sameAs < 0 && !candidate.existingId)
		{
			NzbgetRsBuf idxBuf = nzbget_rs_dqh_conn_format_port_str(candidate.index);
			std::string idxStr(idxBuf.data ? idxBuf.data : "", idxBuf.len);
			nzbget_rs_free(idxBuf);
			postings.push_back({ std::move(idxStr), candidate.summary.messageIds,
				std::make_shared<std::vector<std::string>>(candidate.summary.groups) });
		}
	}
	DonorHealth::ServerList servers = NntpHealthServer::Servers();
	std::unique_lock<std::timed_mutex> checkGuard(g_checkMutex, std::defer_lock);
	bool mayCheck = !servers.empty() && !postings.empty() && checkGuard.try_lock_for(
		std::chrono::milliseconds(nzbget_rs_dqh_fleet_calc_check_lock_wait_ms(
			deadlineMs, DonorHealth::NowMs(), FinishReserveMs)));
	if (!servers.empty() && !postings.empty() && !mayCheck)
	{
		result.complete = false;
	}
	if (mayCheck)
	{
		DonorHealth::Options options;
		options.sample.percent = g_Options->GetDupeHealthPercent();
		options.sample.minimum = g_Options->GetDupeHealthMin();
		options.sample.maxBody = g_Options->GetDupeBodyChecks();
		long long checkStart = DonorHealth::NowMs();
		int64_t checkDeadlineMs = 0;
		nzbget_rs_dqh_fleet_calc_check_budget_and_deadline(
			g_Options->GetDupeHealthMax(), FleetSampleMax,
			deadlineMs, checkStart, FinishReserveMs,
			&options.sample.maximum, &options.budgetMs, &checkDeadlineMs);
		options.deadlineMs = checkDeadlineMs;
		std::mutex mutex;
		{
			g_WorkState->HoldDownloadForFleet(true);
			struct HoldRelease
			{
				~HoldRelease() { g_WorkState->HoldDownloadForFleet(false); }
			} holdRelease;
			DonorHealth::CheckPostings(servers, postings, options, true,
				std::min((int)postings.size(), MaxParallel),
				[&](const std::string& key, const DonorHealth::Health& health)
				{
					std::lock_guard<std::mutex> guard(mutex);
					Candidate& candidate = candidates[(size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str())];
					candidate.health = health;
					candidate.checked = true;
				});
		}
		result.complete = DonorHealth::NowMs() - checkStart < options.budgetMs;
	}
	for (Candidate& candidate : candidates)
	{
		if (!candidate.error.empty() || candidate.sameAs >= 0)
		{
			continue;
		}
		int alivePct = -1, isDead = 0, memberComplete = 0;
		nzbget_rs_df_eval_fleet_member_health(
			candidate.checked ? 1 : 0, result.complete ? 1 : 0,
			candidate.health.present, candidate.health.missing, candidate.health.error,
			DonorHealth::MinKnown, g_Options->GetDupeMinAlive(),
			&alivePct, &isDead, &memberComplete);
		candidate.alive = alivePct;
		candidate.dead = isDead != 0;
		result.complete &= memberComplete != 0;
	}
	for (Candidate& candidate : candidates)
	{
		if (candidate.sameAs >= 0)
		{
			candidate.alive = candidates[candidate.sameAs].alive;
			candidate.dead = candidates[candidate.sameAs].dead;
		}
	}

	std::vector<uint8_t> hasError(candidates.size());
	std::vector<int> sameAs(candidates.size());
	std::vector<uint8_t> isDeadArr(candidates.size());
	std::vector<int> aliveArr(candidates.size());
	std::vector<int64_t> totalBytesArr(candidates.size());
	std::vector<size_t> rankedIdx(candidates.size());
	for (size_t i = 0; i < candidates.size(); i++)
	{
		hasError[i] = candidates[i].error.empty() ? 0 : 1;
		sameAs[i] = candidates[i].sameAs;
		isDeadArr[i] = candidates[i].dead ? 1 : 0;
		aliveArr[i] = candidates[i].alive;
		totalBytesArr[i] = candidates[i].summary.totalBytes;
	}
	size_t rankedCount = nzbget_rs_df_rank_fleet_candidates(
		hasError.data(), sameAs.data(), isDeadArr.data(), aliveArr.data(), totalBytesArr.data(),
		candidates.size(), rankedIdx.data());
	std::vector<Candidate*> order;
	std::vector<Candidate*> ranked;
	ranked.reserve(rankedCount);
	for (size_t i = 0; i < rankedCount; i++)
	{
		Candidate* c = &candidates[rankedIdx[i]];
		ranked.push_back(c);
		if (c->sameAs < 0)
		{
			order.push_back(c);
		}
	}
	auto tier = [](const Candidate& c)
	{
		return nzbget_rs_dqh_fleet_candidate_tier(
			!c.error.empty() ? 1 : 0, c.dead ? 1 : 0, c.alive);
	};

	if (DonorHealth::Stopping() || HttpGet::Stopped())
	{
		result.reason = "SHUTDOWN";
		result.complete = false;
		return result;
	}

	bool anyAlive = std::any_of(order.begin(), order.end(),
		[&](const Candidate* c) { return tier(*c) <= 1; });
	std::vector<int> added(candidates.size(), 0);
	int rank = 0;
	for (Candidate* candidate : ranked)
	{
		Member& member = request.members[candidate->index];
		Entry entry;
		entry.member = (int)candidate->index;
		entry.name = member.name;
		entry.rank = ++rank;
		entry.alive = candidate->alive;
		if (!candidate->error.empty())
		{
			entry.status = "ERROR";
			entry.reason = candidate->error;
		}
		else if (candidate->sameAs >= 0 || candidate->existingId)
		{
			entry.status = "SAME_POSTING";
			entry.sameAs = candidate->existingId ? candidate->existingId : added[candidate->sameAs];
			for (const Entry& other : result.members)
			{
				if (!candidate->existingId && other.member == candidate->sameAs)
				{
					entry.sameAsRank = other.rank;
				}
			}
		}
		else if (!anyAlive)
		{
			entry.status = "DEAD";
		}
		else
		{
			NzbParameterList parameters;
			NzbgetRsBuf rankBuf = nzbget_rs_dqh_conn_format_port_str(entry.rank);
			parameters.SetParameter(FleetParam, rankBuf.data ? rankBuf.data : "0");
			nzbget_rs_free(rankBuf);
			if (candidate->alive >= 0)
			{
				NzbgetRsBuf aliveBuf = nzbget_rs_dqh_conn_format_port_str(
					candidate->dead ? 0 : candidate->alive);
				parameters.SetParameter(DupeSearch::AliveParam, aliveBuf.data ? aliveBuf.data : "0");
				nzbget_rs_free(aliveBuf);
			}
			int nzbId = 0;
			NzbgetRsBuf nameBuf = nzbget_rs_dqh_fleet_ensure_nzb_ext(member.name.data(), member.name.size());
			std::string name(nameBuf.data ? nameBuf.data : "", nameBuf.len);
			nzbget_rs_free(nameBuf);
			int memberScore = nzbget_rs_dqh_fleet_calc_member_add_score(
				runningId, lowest, top, entry.rank);
			Scanner::EAddStatus status = g_Scanner->AddExternalFile(name.c_str(), request.category.c_str(),
				false, request.priority, request.dupeKey.c_str(),
				memberScore, dmScore, &parameters,
				false, false, nullptr, nullptr, member.data.data(), (int)member.data.size(), &nzbId);
			entry.nzbId = status == Scanner::asSuccess ? nzbId : 0;
			added[candidate->index] = entry.nzbId;
			entry.status = entry.nzbId == 0 ? "ERROR" : candidate->dead ? "DEAD" : "BACKUP";
			if (entry.nzbId == 0)
			{
				entry.reason = "RustyGet could not add the nzb-file";
			}
		}
		result.members.push_back(entry);
	}

	bool onDisk = false;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (runningId && downloadQueue->GetQueue()->Find(runningId))
		{
			result.chosen = runningId;
			result.reason = "ALREADY_QUEUED";
		}
		for (Entry& entry : result.members)
		{
			if (!runningId && !result.chosen && entry.nzbId > 0 && downloadQueue->GetQueue()->Find(entry.nzbId))
			{
				entry.status = "QUEUED";
				result.chosen = entry.nzbId;
			}
		}
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() != HistoryInfo::hkNzb ||
				historyInfo->GetNzbInfo()->GetDeleteStatus() != NzbInfo::dsCopy)
			{
				continue;
			}
			for (Entry& entry : result.members)
			{
				if (entry.nzbId == historyInfo->GetNzbInfo()->GetId())
				{
					entry.status = "COPY";
					entry.reason = "the same nzb-file as an item RustyGet has already";
				}
			}
		}
		NzbInfo probe;
		probe.SetDupeKey(request.dupeKey.c_str());
		onDisk = DupeCoordinator::DownloadedOnDisk(downloadQueue, &probe) != nullptr;
	}
	if (result.chosen == 0 && result.reason.empty())
	{
		bool anyUsable = std::any_of(candidates.begin(), candidates.end(),
			[](const Candidate& c) { return c.error.empty(); });
		bool anyUnknown = std::any_of(order.begin(), order.end(),
			[&](const Candidate* c) { return c->error.empty() && c->alive < 0; });
		int reasonCode = nzbget_rs_df_eval_fleet_outcome_reason(
			anyUsable ? 1 : 0, anyAlive ? 1 : 0, anyUnknown ? 1 : 0, onDisk ? 1 : 0);
		static const char* const kReasons[] = {
			"NO_USABLE_MEMBERS", "INCOMPLETE", "ALL_DEAD", "ALREADY_DOWNLOADED", "NOT_QUEUED"
		};
		result.reason = kReasons[(reasonCode >= 0 && reasonCode < 5) ? reasonCode : 4];
	}
	NzbgetRsBuf sumBuf = nzbget_rs_dqh_fleet_format_summary_log(
		(int)request.members.size(), request.dupeKey.c_str(),
		result.chosen, result.reason.c_str(), result.complete ? 1 : 0);
	info("%s", sumBuf.data ? sumBuf.data : "");
	nzbget_rs_free(sumBuf);
	return result;
}

#endif
