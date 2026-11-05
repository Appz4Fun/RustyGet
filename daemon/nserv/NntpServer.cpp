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
		m_speed(speed), m_cache(cache) {}
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
	NntpCache* m_cache;
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

	info("%s [%i] Incoming connection from: %s", *Util::FormatTime(Util::CurrentTime()), m_id, m_connection->GetHost());
	m_connection->WriteLine("200 Welcome (NServ)\r\n");

	CharBuffer buf(1024);
	int bytesRead = 0;
	while (CString line = m_connection->ReadLine(buf, 1024, &bytesRead))
	{
		int keepGoing = nzbget_rs_nntp_processor_handle_line(
			m_serverId,
			m_dataDir,
			m_cacheDir,
			m_latency,
			m_speed,
			m_cache ? m_cache->GetRustHandle() : nullptr,
			*line,
			this,
			[](void* rawCtx, const char* replyLine) {
				static_cast<NntpProcessor*>(rawCtx)->m_connection->WriteLine(replyLine ? replyLine : "");
			},
			[](void* rawCtx, const char* data, int size) {
				static_cast<NntpProcessor*>(rawCtx)->m_connection->Send(data, size);
			},
			[](void* rawCtx, int kind, const char* msg) {
				auto* self = static_cast<NntpProcessor*>(rawCtx);
				switch (kind)
				{
				case 0:
					detail("%s [%i] %s", *Util::FormatTime(Util::CurrentTime()), self->m_id, msg ? msg : "");
					break;
				case 1:
					detail("[%i] %s", self->m_id, msg ? msg : "");
					break;
				case 2:
					warn("[%i] %s", self->m_id, msg ? msg : "");
					break;
				case 3:
					error("%s", msg ? msg : "");
					break;
				}
			});
		if (!keepGoing)
		{
			break;
		}
	}

	m_connection->SetGracefull(true);
	m_connection->Disconnect();
}

NntpCache::NntpCache()
	: m_handle(nzbget_rs_nntp_cache_new())
{
}

NntpCache::~NntpCache()
{
	nzbget_rs_nntp_cache_free(m_handle);
}

void NntpCache::Append(const char* key, const char* data, int len)
{
	nzbget_rs_nntp_cache_append(m_handle, key, data, len);
}

bool NntpCache::Find(const char* key, const char*& data, int& size)
{
	return nzbget_rs_nntp_cache_find(m_handle, key, &data, &size) != 0;
}
