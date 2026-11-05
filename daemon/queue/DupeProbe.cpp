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
		if (probe->m_nzbId == nzbId && !probe->m_countAll)
		{
			return std::max(0, (int)(Util::CurrentTime() - probe->m_started));
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
			if (position >= offset + count)
			{
				offset += count;
				continue;
			}
			size_t index = position - offset;

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
		result.Unreachable = g_ServerPool->IsServerBlocked(wanted) ||
			(blockTime > 0 && Util::CurrentTime() - blockTime < RecentBlockSec);
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
				result.Unknown += remaining + 1;
				result.Unreachable = result.Exists + result.Missing == 0;
				break;
			}

			if (server->GetJoinGroup())
			{
				const char* joined = nullptr;
				for (const CString& group : *sample.Groups)
				{
					joined = connection->JoinGroup(group);
					if (joined && !strncmp(joined, "2", 1))
					{
						break;
					}
				}
				if (!joined || strncmp(joined, "2", 1))
				{
					result.Unknown += remaining + 1;
					if (!joined)
					{
						connection->Disconnect();
					}
					break;
				}
			}

			BString<1024> statCmd("STAT %s\r\n", *sample.MessageId);
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
						m_foundOn = std::string(server->GetName()) + " (" + *sample.MessageId + ")";
					}
					break;
				case daMissing:
					result.Missing++;
					break;
				case daUnknown:
					result.Unknown++;
					break;
			}

			int foundCount = (int)std::count(m_found.begin(), m_found.end(), 1);
			if (!m_countAll && foundCount >= MinAliveSamples)
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
	size_t orderLen = nzbget_rs_dupe_probe_select_server_order(
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

		verdict.Existing = (int)std::count(m_found.begin(), m_found.end(), 1);
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
		if (result.Unknown == 0 && result.Exists + result.Missing > 0)
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
		const char* duplicateQueuedName = "";
		if (verdict.ReachedServers > 0 && verdict.Existing * 2 >= sampled)
		{
			for (NzbInfo* queued : downloadQueue->GetQueue())
			{
				if (DupeCoordinator::SameNameOrKey(queued->GetName(), queued->GetDupeKey(),
					nzbInfo->GetName(), nzbInfo->GetDupeKey()))
				{
					duplicateQueuedName = queued->GetName();
					break;
				}
			}
		}

		int outcome = 0;
		NzbgetRsBuf msgBuf = nzbget_rs_dupe_probe_eval_recheck_outcome(
			verdict.Existing, sampled, verdict.ReachedServers, duplicateQueuedName, &outcome);
		std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
		nzbget_rs_free(msgBuf);

		nzbInfo->PrintMessage(Message::mkInfo, "%s", message.c_str());
		if (outcome == 1)
		{
			nzbInfo->GetParameters()->SetParameter("*DupeRecheck", nullptr);
			return;
		}
		if (outcome == 2)
		{
			return;
		}
		if (outcome == 3)
		{
			IdList ids = { historyInfo->GetId() };
			downloadQueue->EditList(&ids, nullptr, DownloadQueue::mmId, DownloadQueue::eaHistoryRetryFailed, nullptr);
		}
		break;
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
	int outcome = 0;
	NzbgetRsBuf msgBuf = nzbget_rs_dupe_probe_eval_run_outcome(
		verdict.Existing,
		sampled,
		verdict.MissingServers,
		verdict.ActiveServers,
		verdict.UnreachableServers,
		verdict.ReachedServers,
		verdict.Finished ? 1 : 0,
		verdict.FoundOn.c_str(),
		&outcome);
	std::string message(msgBuf.data ? msgBuf.data : "", msgBuf.len);
	nzbget_rs_free(msgBuf);

	if (outcome == 1)
	{
		Note(Message::mkInfo, "%s", message.c_str());
		Abandon(verdict.MissingServers, verdict.ActiveServers);
	}
	else if (outcome == 2)
	{
		Note(Message::mkInfo, "%s", message.c_str());
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
			nzbInfo->PrintMessage(Message::mkDetail, "%s", message.c_str());
		}
	}
	else if (outcome == 4)
	{
		Note(Message::mkInfo, "%s", message.c_str());
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
