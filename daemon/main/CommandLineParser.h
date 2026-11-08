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


#ifndef COMMANDLINEPARSER_H
#define COMMANDLINEPARSER_H

#include <exception>
#include <stdexcept>
#include "NString.h"
#include "Log.h"
#include "MessageBase.h"
#include "DownloadInfo.h"
#include "FileSystem.h"
#include "Util.h"
#include "nzbget_rs.h"

class CommandLineParser
{
public:
	enum EClientOperation
	{
		opClientNoOperation,
		opClientRequestDownload,
		opClientRequestListFiles,
		opClientRequestListGroups,
		opClientRequestListStatus,
		opClientRequestSetRate,
		opClientRequestDumpDebug,
		opClientRequestEditQueue,
		opClientRequestLog,
		opClientRequestShutdown,
		opClientRequestReload,
		opClientRequestVersion,
		opClientRequestPostQueue,
		opClientRequestWriteLog,
		opClientRequestScanSync,
		opClientRequestScanAsync,
		opClientRequestDownloadPause,
		opClientRequestDownloadUnpause,
		opClientRequestPostPause,
		opClientRequestPostUnpause,
		opClientRequestScanPause,
		opClientRequestScanUnpause,
		opClientRequestHistory,
		opClientRequestHistoryAll
	};
	enum EMatchMode
	{
		mmId = 1,
		mmName,
		mmRegEx
	};

	typedef std::vector<int> IdList;
	typedef std::vector<CString> NameList;

	CommandLineParser(int argc, const char* argv[])
	{
		if (argc < 1 || !argv)
		{
			throw std::invalid_argument("Invalid command line");
		}
		m_args.reserve(argc);
		for (int i = 0; i < argc; i++)
		{
			m_args.emplace_back(argv[i]);
		}

		std::vector<char*> args;
		args.reserve(static_cast<size_t>(argc) + 1);
		for (CString& arg : m_args) args.push_back(arg);
		args.push_back(nullptr);
		RsSink sink{this, args.data(), nullptr};
		NzbgetRsCmdlineSink rsSink{nullptr, &sink, RsSetInt, RsSetStr, RsSteal, RsPushOption, RsPushId, RsPushName, RsError};
#ifdef HAVE_GETOPT_LONG
		const int useLong = 1;
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

	void PrintUsage(const char* com)
	{
		NzbgetRsBuf buf = nzbget_rs_cmdline_format_usage(
			FileSystem::BaseFileName(com), PATH_SEPARATOR, 1);
		if (buf.data)
		{
			printf("%s", buf.data);
		}
		nzbget_rs_free(buf);
	}

	bool GetErrors() { return m_errors; }
	bool GetNoConfig() { return m_noConfig; }
	const char* GetConfigFilename() { return m_configFilename; }
	bool GetServerMode() { return m_serverMode; }
	bool GetDaemonMode() { return m_daemonMode; }
	bool GetRemoteClientMode() { return m_remoteClientMode; }
	EClientOperation GetClientOperation() { return m_clientOperation; }
	NameList* GetOptionList() { return &m_optionList; }
	int GetEditQueueAction() { return m_editQueueAction; }
	int GetEditQueueOffset() { return m_editQueueOffset; }
	IdList* GetEditQueueIdList() { return &m_editQueueIdList; }
	NameList* GetEditQueueNameList() { return &m_editQueueNameList; }
	EMatchMode GetMatchMode() { return m_matchMode; }
	const char* GetEditQueueText() { return m_editQueueText; }
	const char* GetArgFilename() { return m_argFilename; }
	const char* GetAddCategory() { return m_addCategory; }
	bool GetAutoCategory() const { return m_autoCategory; }
	bool GetAddPaused() { return m_addPaused; }
	const char* GetLastArg() { return m_lastArg; }
	int GetAddPriority() { return m_addPriority; }
	const char* GetAddNzbFilename() { return m_addNzbFilename; }
	bool GetAddTop() { return m_addTop; }
	const char* GetAddDupeKey() { return m_addDupeKey; }
	int GetAddDupeScore() { return m_addDupeScore; }
	int GetAddDupeMode() { return m_addDupeMode; }
	int GetSetRate() { return m_setRate; }
	int GetLogLines() { return m_logLines; }
	int GetWriteLogKind() { return m_writeLogKind; }
	bool GetTestBacktrace() { return m_testBacktrace; }
	bool GetWebGet() { return m_webGet; }
	const char* GetWebGetFilename() { return m_webGetFilename; }
	bool GetSigVerify() { return m_sigVerify; }
	const char* GetPubKeyFilename() { return m_pubKeyFilename; }
	const char* GetSigFilename() { return m_sigFilename; }
	bool GetPrintOptions() { return m_printOptions; }
	bool GetPrintVersion() { return m_printVersion; }
	bool GetPrintUsage() { return m_printUsage; }
	bool GetPauseDownload() const { return m_pauseDownload; }

private:
	void SetAddCategory(const char* category)
	{ 
		m_addCategory = category; 
		m_autoCategory = false; 
	}

	bool m_noConfig = false;
	CString m_configFilename;

	// Parsed command-line parameters
	bool m_errors = false;
	bool m_printVersion = false;
	bool m_printUsage = false;
	bool m_serverMode = false;
	bool m_daemonMode = false;
	bool m_remoteClientMode = false;
	EClientOperation m_clientOperation;
	NameList m_optionList;
	int m_editQueueAction = 0;
	int m_editQueueOffset = 0;
	IdList m_editQueueIdList;
	NameList m_editQueueNameList;
	EMatchMode m_matchMode = mmId;
	CString m_editQueueText;
	CString m_argFilename;
	CString m_addCategory;
	bool m_autoCategory = true;
	int m_addPriority = 0;
	bool m_addPaused = false;
	CString m_addNzbFilename;
	CString m_lastArg;
	bool m_printOptions = false;
	bool m_addTop = false;
	CString m_addDupeKey;
	int m_addDupeScore = 0;
	int m_addDupeMode = 0;
	int m_setRate = 0;
	int m_logLines = 0;
	int m_writeLogKind = 0;
	bool m_testBacktrace = false;
	bool m_webGet = false;
	CString m_webGetFilename;
	bool m_sigVerify = false;
	CString m_pubKeyFilename;
	CString m_sigFilename;
	bool m_pauseDownload = false;

	std::vector<CString> m_args;
	void ReportError(const char* errMessage)
	{
		m_errors = true;
		printf("%s\n", errMessage);
	}

	struct RsSink
	{
		CommandLineParser* parser;
		char** argv;
		std::exception_ptr error;

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

	static int RsSetInt(void* ctx, int field, int value)
	{
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

	static int RsSetStr(void* ctx, int field, const char* value)
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
				case 40: p->m_setRate = (int)(atof(value) * 1024); break;
			}
		});
	}

	static int RsSteal(void* ctx, int field, int index)
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

	static int RsPushOption(void* ctx, const char* value)
	{
		return RsSink::Call(ctx, [value](CommandLineParser* p) { p->m_optionList.push_back(value); });
	}

	static int RsPushId(void* ctx, int id)
	{
		return RsSink::Call(ctx, [id](CommandLineParser* p) { p->m_editQueueIdList.push_back(id); });
	}

	static int RsPushName(void* ctx, const char* value)
	{
		return RsSink::Call(ctx, [value](CommandLineParser* p) { p->m_editQueueNameList.push_back(value); });
	}

	static int RsError(void* ctx, const char* msg)
	{
		return RsSink::Call(ctx, [msg](CommandLineParser* p) { p->ReportError(msg); });
	}
};

#endif
