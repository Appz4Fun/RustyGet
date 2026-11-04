/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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
#include "ArticleDownloader.h"
#include "ArticleWriter.h"
#include "Decoder.h"
#include "Log.h"
#include "Options.h"
#include "WorkState.h"
#include "ServerPool.h"
#include "StatMeter.h"
#include "Util.h"
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

ArticleDownloader::ArticleDownloader()
	: m_fileInfo(nullptr)
	, m_articleInfo(nullptr)
	, m_writingStarted(false)
{
	debug("Creating ArticleDownloader");

	SetLastUpdateTimeNow();
}

ArticleDownloader::~ArticleDownloader()
{
	debug("Destroying ArticleDownloader");

#ifndef DISABLE_TLS
	OpenSSL::StopSSLThread();
#endif
}

void ArticleDownloader::SetInfoName(const char* infoName)
{
	m_infoName = infoName;
	m_articleWriter.SetInfoName(m_infoName);
}

/*
 * How server management (for one particular article) works:
	- there is a list of failed servers which is initially empty;
	- level is initially 0;

	<loop>
		- request a connection from server pool for current level;
		  Exception: this step is skipped for the very first download attempt, because a
		  level-0 connection is initially passed from queue manager;
		- try to download from server;
		- if connection to server cannot be established or download fails due to interrupted connection,
		  try again (as many times as needed without limit) the same server until connection is OK;
		- if download fails with error "Not-Found" (article or group not found) or with CRC error,
		  add the server to failed server list;
		- if download fails with general failure error (article incomplete, other unknown error
		  codes), try the same server again as many times as defined by option <ArticleRetries>;
		  if all attempts fail, add the server to failed server list;
		- if all servers from current level were tried, increase level;
		- if all servers from all levels were tried, break the loop with failure status.
	<end-loop>
*/
void ArticleDownloader::Run()
{
	debug("Entering ArticleDownloader-loop");

	SetStatus(adRunning);

	m_articleWriter.SetFileInfo(m_fileInfo);
	m_articleWriter.SetArticleInfo(m_articleInfo);
	m_articleWriter.Prepare();

	EStatus status = adFailed;
#ifdef NZBGET_USE_RUST
	int retries = nzbget_rs_rrd_article_dl_resolve_initial_retries(g_Options->GetArticleRetries());
#else
	int retries = g_Options->GetArticleRetries() > 0 ? g_Options->GetArticleRetries() : 1;
#endif
	int remainedRetries = retries;
	ServerPool::RawServerList failedServers;
	failedServers.reserve(g_ServerPool->GetServers()->size());
	NewsServer* wantServer = nullptr;
	NewsServer* lastServer = nullptr;
	int level = 0;
	int serverConfigGeneration = g_ServerPool->GetGeneration();
	bool force = m_fileInfo->GetNzbInfo()->GetForcePriority();

	while (!IsStopped())
	{
		status = adFailed;

		SetStatus(adWaiting);
#ifdef NZBGET_USE_RUST
		while (nzbget_rs_aws_article_dl_should_wait_for_connection(
			m_connection ? 1 : 0, IsStopped() ? 1 : 0, serverConfigGeneration, g_ServerPool->GetGeneration()) != 0)
#else
		while (!m_connection && !(IsStopped() || serverConfigGeneration != g_ServerPool->GetGeneration()))
#endif
		{
			m_connection = g_ServerPool->GetConnection(level, wantServer, &failedServers);
			Util::Sleep(5);
		}
		SetLastUpdateTimeNow();
		SetStatus(adRunning);

#ifdef NZBGET_USE_RUST
		if (nzbget_rs_dad_article_dl_should_abort_for_work_state(
			IsStopped() ? 1 : 0,
			g_WorkState->GetPauseDownload() ? 1 : 0,
			g_WorkState->GetQuotaReached() ? 1 : 0,
			force ? 1 : 0,
			g_WorkState->GetDownloadHeld() ? 1 : 0,
			m_fileInfo->GetExtraPriority() ? 1 : 0,
			serverConfigGeneration != g_ServerPool->GetGeneration() ? 1 : 0) != 0)
#else
		if (IsStopped() || ((g_WorkState->GetPauseDownload() || g_WorkState->GetQuotaReached()) && !force) ||
			(g_WorkState->GetDownloadHeld() && !m_fileInfo->GetExtraPriority()) ||
			serverConfigGeneration != g_ServerPool->GetGeneration())
#endif
		{
			status = adRetry;
			break;
		}

		lastServer = m_connection->GetNewsServer();
		level = lastServer->GetNormLevel();

		m_connection->SetSuppressErrors(false);

#ifndef DISABLE_TLS
		m_connection->SetCertVerifLevel(lastServer->GetCertVerificationLevel());
#endif

		m_connectionName.Format("%s (%s)",
			m_connection->GetNewsServer()->GetName(), m_connection->GetHost());

		// check server retention
		// a file without a date (none in the nzb) has time 0: its age isn't known,
		// and counting from 1970 failed every article on a server with a retention
#ifdef NZBGET_USE_RUST
		int fileAgeDays = 0;
		bool retentionFailure = nzbget_rs_dur_article_dl_is_retention_failure(
			m_connection->GetNewsServer()->GetRetention(),
			(int64)m_fileInfo->GetTime(),
			(int64)Util::CurrentTime(),
			&fileAgeDays) != 0;
		if (retentionFailure)
		{
			detail("Article %s @ %s failed: out of server retention (file age: %i, configured retention: %i)",
				*m_infoName, *m_connectionName, fileAgeDays,
				m_connection->GetNewsServer()->GetRetention());
			status = adFailed;
			FreeConnection(true);
		}
#else
		bool retentionFailure = m_connection->GetNewsServer()->GetRetention() > 0 &&
			m_fileInfo->GetTime() > 0 &&
			(Util::CurrentTime() - m_fileInfo->GetTime()) / 86400 > m_connection->GetNewsServer()->GetRetention();
		if (retentionFailure)
		{
			detail("Article %s @ %s failed: out of server retention (file age: %i, configured retention: %i)",
				*m_infoName, *m_connectionName,
				(int)(Util::CurrentTime() - m_fileInfo->GetTime()) / 86400,
				m_connection->GetNewsServer()->GetRetention());
			status = adFailed;
			FreeConnection(true);
		}
#endif

		if (m_connection && !IsStopped())
		{
			detail("Downloading %s @ %s", *m_infoName, *m_connectionName);
		}

		// test connection
		bool connected = m_connection && m_connection->Connect();
		// a server that refuses the login for good (502) can't serve this article: it
		// counts as tried. Waited for, a server with a broken account stalled every
		// download that had failed on the other servers of its level
#ifdef NZBGET_USE_RUST
		bool authRejected = nzbget_rs_aws_article_dl_eval_auth_rejected(
			connected ? 1 : 0, m_connection ? 1 : 0,
			(m_connection && m_connection->GetAuthRejected()) ? 1 : 0) != 0;
#else
		bool authRejected = !connected && m_connection && m_connection->GetAuthRejected();
#endif
		if (connected && !IsStopped())
		{
			NewsServer* newsServer = m_connection->GetNewsServer();

			// Download article
			status = Download();

#ifdef NZBGET_USE_RUST
			int success = 0, failed = 0;
			if (nzbget_rs_dad_article_dl_classify_attempt_stat(
				(int)status,
				(int)adFinished,
				(int)adFailed,
				(int)adNotFound,
				(int)adCrcError,
				&success,
				&failed) != 0)
			{
				int serverId = newsServer->GetId();
				m_serverStats.StatOp(serverId, success, failed, ServerStatList::soSet);
				ServerVolume::Stats stats;
				stats.bytes = 0;
				stats.articles.failed = failed;
				stats.articles.success = success;
				g_StatMeter->AddServerStats(stats, serverId);
			}
#else
			if (status == adFinished || status == adFailed || status == adNotFound || status == adCrcError)
			{
				int serverId = newsServer->GetId();
				int success = status == adFinished ? 1 : 0;
				int failed = status == adFinished ? 0 : 1;
				m_serverStats.StatOp(serverId, success, failed, ServerStatList::soSet);
				ServerVolume::Stats stats;
				stats.bytes = 0;
				stats.articles.failed = failed;
				stats.articles.success = success;
				g_StatMeter->AddServerStats(stats, serverId);
			}
#endif
		}

		if (m_connection)
		{
			AddServerStats();
		}

#ifdef NZBGET_USE_RUST
		{
			int shouldBreak = 0;
			int statusInt = (int)status;
			int crInt = m_contentRejected ? 1 : 0;
			if (nzbget_rs_cwp_article_dl_handle_content_rejected(
				crInt,
				m_articleInfo->GetDupeFallbackRound(),
				statusInt,
				(int)adFailed,
				remainedRetries,
				&shouldBreak,
				&statusInt,
				&crInt,
				&remainedRetries) != 0)
			{
				FreeConnection(false);
				status = (EStatus)statusInt;
				if (shouldBreak != 0)
				{
					break;
				}
				m_contentRejected = crInt != 0;
			}
		}
#else
		if (m_contentRejected)
		{
			// the abandoned body still occupies the connection, so it must not
			// serve another request
			FreeConnection(false);
			status = adFailed;
			if (m_articleInfo->GetDupeFallbackRound() > 0)
			{
				// a duplicate's article that doesn't fit this file: every server
				// serves the same content, so it fails here (the next duplicate
				// source, if any, is tried by the queue coordinator)
				break;
			}
			// the release's own article with malformed headers: that server's copy
			// is bad, another server may have a good one - this server is done
			m_contentRejected = false;
			remainedRetries = 0;
		}
#endif

		if (!connected && m_connection)
		{
			detail("Article %s @ %s failed: could not establish connection", *m_infoName, *m_connectionName);
		}

#ifdef NZBGET_USE_RUST
		int connInt = connected ? 1 : 0;
		int statusInt = (int)status;
		nzbget_rs_rrd_article_dl_normalize_connect_and_decrement_retries(
			connInt, statusInt, (int)adConnectError, (int)adFailed, remainedRetries,
			&connInt, &statusInt, &remainedRetries);
		connected = connInt != 0;
		status = (EStatus)statusInt;
#else
		if (status == adConnectError)
		{
			connected = false;
			status = adFailed;
		}

		if (connected && status == adFailed)
		{
			remainedRetries--;
		}
#endif

		bool optionalBlocked = false;
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_aws_article_dl_should_block_failed_server(
			connected ? 1 : 0, m_connection ? 1 : 0, IsStopped() ? 1 : 0) != 0)
#else
		if (!connected && m_connection && !IsStopped())
#endif
		{
			g_ServerPool->BlockServer(lastServer);
			optionalBlocked = lastServer->GetOptional();
		}

		wantServer = nullptr;
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_dad_article_dl_should_retry_same_server(
			connected ? 1 : 0,
			(int)status,
			(int)adFailed,
			remainedRetries,
			retentionFailure ? 1 : 0) != 0)
#else
		if (connected && status == adFailed && remainedRetries > 0 && !retentionFailure)
#endif
		{
			wantServer = lastServer;
		}
		else
		{
#ifdef NZBGET_USE_RUST
			FreeConnection(nzbget_rs_aws_article_dl_keep_conn_on_no_retry(
				(int)status, (int)adFinished, (int)adNotFound) != 0);
#else
			FreeConnection(status == adFinished || status == adNotFound);
#endif
		}

		if (status == adFinished || status == adFatalError)
		{
			break;
		}

#ifdef NZBGET_USE_RUST
		if (nzbget_rs_dad_article_dl_should_abort_for_work_state(
			IsStopped() ? 1 : 0,
			g_WorkState->GetPauseDownload() ? 1 : 0,
			g_WorkState->GetQuotaReached() ? 1 : 0,
			force ? 1 : 0,
			g_WorkState->GetDownloadHeld() ? 1 : 0,
			m_fileInfo->GetExtraPriority() ? 1 : 0,
			serverConfigGeneration != g_ServerPool->GetGeneration() ? 1 : 0) != 0)
#else
		if (IsStopped() || ((g_WorkState->GetPauseDownload() || g_WorkState->GetQuotaReached()) && !force) ||
			(g_WorkState->GetDownloadHeld() && !m_fileInfo->GetExtraPriority()) ||
			serverConfigGeneration != g_ServerPool->GetGeneration())
#endif
		{
			status = adRetry;
			break;
		}

		if (!wantServer && m_fileInfo->GetNzbInfo()->HasDesiredServer())
		{
			status = adFailed;
			break;
		}

#ifdef NZBGET_USE_RUST
		int pushFailedServer = 0;
		if (nzbget_rs_dad_article_dl_should_advance_server_list(
			wantServer ? 1 : 0,
			connected ? 1 : 0,
			retentionFailure ? 1 : 0,
			optionalBlocked ? 1 : 0,
			authRejected ? 1 : 0,
			&pushFailedServer) != 0)
		{
			if (pushFailedServer)
			{
				failedServers.push_back(lastServer);
			}
#else
		if (!wantServer && (connected || retentionFailure || optionalBlocked || authRejected))
		{
			if (!optionalBlocked)
			{
				failedServers.push_back(lastServer);
			}
#endif

			// if all servers from current level were tried, increase level
			// if all servers from all levels were tried, break the loop with failure status

#ifdef NZBGET_USE_RUST
			size_t candCount = g_ServerPool->GetServers()->size();
			std::vector<int32_t> candIds(candCount), candLevels(candCount), candGroups(candCount);
			std::vector<uint8_t> candAvail(candCount);
			size_t ci = 0;
			for (NewsServer* candidateServer : g_ServerPool->GetServers())
			{
				candIds[ci] = candidateServer->GetId();
				candLevels[ci] = candidateServer->GetNormLevel();
				candGroups[ci] = candidateServer->GetGroup();
				bool unavail = !candidateServer->GetActive() || candidateServer->GetMaxConnections() == 0 ||
					(candidateServer->GetOptional() && g_ServerPool->IsServerBlocked(candidateServer));
				candAvail[ci] = unavail ? 0 : 1;
				++ci;
			}
			size_t failCount = failedServers.size();
			std::vector<int32_t> failIds(failCount), failLevels(failCount), failGroups(failCount);
			for (size_t fi = 0; fi < failCount; ++fi)
			{
				failIds[fi] = failedServers[fi]->GetId();
				failLevels[fi] = failedServers[fi]->GetNormLevel();
				failGroups[fi] = failedServers[fi]->GetGroup();
			}
			bool allServersOnLevelFailed = nzbget_rs_dur_article_dl_all_servers_on_level_failed(
				level, candIds.data(), candLevels.data(), candGroups.data(), candAvail.data(), candCount,
				failIds.data(), failLevels.data(), failGroups.data(), failCount) != 0;
#else
			bool allServersOnLevelFailed = true;
			for (NewsServer* candidateServer : g_ServerPool->GetServers())
			{
				if (candidateServer->GetNormLevel() == level)
				{
					bool serverFailed = !candidateServer->GetActive() || candidateServer->GetMaxConnections() == 0 ||
						(candidateServer->GetOptional() && g_ServerPool->IsServerBlocked(candidateServer));
					if (!serverFailed)
					{
						for (NewsServer* ignoreServer : failedServers)
						{
							if (ignoreServer == candidateServer ||
								(ignoreServer->GetGroup() > 0 && ignoreServer->GetGroup() == candidateServer->GetGroup() &&
								 ignoreServer->GetNormLevel() == candidateServer->GetNormLevel()))
							{
								serverFailed = true;
								break;
							}
						}
					}
					if (!serverFailed)
					{
						allServersOnLevelFailed = false;
						break;
					}
				}
			}
#endif

#ifdef NZBGET_USE_RUST
			int advanceLevel = 0;
			int newLevel = level;
			int breakAllFailed = 0;
			nzbget_rs_aws_article_dl_eval_level_exhaustion(
				allServersOnLevelFailed ? 1 : 0, level, g_ServerPool->GetMaxNormLevel(),
				&advanceLevel, &newLevel, &breakAllFailed);
			if (advanceLevel != 0)
			{
				detail("Article %s @ all level %i servers failed, increasing level", *m_infoName, level);
				level = newLevel;
			}
			else if (breakAllFailed != 0)
			{
				detail("Article %s @ all servers failed", *m_infoName);
				status = adFailed;
				break;
			}
#else
			if (allServersOnLevelFailed)
			{
				if (level < g_ServerPool->GetMaxNormLevel())
				{
					detail("Article %s @ all level %i servers failed, increasing level", *m_infoName, level);
					level++;
				}
				else
				{
					detail("Article %s @ all servers failed", *m_infoName);
					status = adFailed;
					break;
				}
			}
#endif

			remainedRetries = retries;
		}
	}

	FreeConnection(status == adFinished);

#ifdef NZBGET_USE_RUST
	bool stopped = IsStopped();
	status = (EStatus)nzbget_rs_dur_article_dl_resolve_final_status(
		(int)status,
		m_articleWriter.GetDuplicate() ? 1 : 0,
		stopped ? 1 : 0,
		(int)adFinished, (int)adFailed, (int)adRetry, (int)adFatalError);
	if (stopped)
	{
		detail("Download %s cancelled", *m_infoName);
	}
#else
	if (m_articleWriter.GetDuplicate())
	{
		status = adFinished;
	}

	if (status != adFinished && status != adRetry && status != adFatalError)
	{
		status = adFailed;
	}

	if (IsStopped())
	{
		detail("Download %s cancelled", *m_infoName);
		status = adRetry;
	}
#endif

	if (status == adFailed)
	{
		detail("Download %s failed", *m_infoName);
	}

	SetStatus(status);
	Notify(nullptr);

	debug("Exiting ArticleDownloader-loop");
}

ArticleDownloader::EStatus ArticleDownloader::Download()
{
	const char* response = nullptr;
	EStatus status = adRunning;
	m_writingStarted = false;
	m_localWriteError = false;
	m_contentRejected = false;
	m_articleInfo->SetCrc(0);

	if (m_contentAnalyzer)
	{
		m_contentAnalyzer->Reset();
	}

	// a file without groups requests the article by its message-id alone: the
	// join loop never ran, its empty answer counted as a connection error, and
	// the article was retried forever
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_dds_article_dl_should_join_group(
		m_connection->GetNewsServer()->GetJoinGroup() ? 1 : 0,
		m_fileInfo->GetGroups()->empty() ? 1 : 0) != 0)
#else
	if (m_connection->GetNewsServer()->GetJoinGroup() && !m_fileInfo->GetGroups()->empty())
#endif
	{
		// change group
		for (CString& group : m_fileInfo->GetGroups())
		{
			response = m_connection->JoinGroup(group);
#ifdef NZBGET_USE_RUST
			if (nzbget_rs_rrd_article_dl_is_join_group_ok(response) != 0)
#else
			if (response && !strncmp(response, "2", 1))
#endif
			{
				break;
			}
		}

		status = CheckResponse(response, "could not join group");
		if (status != adFinished)
		{
			return status;
		}
	}

	// retrieve article
	response = m_connection->Request(BString<1024>("%s %s\r\n",
		g_Options->GetRawArticle() ? "ARTICLE" : "BODY", m_articleInfo->GetMessageId()));

	status = CheckResponse(response, "could not fetch article");
	if (status != adFinished)
	{
		return status;
	}

	m_decoder.Clear();
	m_decoder.SetCrcCheck(g_Options->GetCrcCheck());
	m_decoder.SetRawMode(g_Options->GetRawArticle());

	status = adRunning;
	// (128 bytes past what is read into it: the decoder may write up to one uuencoded
	// line's worth more than it was given)
	int readChunk = g_Options->GetArticleReadChunkSize();
#ifdef NZBGET_USE_RUST
	CharBuffer lineBuf(nzbget_rs_cwp_article_dl_calc_line_buf_capacity(readChunk));
#else
	CharBuffer lineBuf(readChunk + 128);
#endif

	while (!IsStopped() && !m_decoder.GetEof())
	{
		// Memory pressure throttling: pace uncached network reads if memory is Critical and cache is flushing.
		// Never block an article that already allocated a cache buffer before pressure turned Critical,
		// as its buffer only becomes flushable after Download() finishes and calls ArticleWriter::Finish().
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_dad_article_dl_should_pace_critical_uncached(
			IsStopped() ? 1 : 0,
			m_articleWriter.HasCachedBuffer() ? 1 : 0,
			(g_MemoryMonitor && g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL) ? 1 : 0,
			(g_ArticleCache && g_ArticleCache->GetFlushing()) ? 1 : 0) != 0)
#else
		if (!IsStopped() && !m_articleWriter.HasCachedBuffer() && g_MemoryMonitor &&
			g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL &&
			g_ArticleCache && g_ArticleCache->GetFlushing())
#endif
		{
			Util::Sleep(10);
		}

		// throttle the bandwidth
#ifdef NZBGET_USE_RUST
		while (nzbget_rs_dur_article_dl_should_throttle_bandwidth(
			IsStopped() ? 1 : 0,
			g_WorkState->GetSpeedLimit(),
			g_StatMeter->CalcCurrentDownloadSpeed(),
			g_StatMeter->CalcMomentaryDownloadSpeed()) != 0)
#else
		while (!IsStopped() && (g_WorkState->GetSpeedLimit() > 0.0f) &&
			(g_StatMeter->CalcCurrentDownloadSpeed() > g_WorkState->GetSpeedLimit() ||
			g_StatMeter->CalcMomentaryDownloadSpeed() > g_WorkState->GetSpeedLimit()))
#endif
		{
			SetLastUpdateTimeNow();
			Util::Sleep(10);
		}

		char* buffer;
		int len;
		m_connection->ReadBuffer(&buffer, &len);
		if (len == 0)
		{
			len = m_connection->TryRecv(lineBuf, readChunk);
			buffer = lineBuf;
		}

		// have we encountered a timeout?
		if (len <= 0)
		{
			if (!IsStopped())
			{
				detail("Article %s @ %s failed: Unexpected end of article", *m_infoName, *m_connectionName);
			}
			// the rest of this body may still arrive: the connection isn't used again (it
			// was, and late body bytes were read as the reply to the next request - at
			// worst another article's body decoded into this file). Marked broken first:
			// a QUIT to a server that stalled waited out another full timeout
			m_connection->SetBroken();
			m_connection->Disconnect();
			status = adFailed;
			break;
		}

		g_StatMeter->AddSpeedReading(len);
		time_t oldTime = m_lastUpdateTime;
		SetLastUpdateTimeNow();
		if (oldTime != m_lastUpdateTime)
		{
			AddServerStats();
		}

		// decode article data
		len = m_decoder.DecodeBuffer(buffer, len);

		// write to output file
		if (len > 0 && !Write(buffer, len))
		{
#ifdef NZBGET_USE_RUST
			int crInt = 0;
			status = (EStatus)nzbget_rs_cwp_article_dl_classify_write_failure(
				m_localWriteError ? 1 : 0, (int)adFatalError, (int)adFailed, &crInt);
			if (crInt != 0)
			{
				m_contentRejected = true;
			}
#else
			if (m_localWriteError)
			{
				status = adFatalError;
			}
			else
			{
				// the article's content was rejected before its body was fully
				// read: the rest of the body is still pending on the connection
				m_contentRejected = true;
				status = adFailed;
			}
#endif
			break;
		}
	}

	if (IsStopped())
	{
		status = adFailed;
	}

	if (status == adRunning)
	{
		FreeConnection(true);
		status = DecodeCheck();
	}

	if (m_writingStarted)
	{
		m_articleWriter.Finish(status == adFinished);
	}

	if (status == adFinished)
	{
		detail("Successfully downloaded %s", *m_infoName);
	}

	return status;
}

ArticleDownloader::EStatus ArticleDownloader::CheckResponse(const char* response, const char* comment)
{
#ifdef NZBGET_USE_RUST
	EStatus status = (EStatus)nzbget_rs_article_downloader_check_response(
		response, m_connection->GetAuthError() ? 1 : 0);
	if (status != adFinished)
	{
		if (!response)
		{
			if (!IsStopped())
			{
				detail("Article %s @ %s failed, %s: Connection closed by remote host",
					*m_infoName, *m_connectionName, comment);
			}
		}
		else
		{
			detail("Article %s @ %s failed, %s: %s", *m_infoName, *m_connectionName, comment, response);
		}
	}
	return status;
#else
	if (!response)
	{
		if (!IsStopped())
		{
			detail("Article %s @ %s failed, %s: Connection closed by remote host",
				*m_infoName, *m_connectionName, comment);
		}
		return adConnectError;
	}
	else if (m_connection->GetAuthError() || !strncmp(response, "400", 3) || !strncmp(response, "499", 3))
	{
		detail("Article %s @ %s failed, %s: %s", *m_infoName, *m_connectionName, comment, response);
		return adConnectError;
	}
	else if (!strncmp(response, "41", 2) || !strncmp(response, "42", 2) || !strncmp(response, "43", 2) ||
		!strncmp(response, "451", 3))
	{
		// 451 is how some providers (super.newsgroupdirect.com) say "no such
		// article": treat it like 430, not as an unknown error that is retried
		// on the same server after <ArticleInterval>
		detail("Article %s @ %s failed, %s: %s", *m_infoName, *m_connectionName, comment, response);
		return adNotFound;
	}
	else if (!strncmp(response, "2", 1))
	{
		// OK
		return adFinished;
	}
	else
	{
		// unknown error, no special handling
		detail("Article %s @ %s failed, %s: %s", *m_infoName, *m_connectionName, comment, response);
		return adFailed;
	}
#endif
}

bool ArticleDownloader::Write(char* buffer, int len)
{
	const char* articleFilename = nullptr;
	int64 articleFileSize = 0;
	int64 articleOffset = 0;
	int articleSize = 0;

	if (!m_writingStarted)
	{
		if (!g_Options->GetRawArticle())
		{
			articleFilename = m_decoder.GetArticleFilename();
			if (m_decoder.GetFormat() == Decoder::efYenc)
			{
#ifdef NZBGET_USE_RUST
				int64 beginPos = m_decoder.GetBeginPos();
				int64 endPos = m_decoder.GetEndPos();
				articleFileSize = m_decoder.GetSize();
				int64 expectedFileSize = m_fileInfo->GetDecodedFileSize();
				int64 dupeExpectedOffset = m_articleInfo->GetDupeExpectedOffset();
				int64 dupeExpectedEnd = m_articleInfo->GetDupeExpectedEnd();
				int rc = nzbget_rs_article_downloader_validate_yenc(
					beginPos, endPos, articleFileSize, expectedFileSize,
					m_articleInfo->GetDupeFallbackRound(), dupeExpectedOffset, dupeExpectedEnd,
					&articleOffset, &articleSize);
				if (rc == 1)
				{
					return false;
				}
				if (rc == 2)
				{
					warn("Malformed article %s: range %lli-%lli out of range (file size %lli)", *m_infoName,
						(long long)beginPos, (long long)endPos, (long long)articleFileSize);
					return false;
				}
				m_decodedFileSize = articleFileSize;
				if (rc == 3)
				{
					detail("Discarding article %s from duplicate: file size mismatch (%lli vs %lli)",
						*m_infoName, (long long)articleFileSize, (long long)expectedFileSize);
					return false;
				}
				if (rc == 4)
				{
					detail("Discarding article %s from duplicate: decoded offset mismatch (%lli vs %lli)",
						*m_infoName, (long long)(beginPos - 1), (long long)dupeExpectedOffset);
					return false;
				}
				if (rc == 5)
				{
					detail("Discarding article %s from duplicate: decoded end mismatch (%lli vs %lli)",
						*m_infoName, (long long)endPos, (long long)dupeExpectedEnd);
					return false;
				}
#else
				if (m_decoder.GetBeginPos() == 0 || m_decoder.GetEndPos() == 0)
				{
					return false;
				}
				articleFileSize = m_decoder.GetSize();
				articleOffset = m_decoder.GetBeginPos() - 1;
				// the range in 64 bits, inside the file size the article declares: cut
				// to an int, a huge range could pass as a small one (F24), and an offset
				// far past the file had the file filled with zeros up to it (F23)
				int64 rangeSize = m_decoder.GetEndPos() - m_decoder.GetBeginPos() + 1;
				if (articleOffset < 0 || rangeSize <= 0 || rangeSize > 1024*1024*1024 ||
					(articleFileSize > 0 && articleOffset + rangeSize > articleFileSize))
				{
					warn("Malformed article %s: range %lli-%lli out of range (file size %lli)", *m_infoName,
						(long long)m_decoder.GetBeginPos(), (long long)m_decoder.GetEndPos(), (long long)articleFileSize);
					return false;
				}
				articleSize = (int)rangeSize;
				m_decodedFileSize = articleFileSize;
				int64 expectedFileSize = m_fileInfo->GetDecodedFileSize();
				if (m_articleInfo->GetDupeFallbackRound() > 0 && expectedFileSize > 0 &&
					articleFileSize != expectedFileSize)
				{
					detail("Discarding article %s from duplicate: file size mismatch (%lli vs %lli)",
						*m_infoName, (long long)articleFileSize, (long long)expectedFileSize);
					return false;
				}
				// a substituted article must decode into exactly the byte range the
				// target article occupies; the expected range was pinned from finished
				// neighbour articles at substitution time (-1 = not known yet). This
				// rejects donors with drifted decoded boundaries before any bytes are
				// written (a mis-placed article would gap-fill and/or overwrite a
				// neighbour's decoded bytes).
				int64 dupeExpectedOffset = m_articleInfo->GetDupeExpectedOffset();
				if (m_articleInfo->GetDupeFallbackRound() > 0 && dupeExpectedOffset >= 0 &&
					articleOffset != dupeExpectedOffset)
				{
					detail("Discarding article %s from duplicate: decoded offset mismatch (%lli vs %lli)",
						*m_infoName, (long long)articleOffset, (long long)dupeExpectedOffset);
					return false;
				}
				int64 dupeExpectedEnd = m_articleInfo->GetDupeExpectedEnd();
				if (m_articleInfo->GetDupeFallbackRound() > 0 && dupeExpectedEnd >= 0 &&
					articleOffset + articleSize != dupeExpectedEnd)
				{
					detail("Discarding article %s from duplicate: decoded end mismatch (%lli vs %lli)",
						*m_infoName, (long long)(articleOffset + articleSize), (long long)dupeExpectedEnd);
					return false;
				}
#endif
			}
		}

		if (!m_articleWriter.Start(m_decoder.GetFormat(), articleFilename, articleFileSize, articleOffset, articleSize))
		{
			// both a write failure and a duplicate-existing-file result end the
			// download of this article on the spot (a duplicate is then reported
			// as finished by Run)
			m_localWriteError = true;
			return false;
		}
		m_writingStarted = true;
	}

	bool ok = m_articleWriter.Write(buffer, len);
	if (!ok)
	{
		m_localWriteError = true;
	}

	if (m_contentAnalyzer)
	{
		m_contentAnalyzer->Append(buffer, len);
	}

	return ok;
}

ArticleDownloader::EStatus ArticleDownloader::DecodeCheck()
{
#ifdef NZBGET_USE_RUST
	bool rawArticle = g_Options->GetRawArticle();
	if (rawArticle)
	{
		return adFinished;
	}
	Decoder::EStatus status = m_decoder.Check();
	bool isYenc = m_decoder.GetFormat() == Decoder::efYenc;
	switch (nzbget_rs_dur_article_dl_classify_decode_status(
		0, (int)status, isYenc ? 1 : 0, m_articleWriter.GetRangeExceeded() ? 1 : 0))
	{
		case 1:
			if (m_decoder.GetArticleFilename())
			{
				m_articleFilename = m_decoder.GetArticleFilename();
			}
			if (isYenc)
			{
				m_articleInfo->SetCrc(g_Options->GetCrcCheck() ?
					m_decoder.GetCalculatedCrc() : m_decoder.GetExpectedCrc());
			}
			return adFinished;
		case 2:
			detail("Decoding %s failed: more data than its range", *m_infoName);
			return adFailed;
		case 3:
			detail("Decoding %s failed: CRC-Error", *m_infoName);
			return adCrcError;
		case 4:
			detail("Decoding %s failed: article incomplete", *m_infoName);
			return adFailed;
		case 5:
			detail("Decoding %s failed: size mismatch", *m_infoName);
			return adFailed;
		case 6:
			detail("Decoding %s failed: no binary data found", *m_infoName);
			return adFailed;
		default:
			detail("Decoding %s failed", *m_infoName);
			return adFailed;
	}
#else
	if (!g_Options->GetRawArticle())
	{
		Decoder::EStatus status = m_decoder.Check();

		// an article decoding to more bytes than its range holds: the bytes past it were
		// dropped, but its size counted them, and the cache padded the segment with
		// memory never written - into the file, past its end or over the next article
		if (status == Decoder::dsFinished && m_decoder.GetFormat() == Decoder::efYenc &&
			!g_Options->GetRawArticle() && m_articleWriter.GetRangeExceeded())
		{
			detail("Decoding %s failed: more data than its range", *m_infoName);
			return adFailed;
		}

		if (status == Decoder::dsFinished)
		{
			if (m_decoder.GetArticleFilename())
			{
				m_articleFilename = m_decoder.GetArticleFilename();
			}

			if (m_decoder.GetFormat() == Decoder::efYenc)
			{
				m_articleInfo->SetCrc(g_Options->GetCrcCheck() ?
					m_decoder.GetCalculatedCrc() : m_decoder.GetExpectedCrc());
			}

			return adFinished;
		}
		else if (status == Decoder::dsCrcError)
		{
			detail("Decoding %s failed: CRC-Error", *m_infoName);
			return adCrcError;
		}
		else if (status == Decoder::dsArticleIncomplete)
		{
			detail("Decoding %s failed: article incomplete", *m_infoName);
			return adFailed;
		}
		else if (status == Decoder::dsInvalidSize)
		{
			detail("Decoding %s failed: size mismatch", *m_infoName);
			return adFailed;
		}
		else if (status == Decoder::dsNoBinaryData)
		{
			detail("Decoding %s failed: no binary data found", *m_infoName);
			return adFailed;
		}
		else
		{
			detail("Decoding %s failed", *m_infoName);
			return adFailed;
		}
	}
	else
	{
		return adFinished;
	}
#endif
}

void ArticleDownloader::SetLastUpdateTimeNow()
{
	m_lastUpdateTime = Util::CurrentTime();
}

void ArticleDownloader::LogDebugInfo()
{
	info("      Download: status=%i, LastUpdateTime=%s, InfoName=%s", m_status,
		 *Util::FormatTime(m_lastUpdateTime.load()), *m_infoName);
}

void ArticleDownloader::Stop()
{
	debug("Trying to stop ArticleDownloader");
	Thread::Stop();
	Guard guard(m_connectionMutex);
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
	debug("ArticleDownloader stopped successfully");
}

void ArticleDownloader::FreeConnection(bool keepConnected)
{
	if (m_connection)
	{
		debug("Releasing connection");
		Guard guard(m_connectionMutex);
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_rrd_article_dl_should_disconnect_on_free(
			keepConnected ? 1 : 0,
			(int)m_connection->GetStatus(),
			(int)Connection::csCancelled) != 0)
#else
		if (!keepConnected || m_connection->GetStatus() == Connection::csCancelled)
#endif
		{
			m_connection->Disconnect();
		}
		AddServerStats();
		g_ServerPool->FreeConnection(m_connection, true);
		m_connection = nullptr;
	}
}

void ArticleDownloader::AddServerStats()
{
	int serverId = m_connection->GetNewsServer()->GetId();
	int bytesRead = m_connection->FetchTotalBytesRead();
	ServerVolume::Stats stats;
	stats.bytes = Util::SafeIntCast<int, uint32>(bytesRead);
	stats.articles.failed = 0;
	stats.articles.success = 0;
	g_StatMeter->AddServerStats(stats, serverId);
	m_downloadedSize += bytesRead;
}
