/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef LOG_H

#include <mutex>
#include <string>
#include "NString.h"
#include "Thread.h"
#include "FileSystem.h"

#ifndef LOG_H
#define LOG_H

void error(const char* msg, ...) PRINTF_SYNTAX(1);
void warn(const char* msg, ...) PRINTF_SYNTAX(1);
void info(const char* msg, ...) PRINTF_SYNTAX(1);
void detail(const char* msg, ...) PRINTF_SYNTAX(1);

#ifdef DEBUG
#ifdef HAVE_VARIADIC_MACROS
void debug(const char* filename, const char* funcname, int lineNr, const char* msg, ...) PRINTF_SYNTAX(4);
#else
void debug(const char* msg, ...) PRINTF_SYNTAX(1);
#endif
#endif

class Message
{
public:
	enum EKind
	{
		mkInfo,
		mkWarning,
		mkError,
		mkDebug,
		mkDetail
	};

	Message(uint32 id, EKind kind, time_t time, const char* text) :
		m_id(id), m_kind(kind), m_time(time), m_text(text) {}
	uint32 GetId() { return m_id; }
	EKind GetKind() { return m_kind; }
	time_t GetTime() { return m_time; }
	const char* GetText() { return m_text; }

private:
	uint32 m_id;
	EKind m_kind;
	time_t m_time;
	CString m_text;

	friend class Log;
};

typedef std::deque<Message> MessageList;
typedef GuardedPtr<MessageList> GuardedMessageList;

class Debuggable;
class DiskFile;

class Log
{
public:
	Log();
	~Log();
	GuardedMessageList GuardMessages() { return GuardedMessageList(&m_messages, &m_logMutex); }
	void Clear();
	void ResetLog();
	void InitOptions();
	void RegisterDebuggable(Debuggable* debuggable);
	void UnregisterDebuggable(Debuggable* debuggable);
	void LogDebugInfo();
	void IntervalCheck();
	std::string GetLogFilename() const;
	fs::path GetLogFilePath() const;
	void UpdateLogPath(const char* newPath);

private:
	typedef std::list<Debuggable*> Debuggables;

	mutable std::mutex m_pathMutex;
	std::mutex m_debugMutex;
	Mutex m_logMutex;
	MessageList m_messages;
	Debuggables m_debuggables;
	std::string m_logFilename;
	fs::path m_logFilePath;
	std::unique_ptr<DiskFile> m_logFile;
	uint32 m_idGen = 0;
	std::atomic<time_t> m_lastWritten{0};
	bool m_optInit = false;
#ifdef DEBUG
	bool m_extraDebug;
#endif

	void Filelog(const char* msg, ...) PRINTF_SYNTAX(2);
	void AddMessage(Message::EKind kind, const char* text);
	void RotateLog();

	friend void error(const char* msg, ...);
	friend void warn(const char* msg, ...);
	friend void info(const char* msg, ...);
	friend void detail(const char* msg, ...);
#ifdef DEBUG
#ifdef HAVE_VARIADIC_MACROS
	friend void debug(const char* filename, const char* funcname, int lineNr, const char* msg, ...);
#else
	friend void debug(const char* msg, ...);
#endif
#endif
};


extern Log* g_Log;

class Debuggable
{
public:
	Debuggable() { g_Log->RegisterDebuggable(this); }
	virtual ~Debuggable() { g_Log->UnregisterDebuggable(this); }
protected:
	virtual void LogDebugInfo() = 0;
	friend class Log;
};

#include "Options.h"
#include "nzbget_rs.h"
#include "Util.h"
#include "FileSystem.h"

inline Log::Log()
{
	g_Log = this;

#ifdef DEBUG
	m_extraDebug = FileSystem::FileExists("extradebug");
#endif
}

inline Log::~Log()
{
	g_Log = nullptr;
}

inline void Log::LogDebugInfo()
{
	info("--------------------------------------------");
	info("Dumping debug info to log");
	info("--------------------------------------------");

	std::lock_guard<std::mutex> guard(m_debugMutex);
	for (Debuggable* debuggable : m_debuggables)
	{
		debuggable->LogDebugInfo();
	}

	info("--------------------------------------------");
}

inline void Log::Filelog(const char* msg, ...)
{
	if (m_logFilename.empty())
	{
		return;
	}

	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	time_t rawtime = Util::CurrentTime() + g_Options->GetTimeCorrection();

	char time[50];
	Util::FormatTime(rawtime, time, 50);

	if (nzbget_rs_rco_log_should_rotate(
		(int64)rawtime, (int64)m_lastWritten.load(), (int)g_Options->GetWriteLog()))
	{
		if (m_logFile)
		{
			m_logFile.reset();
		}
		RotateLog();
	}

	m_lastWritten = rawtime;

	if (!m_logFile)
	{
		m_logFile = std::make_unique<DiskFile>();
		if (!m_logFile->Open(m_logFilename.c_str(), DiskFile::omAppend))
		{
			perror(m_logFilename.c_str());
			m_logFile.reset();
			return;
		}

		if (getuid() == 0 || geteuid() == 0)
		{
			struct passwd* pwd = getpwnam(g_Options->GetDaemonUsername());
			int fileDescriptor = m_logFile->GetFileDescriptor();
			if (pwd != nullptr && fileDescriptor >= 0)
			{
				fchown(fileDescriptor, pwd->pw_uid, pwd->pw_gid);
			}
		}
	}

	m_logFile->Seek(0, DiskFile::soEnd);

#ifdef DEBUG
	uint64 processId = (uint64)getpid();
	uint64 threadId = (uint64)pthread_self();
	m_logFile->Print("%s\t%" PRIu64 "\t%" PRIu64 "\t%s%s", time, processId, threadId, tmp2, LINE_ENDING);
#else
	m_logFile->Print("%s\t%s%s", time, tmp2, LINE_ENDING);
#endif

	m_logFile->Flush();
}

inline void Log::IntervalCheck()
{
	// Close log-file on idle (if last write into log was more than a second ago)
	Guard guard(m_logMutex);
	if (m_logFile)
	{
		time_t curTime = Util::CurrentTime() + g_Options->GetTimeCorrection();
		if (nzbget_rs_rol_log_should_close_idle((int64)curTime, (int64)m_lastWritten.load()))
		{
			m_logFile.reset();
		}
	}
}

#ifdef DEBUG
#ifdef HAVE_VARIADIC_MACROS
inline void debug(const char* filename, const char* funcname, int lineNr, const char* msg, ...)
#else
inline void debug(const char* msg, ...)
#endif
{
	if (!g_Log)
	{
		return;
	}

	char tmp1[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp1, 1024, msg, ap);
	tmp1[1024-1] = '\0';
	va_end(ap);

	BString<1024> tmp2;
#ifdef HAVE_VARIADIC_MACROS
	NzbgetRsBuf dbgBuf = nzbget_rs_dqh_log_format_debug_line(
		tmp1, FileSystem::BaseFileName(filename), lineNr, funcname);
	tmp2.Set(dbgBuf.data ? dbgBuf.data : "", (int)dbgBuf.len);
	nzbget_rs_free(dbgBuf);
#else
	tmp2.Set(tmp1);
#endif

	Guard guard(g_Log->m_logMutex);

	if (!g_Options && g_Log->m_extraDebug)
	{
		printf("%s\n", *tmp2);
	}

	Options::EMessageTarget messageTarget = g_Options ? g_Options->GetDebugTarget() : Options::mtScreen;
	if (messageTarget == Options::mtScreen || messageTarget == Options::mtBoth)
	{
		g_Log->AddMessage(Message::mkDebug, tmp2);
	}
	if (messageTarget == Options::mtLog || messageTarget == Options::mtBoth)
	{
		g_Log->Filelog("DEBUG\t%s", *tmp2);
	}
}
#endif

inline void error(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	Guard guard(g_Log->m_logMutex);

	Options::EMessageTarget messageTarget = g_Options ? g_Options->GetErrorTarget() : Options::mtBoth;
	if (messageTarget == Options::mtScreen || messageTarget == Options::mtBoth)
	{
		g_Log->AddMessage(Message::mkError, tmp2);
	}
	if (messageTarget == Options::mtLog || messageTarget == Options::mtBoth)
	{
		g_Log->Filelog("ERROR\t%s", tmp2);
	}
}

inline void warn(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	Guard guard(g_Log->m_logMutex);

	Options::EMessageTarget messageTarget = g_Options ? g_Options->GetWarningTarget() : Options::mtScreen;
	if (messageTarget == Options::mtScreen || messageTarget == Options::mtBoth)
	{
		g_Log->AddMessage(Message::mkWarning, tmp2);
	}
	if (messageTarget == Options::mtLog || messageTarget == Options::mtBoth)
	{
		g_Log->Filelog("WARNING\t%s", tmp2);
	}
}

inline void info(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	Guard guard(g_Log->m_logMutex);

	Options::EMessageTarget messageTarget = g_Options ? g_Options->GetInfoTarget() : Options::mtScreen;
	if (messageTarget == Options::mtScreen || messageTarget == Options::mtBoth)
	{
		g_Log->AddMessage(Message::mkInfo, tmp2);
	}
	if (messageTarget == Options::mtLog || messageTarget == Options::mtBoth)
	{
		g_Log->Filelog("INFO\t%s", tmp2);
	}
}

inline void detail(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	Guard guard(g_Log->m_logMutex);

	Options::EMessageTarget messageTarget = g_Options ? g_Options->GetDetailTarget() : Options::mtScreen;
	if (messageTarget == Options::mtScreen || messageTarget == Options::mtBoth)
	{
		g_Log->AddMessage(Message::mkDetail, tmp2);
	}
	if (messageTarget == Options::mtLog || messageTarget == Options::mtBoth)
	{
		g_Log->Filelog("DETAIL\t%s", tmp2);
	}
}


inline void Log::Clear()
{
	Guard guard(m_logMutex);
	m_messages.clear();
}

inline void Log::AddMessage(Message::EKind kind, const char * text)
{
	m_messages.emplace_back(++m_idGen, kind, Util::CurrentTime(), text);

	if (m_optInit && g_Options)
	{
		while (m_messages.size() > (uint32)g_Options->GetLogBuffer())
		{
			m_messages.pop_front();
		}
	}
}

inline void Log::ResetLog()
{
	FileSystem::DeleteFile(g_Options->GetLogFile());
}

inline void Log::RotateLog()
{
	BString<1024> directory = g_Options->GetLogFile();

	// split the full filename into path, basename and extension
	char* baseName = FileSystem::BaseFileName(directory);
	if (baseName > directory)
	{
		baseName[-1] = '\0';
	}

	NzbgetRsBuf extBuf{};
	NzbgetRsBuf stemBuf = nzbget_rs_dqh_log_split_basename_and_ext(baseName, &extBuf);
	BString<1024> baseExt;
	baseExt.Set(extBuf.data ? extBuf.data : "", (int)extBuf.len);
	nzbget_rs_free(extBuf);
	if (stemBuf.data)
	{
		memcpy(baseName, stemBuf.data, stemBuf.len);
		baseName[stemBuf.len] = '\0';
	}
	nzbget_rs_free(stemBuf);

	NzbgetRsBuf maskBuf = nzbget_rs_dqh_log_format_rotate_mask(baseName, *baseExt);
	BString<1024> fileMask;
	fileMask.Set(maskBuf.data ? maskBuf.data : "", (int)maskBuf.len);
	nzbget_rs_free(maskBuf);

	time_t curTime = Util::CurrentTime() + g_Options->GetTimeCorrection();
	int curDay = (int)curTime / 86400;
	BString<1024> fullFilename;

	WildMask mask(fileMask, true);
	DirBrowser dir(directory);
	while (const char* filename = dir.Next())
	{
		if (mask.Match(filename))
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				*directory, PATH_SEPARATOR, filename);
			fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
			nzbget_rs_free(fullFnBuf);

			struct tm tm;
			memset(&tm, 0, sizeof(tm));
			nzbget_rs_dqh_log_parse_rotated_date_fields(
				filename,
				(size_t)mask.GetMatchStart(0),
				(size_t)mask.GetMatchStart(1),
				(size_t)mask.GetMatchStart(2),
				&tm.tm_year,
				&tm.tm_mon,
				&tm.tm_mday);
			time_t fileTime = Util::Timegm(&tm);
			int fileDay = (int)fileTime / 86400;

			if (nzbget_rs_rol_log_should_delete_old_rotated(fileDay, curDay, g_Options->GetRotateLog()))
			{
				NzbgetRsBuf delMsgBuf = nzbget_rs_dqh_log_format_deleting_old_msg(filename);
				BString<1024> message;
				message.Set(delMsgBuf.data ? delMsgBuf.data : "", (int)delMsgBuf.len);
				nzbget_rs_free(delMsgBuf);
				g_Log->AddMessage(Message::mkInfo, message);

				FileSystem::DeleteFile(fullFilename);
			}
		}
	}

	struct tm tm;
	gmtime_r(&curTime, &tm);
	NzbgetRsBuf rotBuf = nzbget_rs_dqh_log_format_rotated_path(
		*directory, PATH_SEPARATOR, baseName, tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, *baseExt);
	fullFilename.Set(rotBuf.data ? rotBuf.data : "", (int)rotBuf.len);
	nzbget_rs_free(rotBuf);

	UpdateLogPath(*fullFilename);
}

/*
* During intializing stage (when options were not read yet) all messages
* are saved in screen log, even if they shouldn't (according to options).
* Method "InitOptions()" check all messages added to screen log during
* intializing stage and does three things:
* 1) save the messages to log-file (if they should according to options);
* 2) delete messages from screen log (if they should not be saved in screen log).
* 3) renumerate IDs
*/
inline void Log::InitOptions()
{
	const char* messageType[] = { "INFO", "WARNING", "ERROR", "DEBUG", "DETAIL"};

	int logFileAction = nzbget_rs_dqh_log_classify_init_logfile_action(
		g_Options->GetRemoteClientMode() ? 1 : 0,
		g_Options->GetDirectClientMode() ? 1 : 0,
		(int)g_Options->GetWriteLog(),
		(int)Options::wlNone,
		(int)Options::wlReset,
		g_Options->GetLogFile() ? 1 : 0,
		g_Options->GetServerMode() ? 1 : 0);
	if (logFileAction != 0)
	{
		UpdateLogPath(g_Options->GetLogFile());

		if (logFileAction == 2)
		{
			g_Log->ResetLog();
		}
	}

	m_idGen = 0;

	for (uint32 i = 0; i < m_messages.size(); )
	{
		Message& message = m_messages.at(i);
		Options::EMessageTarget target = Options::mtNone;
		switch (message.GetKind())
		{
			case Message::mkDebug:
				target = g_Options->GetDebugTarget();
				break;
			case Message::mkDetail:
				target = g_Options->GetDetailTarget();
				break;
			case Message::mkInfo:
				target = g_Options->GetInfoTarget();
				break;
			case Message::mkWarning:
				target = g_Options->GetWarningTarget();
				break;
			case Message::mkError:
				target = g_Options->GetErrorTarget();
				break;
		}

		int writeToFile = 0;
		int keepInScreen = 0;
		nzbget_rs_rco_log_init_message_disposition((int)target, &writeToFile, &keepInScreen);
		if (writeToFile)
		{
			Filelog("%s\t%s", messageType[message.GetKind()], message.GetText());
		}

		if (!keepInScreen)
		{
			m_messages.erase(m_messages.begin() + i);
		}
		else
		{
			message.m_id = ++m_idGen;
			i++;
		}
	}

	m_optInit = true;
}

inline void Log::RegisterDebuggable(Debuggable* debuggable)
{
	std::lock_guard<std::mutex> guard(m_debugMutex);
	m_debuggables.push_back(debuggable);
}

inline void Log::UnregisterDebuggable(Debuggable* debuggable)
{
	std::lock_guard<std::mutex> guard(m_debugMutex);
	m_debuggables.remove(debuggable);
}

inline std::string Log::GetLogFilename() const
{
	std::lock_guard<std::mutex> guard(m_pathMutex);
	return m_logFilename;
}

inline fs::path Log::GetLogFilePath() const
{
	std::lock_guard<std::mutex> guard(m_pathMutex);
	return m_logFilePath; 
}

inline void Log::UpdateLogPath(const char* newPath)
{
	std::lock_guard<std::mutex> guard(m_pathMutex);
	m_logFilename = newPath;
	m_logFilePath = fs::u8path(m_logFilename);
}

#ifdef DEBUG
#ifdef HAVE_VARIADIC_MACROS
#define debug(...) debug(__FILE__, FUNCTION_MACRO_NAME, __LINE__, __VA_ARGS__)
#endif
#else
#define debug(...) do { } while(0)
#endif

#endif
#endif
