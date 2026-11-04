/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#include "nzbget.h"
#include "CommandLineParser.h"
#include "Log.h"
#include "MessageBase.h"
#include "DownloadInfo.h"
#include "FileSystem.h"
#include "Util.h"
#include <exception>
#include <stdexcept>
#include "nzbget_rs.h"

#ifdef HAVE_GETOPT_LONG
static struct option long_options[] =
	{
		{"help", no_argument, nullptr, 'h'},
		{"configfile", required_argument, nullptr, 'c'},
		{"noconfigfile", no_argument, nullptr, 'n'},
		{"printconfig", no_argument, nullptr, 'p'},
		{"server", no_argument, nullptr, 's' },
		{"daemon", no_argument, nullptr, 'D' },
		{"version", no_argument, nullptr, 'v'},
		{"serverversion", no_argument, nullptr, 'V'},
		{"option", required_argument, nullptr, 'o'},
		{"append", no_argument, nullptr, 'A'},
		{"list", no_argument, nullptr, 'L'},
		{"pause", no_argument, nullptr, 'P'},
		{"unpause", no_argument, nullptr, 'U'},
		{"rate", required_argument, nullptr, 'R'},
		{"system", no_argument, nullptr, 'B'},
		{"log", required_argument, nullptr, 'G'},
		{"top", no_argument, nullptr, 'T'},
		{"edit", required_argument, nullptr, 'E'},
		{"connect", no_argument, nullptr, 'C'},
		{"quit", no_argument, nullptr, 'Q'},
		{"reload", no_argument, nullptr, 'O'},
		{"write", required_argument, nullptr, 'W'},
		{"category", required_argument, nullptr, 'K'},
		{"scan", no_argument, nullptr, 'S'},
		{nullptr, 0, nullptr, 0}
	};
#endif

static char short_options[] = "c:hno:psvAB:DCE:G:K:LPR:STUQOVW:";


// a callback that threw stops the parser; the exception is rethrown after
// Rust returned
struct CommandLineParser::RsSink
{
	CommandLineParser* parser;
	char** argv;
	std::exception_ptr error;

	// getopt permutes pointers, not CString objects. Transfer the existing
	// ownership into that order without aliasing a vector<CString> as char**.
	void SyncArgs() noexcept
	{
		for (CString& arg : parser->m_args) arg.Unbind();
		for (size_t i = 0; i < parser->m_args.size(); i++) parser->m_args[i].Bind(argv[i]);
	}

	template <typename F>
	static int Call(void* ctx, F f) noexcept
	{
		RsSink* sink = static_cast<RsSink*>(ctx);
		try
		{
			f(sink->parser);
			return 0;
		}
		catch (...)
		{
			sink->error = std::current_exception();
			return -1;
		}
	}
};

int CommandLineParser::RsSetInt(void* ctx, int field, int value)
{
	// in the order of rust/src/cmdline.rs: edit, and the dupe modes and log kinds
	static const int EDIT_ACTIONS[] = {
		DownloadQueue::eaPostDelete, DownloadQueue::eaHistoryDelete, DownloadQueue::eaHistoryReturn,
		DownloadQueue::eaHistoryProcess, DownloadQueue::eaHistoryRedownload, DownloadQueue::eaHistoryRetryFailed,
		DownloadQueue::eaHistorySetParameter, DownloadQueue::eaHistoryMarkBad, DownloadQueue::eaHistoryMarkGood,
		DownloadQueue::eaHistoryMarkSuccess, DownloadQueue::eaGroupMoveTop, DownloadQueue::eaFileMoveTop,
		DownloadQueue::eaGroupMoveBottom, DownloadQueue::eaFileMoveBottom, DownloadQueue::eaGroupPause,
		DownloadQueue::eaFilePause, DownloadQueue::eaGroupPauseAllPars, DownloadQueue::eaFilePauseAllPars,
		DownloadQueue::eaGroupPauseExtraPars, DownloadQueue::eaFilePauseExtraPars, DownloadQueue::eaGroupResume,
		DownloadQueue::eaFileResume, DownloadQueue::eaGroupDelete, DownloadQueue::eaFileDelete,
		DownloadQueue::eaGroupParkDelete, DownloadQueue::eaGroupSortFiles, DownloadQueue::eaGroupApplyCategory,
		DownloadQueue::eaGroupSetCategory, DownloadQueue::eaGroupSetName, DownloadQueue::eaGroupMerge,
		DownloadQueue::eaFileSplit, DownloadQueue::eaGroupSetParameter, DownloadQueue::eaGroupSetPriority,
		DownloadQueue::eaGroupMoveOffset, DownloadQueue::eaFileMoveOffset};
	static const int DUPE_MODES[] = {dmScore, dmAll, dmForce};
	static const int LOG_KINDS[] = {(int)Message::mkInfo, (int)Message::mkWarning, (int)Message::mkError,
		(int)Message::mkDetail, (int)Message::mkDebug};
	return RsSink::Call(ctx, [field, value](CommandLineParser* p)
	{
		switch (field)
		{
			case 0: p->m_noConfig = value; break;
			case 1: p->m_printUsage = value; break;
			case 2: p->m_printVersion = value; break;
			case 3: p->m_printOptions = value; break;
			case 4: p->m_serverMode = value; break;
			case 5: p->m_daemonMode = value; break;
			case 6: p->m_remoteClientMode = value; break;
			case 7: p->m_clientOperation = (EClientOperation)value; break;
			case 8: p->m_addTop = value; break;
			case 9: p->m_addPaused = value; break;
			case 10: p->m_addPriority = value; break;
			case 11: p->m_addDupeScore = value; break;
			case 12: p->m_addDupeMode = DUPE_MODES[value]; break;
			case 13: p->m_matchMode = (EMatchMode)value; break;
			case 15: p->m_testBacktrace = value; break;
			case 16: p->m_webGet = value; break;
			case 17: p->m_sigVerify = value; break;
			case 18: p->m_logLines = value; break;
			case 19: p->m_editQueueAction = EDIT_ACTIONS[value]; break;
			case 20: p->m_editQueueOffset = value; break;
			case 21: p->m_writeLogKind = LOG_KINDS[value]; break;
			case 22: p->m_pauseDownload = value; break;
			case 23: p->m_errors = value; break;
		}
	});
}

int CommandLineParser::RsSetStr(void* ctx, int field, const char* value)
{
	return RsSink::Call(ctx, [field, value](CommandLineParser* p)
	{
		switch (field)
		{
			case 30: p->m_configFilename = value; break;
			case 31: p->m_webGetFilename = value; break;
			case 32: p->m_pubKeyFilename = value; break;
			case 33: p->m_sigFilename = value; break;
			case 34: p->SetAddCategory(value); break;
			case 35: p->m_lastArg = value; break;
			case 36: p->m_argFilename = value; break;
			// Keep the original host conversion, including its platform-specific
			// result for NaN and out-of-range values (not portable C++ behavior).
			case 40: p->m_setRate = (int)(atof(value) * 1024); break;
		}
	});
}

int CommandLineParser::RsSteal(void* ctx, int field, int index)
{
	return RsSink::Call(ctx, [ctx, field, index](CommandLineParser* p)
	{
		RsSink* sink = static_cast<RsSink*>(ctx);
		sink->SyncArgs();
		CString& arg = p->m_args[index];
		switch (field)
		{
			case 37: p->m_addNzbFilename = std::move(arg); break;
			case 38: p->m_addDupeKey = std::move(arg); break;
			case 39: p->m_editQueueText = std::move(arg); break;
		}
		sink->argv[index] = arg;
	});
}

int CommandLineParser::RsPushOption(void* ctx, const char* value)
{
	return RsSink::Call(ctx, [value](CommandLineParser* p) { p->m_optionList.push_back(value); });
}

int CommandLineParser::RsPushId(void* ctx, int id)
{
	return RsSink::Call(ctx, [id](CommandLineParser* p) { p->m_editQueueIdList.push_back(id); });
}

int CommandLineParser::RsPushName(void* ctx, const char* value)
{
	return RsSink::Call(ctx, [value](CommandLineParser* p) { p->m_editQueueNameList.push_back(value); });
}

int CommandLineParser::RsError(void* ctx, const char* msg)
{
	return RsSink::Call(ctx, [msg](CommandLineParser* p) { p->ReportError(msg); });
}

#ifdef HAVE_GETOPT_LONG
static int RsGetoptLong(int argc, char** argv) noexcept
{
	int index = 0;
	return getopt_long(argc, argv, short_options, long_options, &index);
}
#endif

CommandLineParser::CommandLineParser(int argc, const char* argv[])
{
	if (argc < 1 || !argv)
	{
		throw std::invalid_argument("Invalid command line");
	}
	// getopt_long reorders it (non-options to the end): rust/src/cmdline.rs
	// parses it in place, as InitCommandLine did
	m_args.reserve(argc);
	for (int i = 0; i < argc; i++)
	{
		m_args.emplace_back(argv[i]);
	}

	std::vector<char*> args;
	args.reserve(static_cast<size_t>(argc) + 1);
	for (CString& arg : m_args) args.push_back(arg);
	args.push_back(nullptr); // getopt's argv[argc] sentinel
	RsSink sink{this, args.data(), nullptr};
	NzbgetRsCmdlineSink rsSink{nullptr, &sink, RsSetInt, RsSetStr, RsSteal, RsPushOption, RsPushId, RsPushName, RsError};
#ifdef HAVE_GETOPT_LONG
	const int useLong = 1;
	rsSink.getoptLong = RsGetoptLong;
#else
	const int useLong = 0;
#endif
	int result = nzbget_rs_cmdline_parse(argc, args.data(), useLong, &rsSink);
	sink.SyncArgs();
	if (sink.error)
	{
		std::rethrow_exception(sink.error);
	}
	if (result != 0)
	{
		throw std::runtime_error("Command line parsing failed");
	}
}

void CommandLineParser::PrintUsage(const char* com)
{
#ifdef WIN32
	const int includeDaemon = 0;
#else
	const int includeDaemon = 1;
#endif
	NzbgetRsBuf buf = nzbget_rs_cmdline_format_usage(
		FileSystem::BaseFileName(com), PATH_SEPARATOR, includeDaemon);
	if (buf.data)
	{
		printf("%s", buf.data);
	}
	nzbget_rs_free(buf);
}

void CommandLineParser::ReportError(const char* errMessage)
{
	m_errors = true;
	printf("%s\n", errMessage);
}
