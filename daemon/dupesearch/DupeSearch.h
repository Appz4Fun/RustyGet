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


#ifndef DUPESEARCH_H
#define DUPESEARCH_H

#include "Observer.h"
#include "Thread.h"
#include "DownloadInfo.h"
#include <condition_variable>
#include <atomic>
#include <ctime>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <set>
#include <string>
#include "DonorHealth.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

struct NzbgetRsDeadPostingsStore;

class DeadPostings
{
public:
	static constexpr int DeadTtlSec = 3 * 24 * 3600;

	DeadPostings()
		: m_store(nzbget_rs_deadpostings_store_new())
	{
	}

	~DeadPostings()
	{
		nzbget_rs_deadpostings_store_free(m_store);
	}

	DeadPostings(const DeadPostings&) = delete;
	DeadPostings& operator=(const DeadPostings&) = delete;

	void SetStatePath(const std::string& path)
	{
		m_statePath = path;
		nzbget_rs_deadpostings_store_set_state_path(
			m_store, reinterpret_cast<const uint8_t*>(path.data()), path.size());
	}

	void Load()
	{
		nzbget_rs_deadpostings_store_load(m_store, (int64_t)Util::CurrentTime());
	}

	bool IsDead(const Posting::Sketch& sketch, time_t now = 0)
	{
		now = now ? now : Util::CurrentTime();
		return nzbget_rs_deadpostings_store_is_dead(
			m_store, sketch.data(), sketch.size(), (int64_t)now) != 0;
	}

	void Add(const Posting::Sketch& sketch, time_t now = 0, bool save = true)
	{
		now = now ? now : Util::CurrentTime();
		if (nzbget_rs_deadpostings_store_add(
			m_store, sketch.data(), sketch.size(), (int64_t)now, save ? 1 : 0) == 0)
		{
			warn("Could not save the DupeSearch dead postings to %s", m_statePath.c_str());
		}
	}

private:
	NzbgetRsDeadPostingsStore* m_store;
	std::string m_statePath;
};

class NzbFetcher
{
public:
	enum EReason
	{
		frOk,
		frFetch,		// the download failed
		frParse,		// not an nzb-file (an error answer, an empty body, HTML)
		frRefused,		// the indexer is at its grab limit: not asked
		frBudget
	};

	struct Fetched
	{
		Newznab::Result listing;
		EReason reason = frFetch;
		std::string data;
		NzbSummary info;
		bool sameInner = false;
		bool lenient = false;
	};

	struct Stats
	{
		int ok = 0;
		int fetch = 0;
		int parse = 0;
		int refused = 0;
		int relisted = 0;			// listings of a fetched posting that weren't needed
		int listingMismatch = 0;	// postings only an nzb-file of another size was served for
		int budget = 0;
		void Add(const Stats& other)
		{
			ok += other.ok;
			fetch += other.fetch;
			parse += other.parse;
			refused += other.refused;
			relisted += other.relisted;
			listingMismatch += other.listingMismatch;
			budget += other.budget;
		}
	};

	static constexpr int CooldownSec = 30 * 60;
	static constexpr int RetryDelaySec = 2;
	static constexpr size_t MaxNzbBytes = 64 * 1024 * 1024;
	class Budget
	{
	public:
		explicit Budget(int maxDonors) : m_left(maxDonors > 0 ? (unsigned long long)maxDonors * 3 : 0), m_unlimited(maxDonors <= 0)
		{
			m_rust = nzbget_rs_dupe_budget_new(maxDonors > 0 ? (uint32_t)maxDonors : 0);
		}
		~Budget()
		{
			nzbget_rs_dupe_budget_free(m_rust);
		}
		Budget(const Budget&) = delete;
		Budget& operator=(const Budget&) = delete;
		bool Take() { return nzbget_rs_dupe_budget_take(m_rust) != 0; }
		bool Spent() const { return nzbget_rs_dupe_budget_spent(m_rust) != 0; }
		void* RustHandle() const { return m_rust; }
	private:
		void* m_rust = nullptr;
		std::atomic<unsigned long long> m_left;
		bool m_unlimited;
	};
	using FetchFunc = std::function<HttpGet::Reply(const std::string&, const std::string&, size_t, long long)>;
	NzbFetcher() : m_rust(nzbget_rs_nzb_fetcher_new()) {}
	explicit NzbFetcher(FetchFunc fetch) : m_rust(nzbget_rs_nzb_fetcher_new()), m_fetch(std::move(fetch)) {}
	~NzbFetcher()
	{
		nzbget_rs_nzb_fetcher_free(m_rust);
	}
	NzbFetcher(const NzbFetcher&) = delete;
	NzbFetcher& operator=(const NzbFetcher&) = delete;

	void SetStatePath(const std::string& path)
	{
		nzbget_rs_nzb_fetcher_set_state_path(m_rust, path.data(), path.size());
	}

	void Load()
	{
		nzbget_rs_nzb_fetcher_load(m_rust, (int64_t)Util::CurrentTime());
	}

	Fetched Fetch(const Newznab::Result& listing, time_t deadline, int retries = 1, Budget* budget = nullptr)
	{
		Fetched fetched;
		fetched.listing = listing;
		struct FetchCtx
		{
			NzbFetcher* self;
			HttpGet::Reply lastReply;
		} ctx{this, {}};
		NzbgetRsBuf dataBuf{};
		RsNzbSummaryScalars scalars{};
		int reason = nzbget_rs_nzb_fetcher_fetch(
			m_rust,
			listing.indexer.data(), listing.indexer.size(),
			listing.title.data(), listing.title.size(),
			listing.link.data(), listing.link.size(),
			listing.size, listing.grabs, (int64_t)listing.date,
			(int64_t)deadline, retries,
			budget ? budget->RustHandle() : nullptr,
			&ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void*) { Util::Sleep(100); },
			[](void* rawCtx, const char* url, size_t urlLen, const char* label, size_t labelLen,
				size_t maxBytes, int64_t deadlineMs, int* outStatus,
				const char** outBodyPtr, size_t* outBodyLen) -> int {
				auto* c = static_cast<FetchCtx*>(rawCtx);
				std::string urlStr(url ? url : "", urlLen);
				std::string labelStr(label ? label : "", labelLen);
				c->lastReply = c->self->m_fetch
					? c->self->m_fetch(urlStr, labelStr, maxBytes, deadlineMs)
					: HttpGet::Fetch(urlStr, labelStr, maxBytes, deadlineMs);
				*outStatus = c->lastReply.status;
				*outBodyPtr = c->lastReply.body.empty() ? nullptr : c->lastReply.body.data();
				*outBodyLen = c->lastReply.body.size();
				return c->lastReply.ok ? 1 : 0;
			},
			[](void*, const char* msg) {
				info("%s", msg ? msg : "");
			},
			[](void*, const char* msg) {
				warn("%s", msg ? msg : "");
			},
			&dataBuf,
			&scalars,
			&fetched.info,
			NzbReader::OnStrings,
			NzbReader::OnSlice,
			NzbReader::OnRecoveryBytes);
		fetched.reason = static_cast<EReason>(reason);
		if (fetched.reason == frOk && dataBuf.data)
		{
			fetched.data.assign(dataBuf.data, dataBuf.len);
			NzbReader::ApplyScalars(fetched.info, scalars);
		}
		nzbget_rs_free(dataBuf);
		return fetched;
	}

	Fetched FetchPosting(const Posting::Group& listings, time_t deadline, Stats& stats, Budget* budget = nullptr)
	{
		Fetched last;
		if (listings.empty()) { stats.parse++; return last; }
		last.listing = listings.front();
		bool haveOther = false;
		Fetched other;

		for (size_t i = 0; i < listings.size(); i++)
		{
			Fetched fetched = Fetch(listings[i], deadline, 1, budget);
			if (fetched.reason == frBudget) { stats.budget++; last = std::move(fetched); break; }
			if (fetched.reason == frOk && !Posting::ListingMismatch(listings[i], fetched.info.totalBytes))
			{
				stats.ok++;
				stats.relisted += (int)(listings.size() - i - 1);
				return fetched;
			}
			if (fetched.reason == frOk)
			{
				if (!haveOther)
				{
					other = std::move(fetched);
					haveOther = true;
				}
				continue;
			}
			switch (fetched.reason)
			{
				case frFetch: stats.fetch++; break;
				case frParse: stats.parse++; break;
				case frRefused: stats.refused++; break;
				default: break;
			}
			last = std::move(fetched);
		}

		if (haveOther)
		{
			stats.ok++;
			stats.listingMismatch++;
			return other;
		}
		return last;
	}

	bool IsRefused(const std::string& indexer)
	{
		return nzbget_rs_nzb_fetcher_is_refused(
			m_rust, indexer.data(), indexer.size(), (int64_t)Util::CurrentTime()) != 0;
	}

private:
	void* m_rust = nullptr;
	FetchFunc m_fetch;
};

/*
 * Option <DupeSearch>: looks for other postings of a new download on a
 * Newznab indexer and queues them as duplicates (see DupeSearchJob).
 *
 * This part decides WHICH downloads are searched. A download added to the
 * queue is noted, and after <DupeSearchDelay> seconds - a client that
 * submits backups of its own has added them by then - it is searched if it
 * is still the top-scored item of its duplicate key, hasn't passed the
 * download stage, isn't a duplicate this search queued itself, and its key
 * wasn't searched lately (at most one search per key within SearchWindowSec
 * unless a pick with a higher score arrives). What was searched is kept
 * across restarts, and a restart looks at the queue once for downloads whose
 * add event it missed.
 */
class DupeSearch : public Thread
{
public:
	DupeSearch();
	~DupeSearch() override;
	void Run() override;
	void Stop() override;
    void NotifyNameChanged(int nzbId);

	// seconds a key is not searched again, unless a higher-scored pick arrives
	static constexpr int SearchWindowSec = 6 * 3600;
	// a pick still in post-processing when its search is due is asked again this much later
	static constexpr int PostProcessRetrySec = 30;
	// a pick that failed this recently is searched from history at start (B64)
	static constexpr int RecentFailureSec = 3600;
	// keys are forgotten after this long
	static constexpr int StateTtlSec = 30 * 24 * 3600;
	// the score of a managed pick, and the room below it for donors
	static constexpr int BasePickScore = 1000000;
	// search and fetch of one pick are meant to fit in this long
	static constexpr int SearchDeadlineSec = 60;

	// parameters of the duplicates this search queued: a marker, and the
	// share of the posting found alive (percent)
	static constexpr const char* DonorParam = "DupeSearch";
	static constexpr const char* AliveParam = "DupeAlive";
	// the measured share alive (percent) of a duplicate in history this search
	// ranked but didn't add (a client's own backup, an earlier donor)
	static constexpr const char* HealthParam = "DupeHealth";
    static constexpr const char* OwnedKeyParam = "DupeSearchOwnedKey";

	/* "dupes:" + the normalized title: the duplicate key of a download that has none */
	static std::string MakeDupeKey(const std::string& name);

	/* has this search queued the nzb itself, or has the nzbget-dupe-proxy
	 * (it marks its duplicates with DupeAlive)? */
	static bool IsDonor(NzbInfo* nzbInfo);

private:
	class DownloadQueueObserver final : public Observer
	{
	public:
		DupeSearch* m_owner;
		void Update(Subject* caller, void* aspect) override { m_owner->DownloadQueueUpdate(static_cast<DownloadQueue*>(caller), aspect); }
	};

	struct Searched
	{
		int score = 0;
		time_t time = 0;
		std::string title;
		bool retryable = false;
        std::string rawName; // Runtime-only normalization cache; never inferred from disk.
	};

	struct Job
	{
		int nzbId = 0;
		bool failed = false;
		bool rankOnly = false;
		std::string name;
		std::string dupeKey;
		std::string category;
		std::string imdb;	// from the nzb-file's meta data
		std::string tvdb;
		std::string queuedFile;	// the nzb-file of the pick
		std::vector<std::string> knownFiles;	// the nzb-files nzbget keeps for this release (queue and history)
		// the duplicates of the key nzbget keeps in history (DELETED/DUPE),
		// whoever sent them: id and the item's own nzb-file
		std::vector<std::pair<int, std::string>> members;
		std::vector<int> memberScores;	// their scores when the search began
		int score = 0;
	};

	// postings fetched at the same time
	static constexpr int FetchParallel = 4;

	DownloadQueueObserver m_observer;
	NzbFetcher m_fetcher;
	DeadPostings m_dead;
	std::mutex m_mutex;
	std::condition_variable m_cond;
	std::set<int> m_rankPending;
	std::map<int, time_t> m_pending;			// nzb id -> when to look at it
	std::map<std::string, Searched> m_searched;	// lowercase dupe key -> last search
    std::map<std::pair<std::string, std::string>, Searched> m_titleSearched;
	// pick id -> fingerprints its search queued; per pick, not per key: a pick sent
	// again after the user deleted it and its duplicates gets them back (B41)
	std::map<int, std::set<std::string>> m_sent;

	void DownloadQueueUpdate(DownloadQueue* downloadQueue, void* aspect);
	void Schedule(int nzbId, time_t due);
	void ScanQueue();
	bool Prepare(DownloadQueue* downloadQueue, int nzbId, Job& job);
	bool PrepareNative(DownloadQueue* downloadQueue, int nzbId, Job& job);
	void Expand(const Job& job, const NzbSummary& pick, const std::vector<Newznab::Result>& prior,
        const std::vector<Posting::Sketch>& known, time_t deadline, NzbFetcher::Budget& budget,
		std::vector<NzbFetcher::Fetched>& fetched, std::set<std::string>& attempted, NzbFetcher::Stats& stats, Newznab::SearchStats& queryStats,
		int& results, int& candidates, int& postings);
	void Search(const Job& job);
	/* health checks, adds and ranks the verified postings and the key's duplicates in history */
	void Place(const Job& job, const NzbSummary& pick, std::vector<NzbFetcher::Fetched>& verified,
		std::map<std::string, int>& rejected, int results, int candidates, int postings);
	void Collect(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, Job& job);

	/*
	 * A search in progress survives a restart: QueueDir/dupesearch-pending/<nzb id>/
	 * holds a phase file ("searching", then "fetched" once the fetched nzb-files
	 * are saved beside it as <n>.nzb and <n>.meta). At startup a fetched search
	 * resumes from the saved files (no search, no fetch: grabs are scarce); one
	 * still searching is searched again; one whose pick is gone is dropped.
	 */
	/* a line of the search of pick <nzbId>: in the pick's own log and the global log */
	void Note(int nzbId, Message::EKind kind, const char* format, ...) PRINTF_SYNTAX(4);
	std::string PendingDir(int nzbId);
	void MarkSearching(const Job& job);
	void SavePending(const Job& job, const NzbSummary& pick, const std::vector<NzbFetcher::Fetched>& verified);
	void RemovePending(int nzbId);
	void ResumePending();
	/* why the search may no longer add duplicates for the pick (deleted by the user,
	 * another duplicate key), empty while it may */
	std::string PickGone(const Job& job);
	/* queues a posting as a duplicate of the pick; the download id, 0 when not added */
	int AddDonor(const Job& job, const NzbSummary& pick, const NzbFetcher::Fetched& posting, int score, double alive);
	/* the DupeScore (and a parameter "Name=Value") of a duplicate that sits in history or in the queue */
	bool SetScore(int id, int score, const std::vector<std::string>& params);
	void LoadState();
	bool SaveState();
	// the key may be searched again (a search that learned nothing)
    // Callers hold m_mutex; the flat map is only the latest-name cache.
    Searched* FindSearch(const std::string& lowerKey, const std::string& title);
    void RecordSearch(const std::string& lowerKey, const Searched& searched);
    void SetRetryable(const std::string& lowerKey, const std::string& title, bool retryable);
	bool ForgetSearch(const std::string& dupeKey, const std::string& title = "");
	std::string StatePath();
};

extern DupeSearch* g_DupeSearch;

#include <chrono>
#include <limits>
#include <fstream>
#include <sstream>
#include "ArticleFetcher.h"
#include "DonorHealth.h"
#include "Scanner.h"
#include <algorithm>
#include <thread>
#include "FileSystem.h"
#include "Log.h"
#include "Options.h"
#include "Util.h"
#include "nzbget_rs.h"

class DonorScore
{
public:
	static constexpr int Dead = 1;
	static constexpr int Top = 90;

	static int Base(int pickScore)
	{
		return nzbget_rs_donorscore_base(pickScore);
	}

	static int Target(double alive, bool twin)
	{
		return nzbget_rs_donorscore_target(alive, twin ? 1 : 0);
	}

	class Ranks
	{
	public:
		int Take(double alive, bool twin)
		{
			std::vector<int32_t> used(m_used.begin(), m_used.end());
			int score = nzbget_rs_donorscore_take_rank(alive, twin ? 1 : 0, used.data(), used.size());
			m_used.insert(score);
			return score;
		}
		void Release(int score) { m_used.erase(score); }
		void Use(int score) { m_used.insert(score); }
	private:
		std::set<int> m_used;
	};

	struct Entry
	{
		int score = 0;
		double alive = -1;
		bool twin = false;
		long long bytes = 0;
		int grabs = 0;
	};

	static std::vector<int> Rerank(const std::vector<Entry>& entries, long long pickBytes)
	{
		std::vector<NzbgetRsDonorEntry> raw;
		raw.reserve(entries.size());
		for (const Entry& e : entries)
		{
			raw.push_back({e.score, e.alive, e.twin ? 1 : 0, (int64_t)e.bytes, e.grabs});
		}
		std::vector<int> scores(entries.size());
		nzbget_rs_donorscore_rerank(raw.data(), raw.size(), (int64_t)pickBytes, scores.data());
		return scores;
	}
};

class DupeIdentity
{
public:
	struct Identity
	{
		std::string name;
		std::string extension;
		unsigned long long bytes = 0;
		explicit operator bool() const { return bytes != 0 && !extension.empty(); }
	};
	using FetchFunc = std::function<ArticleFetcher::FetchedArticle(const std::string&, const std::vector<CString>&)>;

	static Identity FromArticle(const std::vector<char>& data, const std::string& name,
		unsigned long long total, unsigned long long minimum)
	{
		Identity identity;
		NzbgetRsBuf raw = nzbget_rs_dupe_inner(data.data(), data.size(), name.data(), name.size(), total, minimum);
		uint64_t bytes = 0;
		size_t extensionSize = 0;
		size_t nameOffset = 0;
		size_t nameLen = 0;
		if (nzbget_rs_dupe_identity_decode_inner_buf(
			raw.data, raw.len, &bytes, &extensionSize, &nameOffset, &nameLen) != 0)
		{
			identity.bytes = bytes;
			identity.extension.assign(raw.data + 12, extensionSize);
			identity.name.assign(raw.data + nameOffset, nameLen);
		}
		nzbget_rs_free(raw);
		return identity;
	}

	static ArticleFetcher::FetchedArticle FetchArticle(const NzbSummary::Head& head,
		const std::vector<std::string>& names, time_t deadline, const FetchFunc& fetch = FetchFunc())
	{
		struct FetchCtx
		{
			const std::vector<std::string>* names;
			const FetchFunc* fetch;
			ArticleFetcher fetcher;
			ArticleFetcher::FetchedArticle article;
		} ctx{&names, &fetch, {}, {}};

		int ok = nzbget_rs_dupe_identity_fetch_article(
			head.messageId.data(), head.messageId.size(), (int64_t)deadline, &ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void* raw, const char* bracketedId, size_t bracketedLen,
			   int* outSuccess, int64_t* outOffset, int64_t* outFileSize) -> int {
				auto* c = static_cast<FetchCtx*>(raw);
				std::vector<CString> groups;
				for (const std::string& name : *c->names) groups.emplace_back(name.c_str());
				std::string id(bracketedId ? bracketedId : "", bracketedLen);
				try
				{
					c->article = (*c->fetch) ? (*c->fetch)(id, groups) : c->fetcher.Fetch(id.c_str(), groups);
				}
				catch (...)
				{
					c->article = ArticleFetcher::FetchedArticle();
					return 0;
				}
				*outSuccess = c->article.Success ? 1 : 0;
				*outOffset = c->article.Offset;
				*outFileSize = c->article.FileSize;
				return 1;
			},
			[](void* raw) {
				static_cast<FetchCtx*>(raw)->fetcher.Stop();
			});
		if (!ok)
			return ArticleFetcher::FetchedArticle();
		return ctx.article;
	}

	static Identity Read(const NzbSummary& info, time_t deadline, const FetchFunc& fetch = FetchFunc())
	{
		std::vector<const char*> headNames(info.heads.size());
		std::vector<size_t> headNameLens(info.heads.size());
		std::vector<const char*> headMsgIds(info.heads.size());
		std::vector<size_t> headMsgIdLens(info.heads.size());
		std::vector<int64_t> headBytes(info.heads.size());
		for (size_t i = 0; i < info.heads.size(); ++i)
		{
			headNames[i] = info.heads[i].name.data();
			headNameLens[i] = info.heads[i].name.size();
			headMsgIds[i] = info.heads[i].messageId.data();
			headMsgIdLens[i] = info.heads[i].messageId.size();
			headBytes[i] = info.heads[i].bytes;
		}
		struct ReadCtx
		{
			const NzbSummary* info;
			const FetchFunc* fetch;
			ArticleFetcher::FetchedArticle lastArticle;
		} ctx{&info, &fetch, {}};

		NzbgetRsBuf nameBuf{nullptr, 0, 0};
		NzbgetRsBuf extBuf{nullptr, 0, 0};
		uint64_t bytes = 0;
		int found = nzbget_rs_dupe_identity_read(
			info.dataBytes,
			headNames.empty() ? nullptr : headNames.data(),
			headNameLens.empty() ? nullptr : headNameLens.data(),
			headMsgIds.empty() ? nullptr : headMsgIds.data(),
			headMsgIdLens.empty() ? nullptr : headMsgIdLens.data(),
			headBytes.empty() ? nullptr : headBytes.data(),
			info.heads.size(), (int64_t)deadline, &ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void* raw, const char* msgId, size_t msgIdLen, int64_t dl,
			   int* outSuccess, int64_t* outFileSize,
			   const char** outDataPtr, size_t* outDataLen) {
				auto* c = static_cast<ReadCtx*>(raw);
				NzbSummary::Head head;
				head.messageId.assign(msgId ? msgId : "", msgIdLen);
				c->lastArticle = FetchArticle(head, c->info->groups, (time_t)dl, *c->fetch);
				*outSuccess = c->lastArticle.Success ? 1 : 0;
				*outFileSize = c->lastArticle.FileSize;
				*outDataPtr = c->lastArticle.Data.empty() ? nullptr : c->lastArticle.Data.data();
				*outDataLen = c->lastArticle.Data.size();
			},
			&nameBuf, &extBuf, &bytes);
		Identity identity;
		if (found)
		{
			if (nameBuf.data && nameBuf.len > 0) identity.name.assign(nameBuf.data, nameBuf.len);
			if (extBuf.data && extBuf.len > 0) identity.extension.assign(extBuf.data, extBuf.len);
			identity.bytes = bytes;
		}
		nzbget_rs_free(nameBuf);
		nzbget_rs_free(extBuf);
		return identity;
	}

	static unsigned long long WholeBytes(const NzbSummary& info)
	{
		if (info.totalBytes <= 0) return 0;
		return nzbget_rs_dupe_whole_bytes((uint64_t)info.totalBytes, info.listedParts, info.declaredParts);
	}

	static double Alive(const DonorHealth::Health& health, const NzbSummary& info)
	{
		return nzbget_rs_dupe_effective_alive(health.present, health.missing, health.error, info.listedParts, info.declaredParts);
	}

	static bool IsDead(const DonorHealth::Health& health, const NzbSummary& info, int minimum)
	{
		return nzbget_rs_dupe_is_dead(health.present, health.missing, health.error, info.listedParts, info.declaredParts, minimum) != 0;
	}

	static int UnlistedMissing(const DonorHealth::Health& health, const NzbSummary& info)
	{
		return nzbget_rs_dupe_unlisted_missing(health.Answered(), info.listedParts, info.declaredParts);
	}

	static bool ParDoomed(const NzbSummary& info, double alive, time_t deadline)
	{
		if (info.parIndex.messageId.empty() || info.recoveryBytes.empty() || info.dataBytes <= 0 || info.articleBytes == 0) return false;
		auto article = FetchArticle(info.parIndex, info.groups, deadline);
		if (!article.Success) return false;
		uint64_t block = nzbget_rs_dupe_par_block(article.Data.data(), article.Data.size());
		return nzbget_rs_dupe_par_doomed((uint64_t)info.dataBytes, info.articleBytes,
			info.recoveryBytes.data(), info.recoveryBytes.size(), alive, block) != 0;
	}

	static bool OuterCompatible(const std::string& title, const std::string& name)
	{
		return nzbget_rs_dupe_outer_ok(title.data(), title.size(), name.data(), name.size()) != 0;
	}

	static bool SameData(const NzbSummary& a, const NzbSummary& b)
	{
		if (a.dataBytes <= 0 || b.dataBytes <= 0 || a.dataFiles <= 0 || b.dataFiles <= 0) return false;
		return nzbget_rs_dupe_same_data((uint64_t)a.dataBytes, (uint32_t)a.dataFiles,
			(uint64_t)b.dataBytes, (uint32_t)b.dataFiles) != 0;
	}

	static bool Candidate(const std::string& title, long long bytes, const std::string& name, long long size)
	{
		return nzbget_rs_dupe_candidate(title.data(), title.size(), bytes > 0 ? bytes : 0,
			name.data(), name.size(), size > 0 ? size : 0) != 0;
	}

	static bool Matches(const std::string& title, const std::string& mainName,
		const Identity& mine, const Identity& theirs)
	{
		NzbgetRsDupeIdentityInput a{ mine.bytes, mine.name.data(), mine.name.size(), mine.extension.data(), mine.extension.size() };
		NzbgetRsDupeIdentityInput b{ theirs.bytes, theirs.name.data(), theirs.name.size(), theirs.extension.data(), theirs.extension.size() };
		return nzbget_rs_dupe_lenient_ok(title.data(), title.size(), mainName.data(), mainName.size(),
			mine ? &a : nullptr, theirs ? &b : nullptr) != 0;
	}

	static std::vector<std::string> Hints(const NzbSummary& pick, const NzbSummary& donor, bool sameInner)
	{
		std::vector<std::string> hints;
		if (SameData(pick, donor)) hints.emplace_back("DupeSameSize=1");
		if (sameInner) hints.emplace_back("DupeSameInner=1");
		return hints;
	}
};

inline DupeSearch* g_DupeSearch = nullptr;

namespace
{
// a download by id, in the queue or else in history
NzbInfo* FindNzb(DownloadQueue* downloadQueue, int nzbId)
{
	if (NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(nzbId))
	{
		return nzbInfo;
	}
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == nzbId)
		{
			return historyInfo->GetNzbInfo();
		}
	}
	return nullptr;
}

bool SavedHistoryBackup(NzbInfo* pick, NzbInfo* candidate)
{
    return nzbget_rs_df_saved_history_backup(
        pick != nullptr ? 1 : 0, pick ? pick->GetFullContentHash() : 0,
        (int)candidate->GetDeleteStatus(), candidate->GetFullContentHash(),
        (int)NzbInfo::dsDupe, (int)NzbInfo::dsCopy) != 0;
}

bool PlacementAvailable(DownloadQueue* queue, NzbInfo* pick, NzbInfo* backup, bool wholeBackup = false)
{
    if (!backup || backup->GetKind() != NzbInfo::nkNzb || backup->GetDeleting() ||
        backup->GetDeleteStatus() == NzbInfo::dsManual || backup->GetMarkStatus() == NzbInfo::ksBad ||
        backup->GetDupeMode() == dmForce) return false;
    bool inHistory = queue->GetQueue()->Find(backup->GetId()) != backup;
    bool isDupeSuccess = inHistory && backup->IsDupeSuccess();
    bool hasPostInfo = backup->GetPostInfo() != nullptr;
    bool savedBackup = SavedHistoryBackup(pick, backup);
    bool needDiskAndHealth = inHistory && !isDupeSuccess && (!wholeBackup || !hasPostInfo) && savedBackup;
    bool queuedFileExists = needDiskAndHealth && FileSystem::FileExists(backup->GetQueuedFilename());
    int health = (needDiskAndHealth && queuedFileExists && wholeBackup) ? backup->CalcHealth() : 0;
    int critHealth = (needDiskAndHealth && queuedFileExists && wholeBackup) ? backup->CalcCriticalHealth(true) : 0;
    NzbParameter* markedAlive = backup->GetParameters()->Find(DupeSearch::AliveParam);
    return nzbget_rs_df_placement_available(
        1, 1, 0, (int)backup->GetDeleteStatus(),
        (int)NzbInfo::dsNone, (int)NzbInfo::dsManual, (int)NzbInfo::dsDupe, (int)NzbInfo::dsCopy,
        (int)backup->GetMarkStatus(), (int)NzbInfo::ksBad,
        (int)backup->GetDupeMode(), (int)dmForce,
        inHistory ? 1 : 0, wholeBackup ? 1 : 0, hasPostInfo ? 1 : 0, isDupeSuccess ? 1 : 0,
        pick != nullptr ? 1 : 0, pick ? pick->GetFullContentHash() : 0, backup->GetFullContentHash(),
        queuedFileExists ? 1 : 0, health, critHealth,
        g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0,
        markedAlive != nullptr ? 1 : 0,
        markedAlive ? nzbget_rs_dqh_parse_c_atoi(markedAlive->GetValue()) : 0) != 0;
}


std::string LowerKey(const std::string& key)
{
	NzbgetRsBuf buf = nzbget_rs_df_lower_key(key.c_str());
	std::string out(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return out;
}

// the key a download is searched under: its own, or one made from its name
std::string EffectiveKey(NzbInfo* nzbInfo)
{
	if (!Util::EmptyStr(nzbInfo->GetDupeKey()))
	{
		return nzbInfo->GetDupeKey();
	}
	return DupeSearch::MakeDupeKey(nzbInfo->GetName());
}

}

inline std::string DupeSearch::MakeDupeKey(const std::string& name)
{
	return "dupes:" + ReleaseName::Normalize(name);
}

inline bool DupeSearch::IsDonor(NzbInfo* nzbInfo)
{
	return nzbInfo->GetParameters()->Find(DonorParam) || nzbInfo->GetParameters()->Find(AliveParam);
}

inline DupeSearch::DupeSearch()
{
	HttpGet::Reset();
	DonorHealth::Reset();
	m_observer.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_observer);
}

inline DupeSearch::~DupeSearch()
{
	debug("Destroying DupeSearch");
	// the queue (QueueCoordinator's) outlives this, and would notify freed memory
	DownloadQueue::Guard()->Detach(&m_observer);
}

inline void DupeSearch::Stop()
{
	Thread::Stop();
	HttpGet::StopAll();
	DonorHealth::StopAll();
	std::lock_guard<std::mutex> guard(m_mutex);
	m_cond.notify_all();
}

inline void DupeSearch::NotifyNameChanged(int nzbId)
{
    if (g_Options->GetDupeSearch() && !IsStopped())
        Schedule(nzbId, Util::CurrentTime() + g_Options->GetDupeSearchDelay());
}

inline void DupeSearch::DownloadQueueUpdate(DownloadQueue* downloadQueue, void* aspect)
{
	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if ((queueAspect->action == DownloadQueue::eaNzbAdded || queueAspect->action == DownloadQueue::eaNzbNamed) &&
		queueAspect->nzbInfo && g_Options->GetDupeSearch() && !IsStopped())
	{
		Schedule(queueAspect->nzbInfo->GetId(), Util::CurrentTime() + g_Options->GetDupeSearchDelay());
        // An external lower-scored backup can arrive after the initial search.
        // Queue notifications run under the queue lock; only schedule work here.
        NzbInfo* added = queueAspect->nzbInfo;
        if (nzbget_rs_dqh_dupesearch_should_check_added_for_parent_rank(
            (int)queueAspect->action,
            (int)DownloadQueue::eaNzbAdded,
            IsDonor(added) ? 1 : 0,
            (int)added->GetDupeMode(),
            (int)dmScore,
            added->GetParameters()->Find("DupeFleet") != nullptr ? 1 : 0) != 0)
        {
            NzbInfo* parent = nullptr;
            std::string addedKey = LowerKey(EffectiveKey(added));
            for (NzbInfo* item : downloadQueue->GetQueue())
            {
                bool sameLowerKey = item != added && LowerKey(EffectiveKey(item)) == addedKey;
                if (nzbget_rs_dqh_dupesearch_is_better_parent_candidate(
                    item == added ? 1 : 0,
                    (int)item->GetKind(),
                    (int)NzbInfo::nkNzb,
                    item->GetDeleting() ? 1 : 0,
                    (int)item->GetDeleteStatus(),
                    (int)NzbInfo::dsNone,
                    item->GetPostInfo() != nullptr ? 1 : 0,
                    sameLowerKey ? 1 : 0,
                    item->GetDupeScore(),
                    added->GetDupeScore(),
                    parent != nullptr ? 1 : 0,
                    parent ? parent->GetDupeScore() : 0) != 0)
                {
                    parent = item;
                }
            }
            if (parent)
            {
                std::lock_guard<std::mutex> guard(m_mutex);
                m_rankPending.insert(parent->GetId());
                m_pending[parent->GetId()] = Util::CurrentTime() + g_Options->GetDupeSearchDelay();
                m_cond.notify_all();
            }
        }
	}
}

inline void DupeSearch::Schedule(int nzbId, time_t due)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	m_pending[nzbId] = due;
	m_cond.notify_all();
}

// an event isn't fired for a download that was queued before a restart; nor for
// a pick that failed over while nzbget was stopping or starting (B64): one that
// failed lately is asked again (Prepare searches it from history once, unless
// its key was searched already)
inline void DupeSearch::ScanQueue()
{
	std::vector<int> ids;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetKind() == NzbInfo::nkNzb)
			{
				ids.push_back(nzbInfo->GetId());
			}
		}
		time_t recent = Util::CurrentTime() - RecentFailureSec;
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (historyInfo->GetKind() == HistoryInfo::hkNzb &&
				historyInfo->GetTime() >= recent &&
				!historyInfo->GetNzbInfo()->IsDupeSuccess())
			{
				ids.push_back(historyInfo->GetNzbInfo()->GetId());
			}
		}
	}
	for (int id : ids)
	{
		Schedule(id, Util::CurrentTime() + g_Options->GetDupeSearchDelay());
	}
}

inline void DupeSearch::Run()
{
	debug("Entering DupeSearch-loop");

	while (!DownloadQueue::IsLoaded() && !IsStopped())
	{
		Util::Sleep(20);
	}
	if (IsStopped())
	{
		return;
	}

	LoadState();
	m_fetcher.SetStatePath(std::string(g_Options->GetQueueDir()) + PATH_SEPARATOR + "dupesearch-indexers");
	m_fetcher.Load();
	m_dead.SetStatePath(std::string(g_Options->GetQueueDir()) + PATH_SEPARATOR + "dupesearch-dead");
	m_dead.Load();
	if (g_Options->GetDupeSearch())
	{
		ResumePending();
		ScanQueue();
	}

	while (!IsStopped())
	{
		std::vector<int> due;
		{
			std::unique_lock<std::mutex> lock(m_mutex);
			m_cond.wait_for(lock, std::chrono::milliseconds(500));
			time_t now = Util::CurrentTime();
			for (auto it = m_pending.begin(); it != m_pending.end();)
			{
				if (it->second <= now)
				{
					due.push_back(it->first);
					it = m_pending.erase(it);
				}
				else
				{
					++it;
				}
			}
		}

		for (int nzbId : due)
		{
			if (IsStopped())
			{
				break;
			}
			Job job;
			bool search;
			{
				GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
				search = Prepare(downloadQueue, nzbId, job);
			}
			if (search)
			{
				MarkSearching(job);
				Search(job);
			}
		}
	}

	// a health check's walkers may still be in a request: they end before this
	// thread does, so before the server pool goes away
	DonorHealth::WaitAll();
	debug("Exiting DupeSearch-loop");
}

/*
 * Decides whether a download is searched now. Runs under the queue lock.
 */
inline bool DupeSearch::Prepare(DownloadQueue* downloadQueue, int nzbId, Job& job)
{
    return PrepareNative(downloadQueue, nzbId, job);
}

inline bool DupeSearch::PrepareNative(DownloadQueue* downloadQueue, int nzbId, Job& job)
{
    if (!g_Options->GetDupeSearch() || !g_Options->GetDupeCheck()) return false;
    NzbInfo* pick = FindNzb(downloadQueue, nzbId);
    if (!pick) { std::lock_guard<std::mutex> guard(m_mutex); m_rankPending.erase(nzbId); return false; }
    bool history = downloadQueue->GetQueue()->Find(nzbId) != pick;
    std::string key = EffectiveKey(pick);
    std::string lowerKey = LowerKey(key);
    const std::string ownerPrefix = std::to_string(nzbId) + ":";
    NzbParameter* owner = pick->GetParameters()->Find(OwnedKeyParam);
    bool generatedKey = nzbget_rs_dqh_dupesearch_is_generated_key(
        Util::EmptyStr(pick->GetDupeKey()) ? 1 : 0,
        owner != nullptr ? 1 : 0,
        owner ? owner->GetValue() : nullptr,
        nzbId,
        key.c_str()) != 0;
    std::string title;
    Searched previous;
    bool havePrevious = false, changedIdentity = false, rankOnly = false, cachedName = false, rankRequested = false;
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        auto found = m_searched.find(lowerKey);
        if (found != m_searched.end()) {
            previous = found->second;
            havePrevious = true;
            // Missing legacy title is ambiguous unless this exact NZB carries
            // the persisted marker written when native discovery generated it.
            if (previous.title.empty() && generatedKey && key.compare(0, 6, "dupes:")==0)
                previous.title = key.substr(6);
            cachedName = !previous.title.empty() && previous.rawName == pick->GetName();
        }
        rankRequested = m_rankPending.erase(nzbId) != 0;
    }
    title = cachedName ? previous.title :
        Util::EmptyStr(pick->GetDupeKey()) ? key.substr(6) : ReleaseName::Normalize(pick->GetName());
    changedIdentity = havePrevious && !previous.title.empty() && previous.title != title;
    bool ownedKeyChanged = false;
    // Only rewrite a key this search previously generated; explicit client keys stay.
    if (generatedKey && !Util::EmptyStr(pick->GetDupeKey()) &&
        key.compare(0, 6, "dupes:") == 0 && key != "dupes:" + title)
    {
        key = "dupes:" + title;
        lowerKey = LowerKey(key);
        ownedKeyChanged = true;
        havePrevious = false;
    }
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        if (Searched* found = FindSearch(lowerKey, title))
        {
            previous = *found;
            havePrevious = true;
            changedIdentity = false;
            if (!previous.title.empty()) previous.rawName = pick->GetName();
            cachedName = !previous.title.empty();
            *found = previous;
            m_searched[lowerKey] = previous;
            rankOnly = rankRequested && previous.title == title;
        }
        else { havePrevious = false; cachedName = false; rankOnly = false; }
    }
    if (ownedKeyChanged)
    {
        rankOnly = false;
        if (!g_Options->GetDupeSearchDryRun())
        {
            pick->SetDupeKey(key.c_str());
            pick->GetParameters()->SetParameter(OwnedKeyParam, (ownerPrefix + key).c_str());
            pick->SetChanged(true);
            if (history) { downloadQueue->HistoryChanged(); downloadQueue->Save(); }
            else downloadQueue->SaveChanged();
        }
    }
    auto flags = [&](NzbInfo* item, bool inHistory) -> uint32_t {
        return nzbget_rs_dupe_watch_build_item_flags(
            (int)item->GetKind(),
            inHistory ? 1 : 0,
            item->IsDupeSuccess() ? 1 : 0,
            item->GetDeleting() ? 1 : 0,
            (int)item->GetDeleteStatus(),
            item->GetPostInfo() ? 1 : 0,
            (int)item->GetDupeMode(),
            IsDonor(item) ? 1 : 0,
            (int)item->GetDupeHint(),
            item->GetParameters()->Find("DupeFleet") ? 1 : 0);
    };
    uint32_t pickFlags = flags(pick, history);
    time_t now = Util::CurrentTime();
    const uint32_t preliminaryFlags = pickFlags;
    int preliminary = -1;
    if (cachedName && havePrevious && !previous.retryable && !rankOnly)
    {
        // A known unchanged title can be suppressed before building snapshots.
        // Any higher peer/fleet could only suppress it too; a Search still needs
        // the complete peer check below. Post-process retry precedes peer rules.
        preliminary = nzbget_rs_dupe_watch_stamped(pickFlags, pick->GetDupeScore(),
            (int64_t)now, previous.score, (int64_t)previous.time);
        if (preliminary == 0) return false;
        if (preliminary == 1) {
            Schedule(nzbId, now + std::max(PostProcessRetrySec, g_Options->GetDupeSearchDelay()));
            return false;
        }
    }
    NzbgetRsDupeWatchPeer snapshot{ nzbId, pick->GetDupeScore(), pickFlags, lowerKey.data(), lowerKey.size() };
    std::vector<NzbInfo*> others;
    std::vector<bool> histories;
    auto retain = [&](NzbInfo* item, bool inHistory) {
        // Lower/equal ordinary peers cannot affect the Rust highest-score rule.
        // Fleet markers still matter at any score, including history entries.
        if (nzbget_rs_dqh_dupesearch_should_retain_peer(
            item == pick ? 1 : 0,
            item->GetDupeScore(),
            pick->GetDupeScore(),
            item->GetParameters()->Find("DupeFleet") != nullptr ? 1 : 0) != 0)
        { others.push_back(item); histories.push_back(inHistory); }
    };
    for (NzbInfo* item : downloadQueue->GetQueue()) retain(item, false);
    for (HistoryInfo* item : downloadQueue->GetHistory()) if (item->GetKind() == HistoryInfo::hkNzb)
        retain(item->GetNzbInfo(), true);
    std::vector<std::string> keys;
    keys.reserve(others.size());
    for (NzbInfo* other : others) keys.push_back(LowerKey(EffectiveKey(other)));
    std::vector<NzbgetRsDupeWatchPeer> peers;
    for (size_t i = 0; i < others.size(); i++)
    {
        uint32_t f = flags(others[i], histories[i]);
        // Only a live eligible fleet member represents current management;
        // retired history/deleting entries must not poison future plain picks.
        if (nzbget_rs_dupe_watch_is_live_fleet_peer(
            keys[i].data(), keys[i].size(), lowerKey.data(), lowerKey.size(), f))
            pickFlags |= rsDupeManagedFleet;
        peers.push_back({ others[i]->GetId(), others[i]->GetDupeScore(), f, keys[i].data(), keys[i].size() });
    }
    snapshot.flags = pickFlags;
    // With no retained peers the snapshot is unchanged: the accepted early
    // decision already checked the exact same stamp, time and identity.
    int decision = preliminary == 2 && peers.empty() && snapshot.flags == preliminaryFlags ? preliminary :
        nzbget_rs_dupe_watch(&snapshot, peers.data(), peers.size(), (int64_t)now,
        previous.score, (int64_t)previous.time, havePrevious && !previous.retryable && !rankOnly, changedIdentity);
    if (decision == 1) {
        if (rankOnly) { std::lock_guard<std::mutex> guard(m_mutex); m_rankPending.insert(nzbId); }
        Schedule(nzbId, now + std::max(PostProcessRetrySec, g_Options->GetDupeSearchDelay()));
        return false;
    }
    if (decision != 2) return false;
    int score = std::max(pick->GetDupeScore(), BasePickScore);
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        if (!rankOnly) RecordSearch(lowerKey, { score, now, title, false, pick->GetName() });
    }
    if (!g_Options->GetDupeSearchDryRun() && !rankOnly) SaveState();
    bool edit = !g_Options->GetDupeSearchDryRun() && !history;
    if (edit)
    {
        pick->SetDupeKey(key.c_str());
        pick->GetParameters()->SetParameter(OwnedKeyParam, generatedKey ? (ownerPrefix + key).c_str() : nullptr);
        if (pick->GetDupeScore() < BasePickScore) pick->SetDupeScore(BasePickScore);
        pick->SetChanged(true);
        downloadQueue->SaveChanged();
    }
    job.rankOnly = rankOnly;
    Collect(downloadQueue, pick, job);
    job.dupeKey = key;
    job.score = score;
    job.failed = history;
    job.rankOnly = rankOnly;
    pick->PrintMessage(Message::mkInfo, "DupeSearch: searching duplicates of %s (key %s)%s",
        pick->GetName(), key.c_str(),
        nzbget_rs_dqh_dupesearch_prepare_status_suffix(rankOnly ? 1 : 0, history ? 1 : 0));
    return true;
}

/*
 * What a search of a pick needs to know of the queue and history. Runs under the queue lock.
 */
inline void DupeSearch::Collect(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, Job& job)
{
	std::string key = EffectiveKey(nzbInfo);
	std::string lowerKey = LowerKey(key);
	job.nzbId = nzbInfo->GetId();
	job.name = nzbInfo->GetName();
	job.dupeKey = key;
	job.category = nzbInfo->GetCategory();
	job.queuedFile = nzbInfo->GetQueuedFilename() ? nzbInfo->GetQueuedFilename() : "";

	// the nzb-files nzbget keeps for this release: queue and history items
	// with this duplicate key, or whose name is the same release
	auto addKnown = [&](NzbInfo* item)
	{
		// one the user deleted is no duplicate nzbget would fetch: its posting may
		// be found and added again (B41)
		if (item->GetKind() != NzbInfo::nkNzb ||
			Util::EmptyStr(item->GetQueuedFilename()) ||
			item->GetDeleteStatus() == NzbInfo::dsManual)
		{
			return;
		}
		if (LowerKey(EffectiveKey(item)) == lowerKey || ReleaseName::SameRelease(nzbInfo->GetName(), item->GetName()))
		{
			// the file of a merged group names several, separated by "|"
			const char* qf = item->GetQueuedFilename() ? item->GetQueuedFilename() : "";
			nzbget_rs_dqh_fleet_visit_pipe_names(qf, strlen(qf), &job.knownFiles,
				[](void* ctx, const char* namePtr, size_t nameLen)
				{
					static_cast<std::vector<std::string>*>(ctx)->emplace_back(namePtr, nameLen);
				});
		}
	};
	for (NzbInfo* item : downloadQueue->GetQueue())
	{
		addKnown(item);
        // Zero-delay ranking can precede asynchronous duplicate filing. The
        // score editor handles either live or history IDs, so include that
        // snapshot rather than losing the only add notification.
        if (nzbget_rs_dupesearch_is_rank_only_queued_member(
            job.rankOnly ? 1 : 0,
            item == nzbInfo ? 1 : 0,
            (int)item->GetKind(),
            (int)NzbInfo::nkNzb,
            (int)item->GetDupeMode(),
            (int)dmScore,
            item->GetPostInfo() != nullptr ? 1 : 0,
            (int)item->GetDeleteStatus(),
            (int)NzbInfo::dsNone,
            (int)NzbInfo::dsDupe,
            item->GetDupeScore(),
            nzbInfo->GetDupeScore()) != 0 &&
            LowerKey(EffectiveKey(item))==lowerKey &&
            ReleaseName::SameRelease(nzbInfo->GetName(),item->GetName()) &&
            !Util::EmptyStr(item->GetQueuedFilename()) &&
            strchr(item->GetQueuedFilename(), '|') == nullptr)
        {
            job.members.emplace_back(item->GetId(),item->GetQueuedFilename());
            job.memberScores.push_back(item->GetDupeScore());
        }
	}
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb)
		{
			NzbInfo* item = historyInfo->GetNzbInfo();
			addKnown(item);
			// the same release only: a client's key can be broad (one key for two editions);
			// a copy (an nzb-file sent again) too, unless it is a copy of the pick
			bool backup = SavedHistoryBackup(nzbInfo, item);
			if (item != nzbInfo && backup && LowerKey(EffectiveKey(item)) == lowerKey &&
				ReleaseName::SameRelease(nzbInfo->GetName(), item->GetName()) &&
				!Util::EmptyStr(item->GetQueuedFilename()) &&
				strchr(item->GetQueuedFilename(), '|') == nullptr)
			{
				job.members.emplace_back(item->GetId(), item->GetQueuedFilename());
				job.memberScores.push_back(item->GetDupeScore());
			}
		}
	}
}

inline void DupeSearch::Expand(const Job& job, const NzbSummary& pick, const std::vector<Newznab::Result>& prior,
    const std::vector<Posting::Sketch>& known, time_t deadline, NzbFetcher::Budget& budget, std::vector<NzbFetcher::Fetched>& fetched,
    std::set<std::string>& attempted, NzbFetcher::Stats& stats, Newznab::SearchStats& queryStats, int& results, int& candidates, int& postings)
{
    if (IsStopped() || Util::CurrentTime() >= deadline || budget.Spent() || pick.messageIds.empty()) return;
    auto servers=NntpHealthServer::Servers();
    if (servers.empty()) return;
    auto healthOptions = [&]() {
        DonorHealth::Options options;
        options.sample.percent=g_Options->GetDupeHealthPercent();
        options.sample.minimum=g_Options->GetDupeHealthMin();
        options.sample.maximum=g_Options->GetDupeHealthMax();
        options.sample.maxBody=g_Options->GetDupeBodyChecks();
        options.budgetMs=nzbget_rs_dupesearch_calc_expand_budget_ms(
            g_Options->GetDupeHealthBudget(), (int64_t)deadline, (int64_t)Util::CurrentTime());
        return options;
    };
    auto check = [&](const NzbSummary& info) {
        return DonorHealth::CheckPosting(servers,info.messageIds,
            std::make_shared<std::vector<std::string>>(info.groups),healthOptions(),true);
    };
    DonorHealth::Health health;
    double alive=-1;
    bool doomed=false;
    int missing=0;
    if (!job.failed)
    {
        health=check(pick);
        alive=DupeIdentity::Alive(health,pick);
        long long combined=(long long)health.missing+DupeIdentity::UnlistedMissing(health,pick);
        missing=(int)std::min(combined,(long long)std::numeric_limits<int>::max());
        if (alive >= 0.85 && !pick.dataMessageIds.empty() &&
            !pick.parIndex.messageId.empty() && !pick.recoveryBytes.empty())
        {
            // Recovery articles are capacity, never evidence that source data
            // exists. Sample only retained data IDs for damaged-block estimates.
            NzbSummary data = pick;
            data.messageIds = pick.dataMessageIds;
            data.listedParts = pick.dataListedParts;
            data.declaredParts = pick.dataDeclaredParts;
            auto dataHealth = check(data);
            double dataAlive = DupeIdentity::Alive(dataHealth, data);
            long long dataMissing = (long long)dataHealth.missing + DupeIdentity::UnlistedMissing(dataHealth, data);
            if (dataAlive >= 0.0 && dataMissing >= DonorHealth::MinKnown)
                doomed = DupeIdentity::ParDoomed(pick, dataAlive, deadline);
        }
    }
    if (!nzbget_rs_dupe_should_expand(alive,missing,job.failed || doomed,false)) return;
    // Do not spend indexer grabs when a usable whole backup already exists.
    std::set<std::string> probed;
    auto whole=[&](const NzbSummary& info) {
        // A failed pick is not its own backup, even if all of its articles exist.
        // Match the same-posting rejection used later before adding donors.
        if (info.Fingerprint()==pick.Fingerprint() || Posting::SamePosting(pick.messageIds,info.messageIds)) return false;
        if (Util::CurrentTime()>=deadline || IsStopped() || !probed.insert(info.Fingerprint()).second) return false;
        auto h=check(info);
        return DupeIdentity::Alive(h,info)>=0.999;
    };
    for (const auto& posting : fetched)
    {
        // Anything rejected as already held or known-dead later cannot be a
        // newly usable backup. Eligible held backups are checked below.
        Posting::Sketch sketch=Posting::MakeSketch(posting.info.messageIds);
        if (m_dead.IsDead(sketch) || std::any_of(known.begin(),known.end(),
            [&](const Posting::Sketch& k) { return Posting::SameSketch(sketch,k); })) continue;
        if ((!ReleaseName::Readable(posting.info.mainName) || ReleaseName::SameRelease(job.name,posting.info.mainName)) && whole(posting.info)) return;
    }
    auto memberAvailable = [&](int id) {
        GuardedDownloadQueue queue = DownloadQueue::Guard();
        NzbInfo* currentPick = FindNzb(queue, job.nzbId);
        return currentPick && PlacementAvailable(queue, currentPick, FindNzb(queue, id), true);
    };
    for (const auto& member : job.members)
    {
        // Ineligible members must not consume the posting's probe slot. Queue
        // ownership can change during parsing or NNTP, so check it again after.
        if (!memberAvailable(member.first)) continue;
        NzbSummary info;
        if (NzbReader::Parse(DupeUtil::ReadAll(member.second),info) && whole(info))
        {
            if (memberAvailable(member.first)) return;
            probed.erase(info.Fingerprint());
        }
    }
    if (IsStopped() || budget.Spent() || Util::CurrentTime()>=deadline) return;
    std::vector<Newznab::Params> queries;
    auto strict=Newznab::BuildQueries(job.name,pick.meta.count("imdb")?pick.meta.at("imdb"):"",pick.meta.count("tvdb")?pick.meta.at("tvdb"):"");
    struct ExpandQueryCtx
    {
        std::vector<Newznab::Params>& queries;
        const std::vector<Newznab::Params>& strict;
    } expandCtx{queries, strict};
    nzbget_rs_dqh_dupesearch_visit_expand_queries(
        job.name.data(), job.name.size(), &expandCtx,
        [](void* ctx, const char* qPtr, size_t qLen)
        {
            auto* c = static_cast<ExpandQueryCtx*>(ctx);
            std::string q(qPtr, qLen);
            bool duplicate = std::any_of(c->strict.begin(), c->strict.end(), [&](const Newznab::Params& params) {
                return std::any_of(params.begin(), params.end(), [&](const auto& p) { return p.first == "q" && p.second == q; });
            });
            if (!duplicate)
            {
                c->queries.push_back({{"t", "search"}, {"q", q}});
            }
        });
    Newznab::SearchStats searchStats;
    auto wider=Newznab::Search(g_Options->GetDupeSearchUrl(),g_Options->GetDupeSearchApiKey(),queries,deadline,&searchStats);
    queryStats.queries += searchStats.queries;
    queryStats.failed += searchStats.failed;
    queryStats.pages += searchStats.pages;
    std::vector<Newznab::Result> all=prior;
    std::set<std::string> seen;
    for (const auto& r:prior) seen.insert(r.link);
    for (auto& r:wider) if (seen.insert(r.link).second) { results++;all.push_back(std::move(r)); }
    unsigned long long estimate=DupeIdentity::WholeBytes(pick);
    long long bytes=(long long)std::min(estimate,(unsigned long long)std::numeric_limits<long long>::max());
    std::vector<Newznab::Result> accepted;
    for (const auto& r:all)
        if (!attempted.count(r.link) && DupeIdentity::Candidate(job.name,bytes,r.title,r.size)) accepted.push_back(r);
    candidates+=(int)accepted.size();
    auto order=Posting::OrderPostings(accepted,bytes,g_Options->GetDupeSearchMaxDonors());
    postings+=(int)order.size();
    Note(job.nzbId,Message::mkInfo,"DupeSearch: %s: failing pick has no whole backup; broader discovery (%i candidate(s))",job.name.c_str(),(int)accepted.size());
    DupeIdentity::Identity mine;
    bool readMine=false;
    for (const auto& group:order)
    {
        if (IsStopped() || budget.Spent() || Util::CurrentTime()>=deadline) break;
        for (const auto& r:group) attempted.insert(r.link);
        NzbFetcher::Stats counts;
        auto posting=m_fetcher.FetchPosting(group,deadline,counts,&budget);
        stats.Add(counts);
        if (posting.reason!=NzbFetcher::frOk || !DupeIdentity::OuterCompatible(job.name,posting.info.mainName)) continue;
        // Known-release names retain the strict path. Everything else needs inner proof or a fully matching readable data name.
        if (ReleaseName::SameRelease(job.name,posting.listing.title) &&
            (!ReleaseName::Readable(posting.info.mainName) || ReleaseName::SameRelease(job.name,posting.info.mainName)))
        { fetched.push_back(std::move(posting));continue; }
        if (!readMine) { mine=DupeIdentity::Read(pick,deadline);readMine=true; }
        auto theirs=DupeIdentity::Read(posting.info,deadline);
        if (!DupeIdentity::Matches(job.name,posting.info.mainName,mine,theirs)) continue;
        posting.lenient=true;
        posting.sameInner=mine && theirs && mine.bytes==theirs.bytes && mine.extension==theirs.extension;
        fetched.push_back(std::move(posting));
    }
}

inline void DupeSearch::Search(const Job& job)
{
	// search and fetch together are meant to fit in about this long
	time_t deadline = Util::CurrentTime() + SearchDeadlineSec;

	// The pick's ids narrow the search. Infer its whole size when a truncated
    // NZB declares unlisted parts, so complete postings rank before partial relists.
	NzbSummary pick;
	if (!job.queuedFile.empty())
	{
		if (!NzbReader::Parse(DupeUtil::ReadAll(job.queuedFile), pick))
		{
			Note(job.nzbId, Message::mkDetail, "DupeSearch: could not read the nzb-file of %s", job.name.c_str());
		}
	}
    if (job.rankOnly)
    {
        std::vector<NzbFetcher::Fetched> verified;
        std::map<std::string,int> rejected;
        Place(job,pick,verified,rejected,0,0,0);
        return;
    }
    NzbFetcher::Budget budget(g_Options->GetDupeSearchMaxDonors());
    std::set<std::string> attempted;
	std::string imdb = pick.meta.count("imdb") ? pick.meta["imdb"] : "";
	std::string tvdb = pick.meta.count("tvdb") ? pick.meta["tvdb"] : "";

	std::vector<Newznab::Params> queries = Newznab::BuildQueries(job.name, imdb, tvdb);
	Newznab::SearchStats stats;
	std::vector<Newznab::Result> results = Newznab::Search(g_Options->GetDupeSearchUrl(),
		g_Options->GetDupeSearchApiKey(), queries, deadline, &stats);

	// the same release by name; the size isn't a criterion (postings of one
	// release differ by gigabytes in par2 and packaging)
	std::vector<Newznab::Result> candidates;
	for (const Newznab::Result& result : results)
	{
		if (ReleaseName::SameRelease(job.name, result.title))
		{
			candidates.push_back(result);
		}
	}

	Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: %i result(s) from %i search(es) (%i failed, %i page(s)), %i of them the same release",
		job.name.c_str(), (int)results.size(), stats.queries, stats.failed, stats.pages, (int)candidates.size());

	// the postings to fetch, best first: fetching each costs a grab of an indexer
    unsigned long long wholeBytes = DupeIdentity::WholeBytes(pick);
    long long orderBytes = (long long)std::min(wholeBytes,
        (unsigned long long)std::numeric_limits<long long>::max());
	std::vector<Posting::Group> order = Posting::OrderPostings(candidates, orderBytes,
		g_Options->GetDupeSearchMaxDonors());

	// the sketches of what nzbget already holds, read beside the fetches
	std::vector<Posting::Sketch> known;
	std::thread knownReader([&]()
		{
			for (const std::string& path : job.knownFiles)
			{
				Posting::Sketch sketch;
				if (Posting::SketchOfFile(path, sketch))
				{
					known.push_back(std::move(sketch));
				}
			}
		});

	NzbFetcher::Stats fetchStats;
	std::vector<NzbFetcher::Fetched> fetched;
	int notTried = 0;
	size_t parallel = g_Options->GetDupeSearchMaxDonors() > 0 ? 1 : (size_t)FetchParallel;
    for (size_t from = 0; from < order.size(); from += parallel)
	{
		if (IsStopped() || Util::CurrentTime() >= deadline || budget.Spent())
		{
			notTried = (int)(order.size() - from);
			break;
		}

		size_t count = std::min(parallel, order.size() - from);
		std::vector<NzbFetcher::Fetched> chunk(count);
		std::vector<NzbFetcher::Stats> chunkStats(count);
		std::vector<std::thread> threads;
		for (size_t i = 0; i < count; i++)
		{
            for (const Newznab::Result& listing : order[from+i]) attempted.insert(listing.link);
			threads.emplace_back([&, i]()
				{ chunk[i] = m_fetcher.FetchPosting(order[from + i], deadline, chunkStats[i], &budget); });
		}
		for (std::thread& thread : threads)
		{
			thread.join();
		}
		for (size_t i = 0; i < count; i++)
		{
			fetchStats.Add(chunkStats[i]);
			if (chunk[i].reason == NzbFetcher::frOk)
			{
				fetched.push_back(std::move(chunk[i]));
			}
		}
	}

	knownReader.join();
    int resultCount=(int)results.size(), candidateCount=(int)candidates.size(), postingCount=(int)order.size();
    Expand(job,pick,results,known,deadline,budget,fetched,attempted,fetchStats,stats,resultCount,candidateCount,postingCount);

	// what is worth keeping, in this order: another release (by the name of
	// its largest data file, when that name is readable), a posting nzbget
	// already holds, one found dead before, one that shares articles with
	// the pick or an accepted duplicate
	std::vector<std::vector<std::string>> postings;
	postings.push_back(pick.messageIds);
	std::vector<NzbFetcher::Fetched> verified;
	std::map<std::string, int> rejected;
	for (NzbFetcher::Fetched& posting : fetched)
	{
		Posting::Sketch sketch = Posting::MakeSketch(posting.info.messageIds);
		const std::vector<std::string>& ids = posting.info.messageIds;
		bool mainReadable = !posting.lenient && ReleaseName::Readable(posting.info.mainName);
		bool sameRel = mainReadable && ReleaseName::SameRelease(job.name, posting.info.mainName);
		if (mainReadable && !sameRel)
		{
			rejected["other-release"]++;
		}
		else if (std::any_of(known.begin(), known.end(), [&](const Posting::Sketch& k) { return Posting::SameSketch(sketch, k); }))
		{
			rejected["in-nzbget"]++;
		}
		else if (m_dead.IsDead(sketch))
		{
			rejected["known-dead"]++;
		}
		else if (std::any_of(postings.begin(), postings.end(), [&](const std::vector<std::string>& p) { return Posting::SamePosting(ids, p); }))
		{
			rejected["same-posting"]++;
		}
		else
		{
			postings.push_back(ids);
			verified.push_back(std::move(posting));
		}
	}
	rejected["listing-mismatch"] += fetchStats.listingMismatch;
	rejected["refused"] += fetchStats.refused;
	rejected["relisted"] += fetchStats.relisted;
	rejected["fetch"] += fetchStats.fetch;
	rejected["parse"] += fetchStats.parse;
	rejected["deadline"] += notTried;
    rejected["budget"] += fetchStats.budget;

	// a search that learned nothing - a query failed and nothing came back, or no
	// posting could be fetched (indexers refusing, the deadline) - doesn't hold the
	// key for SearchWindowSec: the next add of the key, or a restart, searches it again
	int retryReason = nzbget_rs_dupesearch_classify_search_retry_reason(
		stats.queries, stats.failed, verified.empty() ? 1 : 0, fetched.empty() ? 1 : 0,
		fetchStats.refused, fetchStats.fetch, notTried);
	if (retryReason > 0)
	{
		ForgetSearch(job.dupeKey, ReleaseName::Normalize(job.name));
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: %s, the key may be searched again", job.name.c_str(),
			retryReason == 1 ? "no indexer answered" : retryReason == 2 ? "an indexer query failed and nothing came back" :
			"no posting could be fetched");
	}

	// what was fetched is kept until it is placed: a restart resumes from it
	// without fetching again (every fetch costs a grab)
	SavePending(job, pick, verified);
	Place(job, pick, verified, rejected, resultCount, candidateCount, postingCount);
}

inline void DupeSearch::Place(const Job& job, const NzbSummary& pick, std::vector<NzbFetcher::Fetched>& verified,
	std::map<std::string, int>& rejected, int results, int candidates, int postings)
{
	// closest size first: the likeliest byte-identical repost; then the most grabbed
	long long pickBytes = (long long)std::min(DupeIdentity::WholeBytes(pick),
        (unsigned long long)std::numeric_limits<long long>::max());
	if (verified.size() > 1)
	{
		std::vector<int64_t> totalBytes(verified.size());
		std::vector<int> grabs(verified.size());
		std::vector<int64_t> dates(verified.size());
		std::vector<size_t> orderIdx(verified.size());
		for (size_t i = 0; i < verified.size(); i++)
		{
			totalBytes[i] = verified[i].info.totalBytes;
			grabs[i] = verified[i].listing.grabs;
			dates[i] = (int64_t)verified[i].listing.date;
		}
		nzbget_rs_df_sort_verified_postings(
			totalBytes.data(), grabs.data(), dates.data(), verified.size(), pickBytes, orderIdx.data());
		std::vector<NzbFetcher::Fetched> sorted;
		sorted.reserve(verified.size());
		for (size_t idx : orderIdx)
		{
			sorted.push_back(std::move(verified[idx]));
		}
		verified = std::move(sorted);
	}

	DonorHealth::ServerList servers;
	if (!verified.empty() || !job.members.empty())
	{
		servers = NntpHealthServer::Servers();
		if (servers.empty())
		{
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: no active news servers, the postings are added unchecked", job.name.c_str());
		}
	}
	DonorHealth::Options healthOptions;
	healthOptions.sample.percent = g_Options->GetDupeHealthPercent();
	healthOptions.sample.minimum = g_Options->GetDupeHealthMin();
	healthOptions.sample.maximum = g_Options->GetDupeHealthMax();
	healthOptions.sample.maxBody = g_Options->GetDupeBodyChecks();
	healthOptions.budgetMs = g_Options->GetDupeHealthBudget() * 1000;

	auto groupsOf = [](const NzbSummary& summary)
		{ return std::make_shared<std::vector<std::string>>(summary.groups); };

	// the pick's own posting: found dead, it is remembered (the dead-pick probe of
	// the queue is what swaps it out)
	std::thread pickCheck;
	if (!servers.empty() && !pick.messageIds.empty())
	{
		pickCheck = std::thread([&]()
			{
				DonorHealth::Health health = DonorHealth::CheckPosting(servers, pick.messageIds,
					groupsOf(pick), healthOptions, false);
				if (DonorHealth::DeadProbe(health))
				{
					m_dead.Add(Posting::MakeSketch(pick.messageIds), 0, !g_Options->GetDupeSearchDryRun());
					Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: the pick is dead (%i of %i probe articles on no server)",
						job.name.c_str(), health.missing, health.Answered());
				}
			});
	}

	int dead = 0;
	int overCap = 0;
	int rescoreFailed = 0;
	int added = 0;
	int cap = g_Options->GetDupeSearchMaxDonors();
	std::mutex placeMutex;
	std::map<size_t, DonorHealth::Health> probe;

	// the quick probe of every posting
	if (!servers.empty())
	{
		std::vector<DonorHealth::Posting> list;
		for (size_t i = 0; i < verified.size(); i++)
		{
			NzbgetRsBuf keyBuf = nzbget_rs_dqh_dupesearch_format_usize_str(i);
			std::string keyStr(keyBuf.data ? keyBuf.data : "", keyBuf.len);
			nzbget_rs_free(keyBuf);
			list.push_back({ std::move(keyStr), verified[i].info.messageIds, groupsOf(verified[i].info) });
		}
		DonorHealth::CheckPostings(servers, list, healthOptions, false, FetchParallel,
			[&](const std::string& key, const DonorHealth::Health& health)
			{
				std::lock_guard<std::mutex> guard(placeMutex);
				probe[(size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str())] = health;
			});
	}

	std::vector<size_t> live;
	for (size_t i = 0; i < verified.size(); i++)
	{
		auto it = probe.find(i);
		if (it != probe.end() && DonorHealth::DeadProbe(it->second))
		{
			m_dead.Add(Posting::MakeSketch(verified[i].info.messageIds), 0, !g_Options->GetDupeSearchDryRun());
			dead++;
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: dropping dead posting %s [%s] after the probe: %i of %i probe articles on no server",
				job.name.c_str(), verified[i].listing.title.c_str(), verified[i].listing.indexer.c_str(),
				it->second.missing, it->second.Answered());
			continue;
		}
		live.push_back(i);
	}

	struct Placed
	{
		int id;
		DonorScore::Entry entry;
		std::string title;
		bool member = false;	// a duplicate that was already in history: always rescored
        bool valid = true; // Final score/parameter update must still succeed.
		int oldScore = 0;		// a member's score before: never raised unless it is whole
	};
	std::map<size_t, Placed> placed;
    std::map<size_t, double> deferred;
    auto slotsUsed = [&]() {
        GuardedDownloadQueue queue = DownloadQueue::Guard();
        NzbInfo* currentPick = FindNzb(queue, job.nzbId);
        return (int)std::count_if(placed.begin(), placed.end(), [&](const auto& item) {
            bool avail = !item.second.member && item.second.valid &&
                item.second.entry.score > DonorScore::Dead && item.second.id >= 0 &&
                PlacementAvailable(queue, currentPick, FindNzb(queue, item.second.id));
            return nzbget_rs_dupesearch_slot_counts_as_used(
                item.second.member ? 1 : 0, item.second.valid ? 1 : 0,
                item.second.entry.score, DonorScore::Dead, item.second.id,
                g_Options->GetDupeSearchDryRun() ? 1 : 0, avail ? 1 : 0) != 0;
        });
    };
	DonorScore::Ranks ranks;
	int base = DonorScore::Base(job.score);

	auto twinOf = [&](const NzbFetcher::Fetched& f)
		{ return DupeIdentity::SameData(pick, f.info); };
	auto aliveOf = [&](const DonorHealth::Health& health, const NzbSummary& summary) { return DupeIdentity::Alive(health, summary); };
    auto cacheableLoss = [&](DonorHealth::Health health) {
        // Only confirmed article loss may invalidate overlapping postings.
        // Structural gaps and provider errors belong to this NZB's score.
        health.error=0;
        return DupeIdentity::IsDead(health,NzbSummary{},g_Options->GetDupeMinAlive());
    };
	auto entryOf = [&](size_t i, int score, double alive)
		{
			DonorScore::Entry entry;
			entry.score = score;
			entry.alive = alive;
			entry.twin = twinOf(verified[i]);
			entry.bytes = verified[i].info.totalBytes;
			entry.grabs = verified[i].listing.grabs;
			return entry;
		};
	// nzbget shuts down: a check cut short knows nothing, and adding a duplicate now
	// would wait for the stopped scanner for good (B37). The search keeps its saved
	// postings and resumes after the restart.
	auto interrupted = [&]()
		{
			if (!IsStopped())
			{
				return false;
			}
			if (pickCheck.joinable())
			{
				pickCheck.join();
			}
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: RustyGet shuts down, the search resumes after the restart",
				job.name.c_str());
			return true;
		};
	auto place = [&](size_t i, double alive, const char* how)
		{
			int score = ranks.Take(alive, twinOf(verified[i]));
			int id = AddDonor(job, pick, verified[i], base + score, alive);
			if (id == 0)
			{
				ranks.Release(score);
				return;
			}
			added++;
			placed[i] = { id, entryOf(i, score, alive), verified[i].listing.title };
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: added %s [%s] score=%i alive=%i%% (%s)", job.name.c_str(),
				verified[i].listing.title.c_str(), verified[i].listing.indexer.c_str(), base + score,
				nzbget_rs_dupesearch_alive_to_percent(alive), how);
		};

	// the probe-alive postings go in at once, then each is rescored when its
	// full sample lands; the rest are added as their samples finish
	size_t fast = nzbget_rs_dupesearch_calc_fast_donor_count(
		g_Options->GetDupeFastDonors(), live.size(), cap);
	std::set<size_t> fastSet;
	for (size_t n = 0; n < fast; n++)
	{
		auto it = probe.find(live[n]);
		place(live[n], it == probe.end() ? -1.0 : aliveOf(it->second, verified[live[n]].info), "fast");
		fastSet.insert(live[n]);
	}

	std::set<size_t> todo(live.begin(), live.end());
	if (!servers.empty() && !todo.empty())
	{
		std::vector<DonorHealth::Posting> list;
		for (size_t i : todo)
		{
			NzbgetRsBuf keyBuf = nzbget_rs_dqh_dupesearch_format_usize_str(i);
			std::string keyStr(keyBuf.data ? keyBuf.data : "", keyBuf.len);
			nzbget_rs_free(keyBuf);
			list.push_back({ std::move(keyStr), verified[i].info.messageIds, groupsOf(verified[i].info) });
		}
		DonorHealth::CheckPostings(servers, list, healthOptions, true, FetchParallel,
			[&](const std::string& key, const DonorHealth::Health& health)
			{
				std::lock_guard<std::mutex> guard(placeMutex);
				size_t i = (size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str());
				if (IsStopped() || !todo.erase(i))
				{
					return;
				}
				double alive = aliveOf(health, verified[i].info);
				bool isDead = DupeIdentity::IsDead(health, verified[i].info, g_Options->GetDupeMinAlive());
				Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: %s [%s]: alive=%i%% (%i of %i articles)", job.name.c_str(),
					verified[i].listing.title.c_str(), verified[i].listing.indexer.c_str(),
					nzbget_rs_dupesearch_alive_to_percent(alive), health.present, health.Answered());

				auto it = placed.find(i);
				if (it != placed.end())
				{
					// a fast donor: its real health moves its score
					Placed& donor = it->second;
					ranks.Release(donor.entry.score);
					int score = isDead ? DonorScore::Dead : ranks.Take(alive, donor.entry.twin);
					NzbgetRsBuf paramBuf = nzbget_rs_dqh_dupesearch_format_key_val_param(
						AliveParam,
						nzbget_rs_dupesearch_fast_donor_alive_param(isDead ? 1 : 0, alive));
					std::string param(paramBuf.data ? paramBuf.data : "", paramBuf.len);
					nzbget_rs_free(paramBuf);
					if (alive < 0 || SetScore(donor.id, base + score,
						param.empty() ? std::vector<std::string>() : std::vector<std::string>{ param }))
					{
						if (alive < 0)
						{
							// inconclusive: the donor keeps its score, and the rank taken for
							// the new one is given back (F19: it stayed taken, the old one free)
							ranks.Release(score);
							ranks.Use(donor.entry.score);
						}
						else
						{
							donor.entry.score = score;
						}
						donor.entry.alive = alive;
						if (isDead)
						{
							if (cacheableLoss(health)) m_dead.Add(Posting::MakeSketch(verified[i].info.messageIds), 0, !g_Options->GetDupeSearchDryRun());
							dead++;
						}
					}
					else
					{
						ranks.Release(score);
						ranks.Use(donor.entry.score);
						rescoreFailed++;
                        donor.valid = false;
						Note(job.nzbId, Message::mkWarning, "DupeSearch: could not rescore %s: keeps score %i", verified[i].listing.title.c_str(),
							base + donor.entry.score);
					}
				}
				else if (isDead)
				{
					if (cacheableLoss(health)) m_dead.Add(Posting::MakeSketch(verified[i].info.messageIds), 0, !g_Options->GetDupeSearchDryRun());
					dead++;
					Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: dropping dead posting %s: alive=%i%%", job.name.c_str(),
						verified[i].listing.title.c_str(), (int)std::lround(100 * alive));
				}
				else
				{
                    // A healthy answer may arrive before a fast donor's dead
                    // answer. Retain it until all checks settle the cap slots.
                    if (cap > 0 && slotsUsed() >= cap) deferred[i] = alive;
                    else place(i, alive, "checked");
				}
			});
	}

	if (interrupted())
	{
		return;
	}

    for (auto it = deferred.begin(); it != deferred.end();)
    {
        if (cap > 0 && slotsUsed() >= cap) { ++it; continue; }
        auto entry = *it;
        it = deferred.erase(it);
        place(entry.first, entry.second, "checked");
    }

	// whatever no check reported on (no servers, a failed check) is added unchecked
	for (size_t i : std::set<size_t>(todo))
	{
		todo.erase(i);
		if (placed.count(i))
		{
			continue;
		}
		if (cap > 0 && slotsUsed() >= cap)
		{
			overCap++;
			continue;
		}
		place(i, -1.0, "unchecked");
	}

	// the duplicates nzbget already kept for the key (a client's own backups,
	// earlier searches' donors) are measured the same way and ranked with the
	// new ones: nzbget tries them by score, whoever sent them. Each is read
	// from its own nzb-file, never matched by size: two postings can have one size.
	std::set<size_t> uncheckedMembers;
	if (!job.members.empty() && (!servers.empty() || job.rankOnly))
	{
		std::vector<NzbSummary> summaries(job.members.size());
		std::vector<DonorHealth::Posting> list;
		std::map<size_t, size_t> sameAs;	// a member whose posting another member already is
		for (size_t m = 0; m < job.members.size(); m++)
		{
			uncheckedMembers.insert(m);
			if (!NzbReader::Parse(DupeUtil::ReadAll(job.members[m].second), summaries[m]))
			{
				continue;
			}
            if (Posting::SamePosting(summaries[m].messageIds, pick.messageIds))
            {
                uncheckedMembers.erase(m);
                continue;
            }
			// each posting is checked once: the client's copies of it share the answer
			auto first = std::find_if(list.begin(), list.end(), [&](const DonorHealth::Posting& posting)
				{ return Posting::SamePosting(summaries[m].messageIds, posting.ids); });
			if (first != list.end())
			{
				sameAs[m] = (size_t)nzbget_rs_dqh_parse_c_atoi(first->key.c_str());
				continue;
			}
			NzbgetRsBuf keyBuf = nzbget_rs_dqh_dupesearch_format_usize_str(m);
			std::string keyStr(keyBuf.data ? keyBuf.data : "", keyBuf.len);
			nzbget_rs_free(keyBuf);
			list.push_back({ std::move(keyStr), summaries[m].messageIds, groupsOf(summaries[m]) });
		}
		std::map<size_t, DonorHealth::Health> measured;
		if (!servers.empty()) DonorHealth::CheckPostings(servers, list, healthOptions, true, FetchParallel,
			[&](const std::string& key, const DonorHealth::Health& health)
			{
				std::lock_guard<std::mutex> guard(placeMutex);
				measured[(size_t)nzbget_rs_dqh_parse_c_atoi(key.c_str())] = health;
			});
		for (const auto& alias : sameAs)
		{
			auto health = measured.find(alias.second);
			if (health != measured.end())
			{
				measured[alias.first] = health->second;
			}
		}
		for (const auto& entry : measured)
		{
			const NzbSummary& summary = summaries[entry.first];
			double alive = aliveOf(entry.second, summary);
			if (alive < 0)
			{
				continue;	// too few answers to judge: its score stays
			}
			bool isDead = DupeIdentity::IsDead(entry.second, summary, g_Options->GetDupeMinAlive());
			if (isDead)
			{
				if (cacheableLoss(entry.second)) m_dead.Add(Posting::MakeSketch(summary.messageIds), 0, !g_Options->GetDupeSearchDryRun());
			}
			Placed member;
			member.id = job.members[entry.first].first;
			member.title = FileSystem::BaseFileName(job.members[entry.first].second.c_str());
			member.member = true;
			member.oldScore = entry.first < job.memberScores.size() ? job.memberScores[entry.first] : 0;
			member.entry.score = isDead ? DonorScore::Dead : DonorScore::Dead + 1;
			member.entry.alive = alive;
			member.entry.twin = DupeIdentity::SameData(pick, summary);
			member.entry.bytes = summary.totalBytes;
			placed[verified.size() + entry.first] = member;
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: duplicate %s in history: alive=%i%% (%i of %i articles)%s", job.name.c_str(),
				member.title.c_str(), (int)std::lround(100 * alive), entry.second.present, entry.second.Answered(),
				isDead ? ", dead" : "");
		}
	}

	if (interrupted())
	{
		return;
	}

    auto rerank = [&]() {
	// once every check is in, the scores in order of wholeness
	std::vector<size_t> order2;
	std::vector<DonorScore::Entry> entries;
	for (auto& entry : placed)
	{
        if (!entry.second.valid) continue;
		order2.push_back(entry.first);
		entries.push_back(entry.second.entry);
	}
	std::vector<int> wanted = DonorScore::Rerank(entries, pickBytes);
	// with every one of them dead there is nothing to order: scores stay, the dead are marked
	bool allDead = std::all_of(wanted.begin(), wanted.end(), [](int score) { return score == DonorScore::Dead; });
	for (size_t n = 0; n < order2.size(); n++)
	{
		Placed& donor = placed[order2[n]];
		if (wanted[n] == donor.entry.score && !donor.member)
		{
			continue;
		}
		std::vector<std::string> params;
		int score = base + wanted[n];
		if (donor.member)
		{
			int percent = 0;
			int addAliveZero = 0;
			nzbget_rs_df_eval_member_rerank(
				base, wanted[n], DonorScore::Dead, donor.oldScore, donor.entry.alive, allDead ? 1 : 0,
				&score, &percent, &addAliveZero);
			NzbgetRsBuf hpBuf = nzbget_rs_dqh_dupesearch_format_key_val_param(HealthParam, percent);
			params.emplace_back(hpBuf.data ? hpBuf.data : "", hpBuf.len);
			nzbget_rs_free(hpBuf);
			if (addAliveZero)
			{
				params.push_back(std::string(AliveParam) + "=0");
			}
		}
		if (SetScore(donor.id, score, params))
		{
            if (donor.member) uncheckedMembers.erase(order2[n] - verified.size());
			Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: reranked %s: score %i", job.name.c_str(), donor.title.c_str(), score);
			donor.entry.score = wanted[n];
		}
		else
		{
            donor.valid = false;
			rescoreFailed++;
		}
	}

    };
    rerank();
    // A final failed score edit can invalidate another occupied slot. Keep
    // measured candidates until that decision and attempt each at most once.
    while (!deferred.empty() && (cap <= 0 || slotsUsed() < cap))
    {
        auto entry = *deferred.begin();
        deferred.erase(deferred.begin());
        place(entry.first, entry.second, "checked");
        rerank();
    }
    overCap += (int)deferred.size();

	if (pickCheck.joinable())
	{
		pickCheck.join();
	}

	rejected["dead"] += dead;
	rejected["over-cap"] += overCap;
	rejected["rescore"] += rescoreFailed;
	std::vector<const char*> rejKeys;
	std::vector<int> rejCounts;
	rejKeys.reserve(rejected.size());
	rejCounts.reserve(rejected.size());
	for (const auto& entry : rejected)
	{
		rejKeys.push_back(entry.first.c_str());
		rejCounts.push_back(entry.second);
	}
	NzbgetRsBuf outcomeBuf = nzbget_rs_dqh_dupesearch_format_rejected_outcome(
		rejKeys.data(), rejCounts.data(), rejected.size());
	std::string outcome(outcomeBuf.data ? outcomeBuf.data : "", outcomeBuf.len);
	nzbget_rs_free(outcomeBuf);
	Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: results=%i candidates=%i postings=%i verified=%i added=%i rejected={%s}",
		job.name.c_str(), results, candidates, postings, (int)verified.size(), added, outcome.c_str());
	RemovePending(job.nzbId);
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		m_sent.erase(job.nzbId);
	}
    // Without a usable whole backup discovery remains retryable while the
    // pick is queued, even if a fast-added donor was later declared dead.
    bool usableBackup = false;
    {
        GuardedDownloadQueue queue = DownloadQueue::Guard();
        NzbInfo* currentPick = FindNzb(queue, job.nzbId);
        usableBackup=std::any_of(placed.begin(),placed.end(),[&](const auto& item) {
            bool avail = item.second.id >= 0 && item.second.valid &&
                item.second.entry.score > DonorScore::Dead && item.second.entry.alive >= 0.999 &&
                PlacementAvailable(queue, currentPick, FindNzb(queue, item.second.id), true);
            return nzbget_rs_dupesearch_is_usable_whole_backup(
                item.second.valid ? 1 : 0, item.second.entry.score, DonorScore::Dead,
                item.second.entry.alive, item.second.id,
                g_Options->GetDupeSearchDryRun() ? 1 : 0, avail ? 1 : 0) != 0;
        });
    }
    bool stateRetryable = false;
    if (job.rankOnly)
    {
        std::lock_guard<std::mutex> guard(m_mutex);
        Searched* state = FindSearch(LowerKey(job.dupeKey), ReleaseName::Normalize(job.name));
        stateRetryable = state && state->retryable;
    }
    if (usableBackup)
    {
        { std::lock_guard<std::mutex> guard(m_mutex);
          SetRetryable(LowerKey(job.dupeKey), ReleaseName::Normalize(job.name), false); }
        if (!g_Options->GetDupeSearchDryRun()) SaveState();
    }
    int retryDiscoveryInt = 0;
    int rankIncompleteInt = 0;
    nzbget_rs_dupesearch_eval_post_place_retry(
        job.rankOnly ? 1 : 0, stateRetryable ? 1 : 0, usableBackup ? 1 : 0,
        uncheckedMembers.size(), &retryDiscoveryInt, &rankIncompleteInt);
    bool rankIncomplete = rankIncompleteInt != 0;
    bool retryDiscovery = retryDiscoveryInt != 0;
    // A rename/deletion may make this pick ineligible for scheduling, but must
    // not falsely complete the original title's unfulfilled discovery stamp.
    if (retryDiscovery && !IsStopped()) ForgetSearch(job.dupeKey, ReleaseName::Normalize(job.name));
    if ((retryDiscovery || rankIncomplete) && !IsStopped())
    {
        GuardedDownloadQueue queue=DownloadQueue::Guard();
        NzbInfo* current=queue->GetQueue()->Find(job.nzbId);
        if (current && !current->GetDeleting() && current->GetDeleteStatus() == NzbInfo::dsNone &&
            !current->GetPostInfo() &&
            LowerKey(EffectiveKey(current))==LowerKey(job.dupeKey) &&
            ReleaseName::Normalize(current->GetName())==ReleaseName::Normalize(job.name))
        {
            if (retryDiscovery)
            {
                // An owed discovery retry takes priority over ranking an
                // inconclusive backup; otherwise rank-only retries can starve it.
                { std::lock_guard<std::mutex> guard(m_mutex);
                  m_rankPending.erase(job.nzbId); }
            }
            else
            {
                // Retain the completed search stamp: only recheck existing
                // backups, without repeating indexer searches or paid grabs.
                std::lock_guard<std::mutex> guard(m_mutex);
                m_rankPending.insert(job.nzbId);
            }
            if (!retryDiscovery) MarkSearching(job);
            Schedule(job.nzbId,Util::CurrentTime()+900);
            if (!retryDiscovery)
                current->PrintMessage(Message::mkInfo, "DupeSearch: %s: retrying backup health in 15 minutes", job.name.c_str());
        }
    }
}

inline std::string DupeSearch::PickGone(const Job& job)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	// in history after a download or a failure it still wants its duplicates;
	// deleted by the user it doesn't: the first duplicate would download
	NzbInfo* pick = FindNzb(downloadQueue, job.nzbId);
	if (!pick || pick->GetDeleting() || pick->GetDeleteStatus() == NzbInfo::dsManual)
	{
		return "the pick was deleted";
	}
	std::string pickEffKey = EffectiveKey(pick);
	std::string pickNormName = ReleaseName::Normalize(pick->GetName());
	std::string jobNormName = ReleaseName::Normalize(job.name);
	int reason = nzbget_rs_df_eval_pick_gone(
		1, 0, (int)pick->GetDeleteStatus(), (int)NzbInfo::dsManual,
		pickEffKey.c_str(), job.dupeKey.c_str(), pickNormName.c_str(), jobNormName.c_str());
	switch (reason)
	{
		case 1: return "the pick was deleted";
		case 2: return "the duplicate key of the pick changed";
		case 3: return "the name of the pick changed";
		default: return "";
	}
}

inline int DupeSearch::AddDonor(const Job& job, const NzbSummary& pick, const NzbFetcher::Fetched& posting, int score, double alive)
{
	if (IsStopped())
	{
		return 0;
	}
	std::string gone = PickGone(job);
	if (!gone.empty())
	{
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: not adding %s [%s]: %s", job.name.c_str(),
			posting.listing.title.c_str(), posting.listing.indexer.c_str(), gone.c_str());
		return 0;
	}
	if (g_Options->GetDupeSearchDryRun())
	{
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: dry run, would add %s [%s] score=%i alive=%i%%", job.name.c_str(),
			posting.listing.title.c_str(), posting.listing.indexer.c_str(), score,
			nzbget_rs_dupesearch_alive_to_percent(alive));
		return -1;
	}
	std::string fingerprint = posting.info.Fingerprint();
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		if (!m_sent[job.nzbId].insert(fingerprint).second)
		{
			return 0;
		}
	}

	NzbParameterList parameters;
	parameters.SetParameter(DonorParam, "yes");
	for (const std::string& hint : DupeIdentity::Hints(pick, posting.info, posting.sameInner))
	{
		size_t nameLen = 0, valOff = 0;
		if (nzbget_rs_tpr_split_param_assignment(hint.c_str(), &nameLen, &valOff) != 0)
		{
			parameters.SetParameter(hint.substr(0, nameLen).c_str(), hint.c_str() + valOff);
		}
	}
	if (alive >= 0)
	{
		NzbgetRsBuf aliveBuf = nzbget_rs_dqh_conn_format_port_str(
			nzbget_rs_dupesearch_alive_to_percent(alive));
		parameters.SetParameter(AliveParam, aliveBuf.data ? aliveBuf.data : "0");
		nzbget_rs_free(aliveBuf);
	}
	std::string name = posting.listing.title;
	if (nzbget_rs_dupesearch_needs_nzb_ext(name.data(), name.size()) != 0)
	{
		name += ".nzb";
	}

	int nzbId = 0;
	Scanner::EAddStatus status = g_Scanner->AddExternalFile(name.c_str(), job.category.c_str(), false, 0,
		job.dupeKey.c_str(), score, dmScore, &parameters, false, false, nullptr, nullptr,
		posting.data.data(), (int)posting.data.size(), &nzbId);
	if (status != Scanner::asSuccess || nzbId == 0)
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		m_sent[job.nzbId].erase(fingerprint);
		return 0;
	}

	// the pick may have been deleted, or have got another key, while the add ran
	// (it takes the queue lock itself, after PickGone let it go): the new duplicate
	// goes again (B33)
	gone = PickGone(job);
	if (!gone.empty())
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			IdList ids{ nzbId };
			bool queued = downloadQueue->GetQueue()->Find(nzbId) != nullptr;
			downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId,
				queued ? DownloadQueue::eaGroupFinalDelete : DownloadQueue::eaHistoryFinalDelete, nullptr);
		}
		Note(job.nzbId, Message::mkInfo, "DupeSearch: %s: removed the duplicate %s just added: %s",
			job.name.c_str(), posting.listing.title.c_str(), gone.c_str());
		return 0;
	}

	// remembered with the search in progress: a resume after a restart must not add
	// it again, even when the user deleted it meanwhile (then nzbget keeps only a
	// hidden duplicate record, which the resume can't recognize)
	std::string sentPath = PendingDir(job.nzbId) + PATH_SEPARATOR + "sent";
	if (FileSystem::DirectoryExists(PendingDir(job.nzbId).c_str()))
	{
		std::ofstream sent(fs::u8path(sentPath), std::ios::app);
		sent << fingerprint << "\n";
	}
	return nzbId;
}

inline bool DupeSearch::SetScore(int id, int score, const std::vector<std::string>& params)
{
	if (g_Options->GetDupeSearchDryRun() || id < 0)
	{
		std::string text;
		for (const std::string& param : params)
		{
			text += " " + param;
		}
		info("DupeSearch: dry run, would set the score of download %i to %i%s", id, score, text.c_str());
		return true;
	}
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	// a duplicate the user deleted is kept as a hidden record (hkDup): its score
	// edit succeeds but means nothing, so it counts as gone
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetId() == id && historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			return false;
		}
	}
	IdList ids{ id };
	NzbgetRsBuf scoreBuf = nzbget_rs_dqh_conn_format_port_str(score);
	std::string text(scoreBuf.data ? scoreBuf.data : "", scoreBuf.len);
	nzbget_rs_free(scoreBuf);
	const std::pair<DownloadQueue::EEditAction, DownloadQueue::EEditAction> kinds[] = {
		{ DownloadQueue::eaHistorySetDupeScore, DownloadQueue::eaHistorySetParameter },
		{ DownloadQueue::eaGroupSetDupeScore, DownloadQueue::eaGroupSetParameter } };
	for (const auto& kind : kinds)
	{
		if (downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId, kind.first, text.c_str()))
		{
			for (const std::string& param : params)
			{
				downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId, kind.second, param.c_str());
			}
			return true;
		}
	}
	return false;
}

inline void DupeSearch::Note(int nzbId, Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list ap;
	va_start(ap, format);
	vsnprintf(text, sizeof(text), format, ap);
	va_end(ap);

	// into the pick's own log as well (with InfoTarget=log and no log file,
	// the global log keeps no info lines); the global log alone once it's gone
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = FindNzb(downloadQueue, nzbId);
	if (nzbInfo)
	{
		nzbInfo->AddMessage(kind, text);
		return;
	}
	switch (kind)
	{
		case Message::mkWarning: warn("%s", text); break;
		case Message::mkDetail: detail("%s", text); break;
		default: info("%s", text); break;
	}
}

inline std::string DupeSearch::PendingDir(int nzbId)
{
	NzbgetRsBuf dirBuf = nzbget_rs_dqh_dupesearch_format_pending_dir(
		g_Options->GetQueueDir(), PATH_SEPARATOR, nzbId);
	std::string dir(dirBuf.data ? dirBuf.data : "", dirBuf.len);
	nzbget_rs_free(dirBuf);
	return dir;
}

namespace
{

void RemoveDir(const std::string& dir)
{
	CString errmsg;
	FileSystem::DeleteDirectoryWithContent(dir.c_str(), errmsg);
}


}

inline void DupeSearch::MarkSearching(const Job& job)
{
	if (g_Options->GetDupeSearchDryRun())
	{
		return;
	}
	std::string dir = PendingDir(job.nzbId);
	RemoveDir(dir);
	CString errmsg;
	if (!FileSystem::ForceDirectories(dir.c_str(), errmsg) ||
        !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-title", ReleaseName::Normalize(job.name)) ||
        !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-key", LowerKey(job.dupeKey)) ||
        !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "phase", job.rankOnly ? "ranking\n" : "searching\n"))
	{
		warn("Could not save the DupeSearch state to %s", dir.c_str());
	}
}

inline void DupeSearch::SavePending(const Job& job, const NzbSummary& pick, const std::vector<NzbFetcher::Fetched>& verified)
{
	if (g_Options->GetDupeSearchDryRun())
	{
		return;
	}
	std::string dir = PendingDir(job.nzbId);
    bool ok = DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-title", ReleaseName::Normalize(job.name)) &&
        DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-key", LowerKey(job.dupeKey)) &&
        DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "pick-posting", pick.Fingerprint());
	for (size_t i = 0; i < verified.size() && ok; i++)
	{
		const Newznab::Result& listing = verified[i].listing;
		NzbgetRsBuf baseBuf = nzbget_rs_dqh_dupesearch_format_indexed_base_path(
			dir.c_str(), PATH_SEPARATOR, i);
		std::string base(baseBuf.data ? baseBuf.data : "", baseBuf.len);
		nzbget_rs_free(baseBuf);
		NzbgetRsBuf metaBuf = nzbget_rs_df_format_pending_meta(
			listing.title.c_str(), listing.indexer.c_str(), listing.grabs, listing.size, (int64_t)listing.date);
		std::string metaStr(metaBuf.data ? metaBuf.data : "", metaBuf.len);
		nzbget_rs_free(metaBuf);
		ok = DupeUtil::WriteAtomic(base + ".nzb", verified[i].data) && DupeUtil::WriteAtomic(base + ".meta", metaStr) &&
            DupeUtil::WriteAtomic(base + ".inner", verified[i].sameInner ? "1\n" : "0\n");
	}
	if (!ok || !DupeUtil::WriteAtomic(dir + PATH_SEPARATOR + "phase", "fetched\n"))
	{
		warn("Could not save the DupeSearch state to %s", dir.c_str());
	}
}

inline void DupeSearch::RemovePending(int nzbId)
{
	if (IsStopped())
	{
		return;	// interrupted: resumed after the restart
	}
	RemoveDir(PendingDir(nzbId));
}

inline void DupeSearch::ResumePending()
{
	std::vector<int> ids;
	DirBrowser dirBrowser(PendingDir(0).c_str());
	while (const char* name = dirBrowser.Next())
	{
		int parsedId = nzbget_rs_dqh_dupesearch_parse_pending_dir_id(name);
		if (parsedId > 0)
		{
			ids.push_back(parsedId);
		}
	}

	for (int nzbId : ids)
	{
		if (IsStopped())
		{
			return;
		}
		std::string dir = PendingDir(nzbId);
        std::string phase = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "phase");
		int fetchedInt = 0;
		int rankingInt = 0;
		nzbget_rs_dupesearch_classify_pending_phase(
			phase.data(), phase.size(), &fetchedInt, &rankingInt);
		bool fetched = fetchedInt != 0;
        bool ranking = rankingInt != 0;

		Job job;
        bool stateErased = false;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = FindNzb(downloadQueue, nzbId);
			if (nzbInfo && fetched)
			{
				Collect(downloadQueue, nzbInfo, job);
				job.score = std::max(nzbInfo->GetDupeScore(), BasePickScore);
			}
			else if (nzbInfo && ranking)
            {
                std::string key = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key");
                std::string title = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title");
                std::lock_guard<std::mutex> guard(m_mutex);
                if (key == LowerKey(EffectiveKey(nzbInfo)) && title == ReleaseName::Normalize(nzbInfo->GetName()))
                    if (Searched* stamp = FindSearch(key, title))
                        if (stamp->title == title) m_rankPending.insert(nzbId);
                m_pending[nzbId] = Util::CurrentTime() + g_Options->GetDupeSearchDelay();
                m_cond.notify_all();
            }
			else if (nzbInfo)
			{
				// the search itself was cut off: it runs again
				std::lock_guard<std::mutex> guard(m_mutex);
                // A rename/key edit during the interrupted search must not
                // invalidate another edition's completed suppression stamp.
                std::string key = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key");
                std::string title = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title");
                if (!key.empty() && !title.empty())
                    stateErased = m_titleSearched.erase({key, title}) != 0;
                // Unknown old checkpoints cannot name an edition to invalidate.
                std::string cacheKey = key.empty() ? LowerKey(EffectiveKey(nzbInfo)) : key;
                auto latest = m_searched.find(cacheKey);
                if (latest != m_searched.end() && (latest->second.title.empty() ||
                    (!key.empty() && !title.empty() && latest->second.title == title)))
                { m_searched.erase(latest); stateErased = true; }
			}
		}
        if (stateErased && !g_Options->GetDupeSearchDryRun() && !SaveState()) continue;
		if (!fetched || job.nzbId == 0)
		{
			RemoveDir(dir);
			continue;
		}

		Note(job.nzbId, Message::mkInfo, "DupeSearch: resuming the search of %s after a restart", job.name.c_str());
		NzbSummary pick;
		NzbReader::Parse(DupeUtil::ReadAll(job.queuedFile), pick);
        // Proof belongs to the original pick. Old state without that binding is
        // searched again rather than assigning its donors to an unknown release.
        bool titleExists = FileSystem::FileExists((dir + PATH_SEPARATOR + "pick-title").c_str());
        std::string savedTitle = titleExists ? DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title") : std::string();
        std::string savedKey = titleExists ? DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key") : std::string();
        std::string savedPosting = titleExists ? DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-posting") : std::string();
        std::string currentTitle = ReleaseName::Normalize(job.name);
        std::string currentKey = LowerKey(job.dupeKey);
        std::string currentPosting = pick.Fingerprint();
        if (nzbget_rs_dqh_dupesearch_pending_identity_matches(
            titleExists ? 1 : 0,
            (const uint8_t*)savedTitle.data(), savedTitle.size(),
            (const uint8_t*)currentTitle.data(), currentTitle.size(),
            (const uint8_t*)savedKey.data(), savedKey.size(),
            (const uint8_t*)currentKey.data(), currentKey.size(),
            (const uint8_t*)savedPosting.data(), savedPosting.size(),
            (const uint8_t*)currentPosting.data(), currentPosting.size()) == 0)
        {
            Note(job.nzbId, Message::mkInfo, "DupeSearch: saved search identity changed; searching the current pick again");
            std::string oldKey = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-key");
            std::string oldTitle = DupeUtil::ReadAll(dir + PATH_SEPARATOR + "pick-title");
            if (!oldKey.empty() && !oldTitle.empty())
            {
                if (!ForgetSearch(oldKey, oldTitle)) continue;
            }
            else
            {
                bool erased = false;
                { std::lock_guard<std::mutex> guard(m_mutex);
                  auto latest = m_searched.find(LowerKey(job.dupeKey));
                  if (latest != m_searched.end() && latest->second.title.empty())
                  { m_searched.erase(latest); erased = true; } }
                if (erased && !g_Options->GetDupeSearchDryRun() && !SaveState()) continue;
            }
            RemoveDir(dir);
            Schedule(job.nzbId, Util::CurrentTime() + g_Options->GetDupeSearchDelay());
            continue;
        }
		std::vector<Posting::Sketch> known;
		for (const std::string& path : job.knownFiles)
		{
			Posting::Sketch sketch;
			if (Posting::SketchOfFile(path, sketch))
			{
				known.push_back(std::move(sketch));
			}
		}

		std::set<std::string> sent;
		{
			std::stringstream lines(DupeUtil::ReadAll(dir + PATH_SEPARATOR + "sent"));
			for (std::string line; std::getline(lines, line);)
			{
				sent.insert(line);
			}
		}

		std::vector<NzbFetcher::Fetched> verified;
		std::map<std::string, int> rejected;
		for (int i = 0;; i++)
		{
			NzbgetRsBuf baseBuf = nzbget_rs_dqh_dupesearch_format_indexed_base_path(
				dir.c_str(), PATH_SEPARATOR, (size_t)i);
			std::string base(baseBuf.data ? baseBuf.data : "", baseBuf.len);
			nzbget_rs_free(baseBuf);
			if (!FileSystem::FileExists((base + ".meta").c_str()))
			{
				break;
			}
			NzbFetcher::Fetched posting;
			std::string metaRaw = DupeUtil::ReadAll(base + ".meta");
			NzbgetRsPendingMetaView view{};
			nzbget_rs_df_parse_pending_meta(metaRaw.data(), metaRaw.size(), &view);
			posting.listing.title.assign(metaRaw.data() + view.title_offset, view.title_len);
			posting.listing.indexer.assign(metaRaw.data() + view.indexer_offset, view.indexer_len);
			posting.listing.grabs = view.grabs;
			posting.listing.size = view.size;
			posting.listing.date = (time_t)view.date;
			posting.data = DupeUtil::ReadAll(base + ".nzb");
			if (!NzbReader::Parse(posting.data, posting.info))
			{
				continue;
			}
			posting.reason = NzbFetcher::frOk;
            posting.sameInner = DupeUtil::ReadAll(base + ".inner") == "1\n";
			// the postings added before the restart are in nzbget now (ranked with
			// the key's other duplicates), or were deleted by the user since
			Posting::Sketch sketch = Posting::MakeSketch(posting.info.messageIds);
			if (sent.count(posting.info.Fingerprint()))
			{
				rejected["already-sent"]++;
				continue;
			}
			if (std::any_of(known.begin(), known.end(), [&](const Posting::Sketch& k) { return Posting::SameSketch(sketch, k); }))
			{
				rejected["in-nzbget"]++;
				continue;
			}
			verified.push_back(std::move(posting));
		}
		Place(job, pick, verified, rejected, 0, 0, 0);
	}
}

inline std::string DupeSearch::StatePath()
{
	return std::string(g_Options->GetQueueDir()) + PATH_SEPARATOR + "dupesearch";
}

inline void DupeSearch::LoadState()
{
	std::ifstream file(fs::u8path(StatePath()));
	std::string line;
	time_t now = Util::CurrentTime();
	std::lock_guard<std::mutex> guard(m_mutex);
	while (std::getline(file, line))
	{
		NzbgetRsDupeSearchStateLine parsed{};
		if (nzbget_rs_df_parse_dupesearch_state_line(line.data(), line.size(), (int64_t)now, StateTtlSec, &parsed))
		{
			if (parsed.within_ttl)
			{
				std::string key(line.data() + parsed.key_offset, parsed.key_len);
				Searched searched;
				searched.score = parsed.score;
				searched.time = (time_t)parsed.time;
				searched.title.assign(line.data() + parsed.title_offset, parsed.title_len);
				searched.retryable = parsed.retryable != 0;
				RecordSearch(key, searched);
			}
		}
	}
}

inline DupeSearch::Searched* DupeSearch::FindSearch(const std::string& lowerKey, const std::string& title)
{
    auto exact = m_titleSearched.find({lowerKey, title});
    if (exact != m_titleSearched.end()) return &exact->second;
    auto latest = m_searched.find(lowerKey);
    return latest != m_searched.end() && (latest->second.title.empty() || latest->second.title == title) ?
        &latest->second : nullptr;
}

inline void DupeSearch::RecordSearch(const std::string& lowerKey, const Searched& searched)
{
    m_searched[lowerKey] = searched;
    if (!searched.title.empty()) m_titleSearched[{lowerKey, searched.title}] = searched;
}

inline void DupeSearch::SetRetryable(const std::string& lowerKey, const std::string& title, bool retryable)
{
    if (Searched* searched = FindSearch(lowerKey, title)) searched->retryable = retryable;
    auto latest = m_searched.find(lowerKey);
    if (latest != m_searched.end() && (latest->second.title.empty() || latest->second.title == title))
        latest->second.retryable = retryable;
}

inline bool DupeSearch::ForgetSearch(const std::string& dupeKey, const std::string& title)
{
	{
		std::lock_guard<std::mutex> guard(m_mutex);
        std::string lowerKey = LowerKey(dupeKey);
        auto latest = m_searched.find(lowerKey);
        SetRetryable(lowerKey, title.empty() && latest != m_searched.end() ? latest->second.title : title, true);
	}
	if (!g_Options->GetDupeSearchDryRun())
	{
		return SaveState();
	}
    return true;
}

inline bool DupeSearch::SaveState()
{
	std::ostringstream text;
	{
		std::lock_guard<std::mutex> guard(m_mutex);
		time_t now = Util::CurrentTime();
        auto append = [&](const std::string& key, const Searched& searched) {
            NzbgetRsBuf line = nzbget_rs_dupe_watch_format_searched_state_line(
                (int64_t)now, StateTtlSec, key.c_str(), searched.score,
                (int64_t)searched.time, searched.title.c_str(), searched.retryable ? 1 : 0);
            if (line.data && line.len > 0)
                text.write(line.data, (std::streamsize)line.len);
            nzbget_rs_free(line);
        };
        for (const auto& entry : m_titleSearched) append(entry.first.first, entry.second);
        for (const auto& entry : m_searched)
            if (!m_titleSearched.count({entry.first, entry.second.title})) append(entry.first, entry.second);
	}
	if (!DupeUtil::WriteAtomic(StatePath(), text.str()))
	{
		warn("Could not save the DupeSearch state to %s", StatePath().c_str());
        return false;
	}
    return true;
}

#endif
