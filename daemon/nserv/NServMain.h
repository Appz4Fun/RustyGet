/*
 *  This file is part of RustyGet.
 *
 *  Copyright (C) 2016-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef NSERVMAIN_H
#define NSERVMAIN_H

#include <cstdio>
#include "NString.h"
#include "Thread.h"
#include "Connection.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "Options.h"
#include "nzbget_rs.h"

class NzbGenerator
{
public:
	NzbGenerator(const char* dataDir, int segmentSize) :
		m_dataDir(dataDir), m_segmentSize(segmentSize) {};

	void Execute()
	{
		auto logCb = +[](int kind, const char* msg)
		{
			switch (kind)
			{
			case 0:
				info("Generating nzbs for %s", msg);
				break;
			case 1:
				info("Generating nzb for %s", msg);
				break;
			case 2:
				error("Could not create file %s", msg);
				break;
			case 3:
				detail("Processing %s", msg);
				break;
			case 4:
				info("Nzb generation finished");
				break;
			}
		};
		nzbget_rs_nzb_generator_execute(m_dataDir, m_segmentSize, logCb);
	}

private:
	CString m_dataDir;
	int m_segmentSize;
};

class NServFrontend : public Thread
{
public:
	NServFrontend() = default;

private:
	uint32 m_neededLogEntries = 0;
	uint32 m_neededLogFirstId = 0;
	bool m_needGoBack = false;

	void Run()
	{
		while (!IsStopped())
		{
			Update();
			Util::Sleep(100);
		}
		Update();
	}

	void Update()
	{
		BeforePrint();

		{
			GuardedMessageList messages = g_Log->GuardMessages();
			if (!messages->empty())
			{
				Message& firstMessage = messages->front();
				int needSkip = 0;
				int start = nzbget_rs_nserv_frontend_calc_start_index(
					m_neededLogFirstId, firstMessage.GetId(), &needSkip);
				if (needSkip)
				{
					PrintSkip();
				}
				for (uint32 i = (uint32)start; i < messages->size(); i++)
				{
					PrintMessage(messages->at(i));
					m_neededLogFirstId = messages->at(i).GetId();
				}
			}
		}

		fflush(stdout);
	}

	void BeforePrint()
	{
		if (m_needGoBack)
		{
			printf("\r\033[1A");
			m_needGoBack = false;
		}
	}

	void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			1, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	void PrintSkip()
	{
		printf(".....\033[K\n");
	}
};

struct NzbgetRsNntpCache;

class NntpCache
{
public:
	NntpCache() : m_handle(nzbget_rs_nntp_cache_new()) {}
	~NntpCache() { nzbget_rs_nntp_cache_free(m_handle); }
	NntpCache(const NntpCache&) = delete;
	NntpCache& operator=(const NntpCache&) = delete;
	const NzbgetRsNntpCache* GetRustHandle() const { return m_handle; }

private:
	NzbgetRsNntpCache* m_handle = nullptr;
};

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

	void Run() override
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

class NntpServer : public Thread
{
public:
	NntpServer(int id, const char* host, int port, const char* secureCert,
		const char* secureKey, const char* dataDir, const char* cacheDir,
		int latency, int speed, NntpCache* cache) :
		m_id(id), m_host(host), m_port(port), m_secureCert(secureCert),
		m_secureKey(secureKey), m_dataDir(dataDir), m_cacheDir(cacheDir),
		m_latency(latency), m_speed(speed), m_cache(cache) {}

	virtual void Run()
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

	virtual void Stop()
	{
		Thread::Stop();
		if (m_connection)
		{
			m_connection->SetSuppressErrors(true);
			m_connection->Cancel();
		}
	}

private:
	int m_id;
	CString m_host;
	int m_port;
	CString m_secureCert;
	CString m_secureKey;
	std::unique_ptr<Connection> m_connection;
	CString m_dataDir;
	CString m_cacheDir;
	int m_latency;
	int m_speed;
	NntpCache* m_cache;
};

struct NServOpts
{
	CString dataDir;
	CString cacheDir;
	CString bindAddress;
	int firstPort = 6791;
	int instances = 1;
	CString logFile;
	CString secureCert;
	CString secureKey;
	std::deque<CString> ownedOpts;
	bool generateNzb = false;
	int segmentSize = 500000;
	bool quit = false;
	int latency = 0;
	int speed = 0;
	bool memCache = false;
	bool paramError = false;

	NServOpts(int argc, char* argv[], Options::CmdOptList& cmdOpts)
		: bindAddress("0.0.0.0")
	{
		struct ParseCtx
		{
			NServOpts& self;
			Options::CmdOptList& list;
		} parseCtx{*this, cmdOpts};

		nzbget_rs_nserv_parse_args(
			argc,
			const_cast<const char* const*>(argv),
			&parseCtx,
			[](void* rawCtx,
				const char* dDir,
				const char* cDir,
				const char* bAddr,
				int fPort,
				int inst,
				const char* lFile,
				const char* sCert,
				const char* sKey,
				int genNzb,
				int segSize,
				int q,
				int lat,
				int spd,
				int mCache,
				int pErr) {
				auto& s = static_cast<ParseCtx*>(rawCtx)->self;
				s.dataDir = dDir;
				s.cacheDir = cDir;
				s.bindAddress = bAddr ? bAddr : "0.0.0.0";
				s.firstPort = fPort;
				s.instances = inst;
				s.logFile = lFile;
				s.secureCert = sCert;
				s.secureKey = sKey;
				s.generateNzb = genNzb != 0;
				s.segmentSize = segSize;
				s.quit = q != 0;
				s.latency = lat;
				s.speed = spd;
				s.memCache = mCache != 0;
				s.paramError = pErr != 0;
			},
			[](void* rawCtx, const char* opt) {
				auto* ctx = static_cast<ParseCtx*>(rawCtx);
				ctx->self.ownedOpts.emplace_back(opt ? opt : "");
				ctx->list.push_back(*ctx->self.ownedOpts.back());
			});
	}
};

inline void NServPrintUsage(const char* com)
{
	NzbgetRsBuf buf = nzbget_rs_nserv_format_usage(FileSystem::BaseFileName(com));
	if (buf.data)
	{
		fwrite(buf.data, 1, buf.len, stdout);
	}
	nzbget_rs_free(buf);
}

inline int NServMain(int argc, char* argv[])
{
	Log log;

	info("%s NServ %s (Test NNTP server)", *Util::FormatTime(Util::CurrentTime()), Util::VersionRevision());

	Options::CmdOptList cmdOpts;
	NServOpts opts(argc, argv, cmdOpts);

	if (opts.dataDir.Empty() || opts.paramError)
	{
		NServPrintUsage(argv[0]);
		return 1;
	}

	if (!FileSystem::DirectoryExists(opts.dataDir))
	{
		if (FileSystem::FileExists(opts.dataDir))
		{
			error("Specified data-dir %s is not a directory, but a file", *opts.dataDir);
		}
		else
		{
			error("Specified data-dir %s does not exist", *opts.dataDir);
		}
	}

	Options options(&cmdOpts, nullptr);

	log.InitOptions();
	Connection::Init();
#ifndef DISABLE_TLS
	TlsSocket::Init();
#endif

	signal(SIGPIPE, SIG_IGN);

	NServFrontend frontend;
	frontend.Start();

	if (opts.generateNzb)
	{
		NzbGenerator gen(opts.dataDir, opts.segmentSize);
		gen.Execute();
		if (opts.quit)
		{
			return 0;
		}
	}

	CString errmsg;
	if (opts.cacheDir && !FileSystem::ForceDirectories(opts.cacheDir, errmsg))
	{
		error("Could not create directory %s: %s", *opts.cacheDir, *errmsg);
	}

	std::vector<std::unique_ptr<NntpServer>> instances;
	NntpCache cache;

	for (int i = 0; i < opts.instances; i++)
	{
		instances.emplace_back(std::make_unique<NntpServer>(i + 1, opts.bindAddress,
			opts.firstPort + i, opts.secureCert, opts.secureKey, opts.dataDir, opts.cacheDir,
			opts.latency, opts.speed, opts.memCache ? &cache : nullptr));
		instances.back()->Start();
	}

	info("Press Ctrl+C to quit");
	while (getchar()) Util::Sleep(200);

	for (std::unique_ptr<NntpServer>& serv: instances)
	{
		serv->Stop();
	}
	frontend.Stop();

	bool hasRunning = false;
	do
	{
		hasRunning = frontend.IsRunning();
		for (std::unique_ptr<NntpServer>& serv : instances)
		{
			hasRunning |= serv->IsRunning();
		}
		Util::Sleep(50);
	} while (hasRunning);

	return 0;
}

#endif
