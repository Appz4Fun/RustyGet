#ifndef UTIL_H
#define UTIL_H

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2023-2026 Denis <denis@nzbget.com>
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


#ifndef NZBGET_H
#define NZBGET_H

#define PACKAGE "rustyget"
#define VERSION "27.0"
#define PACKAGE_BUGREPORT "https://github.com/xbmc4lyfe/RustyGet/issues"
#define FUNCTION_MACRO_NAME __FUNCTION__
#define HAVE_X509_CHECK_HOST 1
#ifndef NZBGET_USE_RUST
#define NZBGET_USE_RUST 1
#endif

/***************** GLOBAL INCLUDES *****************/

// POSIX INCLUDES

#include <unistd.h>
#include <pwd.h>
#include <signal.h>
#include <pthread.h>
#include <getopt.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <dirent.h>
#include <poll.h>

// COMMON INCLUDES

#include <sys/types.h>
#include <sys/stat.h>
#include <errno.h>
#include <stdio.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <stdint.h>
#include <time.h>
#include <ctype.h>
#include <inttypes.h>
#include <cstdint>

#include <string>
#include <string_view>
#include <vector>
#include <deque>
#include <list>
#include <set>
#include <map>
#include <unordered_map>
#include <unordered_set>
#include <iterator>
#include <algorithm>
#include <numeric>
#include <fstream>
#include <memory>
#include <functional>
#include <thread>
#include <atomic>
#include <utility>
#include <mutex>
#include <shared_mutex>
#include <condition_variable>
#include <chrono>
#include <optional>
#include <limits>
#include <type_traits>
#include <exception>
#include <span>

#ifndef DISABLE_TLS
#include <openssl/opensslv.h> // version text reported by SystemInfo; TLS itself is in Rust
#endif

extern "C" void nzbget_rs_tls_thread_stop(void);

namespace OpenSSL
{
	// Frees this thread's OpenSSL state (rust/src/tlssock.rs).
	inline void StopSSLThread() { nzbget_rs_tls_thread_stop(); }
}

// NOTE: do not include <iostream> in "nzbget.h". <iostream> contains objects requiring
// initialization, causing every unit in nzbget to have initialization routine. This in particular
// is causing fatal problems in SIMD units which must not have static initialization because
// they contain code with runtime CPU dispatching.
//#include <iostream>

#ifndef DISABLE_PARCHECK
#include <assert.h>
#include <cassert>
#endif /* NOT DISABLE_PARCHECK */

/***************** GLOBAL FUNCTION AND CONST OVERRIDES *****************/

// POSIX

#define closesocket(sock) close(sock)
#define SOCKET int
#define INVALID_SOCKET (-1)
#define PATH_SEPARATOR '/'
#define ALT_PATH_SEPARATOR '\\'
#define LINE_ENDING "\n"
#define FOPEN_RB "rb"
#define FOPEN_RBP "rb+"
#define FOPEN_WB "wb"
#define FOPEN_AB "ab"
#define CHILD_WATCHDOG 1
#define fseek fseeko
#define ftell ftello

// COMMON DEFINES FOR ALL PLATFORMS
#ifndef SHUT_RDWR
#define SHUT_RDWR 2
#endif

using uint8 = uint8_t;
using int32 = int32_t;
using uint32 = uint32_t;
using int64 = int64_t;
using uint64 = uint64_t;

#ifndef PRId64
#define PRId64 "lld"
#endif
#ifndef PRIi64
#define PRIi64 "lli"
#endif
#ifndef PRIu64
#define PRIu64 "llu"
#endif

typedef unsigned char uchar;

#ifdef __GNUC__
#define PRINTF_SYNTAX(strindex) __attribute__ ((format (printf, strindex, strindex+1)))
#else
#define PRINTF_SYNTAX(strindex)
#endif


#ifndef CONTAINER_H
#define CONTAINER_H

// for-range loops on pointers to std-containers (avoiding dereferencing)
template <typename T> typename std::deque<T>::iterator begin(std::deque<T>* c) { return c->begin(); }
template <typename T> typename std::deque<T>::iterator end(std::deque<T>* c) { return c->end(); }
template <typename T> typename std::vector<T>::iterator begin(std::vector<T>* c) { return c->begin(); }
template <typename T> typename std::vector<T>::iterator end(std::vector<T>* c) { return c->end(); }
template <typename T> typename std::list<T>::iterator begin(std::list<T>* c) { return c->begin(); }
template <typename T> typename std::list<T>::iterator end(std::list<T>* c) { return c->end(); }

template <typename T>
struct RawDequeIterator
{
	RawDequeIterator(typename std::deque<std::unique_ptr<T>>::iterator baseIterator) : m_baseIterator(baseIterator) {}
	typename std::deque<std::unique_ptr<T>>::iterator m_baseIterator;
};
template <typename T> bool operator!=(RawDequeIterator<T>& it1, RawDequeIterator<T>& it2) { return it1.m_baseIterator != it2.m_baseIterator; }
template <typename T> T* operator*(RawDequeIterator<T>& it) { return (*it.m_baseIterator).get(); }
template <typename T> RawDequeIterator<T> operator++(RawDequeIterator<T>& it) { return RawDequeIterator<T>(it.m_baseIterator++); }
template <typename T> RawDequeIterator<T> begin(std::deque<std::unique_ptr<T>>* c) { return RawDequeIterator<T>(c->begin()); }
template <typename T> RawDequeIterator<T> end(std::deque<std::unique_ptr<T>>* c) { return RawDequeIterator<T>(c->end()); }

template <typename T>
struct RawVectorIterator
{
	RawVectorIterator(typename std::vector<std::unique_ptr<T>>::iterator baseIterator) : m_baseIterator(baseIterator) {}
	typename std::vector<std::unique_ptr<T>>::iterator m_baseIterator;
};
template <typename T> bool operator!=(RawVectorIterator<T>& it1, RawVectorIterator<T>& it2) { return it1.m_baseIterator != it2.m_baseIterator; }
template <typename T> T* operator*(RawVectorIterator<T>& it) { return (*it.m_baseIterator).get(); }
template <typename T> RawVectorIterator<T> operator++(RawVectorIterator<T>& it) { return RawVectorIterator<T>(it.m_baseIterator++); }
template <typename T> RawVectorIterator<T> begin(std::vector<std::unique_ptr<T>>* c) { return RawVectorIterator<T>(c->begin()); }
template <typename T> RawVectorIterator<T> end(std::vector<std::unique_ptr<T>>* c) { return RawVectorIterator<T>(c->end()); }

/* Template class for deque of unique_ptr with useful utility functions */
template <typename T>
class UniqueDeque : public std::deque<std::unique_ptr<T>>
{
public:
	void Add(std::unique_ptr<T> uptr, bool addTop = false)
	{
		if (addTop)
		{
			this->push_front(std::move(uptr));
		}
		else
		{
			this->push_back(std::move(uptr));
		}
	}

	std::unique_ptr<T> Remove(T* p)
	{
		std::unique_ptr<T> uptr;

		typename UniqueDeque::iterator it = Find(p);
		if (it != this->end())
		{
			uptr = std::move(*it);
			this->erase(it);
		}

		return uptr;
	}

	typename UniqueDeque::iterator Find(T* p)
	{
		return std::find_if(this->begin(), this->end(),
			[p](std::unique_ptr<T>& uptr)
			{
				return uptr.get() == p;
			});
	}

	T* Find(int id)
	{
		for (std::unique_ptr<T>& uptr : *this)
		{
			if (uptr->GetId() == id)
			{
				return uptr.get();
			}
		}

		return nullptr;
	}
};

#endif

#ifndef THREAD_H
#define THREAD_H

class Mutex
{
public:
	Mutex() {};
	Mutex(const Mutex&) = delete;
	void Lock() { m_mutexObj.lock(); }
	void Unlock() { m_mutexObj.unlock(); }

private:
	std::mutex m_mutexObj;
};

class Guard
{
public:
	Guard(Mutex& mutex) : m_mutex(&mutex) { if (m_mutex) m_mutex->Lock(); }
	Guard(std::unique_ptr<Mutex>& mutex) : m_mutex(mutex.get()) { if (m_mutex) m_mutex->Lock(); }
	Guard(Guard&& other) : m_mutex(other.m_mutex) { other.m_mutex = nullptr; }
	Guard(const Guard&) = delete;
	~Guard() { Unlock(); }

private:
	Mutex* m_mutex;

	void Unlock() { if (m_mutex) { m_mutex->Unlock(); m_mutex = nullptr; } }
};

template<typename T>
class GuardedPtr
{
public:
	GuardedPtr(T* ptr, Mutex* mutex) : m_ptr(ptr), m_mutex(mutex) { if (m_mutex) m_mutex->Lock(); }
	GuardedPtr(GuardedPtr&& other) : m_ptr(other.m_ptr), m_mutex(other.m_mutex) { other.m_mutex = nullptr; }
	GuardedPtr(const GuardedPtr& other) = delete;
	~GuardedPtr() { Unlock(); }
	T* operator->() { return m_ptr; }
	operator T*() { return m_ptr; }

	// for-range loops on GuardedPtr
	auto begin() { return m_ptr->begin(); }
	auto end() { return m_ptr->end(); }

private:
	T* m_ptr;
	Mutex* m_mutex;

	void Unlock() { if (m_mutex) { m_mutex->Unlock(); m_mutex = nullptr; } }
};

#include "nzbget_rs.h"


#endif

#ifndef NSTRING_H
#define NSTRING_H

/*
BString is a replacement for char-arrays allocated on stack.
It has no memory overhead, provides memory management and formatting functions.
 */
template <int size>
class BString
{
public:
	BString() { m_data[0] = '\0'; }
	explicit BString(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		FormatV(format, ap);
		va_end(ap);
	}
	BString(BString& other) = delete;
	BString(const char* str) { Set(str); }
	BString(BString&& other) noexcept { Set(other.m_data); }
	BString& operator=(const char* str) { Set(str); return *this; }
	bool Empty() const { return !*m_data; }
	void Clear() { m_data[0] = '\0'; }
	operator char*() const { return const_cast<char*>(m_data); }
	char* operator*() const { return const_cast<char*>(m_data); }
	void Set(const char* str, int len = 0)
	{
		m_data[0] = '\0';
		if (!str || size <= 0)
		{
			return;
		}
		int cap = size - 1;
		size_t addLen = static_cast<size_t>((len > 0 && len < cap) ? len : cap);
		size_t copyLen = strnlen(str, addLen);
		memmove(m_data, str, copyLen);
		memset(m_data + copyLen, 0, addLen + 1 - copyLen);
	}
	void Append(const char* str, int len = 0)
	{
		if (!str || size <= 0)
		{
			return;
		}
		int effLen = len == 0 ? static_cast<int>(strlen(str)) : len;
		int curLen = static_cast<int>(strlen(m_data));
		int avail = size - curLen - 1;
		int addLen = avail < effLen ? avail : effLen;
		if (addLen > 0)
		{
			size_t copyLen = strnlen(str, static_cast<size_t>(addLen));
			memmove(m_data + curLen, str, copyLen);
			memset(m_data + curLen + copyLen, 0, static_cast<size_t>(addLen) + 1 - copyLen);
		}
	}
	int Format(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		int len = FormatV(format, ap);
		va_end(ap);
		return len;
	}
	int FormatV(const char* format, va_list ap)
	{
		if (size <= 0)
		{
			return 0;
		}
		va_list apCopy;
		va_copy(apCopy, ap);
		int len = vsnprintf(m_data, size, format, apCopy);
		va_end(apCopy);
		return len > 0 ? len : 0;
	}

protected:
	char m_data[size];
};

/*
CString is a replacement for C-Style null-terminated strings.
It has no memory overhead, provides memory management and string handling functions.
 */
class CString
{
public:
	CString() {}
	~CString()
	{
		Clear();
	}
	CString(const char* str, int len = 0) { Set(str, len); }
	CString(CString&& other) noexcept
	{
		m_data = other.m_data;
		other.m_data = nullptr;
	}
	CString(CString& other) = delete;
	CString& operator=(const CString& other) = delete;
	CString& operator=(CString&& other) noexcept
	{
		Clear();
		m_data = other.m_data;
		other.m_data = nullptr;
		return *this;
	}
	CString& operator=(const char* str) { Set(str); return *this; }
	operator char*() const { return m_data; }
	char* operator*() const { return m_data; }
	const char* Str() const { return m_data ? m_data : ""; }
	bool Empty() const { return !m_data || !*m_data; }
	void Clear()
	{
		free(m_data);
		m_data = nullptr;
	}
	void Reserve(int capacity)
	{
		int curLen = m_data ? static_cast<int>(strlen(m_data)) : 0;
		if ((capacity > 0 && capacity > curLen) || curLen == 0)
		{
			size_t capU = capacity > 0 ? static_cast<size_t>(capacity) : 0;
			char* data = static_cast<char*>(realloc(m_data, capU + 1));
			if (data)
			{
				m_data = data;
				data[curLen] = '\0';
			}
		}
	}
	void Set(const char* str, int len = 0)
	{
		if (str)
		{
			size_t effLen = len > 0 ? static_cast<size_t>(len) : strlen(str);
			ptrdiff_t aliasOffset = -1;
			if (m_data)
			{
				size_t oldLen = strlen(m_data);
				uintptr_t oldStart = reinterpret_cast<uintptr_t>(m_data);
				uintptr_t srcAddr = reinterpret_cast<uintptr_t>(str);
				if (srcAddr >= oldStart && srcAddr <= oldStart + oldLen)
				{
					aliasOffset = static_cast<ptrdiff_t>(srcAddr - oldStart);
				}
			}
			char* data = static_cast<char*>(realloc(m_data, effLen + 1));
			if (data)
			{
				m_data = data;
				const char* src = aliasOffset >= 0 ? (data + aliasOffset) : str;
				size_t copyLen = strnlen(src, effLen);
				if (src != data)
				{
					memmove(data, src, copyLen);
				}
				memset(data + copyLen, 0, effLen + 1 - copyLen);
			}
		}
		else
		{
			free(m_data);
			m_data = nullptr;
		}
	}
	int Format(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		int len = FormatV(format, ap);
		va_end(ap);
		return len;
	}
	int FormatV(const char* format, va_list ap)
	{
		va_list ap2;
		va_copy(ap2, ap);
		int newLen = vsnprintf(nullptr, 0, format, ap);
		int res = 0;
		if (newLen >= 0)
		{
			size_t bufLen = static_cast<size_t>(newLen) + 1;
			char* data = static_cast<char*>(realloc(m_data, bufLen));
			if (data)
			{
				m_data = data;
				if (vsnprintf(data, bufLen, format, ap2) < 0)
				{
					*data = '\0';
				}
				else
				{
					res = static_cast<int>(bufLen);
				}
			}
		}
		va_end(ap2);
		return res;
	}

protected:
	char* m_data = nullptr;
};

/*
Plain char-buffer for I/O operations.
 */

#endif

#endif /* NZBGET_H */

#include <sstream>
#include <array>
#include <climits>
#include <stdexcept>

class Util
{
public:
	static bool EmptyStr(const char* str) { return !str || !*str; }

	static time_t CurrentTime() { return ::time(nullptr); }

	static int64 CurrentTicks()
	{
		timeval t;
		gettimeofday(&t, nullptr);
		return (int64)(t.tv_sec) * 1000000ll + (int64)(t.tv_usec);
	}

	static void Sleep(int ms) { std::this_thread::sleep_for(std::chrono::milliseconds(ms)); }

	static void FormatTime(time_t timeSec, char* buffer, int bufsize)
	{
		nzbget_rs_util_format_time(static_cast<int64_t>(timeSec), buffer, bufsize);
	}

	static CString FormatTime(time_t timeSec)
	{
		CString result;
		result.Reserve(50);
		FormatTime(timeSec, result, 50);
		return result;
	}

	static const char* VersionRevision() { return VersionRevisionBuf; }
	static inline char VersionRevisionBuf[100];

	static void Init();
};

inline void InitRsBuildEnv();
inline void Util::Init()
{
	snprintf(VersionRevisionBuf, sizeof(VersionRevisionBuf), "%s", VERSION);
	CurrentTicks();
	InitRsBuildEnv();
}

#ifndef FILESYSTEM_H
#define FILESYSTEM_H

#include <filesystem>

namespace fs
{
using namespace std::filesystem;
using error_code = std::error_code;
using errc = std::errc;

inline fs::path u8path(std::string_view pathStr) { return fs::path(pathStr); }
inline std::string u8string(const path& p) { return p.string(); }
}

class FileSystem
{
public:
	static char* BaseFileName(const char* filename)
	{
		return const_cast<char*>(filename) + nzbget_rs_path_position(0, filename, strlen(filename));
	}

	static bool SameFilename(const char* filename1, const char* filename2)
	{
		return !strcmp(filename1 ? filename1 : "", filename2 ? filename2 : "");
	}

	static CString MakeValidFilename(const char* filename, bool allowSlashes = false)
	{
		NzbgetRsBuf buf = nzbget_rs_path_text(0, filename, strlen(filename), allowSlashes);
		CString result(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return result;
	}

	static bool DeleteFile(const char* filename) { return filename && unlink(filename) == 0; }
	static bool FileExists(const char* filename) { return nzbget_rs_file_exists(filename) != 0; }
	static bool DirectoryExists(const char* dirFilename) { return nzbget_rs_directory_exists(dirFilename) != 0; }

	static bool CreateDirectory(const char* dirFilename)
	{
		if (dirFilename)
		{
			mkdir(dirFilename, S_IRWXU | S_IRWXG | S_IRWXO);
		}
		return DirectoryExists(dirFilename);
	}
};

#endif /* FILESYSTEM_H */

#ifndef LOG_H
#define LOG_H

void error(const char* msg, ...) PRINTF_SYNTAX(1);
void warn(const char* msg, ...) PRINTF_SYNTAX(1);
void info(const char* msg, ...) PRINTF_SYNTAX(1);
void detail(const char* msg, ...) PRINTF_SYNTAX(1);

#ifdef DEBUG
void debug(const char* filename, const char* funcname, int lineNr, const char* msg, ...) PRINTF_SYNTAX(4);
#endif

// message kinds (rust/src/qnzb.rs keeps a collection's messages)
namespace Message { enum EKind { mkInfo, mkWarning, mkError, mkDebug, mkDetail }; }

#ifndef OPTIONS_H
#define OPTIONS_H

#ifndef FEEDINFO_H
#define FEEDINFO_H

#ifndef EDUPEMODE_DEFINED
#define EDUPEMODE_DEFINED
enum EDupeMode
{
	dmScore,
	dmAll,
	dmForce
};
#endif

#endif


class Options;
extern Options* g_Options;

class Options
{
public:
	enum EOutputMode { omLoggable, omColored, omNCurses };
	enum EParCheck { pcAuto, pcAlways, pcForce, pcManual };
	enum EParScan { psLimited, psExtended, psFull, psDupe };
	enum EDupeArticleFallback
	{
		// ordered by capability: every value includes the capabilities of the
		// values before it, so feature gates compare with >= (dafLive adds
		// download-concurrent repair on top of dafStream's post-processing repair)
		dafNone,
		dafArticle,
		dafStream,
		dafLive
	};
	enum EHealthCheck { hcPause, hcDelete, hcPark, hcNone, hcDupe };
	enum ECertVerifLevel { cvNone, cvMinimal, cvStrict, Count };

	typedef std::vector<const char*> CmdOptList;

	Options() { Init(nullptr, nullptr, -1, nullptr, false); }

	Options(CmdOptList* commandLineOptions, void* = nullptr)
	{
		Init("rustyget/rustyget", nullptr, 1, commandLineOptions, true);
	}

	~Options()
	{
		nzbget_rs_options_free(m_rs);
		g_Options = nullptr;
	}

	const NzbgetRsOptions* RsHandle() const { return m_rs; }

	bool QueryCategory(const char* name, bool searchAliases, int* outUnpack, const char** outStrs) const
	{
		return nzbget_rs_options_query(
			m_rs, 0, name, searchAliases ? 1 : 0, outUnpack, outStrs, 0, nullptr) != 0;
	}

	const char* GetDestDir() const { return Str(3); }
	const char* GetInterDir() const { return Str(4); }
	bool GetNzbLog() const { return m_scalars.nzb_log != 0; }
	bool GetRawArticle() const { return m_scalars.raw_article != 0; }
	bool GetSkipWrite() const { return m_scalars.skip_write != 0; }
	bool GetAppendCategoryDir() const { return m_scalars.append_category_dir != 0; }
	const char* GetCertStore() const { return Str(23); }
	bool GetCertCheck() const { return m_scalars.cert_check != 0; }
	int GetLogBuffer() const { return m_scalars.log_buffer; }
	EHealthCheck GetHealthCheck() const { return (EHealthCheck)m_scalars.health_check; }
	bool GetDirectWrite() const { return m_scalars.direct_write != 0; }
	int GetWriteBuffer() const { return m_scalars.write_buffer; }

	bool GetServerMode() const { return Mode(0); }

private:
	NzbgetRsOptions* m_rs = nullptr;
	NzbgetRsParsedOptions m_scalars{};

	const char* Str(int id) const { return nzbget_rs_options_str(m_rs, id); }
	bool Mode(int idx) const
	{
		return nzbget_rs_options_query(
			m_rs, 4, nullptr, idx, nullptr, nullptr, 0, nullptr) != 0;
	}

	void Init(const char* exeName, const char* configFilename, int noConfig,
		CmdOptList* commandLineOptions, bool noDiskAccess)
	{
		g_Options = this;
		int flags = 32;
#ifdef DISABLE_CURSES
		flags |= 1;
#endif
#ifdef DISABLE_PARCHECK
		flags |= 2;
#endif
#ifdef DISABLE_TLS
		flags |= 4;
#endif
#ifdef HAVE_X509_CHECK_HOST
		flags |= 8;
#endif
		int stateFlags = 0;
		m_rs = nzbget_rs_options_new(
			exeName, configFilename, noConfig, noDiskAccess ? 1 : 0,
			commandLineOptions && !commandLineOptions->empty() ? commandLineOptions->data() : nullptr,
			commandLineOptions ? commandLineOptions->size() : 0,
			flags, Util::VersionRevision(), nullptr, nullptr, &m_scalars, &stateFlags);
	}
};

#endif /* OPTIONS_H */

class Log;
extern Log* g_Log;

class Log
{
public:
	Log();

	~Log()
	{
		nzbget_rs_log_free(m_rs);
		g_Log = nullptr;
	}

private:
	NzbgetRsLog* m_rs = nullptr;

	static const NzbgetRsOptions* OptRs() { return g_Options ? g_Options->RsHandle() : nullptr; }

	void EmitV(Message::EKind kind, const char* msg, va_list ap)
	{
		char tmp2[1024];
		vsnprintf(tmp2, 1024, msg, ap);
		tmp2[1024 - 1] = '\0';
		nzbget_rs_log_emit(m_rs, static_cast<int>(kind), OptRs(), tmp2);
	}

	friend void error(const char* msg, ...);
	friend void warn(const char* msg, ...);
	friend void info(const char* msg, ...);
	friend void detail(const char* msg, ...);
#ifdef DEBUG
	friend void debug(const char* filename, const char* funcname, int lineNr, const char* msg, ...);
#endif
};

inline Log::Log()
{
	g_Log = this;
#ifdef DEBUG
	m_rs = nzbget_rs_log_new(1);
#else
	m_rs = nzbget_rs_log_new(0);
#endif
}

#ifdef DEBUG
inline void debug(const char* filename, const char* funcname, int lineNr, const char* msg, ...)
{
	if (!g_Log) return;
	char tmp1[1024];
	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp1, 1024, msg, ap);
	tmp1[1024 - 1] = '\0';
	va_end(ap);
	BString<1024> tmp2;
	if (funcname) tmp2.Format("%s (%s:%i:%s)", tmp1, FileSystem::BaseFileName(filename), lineNr, funcname);
	else tmp2.Format("%s (%s:%i)", tmp1, FileSystem::BaseFileName(filename), lineNr);
	nzbget_rs_log_emit(g_Log->m_rs, static_cast<int>(Message::mkDebug), Log::OptRs(), *tmp2);
}
#endif

inline void error(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	g_Log->EmitV(Message::mkError, msg, ap);
	va_end(ap);
}

inline void warn(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	g_Log->EmitV(Message::mkWarning, msg, ap);
	va_end(ap);
}

inline void info(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	g_Log->EmitV(Message::mkInfo, msg, ap);
	va_end(ap);
}

inline void detail(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	g_Log->EmitV(Message::mkDetail, msg, ap);
	va_end(ap);
}

#ifdef DEBUG
#define debug(...) debug(__FILE__, FUNCTION_MACRO_NAME, __LINE__, __VA_ARGS__)
#else
#define debug(...) do { } while(0)
#endif

#endif /* LOG_H */

#ifndef SERVICE_H
#define SERVICE_H

class ServiceCoordinator {};
extern ServiceCoordinator* g_ServiceCoordinator;

#endif

#ifndef WORKSTATE_H
#define WORKSTATE_H

class WorkState final
{
public:
	WorkState() : m_rs(nzbget_rs_workstate_new()) {}
	~WorkState() { nzbget_rs_workstate_free(m_rs); }

	bool GetPausePostProcess() const { return Ctl(1) != 0; }
	void SetTempPausePostprocess(bool v) { Ctl(5, 1, v ? 1 : 0); }
	bool GetTempPausePostprocess() const { return Ctl(5) != 0; }

private:
	NzbgetRsWorkState* m_rs = nullptr;
	int64_t Ctl(int id, int write = 0, int64_t val = 0) const { return nzbget_rs_workstate_ctl(m_rs, id, write, val); }
};

extern WorkState* g_WorkState;

#endif

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef CONNECTION_H
#define CONNECTION_H

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2008-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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




inline uintptr_t NzbgetCQueueHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

inline void InitRsBuildEnv()
{
	static std::once_flag once;
	std::call_once(once, [] {
		// the queue model shares this struct's layout with Rust (rust/src/qnzb.rs)
		if (nzbget_rs_nzb_scalars_size() != sizeof(NzbgetRsNzbScalars))
		{
			fprintf(stderr, "NzbgetRsNzbScalars layout differs between C++ and Rust\n");
			abort();
		}
		int flags = 0;
#ifdef DEBUG
		flags |= 1;
#endif
#ifdef DISABLE_TLS
		flags |= 2;
#endif
#ifdef DISABLE_GZIP
		flags |= 4;
#endif
#ifdef DISABLE_PARCHECK
		flags |= 16;
#endif
#ifdef DISABLE_CURSES
		flags |= 32;
#endif
		const char* sslLib = "";
		const char* sslVer = "";
#ifndef DISABLE_TLS
#ifdef LIBRESSL_VERSION_TEXT
		sslLib = "LibreSSL";
		sslVer = LIBRESSL_VERSION_TEXT;
		flags |= 8;
#elif defined(OPENSSL_FULL_VERSION_STR)
		sslLib = "OpenSSL";
		sslVer = OPENSSL_FULL_VERSION_STR;
#else
		sslLib = "OpenSSL";
		sslVer = OPENSSL_VERSION_TEXT;
		flags |= 8;
#endif
#endif
		nzbget_rs_build_env_init(flags, Util::VersionRevision(), sslLib, sslVer, nullptr,
			+[](const char* ptr, size_t len, int doFlush) {
				if (ptr && len > 0) fwrite(ptr, 1, len, stdout);
				if (doFlush) fflush(stdout);
			}, &NzbgetCQueueHost);
	});
}

namespace System { class SystemInfo; }

#endif

#endif /* UTIL_H */

#ifndef DISKSTATE_H
#define DISKSTATE_H

#ifndef DOWNLOADINFO_H
#define DOWNLOADINFO_H
class NzbInfo;
class DownloadQueue;

// server-stat operations, as nzbget_rs_server_stat_apply_op takes them
namespace ServerStatList { enum EStatOperation { soSet, soAdd, soSubtract }; }

// Articles live in Rust (rust/src/qarticle.rs). Status values as there.
enum EArticleStatus { aiUndefined, aiRunning, aiFinished, aiFailed };
inline uintptr_t ArtOp(RsArticleList* list, uintptr_t article, int op, int a2 = 0, uintptr_t p2 = 0,
	const char* s1 = nullptr, int* ints = nullptr, int64_t* i64s = nullptr, const char** strs = nullptr)
{
	return nzbget_rs_article_host(list, reinterpret_cast<RsArticle*>(article), op, a2, p2, s1, ints, i64s, strs);
}

// The file's data lives in Rust (rust/src/qfile.rs); this class owns it and
// keeps the C++ queue model's accessors.
class FileInfo;
class FileOutputGuard
{
public:
	explicit FileOutputGuard(RsFile* file) : m_file(file), m_taken(nzbget_rs_file_op(file, 20, 0, nullptr, nullptr, nullptr) != 0) {}
	FileOutputGuard(FileOutputGuard&& other) : m_file(other.m_file), m_taken(other.m_taken) { other.m_taken = false; }
	FileOutputGuard(const FileOutputGuard&) = delete;
	~FileOutputGuard() { if (m_taken) nzbget_rs_file_op(m_file, 21, 0, nullptr, nullptr, nullptr); }

private:
	RsFile* m_file;
	bool m_taken;
};

class FileInfo
{
public:
	enum EPartialState
	{
		psNone,
		psPartial,
		psCompleted
	};
	// what the file's first article holds: borrowing from duplicates never writes
	// into par2 data, whatever the file is called
	enum EFirstContent
	{
		fcUnknown,
		fcPar2,
		fcOther
	};

	FileInfo(int id = 0) : m_rs(nzbget_rs_file_new(id)) {}
	~FileInfo() { nzbget_rs_file_free(m_rs); }
	FileInfo(const FileInfo&) = delete;
	FileInfo& operator=(const FileInfo&) = delete;
	RsFile* Rs() { return m_rs; }
	void SetId(int id) { S(0, id); }
	static void ResetGenId(bool max) { nzbget_rs_file_ids(max ? 1 : 2, 0); }
	static void RaiseIdFloor(int id) { nzbget_rs_file_ids(3, id); }
	static int LastGenId() { return nzbget_rs_file_ids(0, 0); }
	NzbInfo* GetNzbInfo() { return reinterpret_cast<NzbInfo*>((uintptr_t)I(35)); }
	void SetNzbInfo(NzbInfo* nzbInfo) { S(35, (int64)reinterpret_cast<uintptr_t>(nzbInfo)); }
	RsArticleList* GetArticleList() { return nzbget_rs_file_articles(m_rs); }
	size_t GroupCount() { return (size_t)nzbget_rs_file_op(m_rs, 0, 0, nullptr, nullptr, nullptr); }
	const char* GroupAt(size_t i) { const char* g = nullptr; nzbget_rs_file_op(m_rs, 1, (int)i, nullptr, nullptr, &g); return g; }
	void AddGroup(const char* group) { nzbget_rs_file_op(m_rs, 2, 0, group, nullptr, nullptr); }
	const char* GetSubject() { return T(0); }
	void SetSubject(const char* subject) { ST(0, subject); }
	const char* GetFilename() { return T(1); }
	void SetFilename(const char* filename) { ST(1, filename); }
	const char* GetOrigname() { return T(2); }
	void SetOrigname(const char* origname) { ST(2, origname); }
	void MakeValidFilename() { SetFilename(FileSystem::MakeValidFilename(GetFilename()).Str()); }
	int GetId() { return (int)I(0); }
	int64 GetSize() { return (int64)I(1); }
	void SetSize(int64 v) { S(1, (int64)v); }
	int64 GetRemainingSize() { return (int64)I(2); }
	void SetRemainingSize(int64 v) { S(2, (int64)v); }
	int64 GetSuccessSize() { return (int64)I(3); }
	void SetSuccessSize(int64 v) { S(3, (int64)v); }
	int64 GetFailedSize() { return (int64)I(4); }
	void SetFailedSize(int64 v) { S(4, (int64)v); }
	int64 GetMissedSize() { return (int64)I(5); }
	void SetMissedSize(int64 v) { S(5, (int64)v); }
	int GetTotalArticles() { return (int)I(6); }
	void SetTotalArticles(int v) { S(6, (int64)v); }
	int GetMissedArticles() { return (int)I(7); }
	void SetMissedArticles(int v) { S(7, (int64)v); }
	int GetFailedArticles() { return (int)I(8); }
	void SetFailedArticles(int v) { S(8, (int64)v); }
	int GetSuccessArticles() { return (int)I(9); }
	void SetSuccessArticles(int v) { S(9, (int64)v); }
	int GetDupeRecoveredArticles() { return (int)I(10); }
	void SetDupeRecoveredArticles(int v) { S(10, (int64)v); }
	int GetDupeAttemptedArticles() { return (int)I(11); }
	void SetDupeAttemptedArticles(int v) { S(11, (int64)v); }
	bool GetDupeCutover() { return (bool)I(12); }
	void SetDupeCutover(bool v) { S(12, (int64)v); }
	bool GetDupeNoSourceLogged() { return (bool)I(13); }
	void SetDupeNoSourceLogged(bool v) { S(13, (int64)v); }
	EFirstContent GetFirstContent() { return (EFirstContent)I(14); }
	void SetFirstContent(EFirstContent v) { S(14, (int64)v); }
	int GetDupeLeadDonorId() { return (int)I(15); }
	void SetDupeLeadDonorId(int v) { S(15, (int64)v); }
	int GetDupeLeadFailures() { return (int)I(16); }
	void SetDupeLeadFailures(int v) { S(16, (int64)v); }
	int GetDupeLeadSwitches() { return (int)I(17); }
	void SetDupeLeadSwitches(int v) { S(17, (int64)v); }
	time_t GetTime() { return (time_t)I(18); }
	void SetTime(time_t v) { S(18, (int64)v); }
	bool GetPaused() { return (bool)I(19); }
	bool GetDeleted() { return (bool)I(20); }
	void SetDeleted(bool v) { S(20, (int64)v); }
	bool GetFilenameConfirmed() { return (bool)I(21); }
	void SetFilenameConfirmed(bool v) { S(21, (int64)v); }
	bool GetParFile() { return (bool)I(22); }
	void SetParFile(bool v) { S(22, (int64)v); }
	int GetCompletedArticles() { return (int)I(23); }
	void SetCompletedArticles(int v) { S(23, (int64)v); }
	bool GetOutputInitialized() { return (bool)I(24); }
	void SetOutputInitialized(bool v) { S(24, (int64)v); }
	bool GetExtraPriority() { return (bool)I(25); }
	int GetActiveDownloads() { return (int)I(26); }
	void SetActiveDownloads(int v) { S(26, (int64)v); }
	bool GetDupeDeleted() { return (bool)I(27); }
	void SetDupeDeleted(bool v) { S(27, (int64)v); }
	int GetCachedArticles() { return (int)I(28); }
	void SetCachedArticles(int v) { S(28, (int64)v); }
	bool GetPartialChanged() { return (bool)I(29); }
	void SetPartialChanged(bool v) { S(29, (int64)v); }
	bool GetForceDirectWrite() { return (bool)I(30); }
	void SetForceDirectWrite(bool v) { S(30, (int64)v); }
	EPartialState GetPartialState() { return (EPartialState)I(31); }
	void SetPartialState(EPartialState v) { S(31, (int64)v); }
	uint32 GetCrc() { return (uint32)I(32); }
	void SetCrc(uint32 v) { S(32, (int64)v); }
	bool GetFlushLocked() { return (bool)I(33); }
	void SetFlushLocked(bool v) { S(33, (int64)v); }
	int64 GetDecodedFileSize() { return (int64)I(34); }
	void SetDecodedFileSize(int64 v) { S(34, (int64)v); }
	void SetPaused(bool paused);
	void SetExtraPriority(bool extraPriority);
	FileOutputGuard GuardOutputFile() { return FileOutputGuard(m_rs); }
	std::string GetOutputFilename() { return T(3); }
	const char* GetOutputFilenameC() { return T(3); }
	void SetOutputFilename(const char* outputFilename) { ST(3, outputFilename); }
	const char* GetHash16k() { return T(4); }
	void SetHash16k(const char* hash16k) { ST(4, hash16k); }
	const char* GetParSetId() { return T(5); }
	void SetParSetId(const char* parSetId) { ST(5, parSetId); }
	std::string GetHardLinkPath() { return T(6); }
	const char* GetHardLinkPathC() { return T(6); }
	void SetHardLinkPath(std::string hardLinkPath) { ST(6, hardLinkPath.c_str()); }
	bool IsHardLinked() { return *T(6) != '\0'; }
	// server stats are kept in Rust
	size_t StatCount() { return (size_t)nzbget_rs_file_op(m_rs, 10, 0, nullptr, nullptr, nullptr); }
	void StatAt(size_t i, int* out3) { nzbget_rs_file_op(m_rs, 11, (int)i, nullptr, out3, nullptr); }
	void StatOp(int serverId, int success, int failed, ServerStatList::EStatOperation op)
	{
		int v[3] = { serverId, success, failed };
		nzbget_rs_file_op(m_rs, 12, (int)op, nullptr, v, nullptr);
	}
	void ClearStats() { nzbget_rs_file_op(m_rs, 13, 0, nullptr, nullptr, nullptr); }

private:
	RsFile* m_rs;
	int64 I(int id) { return nzbget_rs_file_get(m_rs, id); }
	void S(int id, int64 v) { nzbget_rs_file_set(m_rs, id, v); }
	const char* T(int id) { return nzbget_rs_file_text(m_rs, id); }
	void ST(int id, const char* v) { nzbget_rs_file_set_text(m_rs, id, v); }

};


typedef UniqueDeque<FileInfo> FileList;
typedef std::vector<FileInfo*> RawFileList;

// completed file status values (rust/src/qnzb.rs keeps completed files and
// stream-repair jobs)
namespace CompletedFile { enum EStatus { cfNone, cfSuccess, cfPartial, cfFailure }; }


// script status values (rust/src/qnzb.rs keeps the statuses)
namespace ScriptStatus { enum EStatus { srNone, srFailure, srSuccess }; }

#ifndef EDUPEMODE_DEFINED
#define EDUPEMODE_DEFINED
enum EDupeMode
{
	dmScore,
	dmAll,
	dmForce
};
#endif

class NzbInfo
{
public:
	NzbInfo() { m_s->id = ++m_idGen; }
	~NzbInfo() { nzbget_rs_nzb_scalars_free(m_s); }
	NzbInfo(const NzbInfo&) = delete;
	NzbInfo& operator=(const NzbInfo&) = delete;
	// numeric and status fields live in Rust (rust/src/qnzb.rs)
	NzbgetRsNzbScalars* Scalars() { return m_s; }
	// strings live in Rust too: 0 url, 1 filename, 2 name, 3 dest dir, 4 final dir,
	// 5 category, 6 queued filename, 7 dupe key, 8 hard-link path, 9 meta name
	const char* NT(int id) { return nzbget_rs_nzb_text(m_s, id); }
	void NST(int id, const char* v) { nzbget_rs_nzb_set_text(m_s, id, v, -1); }
	enum EDirectRenameStatus
	{
		tsNone,
		tsRunning,
		tsFailure,
		tsSuccess
	};

	enum EPostRenameStatus
	{
		rsNone,
		rsSkipped,
		rsNothing,
		rsSuccess
	};

	enum EParStatus
	{
		psNone,
		psSkipped,
		psFailure,
		psSuccess,
		psRepairPossible,
		psManual
	};

	enum EDirectUnpackStatus
	{
		nsNone,
		nsRunning,
		nsFailure,
		nsSuccess
	};

	enum EPostUnpackStatus
	{
		usNone,
		usSkipped,
		usFailure,
		usSuccess,
		usSpace,
		usPassword
	};

	enum ECleanupStatus
	{
		csNone,
		csFailure,
		csSuccess
	};

	enum EMoveStatus
	{
		msNone,
		msFailure,
		msSuccess
	};

	enum class PostUnpackRenamingStatus
	{
		None,
		Failure,
		Success,
		Skipped
	};

	enum class PostDownloadRenamingStatus
	{
		None,
		Failure,
		Success,
		Skipped
	};

	enum EDeleteStatus
	{
		dsNone,
		dsManual,
		dsHealth,
		dsDupe,
		dsBad,
		dsGood,
		dsCopy,
		dsScan
	};

	enum EMarkStatus
	{
		ksNone,
		ksBad,
		ksGood,
		ksSuccess
	};

	enum EUrlStatus
	{
		lsNone,
		lsRunning,
		lsFinished,
		lsFailed,
		lsRetry,
		lsScanSkipped,
		lsScanFailed
	};

	enum EKind
	{
		nkNzb,
		nkUrl
	};

	enum EDupeHint
	{
		dhNone,
		dhRedownloadManual,
		dhRedownloadAuto
	};

	int GetId() { return m_s->id; }
	void SetId(int id)
	{
		m_s->id = id;
		if (m_idMax < m_s->id)
		{
			m_idMax = m_s->id;
		}
	}
	static void ResetGenId(bool max)
	{
		if (max)
		{
			m_idGen = m_idMax;
		}
		else
		{
			m_idGen = 0;
			m_idMax = 0;
		}
	}
	static void RaiseIdFloor(int id) { m_idMax = std::max(m_idMax, id); }
	static int LastGenId() { return m_idGen; }
	EKind GetKind() { return (EKind)m_s->kind; }
	void SetKind(EKind kind) { m_s->kind = kind; }
	const char* GetUrl() { return NT(0); }
	void SetUrl(const char* url)
	{
		NST(0, url);

		if (!NT(2))
		{
			CString nzbNicename = MakeNiceUrlName(url, NT(1));
			SetName(nzbNicename);
		}
	}
	const char* GetFilename() { return NT(1); }
	void SetFilename(const char* filename)
	{
		bool hadFilename = !Util::EmptyStr(NT(1));
		NST(1, filename);

		if ((!NT(2) || !hadFilename) && !Util::EmptyStr(filename))
		{
			CString nzbNicename = MakeNiceNzbName(NT(1), true);
			if (nzbNicename.Empty())
			{
				nzbNicename.Format("nzb_%i", GetId());
			}
			SetName(nzbNicename);
		}
	}
	static CString MakeNiceNzbName(const char* nzbFilename, bool removeExt)
	{
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			0, nzbFilename, nullptr, nullptr, nullptr, nullptr, removeExt ? 1 : 0, 0);
		CString out(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return out;
	}
	static CString MakeNiceUrlName(const char* urlStr, const char* nzbFilename)
	{
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			1, urlStr, nzbFilename, nullptr, nullptr, nullptr, 0, 0);
		CString out(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return out;
	}
	const char* GetDestDir() { return NT(3); }
	void SetDestDir(const char* destDir) { NST(3, destDir); }
	const char* GetFinalDir() { return NT(4); }
	void SetFinalDir(const char* finalDir) { NST(4, finalDir); }
	const char* GetCategory() { return NT(5); }
	void SetCategory(const char* category) { NST(5, category); }
	const char* GetName() { return NT(2); }
	void SetHardLinkPath(std::string hardLinkPath) { NST(8, hardLinkPath.c_str()); }
	std::string GetHardLinkPath() { return NT(8); }
	const char* GetHardLinkPathC() { return NT(8); }
	void SetName(const char* name) { NST(2, name); }
	std::string GetMetaName() { return NT(9); }
	const char* GetMetaNameC() { return NT(9); }
	void SetMetaName(std::string metaName) { NST(9, metaName.c_str()); }
	int GetFileCount() { return m_s->fileCount; }
	void SetFileCount(int fileCount) { m_s->fileCount = fileCount; }
	int GetParkedFileCount() { return m_s->parkedFileCount; }
	void SetParkedFileCount(int parkedFileCount) { m_s->parkedFileCount = parkedFileCount; }
	int64 GetSize() { return m_s->size; }
	void SetSize(int64 size) { m_s->size = size; }
	int64 GetRemainingSize() { return m_s->remainingSize; }
	void SetRemainingSize(int64 remainingSize) { m_s->remainingSize = remainingSize; }
	int64 GetPausedSize() { return m_s->pausedSize; }
	void SetPausedSize(int64 pausedSize) { m_s->pausedSize = pausedSize; }
	int GetPausedFileCount() { return m_s->pausedFileCount; }
	void SetPausedFileCount(int pausedFileCount) { m_s->pausedFileCount = pausedFileCount; }
	int GetRemainingParCount() { return m_s->remainingParCount; }
	void SetRemainingParCount(int remainingParCount) { m_s->remainingParCount = remainingParCount; }
	int GetActiveDownloads() { return m_s->activeDownloads; }
	void SetActiveDownloads(int activeDownloads)
	{
		int64_t startTime = static_cast<int64_t>(m_s->downloadStartTime);
		int changed = 0;
		nzbget_rs_nzb_calc_active_downloads_sec(
			m_s->activeDownloads, activeDownloads, m_s->kind == NzbInfo::nkNzb ? 1 : 0,
			static_cast<int64_t>(Util::CurrentTime()), &m_s->downloadSec, &startTime,
			&m_s->downloadStartSec, &changed);
		m_s->downloadStartTime = static_cast<time_t>(startTime);
		if (changed) m_s->changed = true;
		m_s->activeDownloads = activeDownloads;
	}
	int64 GetSuccessSize() { return m_s->successSize; }
	void SetSuccessSize(int64 successSize) { m_s->successSize = successSize; }
	int64 GetFailedSize() { return m_s->failedSize; }
	void SetFailedSize(int64 failedSize) { m_s->failedSize = failedSize; }
	int64 GetCurrentSuccessSize() { return m_s->currentSuccessSize; }
	void SetCurrentSuccessSize(int64 currentSuccessSize) { m_s->currentSuccessSize = currentSuccessSize; }
	int64 GetCurrentFailedSize() { return m_s->currentFailedSize; }
	void SetCurrentFailedSize(int64 currentFailedSize) { m_s->currentFailedSize = currentFailedSize; }
	int64 GetParSize() { return m_s->parSize; }
	void SetParSize(int64 parSize) { m_s->parSize = parSize; }
	int64 GetParSuccessSize() { return m_s->parSuccessSize; }
	void SetParSuccessSize(int64 parSuccessSize) { m_s->parSuccessSize = parSuccessSize; }
	int64 GetParFailedSize() { return m_s->parFailedSize; }
	void SetParFailedSize(int64 parFailedSize) { m_s->parFailedSize = parFailedSize; }
	int64 GetParCurrentSuccessSize() { return m_s->parCurrentSuccessSize; }
	void SetParCurrentSuccessSize(int64 parCurrentSuccessSize) { m_s->parCurrentSuccessSize = parCurrentSuccessSize; }
	int64 GetParCurrentFailedSize() { return m_s->parCurrentFailedSize; }
	void SetParCurrentFailedSize(int64 parCurrentFailedSize) { m_s->parCurrentFailedSize = parCurrentFailedSize; }
	int GetTotalArticles() { return m_s->totalArticles; }
	void SetTotalArticles(int totalArticles) { m_s->totalArticles = totalArticles; }
	int GetSuccessArticles() { return m_s->successArticles; }
	void SetSuccessArticles(int successArticles) { m_s->successArticles = successArticles; }
	int GetFailedArticles() { return m_s->failedArticles; }
	void SetFailedArticles(int failedArticles) { m_s->failedArticles = failedArticles; }
	int GetCurrentSuccessArticles() { return m_s->currentSuccessArticles; }
	void SetCurrentSuccessArticles(int currentSuccessArticles) { m_s->currentSuccessArticles = currentSuccessArticles; }
	int GetCurrentFailedArticles() { return m_s->currentFailedArticles; }
	void SetCurrentFailedArticles(int currentFailedArticles) { m_s->currentFailedArticles = currentFailedArticles; }
	int GetDupeRecoveredArticles() { return m_s->dupeRecoveredArticles; }
	// duplicates whose articles failed the par2 checksums: no more articles from them (B40, not persisted)
	// failed articles of its own in a row, first articles of files aside (B49, not persisted)
	int GetDupeFailedRun() { return m_s->dupeFailedRun; }
	void SetDupeFailedRun(int dupeFailedRun) { m_s->dupeFailedRun = dupeFailedRun; }
	void SetDupeRecoveredArticles(int dupeRecoveredArticles) { m_s->dupeRecoveredArticles = dupeRecoveredArticles; }
	int64 GetDupeRecoveredBytes() { return m_s->dupeRecoveredBytes; }
	void SetDupeRecoveredBytes(int64 value) { m_s->dupeRecoveredBytes = value; }
	int GetDupeRecoveredHoles() { return m_s->dupeRecoveredHoles; }
	void SetDupeRecoveredHoles(int value) { m_s->dupeRecoveredHoles = value; }
	int GetDupeAttemptedArticles() { return m_s->dupeAttemptedArticles; }
	void SetDupeAttemptedArticles(int value) { m_s->dupeAttemptedArticles = value; }
	int GetDupeUnsourcedArticles() { return m_s->dupeUnsourcedArticles; }
	void SetDupeUnsourcedArticles(int value) { m_s->dupeUnsourcedArticles = value; }
	int GetDupeFailoverChecked() { return m_s->dupeFailoverChecked; }
	void SetDupeFailoverChecked(int value) { m_s->dupeFailoverChecked = value; }
	bool GetDeadPickProbed() { return m_s->deadPickProbed; }
	void SetDeadPickProbed(bool value) { m_s->deadPickProbed = value; }
	enum EDupeParDeferState { dpNone, dpDeferred, dpLifted };
	EDupeParDeferState GetDupeParDeferState() { return (EDupeParDeferState)m_s->dupeParDeferState; }
	void SetDupeParDeferState(EDupeParDeferState state) { m_s->dupeParDeferState = state; }
	bool GetDupeNoParNoted() { return m_s->dupeNoParNoted; }
	void SetDupeNoParNoted(bool noted) { m_s->dupeNoParNoted = noted; }
	// completed files and stream-repair jobs (ops in rust/src/qnzb.rs nzbget_rs_nzb_files)
	int64 NF(int op, int a2 = 0, NzbInfo* p2 = nullptr, const char* s1 = nullptr, const char* s2 = nullptr,
		int* ints = nullptr, int64_t* i64s = nullptr, const char** strs = nullptr, const int64_t* raw = nullptr)
	{
		return nzbget_rs_nzb_files(m_s, op, a2, p2 ? p2->m_s : nullptr, s1, s2, ints, i64s, strs, raw);
	}
	int GetCompletedFileCount() { return (int)NF(0); }
	void AddCompletedFile(int id, const char* filename, const char* origname, CompletedFile::EStatus status,
		uint32 crc, bool parFile, const char* hash16k, const char* parSetId)
	{
		FileInfo::RaiseIdFloor(id);
		int ints[4] = { id, (int)status, (int)crc, parFile ? 1 : 0 };
		const char* strs[4] = { filename, origname, hash16k, parSetId };
		NF(2, 0, nullptr, nullptr, nullptr, ints, nullptr, strs);
	}
	bool HasStreamRepairJobs() { return NF(20) > 0; }
	int GetPriority() { return m_s->priority; }
	void SetPriority(int priority) { m_s->priority = priority; }
	int GetExtraPriority() { return m_s->extraPriority; }
	void SetExtraPriority(int extraPriority) { m_s->extraPriority = extraPriority; }
	bool HasExtraPriority() { return m_s->extraPriority > 0; }
	bool GetForcePriority() { return m_s->priority >= FORCE_PRIORITY; }
	time_t GetMinTime() { return m_s->minTime; }
	void SetMinTime(time_t minTime) { m_s->minTime = minTime; }
	time_t GetMaxTime() { return m_s->maxTime; }
	void SetMaxTime(time_t maxTime) { m_s->maxTime = maxTime; }
	CString BuildFinalDirName()
	{
		const char* categoryDestDir = nullptr;
		if (!Util::EmptyStr(NT(5)))
		{
			const char* strs[3]{};
			if (g_Options->QueryCategory(NT(5), false, nullptr, strs) && !Util::EmptyStr(strs[1]))
			{
				categoryDestDir = strs[1];
			}
		}
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			2, g_Options->GetDestDir(), categoryDestDir, NT(5),
			GetName(), nullptr, g_Options->GetAppendCategoryDir() ? 1 : 0, GetId());
		CString out(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return out;
	}
	void BuildDestDirName()
	{
		const char* categoryDestDir = nullptr;
		if (Util::EmptyStr(g_Options->GetInterDir()) && !Util::EmptyStr(NT(5)))
		{
			const char* strs[3]{};
			if (g_Options->QueryCategory(NT(5), false, nullptr, strs) && !Util::EmptyStr(strs[1]))
			{
				categoryDestDir = strs[1];
			}
		}
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			3, g_Options->GetInterDir(), g_Options->GetDestDir(), categoryDestDir,
			NT(5), GetName(), g_Options->GetAppendCategoryDir() ? 1 : 0, GetId());
		nzbget_rs_nzb_set_text(m_s, 3, buf.data, (intptr_t)buf.len);
		nzbget_rs_free(buf);
	}
	bool RenameCompletedFile(const char* oldName, const char* newAbsoluteOrRelativePath)
	{
		return NF(4, 0, nullptr, oldName, newAbsoluteOrRelativePath) != 0;
	}
	void SetDirectRenameStatus(EDirectRenameStatus renameStatus) { m_s->directRenameStatus = renameStatus; }
	EDirectRenameStatus GetDirectRenameStatus() { return (EDirectRenameStatus)m_s->directRenameStatus; }
	EPostRenameStatus GetParRenameStatus() { return (EPostRenameStatus)m_s->parRenameStatus; }
	void SetParRenameStatus(EPostRenameStatus renameStatus) { m_s->parRenameStatus = renameStatus; }
	EPostRenameStatus GetRarRenameStatus() { return (EPostRenameStatus)m_s->rarRenameStatus; }
	void SetRarRenameStatus(EPostRenameStatus renameStatus) { m_s->rarRenameStatus = renameStatus; }
	EParStatus GetParStatus() { return (EParStatus)m_s->parStatus; }
	void SetParStatus(EParStatus parStatus) { m_s->parStatus = parStatus; }
	EDirectUnpackStatus GetDirectUnpackStatus() { return (EDirectUnpackStatus)m_s->directUnpackStatus; }
	void SetDirectUnpackStatus(EDirectUnpackStatus directUnpackStatus) { m_s->directUnpackStatus = directUnpackStatus; }
	EPostUnpackStatus GetUnpackStatus() { return (EPostUnpackStatus)m_s->unpackStatus; }
	void SetUnpackStatus(EPostUnpackStatus unpackStatus) { m_s->unpackStatus = unpackStatus; }
	ECleanupStatus GetCleanupStatus() { return (ECleanupStatus)m_s->cleanupStatus; }
	void SetCleanupStatus(ECleanupStatus cleanupStatus) { m_s->cleanupStatus = cleanupStatus; }
	EMoveStatus GetMoveStatus() { return (EMoveStatus)m_s->moveStatus; }
	void SetPostUnpackRenamingStatus(PostUnpackRenamingStatus status) { m_s->postUnpackRenamingStatus = (int32_t)status; }
	PostUnpackRenamingStatus GetPostUnpackRenamingStatus() { return (PostUnpackRenamingStatus)m_s->postUnpackRenamingStatus; }
	void SetPostDownloadRenamingStatus(PostDownloadRenamingStatus status) { m_s->postDownloadRenamingStatus = (int32_t)status; }
	PostDownloadRenamingStatus GetPostDownloadRenamingStatus() { return (PostDownloadRenamingStatus)m_s->postDownloadRenamingStatus; }
	void SetMoveStatus(EMoveStatus moveStatus) { m_s->moveStatus = moveStatus; }
	EDeleteStatus GetDeleteStatus() { return (EDeleteStatus)m_s->deleteStatus; }
	void SetDeleteStatus(EDeleteStatus deleteStatus) { m_s->deleteStatus = deleteStatus; }
	EMarkStatus GetMarkStatus() { return (EMarkStatus)m_s->markStatus; }
	void SetMarkStatus(EMarkStatus markStatus) { m_s->markStatus = markStatus; }
	EUrlStatus GetUrlStatus() { return (EUrlStatus)m_s->urlStatus; }
	int GetExtraParBlocks() { return m_s->extraParBlocks; }
	void SetExtraParBlocks(int extraParBlocks) { m_s->extraParBlocks = extraParBlocks; }
	void SetUrlStatus(EUrlStatus urlStatus) { m_s->urlStatus = urlStatus; }
	const char* GetQueuedFilename() { return NT(6); }
	void SetQueuedFilename(const char* queuedFilename) { NST(6, queuedFilename); }
	bool GetDeleting() { return m_s->deleting; }
	void SetDeleting(bool deleting) { m_s->deleting = deleting; }
	bool GetParking() { return m_s->parking; }
	void SetParking(bool parking) { m_s->parking = parking; }
	bool GetDeletePaused() { return m_s->deletePaused; }
	void SetDeletePaused(bool deletePaused) { m_s->deletePaused = deletePaused; }
	bool GetManyDupeFiles() { return m_s->manyDupeFiles; }
	void SetManyDupeFiles(bool manyDupeFiles) { m_s->manyDupeFiles = manyDupeFiles; }
	bool GetAvoidHistory() { return m_s->avoidHistory; }
	void SetAvoidHistory(bool avoidHistory) { m_s->avoidHistory = avoidHistory; }
	bool GetHealthPaused() { return m_s->healthPaused; }
	void SetHealthPaused(bool healthPaused) { m_s->healthPaused = healthPaused; }
	bool GetCleanupDisk() { return m_s->cleanupDisk; }
	void SetCleanupDisk(bool cleanupDisk) { m_s->cleanupDisk = cleanupDisk; }
	bool GetUnpackCleanedUpDisk() { return m_s->unpackCleanedUpDisk; }
	void SetUnpackCleanedUpDisk(bool unpackCleanedUpDisk) { m_s->unpackCleanedUpDisk = unpackCleanedUpDisk; }
	bool GetAddUrlPaused() { return m_s->addUrlPaused; }
	void SetAddUrlPaused(bool addUrlPaused) { m_s->addUrlPaused = addUrlPaused; }
	FileList* GetFileList() { return &m_fileList; }
	// parameters, script statuses, blocked donors and server stats live in Rust;
	// op codes as in rust/src/qnzb.rs nzbget_rs_nzb_coll
	int64 NC(int op, int a2 = 0, NzbInfo* other = nullptr, const char* s1 = nullptr, const char* s2 = nullptr,
		int* ints = nullptr, const char** strs = nullptr)
	{
		return nzbget_rs_nzb_coll(m_s, op, a2, other ? other->m_s : nullptr, s1, s2, ints, strs);
	}
	ScriptStatus::EStatus ScriptTotalStatus() { return (ScriptStatus::EStatus)NC(14); }
	// applies op (0 set, 1 add, 2 subtract) of a file's server stats to the
	// totals (which 0) or the current-session list (which 1)
	void AddFileStats(FileInfo* file, int which, int op)
	{
		for (size_t i = 0, n = file->StatCount(); i < n; i++)
		{
			int v[3];
			file->StatAt(i, v);
			NC(44 + which, op, nullptr, nullptr, nullptr, v);
		}
	}
	int CalcHealth()
	{
		return nzbget_rs_nzb_calc_health(
			0, m_s->size, m_s->parSize, m_s->currentFailedSize, m_s->parCurrentFailedSize, 0);
	}
	int CalcCriticalHealth(bool allowEstimation)
	{
		return nzbget_rs_nzb_calc_health(
			1, m_s->size, m_s->parSize, m_s->currentFailedSize, m_s->parCurrentFailedSize, allowEstimation ? 1 : 0);
	}
	const char* GetDupeKey() { return NT(7); }
	void SetDupeKey(const char* dupeKey) { NST(7, dupeKey ? dupeKey : ""); }
	int GetDupeScore() { return m_s->dupeScore; }
	void SetDupeScore(int dupeScore) { m_s->dupeScore = dupeScore; }
	EDupeMode GetDupeMode() { return (EDupeMode)m_s->dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { m_s->dupeMode = dupeMode; }
	EDupeHint GetDupeHint() { return (EDupeHint)m_s->dupeHint; }
	void SetDupeHint(EDupeHint dupeHint) { m_s->dupeHint = dupeHint; }
	uint32 GetFullContentHash() { return m_s->fullContentHash; }
	void SetFullContentHash(uint32 fullContentHash) { m_s->fullContentHash = fullContentHash; }
	uint32 GetFilteredContentHash() { return m_s->filteredContentHash; }
	void SetFilteredContentHash(uint32 filteredContentHash) { m_s->filteredContentHash = filteredContentHash; }
	int64 GetDownloadedSize() { return m_s->downloadedSize; }
	void SetDownloadedSize(int64 downloadedSize) { m_s->downloadedSize = downloadedSize; }
	int GetDownloadSec() { return m_s->downloadSec; }
	void SetDownloadSec(int downloadSec) { m_s->downloadSec = downloadSec; }
	int GetPostTotalSec() { return m_s->postTotalSec; }
	void SetPostTotalSec(int postTotalSec) { m_s->postTotalSec = postTotalSec; }
	int GetParSec() { return m_s->parSec; }
	void SetParSec(int parSec) { m_s->parSec = parSec; }
	int GetRepairSec() { return m_s->repairSec; }
	void SetRepairSec(int repairSec) { m_s->repairSec = repairSec; }
	int GetUnpackSec() { return m_s->unpackSec; }
	void SetUnpackSec(int unpackSec) { m_s->unpackSec = unpackSec; }
	time_t GetDownloadStartTime() { return m_s->downloadStartTime; }
	void SetDownloadStartTime(time_t downloadStartTime) { m_s->downloadStartTime = downloadStartTime; }
	bool GetChanged() { return m_s->changed; }
	void SetChanged(bool changed) { m_s->changed = changed; }
	void SetReprocess(bool reprocess) { m_s->reprocess = reprocess; }
	bool GetReprocess() { return m_s->reprocess; }
	time_t GetQueueScriptTime() { return m_s->queueScriptTime; }
	void SetQueueScriptTime(time_t queueScriptTime) { m_s->queueScriptTime = queueScriptTime; }
	void SetParFull(bool parFull) { m_s->parFull = parFull; }
	bool GetParFull() { return m_s->parFull; }
	int GetFeedId() { return m_s->feedId; }
	void SetFeedId(int feedId) { m_s->feedId = feedId; }
	void MoveFileList(NzbInfo* srcNzbInfo)
	{
		m_fileList = std::move(*srcNzbInfo->GetFileList());
		for (FileInfo* fileInfo : &m_fileList)
		{
			fileInfo->SetNzbInfo(this);
		}

		SetFullContentHash(srcNzbInfo->GetFullContentHash());
		SetFilteredContentHash(srcNzbInfo->GetFilteredContentHash());

		SetFileCount(srcNzbInfo->GetFileCount());
		SetPausedFileCount(srcNzbInfo->GetPausedFileCount());
		SetRemainingParCount(srcNzbInfo->GetRemainingParCount());

		SetSize(srcNzbInfo->GetSize());
		SetRemainingSize(srcNzbInfo->GetRemainingSize());
		SetPausedSize(srcNzbInfo->GetPausedSize());
		SetSuccessSize(srcNzbInfo->GetSuccessSize());
		SetCurrentSuccessSize(srcNzbInfo->GetCurrentSuccessSize());
		SetFailedSize(srcNzbInfo->GetFailedSize());
		SetCurrentFailedSize(srcNzbInfo->GetCurrentFailedSize());

		SetParSize(srcNzbInfo->GetParSize());
		SetParSuccessSize(srcNzbInfo->GetParSuccessSize());
		SetParCurrentSuccessSize(srcNzbInfo->GetParCurrentSuccessSize());
		SetParFailedSize(srcNzbInfo->GetParFailedSize());
		SetParCurrentFailedSize(srcNzbInfo->GetParCurrentFailedSize());

		SetTotalArticles(srcNzbInfo->GetTotalArticles());
		SetSuccessArticles(srcNzbInfo->GetSuccessArticles());
		SetFailedArticles(srcNzbInfo->GetFailedArticles());
		SetCurrentSuccessArticles(srcNzbInfo->GetSuccessArticles());
		SetCurrentFailedArticles(srcNzbInfo->GetFailedArticles());

		SetMinTime(srcNzbInfo->GetMinTime());
		SetMaxTime(srcNzbInfo->GetMaxTime());
	}
	void UpdateMinMaxTime()
	{
		std::vector<int64_t> times;
		times.reserve(m_fileList.size());
		for (FileInfo* f : &m_fileList) times.push_back((int64_t)f->GetTime());
		int64_t minT = 0, maxT = 0;
		nzbget_rs_nzb_calc_min_max_time(times.data(), times.size(), &minT, &maxT);
		m_s->minTime = (time_t)minT;
		m_s->maxTime = (time_t)maxT;
	}
	bool GetParRepaired() { return m_s->parRepaired; }
	void SetParRepaired(bool parRepaired) { m_s->parRepaired = parRepaired; }
	bool GetUnpackTried() { return m_s->unpackTried; }
	void SetUnpackTried(bool unpackTried) { m_s->unpackTried = unpackTried; }
	int GetLastUnpackStatus() { return m_s->lastUnpackStatus; }
	void SetLastUnpackStatus(int lastUnpackStatus) { m_s->lastUnpackStatus = lastUnpackStatus; }
	// archives unpacked so far (ops 30-33 of nzbget_rs_nzb_files)
	int64 EA(int op, int index = 0, const char* s1 = nullptr, const char** strs = nullptr)
	{
		return NF(30 + op, index, nullptr, s1, nullptr, nullptr, nullptr, strs);
	}

	class PostInfo
	{
	public:
		enum EStage
		{
			ptQueued,
			ptLoadingPars,
			ptVerifyingSources,
			ptRepairing,
			ptVerifyingRepaired,
			ptParRenaming,
			ptRarRenaming,
			ptUnpacking,
			ptCleaningUp,
			ptMoving,
			ptPostUnpackRenaming,
			ptExecutingScript,
			ptFinished,
			// appended after ptFinished: stage numbers are persisted (stage+1 in
			// the queue file) and range checks rely on the existing order
			ptPostDownloadRenaming,
			ptStreamRepairing
		};

		// the state lives in Rust (rust/src/qnzb.rs nzbget_rs_post_op)
		PostInfo() = default;
		PostInfo(const PostInfo&) = delete;
		PostInfo& operator=(const PostInfo&) = delete;
		~PostInfo() { nzbget_rs_post_free(m_rs); }

		NzbInfo* GetNzbInfo() { return m_nzbInfo; }
		void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
		EStage GetStage() { return (EStage)V(0); }
		void SetStage(EStage stage) { SetV(0, stage); }
		void SetProgressLabel(const char* progressLabel) { nzbget_rs_post_op(m_rs, 11, 0, 0, progressLabel, nullptr); }
		// a copy, valid on this thread until the next call
		const char* GetProgressLabel()
		{
			static thread_local CString label;
			NzbgetRsBuf buf = nzbget_rs_post_label(m_rs);
			if (buf.data && buf.len > 0) label.Set(reinterpret_cast<const char*>(buf.data), (int)buf.len); else label = "";
			nzbget_rs_free(buf);
			return label;
		}
		int GetFileProgress() { return (int)V(13); }
		void SetFileProgress(int fileProgress) { SetV(13, fileProgress); }
		int GetStageProgress() { return (int)V(14); }
		void SetStageProgress(int stageProgress) { SetV(14, std::clamp(stageProgress, 0, 1000)); }
		time_t GetStartTime() { return (time_t)V(17); }
		void SetStartTime(time_t startTime) { SetV(17, (int64)startTime); }
		time_t GetStageTime() { return (time_t)V(18); }
		void SetStageTime(time_t stageTime) { SetV(18, (int64)stageTime); }
		bool GetWorking() { return V(1) != 0; }
		void SetWorking(bool working) { SetV(1, working); }
		bool GetDeleted() { return V(2) != 0; }
		void SetDeleted(bool deleted) { SetV(2, deleted); }
		bool GetRequestParCheck() { return V(3) != 0; }
		void SetRequestParCheck(bool requestParCheck) { SetV(3, requestParCheck); }
		bool GetForceParFull() { return V(4) != 0; }
		void SetForceParFull(bool forceParFull) { SetV(4, forceParFull); }
		// the post-processing stream repair has run, and whether it wrote bytes
		bool GetStreamRepairDone() { return V(15) != 0; }
		void SetStreamRepairDone(bool streamRepairDone) { SetV(15, streamRepairDone); }
		bool GetStreamRepairRecovered() { return V(16) != 0; }
		void SetStreamRepairRecovered(bool streamRepairRecovered) { SetV(16, streamRepairRecovered); }
		// par-check was postponed because the par2 files cannot cover the damage
		bool GetParCannotCover() { return V(5) != 0; }
		void SetParCannotCover(bool parCannotCover) { SetV(5, parCannotCover); }
		bool GetForceRepair() { return V(8) != 0; }
		void SetForceRepair(bool forceRepair) { SetV(8, forceRepair); }
		bool GetParRepaired() { return m_nzbInfo ? m_nzbInfo->GetParRepaired() : V(9) != 0; }
		void SetParRepaired(bool parRepaired)
		{
			SetV(9, parRepaired);
			if (m_nzbInfo) m_nzbInfo->SetParRepaired(parRepaired);
		}
		bool GetUnpackTried() { return m_nzbInfo ? m_nzbInfo->GetUnpackTried() : V(10) != 0; }
		void SetUnpackTried(bool unpackTried)
		{
			SetV(10, unpackTried);
			if (m_nzbInfo) m_nzbInfo->SetUnpackTried(unpackTried);
		}
		bool GetPassListTried() { return V(11) != 0; }
		void SetPassListTried(bool passListTried) { SetV(11, passListTried); }
		int GetLastUnpackStatus() { return m_nzbInfo ? m_nzbInfo->GetLastUnpackStatus() : (int)V(12); }
		void SetLastUnpackStatus(int unpackStatus)
		{
			SetV(12, unpackStatus);
			if (m_nzbInfo) m_nzbInfo->SetLastUnpackStatus(unpackStatus);
		}
		bool GetNeedParCheck() { return V(6) != 0; }
		void SetNeedParCheck(bool needParCheck) { SetV(6, needParCheck); }
		bool GetNeedUnpack() { return V(7) != 0; }
		void SetNeedUnpack(bool needUnpack) { SetV(7, needUnpack); }
		JobThread* GetPostThread() { return reinterpret_cast<JobThread*>((uintptr_t)V(19)); }
		void SetPostThread(JobThread* postThread) { SetV(19, (int64)reinterpret_cast<uintptr_t>(postThread)); }
		int GetParredFileCount() { return (int)nzbget_rs_post_op(m_rs, 20, 0, 0, nullptr, nullptr); }
		const char* GetParredFile(int index)
		{
			const char* s = "";
			nzbget_rs_post_op(m_rs, 21, index, 0, nullptr, &s);
			return s;
		}
		void AddParredFile(const char* filename) { nzbget_rs_post_op(m_rs, 22, 0, 0, filename ? filename : "", nullptr); }
		// extracted archives belong to the collection; a post job without one keeps its own
		int GetExtractedArchiveCount();
		const char* GetExtractedArchive(int index);
		void ClearExtractedArchives();

	private:
		NzbInfo* m_nzbInfo = nullptr;
		RsPost* m_rs = nzbget_rs_post_new();
		int64 V(int id) { return nzbget_rs_post_op(m_rs, 0, id, 0, nullptr, nullptr); }
		void SetV(int id, int64 v) { nzbget_rs_post_op(m_rs, 1, id, v, nullptr, nullptr); }
		int64 EA(int op, int index = 0, const char* s1 = nullptr, const char** strs = nullptr);
	};

	PostInfo* GetPostInfo() { return m_postInfo.get(); }
	void EnterPostProcess()
	{
		m_postInfo = std::make_unique<PostInfo>();
		m_postInfo->SetNzbInfo(this);
	}
	void LeavePostProcess()
	{
		m_postInfo.reset();
		m_s->unpackTried = false;
		m_s->lastUnpackStatus = 0;
		EA(3);
		// messages stay: there are no NZB log files, so a finished download's Log tab
		// shows what is kept here (LogBuffer bounds it)
	}
	bool IsDupeSuccess()
	{
		return nzbget_rs_nzb_is_dupe_success(
			static_cast<int>(m_s->markStatus), static_cast<int>(m_s->deleteStatus),
			static_cast<int>(m_s->parStatus), static_cast<int>(m_s->unpackStatus),
			static_cast<int>(m_s->urlStatus), CalcHealth(), CalcCriticalHealth(true),
			CalcCriticalHealth(false), g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0) != 0;
	}
	const char* MakeTextStatus(bool ignoreScriptStatus)
	{
		ScriptStatus::EStatus scriptStatus = ignoreScriptStatus ?
			ScriptStatus::srSuccess : ScriptTotalStatus();
		return nzbget_rs_nzb_make_text_status(
			static_cast<int>(m_s->kind), static_cast<int>(m_s->markStatus),
			static_cast<int>(m_s->deleteStatus), static_cast<int>(m_s->parStatus),
			static_cast<int>(m_s->unpackStatus), static_cast<int>(m_s->moveStatus),
			static_cast<int>(m_s->urlStatus), static_cast<int>(scriptStatus),
			CalcHealth(), CalcCriticalHealth(false));
	}
	void AddMessage(Message::EKind kind, const char* text, bool print = true);
	void PrintMessage(Message::EKind kind, const char* format, ...) PRINTF_SYNTAX(3)
	{
		char tmp2[1024];

		va_list ap;
		va_start(ap, format);
		vsnprintf(tmp2, 1024, format, ap);
		tmp2[1024-1] = '\0';
		va_end(ap);

		AddMessage(kind, tmp2);
	}
	int GetMessageCount() const { return m_messageCount; }
	void SetMessageCount(int messageCount) { m_messageCount = messageCount; }
	int GetCachedMessageCount() { return m_s->cachedMessageCount; }
	// message log ops in rust/src/qnzb.rs nzbget_rs_nzb_msgs
	int64 NM(int op, int a2 = 0, NzbInfo* p2 = nullptr, const char* s1 = nullptr, const int* ints = nullptr,
		void (*visit)(uintptr_t, int, int, int, const char*) = nullptr, uintptr_t ctx = 0)
	{
		return nzbget_rs_nzb_msgs(m_s, op, a2, p2 ? p2->m_s : nullptr, s1, ints, visit, ctx);
	}
	bool GetAllFirst() { return m_s->allFirst; }
	void SetAllFirst(bool allFirst) { m_s->allFirst = allFirst; }
	bool GetWaitingPar() { return m_s->waitingPar; }
	void SetWaitingPar(bool waitingPar) { m_s->waitingPar = waitingPar; }
	bool GetLoadingPar() { return m_s->loadingPar; }
	void SetLoadingPar(bool loadingPar) { m_s->loadingPar = loadingPar; }
	JobThread* GetUnpackThread() { return m_unpackThread; }
	void SetUnpackThread(JobThread* unpackThread) { m_unpackThread = unpackThread; }
	JobThread* GetLiveRepairThread() { return m_liveRepairThread; }
	void SetLiveRepairThread(JobThread* liveRepairThread) { m_liveRepairThread = liveRepairThread; }
	void UpdateCurrentStats()
	{
		RsDownloadCurrentStatsAcc acc{};
		acc.current_success_articles = m_s->successArticles;
		acc.current_failed_articles = m_s->failedArticles;
		acc.current_success_size = m_s->successSize;
		acc.current_failed_size = m_s->failedSize;
		acc.par_current_success_size = m_s->parSuccessSize;
		acc.par_current_failed_size = m_s->parFailedSize;
		NC(48);
		for (FileInfo* fileInfo : &m_fileList)
		{
			nzbget_rs_nzb_update_current_stats_step(
				&acc, fileInfo->GetRemainingSize(), fileInfo->GetSuccessArticles(),
				fileInfo->GetFailedArticles(), fileInfo->GetSuccessSize(), fileInfo->GetFailedSize(),
				fileInfo->GetExtraPriority() ? 1 : 0, fileInfo->GetPaused() ? 1 : 0, fileInfo->GetParFile() ? 1 : 0);
			AddFileStats(fileInfo, 1, 1);
		}
		m_s->pausedFileCount = acc.paused_file_count;
		m_s->remainingParCount = acc.remaining_par_count;
		m_s->remainingSize = acc.remaining_size;
		m_s->pausedSize = acc.paused_size;
		m_s->currentSuccessArticles = acc.current_success_articles;
		m_s->currentFailedArticles = acc.current_failed_articles;
		m_s->currentSuccessSize = acc.current_success_size;
		m_s->currentFailedSize = acc.current_failed_size;
		m_s->parCurrentSuccessSize = acc.par_current_success_size;
		m_s->parCurrentFailedSize = acc.par_current_failed_size;
		m_s->extraPriority = acc.extra_priority;
	}
	void UpdateCompletedStats(FileInfo* fileInfo)
	{
		nzbget_rs_nzb_update_completed_stats(
			fileInfo->GetSuccessSize(), fileInfo->GetFailedSize(),
			fileInfo->GetFailedArticles(), fileInfo->GetSuccessArticles(),
			fileInfo->GetExtraPriority() ? 1 : 0, fileInfo->GetParFile() ? 1 : 0,
			fileInfo->GetPaused() ? 1 : 0,
			&m_s->successSize, &m_s->failedSize, &m_s->failedArticles, &m_s->successArticles,
			&m_s->extraPriority, &m_s->parSuccessSize, &m_s->parFailedSize,
			&m_s->remainingParCount, &m_s->pausedFileCount);
		AddFileStats(fileInfo, 0, 1);
	}
	void UpdateDeletedStats(FileInfo* fileInfo)
	{
		nzbget_rs_nzb_update_deleted_stats(
			fileInfo->GetSize(), fileInfo->GetSuccessSize(), fileInfo->GetFailedSize(),
			fileInfo->GetMissedSize(), fileInfo->GetTotalArticles(),
			fileInfo->GetSuccessArticles(), fileInfo->GetFailedArticles(),
			fileInfo->GetMissedArticles(), fileInfo->GetRemainingSize(),
			fileInfo->GetExtraPriority() ? 1 : 0, fileInfo->GetParFile() ? 1 : 0,
			fileInfo->GetPaused() ? 1 : 0,
			&m_s->fileCount, &m_s->size, &m_s->currentSuccessSize, &m_s->failedSize,
			&m_s->failedArticles, &m_s->currentFailedSize, &m_s->totalArticles,
			&m_s->currentSuccessArticles, &m_s->currentFailedArticles, &m_s->remainingSize,
			&m_s->extraPriority, &m_s->remainingParCount, &m_s->parSize,
			&m_s->parCurrentSuccessSize, &m_s->parFailedSize, &m_s->parCurrentFailedSize,
			&m_s->pausedFileCount, &m_s->pausedSize);
		AddFileStats(fileInfo, 1, 2);
	}
	bool IsDownloadCompleted(bool ignorePausedPars)
	{
		if (m_s->activeDownloads != 0)
		{
			return false;
		}
		for (FileInfo* fileInfo : &m_fileList)
		{
			if (((!fileInfo->GetPaused() || !ignorePausedPars) || !fileInfo->GetParFile()) &&
				!fileInfo->GetDeleted())
			{
				return false;
			}
		}
		return true;
	}
	void SetDesiredServerId(int id) { m_s->desiredServerId = id; }
	int GetDesiredServerId() { return m_s->desiredServerId; }
	bool GetSkipScriptProcessing() { return m_s->skipScriptProcessing; }
	void SetSkipScriptProcessing(bool skip) { m_s->skipScriptProcessing = skip; }
	void SetSkipDiskWrite(bool skipWrite) { m_s->skipDiskWrite = skipWrite; }
	bool GetSkipDiskWrite() { return m_s->skipDiskWrite; }
	void SetAutoCategory(bool autoCategory) { m_s->autoCategory = autoCategory; }
	bool GetAutoCategory() const { return m_s->autoCategory; }
	bool HasDesiredServer() const { return m_s->desiredServerId > 0; }

	static const int FORCE_PRIORITY = 900;

private:
	// aggregate of FileInfo::m_s->dupeRecoveredArticles (persisted for restart)
	// failed articles that were tried from duplicates / that no duplicate
	// could even offer a source for (the early dupe failover's sample of how
	// the download-time recovery is faring; not persisted)
	// failed-article count at the last history scan of the early dupe failover
	// (-1 = never scanned), so a download without a backup is not rescanned
	// for every failed article
	// which way the par-first rule of duplicate recovery went, for logging once (not persisted)
	// missing byte ranges of completed media files awaiting stream repair
	// from duplicate collections in post-processing (persisted)
	FileList m_fileList;
	std::unique_ptr<PostInfo> m_postInfo;
	std::atomic<int> m_messageCount{0};
	JobThread* m_unpackThread = nullptr;
	// download-concurrent stream repair for completed-with-holes files (see
	// option <DupeArticleFallback> value "live"; not persisted)
	JobThread* m_liveRepairThread = nullptr;

	NzbgetRsNzbScalars* m_s = nzbget_rs_nzb_scalars_new();
	inline static std::atomic<int> m_idGen{0};
	inline static int m_idMax = 0;

	void ClearMessages() { NM(2); }

	friend class DupInfo;
};

using PostInfo = NzbInfo::PostInfo;

inline int64 PostInfo::EA(int op, int index, const char* s1, const char** strs)
{
	if (m_nzbInfo) return m_nzbInfo->EA(op, index, s1, strs);
	return nzbget_rs_post_op(m_rs, 30 + op, index, 0, s1, strs);
}
inline int PostInfo::GetExtractedArchiveCount() { return (int)EA(0); }
inline const char* PostInfo::GetExtractedArchive(int index)
{
	const char* s = "";
	EA(1, index, nullptr, &s);
	return s;
}
inline void PostInfo::ClearExtractedArchives() { EA(3); }

inline void FileInfo::SetPaused(bool paused)
{
	NzbInfo* nzb = GetNzbInfo();
	if (GetPaused() != paused && nzb)
	{
		nzb->SetPausedFileCount(nzb->GetPausedFileCount() + (paused ? 1 : -1));
		nzb->SetPausedSize(nzb->GetPausedSize() + (paused ? GetRemainingSize() : -GetRemainingSize()));
	}
	S(19, paused ? 1 : 0);
}

inline void FileInfo::SetExtraPriority(bool extraPriority)
{
	NzbInfo* nzb = GetNzbInfo();
	if (GetExtraPriority() != extraPriority && nzb)
	{
		nzb->SetExtraPriority(nzb->GetExtraPriority() + (extraPriority ? 1 : -1));
	}
	S(25, extraPriority ? 1 : 0);
}

typedef UniqueDeque<NzbInfo> NzbList;
typedef std::vector<NzbInfo*> RawNzbList;

typedef std::vector<int> IdList;

typedef std::vector<CString> NameList;

class DupInfo
{
public:
	enum EStatus
	{
		dsUndefined,
		dsSuccess,
		dsFailed,
		dsDeleted,
		dsDupe,
		dsBad,
		dsGood
	};

	// the record lives in Rust (rust/src/qdup.rs)
	DupInfo() = default;
	DupInfo(const DupInfo&) = delete;
	DupInfo& operator=(const DupInfo&) = delete;
	~DupInfo() { nzbget_rs_dup_free(m_rs); }

	int GetId() { return (int)Get(5); }
	void SetId(int id)
	{
		nzbget_rs_dup_set(m_rs, 5, id, nullptr);
		if (NzbInfo::m_idMax < id)
		{
			NzbInfo::m_idMax = id;
		}
	}
	const char* GetName() { return Text(0); }
	void SetName(const char* name) { nzbget_rs_dup_set(m_rs, 7, 0, name); }
	const char* GetDupeKey() { return Text(1); }
	void SetDupeKey(const char* dupeKey) { nzbget_rs_dup_set(m_rs, 8, 0, dupeKey); }
	int GetDupeScore() { return (int)Get(1); }
	void SetDupeScore(int dupeScore) { nzbget_rs_dup_set(m_rs, 1, dupeScore, nullptr); }
	EDupeMode GetDupeMode() { return (EDupeMode)Get(0); }
	void SetDupeMode(EDupeMode dupeMode) { nzbget_rs_dup_set(m_rs, 0, dupeMode, nullptr); }
	int64 GetSize() { return Get(6); }
	void SetSize(int64 size) { nzbget_rs_dup_set(m_rs, 6, size, nullptr); }
	uint32 GetFullContentHash() { return (uint32)Get(3); }
	void SetFullContentHash(uint32 fullContentHash) { nzbget_rs_dup_set(m_rs, 3, fullContentHash, nullptr); }
	uint32 GetFilteredContentHash() { return (uint32)Get(4); }
	void SetFilteredContentHash(uint32 filteredContentHash) { nzbget_rs_dup_set(m_rs, 4, filteredContentHash, nullptr); }
	EStatus GetStatus() { return (EStatus)Get(2); }
	void SetStatus(EStatus Status) { nzbget_rs_dup_set(m_rs, 2, Status, nullptr); }
	// every field at once (see nzbget_rs_dup_get)
	void Read(int* ints, int64_t* i64s, const char** strs) { nzbget_rs_dup_get(m_rs, ints, i64s, strs); }

private:
	RsDup* m_rs = nzbget_rs_dup_new();
	int64 Get(int field)
	{
		int ints[6]; int64_t size = 0; const char* strs[2];
		nzbget_rs_dup_get(m_rs, ints, &size, strs);
		return field == 6 ? size : ints[field];
	}
	const char* Text(int which)
	{
		int ints[6]; const char* strs[2];
		nzbget_rs_dup_get(m_rs, ints, nullptr, strs);
		return strs[which];
	}
};

class HistoryInfo
{
public:
	enum EKind
	{
		hkUnknown,
		hkNzb,
		hkUrl,
		hkDup
	};

	HistoryInfo(std::unique_ptr<NzbInfo> nzbInfo) : m_info(nzbInfo.release()),
		m_kind(GetNzbInfo()->GetKind() == NzbInfo::nkNzb ? hkNzb : hkUrl) {}
	HistoryInfo(std::unique_ptr<DupInfo> dupInfo) : m_info(dupInfo.release()), m_kind(hkDup) {}
	~HistoryInfo()
	{
		if ((m_kind == hkNzb || m_kind == hkUrl) && m_info)
		{
			delete (NzbInfo*)m_info;
		}
		else if (m_kind == hkDup && m_info)
		{
			delete (DupInfo*)m_info;
		}
	}
	EKind GetKind() { return m_kind; }
	int GetId()
	{
		if ((m_kind == hkNzb || m_kind == hkUrl))
		{
			return ((NzbInfo*)m_info)->GetId();
		}
		else // if (m_eKind == hkDup)
		{
			return ((DupInfo*)m_info)->GetId();
		}
	}
	NzbInfo* GetNzbInfo() { return (NzbInfo*)m_info; }
	DupInfo* GetDupInfo() { return (DupInfo*)m_info; }
	void DiscardNzbInfo() { m_info = nullptr; }
	time_t GetTime() { return m_time; }
	void SetTime(time_t time) { m_time = time; }
	const char* GetName()
	{
		if (m_kind == hkNzb || m_kind == hkUrl)
		{
			return GetNzbInfo()->GetName();
		}
		else if (m_kind == hkDup)
		{
			return GetDupInfo()->GetName();
		}
		else
		{
			return "<unknown>";
		}
	}

private:
	void* m_info;
	EKind m_kind;
	time_t m_time = 0;
};

typedef UniqueDeque<HistoryInfo> HistoryList;

typedef GuardedPtr<DownloadQueue> GuardedDownloadQueue;

class DownloadQueue
{
public:
	enum EAspectAction
	{
		eaNzbFound,
		eaNzbAdded,
		eaNzbDeleted,
		eaNzbNamed,
		eaNzbReturned,
		eaFileCompleted,
		eaFileDeleted,
		eaUrlFound,
		eaUrlAdded,
		eaUrlDeleted,
		eaUrlCompleted,
		eaUrlFailed,
		eaUrlReturned
	};

	struct Aspect
	{
		EAspectAction action;
		DownloadQueue* downloadQueue;
		NzbInfo* nzbInfo;
		FileInfo* fileInfo;
	};

	// tells the pre/post-processor about a queue change (rust/src/jobthread.rs)
	void Notify(Aspect* aspect)
	{
		nzbget_rs_prepost_queue_changed((int)aspect->action, reinterpret_cast<uintptr_t>(aspect->downloadQueue),
			reinterpret_cast<uintptr_t>(aspect->nzbInfo), reinterpret_cast<uintptr_t>(aspect->fileInfo));
	}

	enum EEditAction
	{
		eaFileMoveOffset = 1, // move files to m_iOffset relative to the current position in download-queue
		eaFileMoveTop, // move files to the top of download-queue
		eaFileMoveBottom, // move files to the bottom of download-queue
		eaFilePause, // pause files
		eaFileResume, // resume (unpause) files
		eaFileDelete, // delete files
		eaFilePauseAllPars, // pause only (all) pars (does not affect other files)
		eaFilePauseExtraPars, // pause (almost all) pars, except main par-file (does not affect other files)
		eaFileReorder, // set file order
		eaFileSplit, // split - create new group from selected files
		eaGroupMoveOffset, // move group to offset relative to the current position in download-queue
		eaGroupMoveTop, // move group to the top of download-queue
		eaGroupMoveBottom, // move group to the bottom of download-queue
		eaGroupMoveBefore, // move group to a certain position
		eaGroupMoveAfter, // move group to a certain position
		eaGroupPause, // pause group
		eaGroupResume, // resume (unpause) group
		eaGroupDelete, // delete group and put to history, delete already downloaded files
		eaGroupParkDelete, // delete group and put to history, keep already downloaded files
		eaGroupDupeDelete, // delete group, put to history and mark as duplicate, delete already downloaded files
		eaGroupFinalDelete, // delete group without adding to history, delete already downloaded files
		eaGroupPauseAllPars, // pause only (all) pars (does not affect other files) in group
		eaGroupPauseExtraPars, // pause only (almost all) pars in group, except main par-file (does not affect other files)
		eaGroupSetPriority, // set priority for groups
		eaGroupSetCategory, // set or change category for a group
		eaGroupApplyCategory, // set or change category for a group and reassign pp-params according to category settings
		eaGroupMerge, // merge groups
		eaGroupSetParameter, // set post-process parameter for group
		eaGroupSetName, // set group name (rename group)
		eaGroupSetDupeKey, // set duplicate key
		eaGroupSetDupeScore, // set duplicate score
		eaGroupSetDupeMode, // set duplicate mode
		eaGroupSort, // sort groups
		eaGroupSortFiles, // sort files for optimal download order
		eaPostDelete, // cancel post-processing
		eaHistoryDelete, // hide history-item
		eaHistoryFinalDelete, // delete history-item
		eaHistoryReturn, // move history-item back to download queue
		eaHistoryProcess, // move history-item back to download queue and start postprocessing
		eaHistoryRedownload, // move history-item back to download queue for full redownload
		eaHistoryRetryFailed, // move history-item back to download queue for redownload of failed articles
		eaHistorySetParameter, // set post-process parameter for history-item
		eaHistorySetDupeKey, // set duplicate key
		eaHistorySetDupeScore, // set duplicate score
		eaHistorySetDupeMode, // set duplicate mode
		eaHistorySetDupeBackup, // set duplicate backup flag
		eaHistoryMarkBad, // mark history-item as bad (and download other duplicate)
		eaHistoryMarkGood, // mark history-item as good (and push it into dup-history)
		eaHistoryMarkSuccess, // mark history-item as success (and do nothing more)
		eaHistorySetCategory, // set or change category for history-item
		eaHistorySetName // set history-item name (rename)
	};

	enum EMatchMode
	{
		mmId = 1,
		mmName,
		mmRegEx
	};

	static bool IsLoaded() { return g_Loaded; }
	static GuardedDownloadQueue Guard() { return GuardedDownloadQueue(g_DownloadQueue, &g_DownloadQueue->m_lockMutex); }
	NzbList* GetQueue() { return &m_queue; }
	HistoryList* GetHistory() { return &m_history; }

	// a reload creates the queue anew: until it is loaded again, threads
	// waiting for IsLoaded() (the post-processor's queue sanitising, for
	// one) must not take the previous queue's state for it
	static void Init(DownloadQueue* globalInstance) { g_DownloadQueue = globalInstance; g_Loaded = false; nzbget_rs_thread_count_op(4); }
	static void Final() { g_DownloadQueue = nullptr; g_Loaded = false; nzbget_rs_thread_count_op(4); }
	static void Loaded() { g_Loaded = true; nzbget_rs_thread_count_op(3); }

	DownloadQueue() = default;

private:
	NzbList m_queue;
	HistoryList m_history;
	Mutex m_lockMutex;

	inline static DownloadQueue* g_DownloadQueue = nullptr;
	inline static std::atomic<bool> g_Loaded{false};
};


class DiskState {};
extern DiskState* g_DiskState;

inline void NzbInfo::AddMessage(Message::EKind kind, const char * text, bool print)
{
	if (print)
	{
		switch (kind)
		{
		case Message::mkDetail:
			detail("%s", text);
			break;

		case Message::mkInfo:
			info("%s", text);
			break;

		case Message::mkWarning:
			warn("%s", text);
			break;

		case Message::mkError:
			error("%s", text);
			break;

		case Message::mkDebug:
			debug("%s", text);
			break;
		}
	}

	// NZB messages go to the main log (above); there are no separate NZB log files,
	// so the details Log tab shows the messages kept here (MessageCount falls back to them)
	// LogBuffer 0 keeps every message
	const int ints[2] = { (int)Util::CurrentTime(), g_Options->GetLogBuffer() };
	NM(0, (int)kind, nullptr, text ? text : "", ints);
}


#ifndef SERVERPOOL_H
#define SERVERPOOL_H

class ServerPool
{
public:
	ServerPool() { InitRsBuildEnv(); m_rs = nzbget_rs_serverpool_new(); }
	~ServerPool() { nzbget_rs_serverpool_free(m_rs); }

private:
	NzbgetRsServerPool* m_rs = nullptr;
};

extern ServerPool* g_ServerPool;

#ifndef NZBGET_TLS_CIPHER_BACKEND_H
#define NZBGET_TLS_CIPHER_BACKEND_H

#if !defined(DISABLE_TLS) && (defined(__APPLE__) || defined(__linux__))
// The OpenSSL backend lives in Rust (rust/src/tlssock.rs); a null table selects it.
namespace TlsCipherBackend
{
	inline constexpr bool Available() { return true; }
	inline std::string Response(NzbgetRsBuf value)
	{
		struct Release { NzbgetRsBuf value; ~Release() { nzbget_rs_free(value); } } release{value};
		return std::string(value.data ? value.data : "", value.len);
	}
	inline std::string Start() { return Response(nzbget_rs_tls_cipher_start(nullptr)); }
	inline std::string StartServer(int id) { return Response(nzbget_rs_tls_cipher_start_server(id)); }
	inline std::string Status(const char* id) { return Response(nzbget_rs_tls_cipher_status(id)); }
	inline std::string Cancel(const char* id) { return Response(nzbget_rs_tls_cipher_cancel(id)); }
	inline void Shutdown() { nzbget_rs_tls_cipher_shutdown(); }
}
#else
namespace TlsCipherBackend
{
	inline constexpr bool Available() { return false; }
	inline std::string Status(const char*) { return R"({"Available":false,"State":"unavailable","Message":"TLS cipher optimization requires a TLS-enabled Rust backend and a supported atomic configuration adapter."})"; }
	inline std::string Start() { return Status(nullptr); }
	inline std::string StartServer(int) { return Status(nullptr); }
	inline std::string Cancel(const char*) { return Status(nullptr); }
	inline void Shutdown() {}
}
#endif
#endif

#ifndef STATMETER_H
#define STATMETER_H

class StatMeter;

#endif

#ifndef ARTICLEWRITER_H
#define ARTICLEWRITER_H

class ArticleCache;
extern ArticleCache* g_ArticleCache;

#endif
#endif

#endif /* DOWNLOADINFO_H */

#endif /* DISKSTATE_H */

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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


#ifndef QUEUECOORDINATOR_H

#ifndef QUEUECOORDINATOR_H
#define QUEUECOORDINATOR_H

class QueueCoordinator;
extern QueueCoordinator* g_QueueCoordinator;


#ifndef HISTORYCOORDINATOR_H
#define HISTORYCOORDINATOR_H

class HistoryCoordinator {};
extern HistoryCoordinator* g_HistoryCoordinator;

class DupeCoordinator {};
extern DupeCoordinator* g_DupeCoordinator;

#endif



#ifndef SCANNER_H
#define SCANNER_H

#include <cstdarg>

#ifndef PREPOSTPROCESSOR_H
#define PREPOSTPROCESSOR_H

// Job threads live in Rust (rust/src/jobthread.rs); the queue model keeps their handles.
inline void StartPostJob(PostInfo* postInfo, int kind)
{
	if (!postInfo || (kind == 9 && postInfo->GetNzbInfo() && postInfo->GetNzbInfo()->GetSkipScriptProcessing()))
		return;
	JobThread* job = nzbget_rs_job_new_post(kind, reinterpret_cast<uintptr_t>(postInfo), 0, 0);
	postInfo->SetPostThread(job);
	if (!nzbget_rs_job_start(job))
	{
		// no worker will finish this stage: let the scheduler clean up and retry it
		postInfo->SetPostThread(nullptr);
		postInfo->SetWorking(false);
		nzbget_rs_job_free(job, 0);
	}
}

inline void StartLiveRepairJob(NzbInfo* nzbInfo)
{
	if (!nzbInfo) return;
	JobThread* job = nzbget_rs_job_new_post(10, 0, nzbInfo->GetId(), 1);
	nzbInfo->SetLiveRepairThread(job);
	if (!nzbget_rs_job_start(job)) { nzbInfo->SetLiveRepairThread(nullptr); nzbget_rs_job_free(job, 0); }
}

inline void StartDirectUnpackJob(NzbInfo* nzbInfo)
{
	JobThread* job = nzbget_rs_job_new_unpack(nzbInfo->GetId());
	nzbInfo->SetUnpackThread(job);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsRunning);
	if (!nzbget_rs_job_start(job))
	{
		nzbInfo->SetUnpackThread(nullptr);
		nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
		nzbget_rs_job_free(job, 0);
	}
}
#endif

class Scanner {};
extern Scanner* g_Scanner;
class UrlCoordinator;

#endif /* SCANNER_H */

#ifndef DUPESEARCH_H
#define DUPESEARCH_H
#include <ctime>
#endif


// Caller holds the queue throughout this synchronous borrowed-field pass.
// 0 = callback/limit refusal, 1 = complete, 2 = transient assembly/parking busy.
// No strings escape the callback, and this visitor neither copies nor normalises.
using LiveParNamespaceVisitor = bool (*)(void*, int, unsigned,
    const char*, const char*, const char*, const char*);
inline int live_par_visit_namespace(NzbInfo* nzb, LiveParNamespaceVisitor visit, void* context)
{
    if (!nzb || !visit) return 0;
    if (nzb->GetParking() || nzb->GetDeleting()) return 2;
    constexpr size_t limit = 32768;
    const size_t queued = nzb->GetFileList()->size();
    if (queued > limit || (size_t)nzb->GetCompletedFileCount() > limit - queued) return 0;
    // CompleteFileParts can clear outputFilename outside the queue, even while
    // its final downloader remains active. Do not borrow during that window.
    for (const auto& file : *nzb->GetFileList())
    {
        const int64_t total = file->GetTotalArticles();
        const int64_t completed = file->GetCompletedArticles();
        const int64_t missed = file->GetMissedArticles();
        if (total < 0 || completed < 0 || missed < 0) return 0;
        if (total > 0 && completed + missed >= total) return 2;
    }
    const auto text = [](const char* value) { return value ? value : ""; };
    for (const auto& file : *nzb->GetFileList())
    {
        // Active-count changes and mutex destruction require the queue. For
        // remaining active files, this also stabilises their output string.
        FileOutputGuard output = file->GuardOutputFile();
        const unsigned flags = (file->GetParFile() ? 1u : 0u) | (file->GetDeleted() ? 2u : 0u);
        if (!visit(context, file->GetId(), flags, text(file->GetFilename()),
            text(file->GetOrigname()), text(file->GetHash16k()), file->GetOutputFilenameC()))
            return 0;
    }
    const int completedCount = nzb->GetCompletedFileCount();
    for (int i = 0; i < completedCount; i++)
    {
        int ints[4] = {};
        const char* strs[4] = {};
        if (nzb->NF(1, i, nullptr, nullptr, nullptr, ints, nullptr, strs) != 0) return 0;
        const unsigned flags = 4u | (ints[2] ? 1u : 0u);
        if (!visit(context, ints[0], flags, text(strs[0]), text(strs[1]), text(strs[2]), ""))
            return 0;
    }
    return 1;
}

inline uintptr_t NzbgetCQueueHost(void*, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
{
	switch (op)
	{
	case 2: {
		if (p1 != 0)
		{
			const char* msg = s1 ? s1 : "";
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (a2 == 1) n->AddMessage((Message::EKind)a1, msg);
			else n->PrintMessage((Message::EKind)a1, "%s", msg);
		}
		return 0;
	}
	case 6: {
		if (a1 == 5) StartLiveRepairJob(reinterpret_cast<NzbInfo*>(p1));
		return 0;
	}
	case 7: {
		std::string r = a1 == 0 ? (s1 && *s1 ? TlsCipherBackend::StartServer(atoi(s1)) : TlsCipherBackend::Start()) :
			a1 == 1 ? TlsCipherBackend::Status(s1) : TlsCipherBackend::Cancel(s1 ? s1 : "");
		if (p1) *reinterpret_cast<NzbgetRsBuf*>(p1) = nzbget_rs_alloc_buf(r.data(), r.size());
		return 1;
	}
	case 20: {
		GuardedDownloadQueue dq = DownloadQueue::Guard();
		reinterpret_cast<void(*)(void*, uintptr_t)>(p1)(reinterpret_cast<void*>(p2),
			reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(dq)));
		return 0;
	}
	case 21: {
		auto* dq = reinterpret_cast<DownloadQueue*>(p1);
		return a1 < 0 ? dq->GetQueue()->size() : reinterpret_cast<uintptr_t>(dq->GetQueue()->at((size_t)a1).get());
	}
	case 22: {
		if (a1 == 7)
		{
			if (a2 == 0) { ints[0] = NzbInfo::LastGenId(); ints[1] = FileInfo::LastGenId(); }
			else
			{
				if (ints[0] >= 0) NzbInfo::RaiseIdFloor(ints[0]);
				if (ints[1] >= 0) FileInfo::RaiseIdFloor(ints[1]);
				NzbInfo::ResetGenId(true);
				FileInfo::ResetGenId(true);
			}
			return 0;
		}
		auto* dq = reinterpret_cast<DownloadQueue*>(p1);
		if (a1 == 0) return reinterpret_cast<uintptr_t>(dq->GetQueue()->Find(a2));
		if (a1 == 1) { dq->GetQueue()->Remove(reinterpret_cast<NzbInfo*>(p2)); return 0; }
		if (a1 == 2)
		{
			auto* up = reinterpret_cast<std::unique_ptr<NzbInfo>*>(p2);
			if (up && *up)
			{
				auto* urlInfo = reinterpret_cast<NzbInfo*>((uintptr_t)i64s[0]);
				if (urlInfo) dq->GetQueue()->insert(dq->GetQueue()->Find(urlInfo), std::move(*up));
				else dq->GetQueue()->Add(std::move(*up), a2 != 0);
			}
			return 0;
		}
		if (a1 == 3)
		{
			auto* up = new std::unique_ptr<NzbInfo>(std::make_unique<NzbInfo>());
			i64s[0] = (int64_t)reinterpret_cast<uintptr_t>(up->get());
			return reinterpret_cast<uintptr_t>(up);
		}
		if (a1 == 4)
		{
			auto* up = reinterpret_cast<std::unique_ptr<NzbInfo>*>(p2);
			dq->GetQueue()->Add(std::move(*up));
			delete up;
			return 0;
		}
		if (a1 == 5)
		{
			NzbList* q = dq->GetQueue();
			const uintptr_t* ord = reinterpret_cast<const uintptr_t*>(p2);
			if (ord && (size_t)a2 == q->size())
			{
				for (size_t i = 0; i < q->size(); i++) (void)q->at(i).release();
				for (size_t i = 0; i < (size_t)a2; i++) q->at(i).reset(reinterpret_cast<NzbInfo*>(ord[i]));
			}
		}
		if (a1 == 6) { delete reinterpret_cast<std::unique_ptr<NzbInfo>*>(p2); return 0; }
		return 0;
	}
	case 23: {
		auto* dq = reinterpret_cast<DownloadQueue*>(p1);
		if (a1 == -7)
		{
			auto* up = reinterpret_cast<std::unique_ptr<NzbInfo>*>(p2);
			if (up && *up)
			{
				auto hi = std::make_unique<HistoryInfo>(std::move(*up));
				hi->SetTime((time_t)a2);
				dq->GetHistory()->push_back(std::move(hi));
			}
			delete up;
			return 0;
		}
		if (a1 == -8)
		{
			auto d = std::make_unique<DupInfo>();
			d->SetStatus((DupInfo::EStatus)ints[2]);
			d->SetFullContentHash((uint32)ints[3]);
			d->SetFilteredContentHash((uint32)ints[4]);
			d->SetSize(i64s[0]);
			d->SetDupeScore(ints[5]);
			d->SetDupeMode((EDupeMode)ints[6]);
			d->SetName(s1 ? s1 : "");
			d->SetDupeKey(s2 ? s2 : "");
			d->SetId(ints[0]);
			auto hi = std::make_unique<HistoryInfo>(std::move(d));
			hi->SetTime((time_t)ints[1]);
			dq->GetHistory()->push_back(std::move(hi));
			return 0;
		}
		if (a1 == -1) return dq->GetHistory()->size();
		if (a1 == -2) return reinterpret_cast<uintptr_t>(dq->GetHistory()->Find(a2));
		if (a1 == -3)
		{
			auto hist = std::make_unique<HistoryInfo>(dq->GetQueue()->Remove(reinterpret_cast<NzbInfo*>(p2)));
			hist->SetTime(Util::CurrentTime());
			dq->GetHistory()->Add(std::move(hist), true);
			return 0;
		}
		if (a1 == -4)
		{
			auto* hist = reinterpret_cast<HistoryInfo*>(p2);
			NzbInfo* nzb = hist->GetNzbInfo();
			auto dup = std::make_unique<DupInfo>();
			dup->SetId(nzb->GetId()); dup->SetName(nzb->GetName()); dup->SetDupeKey(nzb->GetDupeKey());
			dup->SetDupeScore(nzb->GetDupeScore()); dup->SetDupeMode(nzb->GetDupeMode()); dup->SetSize(nzb->GetSize());
			dup->SetFullContentHash(nzb->GetFullContentHash()); dup->SetFilteredContentHash(nzb->GetFilteredContentHash());
			dup->SetStatus(static_cast<DupInfo::EStatus>(a2));
			auto newHist = std::make_unique<HistoryInfo>(std::move(dup));
			newHist->SetTime(hist->GetTime());
			(*dq->GetHistory())[dq->GetHistory()->size() - 1 - (size_t)ints[0]] = std::move(newHist);
			return 0;
		}
		if (a1 == -5) { dq->GetHistory()->erase(dq->GetHistory()->begin() + a2); return 0; }
		if (a1 == -6)
		{
			auto* hist = reinterpret_cast<HistoryInfo*>(p2);
			NzbInfo* nzb = hist->GetNzbInfo();
			hist->DiscardNzbInfo();
			dq->GetQueue()->Add(std::unique_ptr<NzbInfo>(nzb), true);
			dq->GetHistory()->erase(dq->GetHistory()->begin() + a2);
			return reinterpret_cast<uintptr_t>(nzb);
		}
		HistoryInfo* h = dq->GetHistory()->at((size_t)a1).get();
		ints[0] = (int)h->GetKind(); ints[1] = h->GetId(); strs[0] = h->GetName();
		i64s[0] = (h->GetKind() == HistoryInfo::hkNzb || h->GetKind() == HistoryInfo::hkUrl)
			? (int64_t)reinterpret_cast<uintptr_t>(h->GetNzbInfo())
			: (h->GetKind() == HistoryInfo::hkDup ? (int64_t)reinterpret_cast<uintptr_t>(h->GetDupInfo()) : 0);
		i64s[1] = (int64_t)h->GetTime();
		return reinterpret_cast<uintptr_t>(h);
	}
	case 24: {
		if (a1 == 3)
		{
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			DownloadQueue::Aspect aspect = { (DownloadQueue::EAspectAction)a2, dq,
				reinterpret_cast<NzbInfo*>(p2), reinterpret_cast<FileInfo*>((uintptr_t)i64s[0]) };
			dq->Notify(&aspect);
		}
		return 0;
	}
	case 25: {
		static DownloadQueue* s_daemonQueue = nullptr;
		if (a1 == 0) return reinterpret_cast<uintptr_t>(s_daemonQueue);
		if (a1 == 1) { delete s_daemonQueue; s_daemonQueue = new DownloadQueue(); DownloadQueue::Init(s_daemonQueue); }
		else if (a1 == 2) DownloadQueue::Loaded();
		else if (a1 == 3) { DownloadQueue::Final(); delete s_daemonQueue; s_daemonQueue = nullptr; }
		return 0;
	}
	case 30: {
		NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
		PostInfo* pi = n->GetPostInfo();
		ints[0] = n->GetId(); ints[1] = (int)n->GetKind(); ints[2] = (int)n->GetDupeMode();
		ints[3] = n->GetDupeScore(); ints[4] = (int)n->GetDupeHint(); ints[5] = (int)n->GetDeleteStatus();
		ints[6] = (int)n->GetMarkStatus(); ints[7] = (int)n->GetParStatus(); ints[8] = (int)n->GetDirectRenameStatus();
		ints[9] = (int)n->GetDirectUnpackStatus(); ints[10] = (int)n->GetDupeParDeferState();
		ints[11] = n->GetDeleting() ? 1 : 0; ints[12] = n->GetParking() ? 1 : 0; ints[13] = n->GetHealthPaused() ? 1 : 0;
		ints[14] = n->GetForcePriority() ? 1 : 0; ints[15] = n->HasExtraPriority() ? 1 : 0;
		ints[16] = n->GetManyDupeFiles() ? 1 : 0; ints[17] = n->HasDesiredServer() ? 1 : 0;
		ints[18] = n->GetDesiredServerId(); ints[19] = n->GetPriority(); ints[20] = n->GetFeedId();
		ints[21] = n->GetChanged() ? 1 : 0; ints[22] = n->GetAllFirst() ? 1 : 0; ints[23] = n->GetDeadPickProbed() ? 1 : 0;
		ints[24] = n->GetDupeNoParNoted() ? 1 : 0; ints[25] = pi ? 1 : 0;
		ints[26] = (pi && pi->GetWorking()) ? 1 : 0; ints[27] = pi ? (int)pi->GetStage() : 0;
		ints[28] = n->GetLiveRepairThread() ? 1 : 0; ints[29] = n->IsDupeSuccess() ? 1 : 0;
		ints[30] = n->CalcHealth(); ints[31] = n->CalcCriticalHealth(true); ints[32] = n->CalcCriticalHealth(false);
		ints[33] = n->IsDownloadCompleted(true) ? 1 : 0; ints[34] = n->GetFileCount(); ints[35] = n->GetActiveDownloads();
		ints[36] = n->GetPausedFileCount(); ints[37] = n->GetRemainingParCount(); ints[38] = n->GetTotalArticles();
		ints[39] = n->GetSuccessArticles(); ints[40] = n->GetFailedArticles();
		ints[41] = n->GetCurrentSuccessArticles(); ints[42] = n->GetCurrentFailedArticles();
		ints[43] = n->GetDupeRecoveredArticles(); ints[44] = n->GetDupeAttemptedArticles();
		ints[45] = n->GetDupeUnsourcedArticles(); ints[46] = n->GetDupeFailedRun();
		ints[47] = n->GetDupeFailoverChecked(); ints[48] = n->GetDownloadSec(); ints[49] = (int)n->GetFullContentHash();
		ints[50] = n->GetSkipDiskWrite() ? 1 : 0; ints[51] = n->GetWaitingPar() ? 1 : 0; ints[52] = n->GetLoadingPar() ? 1 : 0;
		ints[53] = n->GetUnpackCleanedUpDisk() ? 1 : 0; ints[54] = n->IsDownloadCompleted(false) ? 1 : 0;
		ints[55] = n->GetSkipScriptProcessing() ? 1 : 0; ints[56] = n->GetAvoidHistory() ? 1 : 0;
		ints[57] = n->GetCleanupDisk() ? 1 : 0; ints[58] = n->GetUnpackTried() ? 1 : 0; ints[59] = n->GetParFull() ? 1 : 0;
		ints[60] = n->GetPostTotalSec(); ints[61] = (int)n->GetParRenameStatus(); ints[62] = (int)n->GetRarRenameStatus();
		ints[63] = (int)n->GetUnpackStatus(); ints[64] = (int)n->GetCleanupStatus(); ints[65] = (int)n->GetMoveStatus();
		ints[66] = (int)n->GetPostDownloadRenamingStatus(); ints[67] = (int)n->GetPostUnpackRenamingStatus();
		ints[68] = n->GetUnpackThread() ? 1 : 0; ints[69] = n->HasStreamRepairJobs() ? 1 : 0;
		ints[70] = (int)n->GetUrlStatus(); ints[71] = n->GetParSec(); ints[72] = n->GetRepairSec();
		ints[73] = n->GetUnpackSec(); ints[74] = n->GetReprocess() ? 1 : 0; ints[75] = n->GetExtraParBlocks();
		ints[76] = n->GetDupeRecoveredHoles(); ints[77] = (int)n->ScriptTotalStatus();
		ints[78] = n->GetParkedFileCount(); ints[79] = n->GetDeletePaused() ? 1 : 0;
		ints[80] = n->GetAutoCategory() ? 1 : 0; ints[81] = n->GetAddUrlPaused() ? 1 : 0;
		ints[82] = n->GetMessageCount(); ints[83] = (pi && pi->GetForceParFull()) ? 1 : 0;
		ints[84] = (pi && pi->GetForceRepair()) ? 1 : 0; ints[85] = n->GetParRepaired() ? 1 : 0;
		ints[86] = n->GetLastUnpackStatus(); ints[87] = n->GetCachedMessageCount();
		i64s[0] = n->GetSize(); i64s[1] = n->GetRemainingSize(); i64s[2] = n->GetPausedSize();
		i64s[3] = n->GetSuccessSize(); i64s[4] = n->GetCurrentSuccessSize(); i64s[5] = n->GetFailedSize();
		i64s[6] = n->GetCurrentFailedSize(); i64s[7] = n->GetParSize(); i64s[8] = n->GetParSuccessSize();
		i64s[9] = n->GetParCurrentSuccessSize(); i64s[10] = n->GetParFailedSize();
		i64s[11] = n->GetParCurrentFailedSize(); i64s[12] = n->GetDownloadedSize();
		i64s[13] = (int64_t)n->GetMinTime(); i64s[14] = (int64_t)n->GetMaxTime();
		i64s[15] = (int64_t)n->GetFilteredContentHash(); i64s[16] = (int64_t)n->GetDownloadStartTime();
		i64s[17] = n->GetDupeRecoveredBytes(); i64s[18] = (int64_t)reinterpret_cast<uintptr_t>(pi);
		i64s[19] = (int64_t)reinterpret_cast<uintptr_t>(n->GetLiveRepairThread());
		i64s[20] = (int64_t)n->GetQueueScriptTime();
		if (strs)
		{
			strs[0] = n->GetName(); strs[1] = n->GetFilename(); strs[2] = n->GetDestDir(); strs[3] = n->GetFinalDir();
			strs[4] = n->GetCategory(); strs[5] = n->GetDupeKey(); strs[6] = n->GetQueuedFilename();
			strs[7] = a1 == 1 ? n->MakeTextStatus(true) : (a1 == 2 ? n->MakeTextStatus(false) : nullptr);
			strs[8] = n->GetUrl();
		}
		return 0;
	}
	case 31: {
		NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
		if (a1 == 0) return n->NC(0, 0, nullptr, s1, nullptr, nullptr, strs) ? 1 : 0;
		if (a1 == 1) { n->NC(1, 0, nullptr, s1, s2); return 0; }
		if (a1 == 2) { n->NC(2, 0, reinterpret_cast<NzbInfo*>(p2)); return 0; }
		if (a1 == 3) { static thread_local CString s_finalDir; s_finalDir = n->BuildFinalDirName(); strs[0] = s_finalDir.Str(); return 0; }
		if (a1 == 4) return n->NC(20, a2) ? 1 : 0;
		if (a1 == 5) { n->NC(21, a2); return 1; }
		if (a1 == 6) { strs[0] = n->GetHardLinkPathC(); return 0; }
		if (a1 == 7) { strs[0] = n->GetMetaNameC(); return 0; }
		if (a1 == 8) { if (a2 < 0) return n->NC(3); n->NC(4, a2, nullptr, nullptr, nullptr, nullptr, strs); return 0; }
		if (a1 == 9) { if (a2 < 0) return n->NC(40); n->NC(42, a2, nullptr, nullptr, nullptr, ints); return 0; }
		if (a1 == 10) { n->NC(10, a2, nullptr, s1 ? s1 : ""); return 0; }
		if (a1 == 11) return n->RenameCompletedFile(s1 ? s1 : "", s2 ? s2 : "") ? 1 : 0;
		if (a1 == 12) return reinterpret_cast<uintptr_t>(n->GetUnpackThread());
		if (a1 == 13) { n->EA(2, 0, s1 ? s1 : ""); return 0; }
		if (a1 == 14) { n->SetMetaName(s1 ? s1 : ""); return 0; }
		if (a1 == 15)
		{
			const int lim[2] = { (int)Util::CurrentTime(), g_Options->GetLogBuffer() };
			n->NM(3, 0, reinterpret_cast<NzbInfo*>(p2), nullptr, lim);
			return 0;
		}
		if (a1 == 16) { static thread_local CString s_niceUrl; s_niceUrl = n->MakeNiceUrlName(s1 ? s1 : "", s2 ? s2 : ""); strs[0] = s_niceUrl.Str(); return 0; }
		if (a1 == 17) { if (a2 < 0) return n->NC(11); n->NC(12, a2, nullptr, nullptr, nullptr, ints, strs); return 0; }
		if (a1 == 18) { if (a2 < 0) return n->EA(0); strs[0] = ""; n->EA(1, a2, nullptr, strs); return 0; }
		if (a1 == 19) { if (a2 == 0) n->NC(44, 0, nullptr, nullptr, nullptr, ints); else n->NC(48); return 0; }
		if (a1 == 20)
		{
			auto pushFn = reinterpret_cast<void(*)(uintptr_t, int, int, int, const char*)>(reinterpret_cast<uintptr_t>(s1));
			n->NM(4, 0, nullptr, nullptr, ints, pushFn, p2);
			return 0;
		}
		if (a1 == 21) { if (a2 < 0) return n->NC(41); n->NC(43, a2, nullptr, nullptr, nullptr, ints); return 0; }
		if (a1 == 22)
		{
			for (FileInfo* fi : n->GetFileList())
				if (fi->GetOutputInitialized() && !fi->GetOutputFilename().empty() && fi->GetActiveDownloads() > 0)
				{
					FileOutputGuard g = fi->GuardOutputFile();
					if (fi->GetOutputInitialized()) return 1;
				}
			return 0;
		}
		return 0;
	}
	case 32: {
		NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
		switch (a1)
		{
		case 0: n->SetId(a2); break;
		case 1: n->SetDeleteStatus((NzbInfo::EDeleteStatus)a2); break;
		case 2: n->SetDeletePaused(a2 != 0); break;
		case 3: n->SetMarkStatus((NzbInfo::EMarkStatus)a2); break;
		case 4: n->SetDupeHint((NzbInfo::EDupeHint)a2); break;
		case 5: n->SetDupeScore(a2); break;
		case 6: n->SetPriority(a2); break;
		case 7: n->SetFullContentHash((uint32)a2); break;
		case 8: n->SetFilteredContentHash((uint32)a2); break;
		case 9: n->SetChanged(a2 != 0); break;
		case 10: n->SetAllFirst(a2 != 0); break;
		case 11: n->SetHealthPaused(a2 != 0); break;
		case 12: n->SetDeadPickProbed(a2 != 0); break;
		case 13: n->SetDupeParDeferState((NzbInfo::EDupeParDeferState)a2); break;
		case 14: n->SetDupeNoParNoted(a2 != 0); break;
		case 15: n->SetDirectRenameStatus((NzbInfo::EDirectRenameStatus)a2); break;
		case 16: n->SetActiveDownloads(a2); break;
		case 17: n->SetCurrentSuccessArticles(a2); break;
		case 18: n->SetCurrentFailedArticles(a2); break;
		case 19: n->SetDupeRecoveredArticles(a2); break;
		case 20: n->SetDupeAttemptedArticles(a2); break;
		case 21: n->SetDupeUnsourcedArticles(a2); break;
		case 22: n->SetDupeFailedRun(a2); break;
		case 23: n->SetDupeFailoverChecked(a2); break;
		case 24: n->SetRemainingParCount(a2); break;
		case 25: n->SetWaitingPar(a2 != 0); break;
		case 26: n->SetLoadingPar(a2 != 0); break;
		case 27: n->SetDeleting(a2 != 0); break;
		case 28: n->SetParking(a2 != 0); break;
		case 29: n->SetAvoidHistory(a2 != 0); break;
		case 30: n->SetRemainingSize(i64s[0]); break;
		case 31: n->SetPausedSize(i64s[0]); break;
		case 32: n->SetCurrentSuccessSize(i64s[0]); break;
		case 33: n->SetCurrentFailedSize(i64s[0]); break;
		case 34: n->SetParCurrentSuccessSize(i64s[0]); break;
		case 35: n->SetParCurrentFailedSize(i64s[0]); break;
		case 36: n->SetDownloadedSize(i64s[0]); break;
		case 37: n->SetParSize(i64s[0]); break;
		case 38: n->SetParFailedSize(i64s[0]); break;
		case 39: n->SetDupeRecoveredBytes(i64s[0]); break;
		case 40: n->SetName(s1); break;
		case 41: n->SetFilename(s1); break;
		case 42: n->SetCategory(s1); break;
		case 43: n->SetDestDir(s1); break;
		case 44: n->SetDupeKey(s1); break;
		case 45: n->SetQueuedFilename(s1); break;
		case 46: n->SetHardLinkPath(s1 ? s1 : ""); break;
		case 47: n->SetFailedSize(i64s[0]); break;
		case 48: n->SetSuccessSize(i64s[0]); break;
		case 49: n->SetParSuccessSize(i64s[0]); break;
		case 50: n->BuildDestDirName(); break;
		case 51: n->UpdateMinMaxTime(); break;
		case 52: n->UpdateCompletedStats(reinterpret_cast<FileInfo*>(p2)); break;
		case 53: n->UpdateDeletedStats(reinterpret_cast<FileInfo*>(p2)); break;
		case 54: n->EnterPostProcess(); break;
		case 55: n->LeavePostProcess(); break;
		case 56: n->NC(13); break;
		case 57: n->SetUnpackThread(nullptr); n->SetLiveRepairThread(nullptr); break;
		case 58: n->SetLiveRepairThread(nullptr); break;
		case 59: n->SetFinalDir(s1 ? s1 : ""); break;
		case 60: n->SetCleanupDisk(a2 != 0); break;
		case 61: n->SetDupeMode((EDupeMode)a2); break;
		case 62: n->SetParStatus((NzbInfo::EParStatus)a2); break;
		case 63: n->SetUnpackStatus((NzbInfo::EPostUnpackStatus)a2); break;
		case 64: n->SetDirectUnpackStatus((NzbInfo::EDirectUnpackStatus)a2); break;
		case 65: n->SetPostTotalSec(a2); break;
		case 66: n->SetUrlStatus((NzbInfo::EUrlStatus)a2); break;
		case 67: n->SetParSec(a2); break;
		case 68: n->SetParFull(a2 != 0); break;
		case 69: n->SetRepairSec(a2); break;
		case 70: n->SetExtraParBlocks(a2); break;
		case 71: n->SetDupeRecoveredHoles(a2); break;
		case 72: n->SetParRenameStatus((NzbInfo::EPostRenameStatus)a2); break;
		case 73: n->SetUnpackCleanedUpDisk(a2 != 0); break;
		case 74: n->SetUnpackSec(a2); break;
		case 75: n->SetMoveStatus((NzbInfo::EMoveStatus)a2); break;
		case 76: n->SetCleanupStatus((NzbInfo::ECleanupStatus)a2); break;
		case 77: n->SetRarRenameStatus((NzbInfo::EPostRenameStatus)a2); break;
		case 78: n->SetPostDownloadRenamingStatus((NzbInfo::PostDownloadRenamingStatus)a2); break;
		case 79: n->SetPostUnpackRenamingStatus((NzbInfo::PostUnpackRenamingStatus)a2); break;
		case 80: n->SetUnpackThread(nullptr); break;
		case 81: n->SetKind((NzbInfo::EKind)a2); break;
		case 82: n->SetFeedId(a2); break;
		case 83: n->SetAddUrlPaused(a2 != 0); break;
		case 84: n->SetAutoCategory(a2 != 0); break;
		case 85: n->SetDesiredServerId(a2); break;
		case 86: n->SetSkipScriptProcessing(a2 != 0); break;
		case 87: n->SetSkipDiskWrite(a2 != 0); break;
		case 88: n->SetExtraPriority(a2); break;
		case 89: n->SetManyDupeFiles(a2 != 0); break;
		case 90: n->SetParkedFileCount(a2); break;
		case 91: n->SetReprocess(a2 != 0); break;
		case 92: n->SetParRepaired(a2 != 0); break;
		case 93: n->SetUnpackTried(a2 != 0); break;
		case 94: n->SetLastUnpackStatus(a2); break;
		case 95: n->EA(3); break;
		case 96: n->SetUrl(s1); break;
		case 97: n->GetFileList()->clear(); break;
		case 98: n->UpdateCurrentStats(); break;
		case 99: n->SetDownloadSec(a2); break;
		case 100: n->SetFailedArticles(a2); break;
		case 101: n->SetSuccessArticles(a2); break;
		case 102: n->SetSize(i64s[0]); break;
		case 103: n->SetMinTime((time_t)i64s[0]); break;
		case 104: n->SetMaxTime((time_t)i64s[0]); break;
		case 105: n->SetQueueScriptTime((time_t)i64s[0]); break;
		case 106: n->SetFileCount(a2); break;
		case 107: n->SetMessageCount(a2); break;
		case 108: n->SetTotalArticles(a2); break;
		case 109: if (n->GetPostInfo()) n->GetPostInfo()->SetStage((PostInfo::EStage)a2); break;
		case 110: if (n->GetPostInfo()) { n->GetPostInfo()->SetForceParFull((a2 & 1) != 0); n->GetPostInfo()->SetForceRepair((a2 & 2) != 0); } break;
		case 111: n->NC(5); break;
		case 112: n->NF(27); break;
		case 113: n->NF(7); break;
		default: break;
		}
		return 0;
	}
	case 33: {
		NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
		n->SetFileCount(ints[0]); n->SetActiveDownloads(ints[1]);
		n->SetFullContentHash((uint32)ints[2]); n->SetFilteredContentHash((uint32)ints[3]);
		n->SetSize(i64s[0]); n->SetRemainingSize(i64s[1]);
		n->SetPausedFileCount(ints[4]); n->SetPausedSize(i64s[2]);
		n->SetSuccessSize(i64s[3]); n->SetCurrentSuccessSize(i64s[4]);
		n->SetFailedSize(i64s[5]); n->SetCurrentFailedSize(i64s[6]);
		n->SetParSize(i64s[7]); n->SetParSuccessSize(i64s[8]);
		n->SetParCurrentSuccessSize(i64s[9]); n->SetParFailedSize(i64s[10]);
		n->SetParCurrentFailedSize(i64s[11]); n->SetRemainingParCount(ints[5]);
		n->SetTotalArticles(ints[6]); n->SetSuccessArticles(ints[7]);
		n->SetFailedArticles(ints[8]); n->SetCurrentSuccessArticles(ints[9]);
		n->SetCurrentFailedArticles(ints[10]); n->SetDownloadSec(ints[11]);
		n->SetMinTime((time_t)i64s[12]); n->SetMaxTime((time_t)i64s[13]);
		n->SetDownloadedSize(i64s[14]); n->SetDownloadStartTime((time_t)i64s[15]);
		return 0;
	}
	case 34: {
		if (a1 == 0) return reinterpret_cast<NzbInfo*>(p1)->GetFileList()->size();
		if (a1 == 1) return reinterpret_cast<uintptr_t>(reinterpret_cast<NzbInfo*>(p1)->GetFileList()->at((size_t)a2).get());
		if (a1 == 2) { reinterpret_cast<NzbInfo*>(p1)->GetFileList()->Remove(reinterpret_cast<FileInfo*>(p2)); return 0; }
		if (a1 == 3)
		{
			NzbInfo* src = reinterpret_cast<NzbInfo*>(p1);
			NzbInfo* dst = reinterpret_cast<NzbInfo*>((uintptr_t)i64s[0]);
			FileInfo* f = reinterpret_cast<FileInfo*>(p2);
			dst->GetFileList()->Add(src->GetFileList()->Remove(f));
			f->SetNzbInfo(dst);
			src->AddFileStats(f, 1, 2);
			dst->AddFileStats(f, 1, 1);
			return 0;
		}
		if (a1 == 4)
		{
			NzbInfo* dst = reinterpret_cast<NzbInfo*>(p1);
			NzbInfo* src = reinterpret_cast<NzbInfo*>(p2);
			for (std::unique_ptr<FileInfo>& fi : *src->GetFileList()) { fi->SetNzbInfo(dst); dst->GetFileList()->Add(std::move(fi)); }
			src->GetFileList()->clear();
			int lists[2] = { 0, 0 };
			dst->NC(49, 1, src, nullptr, nullptr, lists);
			lists[0] = lists[1] = 1;
			dst->NC(49, 1, src, nullptr, nullptr, lists);
			dst->NF(8, 0, src);
			return 0;
		}
		if (a1 == 5)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (a2 < 0) return n->NF(0);
			n->NF(1, a2, nullptr, nullptr, nullptr, ints, nullptr, strs);
			return 0;
		}
		if (a1 == 6)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			FileInfo* f = reinterpret_cast<FileInfo*>(p2);
			n->AddCompletedFile(f->GetId(), s1, f->GetOrigname(), (CompletedFile::EStatus)a2,
				(uint32)ints[0], f->GetParFile(), f->GetHash16k(), f->GetParSetId());
			return 0;
		}
		if (a1 == 7)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			n->NF(3, a2, nullptr, s1, s2);
			return 0;
		}
		if (a1 == 8)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			n->NF(23, a2, nullptr, s1, nullptr, ints, i64s, nullptr, reinterpret_cast<const int64_t*>(p2));
			return 0;
		}
		if (a1 == 9)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			FileInfo* f = reinterpret_cast<FileInfo*>(p2);
			auto* dq = reinterpret_cast<DownloadQueue*>((uintptr_t)i64s[0]);
			std::unique_ptr<FileInfo> srcFi = n->GetFileList()->Remove(f);
			DownloadQueue::Aspect aspect = { (DownloadQueue::EAspectAction)a2, dq, n, f };
			dq->Notify(&aspect);
			srcFi.reset();
			return 0;
		}
		if (a1 == 10)
		{
			FileList* fl = reinterpret_cast<NzbInfo*>(p1)->GetFileList();
			const uintptr_t* ord = reinterpret_cast<const uintptr_t*>(p2);
			if (ord && (size_t)a2 == fl->size())
			{
				for (size_t i = 0; i < fl->size(); i++) (void)fl->at(i).release();
				for (size_t i = 0; i < (size_t)a2; i++) fl->at(i).reset(reinterpret_cast<FileInfo*>(ord[i]));
			}
			return 0;
		}
		if (a1 == 11)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (a2 < 0) return n->NF(20);
			n->NF(21, a2, nullptr, nullptr, nullptr, ints, i64s, strs);
			return 0;
		}
		if (a1 == 12)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			return n->NF(5, a2, nullptr, nullptr, nullptr, ints);
		}
		if (a1 == 13)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (ints[0] == 1) return n->NF(24, a2);
			if (ints[0] == 2)
			{
				n->NF(25, a2);
				if (ints[1] != 0) n->SetChanged(true);
				return 0;
			}
			if (ints[0] == 3)
			{
				n->NF(26, a2, nullptr, nullptr, nullptr, ints, i64s, nullptr, reinterpret_cast<const int64_t*>(p2));
				return 0;
			}
			n->NF(22, a2, nullptr, nullptr, nullptr, ints, i64s, strs);
			return 0;
		}
		if (a1 == 14)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			auto fi = std::make_unique<FileInfo>(ints[0]);
			fi->SetNzbInfo(n); fi->SetSubject(s1 ? s1 : ""); fi->SetFilename(s2 ? s2 : "");
			fi->SetTime((time_t)ints[1]); fi->SetSize(i64s[0]); fi->SetRemainingSize(i64s[1]); fi->SetMissedSize(i64s[2]);
			fi->SetTotalArticles(ints[2]); fi->SetMissedArticles(ints[3]); fi->SetParFile(ints[4] != 0);
			const auto* gPtrs = reinterpret_cast<const char* const*>(strs[0]);
			for (int g = 0; g < ints[5]; g++) fi->AddGroup(gPtrs[g] ? gPtrs[g] : "");
			const auto* aParts = reinterpret_cast<const int*>((uintptr_t)i64s[3]);
			const auto* aSizes = reinterpret_cast<const int64_t*>((uintptr_t)i64s[4]);
			const auto* aMids = reinterpret_cast<const char* const*>(strs[1]);
			int64_t artArgs[2] = { (int64_t)(uintptr_t)aParts, (int64_t)(uintptr_t)aSizes };
			ArtOp(fi->GetArticleList(), 0, 105, ints[6], 0, nullptr, nullptr, artArgs, const_cast<const char**>(aMids));
			FileInfo* raw = fi.get();
			n->GetFileList()->Add(std::move(fi));
			return reinterpret_cast<uintptr_t>(raw);
		}
		if (a1 == 15)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			n->NF(7); n->NF(27);
			n->NC(46); n->NC(47);
			n->MoveFileList(reinterpret_cast<NzbInfo*>(p2));
			return 0;
		}
		if (a1 == 16) { reinterpret_cast<NzbInfo*>(p1)->NF(6, a2); return 0; }
		if (a1 == 17) { reinterpret_cast<NzbInfo*>(p1)->GetFileList()->Add(std::unique_ptr<FileInfo>(reinterpret_cast<FileInfo*>(p2)), false); return 0; }
		if (a1 == 18)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			n->AddCompletedFile(ints[0], strs[0], strs[1], (CompletedFile::EStatus)ints[1],
				(uint32)ints[2], ints[3] != 0, strs[2], strs[3]);
			return 0;
		}
		if (a1 == 19)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			auto fi = std::make_unique<FileInfo>();
			fi->SetId(ints[0]);
			fi->SetPaused(ints[1] != 0);
			if (a2 != 0) fi->SetTime((time_t)ints[2]);
			fi->SetExtraPriority(ints[3] != 0);
			fi->SetNzbInfo(n);
			n->GetFileList()->Add(std::move(fi));
			return 0;
		}
		return 0;
	}
	case 35: {
		if (a1 == 0)
		{
			int all[6];
			reinterpret_cast<DupInfo*>(p1)->Read(all, i64s, strs);
			for (int k = 0; k < 5; k++) ints[k] = all[k];
			return 0;
		}
		if (a1 == 1) { reinterpret_cast<DupInfo*>(p1)->SetStatus((DupInfo::EStatus)a2); return 0; }
		if (a1 == 2)
		{
			HistoryInfo* h = reinterpret_cast<HistoryInfo*>(p1);
			ints[0] = (int)h->GetKind(); ints[1] = h->GetId(); strs[0] = h->GetName();
			if (h->GetKind() == HistoryInfo::hkNzb || h->GetKind() == HistoryInfo::hkUrl) return reinterpret_cast<uintptr_t>(h->GetNzbInfo());
			if (h->GetKind() == HistoryInfo::hkDup) return reinterpret_cast<uintptr_t>(h->GetDupInfo());
		}
		if (a1 == 3) { reinterpret_cast<DupInfo*>(p1)->SetName(s1 ? s1 : ""); return 0; }
		if (a1 == 4) { reinterpret_cast<DupInfo*>(p1)->SetDupeKey(s1 ? s1 : ""); return 0; }
		if (a1 == 5) { reinterpret_cast<DupInfo*>(p1)->SetDupeScore(a2); return 0; }
		if (a1 == 6) { reinterpret_cast<DupInfo*>(p1)->SetDupeMode((EDupeMode)a2); return 0; }
		return 0;
	}
	case 40: {
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		ints[0] = f->GetId(); ints[1] = f->GetPaused() ? 1 : 0; ints[2] = f->GetDeleted() ? 1 : 0;
		ints[3] = f->GetDupeDeleted() ? 1 : 0; ints[4] = f->GetParFile() ? 1 : 0;
		ints[5] = f->GetFilenameConfirmed() ? 1 : 0; ints[6] = (int)f->GetFirstContent();
		ints[7] = f->GetExtraPriority() ? 1 : 0; ints[8] = f->GetForceDirectWrite() ? 1 : 0;
		ints[9] = f->GetOutputInitialized() ? 1 : 0; ints[10] = f->IsHardLinked() ? 1 : 0;
		ints[11] = (int)f->GetPartialState(); ints[12] = f->GetPartialChanged() ? 1 : 0;
		ints[13] = f->GetDupeCutover() ? 1 : 0; ints[14] = f->GetDupeNoSourceLogged() ? 1 : 0;
		ints[15] = f->GetDupeLeadDonorId(); ints[16] = f->GetDupeLeadFailures();
		ints[17] = f->GetDupeLeadSwitches(); ints[18] = f->GetDupeRecoveredArticles();
		ints[19] = f->GetDupeAttemptedArticles(); ints[20] = f->GetTotalArticles();
		ints[21] = f->GetMissedArticles(); ints[22] = f->GetCompletedArticles();
		ints[23] = f->GetSuccessArticles(); ints[24] = f->GetFailedArticles();
		ints[25] = f->GetActiveDownloads(); ints[26] = f->GetCachedArticles(); ints[27] = (int)f->GetCrc();
		i64s[0] = f->GetSize(); i64s[1] = f->GetRemainingSize(); i64s[2] = f->GetMissedSize();
		i64s[3] = f->GetSuccessSize(); i64s[4] = f->GetFailedSize(); i64s[5] = f->GetDecodedFileSize();
		i64s[6] = (int64_t)reinterpret_cast<uintptr_t>(f->GetNzbInfo()); i64s[7] = (int64_t)f->GetTime();
		if (strs)
		{
			static thread_local std::string s_outFn;
			if (f->GetActiveDownloads() > 0) { FileOutputGuard og = f->GuardOutputFile(); s_outFn = f->GetOutputFilename(); strs[2] = s_outFn.c_str(); }
			else strs[2] = f->GetOutputFilenameC();
			strs[0] = f->GetFilename(); strs[1] = f->GetSubject(); strs[3] = f->GetHardLinkPathC();
			strs[4] = f->GetOrigname(); strs[5] = f->GetHash16k(); strs[6] = f->GetParSetId();
		}
		return ArtOp(f->GetArticleList(), 0, 100);
	}
	case 41: {
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		if (a1 == 0)
		{
			if (a2 < 0) return f->GroupCount();
			strs[0] = f->GroupAt((size_t)a2);
			return 0;
		}
		if (a1 == 1)
		{
			if (a2 < 0) return f->StatCount();
			f->StatAt((size_t)a2, ints);
			return 0;
		}
		if (a1 == 2) { f->StatOp(ints[0], ints[1], ints[2], ServerStatList::soSet); return 0; }
		if (a1 == 3) { f->AddGroup(s1 ? s1 : ""); return 0; }
		return 0;
	}
	case 42: {
		if (a1 == 56) { FileInfo tmp; return (uintptr_t)tmp.GetId(); }
		if (a1 == 60)
		{
			if (a2 != 0) { auto* nf = new FileInfo(); nf->SetId((int)p2); return reinterpret_cast<uintptr_t>(nf); }
			return reinterpret_cast<uintptr_t>(new FileInfo((int)p2));
		}
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		switch (a1)
		{
		case 0: f->SetPaused(a2 != 0); break;
		case 1: f->SetDeleted(a2 != 0); break;
		case 2: f->SetDupeDeleted(a2 != 0); break;
		case 3: f->SetFilenameConfirmed(a2 != 0); break;
		case 4: f->SetFirstContent((FileInfo::EFirstContent)a2); break;
		case 5: f->SetExtraPriority(a2 != 0); break;
		case 6: f->SetForceDirectWrite(a2 != 0); break;
		case 7: f->SetOutputInitialized(a2 != 0); break;
		case 8: f->SetPartialState((FileInfo::EPartialState)a2); break;
		case 9: f->SetPartialChanged(a2 != 0); break;
		case 10: f->SetDupeCutover(a2 != 0); break;
		case 11: f->SetDupeNoSourceLogged(a2 != 0); break;
		case 12: f->SetDupeLeadDonorId(a2); break;
		case 13: f->SetDupeLeadFailures(a2); break;
		case 14: f->SetDupeLeadSwitches(a2); break;
		case 15: f->SetDupeRecoveredArticles(a2); break;
		case 16: f->SetDupeAttemptedArticles(a2); break;
		case 17: f->SetCompletedArticles(a2); break;
		case 18: f->SetSuccessArticles(a2); break;
		case 19: f->SetFailedArticles(a2); break;
		case 20: f->SetActiveDownloads(a2); break;
		case 21: f->SetCachedArticles(a2); break;
		case 22: f->SetParFile(a2 != 0); break;
		case 30: f->SetRemainingSize(i64s[0]); break;
		case 31: f->SetSuccessSize(i64s[0]); break;
		case 32: f->SetFailedSize(i64s[0]); break;
		case 33: f->SetDecodedFileSize(i64s[0]); break;
		case 40: f->SetFilename(s1); break;
		case 41: {
			std::optional<FileOutputGuard> og;
			if (f->GetActiveDownloads() > 0) og.emplace(f->GuardOutputFile());
			f->SetOutputFilename(s1);
			break;
		}
		case 42: f->SetOrigname(s1); break;
		case 43: f->SetHash16k(s1); break;
		case 44: f->SetParSetId(s1); break;
		case 45: f->SetHardLinkPath(s1 ? s1 : ""); break;
		case 50: f->MakeValidFilename(); break;
		case 51: ArtOp(f->GetArticleList(), 0, 101); break;
		case 52: ArtOp(f->GetArticleList(), 0, 102); break;
		case 53: reinterpret_cast<NzbInfo*>(p2)->AddFileStats(f, 1, 2); f->ClearStats(); break;
		case 54: ArtOp(f->GetArticleList(), 0, 103); break;
		case 55: {
			std::optional<FileOutputGuard> og;
			if (f->GetActiveDownloads() > 0) og.emplace(f->GuardOutputFile());
			ArtOp(f->GetArticleList(), 0, 104, 0, p2);
			break;
		}
		case 57: delete f; break;
		case 58: f->SetNzbInfo(reinterpret_cast<NzbInfo*>(p2)); break;
		case 59: reinterpret_cast<NzbInfo*>(p2)->AddFileStats(f, 0, 2); break;
		case 61: f->SetSubject(s1); break;
		case 62: f->SetTime((time_t)i64s[0]); break;
		case 63: f->SetSize(i64s[0]); break;
		case 64: f->SetMissedSize(i64s[0]); break;
		case 65: f->SetTotalArticles(a2); break;
		case 66: f->SetMissedArticles(a2); break;
		default: break;
		}
		return 0;
	}
	case 43: {
		if (a1 == 15)
		{
			auto visit = reinterpret_cast<LiveParNamespaceVisitor>(reinterpret_cast<uintptr_t>(s1));
			return (uintptr_t)live_par_visit_namespace(reinterpret_cast<NzbInfo*>(p1), visit, reinterpret_cast<void*>(p2));
		}
		// file-based ops take the FileInfo in p1 (and the article in p2 for 8 and 13)
		const bool fileOp = a1 == 0 || a1 == 7 || a1 == 8 || a1 == 10 || a1 == 11 || a1 == 12 || a1 == 13;
		RsArticleList* list = fileOp ? reinterpret_cast<FileInfo*>(p1)->GetArticleList() : nullptr;
		uintptr_t article = fileOp ? ((a1 == 8 || a1 == 13) ? p2 : 0) : p1;
		return ArtOp(list, article, a1, a2, p2, s1, ints, i64s, strs);
	}
	case 44: {
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		if (a1 == 1)
		{
			const char* data = reinterpret_cast<const char*>(ArtOp(nullptr, p2, 110));
			if (!f->GetFlushLocked() && data && a2 > 0)
			{
				memcpy(reinterpret_cast<void*>((uintptr_t)i64s[0]), data, (size_t)a2);
				return 1;
			}
			return 0;
		}
		if (a1 == 2)
		{
			const char* result = reinterpret_cast<const char*>(ArtOp(nullptr, p2, 114));
			if (ArtOp(nullptr, p2, 117) > 0 && result) FileSystem::DeleteFile(result);
			if (!ArtOp(nullptr, p2, 110) || f->GetFlushLocked()) return 0;
			ArtOp(nullptr, p2, 111);
			f->SetCachedArticles(f->GetCachedArticles() - 1);
			return 1;
		}
		if (a1 == 3)
		{
			if (a2 != 0) { if (f->GetFlushLocked()) return 0; f->SetFlushLocked(true); return 1; }
			f->SetFlushLocked(false);
			return 1;
		}
		if (a1 == 4)
		{
			static thread_local std::string s_outFn;
			bool outInit = false;
			{ FileOutputGuard og = f->GuardOutputFile(); outInit = f->GetOutputInitialized(); s_outFn = f->GetOutputFilename(); }
			ints[0] = ((g_Options->GetDirectWrite() || f->GetForceDirectWrite()) && outInit) ? 1 : 0;
			ints[1] = ((f->GetNzbInfo() && f->GetNzbInfo()->GetSkipDiskWrite()) || g_Options->GetSkipWrite()) ? 1 : 0;
			i64s[0] = (int64_t)s_outFn.size();
			i64s[1] = (int64_t)reinterpret_cast<uintptr_t>(f->GetNzbInfo());
			strs[0] = s_outFn.data();
			return 1;
		}
		if (a1 == 5)
		{
			if (f->GetFlushLocked()) return 0;
			f->SetFlushLocked(true);
			auto pushSeg = reinterpret_cast<void(*)(uintptr_t, uintptr_t, int64_t, int, const char*, const char*)>(reinterpret_cast<uintptr_t>(s1));
			RsArticleList* list = f->GetArticleList();
			for (uintptr_t i = 0, n = ArtOp(list, 0, 100); i < n; i++)
			{
				uintptr_t pa = ArtOp(list, 0, 106, (int)i);
				int ai[5]; int64_t ao[1]; const char* as[2];
				ArtOp(nullptr, pa, 120, 0, 0, nullptr, ai, ao, as);
				if (as[0]) pushSeg(p2, pa, ao[0], ai[1], as[0], as[1]);
			}
			return 1;
		}
		if (a1 == 6)
		{
			if (a2 == 0) return (f->GetDeleted() && !f->GetNzbInfo()->GetParking()) ? 1 : 0;
			ArtOp(nullptr, p2, 111);
			return 0;
		}
		if (a1 == 7)
		{
			f->SetCachedArticles(f->GetCachedArticles() - a2);
			f->SetFlushLocked(false);
			ints[0] = ((f->GetNzbInfo() && f->GetNzbInfo()->GetSkipDiskWrite()) || g_Options->GetSkipWrite()) ? 1 : 0;
			return 0;
		}
		return 0;
	}
	case 45: {
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		if (a1 == 0)
		{
			std::optional<FileOutputGuard> og;
			if (a2 != 0 || f->GetActiveDownloads() > 0) og.emplace(f->GuardOutputFile());
			reinterpret_cast<void(*)(void*)>(p2)(reinterpret_cast<void*>((uintptr_t)i64s[0]));
			return 0;
		}
		if (a1 == 1)
		{
			ints[0] = f->GetId(); ints[1] = (int)ArtOp(nullptr, p2, 118);
			ints[2] = f->GetForceDirectWrite() ? 1 : 0; ints[3] = f->GetOutputInitialized() ? 1 : 0;
			strs[0] = f->GetNzbInfo()->GetDestDir(); strs[1] = f->GetOutputFilenameC(); strs[2] = f->GetFilename();
			return 0;
		}
		if (a1 == 2)
		{
			const char* result = reinterpret_cast<const char*>(ArtOp(nullptr, p2, 115, 0, 0, s1));
			if (s2) f->SetOutputFilename(s2);
			strs[0] = result;
			return 0;
		}
		if (a1 == 3) { ArtOp(nullptr, p2, 116, a2); return 0; }
		if (a1 == 4)
		{
			ints[0] = (int)ArtOp(nullptr, p2, 117);
			int64_t range[2];
			ArtOp(nullptr, p2, 119, 0, 0, nullptr, nullptr, range);
			i64s[0] = f->GetDecodedFileSize(); i64s[1] = range[0]; i64s[2] = range[1];
			return 0;
		}
		if (a1 == 5)
		{
			bool outInit = false, isOwn = false;
			{
				FileOutputGuard g = f->GuardOutputFile();
				outInit = f->GetOutputInitialized();
				if (!g_Options->GetDirectWrite() && !f->GetForceDirectWrite()) f->SetOutputInitialized(true);
			}
			if (g_Options->GetDirectWrite() || f->GetForceDirectWrite())
			{
				FileOutputGuard g = f->GuardOutputFile();
				isOwn = f->GetForceDirectWrite() || (s1 && !f->GetOutputFilename().empty() && FileSystem::SameFilename(f->GetOutputFilename().c_str(), s1));
			}
			ints[0] = outInit ? 1 : 0; ints[1] = isOwn ? 1 : 0;
			return 0;
		}
		if (a1 == 6) { f->SetOutputInitialized(true); return 0; }
		if (a1 == 7)
		{
			if (a2 == 0)
			{
				ArtOp(nullptr, p2, 112, 0, 0, nullptr, ints, i64s);
				f->SetCachedArticles(f->GetCachedArticles() + 1);
			}
			else
				ArtOp(nullptr, p2, 113, 0, 0, nullptr, ints, i64s);
			return 0;
		}
		if (a1 == 8) { f->SetOutputFilename(s1 ? s1 : ""); return 0; }
		if (a1 == 9)
		{
			if (a2 == 0)
			{
				NzbInfo* nzb = f->GetNzbInfo();
				static thread_local std::string s_nName, s_nDest, s_fName, s_fOut;
				{ GuardedDownloadQueue g = DownloadQueue::Guard(); s_nName = nzb->GetName(); s_nDest = nzb->GetDestDir(); s_fName = f->GetFilename(); }
				s_fOut = f->GetOutputFilename();
				ints[0] = g_Options->GetDirectWrite() ? 1 : 0; ints[1] = f->GetForceDirectWrite() ? 1 : 0;
				ints[2] = f->GetOutputInitialized() ? 1 : 0; ints[3] = f->GetCachedArticles();
				ints[4] = f->GetSuccessArticles(); ints[5] = f->GetFailedArticles();
				ints[6] = f->GetMissedArticles(); ints[7] = f->GetTotalArticles();
				ints[8] = f->IsHardLinked() ? 1 : 0; ints[9] = Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0;
				ints[10] = g_Options->GetRawArticle() ? 1 : 0; ints[11] = (nzb->GetSkipDiskWrite() || g_Options->GetSkipWrite()) ? 1 : 0;
				ints[12] = g_Options->GetWriteBuffer(); ints[15] = (int)ArtOp(f->GetArticleList(), 0, 100);
				strs[0] = s_nName.c_str(); strs[1] = s_nDest.c_str(); strs[2] = s_fName.c_str(); strs[3] = s_fOut.c_str();
				return reinterpret_cast<uintptr_t>(nzb);
			}
			if (a2 == 2) { GuardedDownloadQueue g = DownloadQueue::Guard(); f->SetCrc(0); f->SetOutputFilename(""); return 0; }
			if (a2 == 4)
			{
				uintptr_t pa = ArtOp(f->GetArticleList(), 0, 106, (int)p2);
				return pa ? ArtOp(nullptr, pa, 120, 0, 0, nullptr, ints, i64s, strs) : 0;
			}
			if (a2 == 5) { ArtOp(nullptr, ArtOp(f->GetArticleList(), 0, 106, (int)p2), 111); return 0; }
			if (a2 == 6) { f->SetFailedArticles(f->GetFailedArticles() + (int)p2); f->SetSuccessArticles(f->GetSuccessArticles() - (int)p2); return 0; }
			if (a2 == 8)
			{
				NzbInfo* nzb = f->GetNzbInfo();
				f->SetCrc((uint32)p2); f->SetOutputFilename(s1 ? s1 : "");
				ints[0] = f->IsHardLinked() ? 1 : 0; ints[1] = Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0;
				strs[0] = f->GetFilename(); strs[1] = f->GetOutputFilenameC(); strs[2] = nzb->GetDestDir();
				return reinterpret_cast<uintptr_t>(nzb);
			}
			return 0;
		}
		if (a1 == 10)
		{
			f->StatOp(ints[0], ints[1], ints[2], ServerStatList::soAdd);
			f->GetNzbInfo()->NC(45, 1, nullptr, nullptr, nullptr, ints);
			return 0;
		}
		return 0;
	}
	case 60: {
		PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
		if (!pi) return 0;
		if (a1 == 0)
		{
			JobThread* pt = pi->GetPostThread();
			ints[0] = (int)pi->GetStage(); ints[1] = pi->GetWorking() ? 1 : 0; ints[2] = pi->GetDeleted() ? 1 : 0;
			ints[3] = pi->GetRequestParCheck() ? 1 : 0; ints[4] = pi->GetForceParFull() ? 1 : 0; ints[5] = pi->GetForceRepair() ? 1 : 0;
			ints[6] = pi->GetStreamRepairDone() ? 1 : 0; ints[7] = pi->GetStreamRepairRecovered() ? 1 : 0; ints[8] = pi->GetParCannotCover() ? 1 : 0;
			ints[9] = pi->GetNeedParCheck() ? 1 : 0; ints[10] = pi->GetNeedUnpack() ? 1 : 0;
			ints[11] = pt ? 1 : 0; ints[12] = (pt && nzbget_rs_job_op(pt, 4, 0, 0, 0)) ? 1 : 0;
			ints[13] = pi->GetUnpackTried() ? 1 : 0; ints[14] = pi->GetParRepaired() ? 1 : 0;
			ints[15] = pi->GetPassListTried() ? 1 : 0; ints[16] = pi->GetLastUnpackStatus();
			ints[17] = pi->GetExtractedArchiveCount();
			ints[18] = pi->GetParredFileCount();
			ints[19] = pi->GetStageProgress(); ints[20] = pi->GetFileProgress();
			i64s[0] = (int64_t)pi->GetStartTime(); i64s[1] = (int64_t)reinterpret_cast<uintptr_t>(pi->GetNzbInfo());
			i64s[2] = (int64_t)pi->GetStageTime();
			if (strs) strs[0] = pi->GetProgressLabel();
			return 0;
		}
		if (a1 == 1)
		{
			int v = (int)p2;
			if (a2 == 0) pi->SetStage((PostInfo::EStage)v);
			else if (a2 == 1) pi->SetWorking(v != 0);
			else if (a2 == 2) pi->SetDeleted(v != 0);
			else if (a2 == 3) pi->SetRequestParCheck(v != 0);
			else if (a2 == 4) pi->SetForceParFull(v != 0);
			else if (a2 == 5) pi->SetParCannotCover(v != 0);
			else if (a2 == 6) pi->SetNeedParCheck(v != 0);
			else if (a2 == 7) pi->SetNeedUnpack(v != 0);
			else if (a2 == 8) pi->SetForceRepair(v != 0);
			else if (a2 == 9) pi->SetParRepaired(v != 0);
			else if (a2 == 10) pi->SetUnpackTried(v != 0);
			else if (a2 == 11) pi->SetPassListTried(v != 0);
			else if (a2 == 12) pi->SetLastUnpackStatus(v);
			else if (a2 == 13) pi->SetFileProgress(v);
			else if (a2 == 14) pi->SetStageProgress(v);
			else if (a2 == 15) pi->SetStreamRepairDone(v != 0);
			else if (a2 == 16) pi->SetStreamRepairRecovered(v != 0);
			return 0;
		}
		if (a1 == 2) { pi->SetStartTime((time_t)i64s[0]); return 0; }
		if (a1 == 3) { pi->SetStageTime((time_t)i64s[0]); pi->SetStageProgress(0); pi->SetFileProgress(0); pi->SetProgressLabel(""); return 0; }
		if (a1 == 4) { pi->SetStageTime((time_t)i64s[0]); return 0; }
		if (a1 == 5) { pi->SetProgressLabel(s1 ? s1 : ""); return 0; }
		if (a1 == 6) { if (a2 < 0) pi->AddParredFile(s1); else strs[0] = pi->GetParredFile(a2); return 0; }
		if (a1 == 7) { if (a2 < 0) pi->ClearExtractedArchives(); else strs[0] = pi->GetExtractedArchive(a2); return 0; }
		return 0;
	}
	case 62: {
		if (a1 == 0) { PostInfo* pi = reinterpret_cast<PostInfo*>(p1); if (pi && pi->GetPostThread()) nzbget_rs_job_op(pi->GetPostThread(), 0, 0, 0, 0); return 0; }
		if (a1 == 1 || a1 == 2)
		{
			PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
			if (pi && pi->GetPostThread()) { JobThread* t = pi->GetPostThread(); pi->SetPostThread(nullptr); nzbget_rs_job_free(t, a1 == 2 ? 1 : 0); }
			return 0;
		}
		if (a1 == 3)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (!n || !n->GetUnpackThread()) return 0;
			JobThread* du = n->GetUnpackThread();
			return (a2 != 0 || (nzbget_rs_job_op(du, 15, 0, 0, 0) && !nzbget_rs_job_op(du, 16, 0, 0, 0))) ? 1 : 0;
		}
		if (a1 >= 4 && a1 <= 7)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p2);
			if (!n || !n->GetUnpackThread()) return 0;
			// direct-unpack ops: 4 stop, 5 collection downloaded, 6 deleted, 7 file downloaded
			static const int du_op[] = {11, 13, 14, 12};
			uintptr_t arg = a1 == 7 ? (uintptr_t)i64s[0] : p2;
			nzbget_rs_job_op(n->GetUnpackThread(), du_op[a1 - 4], 0, p1, arg);
			return 0;
		}
		if (a1 == 8) { StartDirectUnpackJob(reinterpret_cast<NzbInfo*>(p1)); return 0; }
		if (a1 == 9) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p1); if (n && n->GetLiveRepairThread()) nzbget_rs_job_op(n->GetLiveRepairThread(), 0, 0, 0, 0); return 0; }
		if (a1 == 10) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p1); if (n && n->GetLiveRepairThread()) { JobThread* t = n->GetLiveRepairThread(); n->SetLiveRepairThread(nullptr); nzbget_rs_job_op(t, 0, 0, 0, 0); } return 0; }
		if (a1 == 11)
		{
#ifndef DISABLE_PARCHECK
			PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
			if (pi && pi->GetPostThread()) return nzbget_rs_job_op(pi->GetPostThread(), 5, a2 != 0 ? 1 : 0, p2, 0) ? 1 : 0;
#endif
			return 0;
		}
		if (a1 == 12)
		{
			PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
#ifdef DISABLE_PARCHECK
			if (a2 == 3) return 0;
#endif
			StartPostJob(pi, a2);
			return 0;
		}
		return 0;
	}
	default: return 0;
	}
}

#endif

class FeedCoordinator;
namespace SystemHealth { class Service; }
class Maintenance;
#include <system_error>

#endif
