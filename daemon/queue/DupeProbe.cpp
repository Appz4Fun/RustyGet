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

#include <algorithm>
#include "DupeProbe.h"
#include "DownloadInfo.h"
#include "QueueCoordinator.h"
#include "DupeCoordinator.h"
#include "ServerPool.h"
#include "NewsServer.h"
#include "NntpConnection.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace
{
	// how long a server gets to hand out a connection (downloads keep them busy)
	constexpr int ConnectionWaitSec = 10;
	// the whole probe
	constexpr int ProbeLimitSec = 90;

	// registry of running probes: shutdown cancels them and waits
	Mutex g_probeMutex;
	std::set<DupeProbe*> g_probes;
	std::atomic<int> g_probeCount{0};
	// set by StopAll: no probe starts after it (it would outlive the server pool)
	bool g_probesStopping = false;
}

std::vector<size_t> DupeProbe::SampleIndexes(size_t total, int count)
{
	std::vector<size_t> indexes;
	if (total == 0 || count <= 0)
	{
		return indexes;
	}

	size_t wanted = std::min(total, (size_t)count);
	indexes.resize(wanted);
	nzbget_rs_dupe_probe_sample_indexes(total, count, indexes.data(), indexes.size());
	return indexes;
}

DupeProbe::EAnswer DupeProbe::Classify(const char* response)
{
	return (EAnswer)nzbget_rs_dupe_probe_classify(response);
}

bool DupeProbe::IsDead(int existing, int definitiveServers, int activeServers)
{
	return nzbget_rs_dupe_probe_is_dead(existing, definitiveServers, activeServers) != 0;
}

void DupeProbe::Start(int nzbId, std::vector<Sample> samples, int recoveredAtStart)
{
	DupeProbe* probe = new DupeProbe(nzbId, std::move(samples));
	probe->m_recoveredAtStart = recoveredAtStart;
	if (!probe->Register())
	{
		delete probe;
		return;
	}
	probe->SetAutoDestroy(true);
	probe->Thread::Start();
}

DupeProbe::Verdict DupeProbe::Check(std::vector<Sample> samples, int limitSec)
{
	DupeProbe probe(0, std::move(samples));
	if (!probe.Register())
	{
		Verdict stopping;
		stopping.Finished = false;
		return stopping;
	}
	Verdict verdict = probe.Measure(limitSec);
	probe.Unregister();
	return verdict;
}

void DupeProbe::Reset()
{
	Guard guard(g_probeMutex);
	g_probesStopping = false;
}

int DupeProbe::ProbingFor(int nzbId)
{
	Guard guard(g_probeMutex);
	for (DupeProbe* probe : g_probes)
	{
		int elapsed = nzbget_rs_tpr_dupe_probe_probing_for_elapsed_sec(
			probe->m_nzbId, nzbId, probe->m_countAll ? 1 : 0,
			(int64)Util::CurrentTime(), (int64)probe->m_started);
		if (elapsed >= 0)
		{
			return elapsed;
		}
	}
	return -1;
}

bool DupeProbe::Register()
{
	Guard guard(g_probeMutex);
	if (g_probesStopping)
	{
		return false;
	}
	m_started = Util::CurrentTime();
	g_probes.insert(this);
	g_probeCount++;
	return true;
}

void DupeProbe::Unregister()
{
	Guard guard(g_probeMutex);
	g_probes.erase(this);
	g_probeCount--;
}

std::vector<DupeProbe::Sample> DupeProbe::SamplesOf(const std::vector<FileInfo*>& files,
	const std::function<void(FileInfo*)>& loadArticles)
{
	size_t total = 0;
	for (FileInfo* fileInfo : files)
	{
		total += fileInfo->GetTotalArticles();
	}

	std::vector<Sample> samples;
	for (size_t position : SampleIndexes(total, SampleCount))
	{
		size_t offset = 0;
		for (FileInfo* fileInfo : files)
		{
			size_t count = fileInfo->GetTotalArticles();
			ptrdiff_t relIdx = nzbget_rs_tpr_dupe_probe_samples_of_file_index(position, offset, count);
			if (relIdx < 0)
			{
				offset += count;
				continue;
			}
			size_t index = (size_t)relIdx;

			if (fileInfo->GetArticles()->empty() && loadArticles)
			{
				loadArticles(fileInfo);
			}
			if (index < fileInfo->GetArticles()->size())
			{
				Sample sample;
				sample.MessageId = fileInfo->GetArticles()->at(index)->GetMessageId();
				sample.Groups = std::make_shared<std::vector<CString>>();
				for (const CString& group : *fileInfo->GetGroups())
				{
					sample.Groups->emplace_back(*group);
				}
				samples.push_back(std::move(sample));
			}
			break;
		}
	}
	return samples;
}

void DupeProbe::StopAll()
{
	Guard guard(g_probeMutex);
	g_probesStopping = true;
	for (DupeProbe* probe : g_probes)
	{
		probe->Thread::Stop();
		probe->Cancel();
	}
}

void DupeProbe::WaitAll()
{
	// without a time limit, like the download threads (QueueCoordinator::WaitJobs):
	// a probe that outlived the wait would use the server pool after it was freed.
	// StopAll cancelled their connections, so they end quickly
	while (g_probeCount > 0)
	{
		Util::Sleep(20);
	}
}

// called with g_probeMutex held
void DupeProbe::Cancel()
{
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
}

bool DupeProbe::ProbeServer(int serverId, std::set<int>& probed, ServerResult& result)
{
	NewsServer* wanted = g_ServerPool->GetServerById(serverId);
	if (!wanted)
	{
		return false;
	}

	NntpConnection* connection = nullptr;
	time_t waitStart = Util::CurrentTime();
	while (!connection && !IsStopped() && Util::CurrentTime() - waitStart <= ConnectionWaitSec)
	{
		connection = g_ServerPool->GetConnection(wanted->GetNormLevel(), wanted, nullptr);
		if (!connection)
		{
			Util::Sleep(50);
		}
	}
	if (!connection)
	{
		// none free (the download holds them all: it's no evidence) or the
		// server is down: blocked after failing to connect, or blocked within
		// the last minute (its block ran out, and the download's own retries
		// hold its connections)
		time_t blockTime = wanted->GetBlockTime();
		result.Unreachable = nzbget_rs_tpr_dupe_probe_is_server_recently_blocked(
			g_ServerPool->IsServerBlocked(wanted) ? 1 : 0,
			(int64)blockTime,
			(int64)Util::CurrentTime(),
			RecentBlockSec) != 0;
		return false;
	}

	// a connection of the same server group may have been handed out instead
	NewsServer* server = connection->GetNewsServer();
	bool fresh = probed.insert(server->GetId()).second;
	bool probedServer = false;
	if (fresh)
	{
		{
			Guard guard(g_probeMutex);
			m_connection = connection;
		}

		int remaining = (int)m_samples.size();
		for (size_t index = 0; index < m_samples.size(); index++)
		{
			const Sample& sample = m_samples[index];
			if (IsStopped())
			{
				break;
			}
			remaining--;
			if (m_found[index])
			{
				continue;	// another server has it
			}

			if (!connection->Connect())
			{
				int outUnreachable = 0;
				nzbget_rs_tpr_dupe_probe_apply_connect_failure(
					result.Unknown, remaining, result.Exists, result.Missing,
					&result.Unknown, &outUnreachable);
				result.Unreachable = outUnreachable != 0;
				break;
			}

			if (server->GetJoinGroup())
			{
				const char* joined = nullptr;
				for (const CString& group : *sample.Groups)
				{
					joined = connection->JoinGroup(group);
					if (nzbget_rs_tpr_dupe_probe_is_join_group_ok(joined) != 0)
					{
						break;
					}
				}
				if (!nzbget_rs_tpr_dupe_probe_is_join_group_ok(joined))
				{
					result.Unknown += remaining + 1;
					if (!joined)
					{
						connection->Disconnect();
					}
					break;
				}
			}

			NzbgetRsBuf statCmdBuf = nzbget_rs_dqh_dupe_probe_format_stat_cmd(*sample.MessageId);
			BString<1024> statCmd;
			statCmd.Set(statCmdBuf.data ? statCmdBuf.data : "", (int)statCmdBuf.len);
			nzbget_rs_free(statCmdBuf);
			const char* response = connection->Request(statCmd);
			if (!response)
			{
				connection->Disconnect();
				result.Unknown += remaining + 1;
				break;
			}

			switch (Classify(response))
			{
				case daExists:
					result.Exists++;
					m_found[index] = 1;
					if (m_foundOn.empty())
					{
						NzbgetRsBuf foundBuf = nzbget_rs_tpr_dupe_probe_format_found_on(
							server->GetName(), *sample.MessageId);
						m_foundOn.assign(foundBuf.data ? foundBuf.data : "", foundBuf.len);
						nzbget_rs_free(foundBuf);
					}
					break;
				case daMissing:
					result.Missing++;
					break;
				case daUnknown:
					result.Unknown++;
					break;
			}

			if (!m_countAll &&
				nzbget_rs_tpr_dupe_probe_count_found_samples(
					reinterpret_cast<const uint8_t*>(m_found.data()), m_found.size()) >= MinAliveSamples)
			{
				break;	// alive: no need to ask further
			}
		}
		probedServer = true;
	}

	{
		Guard guard(g_probeMutex);
		m_connection = nullptr;
	}
	if (IsStopped() || connection->GetStatus() == Connection::csCancelled)
	{
		connection->Disconnect();
	}
	g_ServerPool->FreeConnection(connection, true);
	return probedServer;
}

DupeProbe::Verdict DupeProbe::Measure(int limitSec)
{
	// the servers to ask: every active one, one per server group
	Verdict verdict;
	Servers* servers = g_ServerPool->GetServers();
	std::vector<uint8_t> srvActive;
	std::vector<int32_t> srvGroups;
	std::vector<int32_t> srvLevels;
	std::vector<int32_t> srvIds;
	srvActive.reserve(servers->size());
	srvGroups.reserve(servers->size());
	srvLevels.reserve(servers->size());
	srvIds.reserve(servers->size());
	for (NewsServer* server : servers)
	{
		srvActive.push_back(server->GetActive() ? 1 : 0);
		srvGroups.push_back(server->GetGroup());
		srvLevels.push_back(server->GetNormLevel());
		srvIds.push_back(server->GetId());
	}
	std::vector<int32_t> order(servers->size());
	size_t orderLen = nzbget_rs_tpr_dupe_probe_select_server_order(
		srvActive.data(), srvGroups.data(), srvLevels.data(), srvIds.data(),
		servers->size(), order.data(), order.size());
	order.resize(orderLen);
	verdict.ActiveServers = (int)orderLen;

	std::set<int> probed;
	time_t start = Util::CurrentTime();

	for (int serverId : order)
	{
		if (IsStopped() || Util::CurrentTime() - start > limitSec)
		{
			verdict.Finished = false;
			break;
		}

		ServerResult result;
		bool reached = ProbeServer(serverId, probed, result);
		if (result.Unreachable)
		{
			verdict.ActiveServers--;
			verdict.UnreachableServers++;
		}
		if (!reached)
		{
			// not reached (no connection, not asked): no evidence either way
			continue;
		}
		verdict.ReachedServers++;

		verdict.Existing = nzbget_rs_tpr_dupe_probe_count_found_samples(
			reinterpret_cast<const uint8_t*>(m_found.data()), m_found.size());
		verdict.FoundOn = m_foundOn;
		if (m_countAll)
		{
			// every sample is asked of every server until each was found somewhere
			if (verdict.Existing == (int)m_samples.size())
			{
				break;
			}
			continue;
		}
		if (verdict.Existing >= MinAliveSamples)
		{
			break;
		}
		if (nzbget_rs_tpr_dupe_probe_is_server_measured_complete(
			result.Unknown, result.Exists, result.Missing) != 0)
		{
			verdict.MissingServers++;
		}
	}
	if (IsStopped())
	{
		// stopped while asking the last server: its answers are incomplete
		verdict.Finished = false;
	}
	return verdict;
}

void DupeProbe::StartRecheck(int nzbId, std::vector<Sample> samples)
{
	DupeProbe* probe = new DupeProbe(nzbId, std::move(samples));
	probe->m_countAll = true;
	probe->m_found.assign(probe->m_samples.size(), 0);
	if (!probe->Register())
	{
		delete probe;
		return;
	}
	probe->SetAutoDestroy(true);
	probe->Thread::Start();
}

void DupeProbe::Recheck()
{
	Verdict verdict = Measure(RecheckLimitSec);
	if (IsStopped() || !verdict.Finished)
	{
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() != HistoryInfo::hkNzb || historyInfo->GetNzbInfo()->GetId() != m_nzbId)
		{
			continue;
		}
		NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
		int sampled = (int)m_samples.size();
		if (verdict.ReachedServers == 0)
		{
			// no server was asked (every connection busy the whole wait): that
			// says nothing about the articles. It read "0 of N exist; not
			// retrying" and used up the one recheck
			nzbInfo->PrintMessage(Message::mkInfo,
				"Failed articles not rechecked: no server connection was free");
			nzbInfo->GetParameters()->SetParameter("*DupeRecheck", nullptr);
			return;
		}
		bool retry = nzbget_rs_tpr_dupe_probe_should_retry_recheck(verdict.Existing, sampled) != 0;
		// a duplicate queued while the probe ran (up to a minute) downloads in its
		// place: retrying as well downloaded the release twice at once (F22)
		for (NzbInfo* queued : downloadQueue->GetQueue())
		{
			if (retry && DupeCoordinator::SameNameOrKey(queued->GetName(), queued->GetDupeKey(),
				nzbInfo->GetName(), nzbInfo->GetDupeKey()))
			{
				nzbInfo->PrintMessage(Message::mkInfo, "Not retrying failed articles: duplicate %s downloads in its place",
					queued->GetName());
				return;
			}
		}
		nzbInfo->PrintMessage(Message::mkInfo, "%i of %i failed articles exist on the servers%s",
			verdict.Existing, sampled, nzbget_rs_dqh_dupe_probe_recheck_retry_suffix(retry ? 1 : 0));
		if (retry)
		{
			IdList ids = { historyInfo->GetId() };
			downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId, DownloadQueue::eaHistoryRetryFailed, nullptr);
		}
		break;
	}
}

namespace
{
BString<100> Unreachable(const DupeProbe::Verdict& verdict)
{
	BString<100> text;
	NzbgetRsBuf buf = nzbget_rs_tpr_dupe_probe_format_unreachable_suffix(verdict.UnreachableServers);
	if (buf.len > 0 && buf.data)
	{
		text.Set(buf.data, (int)buf.len);
	}
	nzbget_rs_free(buf);
	return text;
}
}

void DupeProbe::Run()
{
	if (m_countAll)
	{
		Recheck();
		Unregister();
		return;
	}

	Verdict verdict = Measure(ProbeLimitSec);
	if (IsStopped())
	{
		// shutting down: never park a download now
		Unregister();
		return;
	}

	int sampled = (int)m_samples.size();
	int outcome = nzbget_rs_tpr_dupe_probe_classify_run_outcome(
		verdict.Dead() ? 1 : 0,
		verdict.Existing,
		MinAliveSamples,
		verdict.Finished ? 1 : 0,
		verdict.ReachedServers);
	if (outcome == 1)
	{
		Note(Message::mkInfo, "Dupe probe: %i of %i sampled articles exist (%i of %i servers answered "
			"definitively%s)%s%s: the posting is dead", verdict.Existing, sampled, verdict.MissingServers,
			verdict.ActiveServers, *Unreachable(verdict),
			nzbget_rs_dqh_dupe_probe_found_on_prefix(verdict.FoundOn.empty() ? 1 : 0),
			verdict.FoundOn.c_str());
		Abandon(verdict.MissingServers, verdict.ActiveServers);
	}
	else if (outcome == 2)
	{
		// the probe stops asking once MinAliveSamples were found: the rest weren't asked
		Note(Message::mkInfo, "Dupe probe: %i of %i sampled articles found (it stops at %i), the first on %s: "
			"not abandoning it", verdict.Existing, sampled, MinAliveSamples, verdict.FoundOn.c_str());
	}
	else if (outcome == 3)
	{
		// no server could be asked (every connection busy): no verdict at all, so
		// the download may be probed again when its next article starts
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo)
		{
			nzbInfo->SetDeadPickProbed(false);
			// (under the queue lock already: not through Note, the lock isn't recursive)
			nzbInfo->PrintMessage(Message::mkDetail, "Dupe probe: no server could be asked, will probe again");
		}
	}
	else if (outcome == 4)
	{
		Note(Message::mkInfo, "Dupe probe: no verdict (%i of %i sampled articles exist, %i of %i servers "
			"answered definitively%s)", verdict.Existing, sampled, verdict.MissingServers, verdict.ActiveServers,
			*Unreachable(verdict));
	}

	Unregister();
}

void DupeProbe::Note(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, sizeof(text), format, args);
	va_end(args);

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (nzbInfo)
	{
		nzbInfo->PrintMessage(kind, "%s", text);
	}
	else
	{
		detail("Dupe probe of download %i: %s", m_nzbId, text);
	}
}

void DupeProbe::Abandon(int missingServers, int activeServers)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	g_QueueCoordinator->FailOverDeadPick(downloadQueue, m_nzbId, (int)m_samples.size(),
		missingServers, activeServers, m_recoveredAtStart);
}
