/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2012-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "WebDownloader.h"
#include "nzbget_rs.h"
#include "Log.h"
#include "Options.h"
#include "WorkState.h"
#include "Util.h"
#include "FileSystem.h"

WebDownloader::WebDownloader()
	: m_contentLen(0)
	, m_redirecting(false)
	, m_redirected(false)
	, m_gzip(false)
{
	debug("Creating WebDownloader");

	SetLastUpdateTimeNow();
}

void WebDownloader::SetUrl(const char* url)
{
	m_url = WebUtil::UrlEncode(url);
}

void WebDownloader::SetStatus(EStatus status)
{
	m_status = status;
	Notify(nullptr);
}

void WebDownloader::SetLastUpdateTimeNow()
{
	m_lastUpdateTime = Util::CurrentTime();
}

void WebDownloader::Run()
{
	debug("Entering WebDownloader-loop");

	SetStatus(adRunning);

	int remainedDownloadRetries = 1;
	int remainedConnectRetries = 1;
	nzbget_rs_ntw_webdl_calc_initial_retries(
		g_Options->GetUrlRetries(), m_retry ? 1 : 0,
		&remainedDownloadRetries, &remainedConnectRetries);

	EStatus Status = adFailed;

	while (!IsStopped() && remainedDownloadRetries > 0 && remainedConnectRetries > 0)
	{
		SetLastUpdateTimeNow();

		Status = DownloadWithRedirects(5);

		if (nzbget_rs_ntw_webdl_should_wait_before_retry(
			(int)Status, (int)adFailed, (int)adConnectError,
			remainedDownloadRetries, remainedConnectRetries,
			IsStopped() ? 1 : 0, m_force ? 1 : 0,
			g_WorkState->GetPauseDownload() ? 1 : 0) != 0)
		{
			detail("Waiting %i sec to retry", g_Options->GetUrlInterval());
			int msec = 0;
			while (!IsStopped() && (msec < g_Options->GetUrlInterval() * 1000) &&
				!(!m_force && g_WorkState->GetPauseDownload()))
			{
				// waiting isn't hanging: an interval longer than UrlTimeout got the
				// download cancelled and restarted with all its retries again
				SetLastUpdateTimeNow();
				Util::Sleep(100);
				msec += 100;
			}
		}

		if (nzbget_rs_rrd_webdl_should_abort_retry_loop(
			IsStopped() ? 1 : 0, m_force ? 1 : 0, g_WorkState->GetPauseDownload() ? 1 : 0) != 0)
		{
			Status = adRetry;
			break;
		}

		if (Status == adFinished || Status == adFatalError || Status == adNotFound)
		{
			break;
		}

		nzbget_rs_rrd_webdl_step_retry_counters(
			(int)Status, (int)adConnectError,
			remainedDownloadRetries, remainedConnectRetries,
			&remainedDownloadRetries, &remainedConnectRetries);
	}

	Status = (EStatus)nzbget_rs_rrd_webdl_resolve_final_loop_status(
		(int)Status, (int)adFinished, (int)adRetry, (int)adFailed);

	if (Status == adFailed)
	{
		if (IsStopped())
		{
			detail("Download %s cancelled", *m_infoName);
		}
		else
		{
			error("Download %s failed", *m_infoName);
		}
	}

	if (Status == adFinished)
	{
		detail("Download %s completed", *m_infoName);
	}

	SetStatus(Status);

	debug("Exiting WebDownloader-loop");
}

WebDownloader::EStatus WebDownloader::Download()
{
	EStatus Status = adRunning;

	URL url(m_url);

	Status = CreateConnection(&url);
	if (Status != adRunning)
	{
		return Status;
	}

	m_connection->SetTimeout(g_Options->GetUrlTimeout());
	m_connection->SetSuppressErrors(false);

	// connection
	bool connected = m_connection->Connect();
	if (!connected || IsStopped())
	{
		FreeConnection();
		return adConnectError;
	}

	// Okay, we got a Connection. Now start downloading.
	detail("Downloading %s", *m_infoName);

	SendHeaders(&url);

	Status = DownloadHeaders();

	if (Status == adRunning)
	{
		Status = DownloadBody();
	}

	if (IsStopped())
	{
		Status = adFailed;
	}

	FreeConnection();

	if (Status != adFinished)
	{
		// Download failed, delete broken output file
		FileSystem::DeleteFile(m_outputFilename);
	}

	return Status;
}

WebDownloader::EStatus WebDownloader::DownloadWithRedirects(int maxRedirects)
{
	// do sync download, following redirects
	EStatus status = adRedirect;
	int tooManyRedirects = 0;
	while (nzbget_rs_dds_webdl_redirect_loop_step(
		(int)status, (int)adRedirect, maxRedirects, &tooManyRedirects) != 0)
	{
		maxRedirects--;
		status = Download();
	}

	if (tooManyRedirects != 0)
	{
		warn("Too many redirects for %s", *m_infoName);
		status = adFailed;
	}

	return status;
}

WebDownloader::EStatus WebDownloader::CreateConnection(URL *url)
{
	int tlsDisabled = 0;
#ifdef DISABLE_TLS
	tlsDisabled = 1;
#endif
	int port = 0;
	int tlsInt = 0;
	int rc = nzbget_rs_rco_webdl_resolve_protocol_port(
		url->IsValid() ? 1 : 0, url->GetProtocol(), url->GetPort(),
		tlsDisabled, &port, &tlsInt);
	if (rc == -1)
	{
		error("URL is not valid: %s", url->GetAddress());
		return adFatalError;
	}
	if (rc == -2)
	{
		error("Unsupported protocol in URL: %s", url->GetAddress());
		return adFatalError;
	}
	if (rc == -3)
	{
		error("Program was compiled without TLS/SSL-support. Cannot download using https protocol. URL: %s", url->GetAddress());
		return adFatalError;
	}
	bool tls = tlsInt != 0;

	m_connection = std::make_unique<Connection>(url->GetHost(), port, tls);

#ifndef DISABLE_TLS
	m_connection->SetCertVerifLevel(m_certVerifLevel);
#endif

	return adRunning;
}

void WebDownloader::SendHeaders(URL *url)
{
	// retrieve file
	// not cut to a fixed size: a long (signed) url lost its "HTTP/1.0" and line end
	m_connection->WriteLine(CString::FormatStr("GET %s HTTP/1.0\r\n", url->GetResource()));
	m_connection->WriteLine(BString<1024>("User-Agent: nzbget/%s\r\n", Util::VersionRevision()));

	if (nzbget_rs_ntw_webdl_is_default_http_port(url->GetProtocol(), url->GetPort()) != 0)
	{
		m_connection->WriteLine(BString<1024>("Host: %s\r\n", url->GetHost()));
	}
	else
	{
		m_connection->WriteLine(BString<1024>("Host: %s:%i\r\n", url->GetHost(), url->GetPort()));
	}

	m_connection->WriteLine("Accept: */*\r\n");
#ifndef DISABLE_GZIP
	m_connection->WriteLine("Accept-Encoding: gzip\r\n");
#endif
	m_connection->WriteLine("Connection: close\r\n");
	m_connection->WriteLine("\r\n");
}

WebDownloader::EStatus WebDownloader::DownloadHeaders()
{
	EStatus Status = adRunning;

	m_confirmedLength = false;
	CharBuffer lineBuf(1024*10);
	m_contentLen = -1;
	bool firstLine = true;
	m_gzip = false;
	m_redirecting = false;
	m_redirected = false;

	// Headers
	while (!IsStopped())
	{
		SetLastUpdateTimeNow();

		int len = 0;
		char* line = m_connection->ReadLine(lineBuf, lineBuf.Size(), &len);

		if (firstLine)
		{
			Status = CheckResponse(line);
			if (Status != adRunning)
			{
				break;
			}
			firstLine = false;
		}

		// Have we encountered a timeout?
		if (!line)
		{
			if (!IsStopped())
			{
				warn("URL %s failed: Unexpected end of file", *m_infoName);
			}
			Status = adFailed;
			break;
		}

		debug("Header: %s", line);

		// detect body of response
		if (nzbget_rs_ntw_webdl_is_header_end_line((unsigned char)*line) != 0)
		{
			break;
		}

		Util::TrimRight(line);
		ProcessHeader(line);

		if (m_redirected)
		{
			Status = adRedirect;
			break;
		}
	}

	return Status;
}

WebDownloader::EStatus WebDownloader::DownloadBody()
{
	EStatus Status = adRunning;

	m_outFile.Close();
	bool end = false;
	CharBuffer lineBuf(1024*10);
	int writtenLen = 0;

#ifndef DISABLE_GZIP
	m_gUnzipStream.reset();
	if (m_gzip)
	{
		m_gUnzipStream = std::make_unique<GUnzipStream>(1024*10);
	}
#endif

	// Body
	while (!IsStopped())
	{
		SetLastUpdateTimeNow();

		char* buffer;
		int len;
		m_connection->ReadBuffer(&buffer, &len);
		if (len == 0)
		{
			len = m_connection->TryRecv(lineBuf, lineBuf.Size());
			buffer = lineBuf;
		}

		if (len <= 0)
		{
			if (nzbget_rs_rco_webdl_check_body_chunk(
				len, m_contentLen, writtenLen, m_gzip ? 1 : 0, m_confirmedLength ? 1 : 0) == 1)
			{
				end = true;
				break;
			}
			if (!IsStopped())
			{
				warn("URL %s failed: Unexpected end of file", *m_infoName);
			}
			Status = adFailed;
			break;
		}

		// write to output file
		if (!Write(buffer, len))
		{
			Status = adFatalError;
			break;
		}
		int bodyStep = nzbget_rs_rco_webdl_check_body_chunk(
			len, m_contentLen, writtenLen, m_gzip ? 1 : 0, m_confirmedLength ? 1 : 0);
		writtenLen += len;

		if (bodyStep == 2)
		{
			end = true;
			break;
		}
	}

#ifndef DISABLE_GZIP
	m_gUnzipStream.reset();
#endif

	m_outFile.Close();

	int warnIncomplete = 0;
	Status = (EStatus)nzbget_rs_rrd_webdl_resolve_body_end_status(
		end ? 1 : 0, (int)Status, IsStopped() ? 1 : 0,
		(int)adRunning, (int)adFailed, (int)adFinished, &warnIncomplete);
	if (warnIncomplete)
	{
		warn("URL %s failed: file incomplete", *m_infoName);
	}

	return Status;
}

WebDownloader::EStatus WebDownloader::CheckResponse(const char* response)
{
	// rust/src/webdownload.rs decides; the warnings are logged here
	NzbgetRsHttpResponse r;
	nzbget_rs_http_check_response(response, &r);
	if (r.setStatus)
	{
		m_httpStatus = r.httpStatus;
	}
	switch (r.warn)
	{
		case 1:
			if (!IsStopped())
			{
				warn("URL %s: Connection closed by remote host", *m_infoName);
			}
			break;

		case 2:
			warn("URL %s failed: %s", *m_infoName, response);
			break;

		case 3:
			warn("URL %s failed: %s", *m_infoName, response + r.statusOffset);
			break;
	}
	switch (r.result)
	{
		case 0:
			return adRunning;

		case 1:
			m_redirecting = true;
			return adRunning;

		case 2:
			return adConnectError;

		case 3:
			return adNotFound;

		default:
			return adFailed;
	}
}

void WebDownloader::ProcessHeader(const char* line)
{
	// rust/src/webdownload.rs
	int value = 0;
	switch (nzbget_rs_http_header(line, m_redirecting, &value))
	{
		case 1:
			m_contentLen = value;
			m_confirmedLength = true;
			break;

		case 2:
			m_gzip = true;
			break;

		case 3:
			ParseFilename(line);
			break;

		case 4:
			ParseRedirect(line + value);
			m_redirected = true;
			break;
	}
}

void WebDownloader::ParseFilename(const char* contentDisposition)
{
	CString filename = WebUtil::ParseContentDispositionFilename(contentDisposition);
	if (filename.Empty())
	{
		return;
	}

	m_originalFilename = FileSystem::BaseFileName(filename);

	debug("OriginalFilename: %s", *m_originalFilename);
}

void WebDownloader::ParseRedirect(const char* location)
{
	// rust/src/webdownload.rs resolves the address
	NzbgetRsBuf buf = nzbget_rs_http_redirect(m_url, location);
	std::string newLocation(buf.data, buf.len);
	nzbget_rs_free(buf);
	// without the query: it carries api keys and passwords (indexer links do),
	// and this went into the log
	auto noQuery = [](const char* url) { return std::string(url, nzbget_rs_ntw_webdl_strip_query_len(url)); };
	detail("URL %s redirected to %s", noQuery(m_url).c_str(), noQuery(newLocation.c_str()).c_str());
	SetUrl(newLocation.c_str());
}

bool WebDownloader::Write(void* buffer, int len)
{
	if (!m_outFile.Active() && !PrepareFile())
	{
		return false;
	}

#ifndef DISABLE_GZIP
	if (m_gzip)
	{
		m_gUnzipStream->Write(buffer, len);
		const void *outBuf;
		int outLen = 1;
		while (outLen > 0)
		{
			GUnzipStream::EStatus gZStatus = m_gUnzipStream->Read(&outBuf, &outLen);

			if (gZStatus == GUnzipStream::zlError)
			{
				error("URL %s: GUnzip failed", *m_infoName);
				return false;
			}

			if (outLen > 0 && m_outFile.Write(outBuf, outLen) <= 0)
			{
				return false;
			}

			if (gZStatus == GUnzipStream::zlFinished)
			{
				m_confirmedLength = true;
				return true;
			}
		}
		return true;
	}
	else
#endif

	return m_outFile.Write(buffer, len) > 0;
}

bool WebDownloader::PrepareFile()
{
	// prepare file for writing

	const char* filename = m_outputFilename;
	if (!m_outFile.Open(filename, DiskFile::omWrite))
	{
		error("Could not %s file %s", "create", filename);
		return false;
	}
	if (g_Options->GetWriteBuffer() > 0)
	{
		m_outFile.SetWriteBuffer(g_Options->GetWriteBuffer() * 1024);
	}

	return true;
}

void WebDownloader::LogDebugInfo()
{
	info("      Web-Download: status=%i, LastUpdateTime=%s, filename=%s", m_status,
		*Util::FormatTime(m_lastUpdateTime.load()), FileSystem::BaseFileName(m_outputFilename));
}

void WebDownloader::Stop()
{
	debug("Trying to stop WebDownloader");
	Thread::Stop();
	Guard guard(m_connectionMutex);
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
	debug("WebDownloader stopped successfully");
}

void WebDownloader::FreeConnection()
{
	if (m_connection)
	{
		debug("Releasing connection");
		Guard guard(m_connectionMutex);
		if (m_connection->GetStatus() == Connection::csCancelled)
		{
			m_connection->Disconnect();
		}
		m_connection.reset();
	}
}
