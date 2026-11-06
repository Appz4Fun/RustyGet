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


#include "nzbget.h"

#include "DonorHealth.h"
#include "nzbget_rs.h"

using Answer = DonorHealth::Answer;
using Health = DonorHealth::Health;

long long DonorHealth::NowMs()
{
	return (long long)nzbget_rs_donor_now_ms();
}

double DonorHealth::Health::Alive() const
{
	return nzbget_rs_df_donor_health_alive(present, missing, error, MinKnown);
}

DonorHealth::Health& DonorHealth::Health::operator+=(const Health& other)
{
	nzbget_rs_dqh_donor_health_accum_health(
		&checked, &present, &missing, &error, &bodyChecked, &bodyBad,
		other.checked, other.present, other.missing, other.error, other.bodyChecked, other.bodyBad);
	return *this;
}

bool DonorHealth::DeadProbe(const Health& health)
{
	return nzbget_rs_df_donor_health_dead_probe(health.present, health.missing, MinKnown) != 0;
}

std::vector<std::string> DonorHealth::Sample(const std::vector<std::string>& ids, int percent, int minimum,
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

std::vector<DonorHealth::Item> DonorHealth::Plan(const std::vector<std::string>& ids,
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

bool DonorHealth::BodyGate::Done()
{
	if (rustGate)
	{
		return nzbget_rs_donor_body_gate_done(rustGate) != 0;
	}
	return ok || (settled && settled());
}

DonorHealth::Server::Server(int cap)
{
	m_rust = nzbget_rs_donor_server_new(this, cap, &Server::ExchangeTrampoline);
}

DonorHealth::Server::~Server()
{
	nzbget_rs_donor_server_free(m_rust);
}

int DonorHealth::Server::Cap() const { return nzbget_rs_donor_server_cap(m_rust); }
int DonorHealth::Server::Depth() const { return nzbget_rs_donor_server_depth(m_rust); }
bool DonorHealth::Server::Paused() const { return nzbget_rs_donor_server_paused(m_rust) != 0; }
int DonorHealth::Server::MaxActive() const { return nzbget_rs_donor_server_max_active(m_rust); }
void DonorHealth::Server::SetRetryAfterMs(int ms) { nzbget_rs_donor_server_set_retry_after_ms(m_rust, ms); }

int DonorHealth::Server::ExchangeTrampoline(
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

DonorHealth::Answer DonorHealth::Server::BodyOnce(BodyGate* gate, const std::function<Answer()>& download)
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

void DonorHealth::StopAll() { nzbget_rs_donor_stop_all(); }
void DonorHealth::Reset() { nzbget_rs_donor_reset(); }
bool DonorHealth::Stopping() { return nzbget_rs_donor_stopping() != 0; }
void DonorHealth::WaitAll() { nzbget_rs_donor_wait_all(); }

std::vector<DonorHealth::Answer> DonorHealth::Server::Ask(const std::vector<Request>& batch,
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

Health DonorHealth::CheckItems(const ServerList& servers, const std::vector<Item>& items, int budgetMs)
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

Health DonorHealth::CheckPosting(const ServerList& servers, const std::vector<std::string>& ids,
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

void DonorHealth::CheckPostings(const ServerList& servers, const std::vector<Posting>& postings,
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
