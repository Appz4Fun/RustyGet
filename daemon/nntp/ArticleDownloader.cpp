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
#include "nzbget_rs.h"

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
	int retries = nzbget_rs_rrd_article_dl_resolve_initial_retries(g_Options->GetArticleRetries());
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
		while (nzbget_rs_aws_article_dl_should_wait_for_connection(
			m_connection ? 1 : 0, IsStopped() ? 1 : 0, serverConfigGeneration, g_ServerPool->GetGeneration()) != 0)
		{
			m_connection = g_ServerPool->GetConnection(level, wantServer, &failedServers);
			Util::Sleep(5);
		}
		SetLastUpdateTimeNow();
		SetStatus(adRunning);

		if (nzbget_rs_dad_article_dl_should_abort_for_work_state(
			IsStopped() ? 1 : 0,
			g_WorkState->GetPauseDownload() ? 1 : 0,
			g_WorkState->GetQuotaReached() ? 1 : 0,
			force ? 1 : 0,
			g_WorkState->GetDownloadHeld() ? 1 : 0,
			m_fileInfo->GetExtraPriority() ? 1 : 0,
			serverConfigGeneration != g_ServerPool->GetGeneration() ? 1 : 0) != 0)
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

		{
			NzbgetRsBuf connNameBuf = nzbget_rs_dqh_article_dl_format_connection_name(
				m_connection->GetNewsServer()->GetName(), m_connection->GetHost());
			m_connectionName.Set(connNameBuf.data ? connNameBuf.data : "", (int)connNameBuf.len);
			nzbget_rs_free(connNameBuf);
		}

		// check server retention
		// a file without a date (none in the nzb) has time 0: its age isn't known,
		// and counting from 1970 failed every article on a server with a retention
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

		if (m_connection && !IsStopped())
		{
			detail("Downloading %s @ %s", *m_infoName, *m_connectionName);
		}

		// test connection
		bool connected = m_connection && m_connection->Connect();
		// a server that refuses the login for good (502) can't serve this article: it
		// counts as tried. Waited for, a server with a broken account stalled every
		// download that had failed on the other servers of its level
		bool authRejected = nzbget_rs_aws_article_dl_eval_auth_rejected(
			connected ? 1 : 0, m_connection ? 1 : 0,
			(m_connection && m_connection->GetAuthRejected()) ? 1 : 0) != 0;
		if (connected && !IsStopped())
		{
			NewsServer* newsServer = m_connection->GetNewsServer();

			// Download article
			status = Download();

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
		}

		if (m_connection)
		{
			AddServerStats();
		}

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

		if (!connected && m_connection)
		{
			detail("Article %s @ %s failed: could not establish connection", *m_infoName, *m_connectionName);
		}

		int connInt = connected ? 1 : 0;
		int statusInt = (int)status;
		nzbget_rs_rrd_article_dl_normalize_connect_and_decrement_retries(
			connInt, statusInt, (int)adConnectError, (int)adFailed, remainedRetries,
			&connInt, &statusInt, &remainedRetries);
		connected = connInt != 0;
		status = (EStatus)statusInt;

		bool optionalBlocked = false;
		if (nzbget_rs_aws_article_dl_should_block_failed_server(
			connected ? 1 : 0, m_connection ? 1 : 0, IsStopped() ? 1 : 0) != 0)
		{
			g_ServerPool->BlockServer(lastServer);
			optionalBlocked = lastServer->GetOptional();
		}

		wantServer = nullptr;
		if (nzbget_rs_dad_article_dl_should_retry_same_server(
			connected ? 1 : 0,
			(int)status,
			(int)adFailed,
			remainedRetries,
			retentionFailure ? 1 : 0) != 0)
		{
			wantServer = lastServer;
		}
		else
		{
			FreeConnection(nzbget_rs_aws_article_dl_keep_conn_on_no_retry(
				(int)status, (int)adFinished, (int)adNotFound) != 0);
		}

		if (nzbget_rs_dqh_article_dl_is_terminal_loop_status(
			(int)status, (int)adFinished, (int)adFatalError) != 0)
		{
			break;
		}

		if (nzbget_rs_dad_article_dl_should_abort_for_work_state(
			IsStopped() ? 1 : 0,
			g_WorkState->GetPauseDownload() ? 1 : 0,
			g_WorkState->GetQuotaReached() ? 1 : 0,
			force ? 1 : 0,
			g_WorkState->GetDownloadHeld() ? 1 : 0,
			m_fileInfo->GetExtraPriority() ? 1 : 0,
			serverConfigGeneration != g_ServerPool->GetGeneration() ? 1 : 0) != 0)
		{
			status = adRetry;
			break;
		}

		if (nzbget_rs_dqh_article_dl_should_fail_for_desired_server(
			wantServer ? 1 : 0,
			m_fileInfo->GetNzbInfo()->HasDesiredServer() ? 1 : 0) != 0)
		{
			status = adFailed;
			break;
		}

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

			// if all servers from current level were tried, increase level
			// if all servers from all levels were tried, break the loop with failure status

			size_t candCount = g_ServerPool->GetServers()->size();
			std::vector<int32_t> candIds(candCount), candLevels(candCount), candGroups(candCount);
			std::vector<uint8_t> candAvail(candCount);
			size_t ci = 0;
			for (NewsServer* candidateServer : g_ServerPool->GetServers())
			{
				candIds[ci] = candidateServer->GetId();
				candLevels[ci] = candidateServer->GetNormLevel();
				candGroups[ci] = candidateServer->GetGroup();
				candAvail[ci] = (uint8_t)nzbget_rs_dqh_article_dl_is_server_available(
					candidateServer->GetActive() ? 1 : 0,
					candidateServer->GetMaxConnections(),
					candidateServer->GetOptional() ? 1 : 0,
					(candidateServer->GetOptional() && g_ServerPool->IsServerBlocked(candidateServer)) ? 1 : 0);
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

			remainedRetries = retries;
		}
	}

	FreeConnection(status == adFinished);

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
	if (nzbget_rs_dds_article_dl_should_join_group(
		m_connection->GetNewsServer()->GetJoinGroup() ? 1 : 0,
		m_fileInfo->GetGroups()->empty() ? 1 : 0) != 0)
	{
		// change group
		for (CString& group : m_fileInfo->GetGroups())
		{
			response = m_connection->JoinGroup(group);
			if (nzbget_rs_rrd_article_dl_is_join_group_ok(response) != 0)
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
	{
		NzbgetRsBuf cmdBuf = nzbget_rs_dqh_article_dl_format_fetch_command(
			g_Options->GetRawArticle() ? 1 : 0, m_articleInfo->GetMessageId());
		response = m_connection->Request(cmdBuf.data ? cmdBuf.data : "");
		nzbget_rs_free(cmdBuf);
	}

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
	CharBuffer lineBuf(nzbget_rs_cwp_article_dl_calc_line_buf_capacity(readChunk));

	while (nzbget_rs_dqh_article_dl_should_continue_read_loop(
		IsStopped() ? 1 : 0, m_decoder.GetEof() ? 1 : 0) != 0)
	{
		// Memory pressure throttling: pace uncached network reads if memory is Critical and cache is flushing.
		// Never block an article that already allocated a cache buffer before pressure turned Critical,
		// as its buffer only becomes flushable after Download() finishes and calls ArticleWriter::Finish().
		if (nzbget_rs_dad_article_dl_should_pace_critical_uncached(
			IsStopped() ? 1 : 0,
			m_articleWriter.HasCachedBuffer() ? 1 : 0,
			(g_MemoryMonitor && g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL) ? 1 : 0,
			(g_ArticleCache && g_ArticleCache->GetFlushing()) ? 1 : 0) != 0)
		{
			Util::Sleep(10);
		}

		// throttle the bandwidth
		while (nzbget_rs_dur_article_dl_should_throttle_bandwidth(
			IsStopped() ? 1 : 0,
			g_WorkState->GetSpeedLimit(),
			g_StatMeter->CalcCurrentDownloadSpeed(),
			g_StatMeter->CalcMomentaryDownloadSpeed()) != 0)
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
			int crInt = 0;
			status = (EStatus)nzbget_rs_cwp_article_dl_classify_write_failure(
				m_localWriteError ? 1 : 0, (int)adFatalError, (int)adFailed, &crInt);
			if (crInt != 0)
			{
				m_contentRejected = true;
			}
			break;
		}
	}

	if (IsStopped())
	{
		status = adFailed;
	}

	if (nzbget_rs_dqh_article_dl_should_run_decode_check(
		(int)status, (int)adRunning) != 0)
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
				m_articleInfo->SetCrc(nzbget_rs_dqh_article_dl_select_yenc_crc(
					g_Options->GetCrcCheck() ? 1 : 0,
					m_decoder.GetCalculatedCrc(), m_decoder.GetExpectedCrc()));
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
		if (nzbget_rs_rrd_article_dl_should_disconnect_on_free(
			keepConnected ? 1 : 0,
			(int)m_connection->GetStatus(),
			(int)Connection::csCancelled) != 0)
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
