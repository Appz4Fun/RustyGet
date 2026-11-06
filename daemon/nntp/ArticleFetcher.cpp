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

#include <map>
#include <set>

#include "ArticleFetcher.h"
#include "NntpConnection.h"
#include "NewsServer.h"
#include "ServerPool.h"
#include "Decoder.h"
#include "Log.h"
#include "Options.h"
#include "StatMeter.h"
#include "Util.h"
#include "WorkState.h"
#include "nzbget_rs.h"

bool ArticleFetchLimits::AddRawBytes(int64 bytes)
{
	return nzbget_rs_article_fetch_add_raw_bytes(&m_rawBytes, bytes) != 0;
}

bool ArticleFetchLimits::HeaderWithinLimit(bool declaredRangeKnown) const
{
	return nzbget_rs_article_fetch_header_within_limit(m_rawBytes, declaredRangeKnown ? 1 : 0) != 0;
}

bool ArticleFetchLimits::AddDecodedBytes(int64 bytes, int64 begin, int64 end,
	int64 fileSize)
{
	return nzbget_rs_article_fetch_add_decoded_bytes(&m_decodedBytes, bytes, begin, end, fileSize) != 0;
}

ArticleFetcher::FetchedArticle ArticleFetcher::Fetch(const char* messageId,
	const std::vector<CString>& groups)
{
	FetchedArticle result;
	ServerPool::RawServerList failedServers;
	std::map<NewsServer*, int> transientAttempts;
	std::map<NewsServer*, int> staleConnections;

	int level = 0;
	while (level <= g_ServerPool->GetMaxNormLevel() && !m_stopped)
	{
		while (g_WorkState->GetQuotaReached() && !m_stopped)
		{
			Util::Sleep(100);
		}
		if (m_stopped)
		{
			break;
		}
		// count servers still worth trying on this level; without this check
		// a fully failed level would spin the acquire loop until its timeout
		// (the pool also skips a server of the same group and level as a failed
		// one - the same account - so such a server isn't worth waiting for)
		int eligible = 0;
		std::vector<int> failedIds, failedGroups, failedLevels;
		failedIds.reserve(failedServers.size());
		failedGroups.reserve(failedServers.size());
		failedLevels.reserve(failedServers.size());
		for (NewsServer* failed : failedServers)
		{
			failedIds.push_back(failed->GetId());
			failedGroups.push_back(failed->GetGroup());
			failedLevels.push_back(failed->GetNormLevel());
		}
		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (nzbget_rs_tpr_article_fetcher_is_server_eligible_on_level(
				server->GetActive() ? 1 : 0, server->GetNormLevel(), level,
				server->GetId(), server->GetGroup(),
				failedIds.data(), failedGroups.data(), failedLevels.data(), failedServers.size()) != 0)
			{
				eligible++;
			}
		}
		if (eligible == 0)
		{
			level++;
			continue;
		}

		// the pool is non-blocking: poll until a connection frees up. A pool the
		// queue's downloads keep busy says nothing about the article (B76: Las
		// Azules S02E06 4440 - each fetch gave up after ArticleTimeout, so whole
		// duplicates counted as missing): the fetch waits until it is stopped,
		// and the repair's watchdog doesn't count the wait as lack of progress
		// (it ends a pass that waited StreamRepairController::MaxConnectionWaitSec)
		NntpConnection* connection = nullptr;
		if (m_waitCounter)
		{
			(*m_waitCounter)++;
		}
		while (!connection && !m_stopped)
		{
			if (g_WorkState->GetQuotaReached())
			{
				Util::Sleep(100);
				continue;
			}
			connection = g_ServerPool->GetConnection(level, nullptr, &failedServers);
			if (!connection)
			{
				Util::Sleep(5);
			}
		}
		if (m_waitCounter)
		{
			(*m_waitCounter)--;
		}

		if (!connection)
		{
			level++;
			continue;
		}

		bool stopped = false;
		{
			Guard guard(m_connectionMutex);
			m_connection = connection;
			stopped = m_stopped;
		}
		if (stopped)
		{
			ReleaseConnection(connection, true);
			break;
		}

		bool reused = connection->GetStatus() == Connection::csConnected;
		result = FetchFromConnection(connection, messageId, groups);

		NewsServer* server = connection->GetNewsServer();
		ReleaseConnection(connection, true);

		if (nzbget_rs_dqh_article_fetcher_should_clear_unreachable(
			result.Success ? 1 : 0, result.Transient ? 1 : 0) != 0)
		{
			SetServerUnreachable(server, false);	// it answered
		}
		if (result.Success)
		{
			return result;
		}
		if (result.Retry)
		{
			while (g_WorkState->GetQuotaReached() && !m_stopped)
			{
				Util::Sleep(100);
			}
			continue;
		}
		if (nzbget_rs_dqh_article_fetcher_can_attempt_transient_retry(
			result.Transient ? 1 : 0, ServerUnreachable(server) ? 1 : 0) != 0)
		{
			// an idle pooled connection the server already closed (a provider
			// dropping every connection of the account) says nothing about the
			// server, and it is closed now, so it can't fail this way again:
			// it doesn't use up an attempt
			if (reused && nzbget_rs_tpr_article_fetcher_is_reused_stale_within_max(
				reused ? 1 : 0, ++staleConnections[server], server->GetMaxConnections()) != 0)
			{
				continue;
			}
			int attempt = ++transientAttempts[server];
			if (nzbget_rs_dqh_article_fetcher_has_more_transient_attempts(
				attempt, MaxTransientAttempts) != 0)
			{
				if (WaitBeforeRetry(TransientRetryDelay(attempt)))
				{
					continue;	// ask this server again on a fresh connection
				}
				break;
			}
			SetServerUnreachable(server, true);
		}

		// this server could not supply the article; try the remaining servers
		// of this level, then the next level
		failedServers.push_back(server);
	}

	return result;
}

namespace
{
	Mutex g_unreachableMutex;
	std::set<int> g_unreachableServers;	// news server ids
}

bool ArticleFetcher::ServerUnreachable(NewsServer* server)
{
	Guard guard(g_unreachableMutex);
	return g_unreachableServers.count(server->GetId()) > 0;
}

void ArticleFetcher::SetServerUnreachable(NewsServer* server, bool unreachable)
{
	Guard guard(g_unreachableMutex);
	if (unreachable)
	{
		g_unreachableServers.insert(server->GetId());
	}
	else
	{
		g_unreachableServers.erase(server->GetId());
	}
}

bool ArticleFetcher::WaitBeforeRetry(int seconds)
{
	for (int waited = 0;
		nzbget_rs_dqh_article_fetcher_should_keep_waiting_retry(
			waited, seconds, m_stopped ? 1 : 0) != 0;
		waited++)
	{
		Util::Sleep(100);
	}
	return !m_stopped;
}

void ArticleFetcher::Stop()
{
	m_stopped = true;
	Guard guard(m_connectionMutex);
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
}

void ArticleFetcher::ReleaseConnection(NntpConnection* connection, bool keepConnected)
{
	Guard guard(m_connectionMutex);
	AddServerStats(connection);
	if (nzbget_rs_dqh_article_fetcher_should_disconnect_on_release(
		keepConnected ? 1 : 0, (int)connection->GetStatus(), (int)Connection::csCancelled) != 0)
	{
		connection->Disconnect();
	}
	if (m_connection == connection)
	{
		m_connection = nullptr;
	}
	g_ServerPool->FreeConnection(connection, true);
}

void ArticleFetcher::AddServerStats(NntpConnection* connection)
{
	int bytesRead = connection->FetchTotalBytesRead();
	if (bytesRead <= 0)
	{
		return;
	}

	ServerVolume::Stats stats;
	stats.bytes = Util::SafeIntCast<int, uint32>(bytesRead);
	stats.articles.failed = 0;
	stats.articles.success = 0;
	g_StatMeter->AddServerStats(stats, connection->GetNewsServer()->GetId());
}

ArticleFetcher::FetchedArticle ArticleFetcher::FetchFromConnection(NntpConnection* connection,
	const char* messageId, const std::vector<CString>& groups)
{
	FetchedArticle result;
	if (m_stopped)
	{
		return result;
	}

	if (!connection->Connect())
	{
		detail("Stream repair: could not connect to %s", connection->GetNewsServer()->GetName());
		result.Transient = true;
		return result;
	}

	if (connection->GetNewsServer()->GetJoinGroup())
	{
		const char* response = nullptr;
		for (const CString& group : groups)
		{
			response = connection->JoinGroup(group);
			if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) != 0)
			{
				break;
			}
		}
		if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) == 0)
		{
			detail("Stream repair: could not join groups on %s for %s",
				connection->GetNewsServer()->GetName(), messageId);
			result.Transient = nzbget_rs_dqh_article_fetcher_is_join_group_failure_transient(
				response ? 1 : 0) != 0;
			if (result.Transient)
			{
				connection->Disconnect();
			}
			return result;
		}
	}

	NzbgetRsBuf bodyCmdBuf = nzbget_rs_dqh_article_fetcher_format_body_cmd(messageId);
	BString<1024> bodyCmd;
	bodyCmd.Set(bodyCmdBuf.data ? bodyCmdBuf.data : "", (int)bodyCmdBuf.len);
	nzbget_rs_free(bodyCmdBuf);
	const char* response = connection->Request(bodyCmd);
	if (!response)
	{
		// no reply at all (a timeout, or a pooled connection the server
		// already closed): not an answer about the article
		detail("Stream repair: no response from %s for %s", connection->GetNewsServer()->GetName(), messageId);
		connection->Disconnect();
		result.Transient = true;
		return result;
	}
	if (nzbget_rs_nntp_article_dl_is_join_group_ok(response) == 0)
	{
		detail("Stream repair: article %s not available on %s",
			messageId, connection->GetNewsServer()->GetName());
		return result;
	}

	Decoder decoder;
	decoder.Clear();
	// always: a repaired range is credited to health and may complete a
	// release without a par-check, so a corrupt duplicate article must never
	// be taken for good bytes (option CrcCheck=no only spares the CPU of the
	// regular download, whose damage par-check can still find)
	decoder.SetCrcCheck(true);
	decoder.SetRawMode(false);

	int readChunk = g_Options->GetArticleReadChunkSize();
	CharBuffer recvBuf(readChunk + 128);	// (decoder slack, see ArticleDownloader)
	ArticleFetchLimits limits;

	while (!m_stopped && !decoder.GetEof())
	{
		if (g_WorkState->GetQuotaReached())
		{
			// A BODY response cannot be parked safely for an arbitrary quota
			// pause. Drop this partial response and retry the same source after
			// quota resumes; do not classify it as a source failure.
			connection->Disconnect();
			result.Retry = true;
			return result;
		}
		while (nzbget_rs_tpr_article_fetcher_should_throttle_bandwidth(
			m_stopped ? 1 : 0,
			g_WorkState->GetQuotaReached() ? 1 : 0,
			g_WorkState->GetSpeedLimit(),
			g_StatMeter->CalcCurrentDownloadSpeed(),
			g_StatMeter->CalcMomentaryDownloadSpeed()) != 0)
		{
			Util::Sleep(10);
		}
		if (g_WorkState->GetQuotaReached())
		{
			connection->Disconnect();
			result.Retry = true;
			return result;
		}
		if (m_stopped)
		{
			break;
		}

		char* buffer;
		int len;
		connection->ReadBuffer(&buffer, &len);
		if (len == 0)
		{
			len = connection->TryRecv(recvBuf, readChunk);
			buffer = recvBuf;
		}

		if (len <= 0)
		{
			// timeout or connection closed mid-article: the connection state
			// is unusable for further commands
			detail("Stream repair: connection to %s lost while fetching %s",
				connection->GetNewsServer()->GetName(), messageId);
			connection->Disconnect();
			result = FetchedArticle();
			result.Transient = true;
			return result;
		}

		g_StatMeter->AddSpeedReading(len);
		if (!limits.AddRawBytes(len))
		{
			detail("Stream repair: article %s from %s exceeded the wire-size limit",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}

		int decodedLen = decoder.DecodeBuffer(buffer, len);
		bool declaredRangeKnown = nzbget_rs_tpr_article_fetcher_is_declared_range_known(
			decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize()) != 0;
		if (!limits.HeaderWithinLimit(declaredRangeKnown))
		{
			detail("Stream repair: article %s from %s exceeded the yEnc-header limit",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}
		if (declaredRangeKnown && !limits.AddDecodedBytes(decodedLen,
			decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize()))
		{
			detail("Stream repair: article %s from %s exceeded its declared decoded range",
				messageId, connection->GetNewsServer()->GetName());
			connection->Disconnect();
			return result;
		}
		if (decodedLen > 0)
		{
			result.Data.insert(result.Data.end(), buffer, buffer + decodedLen);
		}
	}

	if (m_stopped)
	{
		// stopped mid-body: the response was not fully drained, so the
		// connection cannot be reused by the next pool consumer
		connection->Disconnect();
		return result;
	}

	int64 decOffset = 0;
	int64 decFileSize = 0;
	int decSuccess = 0;
	if (!nzbget_rs_nntp_article_fetcher_eval_decoded_yenc(
		(int)decoder.Check(), (int)Decoder::dsFinished,
		(int)decoder.GetFormat(), (int)Decoder::efYenc,
		decoder.GetBeginPos(), decoder.GetEndPos(), decoder.GetSize(),
		result.Data.size(), &decOffset, &decFileSize, &decSuccess))
	{
		detail("Stream repair: article %s from %s failed decoding",
			messageId, connection->GetNewsServer()->GetName());
		return result;
	}
	result.Offset = decOffset;
	result.FileSize = decFileSize;
	result.Success = decSuccess != 0;

	return result;
}

ArticleBatchFetcher::ArticleBatchFetcher()
{
	InitRsBatch();
}

ArticleBatchFetcher::ArticleBatchFetcher(FetchFunc fetchFunc) :
	m_fetchFunc(std::move(fetchFunc))
{
	InitRsBatch();
}

ArticleBatchFetcher::~ArticleBatchFetcher()
{
	nzbget_rs_article_batch_fetcher_free(m_rsBatch);
	m_rsBatch = nullptr;
}

void ArticleBatchFetcher::InitRsBatch()
{
	m_rsBatch = nzbget_rs_article_batch_fetcher_new(
		this,
		[](void* raw, size_t workerIdx, const char* msgId,
			const char* const* groups, size_t groupCount,
			NzbgetRsBuf* outData, int64_t* outOffset, int64_t* outFileSize,
			int* outSuccess, int* outRetry, int* outTransient) -> int {
			auto* self = static_cast<ArticleBatchFetcher*>(raw);
			ArticleFetcher::FetchedArticle fetched;
			try
			{
				auto grpVec = std::make_shared<std::vector<CString>>();
				grpVec->reserve(groupCount);
				for (size_t i = 0; i < groupCount; i++)
				{
					grpVec->emplace_back(groups[i] ? groups[i] : "");
				}
				if (self->m_fetchFunc)
				{
					Request req{CString(msgId ? msgId : ""), grpVec};
					fetched = self->m_fetchFunc(req);
				}
				else
				{
					ArticleFetcher* fetcher = &self->m_inlineFetcher;
					if (workerIdx != SIZE_MAX)
					{
						Guard guard(self->m_mutex);
						if (workerIdx < self->m_workerFetchers.size())
						{
							fetcher = self->m_workerFetchers[workerIdx].get();
						}
					}
					fetched = fetcher->Fetch(msgId ? msgId : "", *grpVec);
				}
			}
			catch (const std::exception& e)
			{
				detail("Batch fetch of %s failed: %s", msgId ? msgId : "", e.what());
				return 0;
			}
			catch (...)
			{
				detail("Batch fetch of %s failed", msgId ? msgId : "");
				return 0;
			}
			*outData = nzbget_rs_alloc_buf(fetched.Data.data(), fetched.Data.size());
			*outOffset = fetched.Offset;
			*outFileSize = fetched.FileSize;
			*outSuccess = fetched.Success ? 1 : 0;
			*outRetry = fetched.Retry ? 1 : 0;
			*outTransient = fetched.Transient ? 1 : 0;
			return 1;
		},
		[](void* raw) {
			auto* self = static_cast<ArticleBatchFetcher*>(raw);
			try
			{
				Guard guard(self->m_mutex);
				self->m_inlineFetcher.Stop();
				for (auto& wf : self->m_workerFetchers)
				{
					wf->Stop();
				}
			}
			catch (...)
			{
			}
		});
}

void ArticleBatchFetcher::SetWorkerCount(int workerCount)
{
	m_workerCount = workerCount;
	nzbget_rs_article_batch_fetcher_set_worker_count(m_rsBatch, workerCount);
}

void ArticleBatchFetcher::SetWaitCounter(std::atomic<int>* counter)
{
	Guard guard(m_mutex);
	m_waitCounter = counter;
	m_inlineFetcher.SetWaitCounter(counter);
	for (auto& wf : m_workerFetchers)
	{
		wf->SetWaitCounter(counter);
	}
}

int ArticleBatchFetcher::GetWindowParts() const
{
	return nzbget_rs_article_batch_fetcher_get_window_parts(m_rsBatch);
}

void ArticleBatchFetcher::EnsureWorkerFetchers()
{
	Guard guard(m_mutex);
	while ((int)m_workerFetchers.size() < m_workerCount)
	{
		m_workerFetchers.push_back(std::make_unique<ArticleFetcher>());
		m_workerFetchers.back()->SetWaitCounter(m_waitCounter);
	}
}

void ArticleBatchFetcher::Begin(std::vector<Request> requests)
{
	EnsureWorkerFetchers();
	nzbget_rs_article_batch_fetcher_begin(
		m_rsBatch, requests.size(), &requests,
		[](void* raw, size_t idx, const char** outMidPtr, size_t* outMidLen) -> size_t {
			auto* reqs = static_cast<std::vector<Request>*>(raw);
			const Request& r = (*reqs)[idx];
			*outMidPtr = r.MessageId ? *r.MessageId : "";
			*outMidLen = r.MessageId.Length();
			return r.Groups ? r.Groups->size() : 0;
		},
		[](void* raw, size_t reqIdx, size_t grpIdx, const char** outGrpPtr, size_t* outGrpLen) {
			auto* reqs = static_cast<std::vector<Request>*>(raw);
			const CString& g = (*(*reqs)[reqIdx].Groups)[grpIdx];
			*outGrpPtr = g ? *g : "";
			*outGrpLen = g.Length();
		});
}

bool ArticleBatchFetcher::Next(ArticleFetcher::FetchedArticle& result)
{
	NzbgetRsBuf dataBuf{nullptr, 0};
	int64_t offset = 0;
	int64_t fileSize = 0;
	int success = 0;
	int retry = 0;
	int transient = 0;
	if (!nzbget_rs_article_batch_fetcher_next(
		m_rsBatch, &dataBuf, &offset, &fileSize, &success, &retry, &transient))
	{
		return false;
	}
	result.Data.assign(dataBuf.data ? dataBuf.data : "", (dataBuf.data ? dataBuf.data : "") + dataBuf.len);
	nzbget_rs_free(dataBuf);
	result.Offset = offset;
	result.FileSize = fileSize;
	result.Success = success != 0;
	result.Retry = retry != 0;
	result.Transient = transient != 0;
	return true;
}

void ArticleBatchFetcher::CancelRemaining()
{
	nzbget_rs_article_batch_fetcher_cancel_remaining(m_rsBatch);
}

void ArticleBatchFetcher::Stop()
{
	nzbget_rs_article_batch_fetcher_stop(m_rsBatch);
}
