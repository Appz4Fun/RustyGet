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

#ifndef OBSERVER_H
#define OBSERVER_H

class Subject;

class Observer
{
protected:
	virtual void Update(Subject* caller, void* aspect) = 0;
	friend class Subject;
};

class Subject
{
public:
	void Attach(Observer* observer)
	{
		m_observers.push_back(observer);
	}

	void Detach(Observer* observer)
	{
		m_observers.erase(std::remove(m_observers.begin(), m_observers.end(), observer), m_observers.end());
	}

	void Notify(void* aspect)
	{
		for (Observer* observer : m_observers)
		{
			observer->Update(this, aspect);
		}
	}

private:
	std::vector<Observer*> m_observers;
};

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

class Thread
{
public:
	Thread() = default;
	Thread(const Thread&) = delete;
	virtual ~Thread() = default;

	virtual void Start()
	{
		m_running = true;

		// NOTE: "m_threadMutex" ensures that "t" lives until the very end of the function
		std::lock_guard<std::mutex> guard(m_threadMutex);

		// start the new thread
		std::thread t([&]{
			{
				// trying to lock "m_threadMutex", this will wait until function "Start()" is completed
				// and "t" is detached.
				std::lock_guard<std::mutex> threadGuard(m_threadMutex);
			}

			thread_handler();
		});

		// save the native handle to be able to cancel (Kill) the thread and then detach
		m_threadObj = t.native_handle();
		t.detach();
	}

	virtual void Stop()
	{
		m_stopped = true;
	}

	virtual void Resume()
	{
		m_stopped = false;
	}

	bool Kill()
	{
		std::lock_guard<std::mutex> guard(m_threadMutex);
		m_stopped = true;

		bool terminated = pthread_cancel(m_threadObj) == 0;

		if (terminated)
		{
			nzbget_rs_thread_count_op(-1);
		}
		return terminated;
	}

	bool IsStopped() { return m_stopped; };
	bool IsRunning() const { return m_running; }
	void SetAutoDestroy(bool autoDestroy) { m_autoDestroy = autoDestroy; }
	static bool IsCurrentThreadStopped()
	{
		return (m_currentThread && m_currentThread->IsStopped()) || nzbget_rs_thread_count_op(2) != 0;
	}

protected:
	virtual void Run() {} // Virtual function - override in derivatives

private:
	inline static std::mutex m_threadMutex;
	inline static thread_local Thread* m_currentThread = nullptr;
	std::thread::native_handle_type m_threadObj = 0;
	std::atomic<bool> m_running{false};
	std::atomic<bool> m_stopped{false};
	std::atomic<bool> m_autoDestroy{false};

	void thread_handler()
	{
		nzbget_rs_thread_count_op(1);
		struct CurrentGuard
		{
			CurrentGuard(Thread* t) { m_currentThread = t; }
			~CurrentGuard() { m_currentThread = nullptr; }
		} currentGuard(this);

		Run();

		// read before m_running goes false: an owner waiting for that may destroy
		// the object right away
		const bool autoDestroy = m_autoDestroy;

		nzbget_rs_thread_count_op(-1);

		m_running = false;

		if (autoDestroy)
		{
			delete this;
		}
	}
};

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
class CharBuffer
{
public:
	CharBuffer() {}
	CharBuffer(int size) { Reserve(size); }
	CharBuffer(CharBuffer& other) : m_data(other.m_data), m_size(other.m_size) { other.m_data = nullptr; other.m_size = 0; }
	~CharBuffer() { Clear(); }
	CharBuffer& operator=(CharBuffer&& other) = delete;
	int Size() { return m_size; }
	void Reserve(int size)
	{
		if (size <= 0)
		{
			return;
		}
		char* data = static_cast<char*>(realloc(m_data, static_cast<size_t>(size)));
		if (data)
		{
			m_data = data;
			m_size = size;
		}
	}
	void Clear()
	{
		free(m_data);
		m_data = nullptr;
		m_size = 0;
	}
	operator char*() const { return m_data; }

protected:
	char* m_data = nullptr;
	int m_size = 0;
};

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

class Crc32
{
public:
	Crc32() { Reset(); }
	void Reset() { m_state = 0; }
	void Append(uchar* block, uint32 length) { m_state = nzbget_rs_crc32_append(block, length, m_state); }
	uint32 Finish() { return m_state; }

private:
	uint32 m_state = 0;
};

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
	const char* GetTempDir() const { return Str(5); }
	const char* GetQueueDir() const { return Str(6); }
	bool GetNzbLog() const { return m_scalars.nzb_log != 0; }
	bool GetRawArticle() const { return m_scalars.raw_article != 0; }
	bool GetSkipWrite() const { return m_scalars.skip_write != 0; }
	bool GetAppendCategoryDir() const { return m_scalars.append_category_dir != 0; }
	bool GetContinuePartial() const { return m_scalars.continue_partial != 0; }
	bool GetFlushQueue() const { return m_scalars.flush_queue != 0; }
	bool GetDupeCheck() const { return m_scalars.dupe_check != 0; }
	EDupeArticleFallback GetDupeArticleFallback() const { return (EDupeArticleFallback)m_scalars.dupe_article_fallback; }
	const char* GetCertStore() const { return Str(23); }
	bool GetCertCheck() const { return m_scalars.cert_check != 0; }
	EOutputMode GetOutputMode() const { return (EOutputMode)m_scalars.output_mode; }
	int GetLogBuffer() const { return m_scalars.log_buffer; }
	EParCheck GetParCheck() const { return (EParCheck)m_scalars.par_check; }
	bool GetParRepair() const { return m_scalars.par_repair != 0; }
	EParScan GetParScan() const { return (EParScan)m_scalars.par_scan; }
	bool GetParQuick() const { return m_scalars.par_quick != 0; }
	bool GetParRename() const { return m_scalars.par_rename != 0; }
	int GetParBuffer() const { return m_scalars.par_buffer; }
	int GetParThreads() const { return m_scalars.par_threads; }
	EHealthCheck GetHealthCheck() const { return (EHealthCheck)m_scalars.health_check; }
	bool GetDirectWrite() const { return m_scalars.direct_write != 0; }
	int GetWriteBuffer() const { return m_scalars.write_buffer; }
	bool GetNzbCleanupDisk() const { return m_scalars.nzb_cleanup_disk != 0; }
	int GetParTimeLimit() const { return m_scalars.par_time_limit; }
	int GetKeepHistory() const { return m_scalars.keep_history; }
	bool GetUseTempUnpackDir() const { return m_scalars.use_temp_unpack_dir != 0; }
	bool GetUnpackCleanupDisk() const { return m_scalars.unpack_cleanup_disk != 0; }
	const char* GetUnrarCmd() const { return Str(30); }
	const char* GetUnpackPassFile() const { return Str(31); }
	const char* GetExtCleanupDisk() const { return Str(32); }
	const char* GetParIgnoreExt() const { return Str(33); }
	const char* GetRenameIgnoreExt() const { return Str(34); }
	const char* GetUnpackIgnoreExt() const { return Str(35); }
	int GetTimeCorrection() const { return m_scalars.time_correction; }
	int GetArticleCache() const { return m_scalars.article_cache; }

	bool GetServerMode() const { return Mode(0); }
	bool GetDaemonMode() const { return Mode(1); }
	bool GetRemoteClientMode() const { return Mode(2); }
	bool GetDirectClientMode() const { return Mode(3); }

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

class WorkState final : public Subject
{
public:
	WorkState() : m_rs(nzbget_rs_workstate_new(reinterpret_cast<uintptr_t>(this),
		+[](uintptr_t c) { reinterpret_cast<WorkState*>(c)->Notify(nullptr); })) {}
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

class ScriptConfig;

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



// Connections use the Rust host callback (rust/src/tlssock.rs). C++ only answers
// operation 2: whether the current C++ Thread was asked to stop.
inline intptr_t TlsHostCb(void*, int op, int, int, uintptr_t, const char*, const char*, const char*, char*, int*)
{
	return (op == 2 && Thread::IsCurrentThreadStopped()) ? 1 : 0;
}

inline uintptr_t NzbgetCQueueHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

inline void InitRsBuildEnv()
{
	static std::once_flag once;
	std::call_once(once, [] {
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
		nzbget_rs_build_env_init(flags, Util::VersionRevision(), sslLib, sslVer, &TlsHostCb,
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

class ServerStat
{
public:
	ServerStat(int serverId) : m_serverId(serverId) {}
	int GetServerId() { return m_serverId; }
	int GetSuccessArticles() { return m_successArticles; }
	void SetSuccessArticles(int successArticles) { m_successArticles = successArticles; }
	int GetFailedArticles() { return m_failedArticles; }
	void SetFailedArticles(int failedArticles) { m_failedArticles = failedArticles; }

private:
	int m_serverId;
	int m_successArticles = 0;
	int m_failedArticles = 0;
};

typedef std::vector<ServerStat> ServerStatListBase;

class ServerStatList : public ServerStatListBase
{
public:
	enum EStatOperation
	{
		soSet,
		soAdd,
		soSubtract
	};

	void StatOp(int serverId, int successArticles, int failedArticles, EStatOperation statOperation)
	{
		ServerStat* serverStat = nullptr;
		for (ServerStat& serverStat1 : this)
		{
			if (serverStat1.GetServerId() == serverId)
			{
				serverStat = &serverStat1;
				break;
			}
		}

		if (!serverStat)
		{
			emplace_back(serverId);
			serverStat = &back();
		}

		int s = serverStat->GetSuccessArticles(), f = serverStat->GetFailedArticles();
		nzbget_rs_server_stat_apply_op((int)statOperation, successArticles, failedArticles, &s, &f);
		serverStat->SetSuccessArticles(s);
		serverStat->SetFailedArticles(f);
	}

	void ListOp(ServerStatList* serverStats, EStatOperation statOperation)
	{
		for (ServerStat& serverStat : serverStats)
		{
			StatOp(serverStat.GetServerId(), serverStat.GetSuccessArticles(), serverStat.GetFailedArticles(), statOperation);
		}
	}
};

class ArticleInfo
{
public:
	enum EStatus
	{
		aiUndefined,
		aiRunning,
		aiFinished,
		aiFailed
	};

	ArticleInfo() = default;
	~ArticleInfo() { DiscardSegment(); }
	ArticleInfo(const ArticleInfo&) = delete;
	ArticleInfo& operator=(const ArticleInfo&) = delete;

	void SetPartNumber(int s) { m_partNumber = s; }
	int GetPartNumber() { return m_partNumber; }
	const char* GetMessageId() { return m_messageId; }
	void SetMessageId(const char* messageId) { m_messageId = messageId; }
	void SetSize(int size) { m_size = size; }
	int GetSize() { return m_size; }
	void AttachSegment(char* data, int allocSize, int64 offset, int size)
	{
		DiscardSegment();
		m_segmentData = data;
		m_segmentAllocSize = allocSize;
		m_segmentOffset = offset;
		m_segmentSize = size;
	}
	void DiscardSegment()
	{
		if (m_segmentAllocSize)
			nzbget_rs_article_dl_exec(nullptr, 112, m_segmentAllocSize, 0, reinterpret_cast<uintptr_t>(m_segmentData), 0, nullptr, nullptr, nullptr);
		m_segmentData = nullptr;
		m_segmentAllocSize = 0;
	}
	const char* GetSegmentContent() { return m_segmentData; }
	void SetSegmentOffset(int64 segmentOffset) { m_segmentOffset = segmentOffset; }
	int64 GetSegmentOffset() { return m_segmentOffset; }
	void SetSegmentSize(int segmentSize) { m_segmentSize = segmentSize; }
	int GetSegmentSize() { return m_segmentSize; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }
	const char* GetResultFilename() { return m_resultFilename; }
	void SetResultFilename(const char* resultFilename) { m_resultFilename = resultFilename; }
	uint32 GetCrc() { return m_crc; }
	void SetCrc(uint32 crc) { m_crc = crc; }
	int GetDupeFallbackRound() { return m_dupeFallbackRound; }
	void SetDupeFallbackRound(int dupeFallbackRound) { m_dupeFallbackRound = dupeFallbackRound; }
	const char* GetDupeOriginalMessageId() { return m_dupeOriginalMessageId; }
	void SetDupeOriginalMessageId(const char* messageId) { m_dupeOriginalMessageId = messageId; }
	int64 GetDupeExpectedOffset() { return m_dupeExpectedOffset; }
	void SetDupeExpectedOffset(int64 dupeExpectedOffset) { m_dupeExpectedOffset = dupeExpectedOffset; }
	int64 GetDupeExpectedEnd() { return m_dupeExpectedEnd; }
	void SetDupeExpectedEnd(int64 dupeExpectedEnd) { m_dupeExpectedEnd = dupeExpectedEnd; }
	std::vector<CString>* GetDupeSources() { return &m_dupeSources; }
	// the donor (nzb-id) behind each pinned source; the release's own message-id has none
	std::vector<int>* GetDupeSourceDonors() { return &m_dupeSourceDonors; }
	// the donor whose article filled this one (0: the release's own)
	int GetDupeDonorId() { return m_dupeDonorId; }
	void SetDupeDonorId(int dupeDonorId) { m_dupeDonorId = dupeDonorId; }
	int GetDupeLeadSnapshot() { return m_dupeLeadSnapshot; }
	void SetDupeLeadSnapshot(int dupeLeadSnapshot) { m_dupeLeadSnapshot = dupeLeadSnapshot; }
	int GetDupeNextLead() { return m_dupeNextLead; }
	void SetDupeNextLead(int dupeNextLead) { m_dupeNextLead = dupeNextLead; }
	int GetDupeDonorCount() { return m_dupeDonorCount; }
	void SetDupeDonorCount(int dupeDonorCount) { m_dupeDonorCount = dupeDonorCount; }
	bool GetDupeProactive() { return m_dupeProactive; }
	void SetDupeProactive(bool dupeProactive) { m_dupeProactive = dupeProactive; }

private:
	char* m_segmentData = nullptr;
	int m_segmentAllocSize = 0;
	std::atomic<int64> m_segmentOffset{0};
	std::atomic<int> m_segmentSize{0};
	std::atomic<uint32> m_crc{0};
	CString m_messageId;
	CString m_resultFilename;
	EStatus m_status = aiUndefined;
	int m_partNumber;
	int m_size = 0;
	// number of times this article was requeued with a message-id borrowed
	// from a duplicate collection (not persisted)
	int m_dupeFallbackRound = 0;
	// the article's own (primary) message-id, saved before the first
	// substitution so the primary can be tried as a revert source (not persisted)
	CString m_dupeOriginalMessageId;
	// decoded byte range a substituted (donor) article is expected to occupy,
	// derived from already-finished neighbour articles at substitution time;
	// -1 = unknown (not persisted)
	int64 m_dupeExpectedOffset = -1;
	int64 m_dupeExpectedEnd = -1;
	// the message-ids this article is tried against, pinned at its first
	// fallback so no later queue/history change or lead rotation can shift the
	// round->source mapping under the article (not persisted)
	std::vector<CString> m_dupeSources;
	std::vector<int> m_dupeSourceDonors;
	int m_dupeDonorId = 0;
	// the nzb-id of the donor whose article the pinned slot 0 fetches; a
	// lead-round result only counts towards demotion while this donor is still
	// the file's lead (not persisted)
	int m_dupeLeadSnapshot = 0;
	// the nzb-id of the next distinct donor in the pinned order - the donor a
	// demotion rotates the file's lead to; 0 = no other donor (not persisted)
	int m_dupeNextLead = 0;
	// distinct donors in the pinned order; bounds the lead rotation (not persisted)
	int m_dupeDonorCount = 0;
	// true when the first donor attempt was proactive cutover rather than a
	// retry after the target article failed on its primary source
	bool m_dupeProactive = false;
};

typedef std::vector<std::unique_ptr<ArticleInfo>> ArticleList;

class FileInfo
{
public:
	enum EPartialState
	{
		psNone,
		psPartial,
		psCompleted
	};

	typedef std::vector<CString> Groups;

	FileInfo(int id = 0) : m_id(id ? id : ++m_idGen) {}
	int GetId() { return m_id; }
	void SetId(int id)
	{
		m_id = id;
		if (m_idMax < m_id)
		{
			m_idMax = m_id;
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
	NzbInfo* GetNzbInfo() { return m_nzbInfo; }
	void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
	ArticleList* GetArticles() { return &m_articles; }
	Groups* GetGroups() { return &m_groups; }
	const char* GetSubject() { return m_subject; }
	void SetSubject(const char* subject) { m_subject = subject; }
	const char* GetFilename() { return m_filename.c_str(); }
	void SetFilename(const char* filename) { m_filename = filename ? filename : ""; }
	void SetOrigname(const char* origname) { m_origname = origname; }
	const char* GetOrigname() { return m_origname; }
	void MakeValidFilename()
	{
		m_filename = FileSystem::MakeValidFilename(m_filename.c_str()).Str();
	}
	bool GetFilenameConfirmed() { return m_filenameConfirmed; }
	void SetFilenameConfirmed(bool filenameConfirmed) { m_filenameConfirmed = filenameConfirmed; }
	void SetSize(int64 size) { m_size = size; m_remainingSize = size; }
	int64 GetSize() { return m_size; }
	int64 GetRemainingSize() { return m_remainingSize; }
	void SetRemainingSize(int64 remainingSize) { m_remainingSize = remainingSize; }
	int64 GetMissedSize() { return m_missedSize; }
	void SetMissedSize(int64 missedSize) { m_missedSize = missedSize; }
	int64 GetSuccessSize() { return m_successSize; }
	void SetSuccessSize(int64 successSize) { m_successSize = successSize; }
	int64 GetFailedSize() { return m_failedSize; }
	void SetFailedSize(int64 failedSize) { m_failedSize = failedSize; }
	int GetTotalArticles() { return m_totalArticles; }
	void SetTotalArticles(int totalArticles) { m_totalArticles = totalArticles; }
	int GetMissedArticles() { return m_missedArticles; }
	void SetMissedArticles(int missedArticles) { m_missedArticles = missedArticles; }
	int GetFailedArticles() { return m_failedArticles; }
	void SetFailedArticles(int failedArticles) { m_failedArticles = failedArticles; }
	int GetSuccessArticles() { return m_successArticles; }
	void SetSuccessArticles(int successArticles) { m_successArticles = successArticles; }
	int GetDupeRecoveredArticles() { return m_dupeRecoveredArticles; }
	void SetDupeRecoveredArticles(int dupeRecoveredArticles) { m_dupeRecoveredArticles = dupeRecoveredArticles; }
	int GetDupeAttemptedArticles() { return m_dupeAttemptedArticles; }
	void SetDupeAttemptedArticles(int dupeAttemptedArticles) { m_dupeAttemptedArticles = dupeAttemptedArticles; }
	bool GetDupeCutover() { return m_dupeCutover; }
	void SetDupeCutover(bool dupeCutover) { m_dupeCutover = dupeCutover; }
	// what the file's first article holds: borrowing from duplicates never writes
	// into par2 data, whatever the file is called
	enum EFirstContent
	{
		fcUnknown,
		fcPar2,
		fcOther
	};
	EFirstContent GetFirstContent() { return m_firstContent; }
	void SetFirstContent(EFirstContent firstContent) { m_firstContent = firstContent; }
	bool GetDupeNoSourceLogged() { return m_dupeNoSourceLogged; }
	void SetDupeNoSourceLogged(bool logged) { m_dupeNoSourceLogged = logged; }
	int GetDupeLeadDonorId() { return m_dupeLeadDonorId; }
	void SetDupeLeadDonorId(int dupeLeadDonorId) { m_dupeLeadDonorId = dupeLeadDonorId; }
	int GetDupeLeadFailures() { return m_dupeLeadFailures; }
	void SetDupeLeadFailures(int dupeLeadFailures) { m_dupeLeadFailures = dupeLeadFailures; }
	int GetDupeLeadSwitches() { return m_dupeLeadSwitches; }
	void SetDupeLeadSwitches(int dupeLeadSwitches) { m_dupeLeadSwitches = dupeLeadSwitches; }
	time_t GetTime() { return m_time; }
	void SetTime(time_t time) { m_time = time; }
	bool GetPaused() { return m_paused; }
	void SetPaused(bool paused);
	bool GetDeleted() { return m_deleted; }
	void SetDeleted(bool Deleted) { m_deleted = Deleted; }
	int GetCompletedArticles() { return m_completedArticles; }
	void SetCompletedArticles(int completedArticles) { m_completedArticles = completedArticles; }
	bool GetParFile() { return m_parFile; }
	void SetParFile(bool parFile) { m_parFile = parFile; }
	Guard GuardOutputFile() { return Guard(m_outputFileMutex); }
	const std::string& GetOutputFilename() const { return m_outputFilename; }
	void SetOutputFilename(const char* outputFilename) { m_outputFilename = outputFilename ? outputFilename : ""; }
	bool GetOutputInitialized() { return m_outputInitialized; }
	void SetOutputInitialized(bool outputInitialized) { m_outputInitialized = outputInitialized; }
	bool GetExtraPriority() { return m_extraPriority; }
	void SetExtraPriority(bool extraPriority);
	int GetActiveDownloads() { return m_activeDownloads; }
	void SetActiveDownloads(int activeDownloads)
	{
		m_activeDownloads = activeDownloads;

		if (m_activeDownloads > 0 && !m_outputFileMutex)
		{
			m_outputFileMutex = std::make_unique<Mutex>();
		}
		else if (m_activeDownloads == 0 && m_outputFileMutex)
		{
			m_outputFileMutex.reset();
		}
	}
	bool GetDupeDeleted() { return m_dupeDeleted; }
	void SetDupeDeleted(bool dupeDeleted) { m_dupeDeleted = dupeDeleted; }
	int GetCachedArticles() { return m_cachedArticles; }
	void SetCachedArticles(int cachedArticles) { m_cachedArticles = cachedArticles; }
	bool GetPartialChanged() { return m_partialChanged; }
	void SetPartialChanged(bool partialChanged) { m_partialChanged = partialChanged; }
	bool GetForceDirectWrite() { return m_forceDirectWrite; }
	void SetForceDirectWrite(bool forceDirectWrite) { m_forceDirectWrite = forceDirectWrite; }
	EPartialState GetPartialState() { return m_partialState; }
	void SetPartialState(EPartialState partialState) { m_partialState = partialState; }
	uint32 GetCrc() { return m_crc; }
	void SetCrc(uint32 crc) { m_crc = crc; }
	const char* GetHash16k() { return m_hash16k; }
	void SetHash16k(const char* hash16k) { m_hash16k = hash16k; }
	const char* GetParSetId() { return m_parSetId; }
	void SetParSetId(const char* parSetId) { m_parSetId = parSetId; }
	bool GetFlushLocked() { return m_flushLocked; }
	void SetFlushLocked(bool flushLocked) { m_flushLocked = flushLocked; }
	const std::string& GetHardLinkPath() const { return m_hardLinkPath; }
	void SetHardLinkPath(std::string hardLinkPath) { m_hardLinkPath = std::move(hardLinkPath); }
	bool IsHardLinked()
	{
		return !m_hardLinkPath.empty();
	}
	int64 GetDecodedFileSize() { return m_decodedFileSize; }
	void SetDecodedFileSize(int64 decodedFileSize) { m_decodedFileSize = decodedFileSize; }

	ServerStatList* GetServerStats() { return &m_serverStats; }

private:
	int m_id;
	NzbInfo* m_nzbInfo = nullptr;
	ArticleList m_articles;
	Groups m_groups;
	ServerStatList m_serverStats;
	CString m_subject;
	std::string m_filename;
	CString m_origname;
	int64 m_size = 0;
	int64 m_remainingSize = 0;
	int64 m_successSize = 0;
	int64 m_failedSize = 0;
	int64 m_missedSize = 0;
	int m_totalArticles = 0;
	int m_missedArticles = 0;
	int m_failedArticles = 0;
	int m_successArticles = 0;
	// articles obtained from a duplicate collection via DupeArticleFallback (not persisted)
	int m_dupeRecoveredArticles = 0;
	// distinct articles for which a duplicate source was tried (not persisted);
	// the denominator for the recovered/attempted completion summary
	int m_dupeAttemptedArticles = 0;
	// once the primary proves to be missing many articles of this file, lead
	// with the duplicate for the remaining articles instead of failing on the
	// primary first (not persisted)
	bool m_dupeCutover = false;
	bool m_dupeNoSourceLogged = false;	// in memory only: said once per file
	EFirstContent m_firstContent = fcUnknown;	// in memory only: read from the file after a restart
	// nzb-id of the duplicate this file's fresh articles try first; rotated to
	// the next duplicate when the lead keeps missing articles, so fresh
	// articles do not re-fail on a holed posting; 0 = not decided yet, the
	// top-scored duplicate leads (not persisted)
	int m_dupeLeadDonorId = 0;
	// consecutive lead-donor misses; reaching LeadDemoteThreshold rotates
	// m_dupeLeadDonorId (not persisted)
	int m_dupeLeadFailures = 0;
	// lead rotations since the last lead success; capped at the donor count so
	// a release whose duplicates are ALL holed stops rotating (and logging)
	// once every duplicate has led (not persisted)
	int m_dupeLeadSwitches = 0;
	time_t m_time = 0;
	bool m_paused = false;
	bool m_deleted = false;
	bool m_filenameConfirmed = false;
	bool m_parFile = false;
	int m_completedArticles = 0;
	bool m_outputInitialized = false;
	std::string m_outputFilename;
	std::unique_ptr<Mutex> m_outputFileMutex;
	bool m_extraPriority = false;
	int m_activeDownloads = 0;
	bool m_dupeDeleted = false;
	std::atomic<int> m_cachedArticles{0};
	bool m_partialChanged = false;
	bool m_forceDirectWrite = false;
	EPartialState m_partialState = psNone;
	uint32 m_crc = 0;
	CString m_hash16k;
	CString m_parSetId;
	bool m_flushLocked = false;
	std::string m_hardLinkPath;
	// raw file size declared by the first successfully decoded yEnc article
	// (not persisted); used to verify articles borrowed from duplicates
	std::atomic<int64> m_decodedFileSize{0};

	inline static std::atomic<int> m_idGen{0};
	inline static int m_idMax = 0;

	friend class CompletedFile;
};

typedef UniqueDeque<FileInfo> FileList;
typedef std::vector<FileInfo*> RawFileList;

class CompletedFile
{
public:
	enum EStatus
	{
		cfNone,
		cfSuccess,
		cfPartial,
		cfFailure
	};

	CompletedFile(int id, std::string filename, std::string origname, EStatus status,
		uint32 crc, bool parFile, std::string hash16k, std::string parSetId)
		: m_id(id)
		, m_status(status)
		, m_crc(crc)
		, m_parFile(parFile)
		, m_filename(std::move(filename))
		, m_origname(std::move(origname))
		, m_hash16k(std::move(hash16k))
		, m_parSetId(std::move(parSetId))
	{
		if (FileInfo::m_idMax < m_id)
		{
			FileInfo::m_idMax = m_id;
		}
	}
	int GetId() { return m_id; }
	void SetFilename(std::string filename) { m_filename = std::move(filename); }
	const char* GetFilename() { return m_filename.c_str(); }
	void SetOrigname(std::string origname) { m_origname = std::move(origname); }
	const char* GetOrigname() { return m_origname.c_str(); }
	bool GetParFile() { return m_parFile; }
	EStatus GetStatus() { return m_status; }
	// a failed file recreated from a duplicate (stream repair) is partial:
	// its bytes are on disk, but were never download-verified
	void SetStatus(EStatus status) { m_status = status; }
	uint32 GetCrc() { return m_crc; }
	const char* GetHash16k() { return m_hash16k.c_str(); }
	const char* GetParSetId() { return m_parSetId.c_str(); }

private:
	int m_id;
	EStatus m_status;
	uint32 m_crc;
	bool m_parFile;
	std::string m_filename;
	std::string m_origname;
	std::string m_hash16k;
	std::string m_parSetId;
};

typedef std::deque<CompletedFile> CompletedFileList;

// a contiguous byte range of a decoded file
struct StreamRange
{
	int64 Offset = 0;
	int64 Size = 0;
};

typedef std::vector<StreamRange> StreamRangeList;

/*
 * Byte ranges of a completed file which no news server could supply,
 * captured while the file's article list still exists, so post-processing
 * can repair them from a differently segmented duplicate posting
 * (see option <DupeArticleFallback> value "stream"; not persisted).
 */
class StreamRepairJob
{
public:
	StreamRepairJob(int fileId, const char* filename, int64 decodedFileSize, int64 failedSize,
		int64 missedSize, int failedArticles, bool parFile, StreamRangeList holes,
		uint64 stepsHash = 0) :
		m_fileId(fileId), m_filename(filename), m_decodedFileSize(decodedFileSize),
		m_failedSize(failedSize), m_missedSize(missedSize), m_failedArticles(failedArticles),
		m_parFile(parFile), m_holes(std::move(holes)), m_stepsHash(stepsHash) {}
	int GetFileId() const { return m_fileId; }
	const char* GetFilename() const { return m_filename; }
	int64 GetDecodedFileSize() const { return m_decodedFileSize; }
	// the file's ENCODED failed size (bytes= units, as counted into
	// m_currentFailedSize) and whether it is a par2 file, captured at
	// job-creation time while the FileInfo was still alive
	int64 GetFailedSize() const { return m_failedSize; }
	int64 GetMissedSize() const { return m_missedSize; }
	int GetFailedArticles() const { return m_failedArticles; }
	bool GetParFile() const { return m_parFile; }
	StreamRangeList* GetHoles() { return &m_holes; }
	const StreamRangeList* GetHoles() const { return &m_holes; }
	void SetHoles(StreamRangeList holes) { m_holes = std::move(holes); }
	// a whole-file job (0) learns its size from the first donor article
	void SetDecodedFileSize(int64 decodedFileSize) { m_decodedFileSize = decodedFileSize; }
	// article-size step fingerprint of the file's posting (see
	// DupeArticleFallback::ArticleSizeStepsHash), 0 = none
	uint64 GetStepsHash() const { return m_stepsHash; }
	bool GetLiveAttempted() const { return m_liveAttempted; }
	void SetLiveAttempted(bool liveAttempted) { m_liveAttempted = liveAttempted; }

private:
	int m_fileId;
	CString m_filename;
	int64 m_decodedFileSize;
	int64 m_failedSize;
	int64 m_missedSize;
	int m_failedArticles;
	bool m_parFile;
	StreamRangeList m_holes;
	uint64 m_stepsHash;
	// whether the download-concurrent live pass already tried this job once:
	// donors are static, so retrying live would only re-pay fetch traffic -
	// whatever is left waits for the post-processing pass (not persisted; a
	// restart re-allows one live attempt, which is harmless)
	bool m_liveAttempted = false;
};

typedef std::vector<StreamRepairJob> StreamRepairJobList;

class NzbParameter
{
public:
	NzbParameter(const char* name, const char* value) :
		m_name(name), m_value(value) {}
	const char* GetName() { return m_name; }
	const char* GetValue() { return m_value; }

private:
	CString m_name;
	CString m_value;

	void SetValue(const char* value) { m_value = value; }

	friend class NzbParameterList;
};

typedef std::deque<NzbParameter> NzbParameterListBase;

class NzbParameterList : public NzbParameterListBase
{
public:
	void SetParameter(const char* name, const char* value)
	{
		bool emptyVal = Util::EmptyStr(value);

		iterator pos = std::find_if(begin(), end(),
			[name](NzbParameter& parameter)
			{
				return !strcasecmp(parameter.GetName(), name);
			});

		if (emptyVal && pos != end())
		{
			erase(pos);
		}
		else if (!emptyVal)
		{
			if (pos != end())
			{
				pos->SetValue(value);
			}
			else
			{
				emplace_back(name, value);
			}
		}
	}

	NzbParameter* Find(const char* name)
	{
		for (NzbParameter& parameter : this)
		{
			if (!strcasecmp(parameter.GetName(), name))
			{
				return &parameter;
			}
		}

		return nullptr;
	}

	void CopyFrom(NzbParameterList* sourceParameters)
	{
		for (NzbParameter& parameter : sourceParameters)
		{
			SetParameter(parameter.GetName(), parameter.GetValue());
		}
	}
};

class ScriptStatus
{
public:
	enum EStatus
	{
		srNone,
		srFailure,
		srSuccess
	};

	ScriptStatus(const char* name, EStatus status) :
		m_name(name), m_status(status) {}
	const char* GetName() { return m_name; }
	EStatus GetStatus() { return m_status; }

private:
	CString m_name;
	EStatus m_status;

	friend class ScriptStatusList;
};

typedef std::deque<ScriptStatus> ScriptStatusListBase;

class ScriptStatusList : public ScriptStatusListBase
{
public:
	ScriptStatus::EStatus CalcTotalStatus()
	{
		std::vector<int> statuses;
		statuses.reserve(size());
		for (ScriptStatus& s : this) statuses.push_back((int)s.GetStatus());
		return (ScriptStatus::EStatus)nzbget_rs_script_calc_total_status(statuses.data(), statuses.size());
	}
};

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

	int GetId() { return m_id; }
	void SetId(int id)
	{
		m_id = id;
		if (m_idMax < m_id)
		{
			m_idMax = m_id;
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
	EKind GetKind() { return m_kind; }
	void SetKind(EKind kind) { m_kind = kind; }
	const char* GetUrl() { return m_url; }
	void SetUrl(const char* url)
	{
		m_url = url;

		if (!m_name)
		{
			CString nzbNicename = MakeNiceUrlName(url, m_filename);
			SetName(nzbNicename);
		}
	}
	const char* GetFilename() { return m_filename; }
	void SetFilename(const char* filename)
	{
		bool hadFilename = !Util::EmptyStr(m_filename);
		m_filename = filename;

		if ((!m_name || !hadFilename) && !Util::EmptyStr(filename))
		{
			CString nzbNicename = MakeNiceNzbName(m_filename, true);
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
	const char* GetDestDir() { return m_destDir; }
	void SetDestDir(const char* destDir) { m_destDir = destDir; }
	const char* GetFinalDir() { return m_finalDir; }
	void SetFinalDir(const char* finalDir) { m_finalDir = finalDir; }
	const char* GetCategory() { return m_category; }
	void SetCategory(const char* category) { m_category = category; }
	const char* GetName() { return m_name; }
	void SetHardLinkPath(std::string hardLinkPath) { m_hardLinkPath = std::move(hardLinkPath); }
	const std::string& GetHardLinkPath() const { return m_hardLinkPath; }
	void SetName(const char* name) { m_name = name; }
	const std::string& GetMetaName() const { return m_metaName; }
	void SetMetaName(std::string metaName) { m_metaName = std::move(metaName); }
	int GetFileCount() { return m_fileCount; }
	void SetFileCount(int fileCount) { m_fileCount = fileCount; }
	int GetParkedFileCount() { return m_parkedFileCount; }
	void SetParkedFileCount(int parkedFileCount) { m_parkedFileCount = parkedFileCount; }
	int64 GetSize() { return m_size; }
	void SetSize(int64 size) { m_size = size; }
	int64 GetRemainingSize() { return m_remainingSize; }
	void SetRemainingSize(int64 remainingSize) { m_remainingSize = remainingSize; }
	int64 GetPausedSize() { return m_pausedSize; }
	void SetPausedSize(int64 pausedSize) { m_pausedSize = pausedSize; }
	int GetPausedFileCount() { return m_pausedFileCount; }
	void SetPausedFileCount(int pausedFileCount) { m_pausedFileCount = pausedFileCount; }
	int GetRemainingParCount() { return m_remainingParCount; }
	void SetRemainingParCount(int remainingParCount) { m_remainingParCount = remainingParCount; }
	int GetActiveDownloads() { return m_activeDownloads; }
	void SetActiveDownloads(int activeDownloads)
	{
		int64_t startTime = static_cast<int64_t>(m_downloadStartTime);
		int changed = 0;
		nzbget_rs_nzb_calc_active_downloads_sec(
			m_activeDownloads, activeDownloads, m_kind == NzbInfo::nkNzb ? 1 : 0,
			static_cast<int64_t>(Util::CurrentTime()), &m_downloadSec, &startTime,
			&m_downloadStartSec, &changed);
		m_downloadStartTime = static_cast<time_t>(startTime);
		if (changed) m_changed = true;
		m_activeDownloads = activeDownloads;
	}
	int64 GetSuccessSize() { return m_successSize; }
	void SetSuccessSize(int64 successSize) { m_successSize = successSize; }
	int64 GetFailedSize() { return m_failedSize; }
	void SetFailedSize(int64 failedSize) { m_failedSize = failedSize; }
	int64 GetCurrentSuccessSize() { return m_currentSuccessSize; }
	void SetCurrentSuccessSize(int64 currentSuccessSize) { m_currentSuccessSize = currentSuccessSize; }
	int64 GetCurrentFailedSize() { return m_currentFailedSize; }
	void SetCurrentFailedSize(int64 currentFailedSize) { m_currentFailedSize = currentFailedSize; }
	int64 GetParSize() { return m_parSize; }
	void SetParSize(int64 parSize) { m_parSize = parSize; }
	int64 GetParSuccessSize() { return m_parSuccessSize; }
	void SetParSuccessSize(int64 parSuccessSize) { m_parSuccessSize = parSuccessSize; }
	int64 GetParFailedSize() { return m_parFailedSize; }
	void SetParFailedSize(int64 parFailedSize) { m_parFailedSize = parFailedSize; }
	int64 GetParCurrentSuccessSize() { return m_parCurrentSuccessSize; }
	void SetParCurrentSuccessSize(int64 parCurrentSuccessSize) { m_parCurrentSuccessSize = parCurrentSuccessSize; }
	int64 GetParCurrentFailedSize() { return m_parCurrentFailedSize; }
	void SetParCurrentFailedSize(int64 parCurrentFailedSize) { m_parCurrentFailedSize = parCurrentFailedSize; }
	int GetTotalArticles() { return m_totalArticles; }
	void SetTotalArticles(int totalArticles) { m_totalArticles = totalArticles; }
	int GetSuccessArticles() { return m_successArticles; }
	void SetSuccessArticles(int successArticles) { m_successArticles = successArticles; }
	int GetFailedArticles() { return m_failedArticles; }
	void SetFailedArticles(int failedArticles) { m_failedArticles = failedArticles; }
	int GetCurrentSuccessArticles() { return m_currentSuccessArticles; }
	void SetCurrentSuccessArticles(int currentSuccessArticles) { m_currentSuccessArticles = currentSuccessArticles; }
	int GetCurrentFailedArticles() { return m_currentFailedArticles; }
	void SetCurrentFailedArticles(int currentFailedArticles) { m_currentFailedArticles = currentFailedArticles; }
	int GetDupeRecoveredArticles() { return m_dupeRecoveredArticles; }
	// duplicates whose articles failed the par2 checksums: no more articles from them (B40, not persisted)
	std::set<int>* GetDupeBlockedDonors() { return &m_dupeBlockedDonors; }
	// failed articles of its own in a row, first articles of files aside (B49, not persisted)
	int GetDupeFailedRun() { return m_dupeFailedRun; }
	void SetDupeFailedRun(int dupeFailedRun) { m_dupeFailedRun = dupeFailedRun; }
	void SetDupeRecoveredArticles(int dupeRecoveredArticles) { m_dupeRecoveredArticles = dupeRecoveredArticles; }
	int64 GetDupeRecoveredBytes() { return m_dupeRecoveredBytes; }
	void SetDupeRecoveredBytes(int64 value) { m_dupeRecoveredBytes = value; }
	int GetDupeRecoveredHoles() { return m_dupeRecoveredHoles; }
	void SetDupeRecoveredHoles(int value) { m_dupeRecoveredHoles = value; }
	int GetDupeAttemptedArticles() { return m_dupeAttemptedArticles; }
	void SetDupeAttemptedArticles(int value) { m_dupeAttemptedArticles = value; }
	int GetDupeUnsourcedArticles() { return m_dupeUnsourcedArticles; }
	void SetDupeUnsourcedArticles(int value) { m_dupeUnsourcedArticles = value; }
	int GetDupeFailoverChecked() { return m_dupeFailoverChecked; }
	void SetDupeFailoverChecked(int value) { m_dupeFailoverChecked = value; }
	bool GetDeadPickProbed() { return m_deadPickProbed; }
	void SetDeadPickProbed(bool value) { m_deadPickProbed = value; }
	enum EDupeParDeferState { dpNone, dpDeferred, dpLifted };
	EDupeParDeferState GetDupeParDeferState() { return m_dupeParDeferState; }
	void SetDupeParDeferState(EDupeParDeferState state) { m_dupeParDeferState = state; }
	bool GetDupeNoParNoted() { return m_dupeNoParNoted; }
	void SetDupeNoParNoted(bool noted) { m_dupeNoParNoted = noted; }
	StreamRepairJobList* GetStreamRepairJobs() { return &m_streamRepairJobs; }
	int GetPriority() { return m_priority; }
	void SetPriority(int priority) { m_priority = priority; }
	int GetExtraPriority() { return m_extraPriority; }
	void SetExtraPriority(int extraPriority) { m_extraPriority = extraPriority; }
	bool HasExtraPriority() { return m_extraPriority > 0; }
	bool GetForcePriority() { return m_priority >= FORCE_PRIORITY; }
	time_t GetMinTime() { return m_minTime; }
	void SetMinTime(time_t minTime) { m_minTime = minTime; }
	time_t GetMaxTime() { return m_maxTime; }
	void SetMaxTime(time_t maxTime) { m_maxTime = maxTime; }
	CString BuildFinalDirName()
	{
		const char* categoryDestDir = nullptr;
		if (!m_category.Empty())
		{
			const char* strs[3]{};
			if (g_Options->QueryCategory(m_category, false, nullptr, strs) && !Util::EmptyStr(strs[1]))
			{
				categoryDestDir = strs[1];
			}
		}
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			2, g_Options->GetDestDir(), categoryDestDir, *m_category,
			GetName(), nullptr, g_Options->GetAppendCategoryDir() ? 1 : 0, GetId());
		CString out(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return out;
	}
	void BuildDestDirName()
	{
		const char* categoryDestDir = nullptr;
		if (Util::EmptyStr(g_Options->GetInterDir()) && !m_category.Empty())
		{
			const char* strs[3]{};
			if (g_Options->QueryCategory(m_category, false, nullptr, strs) && !Util::EmptyStr(strs[1]))
			{
				categoryDestDir = strs[1];
			}
		}
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			3, g_Options->GetInterDir(), g_Options->GetDestDir(), categoryDestDir,
			*m_category, GetName(), g_Options->GetAppendCategoryDir() ? 1 : 0, GetId());
		m_destDir = CString(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
	}
	CompletedFileList* GetCompletedFiles() { return &m_completedFiles; }
	bool RenameCompletedFile(const char* oldName, const char* newAbsoluteOrRelativePath)
	{
		if (Util::EmptyStr(oldName) || Util::EmptyStr(newAbsoluteOrRelativePath)) return false;
		for (CompletedFile& completedFile : m_completedFiles)
		{
			const char* fn = completedFile.GetFilename() ? completedFile.GetFilename() : "";
			if (nzbget_rs_nzb_completed_same_filename(fn, oldName))
			{
				if (Util::EmptyStr(completedFile.GetOrigname())) completedFile.SetOrigname(fn);
				completedFile.SetFilename(newAbsoluteOrRelativePath);
				return true;
			}
		}
		return false;
	}
	void SetDirectRenameStatus(EDirectRenameStatus renameStatus) { m_directRenameStatus = renameStatus; }
	EDirectRenameStatus GetDirectRenameStatus() { return m_directRenameStatus; }
	EPostRenameStatus GetParRenameStatus() { return m_parRenameStatus; }
	void SetParRenameStatus(EPostRenameStatus renameStatus) { m_parRenameStatus = renameStatus; }
	EPostRenameStatus GetRarRenameStatus() { return m_rarRenameStatus; }
	void SetRarRenameStatus(EPostRenameStatus renameStatus) { m_rarRenameStatus = renameStatus; }
	EParStatus GetParStatus() { return m_parStatus; }
	void SetParStatus(EParStatus parStatus) { m_parStatus = parStatus; }
	EDirectUnpackStatus GetDirectUnpackStatus() { return m_directUnpackStatus; }
	void SetDirectUnpackStatus(EDirectUnpackStatus directUnpackStatus) { m_directUnpackStatus = directUnpackStatus; }
	EPostUnpackStatus GetUnpackStatus() { return m_unpackStatus; }
	void SetUnpackStatus(EPostUnpackStatus unpackStatus) { m_unpackStatus = unpackStatus; }
	ECleanupStatus GetCleanupStatus() { return m_cleanupStatus; }
	void SetCleanupStatus(ECleanupStatus cleanupStatus) { m_cleanupStatus = cleanupStatus; }
	EMoveStatus GetMoveStatus() { return m_moveStatus; }
	void SetPostUnpackRenamingStatus(PostUnpackRenamingStatus status) { m_postUnpackRenamingStatus = status; }
	PostUnpackRenamingStatus GetPostUnpackRenamingStatus() { return m_postUnpackRenamingStatus; }
	void SetPostDownloadRenamingStatus(PostDownloadRenamingStatus status) { m_postDownloadRenamingStatus = status; }
	PostDownloadRenamingStatus GetPostDownloadRenamingStatus() { return m_postDownloadRenamingStatus; }
	void SetMoveStatus(EMoveStatus moveStatus) { m_moveStatus = moveStatus; }
	EDeleteStatus GetDeleteStatus() { return m_deleteStatus; }
	void SetDeleteStatus(EDeleteStatus deleteStatus) { m_deleteStatus = deleteStatus; }
	EMarkStatus GetMarkStatus() { return m_markStatus; }
	void SetMarkStatus(EMarkStatus markStatus) { m_markStatus = markStatus; }
	EUrlStatus GetUrlStatus() { return m_urlStatus; }
	int GetExtraParBlocks() { return m_extraParBlocks; }
	void SetExtraParBlocks(int extraParBlocks) { m_extraParBlocks = extraParBlocks; }
	void SetUrlStatus(EUrlStatus urlStatus) { m_urlStatus = urlStatus; }
	const char* GetQueuedFilename() { return m_queuedFilename; }
	void SetQueuedFilename(const char* queuedFilename) { m_queuedFilename = queuedFilename; }
	bool GetDeleting() { return m_deleting; }
	void SetDeleting(bool deleting) { m_deleting = deleting; }
	bool GetParking() { return m_parking; }
	void SetParking(bool parking) { m_parking = parking; }
	bool GetDeletePaused() { return m_deletePaused; }
	void SetDeletePaused(bool deletePaused) { m_deletePaused = deletePaused; }
	bool GetManyDupeFiles() { return m_manyDupeFiles; }
	void SetManyDupeFiles(bool manyDupeFiles) { m_manyDupeFiles = manyDupeFiles; }
	bool GetAvoidHistory() { return m_avoidHistory; }
	void SetAvoidHistory(bool avoidHistory) { m_avoidHistory = avoidHistory; }
	bool GetHealthPaused() { return m_healthPaused; }
	void SetHealthPaused(bool healthPaused) { m_healthPaused = healthPaused; }
	bool GetCleanupDisk() { return m_cleanupDisk; }
	void SetCleanupDisk(bool cleanupDisk) { m_cleanupDisk = cleanupDisk; }
	bool GetUnpackCleanedUpDisk() { return m_unpackCleanedUpDisk; }
	void SetUnpackCleanedUpDisk(bool unpackCleanedUpDisk) { m_unpackCleanedUpDisk = unpackCleanedUpDisk; }
	bool GetAddUrlPaused() { return m_addUrlPaused; }
	void SetAddUrlPaused(bool addUrlPaused) { m_addUrlPaused = addUrlPaused; }
	FileList* GetFileList() { return &m_fileList; }
	NzbParameterList* GetParameters() { return &m_ppParameters; }
	ScriptStatusList* GetScriptStatuses() { return &m_scriptStatuses; }
	ServerStatList* GetServerStats() { return &m_serverStats; }
	ServerStatList* GetCurrentServerStats() { return &m_currentServerStats; }
	int CalcHealth()
	{
		return nzbget_rs_nzb_calc_health(
			0, m_size, m_parSize, m_currentFailedSize, m_parCurrentFailedSize, 0);
	}
	int CalcCriticalHealth(bool allowEstimation)
	{
		return nzbget_rs_nzb_calc_health(
			1, m_size, m_parSize, m_currentFailedSize, m_parCurrentFailedSize, allowEstimation ? 1 : 0);
	}
	const char* GetDupeKey() { return m_dupeKey; }
	void SetDupeKey(const char* dupeKey) { m_dupeKey = dupeKey ? dupeKey : ""; }
	int GetDupeScore() { return m_dupeScore; }
	void SetDupeScore(int dupeScore) { m_dupeScore = dupeScore; }
	EDupeMode GetDupeMode() { return m_dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { m_dupeMode = dupeMode; }
	EDupeHint GetDupeHint() { return m_dupeHint; }
	void SetDupeHint(EDupeHint dupeHint) { m_dupeHint = dupeHint; }
	uint32 GetFullContentHash() { return m_fullContentHash; }
	void SetFullContentHash(uint32 fullContentHash) { m_fullContentHash = fullContentHash; }
	uint32 GetFilteredContentHash() { return m_filteredContentHash; }
	void SetFilteredContentHash(uint32 filteredContentHash) { m_filteredContentHash = filteredContentHash; }
	int64 GetDownloadedSize() { return m_downloadedSize; }
	void SetDownloadedSize(int64 downloadedSize) { m_downloadedSize = downloadedSize; }
	int GetDownloadSec() { return m_downloadSec; }
	void SetDownloadSec(int downloadSec) { m_downloadSec = downloadSec; }
	int GetPostTotalSec() { return m_postTotalSec; }
	void SetPostTotalSec(int postTotalSec) { m_postTotalSec = postTotalSec; }
	int GetParSec() { return m_parSec; }
	void SetParSec(int parSec) { m_parSec = parSec; }
	int GetRepairSec() { return m_repairSec; }
	void SetRepairSec(int repairSec) { m_repairSec = repairSec; }
	int GetUnpackSec() { return m_unpackSec; }
	void SetUnpackSec(int unpackSec) { m_unpackSec = unpackSec; }
	time_t GetDownloadStartTime() { return m_downloadStartTime; }
	void SetDownloadStartTime(time_t downloadStartTime) { m_downloadStartTime = downloadStartTime; }
	bool GetChanged() { return m_changed; }
	void SetChanged(bool changed) { m_changed = changed; }
	void SetReprocess(bool reprocess) { m_reprocess = reprocess; }
	bool GetReprocess() { return m_reprocess; }
	time_t GetQueueScriptTime() { return m_queueScriptTime; }
	void SetQueueScriptTime(time_t queueScriptTime) { m_queueScriptTime = queueScriptTime; }
	void SetParFull(bool parFull) { m_parFull = parFull; }
	bool GetParFull() { return m_parFull; }
	int GetFeedId() { return m_feedId; }
	void SetFeedId(int feedId) { m_feedId = feedId; }
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
		m_minTime = (time_t)minT;
		m_maxTime = (time_t)maxT;
	}
	bool GetParRepaired() { return m_parRepaired; }
	void SetParRepaired(bool parRepaired) { m_parRepaired = parRepaired; }
	bool GetUnpackTried() { return m_unpackTried; }
	void SetUnpackTried(bool unpackTried) { m_unpackTried = unpackTried; }
	int GetLastUnpackStatus() { return m_lastUnpackStatus; }
	void SetLastUnpackStatus(int lastUnpackStatus) { m_lastUnpackStatus = lastUnpackStatus; }
	std::vector<CString>* GetExtractedArchives() { return &m_extractedArchives; }

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

		typedef std::vector<CString> ParredFiles;
		typedef std::vector<CString> ExtractedArchives;

		NzbInfo* GetNzbInfo() { return m_nzbInfo; }
		void SetNzbInfo(NzbInfo* nzbInfo) { m_nzbInfo = nzbInfo; }
		EStage GetStage() { return m_stage; }
		void SetStage(EStage stage) { m_stage = stage; }
		void SetProgressLabel(const char* progressLabel) { m_progressLabel = progressLabel; }
		const char* GetProgressLabel() { return m_progressLabel; }
		int GetFileProgress() { return m_fileProgress; }
		void SetFileProgress(int fileProgress) { m_fileProgress = fileProgress; }
		int GetStageProgress() { return m_stageProgress; }
		void SetStageProgress(int stageProgress) { m_stageProgress = std::clamp(stageProgress, 0, 1000); }
		time_t GetStartTime() { return m_startTime; }
		void SetStartTime(time_t startTime) { m_startTime = startTime; }
		time_t GetStageTime() { return m_stageTime; }
		void SetStageTime(time_t stageTime) { m_stageTime = stageTime; }
		bool GetWorking() { return m_working; }
		void SetWorking(bool working) { m_working = working; }
		bool GetDeleted() { return m_deleted; }
		void SetDeleted(bool deleted) { m_deleted = deleted; }
		bool GetRequestParCheck() { return m_requestParCheck; }
		void SetRequestParCheck(bool requestParCheck) { m_requestParCheck = requestParCheck; }
		bool GetForceParFull() { return m_forceParFull; }
		void SetForceParFull(bool forceParFull) { m_forceParFull = forceParFull; }
		bool GetStreamRepairDone() { return m_streamRepairDone; }
		void SetStreamRepairDone(bool streamRepairDone) { m_streamRepairDone = streamRepairDone; }
		bool GetStreamRepairRecovered() { return m_streamRepairRecovered; }
		void SetStreamRepairRecovered(bool streamRepairRecovered) { m_streamRepairRecovered = streamRepairRecovered; }
		bool GetParCannotCover() { return m_parCannotCover; }
		void SetParCannotCover(bool parCannotCover) { m_parCannotCover = parCannotCover; }
		bool GetForceRepair() { return m_forceRepair; }
		void SetForceRepair(bool forceRepair) { m_forceRepair = forceRepair; }
		bool GetParRepaired() { return m_nzbInfo ? m_nzbInfo->GetParRepaired() : m_parRepaired; }
		void SetParRepaired(bool parRepaired)
		{
			m_parRepaired = parRepaired;
			if (m_nzbInfo) m_nzbInfo->SetParRepaired(parRepaired);
		}
		bool GetUnpackTried() { return m_nzbInfo ? m_nzbInfo->GetUnpackTried() : m_unpackTried; }
		void SetUnpackTried(bool unpackTried)
		{
			m_unpackTried = unpackTried;
			if (m_nzbInfo) m_nzbInfo->SetUnpackTried(unpackTried);
		}
		bool GetPassListTried() { return m_passListTried; }
		void SetPassListTried(bool passListTried) { m_passListTried = passListTried; }
		int GetLastUnpackStatus() { return m_nzbInfo ? m_nzbInfo->GetLastUnpackStatus() : m_lastUnpackStatus; }
		void SetLastUnpackStatus(int unpackStatus)
		{
			m_lastUnpackStatus = unpackStatus;
			if (m_nzbInfo) m_nzbInfo->SetLastUnpackStatus(unpackStatus);
		}
		bool GetNeedParCheck() { return m_needParCheck; }
		void SetNeedParCheck(bool needParCheck) { m_needParCheck = needParCheck; }
		bool GetNeedUnpack() const { return m_needUnpack; }
		void SetNeedUnpack(bool needUnpack) { m_needUnpack = needUnpack; }
		Thread* GetPostThread() { return m_postThread; }
		void SetPostThread(Thread* postThread) { m_postThread = postThread; }
		ParredFiles* GetParredFiles() { return &m_parredFiles; }
		ExtractedArchives* GetExtractedArchives() { return m_nzbInfo ? m_nzbInfo->GetExtractedArchives() : &m_extractedArchives; }

	private:
		NzbInfo* m_nzbInfo = nullptr;
		std::atomic<bool> m_working{false};
		bool m_deleted = false;
		bool m_requestParCheck = false;
		bool m_forceParFull = false;
		// the post-processing stream repair has run, and whether it wrote bytes
		bool m_streamRepairDone = false;
		bool m_streamRepairRecovered = false;
		// par-check was postponed because the par2 files cannot cover the damage
		bool m_parCannotCover = false;
		bool m_forceRepair = false;
		bool m_parRepaired = false;
		bool m_unpackTried = false;
		bool m_passListTried = false;
		int m_lastUnpackStatus = 0;
		bool m_needParCheck = false;
		bool m_needUnpack = false;
		EStage m_stage = ptQueued;
		CString m_progressLabel = "";
		int m_fileProgress = 0;
		std::atomic<int> m_stageProgress{0};
		time_t m_startTime = 0;
		time_t m_stageTime = 0;
		Thread* m_postThread = nullptr;
		ParredFiles m_parredFiles;
		ExtractedArchives m_extractedArchives;
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
		m_unpackTried = false;
		m_lastUnpackStatus = 0;
		m_extractedArchives.clear();
		ClearMessages();
	}
	bool IsDupeSuccess()
	{
		return nzbget_rs_nzb_is_dupe_success(
			static_cast<int>(m_markStatus), static_cast<int>(m_deleteStatus),
			static_cast<int>(m_parStatus), static_cast<int>(m_unpackStatus),
			static_cast<int>(m_urlStatus), CalcHealth(), CalcCriticalHealth(true),
			CalcCriticalHealth(false), g_Options->GetHealthCheck() == Options::hcDupe ? 1 : 0) != 0;
	}
	const char* MakeTextStatus(bool ignoreScriptStatus)
	{
		ScriptStatus::EStatus scriptStatus = ignoreScriptStatus ?
			ScriptStatus::srSuccess : m_scriptStatuses.CalcTotalStatus();
		return nzbget_rs_nzb_make_text_status(
			static_cast<int>(m_kind), static_cast<int>(m_markStatus),
			static_cast<int>(m_deleteStatus), static_cast<int>(m_parStatus),
			static_cast<int>(m_unpackStatus), static_cast<int>(m_moveStatus),
			static_cast<int>(m_urlStatus), static_cast<int>(scriptStatus),
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
	int GetCachedMessageCount() { return m_cachedMessageCount; }
	GuardedMessageList GuardCachedMessages() { return GuardedMessageList(&m_messages, &m_logMutex); }
	bool GetAllFirst() { return m_allFirst; }
	void SetAllFirst(bool allFirst) { m_allFirst = allFirst; }
	bool GetWaitingPar() { return m_waitingPar; }
	void SetWaitingPar(bool waitingPar) { m_waitingPar = waitingPar; }
	bool GetLoadingPar() { return m_loadingPar; }
	void SetLoadingPar(bool loadingPar) { m_loadingPar = loadingPar; }
	Thread* GetUnpackThread() { return m_unpackThread; }
	void SetUnpackThread(Thread* unpackThread) { m_unpackThread = unpackThread; }
	Thread* GetLiveRepairThread() { return m_liveRepairThread; }
	void SetLiveRepairThread(Thread* liveRepairThread) { m_liveRepairThread = liveRepairThread; }
	void UpdateCurrentStats()
	{
		RsDownloadCurrentStatsAcc acc{};
		acc.current_success_articles = m_successArticles;
		acc.current_failed_articles = m_failedArticles;
		acc.current_success_size = m_successSize;
		acc.current_failed_size = m_failedSize;
		acc.par_current_success_size = m_parSuccessSize;
		acc.par_current_failed_size = m_parFailedSize;
		m_currentServerStats.ListOp(&m_serverStats, ServerStatList::soSet);
		for (FileInfo* fileInfo : &m_fileList)
		{
			nzbget_rs_nzb_update_current_stats_step(
				&acc, fileInfo->GetRemainingSize(), fileInfo->GetSuccessArticles(),
				fileInfo->GetFailedArticles(), fileInfo->GetSuccessSize(), fileInfo->GetFailedSize(),
				fileInfo->GetExtraPriority() ? 1 : 0, fileInfo->GetPaused() ? 1 : 0, fileInfo->GetParFile() ? 1 : 0);
			m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
		}
		m_pausedFileCount = acc.paused_file_count;
		m_remainingParCount = acc.remaining_par_count;
		m_remainingSize = acc.remaining_size;
		m_pausedSize = acc.paused_size;
		m_currentSuccessArticles = acc.current_success_articles;
		m_currentFailedArticles = acc.current_failed_articles;
		m_currentSuccessSize = acc.current_success_size;
		m_currentFailedSize = acc.current_failed_size;
		m_parCurrentSuccessSize = acc.par_current_success_size;
		m_parCurrentFailedSize = acc.par_current_failed_size;
		m_extraPriority = acc.extra_priority;
	}
	void UpdateCompletedStats(FileInfo* fileInfo)
	{
		nzbget_rs_nzb_update_completed_stats(
			fileInfo->GetSuccessSize(), fileInfo->GetFailedSize(),
			fileInfo->GetFailedArticles(), fileInfo->GetSuccessArticles(),
			fileInfo->GetExtraPriority() ? 1 : 0, fileInfo->GetParFile() ? 1 : 0,
			fileInfo->GetPaused() ? 1 : 0,
			&m_successSize, &m_failedSize, &m_failedArticles, &m_successArticles,
			&m_extraPriority, &m_parSuccessSize, &m_parFailedSize,
			&m_remainingParCount, &m_pausedFileCount);
		m_serverStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soAdd);
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
			&m_fileCount, &m_size, &m_currentSuccessSize, &m_failedSize,
			&m_failedArticles, &m_currentFailedSize, &m_totalArticles,
			&m_currentSuccessArticles, &m_currentFailedArticles, &m_remainingSize,
			&m_extraPriority, &m_remainingParCount, &m_parSize,
			&m_parCurrentSuccessSize, &m_parFailedSize, &m_parCurrentFailedSize,
			&m_pausedFileCount, &m_pausedSize);
		m_currentServerStats.ListOp(fileInfo->GetServerStats(), ServerStatList::soSubtract);
	}
	bool IsDownloadCompleted(bool ignorePausedPars)
	{
		if (m_activeDownloads != 0)
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
	void SetDesiredServerId(int id) { m_desiredServerId = id; }
	int GetDesiredServerId() { return m_desiredServerId; }
	bool GetSkipScriptProcessing() { return m_skipScriptProcessing; }
	void SetSkipScriptProcessing(bool skip) { m_skipScriptProcessing = skip; }
	void SetSkipDiskWrite(bool skipWrite) { m_skipDiskWrite = skipWrite; }
	bool GetSkipDiskWrite() { return m_skipDiskWrite; }
	void SetAutoCategory(bool autoCategory) { m_autoCategory = autoCategory; }
	bool GetAutoCategory() const { return m_autoCategory; }
	bool HasDesiredServer() const { return m_desiredServerId > 0; }

	static const int FORCE_PRIORITY = 900;

private:
	int m_id = ++m_idGen;
	EKind m_kind = nkNzb;
	CString m_url = "";
	CString m_filename = "";
	CString m_name;
	CString m_destDir = "";
	CString m_finalDir = "";
	CString m_category = "";
	std::string m_hardLinkPath;
	std::string m_metaName;
	int m_fileCount = 0;
	int m_parkedFileCount = 0;
	int64 m_size = 0;
	int64 m_remainingSize = 0;
	int m_pausedFileCount = 0;
	int64 m_pausedSize = 0;
	int m_remainingParCount = 0;
	int m_activeDownloads = 0;
	int64 m_successSize = 0;
	int64 m_failedSize = 0;
	int64 m_currentSuccessSize = 0;
	int64 m_currentFailedSize = 0;
	int64 m_parSize = 0;
	int64 m_parSuccessSize = 0;
	int64 m_parFailedSize = 0;
	int64 m_parCurrentSuccessSize = 0;
	int64 m_parCurrentFailedSize = 0;
	int m_totalArticles = 0;
	int m_successArticles = 0;
	int m_failedArticles = 0;
	int m_currentSuccessArticles = 0;
	int m_currentFailedArticles = 0;
	// aggregate of FileInfo::m_dupeRecoveredArticles (persisted for restart)
	int m_dupeRecoveredArticles = 0;
	std::set<int> m_dupeBlockedDonors;
	int m_dupeFailedRun = 0;
	int64 m_dupeRecoveredBytes = 0;
	int m_dupeRecoveredHoles = 0;
	// failed articles that were tried from duplicates / that no duplicate
	// could even offer a source for (the early dupe failover's sample of how
	// the download-time recovery is faring; not persisted)
	int m_dupeAttemptedArticles = 0;
	int m_dupeUnsourcedArticles = 0;
	// failed-article count at the last history scan of the early dupe failover
	// (-1 = never scanned), so a download without a backup is not rescanned
	// for every failed article
	int m_dupeFailoverChecked = -1;
	bool m_deadPickProbed = false;
	// which way the par-first rule of duplicate recovery went, for logging once (not persisted)
	EDupeParDeferState m_dupeParDeferState = dpNone;
	bool m_dupeNoParNoted = false;	// "no par2: not borrowing" logged (not saved)
	// missing byte ranges of completed media files awaiting stream repair
	// from duplicate collections in post-processing (persisted)
	StreamRepairJobList m_streamRepairJobs;
	time_t m_minTime = 0;
	time_t m_maxTime = 0;
	int m_priority = 0;
	int m_extraPriority = 0;
	CompletedFileList m_completedFiles;
	EDirectRenameStatus m_directRenameStatus = tsNone;
	EPostRenameStatus m_parRenameStatus = rsNone;
	EPostRenameStatus m_rarRenameStatus = rsNone;
	EParStatus m_parStatus = psNone;
	EDirectUnpackStatus m_directUnpackStatus = nsNone;
	EPostUnpackStatus m_unpackStatus = usNone;
	ECleanupStatus m_cleanupStatus = csNone;
	EMoveStatus m_moveStatus = msNone;
	PostUnpackRenamingStatus m_postUnpackRenamingStatus = PostUnpackRenamingStatus::None;
	PostDownloadRenamingStatus m_postDownloadRenamingStatus = PostDownloadRenamingStatus::None;
	EDeleteStatus m_deleteStatus = dsNone;
	EMarkStatus m_markStatus = ksNone;
	EUrlStatus m_urlStatus = lsNone;
	int m_extraParBlocks = 0;
	bool m_addUrlPaused = false;
	bool m_deletePaused = false;
	bool m_manyDupeFiles = false;
	CString m_queuedFilename = "";
	bool m_deleting = false;
	bool m_parking = false;
	bool m_avoidHistory = false;
	bool m_healthPaused = false;
	bool m_cleanupDisk = false;
	bool m_unpackCleanedUpDisk = false;
	CString m_dupeKey = "";
	int m_dupeScore = 0;
	EDupeMode m_dupeMode = dmScore;
	EDupeHint m_dupeHint = dhNone;
	uint32 m_fullContentHash = 0;
	uint32 m_filteredContentHash = 0;
	FileList m_fileList;
	NzbParameterList m_ppParameters;
	ScriptStatusList m_scriptStatuses;
	ServerStatList m_serverStats;
	ServerStatList m_currentServerStats;
	Mutex m_logMutex;
	MessageList m_messages;
	int m_idMessageGen = 0;
	std::unique_ptr<PostInfo> m_postInfo;
	bool m_parRepaired = false;
	bool m_unpackTried = false;
	int m_lastUnpackStatus = 0;
	std::vector<CString> m_extractedArchives;
	int64 m_downloadedSize = 0;
	time_t m_downloadStartTime = 0;
	int m_downloadStartSec = 0;
	int m_downloadSec = 0;
	int m_postTotalSec = 0;
	int m_parSec = 0;
	int m_repairSec = 0;
	int m_unpackSec = 0;
	bool m_reprocess = false;
	bool m_changed = false;
	time_t m_queueScriptTime = 0;
	bool m_parFull = false;
	std::atomic<int> m_messageCount{0};
	int m_cachedMessageCount = 0;
	int m_feedId = 0;
	bool m_allFirst = false;
	bool m_waitingPar = false;
	bool m_loadingPar = false;
	Thread* m_unpackThread = nullptr;
	// download-concurrent stream repair for completed-with-holes files (see
	// option <DupeArticleFallback> value "live"; not persisted)
	Thread* m_liveRepairThread = nullptr;
	int m_desiredServerId = 0;
	bool m_skipScriptProcessing = false;
	bool m_skipDiskWrite = false;
	bool m_autoCategory = false;

	inline static std::atomic<int> m_idGen{0};
	inline static int m_idMax = 0;

	void ClearMessages()
	{
		Guard guard(m_logMutex);
		m_messages.clear();
		m_cachedMessageCount = 0;
	}

	friend class DupInfo;
};

using PostInfo = NzbInfo::PostInfo;

inline void FileInfo::SetPaused(bool paused)
{
	if (m_paused != paused && m_nzbInfo)
	{
		m_nzbInfo->SetPausedFileCount(m_nzbInfo->GetPausedFileCount() + (paused ? 1 : -1));
		m_nzbInfo->SetPausedSize(m_nzbInfo->GetPausedSize() + (paused ? m_remainingSize : -m_remainingSize));
	}
	m_paused = paused;
}

inline void FileInfo::SetExtraPriority(bool extraPriority)
{
	if (m_extraPriority != extraPriority && m_nzbInfo)
	{
		m_nzbInfo->SetExtraPriority(m_nzbInfo->GetExtraPriority() + (extraPriority ? 1 : -1));
	}
	m_extraPriority = extraPriority;
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

	int GetId() { return m_id; }
	void SetId(int id)
	{
		m_id = id;
		if (NzbInfo::m_idMax < m_id)
		{
			NzbInfo::m_idMax = m_id;
		}
	}
	const char* GetName() { return m_name; }
	void SetName(const char* name) { m_name = name; }
	const char* GetDupeKey() { return m_dupeKey; }
	void SetDupeKey(const char* dupeKey) { m_dupeKey = dupeKey; }
	int GetDupeScore() { return m_dupeScore; }
	void SetDupeScore(int dupeScore) { m_dupeScore = dupeScore; }
	EDupeMode GetDupeMode() { return m_dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { m_dupeMode = dupeMode; }
	int64 GetSize() { return m_size; }
	void SetSize(int64 size) { m_size = size; }
	uint32 GetFullContentHash() { return m_fullContentHash; }
	void SetFullContentHash(uint32 fullContentHash) { m_fullContentHash = fullContentHash; }
	uint32 GetFilteredContentHash() { return m_filteredContentHash; }
	void SetFilteredContentHash(uint32 filteredContentHash) { m_filteredContentHash = filteredContentHash; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }

private:
	int m_id = 0;
	CString m_name;
	CString m_dupeKey;
	int m_dupeScore = 0;
	EDupeMode m_dupeMode = dmScore;
	int64 m_size = 0;
	uint32 m_fullContentHash = 0;
	uint32 m_filteredContentHash = 0;
	EStatus m_status = dsUndefined;
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

class DownloadQueue : public Subject
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
	virtual bool EditEntry(int ID, EEditAction action, const char* args) = 0;
	virtual bool EditList(IdList* idList, NameList* nameList, EMatchMode matchMode, EEditAction action, const char* args) = 0;
	virtual void HistoryChanged() = 0;
	virtual void Save() = 0;
	virtual void SaveChanged() = 0;
	void CalcRemainingSize(int64* remaining, int64* remainingForced)
	{
		int64 remainingSize = 0;
		int64 remainingForcedSize = 0;

		for (NzbInfo* nzbInfo : &m_queue)
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (!fileInfo->GetPaused() && !fileInfo->GetDeleted())
				{
					remainingSize += fileInfo->GetRemainingSize();
					if (nzbInfo->GetForcePriority())
					{
						remainingForcedSize += fileInfo->GetRemainingSize();
					}
				}
			}
		}

		*remaining = remainingSize;

		if (remainingForced)
		{
			*remainingForced = remainingForcedSize;
		}
	}

	// a reload creates the queue anew: until it is loaded again, threads
	// waiting for IsLoaded() (the post-processor's queue sanitising, for
	// one) must not take the previous queue's state for it
	static void Init(DownloadQueue* globalInstance) { g_DownloadQueue = globalInstance; g_Loaded = false; nzbget_rs_thread_count_op(4); }
	static void Final() { g_DownloadQueue = nullptr; g_Loaded = false; nzbget_rs_thread_count_op(4); }
	static void Loaded() { g_Loaded = true; nzbget_rs_thread_count_op(3); }

protected:
	DownloadQueue() {}

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

	Guard guard(m_logMutex);

	m_messages.emplace_back(++m_idMessageGen, kind, Util::CurrentTime(), text);

	if (g_Options->GetServerMode() && g_Options->GetNzbLog())
	{
		nzbget_rs_diskstate_op(2, (uintptr_t)m_id, (int)kind, text);
		m_messageCount.fetch_add(1, std::memory_order_relaxed);
	}

	while (m_messages.size() > (uint32)g_Options->GetLogBuffer())
	{
		m_messages.pop_front();
	}

	m_cachedMessageCount = m_messages.size();
}


#ifndef SERVERPOOL_H
#define SERVERPOOL_H

inline uintptr_t NzbgetCArticleDlHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

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

#ifndef ARTICLEDOWNLOADER_H
#define ARTICLEDOWNLOADER_H

class ArticleDownloader final : public Thread, public Subject
{
public:
	enum EStatus
	{
		adUndefined,
		adRunning,
		adWaiting,
		adFinished,
		adFailed,
		adRetry,
		adCrcError,
		adNotFound,
		adConnectError,
		adTransientError,
		adFatalError
	};

	ArticleDownloader(FileInfo* fileInfo, ArticleInfo* articleInfo, uintptr_t connection,
		bool hasAnalyzer, const char* infoName)
		: m_fileInfo(fileInfo), m_articleInfo(articleInfo)
	{
		debug("Creating ArticleDownloader");
#ifndef DISABLE_PARCHECK
		constexpr int parEnabled = 1;
#else
		constexpr int parEnabled = 0;
#endif
		m_rs = nzbget_rs_article_dl_new(hasAnalyzer ? 1 : 0, parEnabled, connection, infoName ? infoName : "");
	}

	~ArticleDownloader() override
	{
		debug("Destroying ArticleDownloader");
		nzbget_rs_article_dl_free(m_rs);
#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}

	FileInfo* GetFileInfo() { return m_fileInfo; }
	ArticleInfo* GetArticleInfo() { return m_articleInfo; }
	EStatus GetStatus() { return static_cast<EStatus>(nzbget_rs_article_dl_exec(m_rs, 18, 0, 0, 0, 0, nullptr, nullptr, nullptr)); }
	time_t GetLastUpdateTime() { return static_cast<time_t>(nzbget_rs_article_dl_exec(m_rs, 19, 0, 0, 0, 0, nullptr, nullptr, nullptr)); }

	uintptr_t ExecRs(int op, int a1 = 0, uintptr_t p1 = 0, uintptr_t p2 = 0, const char* s1 = nullptr)
	{
		return nzbget_rs_article_dl_exec(m_rs, op, a1, 0, p1, p2, s1, NzbgetCArticleDlHost, this);
	}

	void Run() override { ExecRs(0); }

	void Stop() override
	{
		debug("Trying to stop ArticleDownloader");
		Thread::Stop();
		nzbget_rs_article_dl_exec(m_rs, 16, 0, 0, 0, 0, nullptr, nullptr, nullptr);
		debug("ArticleDownloader stopped successfully");
	}

private:
	FileInfo* m_fileInfo = nullptr;
	ArticleInfo* m_articleInfo = nullptr;
	NzbgetRsArticleDownloader* m_rs = nullptr;

	friend uintptr_t NzbgetCArticleDlHost(void*, int, int, int, uintptr_t, uintptr_t,
		const char*, const char*, int*, int64_t*, const char**);
};

inline uintptr_t NzbgetCArticleDlHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char*, int* ints, int64_t* i64s, const char** strs)
{
	switch (op)
	{
	case 1:
		if (p1 != 0) reinterpret_cast<NzbInfo*>(p1)->PrintMessage((Message::EKind)a1, "%s", s1 ? s1 : "");
		return 0;
	case 4: {
		auto* ad = static_cast<ArticleDownloader*>(ctx);
		if (a1 == 0)
		{
			ints[0] = ad->m_fileInfo->GetNzbInfo()->GetForcePriority() ? 1 : 0;
			ints[1] = ad->m_fileInfo->GetExtraPriority() ? 1 : 0;
			ints[2] = ad->m_fileInfo->GetNzbInfo()->HasDesiredServer() ? 1 : 0;
			ints[3] = ad->m_articleInfo->GetDupeFallbackRound();
			i64s[0] = (int64_t)ad->m_fileInfo->GetTime();
			return 0;
		}
		if (a1 == 1)
		{
			if (a2 == 0)
			{
				if (!ad->m_fileInfo || !ad->m_articleInfo) return 0;
				std::optional<Guard> guard;
				if (ad->m_fileInfo->GetActiveDownloads() > 0) guard.emplace(ad->m_fileInfo->GuardOutputFile());
				ad->ExecRs(17);
				return 0;
			}
			if (a2 == 1)
			{
				ints[0] = ad->m_fileInfo->GetId();
				ints[1] = ad->m_articleInfo->GetPartNumber();
				ints[2] = ad->m_fileInfo->GetForceDirectWrite() ? 1 : 0;
				ints[3] = ad->m_fileInfo->GetOutputInitialized() ? 1 : 0;
				strs[0] = ad->m_fileInfo->GetNzbInfo()->GetDestDir();
				strs[1] = ad->m_fileInfo->GetOutputFilename().c_str();
				strs[2] = ad->m_fileInfo->GetFilename();
				return 0;
			}
			if (a2 == 2) { strs[0] = ad->m_articleInfo->GetResultFilename(); return 0; }
			return 0;
		}
		if (a1 == 2) return ad->IsStopped() ? 1 : 0;
		if (a1 == 3) { ad->Notify(nullptr); return 0; }
		if (a1 == 12)
		{
			if (a2 == 0)
			{
				ad->m_articleInfo->SetCrc(0);
				ints[0] = (int)ad->m_fileInfo->GetGroups()->size();
				ints[1] = (ad->m_fileInfo->GetNzbInfo() && ad->m_fileInfo->GetNzbInfo()->GetSkipDiskWrite()) ? 1 : 0;
				strs[0] = ad->m_articleInfo->GetMessageId();
			}
			else if (a2 == 1) strs[0] = *ad->m_fileInfo->GetGroups()->at(p2);
			return 0;
		}
		if (a1 == 14)
		{
			ints[0] = ad->m_articleInfo->GetDupeFallbackRound();
			i64s[0] = ad->m_fileInfo->GetDecodedFileSize();
			i64s[1] = ad->m_articleInfo->GetDupeExpectedOffset();
			i64s[2] = ad->m_articleInfo->GetDupeExpectedEnd();
			return 0;
		}
		if (a1 == 15)
		{
			NzbInfo* nzb = ad->m_fileInfo->GetNzbInfo();
			ints[0] = (int)nzb->GetDupeMode();
			ints[1] = nzb->GetManyDupeFiles() ? 1 : 0;
			ints[2] = ad->m_fileInfo->GetForceDirectWrite() ? 1 : 0;
			ints[3] = nzb->GetSkipDiskWrite() ? 1 : 0;
			ints[4] = ad->m_articleInfo->GetSize();
			ints[5] = ad->m_articleInfo->GetDupeFallbackRound();
			strs[0] = nzb->GetDestDir();
			return reinterpret_cast<uintptr_t>(nzb);
		}
		if (a1 == 16)
		{
			bool outputInitialized = false;
			{
				Guard guard = ad->m_fileInfo->GuardOutputFile();
				outputInitialized = ad->m_fileInfo->GetOutputInitialized();
				if (!g_Options->GetDirectWrite() && !ad->m_fileInfo->GetForceDirectWrite())
				{
					ad->m_fileInfo->SetOutputInitialized(true);
				}
			}
			bool isOwnOutputFile = false;
			if (g_Options->GetDirectWrite() || ad->m_fileInfo->GetForceDirectWrite())
			{
				Guard guard = ad->m_fileInfo->GuardOutputFile();
				isOwnOutputFile = ad->m_fileInfo->GetForceDirectWrite() ||
					(s1 && !ad->m_fileInfo->GetOutputFilename().empty() &&
					 FileSystem::SameFilename(ad->m_fileInfo->GetOutputFilename().c_str(), s1));
			}
			ints[0] = outputInitialized ? 1 : 0;
			ints[1] = isOwnOutputFile ? 1 : 0;
			return 0;
		}
		if (a1 == 17)
		{
			Guard guard = ad->m_fileInfo->GuardOutputFile();
			bool skipDiskWrite = a2 != 0;
			if (!ad->m_fileInfo->GetOutputInitialized() ||
				(!skipDiskWrite && !FileSystem::FileExists(s1 ? s1 : "")))
			{
				if (!skipDiskWrite &&
					nzbget_rs_article_dl_exec(nullptr, 20, 0, 0,
						reinterpret_cast<uintptr_t>(ad->m_fileInfo->GetNzbInfo()),
						(uintptr_t)i64s[0], s1 ? s1 : "", NzbgetCArticleDlHost, nullptr) == 0)
				{
					return 0;
				}
				ad->m_fileInfo->SetOutputInitialized(true);
			}
			return 1;
		}
		if (a1 == 18)
		{
			if (a2 == 2)
			{
				ad->m_articleInfo->AttachSegment(reinterpret_cast<char*>(p1), ints[0], i64s[0], (int)p2);
				ad->m_fileInfo->SetCachedArticles(ad->m_fileInfo->GetCachedArticles() + 1);
			}
			else if (a2 == 3)
			{
				ad->m_articleInfo->SetSegmentOffset(i64s[0]);
				ad->m_articleInfo->SetSegmentSize((int)p1);
			}
			else if (a2 == 4) ad->m_articleInfo->SetCrc((uint32)p1);
			else if (a2 == 5) ad->m_articleInfo->SetResultFilename(s1);
			else if (a2 == 6) ad->m_fileInfo->SetOutputFilename(s1);
			return 0;
		}
		if (a1 == 19)
		{
			FileInfo* f = ad->m_fileInfo;
			if (a2 == 0)
			{
				NzbInfo* nzb = f->GetNzbInfo();
				static thread_local std::string s_nzbName, s_nzbDestDir, s_filename, s_fileOutFn;
				{
					GuardedDownloadQueue guard = DownloadQueue::Guard();
					s_nzbName = nzb->GetName();
					s_nzbDestDir = nzb->GetDestDir();
					s_filename = f->GetFilename();
				}
				s_fileOutFn = f->GetOutputFilename();
				ints[0] = g_Options->GetDirectWrite() ? 1 : 0;
				ints[1] = f->GetForceDirectWrite() ? 1 : 0;
				ints[2] = f->GetOutputInitialized() ? 1 : 0;
				ints[3] = f->GetCachedArticles();
				ints[4] = f->GetSuccessArticles();
				ints[5] = f->GetFailedArticles();
				ints[6] = f->GetMissedArticles();
				ints[7] = f->GetTotalArticles();
				ints[8] = f->IsHardLinked() ? 1 : 0;
				ints[9] = Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0;
				ints[10] = g_Options->GetRawArticle() ? 1 : 0;
				ints[11] = (nzb->GetSkipDiskWrite() || g_Options->GetSkipWrite()) ? 1 : 0;
				ints[12] = g_Options->GetWriteBuffer();
				ints[15] = (int)f->GetArticles()->size();
				strs[0] = s_nzbName.c_str();
				strs[1] = s_nzbDestDir.c_str();
				strs[2] = s_filename.c_str();
				strs[3] = s_fileOutFn.c_str();
				return reinterpret_cast<uintptr_t>(nzb);
			}
			if (a2 == 1) { f->SetOutputInitialized(true); return 0; }
			if (a2 == 2)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				f->SetCrc(0);
				f->SetOutputFilename("");
				return 0;
			}
			if (a2 == 4)
			{
				ArticleInfo* pa = f->GetArticles()->at(p1).get();
				ints[0] = (int)pa->GetStatus();
				ints[1] = pa->GetSegmentSize();
				ints[2] = (int)pa->GetCrc();
				ints[3] = pa->GetPartNumber();
				ints[4] = pa->GetDupeFallbackRound();
				i64s[0] = pa->GetSegmentOffset();
				strs[0] = pa->GetSegmentContent();
				strs[1] = pa->GetResultFilename();
				return pa->GetStatus() == ArticleInfo::aiFinished ? 1 : 0;
			}
			if (a2 == 5) { f->GetArticles()->at(p1)->DiscardSegment(); return 0; }
			if (a2 == 6)
			{
				f->SetFailedArticles(f->GetFailedArticles() + (int)p1);
				f->SetSuccessArticles(f->GetSuccessArticles() - (int)p1);
				return 0;
			}
			if (a2 == 7)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				ad->ExecRs(22);
				return 0;
			}
			if (a2 == 8)
			{
				NzbInfo* nzb = f->GetNzbInfo();
				f->SetCrc((uint32)p1);
				f->SetOutputFilename(s1 ? s1 : "");
				ints[0] = f->IsHardLinked() ? 1 : 0;
				ints[1] = Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0;
				strs[0] = f->GetFilename();
				strs[1] = f->GetOutputFilename().c_str();
				strs[2] = nzb->GetDestDir();
				return reinterpret_cast<uintptr_t>(nzb);
			}
			return 0;
		}
		if (a1 == 20)
		{
			FileInfo* f = ad->m_fileInfo;
			f->GetServerStats()->StatOp(ints[0], ints[1], ints[2], ServerStatList::soAdd);
			f->GetNzbInfo()->GetCurrentServerStats()->StatOp(ints[0], ints[1], ints[2], ServerStatList::soAdd);
			return 0;
		}
		if (a1 == 21) { ad->Stop(); return 0; }
		return 0;
	}
	case 5: {
		NzbInfo* nzb = reinterpret_cast<NzbInfo*>(p1);
		if (a1 == 0)
		{
			ints[0] = nzb->GetSkipDiskWrite() ? 1 : 0;
			ints[1] = (int)nzb->GetCompletedFiles()->size();
			strs[0] = nzb->GetDestDir();
			return 0;
		}
		if (a1 == 1)
		{
			strs[0] = nzb->GetCompletedFiles()->at((size_t)a2).GetFilename();
			return 0;
		}
		if (a1 == 2)
		{
			for (FileInfo* fileInfo : nzb->GetFileList())
			{
				if (fileInfo->GetOutputInitialized() && !fileInfo->GetOutputFilename().empty() &&
					fileInfo->GetActiveDownloads() > 0)
				{
					Guard guard = fileInfo->GuardOutputFile();
					if (fileInfo->GetOutputInitialized()) return 1;
				}
			}
			return 0;
		}
		return 0;
	}
	default:
		return 0;
	}
}
#endif

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

class QueueCoordinator : public Thread, public Observer
{
public:
	bool HasMoreJobs() const { return m_hasMoreJobs; }

	QueueCoordinator() : m_rs(nzbget_rs_queue_coord_new())
	{
		InitRsBuildEnv();
		nzbget_rs_queue_coord_exec(m_rs, 0, 0, 0, 0, 0, 0, nullptr, nullptr, &NzbgetCQueueHost, this);
	}

	~QueueCoordinator() override
	{
		nzbget_rs_queue_coord_exec(m_rs, 1, 0, 0, 0, 0, 0, nullptr, nullptr, &NzbgetCQueueHost, this);
		nzbget_rs_queue_coord_free(m_rs);
	}

	void Run() override
	{
		nzbget_rs_queue_coord_exec(m_rs, 2, 0, 0, 0, 0, 0, nullptr, nullptr, &NzbgetCQueueHost, this);
	}

	void Stop() override
	{
		Thread::Stop();
		nzbget_rs_queue_coord_exec(m_rs, 5, 0, 0, 0, 0, 0, nullptr, nullptr, &NzbgetCQueueHost, this);
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_waitCond.notify_all();
	}

	void Update(Subject* caller, void* aspect) override
	{
		if (caller == g_WorkState)
		{
			debug("Notification from WorkState received");
			WakeUp();
			return;
		}
		debug("Notification from ArticleDownloader received");
		ArticleDownloader* ad = static_cast<ArticleDownloader*>(caller);
		if (ad->GetStatus() == ArticleDownloader::adFinished ||
			ad->GetStatus() == ArticleDownloader::adFailed ||
			ad->GetStatus() == ArticleDownloader::adRetry ||
			ad->GetStatus() == ArticleDownloader::adFatalError)
		{
			nzbget_rs_queue_coord_exec(m_rs, 6, 0, 0, reinterpret_cast<uintptr_t>(ad), 0, 0,
				nullptr, nullptr, &NzbgetCQueueHost, this);
		}
	}

	friend uintptr_t NzbgetCQueueHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

private:
	class CoordinatorDownloadQueue : public DownloadQueue
	{
	public:
		CoordinatorDownloadQueue(QueueCoordinator* owner) : m_owner(owner) {}
		void HistoryChanged() override { m_historyChanged = true; }

		bool EditEntry(int ID, EEditAction action, const char* args) override
		{
			return nzbget_rs_queue_coord_exec(m_owner->m_rs, 8, ID, (int)action,
				reinterpret_cast<uintptr_t>(&m_owner->m_downloadQueue), 0, 0,
				args, nullptr, &NzbgetCQueueHost, m_owner) != 0;
		}

		bool EditList(IdList* idList, NameList* nameList, EMatchMode matchMode, EEditAction action, const char* args) override
		{
			m_massEdit = true;
			int dummyId = 0;
			const int* idsPtr = idList ? (idList->empty() ? &dummyId : idList->data()) : nullptr;
			size_t idsLen = idList ? idList->size() : 0;
			std::vector<const char*> namePtrs;
			uintptr_t namesArg = 0;
			int namesLen = -1;
			if (nameList)
			{
				namePtrs.reserve(nameList->size());
				for (const CString& s : *nameList) namePtrs.push_back(s.Str() ? s.Str() : "");
				const char* dummyName = "";
				namesArg = reinterpret_cast<uintptr_t>(namePtrs.empty() ? &dummyName : namePtrs.data());
				namesLen = (int)namePtrs.size();
			}
			int packed = (((int)matchMode & 0xFFFF) << 16) | ((int)action & 0xFFFF);
			bool ret = nzbget_rs_queue_coord_exec(m_owner->m_rs, 9, namesLen, packed,
				reinterpret_cast<uintptr_t>(&m_owner->m_downloadQueue), idsLen, namesArg,
				args, idsPtr, &NzbgetCQueueHost, m_owner) != 0;
			m_massEdit = false;
			if (m_wantSave) Save();
			return ret;
		}

		void Save() override
		{
			if (m_massEdit) { m_wantSave = true; return; }
			if (g_Options->GetServerMode())
			{
				nzbget_rs_diskstate_op(0, reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(this)), m_historyChanged ? 1 : 0, nullptr);
				m_stateChanged = true;
			}
			for (NzbInfo* nzbInfo : GetQueue()) nzbInfo->SetChanged(false);
			m_wantSave = false;
			m_historyChanged = false;
			m_owner->WakeUp();
		}

		void SaveChanged() override
		{
			if (g_Options->GetServerMode())
			{
				nzbget_rs_diskstate_op(1, reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(this)), 0, nullptr);
				m_stateChanged = true;
			}
		}
		bool m_stateChanged = false;
	private:
		QueueCoordinator* m_owner;
		bool m_massEdit = false;
		bool m_wantSave = false;
		bool m_historyChanged = false;
		friend class QueueCoordinator;
	};

	void WakeUp()
	{
		debug("Waking up QueueCoordinator");
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_hasMoreJobs = true;
		m_wakeUp = true;
		m_waitCond.notify_all();
	}

	NzbgetRsQueueCoordinator* m_rs = nullptr;
	CoordinatorDownloadQueue m_downloadQueue{this};
	std::vector<ArticleDownloader*> m_activeDownloads;
	std::atomic<bool> m_hasMoreJobs{true};
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	bool m_wakeUp = false;
};

#endif



#ifndef SCANNER_H
#define SCANNER_H

#include <cstdarg>

#ifndef PREPOSTPROCESSOR_H
#define PREPOSTPROCESSOR_H

class PrePostProcessor;
extern PrePostProcessor* g_PrePostProcessor;

#ifndef EXTENSION_MANAGER_H
#define EXTENSION_MANAGER_H

namespace ExtensionManager { class Manager {}; }

class CommandScriptLog;

#ifndef QUEUESCRIPT_H
#define QUEUESCRIPT_H

class QueueScriptCoordinator {};
extern QueueScriptCoordinator* g_QueueScriptCoordinator;

#endif

#endif

#ifndef DISABLE_PARCHECK
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef PARCHECKER_H
#define PARCHECKER_H

#ifndef DISABLE_PARCHECK

class ParChecker
{
public:
	ParChecker() : m_rs(nzbget_rs_par_checker_new()) {}
	virtual ~ParChecker() { nzbget_rs_par_checker_free(m_rs); }

	enum EStatus
	{
		psFailed,
		psRepairPossible,
		psRepaired,
		psRepairNotNeeded
	};

	void SetDestDir(const char* destDir) { nzbget_rs_par_checker_exec(m_rs, 1, 0, destDir ? destDir : "", nullptr, nullptr); }
	void SetParQuick(bool parQuick) { nzbget_rs_par_checker_exec(m_rs, 2, parQuick ? 1 : 0, nullptr, nullptr, nullptr); }
	bool GetParFull() { return nzbget_rs_par_checker_exec(m_rs, 3, 0, nullptr, nullptr, nullptr) != 0; }
	EStatus GetStatus() { return (EStatus)nzbget_rs_par_checker_exec(m_rs, 4, 0, nullptr, nullptr, nullptr); }
	void Execute() { nzbget_rs_par_checker_exec(m_rs, 0, 0, nullptr, &HostCb, this); }

protected:
	struct Segment
	{
		bool m_success;
		int64 m_offset;
		int m_size;
		uint32 m_crc;
	};
	typedef std::deque<Segment> SegmentList;

	enum EFileStatus
	{
		fsUnknown,
		fsSuccess,
		fsPartial,
		fsFailure
	};

	virtual bool RequestMorePars(int blockNeeded, int* blockFound) = 0;
	virtual EFileStatus FindFileCrc(const char* /*filename*/, uint32* /*crc*/, SegmentList* /*segments*/) { return fsUnknown; }
	virtual void RegisterVerifiedFile(const char* /*filename*/) {}

private:
	NzbgetRsParChecker* m_rs = nullptr;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int /*a2*/, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* /*i64s*/, const char** strs)
	{
		auto* c = static_cast<ParChecker*>(rawCtx);
		switch (op)
		{
		case 0:
			ints[0] = g_Options ? g_Options->GetParBuffer() : 0;
			ints[1] = 0;
			ints[2] = g_Options ? g_Options->GetParThreads() : 1;
			ints[3] = g_Options ? (int)g_Options->GetParScan() : 0;
			ints[4] = (g_Options && g_Options->GetParRepair()) ? 1 : 0;
			strs[0] = g_Options ? g_Options->GetParIgnoreExt() : "";
			return 0;
		case 1: {
			int found = 0;
			bool ok = c->RequestMorePars(a1, &found);
			ints[0] = found;
			return ok ? 1 : 0;
		}
		case 2:
			c->RegisterVerifiedFile(s1 ? s1 : "");
			return 0;
		case 3: {
			SegmentList segs;
			EFileStatus st = c->FindFileCrc(s1 ? s1 : "", reinterpret_cast<uint32*>(p1), &segs);
			auto pushFn = reinterpret_cast<void(*)(uintptr_t, int, int64_t, int, uint32_t)>(reinterpret_cast<uintptr_t>(s2));
			for (const Segment& seg : segs)
				pushFn(p2, seg.m_success ? 1 : 0, seg.m_offset, seg.m_size, seg.m_crc);
			return (uintptr_t)st;
		}
		case 4:
			if (a1 == 0) info("%s", s1 ? s1 : "");
			else if (a1 == 3) debug("%s", s1 ? s1 : "");
			else if (a1 == 4) detail("%s", s1 ? s1 : "");
			return 0;
		default:
			return 0;
		}
	}
};

#endif

#endif

#endif

class PostJobThread final : public Thread
{
public:
	PostJobThread(int kind, PostInfo* postInfo, int nzbId) : m_kind(kind)
	{
		static_assert(sizeof(StreamRange) == 16, "StreamRange ABI match");
		InitRsBuildEnv();
		m_rs = nzbget_rs_post_job_new(kind, reinterpret_cast<uintptr_t>(static_cast<Thread*>(this)),
			reinterpret_cast<uintptr_t>(postInfo), nzbId);
	}

	~PostJobThread() override
	{
		nzbget_rs_post_job_free(m_rs);
#ifndef DISABLE_TLS
		if (m_kind == 4) OpenSSL::StopSSLThread();
#endif
	}

	static void StartPost(PostInfo* postInfo, int kind)
	{
		if (!postInfo || (kind == 9 && postInfo->GetNzbInfo() && postInfo->GetNzbInfo()->GetSkipScriptProcessing()))
			return;
		PostJobThread* job = new PostJobThread(kind, postInfo, 0);
		job->SetAutoDestroy(false);
		postInfo->SetPostThread(job);
		job->Start();
	}

	static void StartLive(NzbInfo* nzbInfo)
	{
		if (!nzbInfo) return;
		PostJobThread* job = new PostJobThread(10, nullptr, nzbInfo->GetId());
		job->SetAutoDestroy(true);
		nzbInfo->SetLiveRepairThread(job);
		job->Start();
	}

	void Run() override { nzbget_rs_post_job_op(m_rs, 1, 0, 0); }

	void Stop() override
	{
		Thread::Stop();
		nzbget_rs_post_job_op(m_rs, 0, 0, 0);
	}

	bool AddPar(FileInfo* fileInfo, bool deleted)
	{
		return nzbget_rs_post_job_op(m_rs, 2, deleted ? 1 : 0, reinterpret_cast<uintptr_t>(fileInfo)) != 0;
	}

private:
	int m_kind = 0;
	NzbgetRsPostJob* m_rs = nullptr;
};

#ifndef DIRECTUNPACK_H
#define DIRECTUNPACK_H

class DirectUnpack final : public Thread
{
public:
	explicit DirectUnpack(int nzbId) : m_rs(nzbget_rs_direct_unpack_new(nzbId, reinterpret_cast<uintptr_t>(static_cast<Thread*>(this)))) { InitRsBuildEnv(); }
	~DirectUnpack() override { nzbget_rs_direct_unpack_free(m_rs); }

	static void StartJob(NzbInfo* nzbInfo)
	{
		DirectUnpack* directUnpack = new DirectUnpack(nzbInfo->GetId());
		directUnpack->SetAutoDestroy(true);
		nzbInfo->SetUnpackThread(directUnpack);
		nzbInfo->SetDirectUnpackStatus(NzbInfo::nsRunning);
		directUnpack->Start();
	}

	bool IsUnpacking() const { return nzbget_rs_direct_unpack_exec(m_rs, 5, 0, 0) != 0; }
	bool IsWaitingVolume() const { return nzbget_rs_direct_unpack_exec(m_rs, 6, 0, 0) != 0; }
	void Run() override { nzbget_rs_direct_unpack_exec(m_rs, 0, 0, 0); }
	void Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) { nzbget_rs_direct_unpack_exec(m_rs, 1, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo)); }
	void FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo) { nzbget_rs_direct_unpack_exec(m_rs, 2, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(fileInfo)); }
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) { nzbget_rs_direct_unpack_exec(m_rs, 3, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo)); }
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) { nzbget_rs_direct_unpack_exec(m_rs, 4, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo)); }

private:
	NzbgetRsDirectUnpack* m_rs = nullptr;
};

#endif

class PrePostProcessor : public Thread, public Observer
{
public:
	bool HasMoreJobs()
	{
		return nzbget_rs_prepost_exec(m_rs, 2, 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost) != 0;
	}

	PrePostProcessor() : m_rs(nzbget_rs_prepost_new(reinterpret_cast<uintptr_t>(this)))
	{
		InitRsBuildEnv();
		debug("Creating PrePostProcessor");
		DownloadQueue::Guard()->Attach(this);
	}

	~PrePostProcessor() override
	{
		nzbget_rs_prepost_free(m_rs);
	}

	void Run() override
	{
		nzbget_rs_prepost_exec(m_rs, 0, 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost);
	}

	void Stop() override
	{
		Thread::Stop();
		nzbget_rs_prepost_exec(m_rs, 1, 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost);
	}

	bool HasActiveUnpack(bool includeWaitingVolume = true)
	{
		return nzbget_rs_prepost_exec(m_rs, 3, includeWaitingVolume ? 1 : 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost) != 0;
	}

	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		nzbget_rs_prepost_exec(m_rs, 6, 0, 0, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost);
	}

protected:
	void Update(Subject*, void* aspect) override
	{
		if (!aspect) return;
		DownloadQueue::Aspect* qa = static_cast<DownloadQueue::Aspect*>(aspect);
		nzbget_rs_prepost_exec(m_rs, 8, (int)qa->action, 0, reinterpret_cast<uintptr_t>(qa->downloadQueue), reinterpret_cast<uintptr_t>(qa->nzbInfo), reinterpret_cast<uintptr_t>(qa->fileInfo), nullptr, g_QueueCoordinator, &NzbgetCQueueHost);
	}

private:
	NzbgetRsPrePostProcessor* m_rs = nullptr;
};
#endif

class Scanner {};
extern Scanner* g_Scanner;
class UrlCoordinator;

#endif /* SCANNER_H */

#ifndef DUPESEARCH_H
#define DUPESEARCH_H
#include <ctime>
#endif


inline uintptr_t NzbgetCQueueHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
{
	auto* self = static_cast<QueueCoordinator*>(ctx);
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
	case 3:
		if (a1 == 4 && a2 == 1 && self && g_WorkState) g_WorkState->Attach(self);
		return 0;
	case 6: {
		if (a1 == 5) PostJobThread::StartLive(reinterpret_cast<NzbInfo*>(p1));
		else if (a1 == 6) return nzbget_rs_article_dl_exec(nullptr, 23, 0, 0, p1, 0, s1 ? s1 : "", NzbgetCArticleDlHost, nullptr);
		return 0;
	}
	case 7: {
		std::string r = a1 == 0 ? TlsCipherBackend::Start() :
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
		auto* dq = reinterpret_cast<DownloadQueue*>(p1);
		if (a1 == 0) dq->Save();
		else if (a1 == 1) dq->SaveChanged();
		else if (a1 == 2) return dq->EditEntry(a2, (DownloadQueue::EEditAction)ints[0], s1) ? 1 : 0;
		else if (a1 == 3)
		{
			DownloadQueue::Aspect aspect = { (DownloadQueue::EAspectAction)a2, dq,
				reinterpret_cast<NzbInfo*>(p2), reinterpret_cast<FileInfo*>((uintptr_t)i64s[0]) };
			dq->Notify(&aspect);
		}
		else if (a1 == 4) { int64 rem = 0, remF = 0; dq->CalcRemainingSize(&rem, &remF); i64s[0] = rem; i64s[1] = remF; }
		else if (a1 == 5) dq->HistoryChanged();
		else if (a1 == 6) { IdList ids{ a2 }; return dq->EditList(&ids, nullptr, DownloadQueue::mmId, (DownloadQueue::EEditAction)ints[0], s1) ? 1 : 0; }
		else if (a1 == 7)
		{
			const int32_t* rawIds = reinterpret_cast<const int32_t*>(p2);
			IdList cIds;
			if (rawIds && ints[2] > 0) cIds.assign(rawIds, rawIds + ints[2]);
			NameList cNames;
			if (strs && ints[3] > 0) { cNames.reserve((size_t)ints[3]); for (int i = 0; i < ints[3]; i++) cNames.emplace_back(strs[i] ? strs[i] : ""); }
			return dq->EditList((a2 & 1) ? &cIds : nullptr, (a2 & 2) ? &cNames : nullptr,
				(DownloadQueue::EMatchMode)ints[1], (DownloadQueue::EEditAction)ints[0], s1 ? s1 : "") ? 1 : 0;
		}
		return 0;
	}
	case 25: {
		if (!self) return 0;
		if (a1 == 0) return reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(&self->m_downloadQueue));
		if (a1 == 1) DownloadQueue::Init(&self->m_downloadQueue);
		else if (a1 == 2) DownloadQueue::Loaded();
		else if (a1 == 3) DownloadQueue::Final();
		else if (a1 == 4) return self->m_downloadQueue.m_stateChanged ? 1 : 0;
		else if (a1 == 6) self->m_hasMoreJobs = (a2 != 0);
		else if (a1 == 7) return self->IsStopped() ? 1 : 0;
		else if (a1 == 8)
		{
			std::unique_lock<std::mutex> lk(self->m_waitMutex);
			self->m_waitCond.wait_for(lk, std::chrono::milliseconds(a2),
				[&] { return self->m_wakeUp || self->IsStopped(); });
			self->m_wakeUp = false;
		}
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
		ints[68] = n->GetUnpackThread() ? 1 : 0; ints[69] = n->GetStreamRepairJobs()->empty() ? 0 : 1;
		ints[70] = (int)n->GetUrlStatus(); ints[71] = n->GetParSec(); ints[72] = n->GetRepairSec();
		ints[73] = n->GetUnpackSec(); ints[74] = n->GetReprocess() ? 1 : 0; ints[75] = n->GetExtraParBlocks();
		ints[76] = n->GetDupeRecoveredHoles(); ints[77] = (int)n->GetScriptStatuses()->CalcTotalStatus();
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
		strs[0] = n->GetName(); strs[1] = n->GetFilename(); strs[2] = n->GetDestDir(); strs[3] = n->GetFinalDir();
		strs[4] = n->GetCategory(); strs[5] = n->GetDupeKey(); strs[6] = n->GetQueuedFilename();
		strs[7] = a1 == 1 ? n->MakeTextStatus(true) : (a1 == 2 ? n->MakeTextStatus(false) : nullptr);
		strs[8] = n->GetUrl();
		return 0;
	}
	case 31: {
		NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
		if (a1 == 0) { NzbParameter* p = n->GetParameters()->Find(s1); if (!p) return 0; strs[0] = p->GetValue(); return 1; }
		if (a1 == 1) { n->GetParameters()->SetParameter(s1, s2); return 0; }
		if (a1 == 2) { n->GetParameters()->CopyFrom(reinterpret_cast<NzbInfo*>(p2)->GetParameters()); return 0; }
		if (a1 == 3) { static thread_local CString s_finalDir; s_finalDir = n->BuildFinalDirName(); strs[0] = s_finalDir.Str(); return 0; }
		if (a1 == 4) return n->GetDupeBlockedDonors()->count(a2) ? 1 : 0;
		if (a1 == 5) { n->GetDupeBlockedDonors()->insert(a2); return 1; }
		if (a1 == 6) { strs[0] = n->GetHardLinkPath().c_str(); return 0; }
		if (a1 == 7) { strs[0] = n->GetMetaName().c_str(); return 0; }
		if (a1 == 8) { if (a2 < 0) return n->GetParameters()->size(); NzbParameter& p = n->GetParameters()->at((size_t)a2); strs[0] = p.GetName(); strs[1] = p.GetValue(); return 0; }
		if (a1 == 9) { if (a2 < 0) return n->GetServerStats()->size(); ServerStat& ss = n->GetServerStats()->at((size_t)a2); ints[0] = ss.GetServerId(); ints[1] = ss.GetSuccessArticles(); ints[2] = ss.GetFailedArticles(); return 0; }
		if (a1 == 10) { n->GetScriptStatuses()->emplace_back(s1 ? s1 : "", (ScriptStatus::EStatus)a2); return 0; }
		if (a1 == 11) return n->RenameCompletedFile(s1 ? s1 : "", s2 ? s2 : "") ? 1 : 0;
		if (a1 == 12) return reinterpret_cast<uintptr_t>(n->GetUnpackThread());
		if (a1 == 13) { if (n->GetExtractedArchives()) n->GetExtractedArchives()->emplace_back(s1 ? s1 : ""); return 0; }
		if (a1 == 14) { n->SetMetaName(s1 ? s1 : ""); return 0; }
		if (a1 == 15) { for (Message& m : reinterpret_cast<NzbInfo*>(p2)->GuardCachedMessages()) n->AddMessage(m.GetKind(), m.GetText(), false); return 0; }
		if (a1 == 16) { static thread_local CString s_niceUrl; s_niceUrl = n->MakeNiceUrlName(s1 ? s1 : "", s2 ? s2 : ""); strs[0] = s_niceUrl.Str(); return 0; }
		if (a1 == 17) { if (a2 < 0) return n->GetScriptStatuses()->size(); ScriptStatus& s = n->GetScriptStatuses()->at((size_t)a2); ints[0] = (int)s.GetStatus(); strs[0] = s.GetName(); return 0; }
		if (a1 == 18) { auto* ea = n->GetExtractedArchives(); if (a2 < 0) return ea ? ea->size() : 0; strs[0] = (*ea)[(size_t)a2].Str() ? (*ea)[(size_t)a2].Str() : ""; return 0; }
		if (a1 == 19) { if (a2 == 0) n->GetServerStats()->StatOp(ints[0], ints[1], ints[2], ServerStatList::soSet); else n->GetCurrentServerStats()->ListOp(n->GetServerStats(), ServerStatList::soSet); return 0; }
		if (a1 == 20)
		{
			GuardedMessageList messages = n->GuardCachedMessages();
			int sz = (int)messages->size();
			int start = sz;
			if (ints[1] > 0) start = sz - std::min(ints[1], sz);
			if (ints[0] > 0 && sz > 0) start = std::max(0, ints[0] - (int)messages->front().GetId());
			auto pushFn = reinterpret_cast<void(*)(uintptr_t, int, int, int, const char*)>(reinterpret_cast<uintptr_t>(s1));
			if ((uint32)start < (uint32)sz)
				for (uint32 i = (uint32)start; i < (uint32)sz; i++)
				{
					Message& m = messages->at(i);
					pushFn(p2, (int)m.GetId(), (int)m.GetKind(), (int)m.GetTime(), m.GetText());
				}
			return 0;
		}
		if (a1 == 21) { if (a2 < 0) return n->GetCurrentServerStats()->size(); ServerStat& ss = n->GetCurrentServerStats()->at((size_t)a2); ints[0] = ss.GetServerId(); ints[1] = ss.GetSuccessArticles(); ints[2] = ss.GetFailedArticles(); return 0; }
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
		case 56: n->GetScriptStatuses()->clear(); break;
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
		case 95: if (n->GetExtractedArchives()) n->GetExtractedArchives()->clear(); break;
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
		case 111: n->GetParameters()->clear(); break;
		case 112: n->GetStreamRepairJobs()->clear(); break;
		case 113: n->GetCompletedFiles()->clear(); break;
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
			src->GetCurrentServerStats()->ListOp(f->GetServerStats(), ServerStatList::soSubtract);
			dst->GetCurrentServerStats()->ListOp(f->GetServerStats(), ServerStatList::soAdd);
			return 0;
		}
		if (a1 == 4)
		{
			NzbInfo* dst = reinterpret_cast<NzbInfo*>(p1);
			NzbInfo* src = reinterpret_cast<NzbInfo*>(p2);
			for (std::unique_ptr<FileInfo>& fi : *src->GetFileList()) { fi->SetNzbInfo(dst); dst->GetFileList()->Add(std::move(fi)); }
			src->GetFileList()->clear();
			dst->GetServerStats()->ListOp(src->GetServerStats(), ServerStatList::soAdd);
			dst->GetCurrentServerStats()->ListOp(src->GetCurrentServerStats(), ServerStatList::soAdd);
			for (CompletedFile& cf : src->GetCompletedFiles()) dst->GetCompletedFiles()->push_back(std::move(cf));
			src->GetCompletedFiles()->clear();
			return 0;
		}
		if (a1 == 5)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (a2 < 0) return n->GetCompletedFiles()->size();
			CompletedFile& cf = n->GetCompletedFiles()->at((size_t)a2);
			ints[0] = cf.GetId(); ints[1] = (int)cf.GetStatus(); ints[2] = cf.GetParFile() ? 1 : 0;
			ints[3] = (int)cf.GetCrc();
			strs[0] = cf.GetFilename(); strs[1] = cf.GetOrigname(); strs[2] = cf.GetHash16k(); strs[3] = cf.GetParSetId();
			return 0;
		}
		if (a1 == 6)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			FileInfo* f = reinterpret_cast<FileInfo*>(p2);
			n->GetCompletedFiles()->emplace_back(f->GetId(), s1 ? s1 : "",
				f->GetOrigname() ? f->GetOrigname() : "", (CompletedFile::EStatus)a2,
				(uint32)ints[0], f->GetParFile(), f->GetHash16k() ? f->GetHash16k() : "",
				f->GetParSetId() ? f->GetParSetId() : "");
			return 0;
		}
		if (a1 == 7)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			CompletedFile& cf = n->GetCompletedFiles()->at((size_t)a2);
			if (s2) cf.SetOrigname(s2);
			cf.SetFilename(s1 ? s1 : "");
			return 0;
		}
		if (a1 == 8)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			const auto* rawHoles = reinterpret_cast<const StreamRange*>(p2);
			StreamRangeList holes;
			if (rawHoles && a2 > 0) holes.assign(rawHoles, rawHoles + a2);
			n->GetStreamRepairJobs()->emplace_back(ints[0], s1 ? s1 : "", i64s[0], i64s[1], i64s[2],
				ints[1], ints[2] != 0, std::move(holes), (uint64)i64s[3]);
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
			if (a2 < 0) return n->GetStreamRepairJobs()->size();
			StreamRepairJob& job = n->GetStreamRepairJobs()->at((size_t)a2);
			int64_t holesTotal = 0;
			for (const StreamRange& r : *job.GetHoles()) holesTotal += r.Size;
			ints[0] = job.GetFileId();
			i64s[0] = job.GetDecodedFileSize();
			i64s[1] = holesTotal;
			i64s[2] = job.GetFailedSize() + job.GetMissedSize();
			strs[0] = job.GetFilename();
			return 0;
		}
		if (a1 == 12)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			for (CompletedFile& cf : n->GetCompletedFiles())
			{
				if (cf.GetId() == a2)
				{
					if (ints[0] == 1 && cf.GetStatus() != CompletedFile::cfSuccess)
					{
						cf.SetStatus(CompletedFile::cfSuccess);
						return 1;
					}
					else if (ints[0] == 2 && cf.GetStatus() == CompletedFile::cfFailure)
					{
						cf.SetStatus(CompletedFile::cfPartial);
					}
					break;
				}
			}
			return 0;
		}
		if (a1 == 13)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			auto* jobs = n->GetStreamRepairJobs();
			if (ints[0] == 1)
			{
				StreamRepairJob& job = jobs->at((size_t)a2);
				bool prev = job.GetLiveAttempted();
				job.SetLiveAttempted(true);
				return prev ? 1 : 0;
			}
			if (ints[0] == 2)
			{
				jobs->erase(jobs->begin() + a2);
				if (ints[1] != 0) n->SetChanged(true);
				return 0;
			}
			if (ints[0] == 3)
			{
				StreamRepairJob& job = jobs->at((size_t)a2);
				if (i64s[0] >= 0) job.SetDecodedFileSize(i64s[0]);
				const auto* ranges = reinterpret_cast<const StreamRange*>(p2);
				job.SetHoles(ranges && ints[1] > 0 ? StreamRangeList(ranges, ranges + ints[1]) : StreamRangeList());
				return 0;
			}
			StreamRepairJob& job = jobs->at((size_t)a2);
			ints[0] = job.GetFileId(); ints[1] = job.GetFailedArticles();
			ints[2] = job.GetParFile() ? 1 : 0; ints[3] = job.GetLiveAttempted() ? 1 : 0;
			ints[4] = (int)job.GetHoles()->size();
			i64s[0] = job.GetDecodedFileSize(); i64s[1] = job.GetFailedSize();
			i64s[2] = job.GetMissedSize(); i64s[3] = (int64_t)job.GetStepsHash();
			strs[0] = job.GetFilename();
			strs[1] = job.GetHoles()->empty() ? nullptr : reinterpret_cast<const char*>(job.GetHoles()->data());
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
			for (int g = 0; g < ints[5]; g++) fi->GetGroups()->push_back(gPtrs[g] ? gPtrs[g] : "");
			const auto* aParts = reinterpret_cast<const int*>((uintptr_t)i64s[3]);
			const auto* aSizes = reinterpret_cast<const int64_t*>((uintptr_t)i64s[4]);
			const auto* aMids = reinterpret_cast<const char* const*>(strs[1]);
			ArticleList* arts = fi->GetArticles();
			arts->reserve((size_t)ints[6]);
			for (int k = 0; k < ints[6]; k++)
			{
				auto art = std::make_unique<ArticleInfo>();
				art->SetPartNumber(aParts[k]); art->SetSize((int)aSizes[k]); art->SetMessageId(aMids[k] ? aMids[k] : "");
				arts->push_back(std::move(art));
			}
			FileInfo* raw = fi.get();
			n->GetFileList()->Add(std::move(fi));
			return reinterpret_cast<uintptr_t>(raw);
		}
		if (a1 == 15)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			n->GetCompletedFiles()->clear(); n->GetStreamRepairJobs()->clear();
			n->GetServerStats()->clear(); n->GetCurrentServerStats()->clear();
			n->MoveFileList(reinterpret_cast<NzbInfo*>(p2));
			return 0;
		}
		if (a1 == 16) { auto* cfs = reinterpret_cast<NzbInfo*>(p1)->GetCompletedFiles(); cfs->erase(cfs->begin() + a2); return 0; }
		if (a1 == 17) { reinterpret_cast<NzbInfo*>(p1)->GetFileList()->Add(std::unique_ptr<FileInfo>(reinterpret_cast<FileInfo*>(p2)), false); return 0; }
		if (a1 == 18)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			n->GetCompletedFiles()->emplace_back(ints[0], strs[0] ? strs[0] : "", strs[1] ? strs[1] : "",
				(CompletedFile::EStatus)ints[1], (uint32)ints[2], ints[3] != 0,
				strs[2] ? strs[2] : "", strs[3] ? strs[3] : "");
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
			DupInfo* d = reinterpret_cast<DupInfo*>(p1);
			ints[0] = (int)d->GetDupeMode(); ints[1] = d->GetDupeScore(); ints[2] = (int)d->GetStatus();
			ints[3] = (int)d->GetFullContentHash(); ints[4] = (int)d->GetFilteredContentHash();
			if (i64s) i64s[0] = d->GetSize();
			strs[0] = d->GetName(); strs[1] = d->GetDupeKey();
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
		static thread_local std::string s_outFn;
		if (f->GetActiveDownloads() > 0) { Guard og = f->GuardOutputFile(); s_outFn = f->GetOutputFilename(); strs[2] = s_outFn.c_str(); }
		else strs[2] = f->GetOutputFilename().c_str();
		strs[0] = f->GetFilename(); strs[1] = f->GetSubject(); strs[3] = f->GetHardLinkPath().c_str();
		strs[4] = f->GetOrigname(); strs[5] = f->GetHash16k(); strs[6] = f->GetParSetId();
		return f->GetArticles()->size();
	}
	case 41: {
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		if (a1 == 0)
		{
			if (a2 < 0) return f->GetGroups()->size();
			strs[0] = *f->GetGroups()->at((size_t)a2);
			return 0;
		}
		if (a1 == 1)
		{
			if (a2 < 0) return f->GetServerStats()->size();
			ServerStat& st = f->GetServerStats()->at((size_t)a2);
			ints[0] = st.GetServerId(); ints[1] = st.GetSuccessArticles(); ints[2] = st.GetFailedArticles();
			return 0;
		}
		if (a1 == 2) { f->GetServerStats()->StatOp(ints[0], ints[1], ints[2], ServerStatList::soSet); return 0; }
		if (a1 == 3) { f->GetGroups()->push_back(s1 ? s1 : ""); return 0; }
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
			std::optional<Guard> og;
			if (f->GetActiveDownloads() > 0) og.emplace(f->GuardOutputFile());
			f->SetOutputFilename(s1);
			break;
		}
		case 42: f->SetOrigname(s1); break;
		case 43: f->SetHash16k(s1); break;
		case 44: f->SetParSetId(s1); break;
		case 45: f->SetHardLinkPath(s1 ? s1 : ""); break;
		case 50: f->MakeValidFilename(); break;
		case 51: f->GetArticles()->clear(); break;
		case 52: for (ArticleInfo* ai : f->GetArticles()) { ai->SetStatus(ArticleInfo::aiUndefined); ai->SetResultFilename(nullptr); ai->DiscardSegment(); } break;
		case 53: reinterpret_cast<NzbInfo*>(p2)->GetCurrentServerStats()->ListOp(f->GetServerStats(), ServerStatList::soSubtract); f->GetServerStats()->clear(); break;
		case 54: for (ArticleInfo* ai : f->GetArticles()) if (ai->GetStatus() == ArticleInfo::aiFinished) ai->SetStatus(ArticleInfo::aiFailed); break;
		case 55: {
			std::optional<Guard> og;
			if (f->GetActiveDownloads() > 0) og.emplace(f->GuardOutputFile());
			for (ArticleInfo* pa : f->GetArticles())
				if (pa->GetStatus() != ArticleInfo::aiRunning && pa->GetResultFilename() && (p2 != 0 || pa->GetDupeFallbackRound() > 0))
					FileSystem::DeleteFile(pa->GetResultFilename());
			break;
		}
		case 57: delete f; break;
		case 58: f->SetNzbInfo(reinterpret_cast<NzbInfo*>(p2)); break;
		case 59: reinterpret_cast<NzbInfo*>(p2)->GetServerStats()->ListOp(f->GetServerStats(), ServerStatList::soSubtract); break;
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
		if (a1 == 10)
		{
			ArticleInfo* pa = reinterpret_cast<FileInfo*>(p1)->GetArticles()->at((size_t)a2).get();
			ints[0] = (int)pa->GetStatus(); ints[1] = pa->GetSegmentSize(); ints[2] = (int)pa->GetCrc();
			ints[3] = pa->GetDupeFallbackRound(); ints[4] = pa->GetResultFilename() ? 1 : 0;
			ints[5] = pa->GetPartNumber(); ints[6] = pa->GetSize();
			i64s[0] = pa->GetSegmentOffset();
			strs[0] = pa->GetMessageId(); strs[1] = pa->GetDupeOriginalMessageId();
			return 0;
		}
		if (a1 == 11)
		{
			auto ai = std::make_unique<ArticleInfo>();
			ai->SetPartNumber(a2); ai->SetSize((int)p2); ai->SetMessageId(s1 ? s1 : "");
			reinterpret_cast<FileInfo*>(p1)->GetArticles()->push_back(std::move(ai));
			return 0;
		}
		if (a1 == 12)
		{
			auto* arts = reinterpret_cast<FileInfo*>(p1)->GetArticles();
			while ((size_t)a2 >= arts->size()) arts->push_back(std::make_unique<ArticleInfo>());
			ArticleInfo* pa = arts->at((size_t)a2).get();
			if (p2 == 0) return (uintptr_t)((pa->GetPartNumber() << 1) | (pa->GetResultFilename() ? 1 : 0));
			if (ints[0] >= 2)
			{
				pa->SetSegmentOffset(i64s[0]);
				pa->SetSegmentSize(ints[1]);
				pa->SetCrc((uint32)ints[2]);
			}
			pa->SetDupeFallbackRound(ints[3]);
			if (s1) pa->SetResultFilename(s1);
			pa->SetStatus((ArticleInfo::EStatus)ints[4]);
			return 0;
		}
		auto fillArt = [&](ArticleInfo* a, bool done) -> uintptr_t {
			bool run = !done && a->GetStatus() == ArticleInfo::aiRunning;
			ints[0] = a->GetPartNumber(); ints[1] = a->GetSize(); ints[2] = (int)a->GetStatus();
			ints[3] = run ? 0 : a->GetSegmentSize(); ints[4] = run ? 0 : (int)a->GetCrc(); ints[5] = a->GetDupeFallbackRound();
			ints[6] = a->GetDupeLeadSnapshot(); ints[7] = a->GetDupeNextLead(); ints[8] = a->GetDupeDonorCount();
			ints[9] = a->GetDupeDonorId(); ints[10] = a->GetDupeProactive() ? 1 : 0;
			i64s[0] = run ? -1 : a->GetSegmentOffset();
			strs[0] = a->GetMessageId(); strs[1] = a->GetDupeOriginalMessageId();
			return reinterpret_cast<uintptr_t>(a);
		};
		if (a1 == 0) return fillArt(reinterpret_cast<FileInfo*>(p1)->GetArticles()->at((size_t)a2).get(), false);
		if (a1 == 1) return fillArt(reinterpret_cast<ArticleInfo*>(p1), true);
		if (a1 == 7 || a1 == 8)
		{
			auto* arts = reinterpret_cast<FileInfo*>(p1)->GetArticles();
			auto* target = reinterpret_cast<ArticleInfo*>(p2);
			for (size_t i = 0; i < arts->size(); i++)
				if (a1 == 7 ? arts->at(i)->GetStatus() == ArticleInfo::aiUndefined : arts->at(i).get() == target)
				{
					ints[11] = (int)i;
					return fillArt(arts->at(i).get(), a1 == 8);
				}
			return 0;
		}
		ArticleInfo* a = reinterpret_cast<ArticleInfo*>(p1);
		if (a1 == 2)
		{
			int v = (int)p2;
			if (a2 == 0) a->SetStatus((ArticleInfo::EStatus)v);
			else if (a2 == 1) a->SetDupeFallbackRound(v);
			else if (a2 == 2) a->SetDupeDonorId(v);
			else if (a2 == 3) a->SetDupeProactive(v != 0);
		}
		else if (a1 == 3)
		{
			if (a2 < 0) return a->GetDupeSources()->size();
			ints[1] = ((size_t)a2 < a->GetDupeSourceDonors()->size()) ? 1 : 0;
			ints[0] = ints[1] ? a->GetDupeSourceDonors()->at((size_t)a2) : 0;
			strs[0] = *a->GetDupeSources()->at((size_t)a2);
		}
		else if (a1 == 4)
		{
			a->SetDupeLeadSnapshot(ints[0]); a->SetDupeNextLead(ints[1]); a->SetDupeDonorCount(ints[2]);
			a->GetDupeSources()->clear(); a->GetDupeSources()->reserve((size_t)a2);
			for (int i = 0; i < a2; i++) a->GetDupeSources()->emplace_back(strs[i]);
			a->GetDupeSourceDonors()->assign(ints + 4, ints + 4 + ints[3]);
		}
		else if (a1 == 5) { a->SetDupeExpectedOffset(i64s[0]); a->SetDupeExpectedEnd(i64s[1]); }
		else if (a1 == 6)
		{
			if (a2 == 0) a->SetMessageId(s1);
			else if (a2 == 1) a->SetDupeOriginalMessageId(s1);
		}
		else if (a1 == 9) { a->SetStatus(ArticleInfo::aiUndefined); a->SetCrc(0); a->SetSegmentOffset(0); a->SetSegmentSize(0); a->SetDupeExpectedOffset(-1); a->SetDupeExpectedEnd(-1); }
		return 0;
	}
	case 44: {
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		if (a1 == 1)
		{
			ArticleInfo* a = reinterpret_cast<ArticleInfo*>(p2);
			if (!f->GetFlushLocked() && a->GetSegmentContent() && a2 > 0)
			{
				memcpy(reinterpret_cast<void*>((uintptr_t)i64s[0]), a->GetSegmentContent(), (size_t)a2);
				return 1;
			}
			return 0;
		}
		if (a1 == 2)
		{
			ArticleInfo* a = reinterpret_cast<ArticleInfo*>(p2);
			if (a->GetDupeFallbackRound() > 0 && a->GetResultFilename()) FileSystem::DeleteFile(a->GetResultFilename());
			if (!a->GetSegmentContent() || f->GetFlushLocked()) return 0;
			a->DiscardSegment();
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
			{ Guard og = f->GuardOutputFile(); outInit = f->GetOutputInitialized(); s_outFn = f->GetOutputFilename(); }
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
			for (ArticleInfo* pa : f->GetArticles())
				if (pa->GetSegmentContent())
					pushSeg(p2, reinterpret_cast<uintptr_t>(pa), pa->GetSegmentOffset(), pa->GetSegmentSize(), pa->GetSegmentContent(), pa->GetResultFilename());
			return 1;
		}
		if (a1 == 6)
		{
			if (a2 == 0) return (f->GetDeleted() && !f->GetNzbInfo()->GetParking()) ? 1 : 0;
			reinterpret_cast<ArticleInfo*>(p2)->DiscardSegment();
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
	case 50: {
		if (!self) return 0;
		if (a1 < 0) return self->m_activeDownloads.size();
		ArticleDownloader* ad = self->m_activeDownloads[(size_t)a1];
		ints[0] = (int)ad->GetStatus();
		i64s[0] = (int64_t)reinterpret_cast<uintptr_t>(ad->GetFileInfo()); i64s[1] = (int64_t)ad->GetLastUpdateTime();
		return reinterpret_cast<uintptr_t>(ad);
	}
	case 51: {
		ArticleDownloader* ad = reinterpret_cast<ArticleDownloader*>(p1);
		if (a1 == 0)
		{
			i64s[0] = (int64_t)reinterpret_cast<uintptr_t>(ad->GetFileInfo());
			i64s[1] = (int64_t)reinterpret_cast<uintptr_t>(ad->GetArticleInfo());
			ad->ExecRs(10, 0, reinterpret_cast<uintptr_t>(ints),
				reinterpret_cast<uintptr_t>(i64s), reinterpret_cast<const char*>(strs));
			return reinterpret_cast<uintptr_t>(ad);
		}
		if (a1 == 1) ad->Stop();
		else if (a1 == 2) ad->ExecRs(14);
		else if (a1 == 3) ad->ExecRs(12);
		else if (a1 == 4) return ad->ExecRs(11, 0, reinterpret_cast<uintptr_t>(ints), 0, reinterpret_cast<const char*>(strs));
		else if (a1 == 5) return ad->ExecRs(13);
		else if (a1 == 6) ad->ExecRs(15);
		return 0;
	}
	case 52: {
		if (!self) return 0;
		debug("Starting new ArticleDownloader");
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		ArticleInfo* a = reinterpret_cast<ArticleInfo*>(p2);
		auto* ad = new ArticleDownloader(f, a, (uintptr_t)i64s[0], a1 != 0, s1 ? s1 : "");
		ad->SetAutoDestroy(true);
		ad->Attach(self);
		a->SetStatus(ArticleInfo::aiRunning);
		f->SetActiveDownloads(f->GetActiveDownloads() + 1);
		f->GetNzbInfo()->SetActiveDownloads(f->GetNzbInfo()->GetActiveDownloads() + 1);
		self->m_activeDownloads.push_back(ad);
		ad->Start();
		return 0;
	}
	case 53: {
		if (!self) return 0;
		if (a1 == 0)
		{
			auto* ad = reinterpret_cast<ArticleDownloader*>(p1);
			auto it = std::find(self->m_activeDownloads.begin(), self->m_activeDownloads.end(), ad);
			if (it != self->m_activeDownloads.end()) self->m_activeDownloads.erase(it);
		}
		else if (a1 == 1)
		{
			for (ArticleDownloader* ad : self->m_activeDownloads) delete ad;
			self->m_activeDownloads.clear();
		}
		return 0;
	}
	case 60: {
		PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
		if (!pi) return 0;
		if (a1 == 0)
		{
			Thread* pt = pi->GetPostThread();
			ints[0] = (int)pi->GetStage(); ints[1] = pi->GetWorking() ? 1 : 0; ints[2] = pi->GetDeleted() ? 1 : 0;
			ints[3] = pi->GetRequestParCheck() ? 1 : 0; ints[4] = pi->GetForceParFull() ? 1 : 0; ints[5] = pi->GetForceRepair() ? 1 : 0;
			ints[6] = pi->GetStreamRepairDone() ? 1 : 0; ints[7] = pi->GetStreamRepairRecovered() ? 1 : 0; ints[8] = pi->GetParCannotCover() ? 1 : 0;
			ints[9] = pi->GetNeedParCheck() ? 1 : 0; ints[10] = pi->GetNeedUnpack() ? 1 : 0;
			ints[11] = pt ? 1 : 0; ints[12] = (pt && pt->IsRunning()) ? 1 : 0;
			ints[13] = pi->GetUnpackTried() ? 1 : 0; ints[14] = pi->GetParRepaired() ? 1 : 0;
			ints[15] = pi->GetPassListTried() ? 1 : 0; ints[16] = pi->GetLastUnpackStatus();
			ints[17] = pi->GetExtractedArchives() ? (int)pi->GetExtractedArchives()->size() : 0;
			ints[18] = (int)pi->GetParredFiles()->size();
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
		if (a1 == 6) { if (a2 < 0) pi->GetParredFiles()->push_back(s1 ? s1 : ""); else strs[0] = (*pi->GetParredFiles())[(size_t)a2]; return 0; }
		if (a1 == 7) { if (a2 < 0) { if (pi->GetExtractedArchives()) pi->GetExtractedArchives()->clear(); } else strs[0] = *(*pi->GetExtractedArchives())[(size_t)a2]; return 0; }
		return 0;
	}
	case 61: {
		if (a1 == 0) { PrePostProcessor* pp = reinterpret_cast<PrePostProcessor*>(p1); return (pp && pp->IsStopped()) ? 1 : 0; }
		Thread* t = reinterpret_cast<Thread*>(p1);
		if (a1 == 1) return (t && t->IsStopped()) ? 1 : 0;
		if (a1 == 2 && t) t->Thread::Stop();
		else if (a1 == 3 && t) t->Thread::Resume();
		return 0;
	}
	case 62: {
		if (a1 == 0) { PostInfo* pi = reinterpret_cast<PostInfo*>(p1); if (pi && pi->GetPostThread()) pi->GetPostThread()->Stop(); return 0; }
		if (a1 == 1) { PostInfo* pi = reinterpret_cast<PostInfo*>(p1); if (pi && pi->GetPostThread()) { delete pi->GetPostThread(); pi->SetPostThread(nullptr); } return 0; }
		if (a1 == 2) { PostInfo* pi = reinterpret_cast<PostInfo*>(p1); if (pi && pi->GetPostThread()) { Thread* t = pi->GetPostThread(); pi->SetPostThread(nullptr); t->Kill(); delete t; } return 0; }
		if (a1 == 3)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (!n || !n->GetUnpackThread()) return 0;
			DirectUnpack* du = static_cast<DirectUnpack*>(n->GetUnpackThread());
			return (a2 != 0 || (du->IsUnpacking() && !du->IsWaitingVolume())) ? 1 : 0;
		}
		if (a1 == 4) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p2); if (n && n->GetUnpackThread()) static_cast<DirectUnpack*>(n->GetUnpackThread())->Stop(reinterpret_cast<DownloadQueue*>(p1), n); return 0; }
		if (a1 == 5) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p2); if (n && n->GetUnpackThread()) static_cast<DirectUnpack*>(n->GetUnpackThread())->NzbDownloaded(reinterpret_cast<DownloadQueue*>(p1), n); return 0; }
		if (a1 == 6) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p2); if (n && n->GetUnpackThread()) static_cast<DirectUnpack*>(n->GetUnpackThread())->NzbDeleted(reinterpret_cast<DownloadQueue*>(p1), n); return 0; }
		if (a1 == 7) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p2); if (n && n->GetUnpackThread()) static_cast<DirectUnpack*>(n->GetUnpackThread())->FileDownloaded(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<FileInfo*>((uintptr_t)i64s[0])); return 0; }
		if (a1 == 8) { DirectUnpack::StartJob(reinterpret_cast<NzbInfo*>(p1)); return 0; }
		if (a1 == 9) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p1); if (n && n->GetLiveRepairThread()) n->GetLiveRepairThread()->Stop(); return 0; }
		if (a1 == 10) { NzbInfo* n = reinterpret_cast<NzbInfo*>(p1); if (n && n->GetLiveRepairThread()) { Thread* t = n->GetLiveRepairThread(); n->SetLiveRepairThread(nullptr); t->Stop(); } return 0; }
		if (a1 == 11)
		{
#ifndef DISABLE_PARCHECK
			PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
			if (pi && pi->GetPostThread()) return static_cast<PostJobThread*>(pi->GetPostThread())->AddPar(reinterpret_cast<FileInfo*>(p2), a2 != 0) ? 1 : 0;
#endif
			return 0;
		}
		if (a1 == 12)
		{
			PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
#ifdef DISABLE_PARCHECK
			if (a2 == 3) return 0;
#endif
			PostJobThread::StartPost(pi, a2);
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
