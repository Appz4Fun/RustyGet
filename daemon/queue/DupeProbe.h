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


#ifndef DUPEPROBE_H
#define DUPEPROBE_H

#include "NString.h"
#include "Log.h"
#include <functional>
#include <memory>
#include <set>
#include <string>
#include <vector>

class NntpConnection;
class FileInfo;

/*
 * Early dead-posting check for option <HealthCheck> value "dupe".
 *
 * A posting whose articles exist on no server is only noticed by failing
 * them, and a large one needs thousands of failures before its health drops
 * below critical. When a duplicate waits in history, the download is probed
 * instead as soon as it starts: a handful of articles spread across the whole
 * posting are checked (STAT) on every active server. Only when none of them
 * exists anywhere, and enough servers answered definitively, is the
 * download abandoned for the duplicate. A posting with any article alive is
 * never abandoned here: the regular health check handles partial damage.
 */
class DupeProbe : public Thread
{
public:
	struct Sample
	{
		CString MessageId;
		std::shared_ptr<std::vector<CString>> Groups;
	};

	enum EAnswer
	{
		daExists,
		daMissing,
		daUnknown	// no answer, an error, an authentication problem: no evidence
	};

	// articles checked per server
	static constexpr int SampleCount = 10;
	// postings of fewer articles than this are not probed
	static constexpr int MinArticles = 4;
	// samples that must exist somewhere for a download to count as alive: a single
	// stray article doesn't keep a dead posting going (B45)
	static constexpr int MinAliveSamples = 2;
	// a server blocked this recently counts as down for the verdict
	static constexpr int RecentBlockSec = 60;

	/* indexes of <count> samples spread evenly over <total> articles (at the
	 * middle of each of <count> equal parts) */
	static std::vector<size_t> SampleIndexes(size_t total, int count);

	/* maps a reply to STAT: 2xx exists; "no such article" replies (41x, 42x,
	 * 43x - 430 - and 451, which some providers use) are definitive; anything
	 * else is no evidence */
	static EAnswer Classify(const char* response);

	/* the verdict: fewer than MinAliveSamples sampled articles exist anywhere,
	 * and every active server answered every sample it was asked definitively
	 * (<definitiveServers>). A server that didn't (busy, errors) may be the one
	 * that has the posting: with 5 of them enough, a posting only one provider
	 * keeps read dead whenever that provider was busy (Velvet Underground 7913:
	 * "0 of 10 exist, 7 of 9 servers answered" with 48% of it arriving) */
	static bool IsDead(int existing, int definitiveServers, int activeServers);

	/* samples spread over the articles of <files>, treated as one run;
	 * <loadArticles> loads the article list of a file not started yet */
	static std::vector<Sample> SamplesOf(const std::vector<FileInfo*>& files,
		const std::function<void(FileInfo*)>& loadArticles = nullptr);

	/* starts a probe of <samples> for download <nzbId>; the thread destroys itself.
	 * <recoveredAtStart>: the download's count of articles borrowed from duplicates
	 * when it started - those prove nothing about its own posting */
	static void Start(int nzbId, std::vector<Sample> samples, int recoveredAtStart = 0);

	struct Verdict
	{
		int Existing = 0;			// samples found on some server
		int MissingServers = 0;		// servers that answered every sample asked definitively
		std::string FoundOn;		// where the first sample found was, for the log
		int ActiveServers = 0;		// those that count: the unreachable ones are left out
		int UnreachableServers = 0;
		int ReachedServers = 0;	// servers asked at all (a free connection was had)
		bool Finished = true;
		bool Dead() const { return Finished && IsDead(Existing, MissingServers, ActiveServers); }
	};

	/* the same check in the calling thread, within <limitSec> (a duplicate
	 * is checked this way before stream repair reads from it) */
	static Verdict Check(std::vector<Sample> samples, int limitSec);

	/*
	 * A download that failed with articles missing: <samples> of its failed
	 * articles are asked of every server. When most of them exist after all (a
	 * timeout or a dropped connection lost them, as ArticleRetries=0 makes
	 * likely), its failed articles are retried once ("Retry failed articles").
	 * Runs in its own thread; the download is in history by id <nzbId>.
	 */
	static void StartRecheck(int nzbId, std::vector<Sample> samples);
	// the most failed articles asked of the servers, and the time for it
	static constexpr int RecheckSampleCount = 10;
	static constexpr int RecheckLimitSec = 60;

	/* shutdown: cancels running probes and refuses new ones; WaitAll() returns
	 * when they ended. Reset() allows probes again (a reload's new coordinator) */
	static void StopAll();
	/* seconds a dead-pick probe of download <nzbId> has been running (its verdict
	 * comes first), or -1 when none runs */
	static int ProbingFor(int nzbId);
	static void WaitAll();
	static void Reset();

protected:
	void Run() override;

private:
	DupeProbe(int nzbId, std::vector<Sample> samples) :
		m_nzbId(nzbId), m_samples(std::move(samples)), m_found(m_samples.size(), 0) {}

	struct ServerResult
	{
		int Exists = 0;
		int Missing = 0;
		int Unknown = 0;
		// down (blocked by the pool, or its connection failed): it can't deliver
		// the posting either, and is left out of the verdict. A busy server isn't:
		// it may be the one that has the posting
		bool Unreachable = false;
	};

	int m_nzbId;
	time_t m_started = 0;
	int m_recoveredAtStart = 0;
	std::vector<Sample> m_samples;
	// recheck mode: every sample is asked of every server until found somewhere
	bool m_countAll = false;
	// samples found on some server: a later server isn't asked them again
	std::vector<char> m_found;
	std::string m_foundOn;
	NntpConnection* m_connection = nullptr;

	bool ProbeServer(int serverId, std::set<int>& probed, ServerResult& result);
	/* writes to the download's own log (seen in the web interface even when
	 * detail messages go nowhere); detail() if the download is gone */
	void Note(Message::EKind kind, const char* format, ...);
	Verdict Measure(int limitSec);
	void Recheck();
	/* false once StopAll ran: the probe must not run */
	bool Register();
	void Unregister();
	void Cancel();
	void Abandon(int missingServers, int activeServers);
};

#include <algorithm>
#include <atomic>
#include <cstdarg>
#include "DownloadInfo.h"
#include "QueueCoordinator.h"
#include "DupeCoordinator.h"
#include "ServerPool.h"
#include "Util.h"
#include "nzbget_rs.h"

namespace DupeProbeDetail
{
	inline constexpr int ConnectionWaitSec = 10;
	inline constexpr int ProbeLimitSec = 90;

	inline Mutex g_probeMutex;
	inline std::set<DupeProbe*> g_probes;
	inline std::atomic<int> g_probeCount{0};
	inline bool g_probesStopping = false;
}

inline std::vector<size_t> DupeProbe::SampleIndexes(size_t total, int count)
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

inline DupeProbe::EAnswer DupeProbe::Classify(const char* response)
{
	return (EAnswer)nzbget_rs_dupe_probe_classify(response);
}

inline bool DupeProbe::IsDead(int existing, int definitiveServers, int activeServers)
{
	return nzbget_rs_dupe_probe_is_dead(existing, definitiveServers, activeServers) != 0;
}

inline void DupeProbe::Start(int nzbId, std::vector<Sample> samples, int recoveredAtStart)
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

inline DupeProbe::Verdict DupeProbe::Check(std::vector<Sample> samples, int limitSec)
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

inline void DupeProbe::Reset()
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	g_probesStopping = false;
}

inline int DupeProbe::ProbingFor(int nzbId)
{
	using namespace DupeProbeDetail;
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

inline bool DupeProbe::Register()
{
	using namespace DupeProbeDetail;
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

inline void DupeProbe::Unregister()
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	g_probes.erase(this);
	g_probeCount--;
}

inline std::vector<DupeProbe::Sample> DupeProbe::SamplesOf(const std::vector<FileInfo*>& files,
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

inline void DupeProbe::StopAll()
{
	using namespace DupeProbeDetail;
	Guard guard(g_probeMutex);
	g_probesStopping = true;
	for (DupeProbe* probe : g_probes)
	{
		probe->Thread::Stop();
		probe->Cancel();
	}
}

inline void DupeProbe::WaitAll()
{
	using namespace DupeProbeDetail;
	while (g_probeCount > 0)
	{
		Util::Sleep(20);
	}
}

inline void DupeProbe::Cancel()
{
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
}

inline bool DupeProbe::ProbeServer(int serverId, std::set<int>& probed, ServerResult& result)
{
	using namespace DupeProbeDetail;
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
		time_t blockTime = wanted->GetBlockTime();
		result.Unreachable = g_ServerPool->IsServerBlocked(wanted) ||
			(blockTime > 0 && Util::CurrentTime() - blockTime < RecentBlockSec);
		return false;
	}

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
				continue;
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
				break;
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

inline DupeProbe::Verdict DupeProbe::Measure(int limitSec)
{
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
			continue;
		}
		verdict.ReachedServers++;

		verdict.Existing = (int)std::count(m_found.begin(), m_found.end(), 1);
		verdict.FoundOn = m_foundOn;
		if (m_countAll)
		{
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
		verdict.Finished = false;
	}
	return verdict;
}

inline void DupeProbe::StartRecheck(int nzbId, std::vector<Sample> samples)
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

inline void DupeProbe::Recheck()
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

inline void DupeProbe::Run()
{
	using namespace DupeProbeDetail;
	if (m_countAll)
	{
		Recheck();
		Unregister();
		return;
	}

	Verdict verdict = Measure(ProbeLimitSec);
	if (IsStopped())
	{
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
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo)
		{
			nzbInfo->SetDeadPickProbed(false);
			nzbInfo->PrintMessage(Message::mkDetail, "%s", message.c_str());
		}
	}
	else if (outcome == 4)
	{
		Note(Message::mkInfo, "%s", message.c_str());
	}

	Unregister();
}

inline void DupeProbe::Note(Message::EKind kind, const char* format, ...)
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

inline void DupeProbe::Abandon(int missingServers, int activeServers)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	g_QueueCoordinator->FailOverDeadPick(downloadQueue, m_nzbId, (int)m_samples.size(),
		missingServers, activeServers, m_recoveredAtStart);
}

#endif
