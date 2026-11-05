/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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
#include "NntpServer.h"
#include "Log.h"
#include "Util.h"
#include "YEncoder.h"
#include "nzbget_rs.h"

#ifndef DISABLE_TLS
#include "OpenSSL.h"
#endif

class NntpProcessor : public Thread
{
public:
	NntpProcessor(int id, int serverId, const char* dataDir, const char* cacheDir,
		const char* secureCert, const char* secureKey, int latency, int speed, NntpCache* cache) :
		m_id(id), m_serverId(serverId), m_dataDir(dataDir), m_cacheDir(cacheDir),
		m_secureCert(secureCert), m_secureKey(secureKey), m_latency(latency),
		m_speed(speed), m_messageid(nullptr), m_part(0), m_offset(0), m_size(0),
		m_sendHeaders(false), m_sendStat(false), m_start(0), m_cache(cache) {}
	~NntpProcessor()
	{
		m_connection->Disconnect();

#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}
	void Run() override;
	void SetConnection(std::unique_ptr<Connection>&& connection) { m_connection = std::move(connection); }

private:
	int m_id;
	int m_serverId;
	std::unique_ptr<Connection> m_connection;
	const char* m_dataDir;
	const char* m_cacheDir;
	const char* m_secureCert;
	const char* m_secureKey;
	int m_latency;
	int m_speed;
	const char* m_messageid;
	CString m_filename;
	int m_part;
	int64 m_offset;
	int m_size;
	bool m_sendHeaders;
	bool m_sendStat;
	int64 m_start;
	NntpCache* m_cache;

	void ServArticle();
	void SendSegment();
	bool ServerInList(const char* servList);
	void SendData(const char* buffer, int size);
};


void NntpServer::Run()
{
	debug("Entering NntpServer-loop");

	info("%s Listening on port %i", *Util::FormatTime(Util::CurrentTime()), m_port);

	int num = 1;

	while (!IsStopped())
	{
		bool bind = true;

		if (!m_connection)
		{
			m_connection = std::make_unique<Connection>(m_host, m_port, m_secureCert);
			m_connection->SetTimeout(10);
			m_connection->SetSuppressErrors(false);
			bind = m_connection->Bind();
		}

		// Accept connections and store the new Connection
		std::unique_ptr<Connection> acceptedConnection;
		if (bind)
		{
			acceptedConnection = m_connection->Accept();
		}
		if (!bind || !acceptedConnection)
		{
			// Server could not bind or accept connection, waiting 1/2 sec and try again
			if (IsStopped())
			{
				break;
			}
			m_connection.reset();
			Util::Sleep(500);
			continue;
		}
		
		NntpProcessor* commandThread = new NntpProcessor(num++, m_id, m_dataDir,
			m_cacheDir, m_secureCert, m_secureKey, m_latency, m_speed, m_cache);
		commandThread->SetAutoDestroy(true);
		commandThread->SetConnection(std::move(acceptedConnection));
		commandThread->Start();
	}

	if (m_connection)
	{
		m_connection->Disconnect();
	}

	debug("Exiting NntpServer-loop");
}

void NntpServer::Stop()
{
	Thread::Stop();
	if (m_connection)
	{
		m_connection->SetSuppressErrors(true);
		m_connection->Cancel();
	}
}


void NntpProcessor::Run()
{
	m_connection->SetSuppressErrors(false);

#ifndef DISABLE_TLS
	if (m_secureCert && !m_connection->StartTls(false, m_secureCert, m_secureKey))
	{
		error("Could not establish secure connection to nntp-client: Start TLS failed");
		return;
	}
#endif

	info("%s [%i] Incoming connection from: %s", *Util::FormatTime(Util::CurrentTime()),m_id, m_connection->GetHost() );
	m_connection->WriteLine("200 Welcome (NServ)\r\n");

	CharBuffer buf(1024);
	int bytesRead = 0;
	while (CString line = m_connection->ReadLine(buf, 1024, &bytesRead))
	{
		line.TrimRight();
		detail("%s [%i] Received: %s", *Util::FormatTime(Util::CurrentTime()), m_id, *line);

		size_t argOffset = 0;
		int cmdKind = nzbget_rs_feh_nntp_classify_command(line, &argOffset);
		if (cmdKind == 1)
		{
			m_messageid = line + argOffset;
			m_sendHeaders = true;
			m_sendStat = false;
			ServArticle();
		}
		else if (cmdKind == 2)
		{
			m_messageid = line + argOffset;
			m_sendHeaders = false;
			m_sendStat = false;
			ServArticle();
		}
		else if (cmdKind == 3)
		{
			m_messageid = line + argOffset;
			m_sendHeaders = false;
			m_sendStat = true;
			ServArticle();
		}
		else if (cmdKind == 4)
		{
			m_connection->WriteLine(CString::FormatStr("211 0 0 0 %s\r\n", line + argOffset));
		}
		else if (cmdKind == 5)
		{
			m_connection->WriteLine("381 Enter passphrase\r\n");
		}
		else if (cmdKind == 6)
		{
			m_connection->WriteLine("281 Authentication accepted\r\n");
		}
		else if (cmdKind == 7)
		{
			detail("%s [%i] Closing connection", *Util::FormatTime(Util::CurrentTime()), m_id);
			m_connection->WriteLine("205 Connection closing\r\n");
			break;
		}
		else
		{
			warn("[%i] Unknown command: %s", m_id, *line);
			m_connection->WriteLine("500 Unknown command\r\n");
		}
	}

	m_connection->SetGracefull(true);
	m_connection->Disconnect();
}

/* 
 Message-id format:
   <file-path-relative-to-dataDir?xxx=yyy:zzz!1,2,3>
where:
   xxx   - part number (integer)
   xxx   - offset from which to read the files (integer)
   yyy   - size of file block to return (integer)
   1,2,3 - list of server ids, which have the article (optional),
           if the list is given and current server is not in the list
           the "article not found"-error is returned.
 Examples:
	<parchecker/testfile.dat?1=0:50000>	       - return first 50000 bytes starting from beginning
	<parchecker/testfile.dat?2=50000:50000>      - return 50000 bytes starting from offset 50000
	<parchecker/testfile.dat?2=50000:50000!2>    - article is missing on server 1
*/
void NntpProcessor::ServArticle()
{
	detail("%s [%i] Serving: %s", *Util::FormatTime(Util::CurrentTime()), m_id, m_messageid);

	if (m_latency)
	{
		Util::Sleep(m_latency);
	}

	bool ok = false;

	int nameLen = 0, part = 0, size = 0, servOff = -1;
	int64 offset = 0;
	if (nzbget_rs_feh_nntp_parse_message_id(
		m_messageid, &nameLen, &part, &offset, &size, &servOff))
	{
		m_filename.Set(m_messageid + 1, nameLen);
		m_part = part;
		m_offset = offset;
		m_size = size;

		ok = servOff < 0 || ServerInList(m_messageid + servOff);

		if (ok)
		{
			SendSegment();
			return;
		}

		m_connection->WriteLine("430 No Such Article Found\r\n");
	}
	else
	{
		m_connection->WriteLine("430 No Such Article Found (invalid message id format)\r\n");
	}
}

bool NntpProcessor::ServerInList(const char* servList)
{
	return nzbget_rs_feh_nntp_server_in_list(servList, m_serverId) != 0;
}

void NntpProcessor::SendSegment()
{
	detail("[%i] Sending segment %s (%i=%" PRIi64 ":%i)", m_id, *m_filename, m_part, m_offset, m_size);

	if (m_speed > 0)
	{
		m_start = Util::CurrentTicks();
	}

	NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		m_dataDir ? m_dataDir : "", '/', *m_filename);
	BString<1024> fullFilename;
	fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
	nzbget_rs_free(fullFnBuf);

	NzbgetRsBuf cacheDirBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		m_cacheDir ? m_cacheDir : "", '/', *m_filename);
	BString<1024> cacheFileDir;
	cacheFileDir.Set(cacheDirBuf.data ? cacheDirBuf.data : "", (int)cacheDirBuf.len);
	nzbget_rs_free(cacheDirBuf);

	NzbgetRsBuf cacheFnBuf = nzbget_rs_dqh_nntpserver_format_cache_file_name(m_part, m_offset, m_size);
	BString<1024> cacheFileName;
	cacheFileName.Set(cacheFnBuf.data ? cacheFnBuf.data : "", (int)cacheFnBuf.len);
	nzbget_rs_free(cacheFnBuf);

	NzbgetRsBuf cacheFullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		*cacheFileDir, '/', *cacheFileName);
	BString<1024> cacheFullFilename;
	cacheFullFilename.Set(cacheFullFnBuf.data ? cacheFullFnBuf.data : "", (int)cacheFullFnBuf.len);
	nzbget_rs_free(cacheFullFnBuf);

	NzbgetRsBuf cacheKeyBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		*m_filename, '/', *cacheFileName);
	BString<1024> cacheKey;
	cacheKey.Set(cacheKeyBuf.data ? cacheKeyBuf.data : "", (int)cacheKeyBuf.len);
	nzbget_rs_free(cacheKeyBuf);

	const char* cachedData = nullptr;
	int cachedSize;
	if (m_cache)
	{
		m_cache->Find(cacheKey, cachedData, cachedSize);
	}

	DiskFile cacheFile;
	bool readCache = !cachedData && m_cacheDir && cacheFile.Open(cacheFullFilename, DiskFile::omRead);
	bool writeCache = !cachedData && m_cacheDir && !readCache;
	StringBuilder cacheMem;
	if (m_cache && !cachedData)
	{
		cacheMem.Reserve((int)(m_size * 1.1));
	}

	CString errmsg;
	if (writeCache && !FileSystem::ForceDirectories(cacheFileDir, errmsg))
	{
		error("Could not create directory %s: %s", *cacheFileDir, *errmsg);
	}

	if (writeCache && !cacheFile.Open(cacheFullFilename, DiskFile::omWrite))
	{
		error("Could not create file %s: %s", *cacheFullFilename, *FileSystem::GetLastErrorMessage());
	}

	if (!cachedData && !readCache && !FileSystem::FileExists(fullFilename))
	{
		m_connection->WriteLine(CString::FormatStr("430 Article not found\r\n"));
		return;
	}

	YEncoder encoder(fullFilename, m_part, m_offset, m_size, 
		[proc = this, writeCache, &cacheFile, &cacheMem](const char* buf, int size)
		{
			if (proc->m_cache)
			{
				cacheMem.Append(buf);
			}
			if (writeCache)
			{
				cacheFile.Write(buf, size);
			}
			proc->SendData(buf, size);
		});

	if (!cachedData && !readCache && !encoder.OpenFile(errmsg))
	{
		m_connection->WriteLine(CString::FormatStr("403 %s\r\n", *errmsg));
		return;
	}

	// if we get here, everything is ok: input is OK, article exists

	if (m_sendStat)
	{
		m_connection->WriteLine(CString::FormatStr("223 0 %s\r\n", m_messageid));
		return; // done
	}

	m_connection->WriteLine(CString::FormatStr("%i 0 %s\r\n", m_sendHeaders ? 220 : 222, m_messageid));
	if (m_sendHeaders)
	{
		m_connection->WriteLine(CString::FormatStr("Message-ID: %s\r\n", m_messageid));
		m_connection->WriteLine(CString::FormatStr("Subject: \"%s\"\r\n", FileSystem::BaseFileName(m_filename)));
		m_connection->WriteLine("\r\n");
	}

	if (cachedData)
	{
		SendData(cachedData, cachedSize);
	}
	else if (readCache)
	{
		cacheFile.Seek(0, DiskFile::soEnd);
		int size = (int)cacheFile.Position();
		CharBuffer buf(size);
		cacheFile.Seek(0);
		if (cacheFile.Read((char*)buf, size) != size)
		{
			error("Could not read file %s: %s", *cacheFullFilename, *FileSystem::GetLastErrorMessage());
		}
		if (m_cache)
		{
			cacheMem.Append(buf, size);
		}
		SendData(buf, size);
	}
	else
	{
		encoder.WriteSegment();
	}

	if (!cachedData && cacheMem.Length() > 0)
	{
		m_cache->Append(cacheKey, cacheMem, cacheMem.Length());
	}

	m_connection->WriteLine(".\r\n");
}

void NntpProcessor::SendData(const char* buffer, int size)
{
	if (m_speed == 0)
	{
		m_connection->Send(buffer, size);
		return;
	}

	int64 expectedTime = 0;
	int chunkNum = 0, chunkSize = 0, pause = 0;
	nzbget_rs_feh_nntp_calc_send_chunks(
		size, m_speed, Util::CurrentTicks() - m_start,
		&expectedTime, &chunkNum, &chunkSize, &pause);

	int sent = 0;
	for (int i = 0; i < chunkNum; i++)
	{
		int len = sent + chunkSize < size ? chunkSize : size - sent;

		while (sent + len < size && *(buffer + sent + len) != '\r')
		{
			len++;
		}
		
		m_connection->Send(buffer + sent, len);
		int64 now = Util::CurrentTicks();
		if (now + pause * 1000 < m_start + expectedTime * 1000)
		{
			Util::Sleep(pause);
		}
		sent += len;
	}
}


void NntpCache::Append(const char* key, const char* data, int len)
{
	Guard guard(m_lock);
	if (!len)
	{
		len = strlen(data);
	}
	m_items.emplace(key, std::make_unique<CacheItem>(key, data, len));
}

bool NntpCache::Find(const char* key, const char*& data, int& size)
{
	Guard guard(m_lock);

	CacheMap::iterator pos = m_items.find(key);
	if (pos != m_items.end())
	{
		data = (*pos).second->m_data;
		size = (*pos).second->m_size;
		return true;
	}

	return false;
}
