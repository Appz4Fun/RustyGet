/*
 *  This file is part of nzbget. See <https://nzbget.com>.
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


#include "nzbget.h"

#include "Thread.h"
#include "Connection.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "NServFrontend.h"
#include "NntpServer.h"
#include "NzbGenerator.h"
#include "Options.h"
#include "nzbget_rs.h"

struct NServOpts
{
	CString dataDir;
	CString cacheDir;
	CString bindAddress;
	int firstPort;
	int instances;
	CString logFile;
	CString secureCert;
	CString secureKey;
	BString<1024> logOpt;
	bool generateNzb;
	int segmentSize;
	bool quit;
	int latency;
	int speed;
	bool memCache;
	bool paramError;

	NServOpts(int argc, char* argv[], Options::CmdOptList& cmdOpts);
};

void NServPrintUsage(const char* com);

int NServMain(int argc, char* argv[])
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
		// dataDir does not exist. Let's find out a bit more, and report:
		if (FileSystem::FileExists(opts.dataDir))
		{
			error("Specified data-dir %s is not a directory, but a file", *opts.dataDir );
		} else {
			error("Specified data-dir %s does not exist", *opts.dataDir );
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

void NServPrintUsage(const char* com)
{
	NzbgetRsBuf buf = nzbget_rs_nserv_format_usage(FileSystem::BaseFileName(com));
	if (buf.data)
	{
		fwrite(buf.data, 1, buf.len, stdout);
	}
	nzbget_rs_free(buf);
}

NServOpts::NServOpts(int argc, char* argv[], Options::CmdOptList& cmdOpts)
	: bindAddress("0.0.0.0")
{
	instances = 1;
	firstPort = 6791;
	generateNzb = false;
	segmentSize = 500000;
	quit = false;
	latency = 0;
	memCache = false;
	speed = 0;
	paramError = false;
	int verbosity = 2;

	char short_options[] = "b:c:d:l:p:i:ms:v:w:r:z:q";

	optind = 2;
	while (true)
	{
		int c = getopt(argc, argv, short_options);
		if (c == -1) break;
		switch (c)
		{
			case 'd':
				dataDir = optind > argc ? nullptr : argv[optind - 1];
				break;

			case 'c':
				cacheDir = optind > argc ? nullptr : argv[optind - 1];
				break;

			case 'm':
				memCache = true;
				break;

			case 'l':
				logFile = optind > argc ? nullptr : argv[optind - 1];
				break;

			case 'b':
				bindAddress= optind > argc ? "0.0.0.0" : argv[optind - 1];
				break;

			case 'p':
				firstPort = atoi(optind > argc ? "6791" : argv[optind - 1]);
				break;

			case 's':
				secureCert = optind > argc ? nullptr : argv[optind - 1];
				optind++;
				secureKey = optind > argc ? nullptr : argv[optind - 1];
				break;

			case 'i':
				instances = atoi(optind > argc ? "1" : argv[optind - 1]);
				break;

			case 'v':
				verbosity = atoi(optind > argc ? "1" : argv[optind - 1]);
				break;

			case 'w':
				latency = atoi(optind > argc ? "0" : argv[optind - 1]);
				break;

			case 'r':
				speed = atoi(optind > argc ? "0" : argv[optind - 1]);
				break;

			case 'z':
				generateNzb = true;
				segmentSize = atoi(optind > argc ? "500000" : argv[optind - 1]);
				break;

			case 'q':
				quit = true;
				break;
		}
	}

	if (optind < argc)
	{
		paramError = true;
	}

	if (logFile.Empty())
	{
		cmdOpts.push_back("WriteLog=none");
	}
	else
	{
		cmdOpts.push_back("WriteLog=append");
		logOpt.Format("LogFile=%s", *logFile);
		cmdOpts.push_back(logOpt);
	}

	int muteInfoWarnErr = 0;
	int muteDetail = 0;
	int enableDebug = 0;
	nzbget_rs_snh_nserv_verbosity_flags(verbosity, &muteInfoWarnErr, &muteDetail, &enableDebug);
	if (muteInfoWarnErr)
	{
		cmdOpts.push_back("InfoTarget=none");
		cmdOpts.push_back("WarningTarget=none");
		cmdOpts.push_back("ErrorTarget=none");
	}
	if (muteDetail)
	{
		cmdOpts.push_back("DetailTarget=none");
	}
	if (enableDebug)
	{
		cmdOpts.push_back("DebugTarget=both");
	}
}
