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


#ifndef DONORHEALTH_H
#define DONORHEALTH_H

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

/*
 * How much of a posting still exists on the news servers: a sample of its
 * articles is asked of ALL servers at once (STAT, and for a few articles also
 * BODY with the yEnc checksum verified, because a server sometimes says
 * "223" when the data is gone). The first server that really has an article
 * settles it; an article is missing only when every server said no, with at
 * least one definitive "430". Each server walks the sample at its own pace,
 * so a slow server never holds up the fast ones.
 *
 * Details that were real bugs on real servers:
 *  - A reply of 451 is an answer (some providers say it for a missing
 *    article): it keeps the connection and never pauses the server, but it
 *    isn't a definitive miss either.
 *  - A server with three transport errors in a row pauses and abstains
 *    meanwhile, so misses don't wait on it.
 *  - When the time runs out, an unsettled article counts as missing when at
 *    least half of the servers missed it and none had it; articles that exist
 *    settle at once, missing ones slowly, so dropping the unsettled ones
 *    made a 12% alive posting read 57%.
 *  - STATs are pipelined: 4 per round trip at first, doubling while a batch
 *    takes under a second (up to 16), halving above two.
 *  - Servers take turns downloading a body: the next server only if the
 *    previous one's data was bad.
 */
class DonorHealth
{
public:
	enum class Answer
	{
		None,
		Present,
		Missing,
		BodyBad,	// the server has the article but its data is bad or gone
		Error		// no usable answer: 451, an error reply, an outage
	};

	struct Health
	{
		int checked = 0;
		int present = 0;
		int missing = 0;
		int error = 0;
		int bodyChecked = 0;	// articles that also got a BODY check
		int bodyBad = 0;		// ... for which no server delivered valid data

		int Answered() const { return present + missing + error; }
		/* the present share of the answered articles; -1 when too few were
		 * answered to judge. Errors count as not present: a live article still
		 * gets a hit from another server, while a server that errors would
		 * otherwise hide every dead article. */
		double Alive() const;
		Health& operator+=(const Health& other);
	};

	// answered articles needed before a posting is judged
	static constexpr int MinKnown = 5;
	static constexpr int ServerGiveUp = 3;
	static constexpr int PipelineStart = 4;
	static constexpr int PipelineMax = 16;

	/* a probe proves a posting dead only with nothing found and enough
	 * definitive misses (an outage, where every answer is an error, never does) */
	static bool DeadProbe(const Health& health);

	struct Item
	{
		std::string messageId;
		bool body = false;
		std::shared_ptr<std::vector<std::string>> groups;
	};

	struct SampleOptions
	{
		int percent = 5;
		int minimum = 50;
		int maximum = 1000;
		int bodyPercent = 20;	// share of the sample that also gets a BODY check ...
		int maxBody = 20;		// ... at most this many
		unsigned seed = 0;
	};

	/* the articles to check: percent of the ids, at least minimum, at most
	 * maximum (deterministic), about bodyPercent of them marked for a body check */
	static std::vector<std::string> Sample(const std::vector<std::string>& ids, int percent, int minimum,
		int maximum, unsigned seed);
	static std::vector<Item> Plan(const std::vector<std::string>& ids,
		const std::shared_ptr<std::vector<std::string>>& groups, const SampleOptions& options);

	struct BodyGate
	{
		void* rustGate = nullptr;
		std::mutex lock;
		std::atomic<bool> ok{false};
		std::function<bool()> settled;
		bool Done();
	};

	struct Request
	{
		std::string messageId;
		BodyGate* gate = nullptr;	// set when the article also gets a BODY check
		std::shared_ptr<std::vector<std::string>> groups;
	};

	struct ExchangeError
	{
		std::vector<Answer> answers;	// the answers read before the connection broke (may be shorter)
		bool busy = false;	// no connection was free (nzbget's own downloads): not the server's fault
	};

	/*
	 * One news server as the check sees it: the pacing, pause and connection
	 * cap shared by every check that runs; the transport (the NNTP
	 * conversation) is a subclass's Exchange().
	 */
	class Server
	{
	public:
		explicit Server(int cap);
		virtual ~Server();

		int Cap() const;
		int Depth() const;
		bool Paused() const;
		int MaxActive() const;

		/* the answers for a batch of articles; waits for a connection slot. Every
		 * wait ends early, with Error answers and nothing asked, once <cancelled>
		 * says so (the check is over) or DonorHealth::StopAll ran */
		std::vector<Answer> Ask(const std::vector<Request>& batch,
			const std::function<bool()>& cancelled = nullptr);

		void SetRetryAfterMs(int ms);
		void* RustHandle() const { return m_rust; }

	protected:
		/* all STATs of the batch in one write, then their replies in order, then
		 * BODY for the articles that have a gate (see BodyOnce); throws ExchangeError
		 * when the connection fails */
		virtual std::vector<Answer> Exchange(const std::vector<Request>& batch) = 0;

		/* one server at a time downloads a gated article's body, and only until one delivered */
		static Answer BodyOnce(BodyGate* gate, const std::function<Answer()>& download);

	private:
		void* m_rust = nullptr;
		static int ExchangeTrampoline(
			void* cppServer, size_t count, const char* const* msgIds, const size_t* msgLens,
			void* const* rustGates, const char* const* const* groupPtrs,
			const size_t* const* groupLens, const size_t* groupCounts,
			int* outAnswers, size_t* outCount, int* outBusy);
	};

	using ServerList = std::vector<std::shared_ptr<Server>>;

	/* one posting's articles on all servers, within the budget (the time starts now) */
	static Health CheckItems(const ServerList& servers, const std::vector<Item>& items, int budgetMs);

	struct Options
	{
		SampleOptions sample;
		int probe = 10;			// articles checked first; a posting with none found is dead
		int budgetMs = 120000;	// for each posting, counted from when its check starts
		long long deadlineMs = 0;	// CheckPostings: no posting checked past it (NowMs; 0: none)
	};

	/* a posting: a probe first; unless that proves it dead (and full is set)
	 * the rest of the sample */
	static Health CheckPosting(const ServerList& servers, const std::vector<std::string>& ids,
		const std::shared_ptr<std::vector<std::string>>& groups, const Options& options, bool full);

	/* postings checked concurrently, each reported as its check finishes */
	struct Posting
	{
		std::string key;
		std::vector<std::string> ids;
		std::shared_ptr<std::vector<std::string>> groups;
	};
	static void CheckPostings(const ServerList& servers, const std::vector<Posting>& postings,
		const Options& options, bool full, int concurrency,
		const std::function<void(const std::string& key, const Health&)>& emit);

	static long long NowMs();

	/* shutdown: the walkers of every check stop asking and end; WaitAll returns
	 * when they did (before the server pool goes away). Reset allows checks again */
	static void StopAll();
	static void WaitAll();
	static void Reset();
	static bool Stopping();
};

#include <algorithm>
#include <exception>
#include "nzbget_rs.h"

inline long long DonorHealth::NowMs()
{
	return (long long)nzbget_rs_donor_now_ms();
}

inline double DonorHealth::Health::Alive() const
{
	return nzbget_rs_df_donor_health_alive(present, missing, error, MinKnown);
}

inline DonorHealth::Health& DonorHealth::Health::operator+=(const Health& other)
{
	nzbget_rs_dqh_donor_health_accum_health(
		&checked, &present, &missing, &error, &bodyChecked, &bodyBad,
		other.checked, other.present, other.missing, other.error, other.bodyChecked, other.bodyBad);
	return *this;
}

inline bool DonorHealth::DeadProbe(const Health& health)
{
	return nzbget_rs_df_donor_health_dead_probe(health.present, health.missing, MinKnown) != 0;
}

inline std::vector<std::string> DonorHealth::Sample(const std::vector<std::string>& ids, int percent, int minimum,
	int maximum, unsigned seed)
{
	std::vector<const char*> ptrs(ids.size());
	std::vector<size_t> lens(ids.size());
	for (size_t i = 0; i < ids.size(); i++)
	{
		ptrs[i] = ids[i].data();
		lens[i] = ids[i].size();
	}
	std::vector<std::string> out;
	nzbget_rs_donor_sample(
		ptrs.data(), lens.data(), ids.size(), percent, minimum, maximum, seed, &out,
		[](void* rawCtx, const char* idPtr, size_t idLen) {
			static_cast<std::vector<std::string>*>(rawCtx)->emplace_back(idPtr ? idPtr : "", idLen);
		});
	return out;
}

inline std::vector<DonorHealth::Item> DonorHealth::Plan(const std::vector<std::string>& ids,
	const std::shared_ptr<std::vector<std::string>>& groups, const SampleOptions& options)
{
	std::vector<const char*> ptrs(ids.size());
	std::vector<size_t> lens(ids.size());
	for (size_t i = 0; i < ids.size(); i++)
	{
		ptrs[i] = ids[i].data();
		lens[i] = ids[i].size();
	}
	struct PlanCtx
	{
		const std::shared_ptr<std::vector<std::string>>& groups;
		std::vector<Item> items;
	} ctx{ groups, {} };
	nzbget_rs_donor_plan(
		ptrs.data(), lens.data(), ids.size(),
		options.percent, options.minimum, options.maximum, options.bodyPercent, options.maxBody,
		options.seed, &ctx,
		[](void* rawCtx, const char* idPtr, size_t idLen, int body) {
			auto* c = static_cast<PlanCtx*>(rawCtx);
			Item item;
			item.messageId.assign(idPtr ? idPtr : "", idLen);
			item.groups = c->groups;
			item.body = body != 0;
			c->items.push_back(std::move(item));
		});
	return ctx.items;
}

inline bool DonorHealth::BodyGate::Done()
{
	if (rustGate)
	{
		return nzbget_rs_donor_body_gate_done(rustGate) != 0;
	}
	return ok || (settled && settled());
}

inline DonorHealth::Server::Server(int cap)
{
	m_rust = nzbget_rs_donor_server_new(this, cap, &Server::ExchangeTrampoline);
}

inline DonorHealth::Server::~Server()
{
	nzbget_rs_donor_server_free(m_rust);
}

inline int DonorHealth::Server::Cap() const { return nzbget_rs_donor_server_cap(m_rust); }
inline int DonorHealth::Server::Depth() const { return nzbget_rs_donor_server_depth(m_rust); }
inline bool DonorHealth::Server::Paused() const { return nzbget_rs_donor_server_paused(m_rust) != 0; }
inline int DonorHealth::Server::MaxActive() const { return nzbget_rs_donor_server_max_active(m_rust); }
inline void DonorHealth::Server::SetRetryAfterMs(int ms) { nzbget_rs_donor_server_set_retry_after_ms(m_rust, ms); }

inline int DonorHealth::Server::ExchangeTrampoline(
	void* cppServer, size_t count, const char* const* msgIds, const size_t* msgLens,
	void* const* rustGates, const char* const* const* groupPtrs,
	const size_t* const* groupLens, const size_t* groupCounts,
	int* outAnswers, size_t* outCount, int* outBusy)
{
	auto* self = static_cast<Server*>(cppServer);
	std::vector<BodyGate> gates(count);
	std::vector<Request> batch(count);
	for (size_t i = 0; i < count; i++)
	{
		batch[i].messageId.assign(msgIds[i] ? msgIds[i] : "", msgLens[i]);
		if (rustGates[i])
		{
			gates[i].rustGate = rustGates[i];
			batch[i].gate = &gates[i];
		}
		if (groupCounts && groupCounts[i] > 0 && groupPtrs && groupPtrs[i] && groupLens && groupLens[i])
		{
			auto g = std::make_shared<std::vector<std::string>>();
			g->reserve(groupCounts[i]);
			for (size_t j = 0; j < groupCounts[i]; j++)
			{
				g->emplace_back(groupPtrs[i][j] ? groupPtrs[i][j] : "", groupLens[i][j]);
			}
			batch[i].groups = std::move(g);
		}
	}
	try
	{
		std::vector<Answer> answers = self->Exchange(batch);
		size_t n = std::min(count, answers.size());
		for (size_t i = 0; i < n; i++)
		{
			outAnswers[i] = (int)answers[i];
		}
		*outCount = n;
		*outBusy = 0;
		return 0;
	}
	catch (ExchangeError& error)
	{
		size_t n = std::min(count, error.answers.size());
		for (size_t i = 0; i < n; i++)
		{
			outAnswers[i] = (int)error.answers[i];
		}
		*outCount = n;
		*outBusy = error.busy ? 1 : 0;
		return 1;
	}
	catch (...)
	{
		*outCount = 0;
		*outBusy = 0;
		return 1;
	}
}

inline DonorHealth::Answer DonorHealth::Server::BodyOnce(BodyGate* gate, const std::function<Answer()>& download)
{
	if (gate && gate->rustGate)
	{
		struct BodyOnceCtx
		{
			const std::function<Answer()>* download;
			std::exception_ptr error;
		} ctx{&download, nullptr};
		int ans = nzbget_rs_donor_body_once(
			gate->rustGate,
			&ctx,
			[](void* rawCtx) -> int {
				auto* c = static_cast<BodyOnceCtx*>(rawCtx);
				try
				{
					return c->download && *c->download ? (int)(*c->download)() : (int)Answer::Error;
				}
				catch (...)
				{
					c->error = std::current_exception();
					return -1;
				}
			});
		if (ctx.error)
		{
			std::rethrow_exception(ctx.error);
		}
		return static_cast<Answer>(ans);
	}
	std::lock_guard<std::mutex> guard(gate->lock);
	if (gate->Done())
	{
		return Answer::Present;
	}
	Answer answer = download();
	gate->ok = answer == Answer::Present;
	return answer;
}

inline void DonorHealth::StopAll() { nzbget_rs_donor_stop_all(); }
inline void DonorHealth::Reset() { nzbget_rs_donor_reset(); }
inline bool DonorHealth::Stopping() { return nzbget_rs_donor_stopping() != 0; }
inline void DonorHealth::WaitAll() { nzbget_rs_donor_wait_all(); }

inline std::vector<DonorHealth::Answer> DonorHealth::Server::Ask(const std::vector<Request>& batch,
	const std::function<bool()>& cancelled)
{
	size_t count = batch.size();
	std::vector<const char*> msgIds(count);
	std::vector<size_t> msgLens(count);
	std::vector<void*> rustGates(count, nullptr);
	std::vector<std::vector<const char*>> groupStrs(count);
	std::vector<std::vector<size_t>> groupLens(count);
	std::vector<const char* const*> groupPtrsOuter(count, nullptr);
	std::vector<const size_t*> groupLensOuter(count, nullptr);
	std::vector<size_t> groupCounts(count, 0);

	for (size_t i = 0; i < count; i++)
	{
		msgIds[i] = batch[i].messageId.data();
		msgLens[i] = batch[i].messageId.size();
		if (batch[i].gate)
		{
			rustGates[i] = batch[i].gate->rustGate;
		}
		if (batch[i].groups && !batch[i].groups->empty())
		{
			const auto& g = *batch[i].groups;
			groupStrs[i].resize(g.size());
			groupLens[i].resize(g.size());
			for (size_t j = 0; j < g.size(); j++)
			{
				groupStrs[i][j] = g[j].data();
				groupLens[i][j] = g[j].size();
			}
			groupPtrsOuter[i] = groupStrs[i].data();
			groupLensOuter[i] = groupLens[i].data();
			groupCounts[i] = g.size();
		}
	}

	std::vector<int> outAnswers(count, (int)Answer::Error);
	nzbget_rs_donor_server_ask(
		m_rust, count, msgIds.data(), msgLens.data(), rustGates.data(),
		groupPtrsOuter.data(), groupLensOuter.data(), groupCounts.data(),
		const_cast<std::function<bool()>*>(&cancelled),
		[](void* rawCtx) -> int {
			auto* fn = static_cast<const std::function<bool()>*>(rawCtx);
			return fn && *fn && (*fn)() ? 1 : 0;
		},
		outAnswers.data());

	std::vector<Answer> res(count);
	for (size_t i = 0; i < count; i++)
	{
		res[i] = static_cast<Answer>(outAnswers[i]);
	}
	return res;
}

inline DonorHealth::Health DonorHealth::CheckItems(const ServerList& servers, const std::vector<Item>& items, int budgetMs)
{
	std::vector<void*> handles(servers.size());
	for (size_t i = 0; i < servers.size(); i++)
	{
		handles[i] = servers[i] ? servers[i]->RustHandle() : nullptr;
	}
	auto* keepalive = new ServerList(servers);
	size_t count = items.size();
	std::vector<const char*> msgIds(count);
	std::vector<size_t> msgLens(count);
	std::vector<int> bodies(count);
	std::vector<std::vector<const char*>> groupStrs(count);
	std::vector<std::vector<size_t>> groupLens(count);
	std::vector<const char* const*> groupPtrsOuter(count, nullptr);
	std::vector<const size_t*> groupLensOuter(count, nullptr);
	std::vector<size_t> groupCounts(count, 0);

	for (size_t i = 0; i < count; i++)
	{
		msgIds[i] = items[i].messageId.data();
		msgLens[i] = items[i].messageId.size();
		bodies[i] = items[i].body ? 1 : 0;
		if (items[i].groups && !items[i].groups->empty())
		{
			const auto& g = *items[i].groups;
			groupStrs[i].resize(g.size());
			groupLens[i].resize(g.size());
			for (size_t j = 0; j < g.size(); j++)
			{
				groupStrs[i][j] = g[j].data();
				groupLens[i][j] = g[j].size();
			}
			groupPtrsOuter[i] = groupStrs[i].data();
			groupLensOuter[i] = groupLens[i].data();
			groupCounts[i] = g.size();
		}
	}

	Health health;
	nzbget_rs_donor_check_items(
		handles.data(), handles.size(),
		keepalive, [](void* p) { delete static_cast<ServerList*>(p); },
		msgIds.data(), msgLens.data(), bodies.data(),
		groupPtrsOuter.data(), groupLensOuter.data(), groupCounts.data(),
		count, budgetMs,
		&health.checked, &health.present, &health.missing, &health.error,
		&health.bodyChecked, &health.bodyBad);
	return health;
}

inline DonorHealth::Health DonorHealth::CheckPosting(const ServerList& servers, const std::vector<std::string>& ids,
	const std::shared_ptr<std::vector<std::string>>& groups, const Options& options, bool full)
{
	std::vector<void*> handles(servers.size());
	for (size_t i = 0; i < servers.size(); i++)
	{
		handles[i] = servers[i] ? servers[i]->RustHandle() : nullptr;
	}
	auto* keepalive = new ServerList(servers);
	std::vector<const char*> idPtrs(ids.size());
	std::vector<size_t> idLens(ids.size());
	for (size_t i = 0; i < ids.size(); i++)
	{
		idPtrs[i] = ids[i].data();
		idLens[i] = ids[i].size();
	}
	std::vector<const char*> groupPtrs;
	std::vector<size_t> groupLens;
	if (groups)
	{
		groupPtrs.resize(groups->size());
		groupLens.resize(groups->size());
		for (size_t i = 0; i < groups->size(); i++)
		{
			groupPtrs[i] = (*groups)[i].data();
			groupLens[i] = (*groups)[i].size();
		}
	}
	Health health;
	nzbget_rs_donor_check_posting(
		handles.data(), handles.size(),
		keepalive, [](void* p) { delete static_cast<ServerList*>(p); },
		idPtrs.data(), idLens.data(), ids.size(),
		groupPtrs.data(), groupLens.data(), groupPtrs.size(),
		options.sample.percent, options.sample.minimum, options.sample.maximum,
		options.sample.bodyPercent, options.sample.maxBody, options.sample.seed,
		options.probe, options.budgetMs, (int64_t)options.deadlineMs, full ? 1 : 0,
		&health.checked, &health.present, &health.missing, &health.error,
		&health.bodyChecked, &health.bodyBad);
	return health;
}

inline void DonorHealth::CheckPostings(const ServerList& servers, const std::vector<Posting>& postings,
	const Options& options, bool full, int concurrency,
	const std::function<void(const std::string&, const Health&)>& emit)
{
	std::vector<void*> handles(servers.size());
	for (size_t i = 0; i < servers.size(); i++)
	{
		handles[i] = servers[i] ? servers[i]->RustHandle() : nullptr;
	}
	auto* keepalive = new ServerList(servers);
	size_t pcount = postings.size();
	std::vector<const char*> keyPtrs(pcount);
	std::vector<size_t> keyLens(pcount);
	std::vector<std::vector<const char*>> idStrs(pcount);
	std::vector<std::vector<size_t>> idLens(pcount);
	std::vector<const char* const*> idPtrsOuter(pcount, nullptr);
	std::vector<const size_t*> idLensOuter(pcount, nullptr);
	std::vector<size_t> idCounts(pcount, 0);
	std::vector<std::vector<const char*>> groupStrs(pcount);
	std::vector<std::vector<size_t>> groupLens(pcount);
	std::vector<const char* const*> groupPtrsOuter(pcount, nullptr);
	std::vector<const size_t*> groupLensOuter(pcount, nullptr);
	std::vector<size_t> groupCounts(pcount, 0);

	for (size_t i = 0; i < pcount; i++)
	{
		keyPtrs[i] = postings[i].key.data();
		keyLens[i] = postings[i].key.size();
		idStrs[i].resize(postings[i].ids.size());
		idLens[i].resize(postings[i].ids.size());
		for (size_t j = 0; j < postings[i].ids.size(); j++)
		{
			idStrs[i][j] = postings[i].ids[j].data();
			idLens[i][j] = postings[i].ids[j].size();
		}
		idPtrsOuter[i] = idStrs[i].data();
		idLensOuter[i] = idLens[i].data();
		idCounts[i] = postings[i].ids.size();

		if (postings[i].groups && !postings[i].groups->empty())
		{
			const auto& g = *postings[i].groups;
			groupStrs[i].resize(g.size());
			groupLens[i].resize(g.size());
			for (size_t j = 0; j < g.size(); j++)
			{
				groupStrs[i][j] = g[j].data();
				groupLens[i][j] = g[j].size();
			}
			groupPtrsOuter[i] = groupStrs[i].data();
			groupLensOuter[i] = groupLens[i].data();
			groupCounts[i] = g.size();
		}
	}

	nzbget_rs_donor_check_postings(
		handles.data(), handles.size(),
		keepalive, [](void* p) { delete static_cast<ServerList*>(p); },
		pcount, keyPtrs.data(), keyLens.data(),
		idPtrsOuter.data(), idLensOuter.data(), idCounts.data(),
		groupPtrsOuter.data(), groupLensOuter.data(), groupCounts.data(),
		options.sample.percent, options.sample.minimum, options.sample.maximum,
		options.sample.bodyPercent, options.sample.maxBody, options.sample.seed,
		options.probe, options.budgetMs, (int64_t)options.deadlineMs,
		full ? 1 : 0, concurrency,
		const_cast<std::function<void(const std::string&, const Health&)>*>(&emit),
		[](void* rawCtx, const char* keyPtr, size_t keyLen,
			int checked, int present, int missing, int error, int bodyChecked, int bodyBad) {
			auto* fn = static_cast<const std::function<void(const std::string&, const Health&)>*>(rawCtx);
			if (fn && *fn)
			{
				Health h;
				h.checked = checked;
				h.present = present;
				h.missing = missing;
				h.error = error;
				h.bodyChecked = bodyChecked;
				h.bodyBad = bodyBad;
				(*fn)(std::string(keyPtr ? keyPtr : "", keyLen), h);
			}
		});
}

#include <algorithm>
#include <cstdint>
#include <ctime>
#include <map>
#include <set>
#include <thread>
#include <utility>
#include "ArticleFetcher.h"
#include "WebDownloader.h"
#include "FileSystem.h"
#include "ServerPool.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

class ReleaseName
{
public:
	static constexpr size_t MaxNameLength = 512;
	struct Attrs
	{
		std::set<std::string> resolution;
		std::set<std::string> quality;
		std::set<std::string> codec;
		std::set<std::string> bitDepth;
		std::set<std::string> audio;
		std::set<std::string> channels;
		std::set<std::string> network;
		std::set<std::string> edition;
		std::set<std::string> hdr;
		std::string title;
		std::string group;
		std::vector<int> seasons;
		std::vector<int> episodes;
		int year = 0;
		bool repack = false;
		bool proper = false;
	};

	static std::string Clean(const std::string& name)
	{
		return Take(nzbget_rs_releasename_clean(name.data(), name.size()));
	}

	static std::string Normalize(const std::string& name)
	{
		return Take(nzbget_rs_releasename_normalize(name.data(), name.size()));
	}

	static std::string ShortQuery(const std::string& title)
	{
		return Take(nzbget_rs_releasename_short_query(title.data(), title.size()));
	}

	static Attrs Parse(const std::string& name)
	{
		std::string raw = Take(nzbget_rs_releasename_parse(name.data(), name.size()));
		struct Reader
		{
			const char* p;
			const char* end;
			uint32_t U32()
			{
				if (end - p < 4) { p = end; return 0; }
				const unsigned char* b = (const unsigned char*)p;
				p += 4;
				return (uint32_t)b[0] | (uint32_t)b[1] << 8 | (uint32_t)b[2] << 16 | (uint32_t)b[3] << 24;
			}
			std::string Str() { uint32_t n = U32(); n = std::min<uint32_t>(n, end - p); std::string s(p, n); p += n; return s; }
			int I32() { return (int)U32(); }
			bool Byte() { return p < end ? *p++ != 0 : false; }
		} in{raw.data(), raw.data() + raw.size()};

		Attrs attrs;
		for (std::set<std::string>* set : {&attrs.resolution, &attrs.quality, &attrs.codec, &attrs.bitDepth, &attrs.audio,
			&attrs.channels, &attrs.network, &attrs.edition, &attrs.hdr})
		{
			for (uint32_t n = in.U32(); n > 0 && in.p < in.end; n--)
			{
				set->insert(in.Str());
			}
		}
		attrs.title = in.Str();
		attrs.group = in.Str();
		for (std::vector<int>* list : {&attrs.seasons, &attrs.episodes})
		{
			for (uint32_t n = in.U32(); n > 0 && in.p < in.end; n--)
			{
				list->push_back(in.I32());
			}
		}
		attrs.year = in.I32();
		attrs.repack = in.Byte();
		attrs.proper = in.Byte();
		return attrs;
	}

	static bool Readable(const std::string& name)
	{
		return nzbget_rs_releasename_readable(name.data(), name.size()) != 0;
	}

	static bool SameRelease(const std::string& aName, const std::string& bName)
	{
		return nzbget_rs_releasename_same_release(aName.data(), aName.size(), bName.data(), bName.size()) != 0;
	}

private:
	static std::string Take(NzbgetRsBuf buf)
	{
		std::string out(buf.data, buf.len);
		nzbget_rs_free(buf);
		return out;
	}
};

namespace DupeUtil
{
	inline bool WriteAtomic(const std::string& path, const std::string& data)
	{
		return nzbget_rs_dupe_util_write_atomic(path.data(), path.size(), data.data(), data.size()) != 0;
	}

	inline std::string Lower(std::string text)
	{
		NzbgetRsBuf buf = nzbget_rs_dqh_dupe_util_lower(text.data(), text.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	inline std::string ReadAll(const std::string& path)
	{
		NzbgetRsBuf buf = nzbget_rs_dupe_util_read_all(path.data(), path.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}
}

struct NzbSummary
{
	struct Head
	{
		std::string name;
		std::string messageId;
		long long bytes = 0;
	};
	int files = 0;
	long long totalBytes = 0;
	std::set<std::string> filenames;
	std::string poster;
	std::vector<std::string> messageIds;
	std::vector<std::string> groups;
	std::map<std::string, std::string> meta;
	std::string mainName;
	uint64_t listedParts = 0;
	uint64_t declaredParts = 0;
	uint64_t articleBytes = 0;
	Head parIndex;
	std::vector<uint64_t> recoveryBytes;
	long long dataBytes = 0;
	int dataFiles = 0;
	std::vector<Head> heads;
	std::vector<std::string> dataMessageIds;
	uint64_t dataListedParts = 0;
	uint64_t dataDeclaredParts = 0;

	std::string Fingerprint() const
	{
		std::vector<const char*> ptrs;
		std::vector<size_t> lens;
		ptrs.reserve(messageIds.size());
		lens.reserve(messageIds.size());
		for (const std::string& id : messageIds)
		{
			ptrs.push_back(id.data());
			lens.push_back(id.size());
		}
		char buffer[17] = {};
		nzbget_rs_dupesearch_fnv1a_fingerprint(ptrs.data(), lens.data(), ptrs.size(), buffer);
		return std::string(buffer, 16);
	}
};

class NzbReader
{
public:
	static bool Parse(const std::string& data, NzbSummary& summary)
	{
		summary = NzbSummary();
		RsNzbSummaryScalars scalars{};
		int ok = nzbget_rs_nzb_reader_parse(
			reinterpret_cast<const uint8_t*>(data.data()),
			data.size(),
			&scalars,
			&summary,
			NzbReader::OnStrings,
			NzbReader::OnSlice,
			NzbReader::OnRecoveryBytes);
		if (ok == 0)
		{
			summary = NzbSummary();
			return false;
		}
		ApplyScalars(summary, scalars);
		return true;
	}

	static void OnStrings(void* ctx,
		const uint8_t* posterPtr, size_t posterLen,
		const uint8_t* mainNamePtr, size_t mainNameLen,
		const uint8_t* parNamePtr, size_t parNameLen,
		const uint8_t* parIdPtr, size_t parIdLen,
		int64_t parBytes)
	{
		NzbSummary* s = static_cast<NzbSummary*>(ctx);
		s->poster.assign(reinterpret_cast<const char*>(posterPtr), posterLen);
		s->mainName.assign(reinterpret_cast<const char*>(mainNamePtr), mainNameLen);
		s->parIndex.name.assign(reinterpret_cast<const char*>(parNamePtr), parNameLen);
		s->parIndex.messageId.assign(reinterpret_cast<const char*>(parIdPtr), parIdLen);
		s->parIndex.bytes = parBytes;
	}

	static void OnSlice(void* ctx, int kind,
		const uint8_t* ptr1, size_t len1,
		const uint8_t* ptr2, size_t len2,
		int64_t bytes)
	{
		NzbSummary* s = static_cast<NzbSummary*>(ctx);
		const char* c1 = reinterpret_cast<const char*>(ptr1);
		const char* c2 = reinterpret_cast<const char*>(ptr2);
		switch (kind)
		{
			case 1:
				s->groups.emplace_back(c1, len1);
				break;
			case 2:
				s->meta.emplace(std::string(c1, len1), std::string(c2, len2));
				break;
			case 3:
				s->filenames.emplace(c1, len1);
				break;
			case 4:
				s->messageIds.emplace_back(c1, len1);
				break;
			case 5:
				s->dataMessageIds.emplace_back(c1, len1);
				break;
			case 6:
				s->heads.push_back({std::string(c1, len1), std::string(c2, len2), bytes});
				break;
		}
	}

	static void OnRecoveryBytes(void* ctx, const uint64_t* ptr, size_t len)
	{
		NzbSummary* s = static_cast<NzbSummary*>(ctx);
		s->recoveryBytes.assign(ptr, ptr + len);
	}

	static void ApplyScalars(NzbSummary& summary, const RsNzbSummaryScalars& scalars)
	{
		summary.files = scalars.files;
		summary.totalBytes = scalars.total_bytes;
		summary.dataFiles = scalars.data_files;
		summary.dataBytes = scalars.data_bytes;
		summary.listedParts = scalars.listed_parts;
		summary.declaredParts = scalars.declared_parts;
		summary.dataListedParts = scalars.data_listed_parts;
		summary.dataDeclaredParts = scalars.data_declared_parts;
		summary.articleBytes = scalars.article_bytes;
	}
};

class HttpGet
{
public:
	struct Reply
	{
		bool ok = false;
		int status = 0;
		std::string body;
	};

	static Reply Fetch(const std::string& url, const std::string& infoName, size_t maxBytes,
		long long deadlineMs = 0)
	{
		Reply reply;
		if (g_stopped)
		{
			return reply;
		}

		NzbgetRsBuf pathBuf = nzbget_rs_dqh_http_get_format_temp_path(
			g_Options->GetTempDir(), PATH_SEPARATOR, (int)Util::CurrentTime(), ++g_counter);
		std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
		nzbget_rs_free(pathBuf);

		WebDownloader downloader;
		downloader.SetUrl(url.c_str());
		downloader.SetInfoName(infoName.c_str());
		downloader.SetOutputFilename(path.c_str());
		downloader.SetForce(true);
		downloader.SetRetry(false);

		{
			Guard guard(g_mutex);
			if (g_stopped)
			{
				return reply;
			}
			g_active.insert(&downloader);
		}
		std::mutex doneMutex;
		std::condition_variable doneCond;
		bool done = false;
		std::thread watchdog([&]()
			{
				std::unique_lock<std::mutex> lock(doneMutex);
				while (!done)
				{
					int64_t waitMs = 250;
					if (nzbget_rs_snh_http_get_calc_watchdog_wait(
						deadlineMs, DonorHealth::NowMs(), FileSystem::FileSize(path.c_str()), maxBytes, &waitMs))
					{
						downloader.Stop();
						return;
					}
					doneCond.wait_for(lock, std::chrono::milliseconds(waitMs), [&]() { return done; });
				}
			});
		WebDownloader::EStatus status = downloader.DownloadWithRedirects(5);
		if (watchdog.joinable())
		{
			{
				std::lock_guard<std::mutex> lock(doneMutex);
				done = true;
			}
			doneCond.notify_all();
			watchdog.join();
		}
		{
			Guard guard(g_mutex);
			g_active.erase(&downloader);
		}

		reply.status = downloader.GetHttpStatus();
		int readOk = 0;
		NzbgetRsBuf bodyBuf = nzbget_rs_http_get_read_and_delete(
			path.data(), path.size(), maxBytes, status == WebDownloader::adFinished ? 1 : 0, &readOk);
		if (readOk != 0)
		{
			reply.body.assign(bodyBuf.data ? bodyBuf.data : "", bodyBuf.len);
			reply.ok = true;
		}
		nzbget_rs_free(bodyBuf);
		return reply;
	}

	static void StopAll()
	{
		Guard guard(g_mutex);
		g_stopped = true;
		for (WebDownloader* downloader : g_active)
		{
			downloader->Stop();
		}
	}

	static void Reset()
	{
		g_stopped = false;
	}

	static bool Stopped()
	{
		return g_stopped;
	}

private:
	inline static Mutex g_mutex;
	inline static std::set<WebDownloader*> g_active;
	inline static std::atomic<bool> g_stopped{false};
	inline static std::atomic<int> g_counter{0};
};

class Newznab
{
public:
	struct Result
	{
		std::string title;
		std::string link;
		long long size = 0;
		int grabs = 0;
		time_t date = 0;
		std::string indexer;
	};

	struct Page
	{
		std::vector<Result> items;
		bool error = false;
		int errorCode = 0;
		std::string errorText;
	};

	using Params = std::vector<std::pair<std::string, std::string>>;

	static constexpr int PageSize = 100;
	static constexpr int MaxPages = 5;
	static constexpr int Parallel = 4;

	static bool ParseResponse(const std::string& xml, Page& page)
	{
		page = Page();
		return nzbget_rs_newznab_parse_response(
			reinterpret_cast<const uint8_t*>(xml.data()),
			xml.size(),
			&page,
			[](void* ctx, int errorCode, const uint8_t* errorTextPtr, size_t errorTextLen) noexcept
			{
				auto* out = static_cast<Page*>(ctx);
				out->error = true;
				out->errorCode = errorCode;
				out->errorText.assign(reinterpret_cast<const char*>(errorTextPtr), errorTextLen);
			},
			[](void* ctx, const uint8_t* titlePtr, size_t titleLen,
			   const uint8_t* linkPtr, size_t linkLen, int64_t size, int32_t grabs,
			   int64_t date, const uint8_t* indexerPtr, size_t indexerLen) noexcept
			{
				auto* out = static_cast<Page*>(ctx);
				Result r;
				r.title.assign(reinterpret_cast<const char*>(titlePtr), titleLen);
				r.link.assign(reinterpret_cast<const char*>(linkPtr), linkLen);
				r.size = (long long)size;
				r.grabs = grabs;
				r.date = (time_t)date;
				r.indexer.assign(reinterpret_cast<const char*>(indexerPtr), indexerLen);
				out->items.push_back(std::move(r));
			}) != 0;
	}

	static time_t ParseDate(const std::string& text)
	{
		return (time_t)nzbget_rs_df_parse_newznab_date(text.data(), text.size());
	}

	static std::vector<Params> BuildQueries(const std::string& title, const std::string& imdb,
		const std::string& tvdb)
	{
		std::vector<Params> queries;
		nzbget_rs_df_build_newznab_queries(
			title.data(), title.size(),
			imdb.data(), imdb.size(),
			tvdb.data(), tvdb.size(),
			&queries,
			[](void* ctx, const char* const* keys, const size_t* keyLens,
			   const char* const* vals, const size_t* valLens, size_t count) noexcept
			{
				auto* out = static_cast<std::vector<Params>*>(ctx);
				Params p;
				p.reserve(count);
				for (size_t i = 0; i < count; i++)
				{
					p.emplace_back(std::string(keys[i], keyLens[i]), std::string(vals[i], valLens[i]));
				}
				out->push_back(std::move(p));
			});
		return queries;
	}

	static std::string BuildUrl(const std::string& base, const Params& params, const std::string& apiKey)
	{
		std::vector<const char*> keys(params.size());
		std::vector<const char*> vals(params.size());
		for (size_t i = 0; i < params.size(); i++)
		{
			keys[i] = params[i].first.c_str();
			vals[i] = params[i].second.c_str();
		}
		NzbgetRsBuf buf = nzbget_rs_df_build_newznab_url(
			base.c_str(), keys.data(), vals.data(), params.size(), apiKey.c_str());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	static std::string Mask(const std::string& text)
	{
		NzbgetRsBuf buf = nzbget_rs_df_mask_newznab_text(text.data(), text.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	struct SearchStats
	{
		int queries = 0;
		int failed = 0;
		int pages = 0;
	};

	static std::vector<Result> Search(const std::string& base, const std::string& apiKey,
		const std::vector<Params>& queries, time_t deadline, SearchStats* stats)
	{
		std::vector<std::vector<Result>> found(queries.size());
		std::atomic<size_t> next{0};
		std::atomic<int> failed{0};
		std::atomic<int> pages{0};

		auto worker = [&]()
		{
			for (size_t index = next++; index < queries.size(); index = next++)
			{
				std::vector<const char*> lKeys;
				std::vector<const char*> lVals;
				lKeys.reserve(queries[index].size());
				lVals.reserve(queries[index].size());
				for (const auto& param : queries[index])
				{
					lKeys.push_back(param.first.c_str());
					lVals.push_back(param.second.c_str());
				}
				NzbgetRsBuf lblBuf = nzbget_rs_dqh_newznab_format_query_label(
					lKeys.data(), lVals.data(), lKeys.size());
				std::string label(lblBuf.data ? lblBuf.data : "", lblBuf.len);
				nzbget_rs_free(lblBuf);

				for (int page = 0; page < MaxPages; page++)
				{
					if (Util::CurrentTime() >= deadline)
					{
						detail("DupeSearch: no time left for the search %s", label.c_str());
						break;
					}

					Params params = queries[index];
					NzbgetRsBuf limBuf = nzbget_rs_dqh_conn_format_port_str(PageSize);
					params.emplace_back("limit", std::string(limBuf.data ? limBuf.data : "", limBuf.len));
					nzbget_rs_free(limBuf);
					NzbgetRsBuf offBuf = nzbget_rs_dqh_conn_format_port_str(page * PageSize);
					params.emplace_back("offset", std::string(offBuf.data ? offBuf.data : "", offBuf.len));
					nzbget_rs_free(offBuf);
					NzbgetRsBuf infoBuf = nzbget_rs_dqh_newznab_format_search_info_name(label.c_str(), page + 1);
					std::string infoName(infoBuf.data ? infoBuf.data : "", infoBuf.len);
					nzbget_rs_free(infoBuf);
					long long pageDeadlineMs = std::max(1LL, DonorHealth::NowMs() + (long long)(deadline - Util::CurrentTime()) * 1000LL);
					HttpGet::Reply reply = HttpGet::Fetch(BuildUrl(base, params, apiKey), infoName, 16 * 1024 * 1024,
						pageDeadlineMs);
					if (!reply.ok && reply.status == 0 && !HttpGet::Stopped() && Util::CurrentTime() < deadline)
					{
						reply = HttpGet::Fetch(BuildUrl(base, params, apiKey), infoName, 16 * 1024 * 1024,
							pageDeadlineMs);
					}
					pages++;

					Page result;
					if (!reply.ok)
					{
						warn("DupeSearch: the search %s failed (HTTP %i)", label.c_str(), reply.status);
						failed++;
						break;
					}
					if (!ParseResponse(reply.body, result))
					{
						warn("DupeSearch: the answer to the search %s isn't XML (is option DupeSearchUrl the "
							"indexer's api address, like http://host:5076/api?)", label.c_str());
						failed++;
						break;
					}
					if (result.error)
					{
						warn("DupeSearch: the indexer answered the search %s with error %i: %s",
							label.c_str(), result.errorCode, Mask(result.errorText).c_str());
						failed++;
						break;
					}

					found[index].insert(found[index].end(), result.items.begin(), result.items.end());
					if ((int)result.items.size() < PageSize)
					{
						break;
					}
				}
			}
		};

		std::vector<std::thread> threads;
		size_t count = std::min(queries.size(), (size_t)Parallel);
		for (size_t i = 0; i < count; i++)
		{
			threads.emplace_back(worker);
		}
		for (std::thread& thread : threads)
		{
			thread.join();
		}

		std::vector<Result> merged;
		std::set<std::string> links;
		for (const std::vector<Result>& results : found)
		{
			for (const Result& result : results)
			{
				if (!result.link.empty() && links.insert(result.link).second)
				{
					merged.push_back(result);
				}
			}
		}

		if (stats)
		{
			stats->queries = (int)queries.size();
			stats->failed = failed;
			stats->pages = pages;
		}
		return merged;
	}
};

class Posting
{
public:
	static constexpr int RelistWindowSec = 120;
	static constexpr double SamePostingShare = 0.01;
	static constexpr size_t SketchSize = 64;
	static constexpr double ListingMismatchShare = 0.02;

	using Sketch = std::vector<uint32_t>;
	using Group = std::vector<Newznab::Result>;

	static Sketch MakeSketch(const std::vector<std::string>& messageIds)
	{
		Strings ids(messageIds);
		Sketch sketch(SketchSize);
		size_t len = 0;
		nzbget_rs_posting_sketch(ids.data.data(), ids.lens.data(), ids.data.size(), sketch.data(), &len);
		sketch.resize(len);
		return sketch;
	}

	static bool SameSketch(const Sketch& a, const Sketch& b)
	{
		return nzbget_rs_posting_same_sketch(a.data(), a.size(), b.data(), b.size()) != 0;
	}

	static bool SketchOfFile(const std::string& path, Sketch& sketch)
	{
		sketch.assign(SketchSize, 0);
		size_t len = 0;
		if (!nzbget_rs_posting_sketch_of_file(path.c_str(), sketch.data(), SketchSize, &len))
		{
			sketch.clear();
			return false;
		}
		sketch.resize(len);
		return true;
	}

	static bool SamePosting(const std::vector<std::string>& a, const std::vector<std::string>& b)
	{
		Strings x(a), y(b);
		return nzbget_rs_posting_same_posting(x.data.data(), x.lens.data(), x.data.size(), y.data.data(), y.lens.data(),
			y.data.size()) != 0;
	}

	static std::vector<Group> GroupListings(std::vector<Newznab::Result> results)
	{
		std::vector<NzbgetRsListing> raw = Listings(results);
		std::vector<size_t> indices(raw.size()), sizes(raw.size());
		size_t count = 0;
		nzbget_rs_posting_group(raw.data(), raw.size(), indices.data(), sizes.data(), &count);
		return Groups(results, indices, sizes, count);
	}

	static bool ListingMismatch(const Newznab::Result& listing, long long nzbBytes)
	{
		return nzbget_rs_posting_listing_mismatch((int64_t)listing.size, (int64_t)nzbBytes) != 0;
	}

	static std::vector<Group> OrderPostings(const std::vector<Newznab::Result>& candidates,
		long long pickBytes, int maxPostings)
	{
		std::vector<NzbgetRsListing> raw = Listings(candidates);
		std::vector<size_t> indices(raw.size()), sizes(raw.size());
		size_t count = 0;
		nzbget_rs_posting_order(raw.data(), raw.size(), (int64_t)pickBytes, maxPostings, indices.data(), sizes.data(), &count);
		return Groups(candidates, indices, sizes, count);
	}

private:
	struct Strings
	{
		std::vector<const char*> data;
		std::vector<size_t> lens;
		explicit Strings(const std::vector<std::string>& texts)
		{
			for (const std::string& t : texts) { data.push_back(t.data()); lens.push_back(t.size()); }
		}
	};

	static std::vector<NzbgetRsListing> Listings(const std::vector<Newznab::Result>& results)
	{
		std::vector<NzbgetRsListing> raw;
		for (const Newznab::Result& r : results)
		{
			raw.push_back({r.link.data(), r.link.size(), (int64_t)r.size, (int32_t)r.grabs, (int64_t)r.date});
		}
		return raw;
	}

	static std::vector<Group> Groups(const std::vector<Newznab::Result>& results, const std::vector<size_t>& indices,
		const std::vector<size_t>& sizes, size_t count)
	{
		std::vector<Group> groups;
		size_t at = 0;
		for (size_t g = 0; g < count; g++)
		{
			Group group;
			for (size_t k = 0; k < sizes[g]; k++) group.push_back(results[indices[at++]]);
			groups.push_back(std::move(group));
		}
		return groups;
	}
};

class NntpHealthServer : public DonorHealth::Server
{
public:
	static constexpr int MaxCap = 20;
	static constexpr int ConnectionWaitSec = 10;

	NntpHealthServer(int serverId, int cap) : Server(cap), m_serverId(serverId) {}

	int ServerId() const { return m_serverId; }

	static int CapFor(int connections)
	{
		return nzbget_rs_snh_nntp_health_cap_for(connections, MaxCap);
	}

	static DonorHealth::ServerList Servers()
	{
		static std::mutex mutex;
		static std::map<int, std::shared_ptr<NntpHealthServer>> known;

		std::lock_guard<std::mutex> guard(mutex);
		DonorHealth::ServerList list;
		std::set<int> groups;
		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (!server->GetActive() || server->GetMaxConnections() == 0)
			{
				continue;
			}
			if (server->GetGroup() > 0 && !groups.insert(server->GetGroup()).second)
			{
				continue;
			}
			std::shared_ptr<NntpHealthServer>& entry = known[server->GetId()];
			int wantedCap = CapFor(server->GetMaxConnections());
			if (!entry || entry->Cap() != wantedCap)
			{
				entry = std::make_shared<NntpHealthServer>(server->GetId(), wantedCap);
			}
			list.push_back(entry);
		}
		return list;
	}

protected:
	std::vector<DonorHealth::Answer> Exchange(const std::vector<DonorHealth::Request>& batch) override
	{
		using Answer = DonorHealth::Answer;

		NewsServer* server = g_ServerPool->GetServerById(m_serverId);
		if (!server)
		{
			throw DonorHealth::ExchangeError();
		}

		NntpConnection* connection = nullptr;
		time_t waitStart = Util::CurrentTime();
		while (!connection && !HttpGet::Stopped() && Util::CurrentTime() - waitStart <= ConnectionWaitSec)
		{
			connection = g_ServerPool->GetConnection(server->GetNormLevel(), server, nullptr);
			if (!connection)
			{
				Util::Sleep(20);
			}
		}
		if (!connection)
		{
			DonorHealth::ExchangeError busy;
			busy.busy = true;
			throw busy;
		}

		struct Release
		{
			NntpConnection* connection;
			bool broken = false;
			~Release()
			{
				if (broken)
				{
					connection->Disconnect();
				}
				g_ServerPool->FreeConnection(connection, true);
			}
		} release{ connection };

		if (!connection->Connect())
		{
			release.broken = true;
			throw DonorHealth::ExchangeError();
		}

		if (server->GetJoinGroup() && batch.front().groups)
		{
			const char* joined = nullptr;
			for (const std::string& group : *batch.front().groups)
			{
				joined = connection->JoinGroup(group.c_str());
				if (joined && joined[0] == '2')
				{
					break;
				}
			}
			if (!joined || joined[0] != '2')
			{
				release.broken = !joined;
				throw DonorHealth::ExchangeError();
			}
		}

		std::vector<const char*> statIds;
		std::vector<size_t> statLens;
		statIds.reserve(batch.size());
		statLens.reserve(batch.size());
		for (const DonorHealth::Request& request : batch)
		{
			statIds.push_back(request.messageId.data());
			statLens.push_back(request.messageId.size());
		}
		NzbgetRsBuf cmdBuf = nzbget_rs_dqh_nntp_health_format_stat_batch(
			statIds.data(), statLens.data(), statIds.size());
		std::string commands(cmdBuf.data ? cmdBuf.data : "", cmdBuf.len);
		nzbget_rs_free(cmdBuf);
		if (!connection->Send(commands.c_str(), (int)commands.size()))
		{
			release.broken = true;
			throw DonorHealth::ExchangeError();
		}

		std::vector<Answer> answers;
		for (size_t i = 0; i < batch.size(); i++)
		{
			if (HttpGet::Stopped() || DonorHealth::Stopping())
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				throw error;
			}
			char line[1024];
			int length = 0;
			if (!connection->ReadLine(line, sizeof(line), &length))
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				throw error;
			}
			int statReply = nzbget_rs_snh_nntp_health_classify_stat_reply(line);
			answers.push_back(statReply == 0 ? Answer::Present : statReply == 1 ? Answer::Missing : Answer::Error);
		}

		for (size_t i = 0; i < batch.size(); i++)
		{
			if (HttpGet::Stopped() || DonorHealth::Stopping())
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				for (size_t j = i; j < batch.size(); j++)
					if (batch[j].gate && error.answers[j] == Answer::Present)
						error.answers[j] = Answer::Error;
				throw error;
			}
			if (!batch[i].gate || answers[i] != Answer::Present)
			{
				continue;
			}
			try
			{
				answers[i] = BodyOnce(batch[i].gate, [&]() -> Answer
					{
						std::vector<CString> groups;
						if (batch[i].groups)
						{
							for (const std::string& group : *batch[i].groups)
							{
								groups.emplace_back(group.c_str());
							}
						}
						ArticleFetcher fetcher;
						ArticleFetcher::FetchedArticle article = fetcher.FetchFromConnection(connection, Bracketed(batch[i].messageId).c_str(), groups);
						int throwError = 0;
						int classified = nzbget_rs_nntp_health_classify_body_fetch_result(
							article.Success ? 1 : 0, article.Transient ? 1 : 0,
							(int)Answer::Present, (int)Answer::BodyBad, &throwError);
						if (throwError != 0)
						{
							throw DonorHealth::ExchangeError();
						}
						return (Answer)classified;
					});
			}
			catch (DonorHealth::ExchangeError&)
			{
				release.broken = true;
				DonorHealth::ExchangeError error;
				error.answers = answers;
				error.answers[i] = Answer::Error;
				for (size_t j = i + 1; j < batch.size(); j++)
				{
					if (batch[j].gate && answers[j] == Answer::Present)
					{
						error.answers[j] = Answer::Error;
					}
				}
				throw error;
			}
		}
		return answers;
	}

private:
	int m_serverId;

	static std::string Bracketed(const std::string& messageId)
	{
		NzbgetRsBuf buf = nzbget_rs_dqh_nntp_health_format_bracketed_id(
			messageId.data(), messageId.size());
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}
};

#endif
