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
#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/ec.h>
#include <openssl/x509v3.h>

namespace OpenSSL
{
	using X509StorePtr = std::unique_ptr<X509_STORE, decltype(&X509_STORE_free)>;
	using X509Ptr = std::unique_ptr<X509, decltype(&X509_free)>;
	using EVPCipherCtxPtr = std::unique_ptr<EVP_CIPHER_CTX, decltype(&EVP_CIPHER_CTX_free)>;
	using SSLCtxPtr = std::unique_ptr<SSL_CTX, decltype(&SSL_CTX_free)>;
	using SSLSessionPtr = std::unique_ptr<SSL, decltype(&SSL_free)>;

	inline void StopSSLThread()
	{
#if !defined(DISABLE_TLS) && !defined(LIBRESSL_VERSION_NUMBER)
		OPENSSL_thread_stop();
#endif
	}
}
#endif

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

class Debuggable;
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

	void InitOptions()
	{
		int flags = (g_Options->GetServerMode() ? 1 : 0) |
			(g_Options->GetRemoteClientMode() ? 2 : 0) |
			(g_Options->GetDirectClientMode() ? 4 : 0);
		nzbget_rs_log_ctl(m_rs, 1, flags, 0, OptRs(), nullptr, nullptr);
	}

	void RegisterDebuggable(Debuggable* debuggable)
	{
		nzbget_rs_log_ctl(m_rs, 5, 0, reinterpret_cast<uintptr_t>(debuggable), nullptr, nullptr, nullptr);
	}

	void UnregisterDebuggable(Debuggable* debuggable)
	{
		nzbget_rs_log_ctl(m_rs, 6, 0, reinterpret_cast<uintptr_t>(debuggable), nullptr, nullptr, nullptr);
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

class Debuggable
{
public:
	Debuggable() { g_Log->RegisterDebuggable(this); }
	virtual ~Debuggable() { g_Log->UnregisterDebuggable(this); }
protected:
	virtual void LogDebugInfo() = 0;
	friend class Log;
};

inline Log::Log()
{
	g_Log = this;
	auto dbgCb = +[](uintptr_t p) { reinterpret_cast<Debuggable*>(p)->LogDebugInfo(); };
#ifdef DEBUG
	m_rs = nzbget_rs_log_new(1, dbgCb);
#else
	m_rs = nzbget_rs_log_new(0, dbgCb);
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

class ServiceCoordinator;
extern ServiceCoordinator* g_ServiceCoordinator;

class Service
{
public:
	Service();
	virtual ~Service() = default;

	static const int Now = 0;
	static const int Sleep = -1;

protected:
	virtual int ServiceInterval() = 0;
	virtual void ServiceWork() = 0;
	void WakeUp();

	friend class ServiceCoordinator;
};

class ServiceCoordinator : public Thread
{
public:
	ServiceCoordinator() : m_rs(nzbget_rs_service_coord_new()) { debug("Creating ServiceCoordinator"); }
	~ServiceCoordinator() override { debug("Destroying ServiceCoordinator"); nzbget_rs_service_coord_free(m_rs); }

	NzbgetRsServiceCoord* RsHandle() { return m_rs; }
	void Run() override { nzbget_rs_service_coord_ctl(m_rs, 3, this, &HostCb); }
	void Stop() override { Thread::Stop(); nzbget_rs_service_coord_ctl(m_rs, 2, nullptr, nullptr); }

private:
	NzbgetRsServiceCoord* m_rs = nullptr;

	void RegisterService(Service* service) { nzbget_rs_service_coord_ctl(m_rs, 0, service, nullptr); }
	void WakeUp() { nzbget_rs_service_coord_ctl(m_rs, 1, nullptr, nullptr); }

	static int HostCb(void* ctx, void* svc, int op)
	{
		if (op == 0) return static_cast<ServiceCoordinator*>(ctx)->IsStopped() ? 1 : 0;
		if (op == 1) return static_cast<Service*>(svc)->ServiceInterval();
		static_cast<Service*>(svc)->ServiceWork();
		return 0;
	}

	friend class Service;
};

inline Service::Service() { g_ServiceCoordinator->RegisterService(this); }
inline void Service::WakeUp() { g_ServiceCoordinator->WakeUp(); }

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

#ifndef TLSSOCKET_H
#define TLSSOCKET_H

struct SocketDeadline
{
	std::chrono::steady_clock::time_point end;
	std::function<bool()> cancelled;
	bool Stopped() const { return std::chrono::steady_clock::now() >= end || (cancelled && cancelled()); }
	bool Wait(SOCKET socket, bool read, bool write) const
	{
		return nzbget_rs_socket_deadline_wait((int)socket, read ? 1 : 0, write ? 1 : 0,
			const_cast<SocketDeadline*>(this),
			[](void* ctx) -> int64_t {
				const auto* self = static_cast<const SocketDeadline*>(ctx);
				return self->Stopped() ? -1 : (int64_t)std::chrono::duration_cast<std::chrono::milliseconds>(
					self->end - std::chrono::steady_clock::now()).count();
			}) != 0;
	}
};

#ifndef DISABLE_TLS

#include <openssl/sslerr.h>

class TlsSocket
{
public:
	TlsSocket(
		SOCKET socket,
		bool isClient,
		const char* host,
		const char* certFile,
		const char* keyFile,
		const char* cipher,
		int certVerifLevel)
		: m_socket(socket)
		, m_host(host ? host : "")
		, m_certFile(certFile ? certFile : "")
		, m_keyFile(keyFile ? keyFile : "")
		, m_cipher(cipher ? cipher : "")
		, m_isClient(isClient)
		, m_retCode(0)
		, m_certVerifLevel(certVerifLevel)
		, m_context{ nullptr, &SSL_CTX_free }
		, m_session{ nullptr, &SSL_free } {}

	virtual ~TlsSocket()
	{
		Close();
		ERR_clear_error();
	}

	static void Init()
	{
		debug("Initializing TLS library");

		if (OPENSSL_init_ssl(0, nullptr) == 0)
		{
			error("Could not initialize the OpenSSL library");
		}
	}

	static void InitOptions(const char* certStore)
	{
		if (Util::EmptyStr(certStore)) 
			return;

		InitX509Store(certStore);
	}

	static void Final()
	{
		m_X509Store.reset();
		OPENSSL_cleanup();
	}

	bool Start()
	{
		m_context.reset(SSL_CTX_new(TLS_method()));

		if (!m_context)
		{
			ReportError("Could not create TLS context", false);
			return false;
		}

		if (!m_certFile.empty() && !m_keyFile.empty())
		{
			if (!SSL_CTX_use_certificate_chain_file(m_context.get(), m_certFile.c_str()))
			{
				ReportError("Could not load certificate file", false);
				Close();
				return false;
			}
			if (!SSL_CTX_use_PrivateKey_file(m_context.get(), m_keyFile.c_str(), SSL_FILETYPE_PEM))
			{
				ReportError("Could not load key file", false);
				Close();
				return false;
			}
		}

		if (m_isClient && m_X509Store)
		{
			SSL_CTX_set1_cert_store(m_context.get(), m_X509Store.get());
			SSL_CTX_set_verify(m_context.get(),
				m_certVerifLevel > (int)Options::ECertVerifLevel::cvNone ? SSL_VERIFY_PEER : SSL_VERIFY_NONE,
				nullptr);
		}

		m_session.reset(SSL_new(m_context.get()));
		if (!m_session)
		{
			ReportError("Could not create TLS session", false);
			Close();
			return false;
		}

		if (!SetCipherSuite(m_cipher))
		{
			ReportError("Could not set cipher suite for SSL/TLS connection", false);
			Close();
			return false;
		}

		if (m_isClient && !m_host.empty() &&
			!SSL_set_tlsext_host_name(m_session.get(), m_host.c_str()))
		{
			ReportError("Could not set host name for TLS");
			Close();
			return false;
		}

		if (!SSL_set_fd(m_session.get(), static_cast<int>(m_socket)))
		{
			ReportError("Could not set the file descriptor for TLS");
			Close();
			return false;
		}

		int ec;
		do
		{
			if (m_deadline && m_deadline->Stopped()) { Close(); return false; }
			ec = m_isClient ? SSL_connect(m_session.get()) : SSL_accept(m_session.get());
		} while (ec < 1 && RetryIo(ec));
		bool shouldVerify = m_X509Store && m_certVerifLevel > (int)Options::ECertVerifLevel::cvNone;
		if (ec < 1)
		{
			if (m_deadline)
			{
				unsigned long code = ERR_peek_last_error();
				int reason = ERR_GET_REASON(code);
				if (ERR_GET_LIB(code) == ERR_LIB_SSL &&
					(reason == SSL_R_SSLV3_ALERT_HANDSHAKE_FAILURE ||
					 reason == SSL_R_TLSV1_ALERT_PROTOCOL_VERSION ||
					 reason == SSL_R_NO_SHARED_CIPHER ||
					 reason == SSL_R_UNSUPPORTED_PROTOCOL))
					m_probeFailure = 0; // Explicit rejection of this single-suite handshake.
			}
			if (m_isClient && shouldVerify)
			{
				long verifyRes = SSL_get_verify_result(m_session.get());
				if (verifyRes != X509_V_OK)
				{
					m_probeFailure = -4;
					PrintError(BString<1024>("TLS certificate verification failed for %s: %s. For more info visit https://github.com/xbmc4lyfe/RustyGet",
						m_host.c_str(), X509_verify_cert_error_string(verifyRes)));
					Close();
					return false;
				}
			}

			ReportError(BString<1024>("TLS handshake failed for %s", m_host.c_str()));
			Close();
			return false;
		}

		if (m_isClient && shouldVerify && !ValidateCert())
		{
			m_probeFailure = -4;
			Close();
			return false;
		}

		const SSL_CIPHER* currentCipher = SSL_get_current_cipher(m_session.get());
		if (currentCipher != nullptr)
		{
			debug("TLS connection established: %s using %s", SSL_get_version(m_session.get()), SSL_CIPHER_get_name(currentCipher));
		}

		m_connected = true;
		return true;
	}

	void Close()
	{
		if (m_session && m_connected && !m_deadline)
		{
			if (SSL_shutdown(m_session.get()) == 0)
			{
				// The bidirectional shutdown is not yet complete. Call SSL_shutdown again.
				SSL_shutdown(m_session.get());
			}
		}

		m_context.reset();
		m_session.reset();
	}

	int Send(const char* buffer, int size)
	{
		do
		{
			if (m_deadline && m_deadline->Stopped()) return -1;
			m_retCode = SSL_write(m_session.get(), buffer, size);
		} while (m_retCode <= 0 && RetryIo(m_retCode));

		if (m_retCode < 0)
		{
			ReportError(ERR_peek_error() == 0 ?
				"Could not write to TLS-Socket: Connection closed by remote host" :
				"Could not write to TLS-Socket");
			return -1;
		}

		return m_retCode;
	}

	int Recv(char* buffer, int size)
	{
		do
		{
			if (m_deadline && m_deadline->Stopped()) return -1;
			m_retCode = SSL_read(m_session.get(), buffer, size);
		} while (m_retCode <= 0 && RetryIo(m_retCode));

		if (m_retCode < 0)
		{
			ReportError(ERR_peek_error() == 0 ?
				"Could not read from TLS-Socket: Connection closed by remote host" :
				"Could not read from TLS-Socket");
			return -1;
		}

		return m_retCode;
	}

	void BindSuppressErrors(uintptr_t suppressSlot) { m_suppressSlot = suppressSlot; }
	const char* GetNegotiatedCipher() const { return m_session ? SSL_get_cipher_name(m_session.get()) : ""; }
	int GetNegotiatedVersion() const { return m_session ? SSL_version(m_session.get()) : 0; }
	static bool HasCertificateStore() { return static_cast<bool>(m_X509Store); }
	int GetProbeFailure() const { return m_probeFailure; }
	void SetDeadline(std::shared_ptr<const SocketDeadline> deadline) { m_deadline = std::move(deadline); }

protected:
	virtual void PrintError(const char* errMsg)
	{
		error("%s", errMsg);
	}

private:
	static void InitX509Store(std::string_view certStore)
	{
		m_X509Store.reset(X509_STORE_new());

		if (!m_X509Store)
		{
			error("Could not create certificate store");
			return;
		}

		if (!X509_STORE_load_locations(m_X509Store.get(), certStore.data(), nullptr))
		{
			error("Could not load certificate store location from %s", certStore.data());
			return;
		}
	}

	inline static OpenSSL::X509StorePtr m_X509Store{ nullptr, &X509_STORE_free };

	/**
	 * This function configures the allowed ciphers for both TLSv1.3 and
	 * older protocols from a single, combined string. OpenSSL
	 * will parse the string and apply the relevant ciphers to their
	 * respective protocol versions.
	 *
	 * * @note This implementation allows a mixed list, but TLSv1.3 ciphers will
	 * always be preferred during the handshake if the server supports them,
	 * regardless of their order in the string.
	 */
	bool SetCipherSuite(std::string_view cipher)
	{
		if (cipher.empty())
		{
			return true;
		}

		// A single modern suite is an exact pin. Without a protocol bound, setting
		// only a TLS 1.2 list leaves the default TLS 1.3 suites usable (and vice versa).
		// Empty values and custom lists retain their historical negotiation behavior.
		if (cipher.find(':') == std::string_view::npos)
		{
			const auto* ciphers = SSL_get_ciphers(m_session.get());
			for (int i = 0; i < sk_SSL_CIPHER_num(ciphers); ++i)
			{
				const SSL_CIPHER* candidate = sk_SSL_CIPHER_value(ciphers, i);
				const char* candName = SSL_CIPHER_get_name(candidate);
				if (cipher != candName) continue;
				bool tls13 = std::string_view(SSL_CIPHER_get_version(candidate)) == "TLSv1.3";
				int auth = SSL_CIPHER_get_auth_nid(candidate), kx = SSL_CIPHER_get_kx_nid(candidate);
				unsigned flags = (SSL_CIPHER_is_aead(candidate) ? 1u : 0u) |
					(tls13 || auth == NID_auth_rsa || auth == NID_auth_ecdsa ? 2u : 0u) |
					(tls13 || kx == NID_kx_ecdhe || kx == NID_kx_dhe ? 4u : 0u);
				unsigned bits = strstr(candName, "CCM8") ? 64u : (unsigned)SSL_CIPHER_get_bits(candidate, nullptr);
				if (!nzbget_rs_tls_cipher_pin(tls13 ? 13 : 12, bits, flags)) break;
				int version = tls13 ? TLS1_3_VERSION : TLS1_2_VERSION;
				int minimum = SSL_get_min_proto_version(m_session.get());
				int maximum = SSL_get_max_proto_version(m_session.get());
				if ((minimum && version < minimum) || (maximum && version > maximum)) return false;
				if (!SSL_set_min_proto_version(m_session.get(), version) || !SSL_set_max_proto_version(m_session.get(), version)) return false;
				return tls13 ? SSL_set_ciphersuites(m_session.get(), m_cipher.c_str()) == 1 :
					SSL_set_cipher_list(m_session.get(), m_cipher.c_str()) == 1;
			}
		}

		bool tls13 = SSL_set_ciphersuites(m_session.get(), cipher.data());
		bool legacy = SSL_set_cipher_list(m_session.get(), cipher.data());

		if (!tls13)
			ERR_clear_error();

		return tls13 || legacy;
	}

	SOCKET m_socket;

	std::string m_host;
	std::string m_certFile;
	std::string m_keyFile;
	std::string m_cipher;

	bool m_isClient;
	uintptr_t m_suppressSlot = 0;
	bool m_connected = false;
	int m_probeFailure = -1;

	int m_retCode;
	int m_certVerifLevel;

	OpenSSL::SSLCtxPtr m_context;
	OpenSSL::SSLSessionPtr m_session;
	std::shared_ptr<const SocketDeadline> m_deadline;

	bool RetryIo(int result)
	{
		if (!m_deadline) return false;
		int error = SSL_get_error(m_session.get(), result);
		if (error != SSL_ERROR_WANT_READ && error != SSL_ERROR_WANT_WRITE)
			return false;
		return m_deadline->Wait(m_socket, error == SSL_ERROR_WANT_READ, error == SSL_ERROR_WANT_WRITE);
	}

	void ReportError(const char* errMsg, bool suppressable = true)
	{
		auto errcode = ERR_get_error();
		do
		{
			char errstr[1024];
			ERR_error_string_n(errcode, errstr, sizeof(errstr));
			errstr[1024 - 1] = '\0';

			if (suppressable && m_suppressSlot != 0 && nzbget_rs_tls_suppressed(m_suppressSlot) != 0)
			{
				debug("%s: %s", errMsg, errstr);
			}
			else if (errcode != 0)
			{
				PrintError(BString<1024>("%s: %s", errMsg, errstr));
			}
			else
			{
				PrintError(errMsg);
			}

			errcode = ERR_get_error();
		} while (errcode);
	}

	bool ValidateCert()
	{
		// verify a server certificate was presented during the negotiation
		OpenSSL::X509Ptr cert{ SSL_get_peer_certificate(m_session.get()), &X509_free };
		if (!cert)
		{
			PrintError(BString<1024>("TLS certificate verification failed for %s: no certificate provided by server. For more info visit https://github.com/xbmc4lyfe/RustyGet",
				m_host.c_str()));
			return false;
		}

		// hostname verification
		if (m_certVerifLevel > (int)Options::ECertVerifLevel::cvMinimal && !m_host.empty() &&
			!X509_check_host(cert.get(), m_host.c_str(), m_host.size(), 0, nullptr))
		{
			const unsigned char* certHost = nullptr;
			// Find the position of the CN field in the Subject field of the certificate
			int common_name_loc = X509_NAME_get_index_by_NID(X509_get_subject_name(cert.get()), NID_commonName, -1);
			if (common_name_loc >= 0)
			{
				// Extract the CN field
				const X509_NAME_ENTRY* common_name_entry = X509_NAME_get_entry(X509_get_subject_name(cert.get()), common_name_loc);
				if (common_name_entry != nullptr)
				{
					// Convert the CN field to a C string
					const ASN1_STRING* common_name_asn1 = X509_NAME_ENTRY_get_data(common_name_entry);
					if (common_name_asn1 != nullptr)
					{
						certHost = ASN1_STRING_get0_data(common_name_asn1);
					}
				}
			}

			PrintError(BString<1024>("TLS certificate verification failed for %s: certificate hostname mismatch (%s). For more info visit https://github.com/xbmc4lyfe/RustyGet",
				m_host.c_str(), certHost ? reinterpret_cast<const char*>(certHost) : "(null)"));
			return false;
		}

		return true;
	}
};

#endif

#endif


inline intptr_t TlsHostCb(void*, int op, int a1, int a2, uintptr_t p1,
	const char* s1, const char* s2, const char* s3, char* buf, int* outInt)
{
	if (op == 0) { debug("%s", s1); return 0; }
	if (op == 1)
	{
		if (a1 != 0) debug("%s", s1);
		else error("%s", s1);
		return 0;
	}
	if (op == 2)
	{
		const auto* dl = reinterpret_cast<const std::shared_ptr<const SocketDeadline>*>(p1);
		return (Thread::IsCurrentThreadStopped() || (dl && *dl && (*dl)->Stopped())) ? 1 : 0;
	}
	if (op == 3)
	{
		const auto* dl = reinterpret_cast<const std::shared_ptr<const SocketDeadline>*>(p1);
		return (dl && *dl && (*dl)->Wait(static_cast<SOCKET>(a1), a2 != 0, outInt && *outInt != 0)) ? 1 : 0;
	}
#ifndef DISABLE_TLS
	if (op == 4)
	{
		const auto* ptrs = reinterpret_cast<const uintptr_t*>(p1);
		auto* sock = new TlsSocket(static_cast<SOCKET>(a1), (a2 & 1) != 0,
			s1 ? s1 : "", s2 ? s2 : "", s3 ? s3 : "", buf ? buf : "",
			static_cast<unsigned int>((a2 >> 2) & 0xff));
		sock->BindSuppressErrors(ptrs[1]);
		const auto* dl = reinterpret_cast<const std::shared_ptr<const SocketDeadline>*>(ptrs[0]);
		if (dl && *dl) sock->SetDeadline(*dl);
		bool started = sock->Start();
		if (outInt) { outInt[0] = started ? 1 : 0; outInt[1] = sock->GetProbeFailure(); }
		return reinterpret_cast<intptr_t>(sock);
	}
	if (op == 5) return reinterpret_cast<TlsSocket*>(p1)->Send(s1, a1);
	if (op == 6) return reinterpret_cast<TlsSocket*>(p1)->Recv(buf, a1);
	if (op == 7)
	{
		auto* sock = reinterpret_cast<TlsSocket*>(p1);
		if (a1) sock->Close();
		delete sock;
		return 0;
	}
	if (op == 9)
	{
		auto* sock = reinterpret_cast<TlsSocket*>(p1);
		if (outInt) *outInt = sock->GetNegotiatedVersion();
		return reinterpret_cast<intptr_t>(sock->GetNegotiatedCipher());
	}
	if (op == 10) { OpenSSL::StopSSLThread(); return 0; }
#else
	(void)s2; (void)s3; (void)buf;
#endif
	return 0;
}

inline uintptr_t ScriptHostCb(
	void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, const char** strs);

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
		nzbget_rs_build_env_init(flags, Util::VersionRevision(), sslLib, sslVer, &TlsHostCb, &ScriptHostCb,
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

class SegmentData
{
public:
	virtual char* GetData() = 0;
	virtual ~SegmentData() {}
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

	void SetPartNumber(int s) { m_partNumber = s; }
	int GetPartNumber() { return m_partNumber; }
	const char* GetMessageId() { return m_messageId; }
	void SetMessageId(const char* messageId) { m_messageId = messageId; }
	void SetSize(int size) { m_size = size; }
	int GetSize() { return m_size; }
	void AttachSegment(std::unique_ptr<SegmentData> content, int64 offset, int size)
	{
		m_segmentContent = std::move(content);
		m_segmentOffset = offset;
		m_segmentSize = size;
	}
	void DiscardSegment()
	{
		m_segmentContent.reset();
	}
	const char* GetSegmentContent() { return m_segmentContent ? m_segmentContent->GetData() : nullptr; }
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
	std::unique_ptr<SegmentData> m_segmentContent;
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


class DiskState
{
public:
	bool LegacyQueueRead() { return m_legacyQueueRead; }

	bool SaveDownloadQueue(DownloadQueue* downloadQueue, bool saveHistory)
	{
		ExecCtx ctx;
		ctx.queue = downloadQueue;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = 0;
		env.a2 = saveHistory ? 1 : 0;
		return nzbget_rs_diskstate_exec(15, &env) != 0;
	}

	bool LoadDownloadQueue(DownloadQueue* downloadQueue)
	{
		ExecCtx ctx;
		ctx.queue = downloadQueue;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		int legacy = 0;
		env.out_flag = &legacy;
		bool ok = nzbget_rs_diskstate_exec(16, &env) != 0;
		if (legacy) m_legacyQueueRead = true;
		return ok;
	}

	bool SaveDownloadProgress(DownloadQueue* downloadQueue)
	{
		ExecCtx ctx;
		ctx.queue = downloadQueue;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = 1;
		return nzbget_rs_diskstate_exec(15, &env) != 0;
	}

	bool SaveFile(FileInfo* fileInfo, bool replace = false)
	{
		ExecCtx ctx;
		ctx.targetFile = fileInfo;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = 0;
		env.a2 = replace ? 1 : 0;
		return nzbget_rs_diskstate_exec(13, &env) != 0;
	}

	bool LoadArticles(FileInfo* fileInfo)
	{
		return LoadFile(fileInfo, false, true);
	}

	bool LoadFile(FileInfo* fileInfo, bool fileSummary, bool articles)
	{
		ExecCtx ctx;
		ctx.targetFile = fileInfo;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = 0;
		env.a2 = fileSummary ? 1 : 0;
		env.a3 = articles ? 1 : 0;
		return nzbget_rs_diskstate_exec(14, &env) != 0;
	}

	bool SaveFileState(FileInfo* fileInfo, bool completed, bool replace = false)
	{
		ExecCtx ctx;
		ctx.targetFile = fileInfo;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = completed ? 2 : 1;
		env.a2 = replace ? 1 : 0;
		return nzbget_rs_diskstate_exec(13, &env) != 0;
	}

	bool LoadFileState(FileInfo* fileInfo, bool completed)
	{
		ExecCtx ctx;
		ctx.targetFile = fileInfo;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = completed ? 2 : 1;
		return nzbget_rs_diskstate_exec(14, &env) != 0;
	}

	void DiscardFiles(NzbInfo* nzbInfo, bool deleteLog = true)
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			DiscardFile(fileInfo->GetId(), true, true, true);
		}
		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetStatus() != CompletedFile::cfSuccess)
			{
				DiscardFile(completedFile.GetId(), true, true, true);
			}
		}
		if (deleteLog)
		{
			ExecCtx ctx;
			NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
			env.a1 = nzbInfo->GetId();
			nzbget_rs_diskstate_exec(5, &env);
		}
	}

	void DiscardDownloadQueue()
	{
		ExecCtx ctx;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		nzbget_rs_diskstate_exec(0, &env);
	}

	bool DownloadQueueExists()
	{
		ExecCtx ctx;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		return nzbget_rs_diskstate_exec(1, &env) != 0;
	}

	void DiscardFile(int fileId, bool deleteData, bool deletePartialState, bool deleteCompletedState)
	{
		ExecCtx ctx;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = fileId;
		env.a2 = deleteData ? 1 : 0;
		env.a3 = deletePartialState ? 1 : 0;
		env.a4 = deleteCompletedState ? 1 : 0;
		nzbget_rs_diskstate_exec(2, &env);
	}

	void CleanupTempDir(DownloadQueue* downloadQueue)
	{
		ExecCtx ctx;
		ctx.queue = downloadQueue;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		nzbget_rs_diskstate_exec(8, &env);
	}

	bool SaveAllFileInfos(DownloadQueue* downloadQueue)
	{
		ExecCtx ctx;
		ctx.queue = downloadQueue;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = 2;
		return nzbget_rs_diskstate_exec(15, &env) != 0;
	}

	void AppendNzbMessage(int nzbId, Message::EKind kind, const char* text)
	{
		time_t tm = Util::CurrentTime();
		time_t rawtime = tm + (g_Options ? g_Options->GetTimeCorrection() : 0);
		BString<100> timeStr;
		Util::FormatTime(rawtime, timeStr, 100);
		ExecCtx ctx;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = nzbId;
		env.a2 = (int)tm;
		env.a3 = (int)kind;
		env.time_str = *timeStr;
		env.text = text;
		nzbget_rs_diskstate_exec(6, &env);
	}

	void LoadNzbMessages(int nzbId, MessageList* messages)
	{
		ExecCtx ctx;
		ctx.messages = messages;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		env.a1 = nzbId;
		nzbget_rs_diskstate_exec(7, &env);
	}

private:
	bool m_legacyQueueRead = false;

	struct ExecCtx
	{
		DownloadQueue* queue = nullptr;
		MessageList* messages = nullptr;
		FileInfo* targetFile = nullptr;
		NzbInfo* targetNzb = nullptr;
		std::unique_ptr<NzbInfo> pendingNzb;
		std::unique_ptr<FileInfo> stagedFile;
		int snapSuccessArticles = 0;
		int snapFailedArticles = 0;
		int64 snapRemainingSize = 0;
		int64 snapSuccessSize = 0;
		int64 snapFailedSize = 0;
	};

	static void ApplyServerStats(ServerStatList* list, const NzbgetRsDiskServerStat* stats, size_t len);

	static void LogCb(void*, int level, const char* msg)
	{
		switch (level)
		{
		case 0: debug("%s", msg); break;
		case 1: detail("%s", msg); break;
		case 2: info("%s", msg); break;
		case 3: warn("%s", msg); break;
		case 4: error("%s", msg); break;
		}
	}

	static int CtrlCb(void* rawCtx, int cmd, int a1, int a2, int64_t a3, const char* s1)
	{
		auto* c = static_cast<ExecCtx*>(rawCtx);
		switch (cmd)
		{
		case 0: return (int)c->queue->GetQueue()->size();
		case 1: return (int)c->queue->GetHistory()->size();
		case 2: return NzbInfo::LastGenId();
		case 3: return FileInfo::LastGenId();
		case 4:
		{
			int total = 0;
			for (NzbInfo* n : c->queue->GetQueue()) total += (int)n->GetFileList()->size();
			return total;
		}
		case 5: return (int)c->queue->GetQueue()->at(a1)->GetFileList()->size();
		case 6:
			c->targetFile = c->queue->GetQueue()->at(a1)->GetFileList()->at(a2).get();
			return c->targetFile->GetId();
		case 7:
			if (FileInfo* f = c->queue->GetQueue()->at(a1)->GetFileList()->Find(a2))
			{
				c->queue->GetQueue()->at(a1)->GetFileList()->Remove(f);
			}
			return 0;
		case 8:
			for (NzbInfo* n : c->queue->GetQueue())
			{
				if (n->GetFileList()->Find(a1)) return 1;
			}
			return 0;
		case 9:
			c->pendingNzb = std::make_unique<NzbInfo>();
			c->targetNzb = c->pendingNzb.get();
			return 0;
		case 10:
			c->queue->GetQueue()->push_back(std::move(c->pendingNzb));
			c->targetNzb = nullptr;
			return 0;
		case 11:
			if (NzbInfo* n = c->queue->GetQueue()->Find(a1))
			{
				c->targetNzb = n;
				return 1;
			}
			c->pendingNzb = std::make_unique<NzbInfo>();
			c->targetNzb = c->pendingNzb.get();
			return 0;
		case 12:
		{
			if (a1 == (int)HistoryInfo::hkNzb) c->pendingNzb->LeavePostProcess();
			auto hi = std::make_unique<HistoryInfo>(std::move(c->pendingNzb));
			hi->SetTime((time_t)a2);
			c->queue->GetHistory()->push_back(std::move(hi));
			c->targetNzb = nullptr;
			return 0;
		}
		case 13:
		{
			NzbInfo* found = nullptr;
			if (a1 == 0)
			{
				found = c->queue->GetQueue()->Find(a2);
			}
			else
			{
				for (HistoryInfo* hi : c->queue->GetHistory())
				{
					if ((hi->GetKind() == HistoryInfo::hkNzb || hi->GetKind() == HistoryInfo::hkUrl) &&
						hi->GetNzbInfo()->GetId() == a2)
					{
						found = hi->GetNzbInfo();
						break;
					}
				}
			}
			if (found)
			{
				c->targetNzb = found;
				return 1;
			}
			c->pendingNzb = std::make_unique<NzbInfo>();
			c->targetNzb = c->pendingNzb.get();
			return 0;
		}
		case 14:
			for (NzbInfo* n : c->queue->GetQueue())
			{
				if (FileInfo* f = n->GetFileList()->Find(a1))
				{
					c->targetFile = f;
					c->snapSuccessArticles = f->GetSuccessArticles();
					c->snapFailedArticles = f->GetFailedArticles();
					c->snapRemainingSize = f->GetRemainingSize();
					c->snapSuccessSize = f->GetSuccessSize();
					c->snapFailedSize = f->GetFailedSize();
					return 1;
				}
			}
			return 0;
		case 15:
		{
			FileInfo* f = c->targetFile;
			f->GetArticles()->clear();
			f->SetSuccessArticles(c->snapSuccessArticles);
			f->SetFailedArticles(c->snapFailedArticles);
			f->SetRemainingSize(c->snapRemainingSize);
			f->SetSuccessSize(c->snapSuccessSize);
			f->SetFailedSize(c->snapFailedSize);
			return 0;
		}
		case 16:
			c->targetFile->GetArticles()->clear();
			c->targetFile->SetPartialState(a1 ? FileInfo::psCompleted : FileInfo::psPartial);
			return 0;
		case 17:
			c->stagedFile = std::make_unique<FileInfo>();
			c->stagedFile->SetId(a1);
			c->targetFile = c->stagedFile.get();
			return 0;
		case 18:
			if (a1 >= 0) NzbInfo::RaiseIdFloor(a1);
			if (a2 >= 0) FileInfo::RaiseIdFloor(a2);
			NzbInfo::ResetGenId(true);
			FileInfo::ResetGenId(true);
			for (NzbInfo* n : c->queue->GetQueue()) n->UpdateCurrentStats();
			return 0;
		case 19:
			c->messages->emplace_back(a1, (Message::EKind)a2, (time_t)a3, s1 ? s1 : "");
			return 0;
		case 20:
		{
			int cnt = 0;
			for (NzbInfo* n : c->queue->GetQueue()) cnt += n->GetChanged() ? 1 : 0;
			return cnt;
		}
		}
		return 0;
	}

	static void ExportNzbCb(void* rawCtx, int source, size_t idx, void* sinkCtx, void (*sinkFn)(void*, const NzbgetRsDiskNzbExport*))
	{
		auto* c = static_cast<ExecCtx*>(rawCtx);
		NzbgetRsDiskNzbExport exp{};
		NzbInfo* nzbInfo = nullptr;
		if (source == 1)
		{
			HistoryInfo* hi = c->queue->GetHistory()->at(idx).get();
			exp.hist_id = hi->GetId();
			exp.hist_kind = (int)hi->GetKind();
			exp.hist_time = (int)hi->GetTime();
			if (hi->GetKind() == HistoryInfo::hkDup)
			{
				DupInfo* d = hi->GetDupInfo();
				exp.info.par_status = (int)d->GetStatus();
				exp.info.size = d->GetSize();
				exp.info.full_content_hash = d->GetFullContentHash();
				exp.info.filtered_content_hash = d->GetFilteredContentHash();
				exp.info.dupe_score = d->GetDupeScore();
				exp.info.dupe_mode = (int)d->GetDupeMode();
				exp.info.name = d->GetName();
				exp.info.dupe_key = d->GetDupeKey();
				sinkFn(sinkCtx, &exp);
				return;
			}
			if (hi->GetKind() != HistoryInfo::hkNzb && hi->GetKind() != HistoryInfo::hkUrl)
			{
				sinkFn(sinkCtx, &exp);
				return;
			}
			nzbInfo = hi->GetNzbInfo();
		}
		else
		{
			nzbInfo = c->queue->GetQueue()->at(idx).get();
			exp.changed = nzbInfo->GetChanged() ? 1 : 0;
			if (source == 2 && !exp.changed) return;
		}

		NzbgetRsDiskNzbInfoInput& f = exp.info;
		f.id = nzbInfo->GetId();
		f.kind = (int)nzbInfo->GetKind();
		f.url = nzbInfo->GetUrl();
		f.filename = nzbInfo->GetFilename();
		f.dest_dir = nzbInfo->GetDestDir();
		f.final_dir = nzbInfo->GetFinalDir();
		f.hard_link_path = nzbInfo->GetHardLinkPath().c_str();
		f.meta_name = nzbInfo->GetMetaName().c_str();
		f.queued_filename = nzbInfo->GetQueuedFilename();
		f.name = nzbInfo->GetName();
		f.category = nzbInfo->GetCategory();
		f.priority = nzbInfo->GetPriority();
		f.has_post_info = nzbInfo->GetPostInfo() ? 1 : 0;
		f.post_stage = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetStage() : 0;
		f.delete_paused = (int)nzbInfo->GetDeletePaused();
		f.many_dupe_files = (int)nzbInfo->GetManyDupeFiles();
		f.feed_id = nzbInfo->GetFeedId();
		f.par_status = (int)nzbInfo->GetParStatus();
		f.unpack_status = (int)nzbInfo->GetUnpackStatus();
		f.move_status = (int)nzbInfo->GetMoveStatus();
		f.par_rename_status = (int)nzbInfo->GetParRenameStatus();
		f.rar_rename_status = (int)nzbInfo->GetRarRenameStatus();
		f.direct_rename_status = (int)nzbInfo->GetDirectRenameStatus();
		f.delete_status = (int)nzbInfo->GetDeleteStatus();
		f.mark_status = (int)nzbInfo->GetMarkStatus();
		f.url_status = (int)nzbInfo->GetUrlStatus();
		f.unpack_cleaned_up_disk = (int)nzbInfo->GetUnpackCleanedUpDisk();
		f.health_paused = (int)nzbInfo->GetHealthPaused();
		f.add_url_paused = (int)nzbInfo->GetAddUrlPaused();
		f.file_count = nzbInfo->GetFileCount();
		f.parked_file_count = nzbInfo->GetParkedFileCount();
		f.message_count = nzbInfo->GetMessageCount();
		f.min_time = (int)nzbInfo->GetMinTime();
		f.max_time = (int)nzbInfo->GetMaxTime();
		f.par_full = (int)nzbInfo->GetParFull();
		f.force_par_full = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetForceParFull() : 0;
		f.force_repair = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetForceRepair() : 0;
		f.extra_par_blocks = nzbInfo->GetExtraParBlocks();
		f.full_content_hash = nzbInfo->GetFullContentHash();
		f.filtered_content_hash = nzbInfo->GetFilteredContentHash();
		f.size = nzbInfo->GetSize();
		f.success_size = nzbInfo->GetSuccessSize();
		f.failed_size = nzbInfo->GetFailedSize();
		f.par_size = nzbInfo->GetParSize();
		f.par_success_size = nzbInfo->GetParSuccessSize();
		f.par_failed_size = nzbInfo->GetParFailedSize();
		f.total_articles = nzbInfo->GetTotalArticles();
		f.success_articles = nzbInfo->GetSuccessArticles();
		f.failed_articles = nzbInfo->GetFailedArticles();
		f.dupe_key = nzbInfo->GetDupeKey();
		f.dupe_mode = (int)nzbInfo->GetDupeMode();
		f.dupe_score = nzbInfo->GetDupeScore();
		f.dupe_hint = (int)nzbInfo->GetDupeHint();
		f.downloaded_size = nzbInfo->GetDownloadedSize();
		f.download_sec = nzbInfo->GetDownloadSec();
		f.post_total_sec = nzbInfo->GetPostTotalSec();
		f.par_sec = nzbInfo->GetParSec();
		f.repair_sec = nzbInfo->GetRepairSec();
		f.unpack_sec = nzbInfo->GetUnpackSec();
		f.skip_script_processing = (int)nzbInfo->GetSkipScriptProcessing();
		f.skip_disk_write = (int)nzbInfo->GetSkipDiskWrite();
		f.desired_server_id = nzbInfo->GetDesiredServerId();

		std::vector<NzbgetRsDiskCompletedFile> completed;
		completed.reserve(nzbInfo->GetCompletedFiles()->size());
		for (CompletedFile& cf : nzbInfo->GetCompletedFiles())
		{
			completed.push_back({ cf.GetId(), (int)cf.GetStatus(), cf.GetCrc(), (int)cf.GetParFile(),
				cf.GetHash16k(), cf.GetParSetId(), cf.GetFilename(), cf.GetOrigname() });
		}
		std::vector<NzbgetRsDiskParam> params;
		params.reserve(nzbInfo->GetParameters()->size());
		for (NzbParameter& p : nzbInfo->GetParameters()) params.push_back({ p.GetName(), p.GetValue() });
		std::vector<NzbgetRsDiskScriptStatus> scripts;
		scripts.reserve(nzbInfo->GetScriptStatuses()->size());
		for (ScriptStatus& s : nzbInfo->GetScriptStatuses()) scripts.push_back({ (int)s.GetStatus(), s.GetName() });
		std::vector<NzbgetRsDiskServerStat> stats;
		stats.reserve(nzbInfo->GetServerStats()->size());
		for (ServerStat& st : nzbInfo->GetServerStats()) stats.push_back({ st.GetServerId(), st.GetSuccessArticles(), st.GetFailedArticles() });
		std::vector<NzbgetRsDiskFileStub> files;
		files.reserve(nzbInfo->GetFileList()->size());
		for (FileInfo* fi : nzbInfo->GetFileList())
		{
			if (!fi->GetDeleted()) files.push_back({ fi->GetId(), (int)fi->GetPaused(), (int)fi->GetExtraPriority() });
		}

		const StreamRepairJobList* repairJobs = nzbInfo->GetStreamRepairJobs();
		size_t totalHoles = 0;
		for (const StreamRepairJob& job : *repairJobs) totalHoles += job.GetHoles()->size();
		std::vector<NzbgetRsDiskStreamHole> allHoles;
		allHoles.reserve(totalHoles);
		std::vector<NzbgetRsDiskRepairJob> jobs;
		jobs.reserve(repairJobs->size());
		for (const StreamRepairJob& job : *repairJobs)
		{
			const StreamRangeList* holes = job.GetHoles();
			size_t startIdx = allHoles.size();
			for (const StreamRange& h : *holes) allHoles.push_back({ h.Offset, h.Size });
			jobs.push_back({ job.GetFileId(), job.GetDecodedFileSize(), job.GetFailedSize(), job.GetMissedSize(),
				job.GetParFile() ? 1 : 0, job.GetFailedArticles(), job.GetStepsHash(), job.GetFilename(),
				holes->empty() ? nullptr : &allHoles[startIdx], holes->size() });
		}

		std::vector<const char*> archivePtrs;
		if (std::vector<CString>* archives = nzbInfo->GetExtractedArchives())
		{
			archivePtrs.reserve(archives->size());
			for (const CString& arc : *archives) archivePtrs.push_back(*arc ? *arc : "");
		}

		exp.completed = completed.data();
		exp.completed_len = completed.size();
		exp.params = params.data();
		exp.params_len = params.size();
		exp.scripts = scripts.data();
		exp.scripts_len = scripts.size();
		exp.stats = stats.data();
		exp.stats_len = stats.size();
		exp.files = files.data();
		exp.files_len = files.size();
		exp.dupe_recovered_articles = nzbInfo->GetDupeRecoveredArticles();
		exp.dupe_recovered_bytes = nzbInfo->GetDupeRecoveredBytes();
		exp.dupe_recovered_holes = nzbInfo->GetDupeRecoveredHoles();
		exp.jobs = jobs.data();
		exp.jobs_len = jobs.size();
		exp.direct_unpack_status = (int)nzbInfo->GetDirectUnpackStatus();
		exp.unpack_tried = nzbInfo->GetUnpackTried() ? 1 : 0;
		exp.par_repaired = nzbInfo->GetParRepaired() ? 1 : 0;
		exp.last_unpack_status = nzbInfo->GetLastUnpackStatus();
		exp.archives = archivePtrs.data();
		exp.archives_len = archivePtrs.size();
		sinkFn(sinkCtx, &exp);
	}

	static void ApplyNzbCb(void* rawCtx, int mode, const NzbgetRsDiskNzbInfoInput* in, int flags,
		const NzbgetRsDiskCompletedFile* completed, size_t completedLen,
		const NzbgetRsDiskParam* params, size_t paramsLen,
		const NzbgetRsDiskScriptStatus* scripts, size_t scriptsLen,
		const NzbgetRsDiskServerStat* stats, size_t statsLen,
		const NzbgetRsDiskFileStub* files, const int* fileTimes, size_t filesLen,
		const NzbgetRsDiskRepairJob* job)
	{
		auto* c = static_cast<ExecCtx*>(rawCtx);
		if (mode == 5)
		{
			auto d = std::make_unique<DupInfo>();
			d->SetStatus((DupInfo::EStatus)in->par_status);
			d->SetFullContentHash(in->full_content_hash);
			d->SetFilteredContentHash(in->filtered_content_hash);
			d->SetSize(in->size);
			d->SetDupeScore(in->dupe_score);
			d->SetDupeMode((EDupeMode)in->dupe_mode);
			d->SetName(in->name);
			d->SetDupeKey(in->dupe_key);
			d->SetId(in->id);
			auto hi = std::make_unique<HistoryInfo>(std::move(d));
			hi->SetTime((time_t)in->feed_id);
			c->queue->GetHistory()->push_back(std::move(hi));
			return;
		}
		NzbInfo* n = c->targetNzb;
		if (!n) return;
		if (mode == 1)
		{
			n->SetDupeRecoveredArticles(in->total_articles);
			n->SetDupeRecoveredBytes(in->size);
			n->SetDupeRecoveredHoles(in->failed_articles);
			return;
		}
		if (mode == 2)
		{
			StreamRangeList holes;
			holes.reserve(job->holes_len);
			for (size_t k = 0; k < job->holes_len; k++) holes.push_back({ job->holes[k].offset, job->holes[k].size });
			n->GetStreamRepairJobs()->emplace_back(job->file_id, job->filename, job->decoded_file_size,
				job->failed_size, job->missed_size, job->failed_articles, job->par_file != 0,
				std::move(holes), job->steps_hash);
			return;
		}
		if (mode == 3)
		{
			n->SetDirectUnpackStatus((NzbInfo::EDirectUnpackStatus)in->par_status);
			n->SetUnpackTried(in->unpack_status != 0);
			n->SetParRepaired(in->move_status != 0);
			n->SetLastUnpackStatus(in->delete_status);
			if (auto* a = n->GetExtractedArchives()) a->clear();
			return;
		}
		if (mode == 4)
		{
			if (auto* a = n->GetExtractedArchives()) a->emplace_back(in->name, flags);
			return;
		}

		n->SetId(in->id);
		n->SetKind((NzbInfo::EKind)in->kind);
		n->SetUrl(in->url);
		n->SetFilename(in->filename);
		n->SetDestDir(in->dest_dir);
		n->SetFinalDir(in->final_dir);
		if (in->hard_link_path) n->SetHardLinkPath(in->hard_link_path);
		if (in->meta_name) n->SetMetaName(in->meta_name);
		n->SetQueuedFilename(in->queued_filename);
		if (in->name) n->SetName(in->name);
		n->SetCategory(in->category);
		n->SetPriority(in->priority);
		n->SetDeletePaused((bool)in->delete_paused);
		n->SetManyDupeFiles((bool)in->many_dupe_files);
		if (in->has_post_info)
		{
			n->EnterPostProcess();
			n->GetPostInfo()->SetStage((PostInfo::EStage)in->post_stage);
		}
		n->SetFeedId(in->feed_id);
		n->SetParStatus((NzbInfo::EParStatus)in->par_status);
		n->SetUnpackStatus((NzbInfo::EPostUnpackStatus)in->unpack_status);
		n->SetMoveStatus((NzbInfo::EMoveStatus)in->move_status);
		n->SetParRenameStatus((NzbInfo::EPostRenameStatus)in->par_rename_status);
		n->SetRarRenameStatus((NzbInfo::EPostRenameStatus)in->rar_rename_status);
		n->SetDirectRenameStatus((NzbInfo::EDirectRenameStatus)in->direct_rename_status);
		n->SetDeleteStatus((NzbInfo::EDeleteStatus)in->delete_status);
		n->SetMarkStatus((NzbInfo::EMarkStatus)in->mark_status);
		if (n->GetKind() == NzbInfo::nkNzb || (NzbInfo::EUrlStatus)in->url_status >= NzbInfo::lsFailed)
		{
			n->SetUrlStatus((NzbInfo::EUrlStatus)in->url_status);
		}
		n->SetUnpackCleanedUpDisk((bool)in->unpack_cleaned_up_disk);
		n->SetHealthPaused((bool)in->health_paused);
		n->SetAddUrlPaused((bool)in->add_url_paused);
		n->SetFileCount(in->file_count);
		n->SetParkedFileCount(in->parked_file_count);
		n->SetMessageCount(in->message_count);
		n->SetMinTime((time_t)in->min_time);
		n->SetMaxTime((time_t)in->max_time);
		if (flags & 1)
		{
			n->SetParFull((bool)in->par_full);
			n->SetExtraParBlocks(in->extra_par_blocks);
			if (n->GetPostInfo())
			{
				n->GetPostInfo()->SetForceParFull((bool)in->force_par_full);
				n->GetPostInfo()->SetForceRepair((bool)in->force_repair);
			}
		}
		n->SetFullContentHash(in->full_content_hash);
		n->SetFilteredContentHash(in->filtered_content_hash);
		n->SetSize(in->size);
		n->SetSuccessSize(in->success_size);
		n->SetFailedSize(in->failed_size);
		n->SetCurrentSuccessSize(in->success_size);
		n->SetCurrentFailedSize(in->failed_size);
		n->SetParSize(in->par_size);
		n->SetParSuccessSize(in->par_success_size);
		n->SetParFailedSize(in->par_failed_size);
		n->SetParCurrentSuccessSize(in->par_success_size);
		n->SetParCurrentFailedSize(in->par_failed_size);
		n->SetTotalArticles(in->total_articles);
		n->SetSuccessArticles(in->success_articles);
		n->SetFailedArticles(in->failed_articles);
		n->SetCurrentSuccessArticles(in->success_articles);
		n->SetCurrentFailedArticles(in->failed_articles);
		n->SetDupeKey(in->dupe_key);
		n->SetDupeMode((EDupeMode)in->dupe_mode);
		n->SetDupeScore(in->dupe_score);
		n->SetDupeHint((NzbInfo::EDupeHint)in->dupe_hint);
		if (flags & 2)
		{
			n->SetDownloadedSize(in->downloaded_size);
			n->SetDownloadSec(in->download_sec);
			n->SetPostTotalSec(in->post_total_sec);
			n->SetParSec(in->par_sec);
			n->SetRepairSec(in->repair_sec);
			n->SetUnpackSec(in->unpack_sec);
		}
		n->GetCompletedFiles()->clear();
		for (size_t i = 0; i < completedLen; i++)
		{
			const auto& cf = completed[i];
			n->GetCompletedFiles()->emplace_back(
				cf.id, cf.filename, cf.origname, (CompletedFile::EStatus)cf.status,
				cf.crc, (bool)cf.par_file, cf.hash16k, cf.par_set_id);
		}
		n->GetParameters()->clear();
		if (flags & 4)
		{
			n->SetSkipScriptProcessing((bool)in->skip_script_processing);
			n->SetSkipDiskWrite((bool)in->skip_disk_write);
			n->SetDesiredServerId(in->desired_server_id);
		}
		for (size_t i = 0; i < paramsLen; i++) n->GetParameters()->SetParameter(params[i].name, params[i].value);
		n->GetScriptStatuses()->clear();
		for (size_t i = 0; i < scriptsLen; i++) n->GetScriptStatuses()->emplace_back(scripts[i].name, (ScriptStatus::EStatus)scripts[i].status);
		ApplyServerStats(n->GetServerStats(), stats, statsLen);
		n->GetCurrentServerStats()->ListOp(n->GetServerStats(), ServerStatList::soSet);
		n->GetFileList()->clear();
		for (size_t i = 0; i < filesLen; i++)
		{
			auto fi = std::make_unique<FileInfo>();
			fi->SetId(files[i].id);
			fi->SetPaused(files[i].paused != 0);
			if (flags & 8) fi->SetTime((time_t)fileTimes[i]);
			fi->SetExtraPriority(files[i].extra_priority != 0);
			fi->SetNzbInfo(n);
			n->GetFileList()->Add(std::move(fi));
		}
		n->GetStreamRepairJobs()->clear();
	}

	static void ExportFileCb(void* rawCtx, int mode, void* sinkCtx, void (*sinkFn)(void*, const NzbgetRsDiskFileExport*))
	{
		FileInfo* fileInfo = static_cast<ExecCtx*>(rawCtx)->targetFile;
		NzbgetRsDiskFileExport exp{};
		exp.file_id = fileInfo->GetId();
		exp.has_articles = fileInfo->GetArticles()->empty() ? 0 : 1;
		exp.articles_len_count = (int)fileInfo->GetArticles()->size();
		exp.force_direct_write = fileInfo->GetForceDirectWrite() ? 1 : 0;
		if (mode == 4)
		{
			exp.info.subject = fileInfo->GetNzbInfo() ? fileInfo->GetNzbInfo()->GetDestDir() : nullptr;
			exp.info.filename = fileInfo->GetFilename();
			exp.info.output_filename = fileInfo->GetOutputFilename().c_str();
			exp.info.output_initialized = fileInfo->GetOutputInitialized() ? 1 : 0;
			sinkFn(sinkCtx, &exp);
			return;
		}
		if (mode == 0 || mode == 1)
		{
			exp.info.subject = fileInfo->GetSubject();
			exp.info.filename = fileInfo->GetFilename();
			exp.info.origname = fileInfo->GetOrigname() ? fileInfo->GetOrigname() : "";
			exp.info.output_filename = fileInfo->GetOutputFilename().c_str();
			exp.info.filename_confirmed = (int)fileInfo->GetFilenameConfirmed();
			exp.info.time = (int)fileInfo->GetTime();
			exp.info.output_initialized = (int)fileInfo->GetOutputInitialized();
			exp.info.size = fileInfo->GetSize();
			exp.info.missed_size = fileInfo->GetMissedSize();
			exp.info.par_file = (int)fileInfo->GetParFile();
			exp.info.total_articles = fileInfo->GetTotalArticles();
			exp.info.missed_articles = fileInfo->GetMissedArticles();

			std::vector<const char*> groups;
			groups.reserve(fileInfo->GetGroups()->size());
			for (CString& g : fileInfo->GetGroups()) groups.push_back(*g);

			std::vector<NzbgetRsDiskArticleInfo> articleEntries;
			if (mode == 0)
			{
				articleEntries.reserve(fileInfo->GetArticles()->size());
				for (ArticleInfo* ai : fileInfo->GetArticles())
				{
					const char* origMsgId = ai->GetDupeOriginalMessageId();
					articleEntries.push_back({ ai->GetPartNumber(), ai->GetSize(),
						(origMsgId && *origMsgId) ? origMsgId : ai->GetMessageId() });
				}
			}
			exp.groups = groups.data();
			exp.groups_len = groups.size();
			exp.articles = articleEntries.data();
			exp.articles_len = articleEntries.size();
			sinkFn(sinkCtx, &exp);
			return;
		}

		bool completed = mode == 3;
		exp.state.success_articles = fileInfo->GetSuccessArticles();
		exp.state.failed_articles = fileInfo->GetFailedArticles();
		exp.state.remaining_size = fileInfo->GetRemainingSize();
		exp.state.success_size = fileInfo->GetSuccessSize();
		exp.state.failed_size = fileInfo->GetFailedSize();
		exp.state.filename = fileInfo->GetFilename();
		exp.state.hash16k = fileInfo->GetHash16k() ? fileInfo->GetHash16k() : "";
		exp.state.par_set_id = fileInfo->GetParSetId() ? fileInfo->GetParSetId() : "";
		exp.state.par_file = (int)fileInfo->GetParFile();
		exp.state.decoded_file_size = fileInfo->GetDecodedFileSize();
		exp.state.output_initialized = (int)fileInfo->GetOutputInitialized();

		std::vector<NzbgetRsDiskServerStat> stats;
		stats.reserve(fileInfo->GetServerStats()->size());
		for (ServerStat& st : fileInfo->GetServerStats())
		{
			stats.push_back({ st.GetServerId(), st.GetSuccessArticles(), st.GetFailedArticles() });
		}
		std::vector<NzbgetRsDiskArticleState> articles;
		articles.reserve(fileInfo->GetArticles()->size());
		for (ArticleInfo* ai : fileInfo->GetArticles())
		{
			int stagedFallback = (!completed && ai->GetStatus() == ArticleInfo::aiFinished &&
				ai->GetDupeFallbackRound() > 0 && ai->GetResultFilename()) ? 1 : 0;
			int savedStatus = stagedFallback != 0 ? (int)ArticleInfo::aiUndefined : (int)ai->GetStatus();
			articles.push_back({ savedStatus, ai->GetSegmentOffset(), ai->GetSegmentSize(), ai->GetCrc(), stagedFallback });
		}
		exp.stats = stats.data();
		exp.stats_len = stats.size();
		exp.article_states = articles.data();
		exp.article_states_len = articles.size();
		sinkFn(sinkCtx, &exp);
	}

	static int ApplyFileCb(void* rawCtx, int mode, int idxOrFlags, int extraI32, int64_t segOffset, uint32_t crc,
		const char* strArg, const NzbgetRsDiskFileInfoInput* finfo, const NzbgetRsDiskArticleInfo* artInfo,
		const NzbgetRsDiskFileStateInput* fstate, const NzbgetRsDiskServerStat* stats, size_t statsLen)
	{
		auto* c = static_cast<ExecCtx*>(rawCtx);
		FileInfo* f = c->targetFile;
		if (mode == 0)
		{
			if (idxOrFlags & 1) f->SetOutputFilename(finfo->output_filename);
			if (extraI32)
			{
				if (*finfo->subject) f->SetSubject(finfo->subject);
				if (*finfo->filename) f->SetFilename(finfo->filename);
				if (*finfo->origname) f->SetOrigname(finfo->origname);
				if (idxOrFlags & 2) f->SetFilenameConfirmed((bool)finfo->filename_confirmed);
				if (idxOrFlags & 4) f->SetTime((time_t)finfo->time);
				if (idxOrFlags & 8) f->SetOutputInitialized((bool)finfo->output_initialized);
				f->SetSize(finfo->size);
				f->SetMissedSize(finfo->missed_size);
				f->SetRemainingSize(finfo->size - finfo->missed_size);
				f->SetParFile((bool)finfo->par_file);
				f->SetTotalArticles(finfo->total_articles);
				f->SetMissedArticles(finfo->missed_articles);
			}
			return 0;
		}
		if (mode == 1)
		{
			if (strArg) f->GetGroups()->push_back(strArg);
			if (artInfo)
			{
				auto ai = std::make_unique<ArticleInfo>();
				ai->SetPartNumber(artInfo->part_number);
				ai->SetSize(artInfo->size);
				ai->SetMessageId(artInfo->message_id);
				f->GetArticles()->push_back(std::move(ai));
			}
			return 0;
		}
		if (mode == 2 || mode == 3)
		{
			while ((size_t)idxOrFlags >= f->GetArticles()->size())
			{
				f->GetArticles()->push_back(std::make_unique<ArticleInfo>());
			}
			ArticleInfo* pa = f->GetArticles()->at(idxOrFlags).get();
			if (mode == 2) return (pa->GetPartNumber() << 1) | (pa->GetResultFilename() ? 1 : 0);
			if (extraI32 >= 2)
			{
				pa->SetSegmentOffset(segOffset);
				pa->SetSegmentSize(fstate->success_articles);
				pa->SetCrc(crc);
			}
			pa->SetDupeFallbackRound(fstate->failed_articles);
			if (strArg) pa->SetResultFilename(strArg);
			pa->SetStatus((ArticleInfo::EStatus)fstate->par_file);
			return 0;
		}
		if (mode == 4)
		{
			f->SetSuccessArticles(fstate->success_articles);
			f->SetFailedArticles(fstate->failed_articles);
			f->SetRemainingSize(fstate->remaining_size);
			f->SetSuccessSize(fstate->success_size);
			f->SetFailedSize(fstate->failed_size);
			if (idxOrFlags & 1) f->SetDecodedFileSize(fstate->decoded_file_size);
			if (idxOrFlags & 2) f->SetFilename(fstate->filename);
			if (idxOrFlags & 4)
			{
				f->SetHash16k(*fstate->hash16k ? fstate->hash16k : nullptr);
				f->SetParFile((bool)fstate->par_file);
			}
			if (idxOrFlags & 8) f->SetParSetId(*fstate->par_set_id ? fstate->par_set_id : nullptr);
			ApplyServerStats(f->GetServerStats(), stats, statsLen);
			f->SetCompletedArticles(extraI32);
			if (idxOrFlags & 16) f->SetOutputInitialized((bool)fstate->output_initialized);
			if (strArg) f->SetOutputFilename(strArg);
		}
		return 0;
	}

	static void AuxCb(void* rawCtx, int op, int, int, uint32_t, const char*, const char*,
		const void*, void* sinkCtx,
		void (*sinkFn)(void*, int, int, int, uint32_t, const char*, const void*, size_t, const void*, size_t))
	{
		if (op != 0) return;
		auto* c = static_cast<ExecCtx*>(rawCtx);
		for (NzbInfo* n : c->queue->GetQueue())
		{
			sinkFn(sinkCtx, 1, n->GetId(), 0, 0, nullptr, nullptr, 0, nullptr, 0);
			for (FileInfo* fi : n->GetFileList()) sinkFn(sinkCtx, 0, fi->GetId(), 0, 0, nullptr, nullptr, 0, nullptr, 0);
			for (CompletedFile& cf : n->GetCompletedFiles()) sinkFn(sinkCtx, 0, cf.GetId(), 0, 0, nullptr, nullptr, 0, nullptr, 0);
		}
		for (HistoryInfo* hi : c->queue->GetHistory())
		{
			if (hi->GetKind() == HistoryInfo::hkNzb || hi->GetKind() == HistoryInfo::hkUrl)
			{
				NzbInfo* n = hi->GetNzbInfo();
				sinkFn(sinkCtx, 1, n->GetId(), 0, 0, nullptr, nullptr, 0, nullptr, 0);
				for (FileInfo* fi : n->GetFileList()) sinkFn(sinkCtx, 0, fi->GetId(), 0, 0, nullptr, nullptr, 0, nullptr, 0);
				for (CompletedFile& cf : n->GetCompletedFiles()) sinkFn(sinkCtx, 0, cf.GetId(), 0, 0, nullptr, nullptr, 0, nullptr, 0);
			}
		}
	}

	static NzbgetRsDiskStateEnv MakeEnv(ExecCtx* ctx)
	{
		NzbgetRsDiskStateEnv env{};
		env.ctx = ctx;
		env.queue_dir = g_Options ? g_Options->GetQueueDir() : nullptr;
		env.temp_dir = g_Options ? g_Options->GetTempDir() : nullptr;
		env.line_ending = LINE_ENDING;
		env.sep = PATH_SEPARATOR;
		env.has_options = g_Options ? 1 : 0;
		env.flush_queue = (g_Options && g_Options->GetFlushQueue()) ? 1 : 0;
		env.direct_write = (g_Options && g_Options->GetDirectWrite()) ? 1 : 0;
		env.continue_partial = (g_Options && g_Options->GetContinuePartial()) ? 1 : 0;
		env.pt_finished = (int)PostInfo::ptFinished;
		env.ns_running = (int)NzbInfo::nsRunning;
		env.ns_none = (int)NzbInfo::nsNone;
		env.now = (int64_t)Util::CurrentTime();
		env.log_cb = LogCb;
		env.ctrl_cb = CtrlCb;
		env.export_nzb_cb = ExportNzbCb;
		env.apply_nzb_cb = ApplyNzbCb;
		env.export_file_cb = ExportFileCb;
		env.apply_file_cb = ApplyFileCb;
		env.aux_cb = AuxCb;
		return env;
	}
};

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
		g_DiskState->AppendNzbMessage(m_id, kind, text);
		m_messageCount.fetch_add(1, std::memory_order_relaxed);
	}

	while (m_messages.size() > (uint32)g_Options->GetLogBuffer())
	{
		m_messages.pop_front();
	}

	m_cachedMessageCount = m_messages.size();
}


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef SERVERPOOL_H
#define SERVERPOOL_H

inline uintptr_t NzbgetCArticleDlHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

class ServerPool : public Debuggable
{
public:
	ServerPool() { InitRsBuildEnv(); m_rs = nzbget_rs_serverpool_new(); }
	~ServerPool() override { nzbget_rs_serverpool_free(m_rs); }
	const NzbgetRsServerPool* RsHandle() const { return m_rs; }

protected:
	void LogDebugInfo() override { nzbget_rs_serverpool_ctl(m_rs, 5, 0, 0, 0, 0); }

private:
	NzbgetRsServerPool* m_rs = nullptr;
};

extern ServerPool* g_ServerPool;

inline void DiskState::ApplyServerStats(ServerStatList* list, const NzbgetRsDiskServerStat* stats, size_t len)
{
	if (!g_ServerPool) return;
	for (size_t i = 0; i < len; i++)
	{
		size_t cursor = 0;
		while (int id = static_cast<int>(nzbget_rs_serverpool_ctl(
			g_ServerPool->RsHandle(), 8, stats[i].server_id, 0, reinterpret_cast<uintptr_t>(&cursor), 0)))
		{
			list->StatOp(id, stats[i].success_articles, stats[i].failed_articles, ServerStatList::soSet);
		}
	}
}

#ifndef NZBGET_TLS_CIPHER_BACKEND_H
#define NZBGET_TLS_CIPHER_BACKEND_H

#if !defined(DISABLE_TLS) && (defined(__APPLE__) || defined(__linux__))
#include <openssl/rand.h>

namespace TlsCipherBackend
{
	namespace Detail
	{
		constexpr int RecordSize = 16384;
		struct Context { std::string snapshot; };
		struct Crypto
		{
			const EVP_CIPHER* cipher = nullptr;
			OpenSSL::EVPCipherCtxPtr encrypt{EVP_CIPHER_CTX_new(), &EVP_CIPHER_CTX_free};
			OpenSSL::EVPCipherCtxPtr decrypt{EVP_CIPHER_CTX_new(), &EVP_CIPHER_CTX_free};
			std::array<unsigned char, 64> key{};
			std::array<unsigned char, 4> prefix{};
			std::array<unsigned char, RecordSize> plaintext{};
			std::array<unsigned char, RecordSize + EVP_MAX_BLOCK_LENGTH> output{};
			std::vector<std::array<unsigned char, RecordSize + EVP_MAX_BLOCK_LENGTH>> ciphertext;
			std::vector<std::array<unsigned char, 16>> tags;
			std::vector<std::array<unsigned char, 12>> nonces;
			uint64_t first = 0;
			int tagSize = 16;
			bool ccm = false;
			bool encryptKeyed = false, decryptKeyed = false;
			~Crypto() { OPENSSL_cleanse(key.data(), key.size()); }
			bool Init(std::string name)
			{
				if (name.ends_with("@8")) { tagSize = 8; name.resize(name.size() - 2); }
				cipher = EVP_get_cipherbyname(name.c_str());
				if (!cipher || !encrypt || !decrypt) return false;
				int mode = EVP_CIPHER_mode(cipher), keySize = EVP_CIPHER_key_length(cipher);
				ccm = mode == EVP_CIPH_CCM_MODE;
				if (mode != EVP_CIPH_GCM_MODE && !ccm && EVP_CIPHER_nid(cipher) != NID_chacha20_poly1305) return false;
				if (keySize <= 0 || keySize > static_cast<int>(key.size()) || RAND_bytes(key.data(), keySize) != 1 || RAND_bytes(prefix.data(), prefix.size()) != 1) return false;
				for (size_t i = 0; i < RecordSize; ++i)
					plaintext[i] = static_cast<unsigned char>((i * 31 + i / 251) & 255);
				return EVP_EncryptInit_ex(encrypt.get(), cipher, nullptr, nullptr, nullptr) == 1 &&
					EVP_DecryptInit_ex(decrypt.get(), cipher, nullptr, nullptr, nullptr) == 1 &&
					EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_SET_IVLEN, 12, nullptr) == 1 &&
					EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_IVLEN, 12, nullptr) == 1;
			}
			bool Prepare(uint64_t sequence, uint32_t count)
			{
				if (count == 0 || count > 64 || sequence > std::numeric_limits<uint64_t>::max() - count) return false;
				first = sequence; ciphertext.resize(count); tags.resize(count); nonces.resize(count);
				for (uint32_t i = 0; i < count; ++i)
				{
					auto& nonce = nonces[i];
					memcpy(nonce.data(), prefix.data(), 4);
					uint64_t seq = sequence + i;
					for (int b = 0; b < 8; ++b)
						nonce[4 + b] = static_cast<unsigned char>((seq >> ((7 - b) * 8)) & 0xff);
					int size = 0, tail = 0;
					unsigned char aad[5]{23, 3, 3, 0x40, static_cast<unsigned char>(tagSize)};
					if (ccm && EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, nullptr) != 1) return false;
					if (EVP_EncryptInit_ex(encrypt.get(), nullptr, nullptr, encryptKeyed ? nullptr : key.data(), nonce.data()) != 1) return false;
					encryptKeyed = true;
					if (ccm && EVP_EncryptUpdate(encrypt.get(), nullptr, &size, nullptr, RecordSize) != 1) return false;
					if (EVP_EncryptUpdate(encrypt.get(), nullptr, &size, aad, sizeof(aad)) != 1 ||
						EVP_EncryptUpdate(encrypt.get(), ciphertext[i].data(), &size, plaintext.data(), RecordSize) != 1 || size != RecordSize ||
						EVP_EncryptFinal_ex(encrypt.get(), ciphertext[i].data() + size, &tail) != 1 || tail != 0 ||
						EVP_CIPHER_CTX_ctrl(encrypt.get(), EVP_CTRL_AEAD_GET_TAG, tagSize, tags[i].data()) != 1) return false;
				}
				if (Record(sequence) != RecordSize) return false;
				tags[0][0] ^= 1;
				bool rejected = Record(sequence) == -1;
				tags[0][0] ^= 1;
				ERR_clear_error();
				return rejected && Record(sequence) == RecordSize;
			}
			int Record(uint64_t sequence)
			{
				if (sequence < first || sequence - first >= ciphertext.size()) return -1;
				size_t i = static_cast<size_t>(sequence - first);
				unsigned char aad[5]{23, 3, 3, 0x40, static_cast<unsigned char>(tagSize)};
				int size = 0, tail = 0;
				if (ccm && EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, tags[i].data()) != 1) return -1;
				if (EVP_DecryptInit_ex(decrypt.get(), nullptr, nullptr, decryptKeyed ? nullptr : key.data(), nonces[i].data()) != 1) return -1;
				decryptKeyed = true;
				if (ccm && EVP_DecryptUpdate(decrypt.get(), nullptr, &size, nullptr, RecordSize) != 1) return -1;
				if (EVP_DecryptUpdate(decrypt.get(), nullptr, &size, aad, sizeof(aad)) != 1 ||
					EVP_DecryptUpdate(decrypt.get(), output.data(), &size, ciphertext[i].data(), RecordSize) != 1 || size != RecordSize) return -1;
				if (!ccm && (EVP_CIPHER_CTX_ctrl(decrypt.get(), EVP_CTRL_AEAD_SET_TAG, tagSize, tags[i].data()) != 1 ||
					EVP_DecryptFinal_ex(decrypt.get(), output.data() + size, &tail) != 1 || tail != 0)) return -1;
				return memcmp(output.data(), plaintext.data(), RecordSize) == 0 ? RecordSize : -1;
			}
		};

		inline int Snapshot(void* context, const unsigned char** data, size_t* length) noexcept
		{
			try
			{
				NzbgetRsBuf buf{};
				if (!nzbget_rs_script_config_exec(1, nullptr, 0, nullptr, 0, nullptr, nullptr, &buf)) return 0;
				auto& value = static_cast<Context*>(context)->snapshot;
				value.assign(buf.data ? buf.data : "", buf.len);
				nzbget_rs_free(buf);
				*data = reinterpret_cast<const unsigned char*>(value.data()); *length = value.size(); return 1;
			}
			catch (...) { return 0; }
		}
		inline int Suites(void*, NzbgetTlsEmitSuite emit, void* context) noexcept
		{
			try
			{
				OpenSSL::SSLCtxPtr tls(SSL_CTX_new(TLS_method()), &SSL_CTX_free);
				if (!tls) return 0;
				int minimum = SSL_CTX_get_min_proto_version(tls.get()), maximum = SSL_CTX_get_max_proto_version(tls.get());
				const auto* suites = SSL_CTX_get_ciphers(tls.get());
				for (int i = 0; i < sk_SSL_CIPHER_num(suites); ++i)
				{
					const auto* suite = sk_SSL_CIPHER_value(suites, i);
					const char* suiteName = SSL_CIPHER_get_name(suite);
					bool tls13 = std::string_view(SSL_CIPHER_get_version(suite)) == "TLSv1.3";
					int version = tls13 ? TLS1_3_VERSION : TLS1_2_VERSION;
					if ((minimum && version < minimum) || (maximum && version > maximum)) continue;
					int auth = SSL_CIPHER_get_auth_nid(suite), kx = SSL_CIPHER_get_kx_nid(suite);
					unsigned flags = (SSL_CIPHER_is_aead(suite) ? 1u : 0u) |
						(tls13 || auth == NID_auth_rsa || auth == NID_auth_ecdsa ? 2u : 0u) |
						(tls13 || kx == NID_kx_ecdhe || kx == NID_kx_dhe ? 4u : 0u);
					const EVP_CIPHER* bulk = EVP_get_cipherbynid(SSL_CIPHER_get_cipher_nid(suite));
					bool hasCcm8 = strstr(suiteName, "CCM8") != nullptr;
					std::string bulkName = bulk ? EVP_CIPHER_name(bulk) : "unavailable";
					if (hasCcm8) bulkName += "@8";
					unsigned bits = hasCcm8 ? 64u : (unsigned)SSL_CIPHER_get_bits(suite, nullptr);
					if (!emit(context, suiteName, bulkName.c_str(), tls13 ? 13 : 12, bits, flags)) return 0;
				}
				return 1;
			}
			catch (...) { return 0; }
		}
		inline void* Prepare(void*, const char* algorithm) noexcept
		{
			try { auto crypto = std::make_unique<Crypto>(); return crypto->Init(algorithm) ? crypto.release() : nullptr; }
			catch (...) { return nullptr; }
		}
		inline int PrepareBatch(void* context, uint64_t first, uint32_t count) noexcept
		{
			try { return static_cast<Crypto*>(context)->Prepare(first, count) ? 1 : 0; } catch (...) { return 0; }
		}
		inline int Record(void* context, uint64_t sequence) noexcept
		{
			try { return static_cast<Crypto*>(context)->Record(sequence); } catch (...) { return -1; }
		}
		inline void FreeCrypto(void* context) noexcept { delete static_cast<Crypto*>(context); ERR_clear_error(); }

		inline int Probe(void*, const NzbgetTlsServer* server, const char* cipher, int protocol, const char* numeric,
			uint64_t milliseconds, NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			try
			{
				if (cancel(cancelContext)) return -6;
				if (milliseconds == 0) return -3;
				if (milliseconds > 10000) return -1;
				if (server->verification && !TlsSocket::HasCertificateStore()) return -4;
				std::shared_ptr<const SocketDeadline> deadline = [&] {
					auto d = std::make_shared<SocketDeadline>();
					d->end = std::chrono::steady_clock::now() + std::chrono::milliseconds(milliseconds);
					d->cancelled = [=] { return cancel(cancelContext) != 0; };
					return d;
				}();
				int expectedVer = protocol == 13 ? TLS1_3_VERSION : TLS1_2_VERSION;
				int result = nzbget_rs_serverpool_probe_tls(server, cipher ? cipher : "", expectedVer,
					numeric ? numeric : "", reinterpret_cast<uintptr_t>(&deadline));
				if (result == -2) return -2;
				if (cancel(cancelContext)) result = -6;
				else if (std::chrono::steady_clock::now() >= deadline->end) result = -3;
				return result;
			}
			catch (...) { ERR_clear_error(); return -1; }
		}
		inline int Commit(void*, const unsigned char* original, size_t originalLength, const unsigned char* replacement, size_t replacementLength,
			NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			return nzbget_rs_script_config_exec(2,
				reinterpret_cast<const char*>(original), originalLength,
				reinterpret_cast<const char*>(replacement), replacementLength,
				cancelContext, cancel, nullptr);
		}
		inline void Destroy(void* context) noexcept { delete static_cast<Context*>(context); }
		inline std::string Response(NzbgetRsBuf value)
		{
			struct Release { NzbgetRsBuf value; ~Release() { nzbget_rs_free(value); } } release{value};
			return std::string(value.data, value.len);
		}
	}

	inline constexpr bool Available() { return true; }
	inline NzbgetTlsBackend Create()
	{
		InitRsBuildEnv();
		return {new Detail::Context(), Detail::Snapshot, Detail::Suites, Detail::Prepare, Detail::PrepareBatch, Detail::Record, Detail::FreeCrypto, Detail::Probe, Detail::Commit, Detail::Destroy};
	}
	inline std::string Start()
	{
		NzbgetTlsBackend io = Create();
		return Detail::Response(nzbget_rs_tls_cipher_start(&io));
	}
	inline std::string Status(const char* id) { return Detail::Response(nzbget_rs_tls_cipher_status(id)); }
	inline std::string Cancel(const char* id) { return Detail::Response(nzbget_rs_tls_cipher_cancel(id)); }
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
	inline NzbgetTlsBackend Create() { return {}; }
}
#endif
#endif

#ifndef STATMETER_H
#define STATMETER_H

class StatMeter;

#endif

#ifndef ARTICLEWRITER_H
#define ARTICLEWRITER_H

class CachedSegmentData final : public SegmentData
{
public:
	CachedSegmentData() {}
	CachedSegmentData(char* data, int size) : m_data(data), m_size(size) {}
	CachedSegmentData(const CachedSegmentData&) = delete;
	CachedSegmentData(CachedSegmentData&& other) :
		m_data(other.m_data), m_size(other.m_size) { other.m_data = nullptr; other.m_size = 0; }
	CachedSegmentData& operator=(CachedSegmentData&& other)
	{
		if (m_size) nzbget_rs_article_dl_exec(nullptr, 112, m_size, 0, reinterpret_cast<uintptr_t>(m_data), 0, nullptr, nullptr, nullptr);
		m_data = other.m_data;
		m_size = other.m_size;
		other.m_data = nullptr;
		other.m_size = 0;
		return *this;
	}
	~CachedSegmentData() override
	{
		if (m_size) nzbget_rs_article_dl_exec(nullptr, 112, m_size, 0, reinterpret_cast<uintptr_t>(m_data), 0, nullptr, nullptr, nullptr);
	}
	char* GetData() override { return m_data; }

private:
	char* m_data = nullptr;
	int m_size = 0;
};

class ArticleCache;
extern ArticleCache* g_ArticleCache;

class ArticleCache final : public Thread
{
public:
	void Run() override { nzbget_rs_article_dl_exec(nullptr, 110, 0, 0, 0, 0, nullptr, NzbgetCArticleDlHost, this); }
	void Stop() override { Thread::Stop(); nzbget_rs_article_dl_exec(nullptr, 113, 0, 0, 0, 0, nullptr, nullptr, nullptr); }
	Guard GuardContent() { return Guard(m_contentMutex); }

private:
	Mutex m_contentMutex;
	std::vector<ArticleInfo*> m_flushArticles;

	friend uintptr_t NzbgetCArticleDlHost(void*, int, int, int, uintptr_t, uintptr_t,
		const char*, const char*, int*, int64_t*, const char**);
};

#endif
#endif

#ifndef NZBFILE_H
#define NZBFILE_H

class NzbFile
{
public:
	NzbFile(const char* fileName, const char* category)
		: m_fileName{ fileName ? fileName : "" }
	{
		debug("Creating NZBFile");

		m_nzbInfo = std::make_unique<NzbInfo>();
		m_nzbInfo->SetFilename(fileName);
		m_nzbInfo->SetCategory(category);
		m_nzbInfo->BuildDestDirName();
	}

	bool Parse()
	{
		std::unique_ptr<NzbgetRsNzbFileParser, void (*)(NzbgetRsNzbFileParser*)> parser(
			nzbget_rs_nzbfile_parser_new(m_fileName.c_str()),
			nzbget_rs_nzbfile_parser_free);

		NzbgetRsCtype ct = RsCtype();
		NzbgetRsNzbFileSummary summary{};
		bool ok = nzbget_rs_nzbfile_parser_parse_file(
			parser.get(),
			g_Options ? g_Options->GetParIgnoreExt() : "",
			&ct,
			nullptr,
			[](void*) -> int
			{
				try
				{
					FileInfo tmp;
					return tmp.GetId();
				}
				catch (...)
				{
					return 0;
				}
			},
			[](const char* subject, const char* filename)
			{
				try
				{
					detail("Extracting a filename from Subject %s", subject);
					detail("Extracted Filename: %s", filename);
				}
				catch (...)
				{
				}
			},
			[](const char* const* filenames, size_t* indices, size_t count)
			{
				try
				{
					std::sort(indices, indices + count,
						[filenames](size_t a, size_t b)
						{
							return strcmp(filenames[a], filenames[b]) > 0;
						});
				}
				catch (...)
				{
				}
			},
			&summary) != 0;

		for (size_t i = 0; i < summary.messages_count; i++)
		{
			int kind = 0;
			const char* text = nullptr;
			if (nzbget_rs_nzbfile_parser_get_message(parser.get(), i, &kind, &text) && text)
			{
				m_nzbInfo->AddMessage(
					kind == 2 ? Message::mkError : Message::mkWarning,
					text);
			}
		}

		if (!ok)
		{
			return false;
		}

		m_nzbInfo->SetManyDupeFiles(summary.many_dupe_files != 0);
		m_nzbInfo->SetFileCount(summary.file_count);
		m_nzbInfo->SetTotalArticles(summary.total_articles);
		m_nzbInfo->SetFailedArticles(summary.failed_articles);
		m_nzbInfo->SetCurrentFailedArticles(summary.cur_failed_articles);
		m_nzbInfo->SetSize(summary.size);
		m_nzbInfo->SetRemainingSize(summary.remaining_size);
		m_nzbInfo->SetFailedSize(summary.failed_size);
		m_nzbInfo->SetCurrentFailedSize(summary.cur_failed_size);
		m_nzbInfo->SetParSize(summary.par_size);
		m_nzbInfo->SetParFailedSize(summary.par_failed_size);
		m_nzbInfo->SetParCurrentFailedSize(summary.par_cur_failed_size);
		m_nzbInfo->SetRemainingParCount(summary.remaining_par_count);
		m_nzbInfo->SetFullContentHash(summary.full_content_hash);
		m_nzbInfo->SetFilteredContentHash(summary.filtered_content_hash);

		for (size_t i = 0; i < summary.kept_files_count; i++)
		{
			NzbgetRsNzbKeptFileView view{};
			if (!nzbget_rs_nzbfile_parser_get_file(parser.get(), i, &view))
			{
				continue;
			}
			std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>(view.id);
			fileInfo->SetNzbInfo(m_nzbInfo.get());
			fileInfo->SetSubject(view.subject ? view.subject : "");
			fileInfo->SetFilename(view.filename ? view.filename : "");
			fileInfo->SetTime(view.time);
			fileInfo->SetSize(view.size);
			fileInfo->SetRemainingSize(view.remaining_size);
			fileInfo->SetMissedSize(view.missed_size);
			fileInfo->SetTotalArticles(view.total_articles);
			fileInfo->SetMissedArticles(view.missed_articles);
			fileInfo->SetParFile(view.par_file != 0);

			for (size_t g = 0; g < view.groups_count; g++)
			{
				fileInfo->GetGroups()->push_back(view.groups_ptrs[g] ? view.groups_ptrs[g] : "");
			}

			ArticleList* articles = fileInfo->GetArticles();
			articles->reserve(view.articles_count);
			for (size_t a = 0; a < view.articles_count; a++)
			{
				std::unique_ptr<ArticleInfo> article = std::make_unique<ArticleInfo>();
				article->SetPartNumber(view.article_parts[a]);
				article->SetSize(view.article_sizes[a]);
				article->SetMessageId(view.article_msg_ids[a] ? view.article_msg_ids[a] : "");
				articles->push_back(std::move(article));
			}

			if (view.cut_short)
			{
				m_cutShortFiles.insert(fileInfo.get());
			}

			m_nzbInfo->GetFileList()->Add(std::move(fileInfo));
		}

		m_nzbInfo->UpdateMinMaxTime();

		if (g_Options && g_Options->GetServerMode())
		{
			for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
			{
				g_DiskState->SaveFile(fileInfo);
				fileInfo->GetArticles()->clear();
			}
		}

		m_password = summary.password ? summary.password : "";
		m_metaName = summary.meta_name ? summary.meta_name : "";
		m_category = summary.category ? summary.category : "";

		if (!m_metaName.empty() && m_nzbInfo)
		{
			m_nzbInfo->SetMetaName(m_metaName);
		}

		return true;
	}

	const std::string& GetCategoryFromFile() const { return m_category; }
	std::unique_ptr<NzbInfo> DetachNzbInfo() { return std::move(m_nzbInfo); }
	const std::string& GetPassword() const { return m_password; }

	const std::set<FileInfo*>& GetCutShortFiles() const { return m_cutShortFiles; }

private:
	static NzbgetRsCtype RsCtype()
	{
#ifdef __GLIBC__
		return {*__ctype_b_loc(), (unsigned short)_ISspace, (unsigned short)_ISdigit,
			reinterpret_cast<const int*>(*__ctype_tolower_loc()), CHAR_MIN < 0};
#else
		static thread_local unsigned short classes[256];
		static thread_local int lower[384];
		for (int c = 0; c < 256; c++)
		{
			classes[c] = (isspace(c) ? 1 : 0) | (isdigit(c) ? 2 : 0);
		}
		for (int c = -128; c < 256; c++)
		{
			lower[c + 128] = c < 0 ? c : tolower(c);
		}
		return {classes, 1, 2, lower + 128, CHAR_MIN < 0};
#endif
	}

	std::unique_ptr<NzbInfo> m_nzbInfo;
	std::string m_fileName;
	std::string m_category;
	std::string m_password;
	std::string m_metaName;
	std::set<FileInfo*> m_cutShortFiles;
};

#endif /* NZBFILE_H */

#endif /* DOWNLOADINFO_H */

#ifndef DUPEARTICLEFALLBACK_H
#define DUPEARTICLEFALLBACK_H

/*
 * Recovers articles missing on all news servers from duplicate collections
 * (same title or dupe-key) found in download queue or history. The failed
 * article is requeued with the message-id of the equivalent article of the
 * duplicate; residual damage from slightly different segment boundaries is
 * left to par-repair. See option <DupeArticleFallback>.
 *
 * Once a file has recovered enough articles from duplicates, it "cuts over":
 * subsequent articles lead with the duplicate instead of failing on the
 * primary first (which would waste a full server sweep per article). The
 * primary remains a revert source in case the duplicate is the one missing
 * that particular article.
 *
 * The duplicate a file leads with rotates as well, independently of the
 * cutover: whenever duplicates are tried at all, the file's lead duplicate is
 * tried first, and when that lead misses enough articles in a row it is
 * demoted and the next duplicate takes over the lead for fresh articles, so a
 * holed lead does not cost a wasted fetch per article when other duplicates
 * could serve them. The rotation is bounded: once every duplicate has led
 * without a single lead success it stops (the duplicates are equally holed
 * and rotating - and logging - would be pure noise) until a lead success
 * resets it.
 *
 * An article's source order is PINNED at its first fallback (PinSources): the
 * donors known at that moment, rotated to the file's lead, primary revert
 * inserted when cut over. The fallback round indexes into that pinned list,
 * so neither a lead rotation nor a queue/history change while the article is
 * in flight can shift the round->source mapping under it (which would skip or
 * repeat sources). The lead is identified by donor nzb-id, never by list
 * position: a lead-round result only counts towards demotion while the pinned
 * slot-0 donor is still the file's lead, so a burst of stale failures cannot
 * cascade-demote donors that were never tried, and a donor's streak can never
 * be charged to another donor even when candidate lists differ per part.
 *
 * All methods must be called within DownloadQueue-lock.
 */
class DupeArticleFallback
{
public:
	class ParDamage
	{
	public:
		static void Forget(int nzbId) { nzbget_rs_dupe_fallback_op(0, nzbId, 0, nullptr); }
	};

	static bool ShouldDeferToPar(NzbInfo* nzbInfo);
};

using ParDamage = DupeArticleFallback::ParDamage;
#endif /* DUPEARTICLEFALLBACK_H */

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
	case 2: {
		if (a1 == 3) return (ctx ? static_cast<ArticleCache*>(ctx) : g_ArticleCache)->IsStopped() ? 1 : 0;
		if (a1 == 5)
		{
			bool flushEverything = a2 != 0;
			static thread_local CString s_infoName;
			FileInfo* found = nullptr;
			{
				GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
				for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
				{
					if (found) break;
					for (FileInfo* fileInfo : nzbInfo->GetFileList())
					{
						if (fileInfo->GetCachedArticles() > 0 &&
							(fileInfo->GetActiveDownloads() == 0 || flushEverything) &&
							(!fileInfo->GetNzbInfo()->GetSkipDiskWrite() || flushEverything))
						{
							found = fileInfo;
							nzbget_rs_article_dl_exec(nullptr, 115, 0, 0, reinterpret_cast<uintptr_t>(found), 0, nullptr, nullptr, nullptr);
							s_infoName.Format("%s%c%s",
								fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, fileInfo->GetFilename());
							break;
						}
					}
				}
			}
			if (!found) return 0;
			strs[0] = *s_infoName;
			return reinterpret_cast<uintptr_t>(found);
		}
		if (a1 == 7)
		{
			FileInfo* f = reinterpret_cast<FileInfo*>(p1);
			static thread_local std::string s_outFn;
			bool outInit = false;
			{
				Guard og = f->GuardOutputFile();
				outInit = f->GetOutputInitialized();
				s_outFn = f->GetOutputFilename();
			}
			{
				Guard cg = g_ArticleCache->GuardContent();
				if (f->GetFlushLocked()) return 0;
				f->SetFlushLocked(true);
				g_ArticleCache->m_flushArticles.clear();
				g_ArticleCache->m_flushArticles.reserve(f->GetArticles()->size());
				for (ArticleInfo* pa : f->GetArticles())
				{
					if (pa->GetSegmentContent()) g_ArticleCache->m_flushArticles.push_back(pa);
				}
			}
			bool directWrite = (g_Options->GetDirectWrite() || f->GetForceDirectWrite()) && outInit;
			bool skipDiskWrite = (f->GetNzbInfo() && f->GetNzbInfo()->GetSkipDiskWrite()) || g_Options->GetSkipWrite();
			ints[0] = directWrite ? 1 : 0;
			ints[1] = skipDiskWrite ? 1 : 0;
			ints[2] = (int)g_ArticleCache->m_flushArticles.size();
			i64s[0] = (int64_t)s_outFn.size();
			i64s[1] = (int64_t)reinterpret_cast<uintptr_t>(f->GetNzbInfo());
			strs[0] = s_outFn.data();
			return 1;
		}
		if (a1 == 8)
		{
			ArticleInfo* pa = g_ArticleCache->m_flushArticles[(size_t)a2];
			i64s[0] = pa->GetSegmentOffset();
			ints[0] = pa->GetSegmentSize();
			strs[0] = pa->GetSegmentContent();
			strs[1] = pa->GetResultFilename();
			return 0;
		}
		if (a1 == 9)
		{
			FileInfo* f = reinterpret_cast<FileInfo*>(p1);
			if (a2 < 0) return (f->GetDeleted() && !f->GetNzbInfo()->GetParking()) ? 1 : 0;
			g_ArticleCache->m_flushArticles[(size_t)a2]->DiscardSegment();
			return 0;
		}
		if (a1 == 10)
		{
			FileInfo* f = reinterpret_cast<FileInfo*>(p1);
			{
				Guard cg = g_ArticleCache->GuardContent();
				f->SetCachedArticles(f->GetCachedArticles() - a2);
				f->SetFlushLocked(false);
			}
			g_ArticleCache->m_flushArticles.clear();
			ints[0] = ((f->GetNzbInfo() && f->GetNzbInfo()->GetSkipDiskWrite()) || g_Options->GetSkipWrite()) ? 1 : 0;
			return 0;
		}
		return 0;
	}
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
				Guard cg = g_ArticleCache->GuardContent();
				ad->m_articleInfo->AttachSegment(
					std::make_unique<CachedSegmentData>(reinterpret_cast<char*>(p1), ints[0]), i64s[0], (int)p2);
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

class HistoryCoordinator;
extern HistoryCoordinator* g_HistoryCoordinator;

class DupeCoordinator {};
extern DupeCoordinator* g_DupeCoordinator;

class HistoryCoordinator : public Service
{
public:
	static_assert(DownloadQueue::eaHistoryDelete == 36 && DownloadQueue::eaHistorySetName == 51);
	static_assert(HistoryInfo::hkNzb == 1 && HistoryInfo::hkUrl == 2 && HistoryInfo::hkDup == 3);

	void DeleteDiskFiles(NzbInfo* nzbInfo)
	{
		NzbgetRsHistoryCoordEnv env = BaseEnv();
		env.p2 = reinterpret_cast<uintptr_t>(nzbInfo);
		env.a1 = g_Options->GetServerMode() ? 1 : 0;
		env.a2 = g_Options->GetNzbCleanupDisk() ? 1 : 0;
		nzbget_rs_history_coord_exec(0, &env);
	}

	void AddToHistory(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		NzbgetRsHistoryCoordEnv env = BaseEnv();
		env.p1 = reinterpret_cast<uintptr_t>(downloadQueue);
		env.p2 = reinterpret_cast<uintptr_t>(nzbInfo);
		nzbget_rs_history_coord_exec(1, &env);
	}

	void HistoryHide(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, int rindex)
	{
		NzbgetRsHistoryCoordEnv env = BaseEnv();
		env.p1 = reinterpret_cast<uintptr_t>(downloadQueue);
		env.p2 = reinterpret_cast<uintptr_t>(historyInfo);
		env.a1 = rindex;
		env.a2 = g_Options->GetServerMode() ? 1 : 0;
		env.a3 = g_Options->GetNzbCleanupDisk() ? 1 : 0;
		nzbget_rs_history_coord_exec(2, &env);
	}

	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action, const char* args)
	{
		NzbgetRsHistoryCoordEnv env = BaseEnv();
		env.p1 = reinterpret_cast<uintptr_t>(downloadQueue);
		env.a1 = static_cast<int>(action);
		env.a2 = g_Options->GetDupeCheck() ? 1 : 0;
		env.a3 = g_Options->GetServerMode() ? 1 : 0;
		env.a4 = g_Options->GetNzbCleanupDisk() ? 1 : 0;
		env.a5 = PATH_SEPARATOR;
		env.a6 = g_Options->GetParCheck() != Options::pcForce ? 1 : 0;
		env.s1 = args;
		env.int_arr = idList ? idList->data() : nullptr;
		env.int_len = idList ? idList->size() : 0;
		return nzbget_rs_history_coord_exec(5, &env) != 0;
	}

	void Redownload(DownloadQueue* downloadQueue, HistoryInfo* historyInfo)
	{
		NzbgetRsHistoryCoordEnv env = BaseEnv();
		env.p1 = reinterpret_cast<uintptr_t>(downloadQueue);
		env.p2 = reinterpret_cast<uintptr_t>(historyInfo);
		env.a5 = PATH_SEPARATOR;
		nzbget_rs_history_coord_exec(4, &env);
	}

protected:
	virtual int ServiceInterval() { return 60 * 60; }

	virtual void ServiceWork()
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbgetRsHistoryCoordEnv env = BaseEnv();
		env.p1 = reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(downloadQueue));
		env.a1 = g_Options->GetKeepHistory();
		env.a2 = g_Options->GetDupeCheck() ? 1 : 0;
		env.a3 = g_Options->GetServerMode() ? 1 : 0;
		env.a4 = g_Options->GetNzbCleanupDisk() ? 1 : 0;
		env.t1 = static_cast<int64_t>(Util::CurrentTime());
		nzbget_rs_history_coord_exec(3, &env);
	}

private:
	static NzbgetRsHistoryCoordEnv BaseEnv()
	{
		NzbgetRsHistoryCoordEnv env{};
		env.log_cb = +[](void*, int level, const char* msg) {
			const char* s = msg ? msg : "";
			if (level == 0) debug("%s", s);
			else if (level == 1) detail("%s", s);
			else if (level == 2) info("%s", s);
			else if (level == 3) warn("%s", s);
			else if (level == 4) error("%s", s);
		};
		env.action_cb = &HistoryCoordinator::ActionCb;
		return env;
	}

	static uintptr_t ActionCb(void* ctx, int sub, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs, uint8_t* bytes, size_t len);
};

class QueueCoordinator : public Thread, public Observer, public Debuggable
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

	NzbInfo* AddNzbFileToQueue(std::unique_ptr<NzbInfo> nzbInfo, NzbInfo* urlInfo, bool addFirst)
	{
		static_assert(sizeof(std::unique_ptr<NzbInfo>) == sizeof(uintptr_t), "unique_ptr layout");
		return reinterpret_cast<NzbInfo*>(nzbget_rs_queue_coord_exec(m_rs, 3, addFirst ? 1 : 0, 0,
			reinterpret_cast<uintptr_t>(&nzbInfo), reinterpret_cast<uintptr_t>(urlInfo), 0,
			nullptr, nullptr, &NzbgetCQueueHost, this));
	}

	void CheckDupeFileInfos(NzbInfo* nzbInfo)
	{
		nzbget_rs_queue_coord_exec(m_rs, 4, 0, 0, reinterpret_cast<uintptr_t>(nzbInfo), 0, 0,
			nullptr, nullptr, &NzbgetCQueueHost, this);
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

protected:
	void LogDebugInfo() override
	{
		nzbget_rs_queue_coord_exec(m_rs, 10, 0, 0, 0, 0, 0, nullptr, nullptr, &NzbgetCQueueHost, this);
	}

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
				g_DiskState->SaveDownloadQueue(this, m_historyChanged);
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
				g_DiskState->SaveDownloadProgress(this);
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
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef PREPOSTPROCESSOR_H
#define PREPOSTPROCESSOR_H

class PrePostProcessor;
extern PrePostProcessor* g_PrePostProcessor;

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2023-2025 Denis <denis@nzbget.com>
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

#ifndef EXTENSION_MANAGER_H
#define EXTENSION_MANAGER_H

extern void ExitProc();
extern int g_ArgumentCount;
extern char* (*g_Arguments)[];
extern char* (*g_EnvironmentVariables)[];

namespace ExtensionManager
{
	class Manager final
	{
	public:
		Manager() { InitRsBuildEnv(); m_rs = nzbget_rs_ext_mgr_new(); }
		~Manager() { nzbget_rs_ext_mgr_free(m_rs); }
		Manager(const Manager&) = delete;
		Manager& operator=(const Manager&) = delete;

	private:
		NzbgetRsExtManager* m_rs = nullptr;
	};
}

class CommandScriptLog;

#ifndef QUEUESCRIPT_H
#define QUEUESCRIPT_H

class QueueScriptCoordinator;
extern QueueScriptCoordinator* g_QueueScriptCoordinator;

class QueueScriptCoordinator
{
public:
	enum EEvent
	{
		qeFileDownloaded, // lowest priority
		qeUrlCompleted,
		qeNzbMarked,
		qeNzbAdded,
		qeNzbNamed,
		qeNzbDownloaded,
		qeNzbDeleted // highest priority
	};

	QueueScriptCoordinator() { InitRsBuildEnv(); m_rs = nzbget_rs_queue_script_new(); }
	~QueueScriptCoordinator() { nzbget_rs_queue_script_free(m_rs); }
	QueueScriptCoordinator(const QueueScriptCoordinator&) = delete;
	QueueScriptCoordinator& operator=(const QueueScriptCoordinator&) = delete;

	static NzbInfo* FindNzbInfo(DownloadQueue* downloadQueue, int nzbId)
	{
		if (NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(nzbId)) return nzbInfo;
		if (HistoryInfo* historyInfo = downloadQueue->GetHistory()->Find(nzbId)) return historyInfo->GetNzbInfo();
		return nullptr;
	}

	void Stop() { nzbget_rs_queue_script_exec(m_rs, 0, nullptr); }

	void EnqueueScript(NzbInfo* nzbInfo, EEvent event)
	{
		if (!nzbInfo || nzbInfo->GetSkipScriptProcessing()) return;
		NzbgetRsQueueScriptCoordEnv env{};
		env.p1 = reinterpret_cast<uintptr_t>(nzbInfo);
		env.a1 = static_cast<int>(event);
		env.a3 = nzbInfo->GetId();
		env.t2 = static_cast<int64_t>(nzbInfo->GetQueueScriptTime());
		env.s1 = nzbInfo->GetCategory();
		env.action_cb = &QueueScriptCoordinator::CoordActionCb;
		nzbget_rs_queue_script_exec(m_rs, 4, &env);
	}

	void CheckQueue()
	{
		if (nzbget_rs_queue_script_exec(m_rs, 6, nullptr) != 0) return;
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbgetRsQueueScriptCoordEnv env{};
		env.p1 = reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(downloadQueue));
		env.action_cb = &QueueScriptCoordinator::CoordActionCb;
		nzbget_rs_queue_script_exec(m_rs, 5, &env);
	}

private:
	NzbgetRsQueueScriptCoord* m_rs;

	static uintptr_t CoordActionCb(void*, int sub, int a1, uintptr_t p1,
		const char* s1, int* ints, const char** strs)
	{
		if (sub == 1)
		{
			NzbParameter* p = reinterpret_cast<NzbInfo*>(p1)->GetParameters()->Find(s1);
			strs[0] = p ? p->GetValue() : nullptr;
			return 1;
		}
		if (sub == 3)
		{
			auto* n = reinterpret_cast<NzbInfo*>(p1);
			if (!n) return 0;
			ints[0] = n->GetSkipScriptProcessing() ? 1 : 0;
			if (ints[0]) return 1;
			ints[1] = n->GetId();
			ints[2] = n->GetPriority();
			ints[3] = n->GetDupeScore();
			ints[4] = static_cast<int>(n->GetDupeMode());
			ints[5] = static_cast<int>(n->GetDeleteStatus());
			ints[6] = static_cast<int>(n->GetUrlStatus());
			ints[7] = static_cast<int>(n->GetMarkStatus());
			ints[8] = static_cast<int>(n->GetParameters()->size());
			strs[0] = n->GetName();
			strs[1] = n->GetFilename();
			strs[2] = n->GetUrl();
			strs[3] = n->GetCategory();
			strs[4] = n->GetDestDir();
			strs[5] = n->GetQueuedFilename();
			strs[6] = n->GetDupeKey();
			return 1;
		}
		if (sub == 4)
		{
			reinterpret_cast<NzbInfo*>(p1)->SetQueueScriptTime(Util::CurrentTime());
			return 1;
		}
		if (sub == 5)
		{
			NzbInfo* nzb = FindNzbInfo(reinterpret_cast<DownloadQueue*>(p1), a1);
			ints[0] = nzb ? static_cast<int>(nzb->GetDeleteStatus()) : 0;
			ints[1] = nzb ? static_cast<int>(nzb->GetMarkStatus()) : 0;
			return reinterpret_cast<uintptr_t>(nzb);
		}
		if (sub == 6)
		{
			NzbParameter& p = (*reinterpret_cast<NzbInfo*>(p1)->GetParameters())[static_cast<size_t>(a1)];
			strs[0] = p.GetName();
			strs[1] = p.GetValue();
			return 1;
		}
		return 0;
	}
};

inline uintptr_t ScriptHostCb(
	void* /*ctx*/, int op, int a1, int a2, uintptr_t p1, uintptr_t /*p2*/,
	const char* s1, const char* s2, int* /*ints*/, const char** strs)
{
	switch (op)
	{
	case 1:
		return g_EnvironmentVariables ? reinterpret_cast<uintptr_t>(*g_EnvironmentVariables) : 0;
	case 5: {
		if (a1 == 0)
		{
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			if (NzbInfo* n = QueueScriptCoordinator::FindNzbInfo(dq, (int)p1))
			{
				n->AddMessage((Message::EKind)a2, s1 ? s1 : "");
				return 1;
			}
			return 0;
		}
		if (a1 == 1)
		{
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			if (NzbInfo* n = QueueScriptCoordinator::FindNzbInfo(dq, (int)p1))
			{
				if (a2 == 1) n->GetParameters()->SetParameter(s1 ? s1 : "", s2 ? s2 : "");
				else if (a2 == 2) n->SetFinalDir(s2 ? s2 : "");
				else if (a2 == 3)
				{
					n->PrintMessage(Message::mkWarning, "%s", s2 ? s2 : "");
					n->SetMarkStatus(NzbInfo::ksBad);
				}
			}
			return 0;
		}
		if (a1 == 2)
		{
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			if (NzbInfo* n = dq->GetQueue()->Find((int)p1))
			{
				n->PrintMessage(Message::mkWarning, "Cancelling download and deleting %s", s1 ? s1 : "");
				n->SetDeleteStatus(NzbInfo::dsBad);
				dq->EditEntry((int)p1, DownloadQueue::eaGroupDelete, nullptr);
			}
			return 0;
		}
		if (a1 == 3 && g_QueueScriptCoordinator)
		{
			g_QueueScriptCoordinator->CheckQueue();
		}
		return 0;
	}
	case 6:
		if (a1 < 0)
		{
			return (g_Arguments && g_ArgumentCount > 0) ? (uintptr_t)g_ArgumentCount : 0;
		}
		strs[0] = (g_Arguments && a1 < g_ArgumentCount) ? (*g_Arguments)[a1] : nullptr;
		return 0;
	case 7:
		ExitProc();
		return 0;
	default:
		return 0;
	}
}

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
		static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange), "StreamRange ABI match");
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
		if (!nzbInfo || DupeArticleFallback::ShouldDeferToPar(nzbInfo)) return;
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
	explicit DirectUnpack(int nzbId) : m_rs(nzbget_rs_direct_unpack_new(nzbId)) {}
	~DirectUnpack() override { nzbget_rs_direct_unpack_free(m_rs); }

	static void StartJob(NzbInfo* nzbInfo)
	{
		DirectUnpack* directUnpack = new DirectUnpack(nzbInfo->GetId());
		directUnpack->SetAutoDestroy(true);
		nzbInfo->SetUnpackThread(directUnpack);
		nzbInfo->SetDirectUnpackStatus(NzbInfo::nsRunning);
		directUnpack->Start();
	}

	bool IsUnpacking() const { return nzbget_rs_direct_unpack_exec(m_rs, 5, 0, 0, nullptr, nullptr) != 0; }
	bool IsWaitingVolume() const { return nzbget_rs_direct_unpack_exec(m_rs, 6, 0, 0, nullptr, nullptr) != 0; }
	void Run() override { nzbget_rs_direct_unpack_exec(m_rs, 0, 0, 0, &HostCb, this); }
	void Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) { nzbget_rs_direct_unpack_exec(m_rs, 1, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), &HostCb, this); }
	void FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo) { nzbget_rs_direct_unpack_exec(m_rs, 2, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(fileInfo), &HostCb, this); }
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) { nzbget_rs_direct_unpack_exec(m_rs, 3, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), &HostCb, this); }
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo) { nzbget_rs_direct_unpack_exec(m_rs, 4, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), &HostCb, this); }

private:
	NzbgetRsDirectUnpack* m_rs = nullptr;
	static bool IsPrePostStopped();

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* /*i64s*/, const char** strs)
	{
		auto* c = static_cast<DirectUnpack*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			if (a1 == 0)
			{
				NzbInfo* n = reinterpret_cast<DownloadQueue*>(p1)->GetQueue()->Find(a2);
				if (!n) return 0;
				NzbParameter* pw = n->GetParameters()->Find("*Unpack:Password");
				ints[0] = pw ? 1 : 0;
				ints[1] = (g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0;
				ints[2] = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
				ints[3] = n->GetUnpackThread() == c ? 1 : 0;
				ints[4] = n->GetPostInfo() ? 1 : 0;
				ints[5] = 0;
				strs[0] = n->GetName();
				strs[1] = n->GetDestDir();
				strs[2] = pw ? pw->GetValue() : nullptr;
				strs[3] = g_Options ? g_Options->GetInterDir() : "";
				if (p2 && s2)
				{
					CString fin = n->BuildFinalDirName();
					reinterpret_cast<void(*)(uintptr_t, const char*)>(reinterpret_cast<uintptr_t>(s2))(p2, fin ? *fin : "");
				}
				return reinterpret_cast<uintptr_t>(n);
			}
			if (a1 == 1)
			{
				FileInfo* fi = reinterpret_cast<FileInfo*>(p1);
				NzbInfo* n = fi->GetNzbInfo();
				ints[0] = fi->GetFailedArticles();
				strs[0] = n ? n->GetName() : "";
				strs[1] = fi->GetFilename();
				return reinterpret_cast<uintptr_t>(n);
			}
			if (a1 == 2)
			{
				NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
				ints[0] = n->GetPostInfo() ? 1 : 0;
				ints[1] = 0;
				strs[0] = n->GetName();
				strs[1] = nullptr;
				return 0;
			}
			if (a1 == 3) { reinterpret_cast<NzbInfo*>(p1)->AddMessage((Message::EKind)a2, s1 ? s1 : ""); return 0; }
			if (a1 == 4)
			{
				NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
				if (n->GetPostInfo()) n->GetPostInfo()->SetProgressLabel(s1 ? s1 : "");
				return 0;
			}
			if (a1 == 5)
			{
				NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
				if (p2 & 2)
				{
					n->SetUnpackSec(n->GetUnpackSec() + a2);
					n->SetPostTotalSec(n->GetPostTotalSec() + a2);
				}
				if ((p2 & 1) && n->GetPostInfo() && !n->GetLiveRepairThread())
					n->GetPostInfo()->SetWorking(false);
				return 0;
			}
			if (a1 == 6)
			{
				NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
				n->SetUnpackThread(nullptr);
				n->SetDirectUnpackStatus(NzbInfo::nsFailure);
				return 0;
			}
			if (a1 == 7)
			{
				NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
				n->SetUnpackThread(nullptr);
				n->SetDirectUnpackStatus((NzbInfo::EDirectUnpackStatus)a2);
				return 0;
			}
			if (a1 == 8)
			{
				NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
				if (n->GetExtractedArchives()) n->GetExtractedArchives()->emplace_back(s1 ? s1 : "");
				return 0;
			}
			return 0;
		}
		case 2:
			if (a1 == 0) return c->IsStopped() ? 1 : 0;
			if (a1 == 1) { c->Thread::Stop(); return 0; }
			if (a1 == 2) return IsPrePostStopped() ? 1 : 0;
			if (a1 == 3)
			{
				const char* m = s1 ? s1 : "";
				if (a2 == 0) info("%s", m);
				else if (a2 == 1) warn("%s", m);
				else if (a2 == 2) error("%s", m);
				else if (a2 == 3) debug("%s", m);
				else if (a2 == 4) detail("%s", m);
			}
			return 0;
		default:
			return 0;
		}
	}
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

	void NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		nzbget_rs_prepost_exec(m_rs, 4, 0, 0, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost);
	}

	void CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		nzbget_rs_prepost_exec(m_rs, 5, 0, 0, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost);
	}

	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		nzbget_rs_prepost_exec(m_rs, 6, 0, 0, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), 0, nullptr, g_QueueCoordinator, &NzbgetCQueueHost);
	}

	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action, const char* args)
	{
		const int* ptr = (idList && !idList->empty()) ? idList->data() : nullptr;
		int len = idList ? (int)idList->size() : 0;
		return nzbget_rs_prepost_exec(m_rs, 7, len, (int)action, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(ptr), 0, args, g_QueueCoordinator, &NzbgetCQueueHost) != 0;
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

inline bool DirectUnpack::IsPrePostStopped()
{
	return g_PrePostProcessor && g_PrePostProcessor->IsStopped();
}
#endif

class Scanner;
extern Scanner* g_Scanner;

class Scanner final : public Service
{
public:
	enum EAddStatus
	{
		asSkipped,
		asSuccess,
		asFailed
	};

	Scanner() : m_rsScanner(nzbget_rs_scanner_new())
	{
		InitRsBuildEnv();
		NzbgetRsScannerEnv env = MakeEnv();
		nzbget_rs_scanner_exec(m_rsScanner, 8, &env, 0, 0, 0);
	}
	~Scanner() override { nzbget_rs_scanner_free(m_rsScanner); }
	Scanner(const Scanner&) = delete;
	Scanner& operator=(const Scanner&) = delete;

	void InitPPParameters(const char* category, NzbParameterList* parameters, bool reset)
	{
		auto paramCb = +[](void* raw, int op, const char* name, const char* val) -> int {
			auto* params = static_cast<NzbParameterList*>(raw);
			if (op == 0) return params->Find(name) != nullptr ? 1 : 0;
			params->SetParameter(name, val);
			return 0;
		};
		NzbgetRsScannerEnv env{};
		env.ctx = reinterpret_cast<void*>(paramCb);
		env.category = category;
		env.parameters = parameters;
		nzbget_rs_scanner_exec(nullptr, 7, &env, reset ? 1 : 0, 0, 0);
	}

	EAddStatus AddArchive(const char* filename, const char* category,
		bool autoCategory, int priority, const char* dupeKey,
		int dupeScore, EDupeMode dupeMode,
		NzbParameterList* parameters, bool addTop, bool addPaused,
		NzbInfo* urlInfo, const char* buffer, int bufSize)
	{
		NzbgetRsScannerEnv env = MakeAddEnv(
			filename, nullptr, category, autoCategory, priority, dupeKey,
			dupeScore, dupeMode, parameters, addTop, addPaused, urlInfo);
		env.buf_ptr = buffer;
		env.buf_len = bufSize;
		return static_cast<EAddStatus>(nzbget_rs_scanner_exec(m_rsScanner, 4, &env, 0, 0, 0));
	}

	EAddStatus AddArchive(const char* filename, const char* category,
		bool autoCategory, int priority, const char* dupeKey,
		int dupeScore, EDupeMode dupeMode,
		NzbParameterList* parameters, bool addTop, bool addPaused,
		NzbInfo* urlInfo, const char* tmpFilename)
	{
		NzbgetRsScannerEnv env = MakeAddEnv(
			filename, tmpFilename, category, autoCategory, priority, dupeKey,
			dupeScore, dupeMode, parameters, addTop, addPaused, urlInfo);
		return static_cast<EAddStatus>(nzbget_rs_scanner_exec(m_rsScanner, 5, &env, 0, 0, 0));
	}

	EAddStatus AddExternalFile(const char* nzbName, const char* category,
		bool autoCategory, int priority, const char* dupeKey,
		int dupeScore, EDupeMode dupeMode,
		NzbParameterList* parameters, bool addTop, bool addPaused,
		NzbInfo* urlInfo, const char* fileName, const char* buffer,
		int bufSize, int* nzbId)
	{
		NzbgetRsScannerEnv env = MakeAddEnv(
			fileName, nzbName, category, autoCategory, priority, dupeKey,
			dupeScore, dupeMode, parameters, addTop, addPaused, urlInfo);
		env.buf_ptr = buffer;
		env.buf_len = bufSize;
		env.out_nzb_id = nzbId;
		return static_cast<EAddStatus>(nzbget_rs_scanner_exec(m_rsScanner, 6, &env, 0, 0, 0));
	}

protected:
	int ServiceInterval() override { return nzbget_rs_scanner_exec(m_rsScanner, 1, nullptr, -1, 0, 0); }

	void ServiceWork() override { nzbget_rs_scanner_exec(m_rsScanner, 2, nullptr, 0, 0, 0); }

private:
	NzbgetRsScanner* m_rsScanner = nullptr;
	NzbParameterList m_workParams;
	std::unique_ptr<NzbFile> m_activeNzb;

	NzbgetRsScannerEnv MakeEnv()
	{
		NzbgetRsScannerEnv env{};
		env.ctx = this;
		env.wake_cb = [](void* ctx) { static_cast<Scanner*>(ctx)->WakeUp(); };
		env.iter_params_cb = [](void* rawParams, void* outCtx, void (*push)(void*, const char*, const char*))
		{
			for (NzbParameter& p : *static_cast<NzbParameterList*>(rawParams))
			{
				push(outCtx, p.GetName(), p.GetValue());
			}
		};
		env.work_params_cb = [](void* ctx, int op, const char* s1, const char* s2)
		{
			auto* self = static_cast<Scanner*>(ctx);
			if (op == 0) self->m_workParams.clear();
			else if (op == 1) self->m_workParams.SetParameter(s1, s2);
			else if (op == 2) self->InitPPParameters(s1, &self->m_workParams, false);
			else if (op == 3) self->InitPPParameters(s1, &self->m_workParams, true);
		};
		env.scan_script_cb = [](void* ctx, const char* fullFilename, void* urlInfo, const char* directory,
			const char* initName, const char* initCat, const char* initKey,
			void* strCtx, void (*setStr)(void*, int, const char*),
			int* priority, int* addTop, int* addPaused, int* dupeScore, int* dupeMode)
		{
			auto* nzbInfo = static_cast<NzbInfo*>(urlInfo);
			if (nzbInfo && nzbInfo->GetSkipScriptProcessing()) return;
			auto* self = static_cast<Scanner*>(ctx);
			struct ScanCtx
			{
				Scanner* self;
				std::string nzbName, nzbCat, dupeKey;
				int* priority; int* addTop; int* addPaused; int* dupeScore; int* dupeMode;
			} sc{self, initName ? initName : "", initCat ? initCat : "", initKey ? initKey : "",
				priority, addTop, addPaused, dupeScore, dupeMode};
			nzbget_rs_scan_scripts_exec(
				fullFilename, nzbInfo ? nzbInfo->GetUrl() : "", directory, &sc,
				[](void* raw, int op, int a1, const char* s1, const char* s2, int* ints, const char** strs) {
					auto* s = static_cast<ScanCtx*>(raw);
					if (op == 0)
					{
						strs[0] = s->nzbName.c_str();
						strs[1] = s->nzbCat.c_str();
						strs[2] = s->dupeKey.c_str();
						ints[0] = *s->priority;
						ints[1] = *s->addTop;
						ints[2] = *s->addPaused;
						ints[3] = *s->dupeScore;
						ints[4] = *s->dupeMode;
						ints[5] = static_cast<int>(s->self->m_workParams.size());
					}
					else if (op == 1)
					{
						NzbParameter& p = s->self->m_workParams[static_cast<size_t>(a1)];
						strs[0] = p.GetName();
						strs[1] = p.GetValue();
					}
					else if (op == 2)
					{
						if (a1 == 1) s->nzbName = s2 ? s2 : "";
						else if (a1 == 2) { s->nzbCat = s2 ? s2 : ""; s->self->InitPPParameters(s->nzbCat.c_str(), &s->self->m_workParams, true); }
						else if (a1 == 3) s->self->m_workParams.SetParameter(s1 ? s1 : "", s2 ? s2 : "");
						else if (a1 == 4) *s->priority = ints[0];
						else if (a1 == 5) *s->addTop = ints[0] != 0 ? 1 : 0;
						else if (a1 == 6) *s->addPaused = ints[0] != 0 ? 1 : 0;
						else if (a1 == 7) s->dupeKey = s2 ? s2 : "";
						else if (a1 == 8) *s->dupeScore = ints[0];
						else if (a1 == 9) *s->dupeMode = static_cast<int>(ints[0] == 0 ? dmScore : (ints[0] == 1 ? dmAll : dmForce));
					}
				});
			setStr(strCtx, 0, sc.nzbName.c_str());
			setStr(strCtx, 1, sc.nzbCat.c_str());
			setStr(strCtx, 2, sc.dupeKey.c_str());
		};
		env.parse_nzb_cb = [](void* ctx, const char* filename, const char* category, const char** outDetectedCat) -> int
		{
			auto* self = static_cast<Scanner*>(ctx);
			self->m_activeNzb = std::make_unique<NzbFile>(filename, category);
			bool ok = self->m_activeNzb->Parse();
			if (outDetectedCat) *outDetectedCat = self->m_activeNzb->GetCategoryFromFile().c_str();
			return ok ? 1 : 0;
		};
		env.commit_nzb_cb = [](void* ctx, int okFlag, const char* queued, const char* autoCat,
			int hasAuto, const char* nzbName, int priority, const char* dupeKey,
			int dupeScore, int dupeMode, int addTop, int addPaused, void* rawUrlInfo) -> int
		{
			auto* self = static_cast<Scanner*>(ctx);
			bool ok = okFlag != 0;
			auto* urlInfo = static_cast<NzbInfo*>(rawUrlInfo);
			std::unique_ptr<NzbInfo> nzbInfo = self->m_activeNzb->DetachNzbInfo();
			nzbInfo->SetQueuedFilename(queued);
			if (hasAuto && autoCat) nzbInfo->SetCategory(autoCat);
			if (!Util::EmptyStr(nzbName))
			{
				nzbInfo->SetName(nullptr);
				nzbInfo->SetFilename(nzbName);
				nzbInfo->BuildDestDirName();
			}
			if (ok)
			{
				for (FileInfo* fileInfo : nzbInfo->GetFileList())
				{
					int sev = nzbget_rs_scanner_exec(
						nullptr, 9, nullptr, fileInfo->GetMissedArticles(), fileInfo->GetTotalArticles(),
						self->m_activeNzb->GetCutShortFiles().count(fileInfo) > 0 ? 1 : 0);
					if (sev > 0)
					{
						nzbInfo->PrintMessage(
							sev == 2 ? Message::mkWarning : Message::mkDetail,
							"Collection %s lists only %i of the %i articles of %s: the missing ones count as failed",
							nzbInfo->GetName(), fileInfo->GetTotalArticles() - fileInfo->GetMissedArticles(),
							fileInfo->GetTotalArticles(), fileInfo->GetFilename());
					}
				}
			}
			nzbInfo->SetDupeKey(dupeKey);
			nzbInfo->SetDupeScore(dupeScore);
			nzbInfo->SetDupeMode(static_cast<EDupeMode>(dupeMode));
			nzbInfo->SetPriority(priority);
			if (urlInfo)
			{
				nzbInfo->SetUrl(urlInfo->GetUrl());
				nzbInfo->SetUrlStatus(urlInfo->GetUrlStatus());
				nzbInfo->SetFeedId(urlInfo->GetFeedId());
				nzbInfo->SetDupeHint(urlInfo->GetDupeHint());
				nzbInfo->SetDesiredServerId(urlInfo->GetDesiredServerId());
				nzbInfo->SetSkipScriptProcessing(urlInfo->GetSkipScriptProcessing());
				nzbInfo->SetSkipDiskWrite(urlInfo->GetSkipDiskWrite());
			}
			if (!self->m_activeNzb->GetPassword().empty())
			{
				nzbInfo->GetParameters()->SetParameter("*Unpack:Password", self->m_activeNzb->GetPassword().c_str());
			}
			nzbInfo->GetParameters()->CopyFrom(&self->m_workParams);
			self->m_activeNzb.reset();
			for (FileInfo* fileInfo : nzbInfo->GetFileList()) fileInfo->SetPaused(addPaused != 0);
			NzbInfo* addedNzb = nullptr;
			if (ok)
			{
				addedNzb = g_QueueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), urlInfo, addTop != 0);
			}
			else if (urlInfo)
			{
				for (Message& message : nzbInfo->GuardCachedMessages())
				{
					urlInfo->AddMessage(message.GetKind(), message.GetText(), false);
				}
			}
			else
			{
				nzbInfo->SetDeleteStatus(NzbInfo::dsScan);
				addedNzb = g_QueueCoordinator->AddNzbFileToQueue(std::move(nzbInfo), urlInfo, addTop != 0);
			}
			return addedNzb ? addedNzb->GetId() : 0;
		};
		return env;
	}

	NzbgetRsScannerEnv MakeAddEnv(
		const char* filename, const char* nzbName, const char* category,
		bool autoCategory, int priority, const char* dupeKey,
		int dupeScore, EDupeMode dupeMode,
		NzbParameterList* parameters, bool addTop, bool addPaused, NzbInfo* urlInfo)
	{
		NzbgetRsScannerEnv env = MakeEnv();
		env.filename = filename;
		env.nzb_name = nzbName;
		env.category = category;
		env.dupe_key = dupeKey;
		env.priority = priority;
		env.dupe_score = dupeScore;
		env.dupe_mode = static_cast<int>(dupeMode);
		env.auto_category = autoCategory ? 1 : 0;
		env.add_top = addTop ? 1 : 0;
		env.add_paused = addPaused ? 1 : 0;
		env.parameters = parameters;
		env.url_info = urlInfo;
		return env;
	}
};

class UrlCoordinator;


#endif /* SCANNER_H */

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef DUPESEARCH_H
#define DUPESEARCH_H
#include <ctime>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


inline uintptr_t HistoryCoordinator::ActionCb(
	void*, int sub, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs, uint8_t* bytes, size_t len)
{
	switch (sub)
	{
		case 0:
			g_DiskState->DiscardFiles(reinterpret_cast<NzbInfo*>(p2), a1 != 0);
			return 0;
		case 1: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			nzb->GetFileList()->clear();
			strs[0] = nzb->GetQueuedFilename() ? nzb->GetQueuedFilename() : "";
			return 0;
		}
		case 2: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			if (Thread* t = nzb->GetLiveRepairThread()) { nzb->SetLiveRepairThread(nullptr); t->Stop(); }
			std::unique_ptr<NzbInfo> oldNzb = dq->GetQueue()->Remove(nzb);
			auto hist = std::make_unique<HistoryInfo>(std::move(oldNzb));
			hist->SetTime(Util::CurrentTime());
			dq->GetHistory()->Add(std::move(hist), true);
			dq->HistoryChanged();
			for (FileInfo* fi : nzb->GetFileList())
			{
				nzb->UpdateCompletedStats(fi);
				nzb->GetCompletedFiles()->emplace_back(fi->GetId(), fi->GetFilename(),
					fi->GetOrigname() ? fi->GetOrigname() : "", CompletedFile::cfNone, 0,
					fi->GetParFile(), fi->GetHash16k() ? fi->GetHash16k() : "",
					fi->GetParSetId() ? fi->GetParSetId() : "");
			}
			ints[0] = static_cast<int>(nzb->GetParStatus());
			ints[1] = static_cast<int>(nzb->GetUnpackStatus());
			ints[2] = static_cast<int>(nzb->GetDeleteStatus());
			ints[3] = nzb->GetParking() ? 1 : 0;
			ints[4] = nzb->GetUnpackCleanedUpDisk() ? 1 : 0;
			ints[5] = static_cast<int>(nzb->GetCompletedFiles()->size());
			i64s[0] = nzb->GetFailedSize();
			i64s[1] = nzb->GetParFailedSize();
			strs[0] = nzb->GetName();
			return 0;
		}
		case 3: {
			auto* cfs = reinterpret_cast<NzbInfo*>(p2)->GetCompletedFiles();
			if (a1 < 0 || static_cast<size_t>(a1) >= cfs->size()) return 0;
			CompletedFile& cf = (*cfs)[static_cast<size_t>(a1)];
			ints[0] = static_cast<int>(cf.GetStatus());
			ints[1] = cf.GetId();
			strs[0] = cf.GetFilename();
			return 1;
		}
		case 4: {
			Message::EKind mk = a1 == 1 ? Message::mkDetail : (a1 == 3 ? Message::mkWarning : Message::mkInfo);
			reinterpret_cast<NzbInfo*>(p2)->PrintMessage(mk, "%s", s1 ? s1 : "");
			return 0;
		}
		case 5: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			nzb->SetParkedFileCount(a1);
			nzb->GetFileList()->clear();
			nzb->SetRemainingParCount(0);
			nzb->SetParking(false);
			if (nzb->GetDirectRenameStatus() == NzbInfo::tsRunning) nzb->SetDirectRenameStatus(NzbInfo::tsFailure);
			if (nzb->GetDirectUnpackStatus() == NzbInfo::nsRunning) nzb->SetDirectUnpackStatus(NzbInfo::nsNone);
			nzb->SetDupeHint(NzbInfo::dhNone);
			return 0;
		}
		case 6:
			ParDamage::Forget(reinterpret_cast<NzbInfo*>(p2)->GetId());
			return 0;
		case 10: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			nzb->GetParameters()->SetParameter("*DupeRecheck", "1");
			auto emit = reinterpret_cast<void(*)(void*, const char*, const char* const*, size_t)>(
				reinterpret_cast<uintptr_t>(s1));
			std::vector<const char*> groups;
			for (CompletedFile& cf : nzb->GetCompletedFiles())
			{
				if ((cf.GetStatus() != CompletedFile::cfPartial && cf.GetStatus() != CompletedFile::cfFailure) || cf.GetId() <= 0) continue;
				bool partial = cf.GetStatus() == CompletedFile::cfPartial;
				FileInfo fi(cf.GetId());
				if (!g_DiskState->LoadFile(&fi, false, true) || (partial && !g_DiskState->LoadFileState(&fi, true))) continue;
				groups.clear();
				for (const CString& g : *fi.GetGroups()) groups.push_back(*g);
				for (ArticleInfo* art : fi.GetArticles())
				{
					if ((!partial || art->GetStatus() == ArticleInfo::aiFailed) && !Util::EmptyStr(art->GetMessageId()))
						emit(reinterpret_cast<void*>(p1), art->GetMessageId(), groups.empty() ? nullptr : groups.data(), groups.size());
				}
			}
			return 0;
		}
		case 12: {
			auto* hist = reinterpret_cast<HistoryInfo*>(p2);
			NzbInfo* nzb = hist->GetNzbInfo();
			ints[0] = static_cast<int>(nzb->GetMarkStatus());
			ints[1] = static_cast<int>(nzb->GetDeleteStatus());
			ints[2] = nzb->IsDupeSuccess() ? 1 : 0;
			strs[0] = hist->GetName();
			return reinterpret_cast<uintptr_t>(nzb);
		}
		case 13: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* hist = reinterpret_cast<HistoryInfo*>(p2);
			NzbInfo* nzb = hist->GetNzbInfo();
			auto dup = std::make_unique<DupInfo>();
			dup->SetId(nzb->GetId());
			dup->SetName(nzb->GetName());
			dup->SetDupeKey(nzb->GetDupeKey());
			dup->SetDupeScore(nzb->GetDupeScore());
			dup->SetDupeMode(nzb->GetDupeMode());
			dup->SetSize(nzb->GetSize());
			dup->SetFullContentHash(nzb->GetFullContentHash());
			dup->SetFilteredContentHash(nzb->GetFilteredContentHash());
			dup->SetStatus(static_cast<DupInfo::EStatus>(a1));
			auto newHist = std::make_unique<HistoryInfo>(std::move(dup));
			newHist->SetTime(hist->GetTime());
			(*dq->GetHistory())[dq->GetHistory()->size() - 1 - static_cast<size_t>(a2)] = std::move(newHist);
			return 0;
		}
		case 14: {
			auto* hl = reinterpret_cast<DownloadQueue*>(p1)->GetHistory();
			if (a1 < 0 || static_cast<size_t>(a1) >= hl->size()) return 0;
			HistoryInfo* hist = (*hl)[hl->size() - 1 - static_cast<size_t>(a1)].get();
			ints[0] = static_cast<int>(hist->GetKind());
			i64s[0] = static_cast<int64_t>(hist->GetTime());
			i64s[1] = hist->GetKind() == HistoryInfo::hkNzb ? static_cast<int64_t>(reinterpret_cast<uintptr_t>(hist->GetNzbInfo())) : 0;
			strs[0] = hist->GetName();
			return reinterpret_cast<uintptr_t>(hist);
		}
		case 15: {
			auto* hl = reinterpret_cast<DownloadQueue*>(p1)->GetHistory();
			size_t pos = a2 != 0 ? (hl->size() - 1 - static_cast<size_t>(a1)) : static_cast<size_t>(a1);
			hl->erase(hl->begin() + static_cast<ptrdiff_t>(pos));
			return 0;
		}
		case 16: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			dq->HistoryChanged();
			dq->Save();
			return 0;
		}
		case 17: {
			HistoryInfo* hist = reinterpret_cast<DownloadQueue*>(p1)->GetHistory()->Find(a1);
			if (hist && hist->GetKind() == HistoryInfo::hkNzb) hist->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
			return 0;
		}
		case 18: {
			auto* hl = reinterpret_cast<DownloadQueue*>(p1)->GetHistory();
			HistoryList::iterator it = a2 != 0 ? hl->Find(reinterpret_cast<HistoryInfo*>(p2)) : hl->end();
			if (a2 == 0)
			{
				for (auto cur = hl->begin(); cur != hl->end(); ++cur)
					if ((*cur)->GetId() == a1) { it = cur; break; }
			}
			if (it == hl->end()) return 0;
			HistoryInfo* hist = (*it).get();
			NzbInfo* nzb = (hist->GetKind() == HistoryInfo::hkNzb || hist->GetKind() == HistoryInfo::hkUrl) ? hist->GetNzbInfo() : nullptr;
			ints[0] = static_cast<int>(it - hl->begin());
			ints[1] = static_cast<int>(hl->size());
			ints[2] = static_cast<int>(hist->GetKind());
			ints[3] = nzb ? static_cast<int>(nzb->GetDeleteStatus()) : 0;
			ints[4] = nzb ? static_cast<int>(nzb->GetParStatus()) : 0;
			ints[5] = nzb ? static_cast<int>(nzb->GetUnpackStatus()) : 0;
			ints[6] = nzb ? nzb->GetParkedFileCount() : 0;
			i64s[0] = static_cast<int64_t>(reinterpret_cast<uintptr_t>(nzb));
			strs[0] = hist->GetName();
			strs[1] = nzb ? nzb->GetDestDir() : "";
			return reinterpret_cast<uintptr_t>(hist);
		}
		case 21: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			if (a1 < 0)
			{
				ints[0] = nzb->GetUnpackCleanedUpDisk() ? 1 : 0;
				ints[1] = nzb->GetPostTotalSec();
				ints[2] = nzb->GetUnpackSec();
				ints[3] = nzb->GetParSec();
				ints[4] = static_cast<int>(nzb->GetMoveStatus());
				ints[5] = static_cast<int>(nzb->GetFileList()->size());
				strs[0] = nzb->GetDestDir();
			}
			else
			{
				strs[0] = (*nzb->GetFileList())[static_cast<size_t>(a1)]->GetFilename();
			}
			return 0;
		}
		case 22: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* hist = reinterpret_cast<HistoryInfo*>(p2);
			NzbInfo* nzb = hist->GetNzbInfo();
			dq->GetQueue()->Add(std::unique_ptr<NzbInfo>(nzb), true);
			hist->DiscardNzbInfo();
			if (!nzb->GetUnpackCleanedUpDisk())
			{
				nzb->SetUnpackStatus(NzbInfo::usNone);
				nzb->SetDirectUnpackStatus(NzbInfo::nsNone);
				nzb->SetParRepaired(false);
				nzb->SetUnpackTried(false);
				nzb->SetLastUnpackStatus(0);
				nzb->GetExtractedArchives()->clear();
				nzb->SetCleanupStatus(NzbInfo::csNone);
				nzb->SetParRenameStatus(NzbInfo::rsNone);
				nzb->SetRarRenameStatus(NzbInfo::rsNone);
				nzb->SetPostTotalSec(ints[1]);
				nzb->SetUnpackSec(0);
				if (ints[0] != 0)
				{
					nzb->SetParStatus(NzbInfo::psNone);
					nzb->SetParSec(0);
					nzb->SetRepairSec(0);
					nzb->SetParFull(false);
				}
			}
			nzb->SetDeleteStatus(NzbInfo::dsNone);
			nzb->SetDeletePaused(false);
			nzb->SetDupeFailoverChecked(-1);
			nzb->SetDeadPickProbed(false);
			nzb->SetDupeAttemptedArticles(0);
			nzb->SetDupeUnsourcedArticles(0);
			nzb->SetDupeFailedRun(0);
			nzb->SetDupeParDeferState(NzbInfo::dpNone);
			nzb->SetMarkStatus(NzbInfo::ksNone);
			nzb->GetScriptStatuses()->clear();
			nzb->SetParkedFileCount(0);
			nzb->SetMoveStatus(static_cast<NzbInfo::EMoveStatus>(ints[2]));
			nzb->SetReprocess(a2 != 0);
			nzb->SetFinalDir("");
			dq->GetHistory()->erase(dq->GetHistory()->begin() + a1);
			return 0;
		}
		case 23: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			if (a1 != 0) g_PrePostProcessor->NzbAdded(dq, nzb);
			else g_PrePostProcessor->NzbDownloaded(dq, nzb);
			DownloadQueue::Aspect aspect = {DownloadQueue::eaNzbReturned, dq, nzb, nullptr};
			dq->Notify(&aspect);
			return 0;
		}
		case 24: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			nzb->BuildDestDirName();
			strs[0] = nzb->GetDestDir();
			return 0;
		}
		case 25: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* hist = reinterpret_cast<HistoryInfo*>(p2);
			NzbInfo* nzb = hist->GetNzbInfo();
			hist->DiscardNzbInfo();
			nzb->SetUrlStatus(NzbInfo::lsNone);
			nzb->SetDeleteStatus(NzbInfo::dsNone);
			if (nzb->GetDupeHint() == NzbInfo::dhNone) nzb->SetDupeHint(NzbInfo::dhRedownloadManual);
			dq->GetQueue()->Add(std::unique_ptr<NzbInfo>(nzb), true);
			dq->GetHistory()->erase(dq->GetHistory()->begin() + a1);
			DownloadQueue::Aspect aspect = {DownloadQueue::eaUrlReturned, dq, nzb, nullptr};
			dq->Notify(&aspect);
			return 0;
		}
		case 26: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			ints[0] = nzb->GetDeletePaused() ? 1 : 0;
			strs[0] = nzb->GetName();
			strs[1] = nzb->GetQueuedFilename();
			strs[2] = nzb->GetDestDir();
			return 0;
		}
		case 27: {
			NzbFile nzbFile(s1 ? s1 : "", "");
			return nzbFile.Parse() ? reinterpret_cast<uintptr_t>(nzbFile.DetachNzbInfo().release()) : 0;
		}
		case 28: {
			std::unique_ptr<NzbInfo> newNzb(reinterpret_cast<NzbInfo*>(p1));
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			for (FileInfo* fi : newNzb->GetFileList()) fi->SetPaused(a1 != 0);
			g_DiskState->DiscardFiles(nzb);
			nzb->SetMoveStatus(NzbInfo::msNone);
			nzb->SetUnpackCleanedUpDisk(false);
			nzb->SetParStatus(NzbInfo::psNone);
			nzb->SetParRenameStatus(NzbInfo::rsNone);
			nzb->SetRarRenameStatus(NzbInfo::rsNone);
			nzb->SetDirectRenameStatus(NzbInfo::tsNone);
			nzb->SetDirectUnpackStatus(NzbInfo::nsNone);
			nzb->SetParRepaired(false);
			nzb->SetUnpackTried(false);
			nzb->SetLastUnpackStatus(0);
			nzb->GetExtractedArchives()->clear();
			nzb->SetHealthPaused(false);
			nzb->SetDownloadedSize(0);
			nzb->SetDownloadSec(0);
			nzb->SetPostTotalSec(0);
			nzb->SetParSec(0);
			nzb->SetRepairSec(0);
			nzb->SetUnpackSec(0);
			nzb->SetExtraParBlocks(0);
			nzb->SetAllFirst(false);
			nzb->SetWaitingPar(false);
			nzb->SetLoadingPar(false);
			nzb->GetCompletedFiles()->clear();
			nzb->GetStreamRepairJobs()->clear();
			nzb->SetDupeRecoveredArticles(0);
			nzb->SetDupeRecoveredBytes(0);
			nzb->SetDupeRecoveredHoles(0);
			nzb->SetDupeParDeferState(NzbInfo::dpNone);
			nzb->SetDupeNoParNoted(false);
			nzb->GetServerStats()->clear();
			nzb->GetCurrentServerStats()->clear();
			nzb->MoveFileList(newNzb.get());
			g_QueueCoordinator->CheckDupeFileInfos(nzb);
			return 0;
		}
		case 30: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			auto fi = std::make_unique<FileInfo>();
			fi->SetId(a1);
			if (!g_DiskState->LoadFile(fi.get(), true, true)) return 0;
			bool stateLoaded = a2 == static_cast<int>(CompletedFile::cfPartial) &&
				g_DiskState->LoadFileState(fi.get(), true);
			fi->SetNzbInfo(nzb);
			ints[0] = stateLoaded ? 1 : 0;
			ints[1] = fi->GetSuccessArticles();
			ints[2] = static_cast<int>(fi->GetPartialState());
			ints[3] = static_cast<int>(fi->GetArticles()->size());
			ints[4] = fi->GetParFile() ? 1 : 0;
			ints[5] = fi->GetTotalArticles();
			ints[6] = fi->GetMissedArticles();
			ints[7] = fi->GetFailedArticles();
			ints[8] = fi->GetCompletedArticles();
			ints[9] = fi->GetSuccessArticles();
			ints[10] = nzb->GetFailedArticles();
			ints[11] = nzb->GetSuccessArticles();
			i64s[0] = fi->GetSize();
			i64s[1] = fi->GetMissedSize();
			i64s[2] = fi->GetFailedSize();
			i64s[3] = fi->GetRemainingSize();
			i64s[4] = fi->GetSuccessSize();
			i64s[5] = nzb->GetFailedSize();
			i64s[6] = nzb->GetSuccessSize();
			i64s[7] = nzb->GetParFailedSize();
			i64s[8] = nzb->GetParSuccessSize();
			strs[0] = fi->GetFilename() ? fi->GetFilename() : "";
			return reinterpret_cast<uintptr_t>(fi.release());
		}
		case 31: {
			auto* fi = reinterpret_cast<FileInfo*>(p2);
			if (a1 == 0) delete fi;
			else fi->SetFilename(s1 ? s1 : "");
			return 0;
		}
		case 32: {
			auto* arts = reinterpret_cast<FileInfo*>(p2)->GetArticles();
			for (size_t i = 0; i < len; ++i)
			{
				ints[i] = static_cast<int>(arts->at(i)->GetStatus());
				ints[len + i] = arts->at(i)->GetSize();
			}
			return 0;
		}
		case 33: {
			std::unique_ptr<FileInfo> fi(reinterpret_cast<FileInfo*>(p1));
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			fi->SetPartialState(static_cast<FileInfo::EPartialState>(a2));
			nzb->GetServerStats()->ListOp(fi->GetServerStats(), ServerStatList::soSubtract);
			nzb->SetFailedSize(i64s[3]);
			nzb->SetSuccessSize(i64s[4]);
			nzb->SetFailedArticles(ints[3]);
			nzb->SetSuccessArticles(ints[4]);
			nzb->SetParFailedSize(i64s[5]);
			nzb->SetParSuccessSize(i64s[6]);
			ArticleList* arts = fi->GetArticles();
			for (size_t i = 0; i < len; ++i)
			{
				if (bytes[i])
				{
					ArticleInfo* pa = arts->at(i).get();
					pa->SetStatus(ArticleInfo::aiUndefined);
					pa->SetCrc(0);
					pa->SetSegmentOffset(0);
					pa->SetSegmentSize(0);
					pa->SetDupeExpectedOffset(-1);
					pa->SetDupeExpectedEnd(-1);
				}
			}
			fi->SetFailedSize(i64s[0]);
			fi->SetRemainingSize(i64s[1]);
			fi->SetSuccessSize(i64s[2]);
			fi->SetFailedArticles(ints[0]);
			fi->SetCompletedArticles(ints[1]);
			fi->SetSuccessArticles(ints[2]);
			g_DiskState->DiscardFile(fi->GetId(), false, true, fi->GetPartialState() != FileInfo::psCompleted);
			if (fi->GetPartialState() == FileInfo::psCompleted) g_DiskState->SaveFileState(fi.get(), true);
			fi->GetArticles()->clear();
			int fid = fi->GetId();
			nzb->GetFileList()->Add(std::move(fi), false);
			nzb->GetCompletedFiles()->erase(nzb->GetCompletedFiles()->begin() + a1);
			return static_cast<uintptr_t>(fid);
		}
		case 34: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			std::set<int> req(ints, ints + len);
			StreamRepairJobList* jobs = nzb->GetStreamRepairJobs();
			jobs->erase(std::remove_if(jobs->begin(), jobs->end(),
				[&req](const StreamRepairJob& job) { return req.count(job.GetFileId()) > 0; }), jobs->end());
			nzb->UpdateCurrentStats();
			if (a1 != 0) nzb->SetHealthPaused(true);
			i64s[0] = nzb->IsDownloadCompleted(true) ? 1 : 0;
			i64s[1] = nzb->GetId();
			return 0;
		}
		case 35:
			reinterpret_cast<DownloadQueue*>(p1)->EditEntry(a1, DownloadQueue::eaGroupPauseExtraPars, nullptr);
			return 0;
		case 36: {
			auto* hist = reinterpret_cast<HistoryInfo*>(p2);
			if (a1 == 0) hist->GetNzbInfo()->GetParameters()->SetParameter(s1 ? s1 : "", s2 ? s2 : "");
			else if (a1 == 1) hist->GetNzbInfo()->SetCategory(s1 ? s1 : "");
			else if (a1 == 2)
			{
				if (hist->GetKind() == HistoryInfo::hkNzb || hist->GetKind() == HistoryInfo::hkUrl)
				{
					std::string prev = hist->GetNzbInfo()->GetName();
					hist->GetNzbInfo()->SetName(s1 ? s1 : "");
					if (hist->GetKind() == HistoryInfo::hkNzb && prev != hist->GetNzbInfo()->GetName())
						return static_cast<uintptr_t>(hist->GetNzbInfo()->GetId());
				}
				else if (hist->GetKind() == HistoryInfo::hkDup) hist->GetDupInfo()->SetName(s1 ? s1 : "");
			}
			else if (a1 == 3)
			{
				hist->GetNzbInfo()->GetParameters()->SetParameter("DupeSearchOwnedKey", nullptr);
				hist->GetNzbInfo()->SetDupeKey(s1 ? s1 : "");
			}
			else if (a1 == 4) hist->GetNzbInfo()->SetDupeScore(a2);
			else if (a1 == 5) hist->GetNzbInfo()->SetDupeMode(static_cast<EDupeMode>(a2));
			else if (a1 == 6) hist->GetNzbInfo()->SetDeleteStatus(static_cast<NzbInfo::EDeleteStatus>(a2));
			else if (a1 == 7) hist->GetDupInfo()->SetDupeKey(s1 ? s1 : "");
			else if (a1 == 8) hist->GetDupInfo()->SetDupeScore(a2);
			else if (a1 == 9) hist->GetDupInfo()->SetDupeMode(static_cast<EDupeMode>(a2));
			return 0;
		}
		default:
			return 0;
	}
}
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
	case 5: {
		if (!g_DiskState) return 0;
		if (a1 == 0) return g_DiskState->DownloadQueueExists() ? 1 : 0;
		if (a1 == 1) return g_DiskState->LoadDownloadQueue(reinterpret_cast<DownloadQueue*>(p1)) ? 1 : 0;
		if (a1 == 2) { g_DiskState->DiscardDownloadQueue(); return 0; }
		if (a1 == 3) return g_DiskState->LegacyQueueRead() ? 1 : 0;
		if (a1 == 4) { g_DiskState->CleanupTempDir(reinterpret_cast<DownloadQueue*>(p1)); return 0; }
		if (a1 == 5) { g_DiskState->SaveAllFileInfos(reinterpret_cast<DownloadQueue*>(p1)); return 0; }
		if (a1 == 6) { g_DiskState->DiscardFiles(reinterpret_cast<NzbInfo*>(p1)); return 0; }
		if (a1 == 7) { g_DiskState->DiscardFile(a2, ints[0] != 0, ints[1] != 0, ints[2] != 0); return 0; }
		if (a1 == 8) { g_DiskState->SaveFile(reinterpret_cast<FileInfo*>(p1)); return 0; }
		if (a1 == 9) { g_DiskState->SaveFileState(reinterpret_cast<FileInfo*>(p1), a2 != 0); return 0; }
		if (a1 == 10) return g_DiskState->LoadFileState(reinterpret_cast<FileInfo*>(p1), a2 != 0) ? 1 : 0;
		if (a1 == 11) { g_DiskState->LoadArticles(reinterpret_cast<FileInfo*>(p1)); return 0; }
		if (a1 == 12)
		{
			FileInfo fi(a2);
			if (g_DiskState->LoadFileState(&fi, true)) g_DiskState->SaveFileState(&fi, true);
		}
		if (a1 == 13)
		{
			FileInfo fi(a2);
			if (!g_DiskState->LoadFileState(&fi, true)) return 0;
			auto pushFn = reinterpret_cast<void(*)(uintptr_t, int, int64_t, int, uint32_t)>(p2);
			bool any = false;
			for (ArticleInfo* pa : fi.GetArticles())
			{
				pushFn(p1, pa->GetStatus() == ArticleInfo::aiFinished ? 1 : 0, pa->GetSegmentOffset(), pa->GetSegmentSize(), pa->GetCrc());
				any = true;
			}
			return any ? 2 : 1;
		}
		return 0;
	}
	case 6: {
		if (a1 == 0 && g_HistoryCoordinator) g_HistoryCoordinator->DeleteDiskFiles(reinterpret_cast<NzbInfo*>(p1));
		else if (a1 == 1 && g_HistoryCoordinator) g_HistoryCoordinator->Redownload(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<HistoryInfo*>(p2));
		else if (a1 == 2 && g_HistoryCoordinator) g_HistoryCoordinator->HistoryHide(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<HistoryInfo*>(p2), a2);
		else if (a1 == 3 && g_PrePostProcessor) g_PrePostProcessor->CheckDownloadCompleted(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<NzbInfo*>(p2));
		else if (a1 == 4 && g_QueueScriptCoordinator) g_QueueScriptCoordinator->EnqueueScript(reinterpret_cast<NzbInfo*>(p1), (QueueScriptCoordinator::EEvent)a2);
		else if (a1 == 5) PostJobThread::StartLive(reinterpret_cast<NzbInfo*>(p1));
		else if (a1 == 6) return nzbget_rs_article_dl_exec(nullptr, 23, 0, 0, p1, 0, s1 ? s1 : "", NzbgetCArticleDlHost, nullptr);
		else if (a1 == 8 && g_HistoryCoordinator) g_HistoryCoordinator->AddToHistory(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<NzbInfo*>(p2));
		else if (a1 == 9)
		{
			if (!g_HistoryCoordinator) return 0;
			IdList idList;
			if (ints && a2 > 0) idList.assign(ints, ints + a2);
			return g_HistoryCoordinator->EditList(reinterpret_cast<DownloadQueue*>(p1), &idList, (DownloadQueue::EEditAction)p2, s1) ? 1 : 0;
		}
		else if (a1 == 10)
		{
			if (!g_PrePostProcessor) return 0;
			IdList idList;
			if (ints && a2 > 0) idList.assign(ints, ints + a2);
			return g_PrePostProcessor->EditList(reinterpret_cast<DownloadQueue*>(p1), &idList, (DownloadQueue::EEditAction)p2, s1) ? 1 : 0;
		}
		else if (a1 == 14)
		{
			auto nzbInfo = std::make_unique<NzbInfo>();
			nzbInfo->SetKind(NzbInfo::nkUrl);
			nzbInfo->SetFeedId(ints[0]);
			nzbInfo->SetUrl(strs[0] ? strs[0] : "");
			if (strs[1] && *strs[1]) nzbInfo->SetFilename(strs[1]);
			nzbInfo->SetCategory(strs[2] ? strs[2] : "");
			nzbInfo->SetPriority(ints[2]);
			nzbInfo->SetAddUrlPaused((ints[1] & 1) != 0);
			nzbInfo->SetDupeKey(strs[3] ? strs[3] : "");
			nzbInfo->SetDupeScore(ints[3]);
			nzbInfo->SetDupeMode(static_cast<EDupeMode>((ints[1] >> 1) & 0xff));
			nzbInfo->SetSize(i64s[0]);
			nzbInfo->SetMinTime(static_cast<time_t>(i64s[1]));
			nzbInfo->SetMaxTime(static_cast<time_t>(i64s[1]));
			nzbget_rs_url_coord_add(reinterpret_cast<uintptr_t>(nzbInfo.get()), reinterpret_cast<uintptr_t>(&nzbInfo), 0);
		}
		else if (a1 == 15 && g_Scanner)
		{
			auto* n = reinterpret_cast<NzbInfo*>(p1);
			return a2 != 0
				? (uintptr_t)g_Scanner->AddArchive(s1, s2, n->GetAutoCategory(), n->GetPriority(), n->GetDupeKey(), n->GetDupeScore(), n->GetDupeMode(), n->GetParameters(), false, n->GetAddUrlPaused(), n, strs[0])
				: (uintptr_t)g_Scanner->AddExternalFile(s1, s2, n->GetAutoCategory(), n->GetPriority(), n->GetDupeKey(), n->GetDupeScore(), n->GetDupeMode(), n->GetParameters(), false, n->GetAddUrlPaused(), n, strs[0], nullptr, 0, nullptr);
		}
		return 0;
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
		return 0;
	}
	case 23: {
		auto* dq = reinterpret_cast<DownloadQueue*>(p1);
		if (a1 == -1) return dq->GetHistory()->size();
		if (a1 == -2) return reinterpret_cast<uintptr_t>(dq->GetHistory()->Find(a2));
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
		i64s[0] = n->GetSize(); i64s[1] = n->GetRemainingSize(); i64s[2] = n->GetPausedSize();
		i64s[3] = n->GetSuccessSize(); i64s[4] = n->GetCurrentSuccessSize(); i64s[5] = n->GetFailedSize();
		i64s[6] = n->GetCurrentFailedSize(); i64s[7] = n->GetParSize(); i64s[8] = n->GetParSuccessSize();
		i64s[9] = n->GetParCurrentSuccessSize(); i64s[10] = n->GetParFailedSize();
		i64s[11] = n->GetParCurrentFailedSize(); i64s[12] = n->GetDownloadedSize();
		i64s[13] = (int64_t)n->GetMinTime(); i64s[14] = (int64_t)n->GetMaxTime();
		i64s[15] = (int64_t)n->GetFilteredContentHash(); i64s[16] = (int64_t)n->GetDownloadStartTime();
		i64s[17] = n->GetDupeRecoveredBytes(); i64s[18] = (int64_t)reinterpret_cast<uintptr_t>(pi);
		i64s[19] = (int64_t)reinterpret_cast<uintptr_t>(n->GetLiveRepairThread());
		strs[0] = n->GetName(); strs[1] = n->GetFilename(); strs[2] = n->GetDestDir(); strs[3] = n->GetFinalDir();
		strs[4] = n->GetCategory(); strs[5] = n->GetDupeKey(); strs[6] = n->GetQueuedFilename();
		strs[7] = a1 != 0 ? n->MakeTextStatus(true) : nullptr;
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
			const auto* rawHoles = reinterpret_cast<const NzbgetRsByteRange*>(p2);
			StreamRangeList holes;
			if (rawHoles && a2 > 0)
			{
				holes.reserve((size_t)a2);
				for (int i = 0; i < a2; i++) holes.push_back({ rawHoles[i].offset, rawHoles[i].size });
			}
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
						if (g_DiskState) g_DiskState->DiscardFile(a2, true, true, true);
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
		return 0;
	}
	case 35: {
		if (a1 == 0)
		{
			DupInfo* d = reinterpret_cast<DupInfo*>(p1);
			ints[0] = (int)d->GetDupeMode(); ints[1] = d->GetDupeScore(); ints[2] = (int)d->GetStatus();
			ints[3] = (int)d->GetFullContentHash(); ints[4] = (int)d->GetFilteredContentHash();
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
		if (a2 < 0) return f->GetGroups()->size();
		strs[0] = *f->GetGroups()->at((size_t)a2);
		return 0;
	}
	case 42: {
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
		default: break;
		}
		return 0;
	}
	case 43: {
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
		return 0;
	}
	case 44: {
		FileInfo* f = reinterpret_cast<FileInfo*>(p1);
		if (a1 == 1)
		{
			if (!g_ArticleCache) return 0;
			ArticleInfo* a = reinterpret_cast<ArticleInfo*>(p2);
			Guard cg = g_ArticleCache->GuardContent();
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
			if (!g_ArticleCache)
			{
				if (a->GetDupeFallbackRound() > 0 && a->GetResultFilename()) FileSystem::DeleteFile(a->GetResultFilename());
				return 0;
			}
			Guard cg = g_ArticleCache->GuardContent();
			if (a->GetDupeFallbackRound() > 0 && a->GetResultFilename()) FileSystem::DeleteFile(a->GetResultFilename());
			if (!a->GetSegmentContent() || f->GetFlushLocked()) return 0;
			a->DiscardSegment();
			f->SetCachedArticles(f->GetCachedArticles() - 1);
			return 1;
		}
		if (a1 == 3)
		{
			if (!g_ArticleCache) return 0;
			Guard cg = g_ArticleCache->GuardContent();
			if (a2 != 0) { if (f->GetFlushLocked()) return 0; f->SetFlushLocked(true); return 1; }
			f->SetFlushLocked(false);
			return 1;
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
			i64s[0] = (int64_t)pi->GetStartTime(); i64s[1] = (int64_t)reinterpret_cast<uintptr_t>(pi->GetNzbInfo());
			i64s[2] = (int64_t)pi->GetStageTime();
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

inline bool DupeArticleFallback::ShouldDeferToPar(NzbInfo* nzbInfo)
{
	return nzbget_rs_dupe_fallback_op(1, 0, reinterpret_cast<uintptr_t>(nzbInfo), &NzbgetCQueueHost) != 0;
}
#endif

class FeedCoordinator;
namespace SystemHealth { class Service; }
class Maintenance;
#include <system_error>

extern void ExitProc();
extern void Reload();

inline NzbgetRsRpcHost BuildRpcHost()
{
	NzbgetRsRpcHost host{};
	host.rpc_action_cb = +[](void*, int op, int a1, int a2, int a3, int a4,
		const char* s1, const char* s2, const char* s3, const char*,
		const char* body, size_t bodyLen, const int32_t* ids, size_t idsLen,
		const NzbgetRsRpcParamItem* params, size_t paramsLen,
		uint32_t* outU32, double*, NzbgetRsBuf* outBuf) -> int {
		auto setBuf = [&](const char* p, size_t n) {
			if (outBuf) *outBuf = nzbget_rs_alloc_buf(p, n);
		};
		switch (op)
		{
			case 10: {
				IdList idList(ids, ids + idsLen);
				return DownloadQueue::Guard()->EditList(&idList, nullptr, DownloadQueue::mmId,
					(DownloadQueue::EEditAction)a1, s1 ? s1 : "") ? 1 : 0;
			}
			case 11: {
				bool addTop = (a1 & 1) != 0, addPaused = (a1 & 2) != 0, autoCat = (a1 & 4) != 0;
				bool isUrl = (a1 & 8) != 0, isZip = (a1 & 16) != 0;
				NzbParameterList paramList;
				for (size_t i = 0; i < paramsLen; i++) paramList.SetParameter(params[i].name, params[i].value);
				if (isUrl)
				{
					auto nzbInfo = std::make_unique<NzbInfo>();
					nzbInfo->SetKind(NzbInfo::nkUrl);
					nzbInfo->SetUrl(body ? body : "");
					nzbInfo->SetFilename(s1 ? s1 : "");
					nzbInfo->SetCategory(s2 ? s2 : "");
					nzbInfo->SetAutoCategory(autoCat);
					nzbInfo->SetPriority(a2);
					nzbInfo->SetAddUrlPaused(addPaused);
					nzbInfo->SetDupeKey(s3 ? s3 : "");
					nzbInfo->SetDupeScore(a3);
					nzbInfo->SetDupeMode((EDupeMode)a4);
					nzbInfo->GetParameters()->CopyFrom(&paramList);
					int nzbId = nzbInfo->GetId();
					info("Queue %s", *nzbInfo->MakeNiceUrlName(body ? body : "", s1 ? s1 : ""));
					nzbget_rs_url_coord_add(reinterpret_cast<uintptr_t>(nzbInfo.get()), reinterpret_cast<uintptr_t>(&nzbInfo), addTop ? 1 : 0);
					if (outU32) *outU32 = (uint32_t)nzbId;
					return 1;
				}
				if (isZip)
				{
					return g_Scanner->AddArchive(s1 ? s1 : "", s2 ? s2 : "", autoCat, a2, s3, a3,
						(EDupeMode)a4, paramList.empty() ? nullptr : &paramList, addTop, addPaused,
						nullptr, body, (int)bodyLen) == Scanner::asSuccess ? 1 : 0;
				}
				int nzbId = -1;
				g_Scanner->AddExternalFile(s1 ? s1 : "", s2 ? s2 : "", autoCat, a2, s3, a3,
					(EDupeMode)a4, paramList.empty() ? nullptr : &paramList, addTop, addPaused,
					nullptr, nullptr, body, (int)bodyLen, &nzbId);
				if (outU32) *outU32 = (uint32_t)nzbId;
				return nzbId > 0 ? 1 : 0;
			}
			case 22: {
				std::string r = a1 == 0 ? TlsCipherBackend::Start() :
					a1 == 1 ? TlsCipherBackend::Status(s1) : TlsCipherBackend::Cancel(s1 ? s1 : "");
				setBuf(r.data(), r.size());
				return 1;
			}
			case 24: {
				auto nzbInfo = std::make_unique<NzbInfo>();
				nzbInfo->SetKind(NzbInfo::nkUrl);
				nzbInfo->SetUrl(s1 ? s1 : "");
				nzbInfo->SetFilename(s1 ? s1 : "");
				nzbInfo->SetPriority(NzbInfo::FORCE_PRIORITY);
				nzbInfo->SetExtraPriority(NzbInfo::FORCE_PRIORITY);
				nzbInfo->SetAddUrlPaused(false);
				nzbInfo->SetDupeMode(EDupeMode::dmForce);
				nzbInfo->SetDesiredServerId(a1);
				nzbInfo->SetSkipScriptProcessing(true);
				nzbInfo->SetSkipDiskWrite(true);
				nzbget_rs_url_coord_add(reinterpret_cast<uintptr_t>(nzbInfo.get()), reinterpret_cast<uintptr_t>(&nzbInfo), 1);
				return 1;
			}
			default: return 0;
		}
	};
	host.rpc_collect_cb = +[](void*, int kind, int a1, int a2, int a3, int a4, int,
		const char*, const char*, const char*, const char*, const char*, const char*,
		const void*, size_t, void* emitCtx, NzbgetRsXmlRpcEmitFn emitCb) -> int {
		auto withNzb = [&](NzbInfo* nzbInfo, auto&& fn) {
			time_t postStartTime = nzbInfo->GetPostInfo() ? nzbInfo->GetPostInfo()->GetStartTime() : 0;
			time_t curTime = postStartTime ? Util::CurrentTime() : 0;
			NzbgetRsRpcNzbInfoInput f{};
			f.id = nzbInfo->GetId(); f.name = nzbInfo->GetName(); f.kind = (int)nzbInfo->GetKind();
			f.url = nzbInfo->GetUrl(); f.filename = nzbInfo->GetFilename();
			f.dest_dir = nzbInfo->GetDestDir(); f.final_dir = nzbInfo->GetFinalDir();
			f.category = nzbInfo->GetCategory(); f.par_status = (int)nzbInfo->GetParStatus();
			f.extra_par_blocks = nzbInfo->GetExtraParBlocks();
			f.unpack_status = (int)nzbInfo->GetUnpackStatus(); f.move_status = (int)nzbInfo->GetMoveStatus();
			f.script_status = (int)nzbInfo->GetScriptStatuses()->CalcTotalStatus();
			f.delete_status = (int)nzbInfo->GetDeleteStatus(); f.mark_status = (int)nzbInfo->GetMarkStatus();
			f.url_status = (int)nzbInfo->GetUrlStatus(); f.size = nzbInfo->GetSize();
			f.file_count = nzbInfo->GetFileCount();
			f.min_time = (int)nzbInfo->GetMinTime(); f.max_time = (int)nzbInfo->GetMaxTime();
			f.total_articles = nzbInfo->GetTotalArticles();
			f.current_success_articles = nzbInfo->GetCurrentSuccessArticles();
			f.current_failed_articles = nzbInfo->GetCurrentFailedArticles();
			f.dupe_recovered_articles = nzbInfo->GetDupeRecoveredArticles();
			f.dupe_recovered_bytes = nzbInfo->GetDupeRecoveredBytes();
			f.dupe_recovered_holes = nzbInfo->GetDupeRecoveredHoles();
			f.live_repairing = nzbInfo->GetLiveRepairThread() != nullptr ? 1 : 0;
			f.health = nzbInfo->CalcHealth(); f.critical_health = nzbInfo->CalcCriticalHealth(false);
			f.dupe_key = nzbInfo->GetDupeKey(); f.dupe_score = nzbInfo->GetDupeScore();
			f.dupe_mode = (int)nzbInfo->GetDupeMode(); f.downloaded_size = nzbInfo->GetDownloadedSize();
			f.download_sec = nzbInfo->GetDownloadSec(); f.post_total_sec = nzbInfo->GetPostTotalSec();
			f.has_post_start_time = postStartTime != 0 ? 1 : 0;
			f.cur_time = (int64)curTime; f.post_start_time = (int64)postStartTime;
			f.par_sec = nzbInfo->GetParSec(); f.repair_sec = nzbInfo->GetRepairSec();
			f.unpack_sec = nzbInfo->GetUnpackSec();
			f.message_count = nzbInfo->GetMessageCount();
			f.cached_message_count = nzbInfo->GetCachedMessageCount();
			std::vector<NzbgetRsRpcParamItem> prms;
			prms.reserve(nzbInfo->GetParameters()->size());
			for (NzbParameter& p : nzbInfo->GetParameters()) prms.push_back({p.GetName(), p.GetValue()});
			std::vector<NzbgetRsRpcScriptItem> scrs;
			scrs.reserve(nzbInfo->GetScriptStatuses()->size());
			for (ScriptStatus& s : nzbInfo->GetScriptStatuses()) scrs.push_back({s.GetName(), (int)s.GetStatus()});
			std::vector<NzbgetRsRpcServerStatItem> sts;
			sts.reserve(nzbInfo->GetCurrentServerStats()->size());
			for (ServerStat& st : nzbInfo->GetCurrentServerStats())
				sts.push_back({st.GetServerId(), st.GetSuccessArticles(), st.GetFailedArticles()});
			fn(f, prms, scrs, sts);
		};
		auto emitPost = [&](PostInfo* postInfo, int logEntries, bool postQueue) {
			time_t curTime = postInfo ? Util::CurrentTime() : 0;
			std::optional<GuardedMessageList> messages;
			std::vector<NzbgetRsRpcLogEntry> entries;
			if (logEntries > 0 && postInfo)
			{
				messages.emplace(postInfo->GetNzbInfo()->GuardCachedMessages());
				if (!(*messages)->empty())
				{
					int cnt = std::min(logEntries, (int)(*messages)->size());
					int start = (int)(*messages)->size() - cnt;
					entries.reserve(cnt);
					for (uint32 i = (uint32)start; i < (*messages)->size(); i++)
					{
						Message& m = (*messages)->at(i);
						entries.push_back({(int)m.GetId(), (int)m.GetKind(), (int)m.GetTime(), m.GetText()});
					}
				}
			}
			emitCb(emitCtx, 5, postQueue ? 1 : 0, postInfo ? 1 : 0, postInfo ? postInfo->GetStageProgress() : 0,
				0, 0, 0, (int64)curTime, postInfo ? (int64)postInfo->GetStageTime() : 0,
				postInfo ? (int64)postInfo->GetStartTime() : 0,
				postInfo ? postInfo->GetProgressLabel() : nullptr, nullptr, nullptr, nullptr,
				entries.data(), entries.size(), nullptr, 0, nullptr, 0, nullptr, 0);
		};
		switch (kind)
		{
			case 1: {
				auto* qst = static_cast<NzbgetRsRpcQueueStatus*>(emitCtx);
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				for (NzbInfo* n : dq->GetQueue())
				{
					qst->post_job_count += n->GetPostInfo() != nullptr ? 1 : 0;
					qst->url_count += n->GetKind() == NzbInfo::nkUrl ? 1 : 0;
				}
				dq->CalcRemainingSize(&qst->remaining_size, &qst->forced_size);
				return 1;
			}
			case 2: {
				MessageList diskMessages;
				std::unique_ptr<GuardedDownloadQueue> dq;
				auto guardMessages = [&]() -> GuardedMessageList {
					g_DiskState->LoadNzbMessages(a2, &diskMessages);
					if (diskMessages.empty())
					{
						dq = std::make_unique<GuardedDownloadQueue>(DownloadQueue::Guard());
						if (NzbInfo* n = (*dq)->GetQueue()->Find(a2)) return n->GuardCachedMessages();
						dq.reset();
					}
					return GuardedMessageList(&diskMessages, nullptr);
				};
				GuardedMessageList messages = guardMessages();
				int start = (int)messages->size();
				if (a4 > 0) start = (int)messages->size() - std::min(a4, (int)messages->size());
				if (a3 > 0 && !messages->empty()) start = std::max(0, a3 - (int)messages->front().GetId());
				std::vector<NzbgetRsRpcLogEntry> entries;
				if ((uint32)start < messages->size())
				{
					entries.reserve(messages->size() - (uint32)start);
					for (uint32 i = (uint32)start; i < messages->size(); i++)
					{
						Message& m = messages->at(i);
						entries.push_back({(int)m.GetId(), (int)m.GetKind(), (int)m.GetTime(), m.GetText()});
					}
				}
				emitCb(emitCtx, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr,
					entries.data(), entries.size(), nullptr, 0, nullptr, 0, nullptr, 0);
				return 1;
			}
			case 3: {
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				for (NzbInfo* nzbInfo : dq->GetQueue())
				{
					for (FileInfo* fi : nzbInfo->GetFileList())
					{
						if ((a3 > 0 && a3 == fi->GetNzbInfo()->GetId()) ||
							(a3 == 0 && (a1 == 0 || (a1 <= fi->GetId() && fi->GetId() <= a2))))
						{
							int64 denom = fi->GetSize() - fi->GetMissedSize();
							int progress = (fi->GetFailedSize() == 0 && fi->GetSuccessSize() == 0) || denom <= 0
								? 0 : (int)(1000 - fi->GetRemainingSize() * 1000 / denom);
							NzbgetRsRpcFileItem item{
								fi->GetId(), fi->GetSize(), fi->GetRemainingSize(), (int)fi->GetTime(),
								fi->GetFilenameConfirmed() ? 1 : 0, fi->GetPaused() ? 1 : 0,
								fi->GetNzbInfo()->GetId(), fi->GetNzbInfo()->GetName(),
								fi->GetNzbInfo()->GetFilename(), fi->GetSubject(), fi->GetFilename(),
								fi->GetNzbInfo()->GetDestDir(), fi->GetNzbInfo()->GetCategory(),
								fi->GetNzbInfo()->GetPriority(), fi->GetActiveDownloads(), progress
							};
							emitCb(emitCtx, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr,
								&item, 0, nullptr, 0, nullptr, 0, nullptr, 0);
						}
					}
				}
				return 1;
			}
			case 4: {
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				for (NzbInfo* nzbInfo : dq->GetQueue())
				{
					bool hasPost = nzbInfo->GetPostInfo() != nullptr;
					int postStage = hasPost ? (int)nzbInfo->GetPostInfo()->GetStage() : 0;
					int flags = (hasPost ? 1 : 0) |
						((postStage & 0xff) << 4) | (((int)nzbInfo->GetDirectUnpackStatus() & 0xff) << 12) |
						(((int)nzbInfo->GetKind() & 0xff) << 20);
					withNzb(nzbInfo, [&](const auto& f, const auto& prms, const auto& scrs, const auto& sts) {
						emitCb(emitCtx, 4, nzbInfo->GetId(), (int)nzbInfo->GetFileList()->size(),
							nzbInfo->GetRemainingParCount(), nzbInfo->GetPriority(), nzbInfo->GetActiveDownloads(),
							flags, nzbInfo->GetRemainingSize(), nzbInfo->GetPausedSize(), 0,
							nullptr, nullptr, nullptr, nullptr,
							&f, 0, prms.data(), prms.size(), scrs.data(), scrs.size(), sts.data(), sts.size());
					});
					emitPost(nzbInfo->GetPostInfo(), a1, false);
				}
				return 1;
			}
			case 5: {
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				for (NzbInfo* nzbInfo : dq->GetQueue())
				{
					PostInfo* pi = nzbInfo->GetPostInfo();
					if (!pi) continue;
					withNzb(pi->GetNzbInfo(), [&](const auto& f, const auto& prms, const auto& scrs, const auto& sts) {
						emitCb(emitCtx, 6, nzbInfo->GetId(), (int)pi->GetStage(), pi->GetFileProgress(), 0, 0, 0,
							0, 0, 0, pi->GetNzbInfo()->GetName(), nullptr, nullptr, nullptr,
							&f, 0, prms.data(), prms.size(), scrs.data(), scrs.size(), sts.data(), sts.size());
					});
					emitPost(pi, a1, true);
				}
				return 1;
			}
			case 6: {
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				for (HistoryInfo* hi : dq->GetHistory())
				{
					if (hi->GetKind() == HistoryInfo::hkDup && !a1) continue;
					if (hi->GetKind() == HistoryInfo::hkNzb || hi->GetKind() == HistoryInfo::hkUrl)
					{
						NzbInfo* n = hi->GetNzbInfo();
						const char* status = n->MakeTextStatus(false);
						withNzb(n, [&](const auto& f, const auto& prms, const auto& scrs, const auto& sts) {
							emitCb(emitCtx, 7, hi->GetId(), n->GetParkedFileCount(),
								!n->GetCompletedFiles()->empty() ? 1 : 0, (int)hi->GetTime(), 0, 0,
								0, 0, 0, hi->GetName(), status, nullptr, nullptr,
								&f, 0, prms.data(), prms.size(), scrs.data(), scrs.size(), sts.data(), sts.size());
						});
					}
					else if (hi->GetKind() == HistoryInfo::hkDup)
					{
						DupInfo* d = hi->GetDupInfo();
						emitCb(emitCtx, 8, hi->GetId(), (int)hi->GetTime(), d->GetDupeScore(),
							(int)d->GetDupeMode(), (int)d->GetStatus(), 0, d->GetSize(), 0, 0,
							hi->GetName(), d->GetDupeKey(), nullptr, nullptr,
							nullptr, 0, nullptr, 0, nullptr, 0, nullptr, 0);
					}
				}
				return 1;
			}
			case 7: {
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				for (NzbInfo* n : dq->GetQueue())
				{
					if (n->GetKind() == NzbInfo::nkUrl)
						emitCb(emitCtx, 9, n->GetId(), n->GetPriority(), 0, 0, 0, 0, 0, 0, 0,
							n->GetFilename(), n->GetUrl(), n->GetName(), n->GetCategory(),
							nullptr, 0, nullptr, 0, nullptr, 0, nullptr, 0);
				}
				return 1;
			}
			default: return 0;
		}
	};
	host.binrpc_action_cb = +[](int kind, int a1, int a2, int a3, int a4,
		const char* s1, const char* s2, const char* s3, const char* body, int,
		const int32_t* ids, size_t idsLen, const char* const* names, size_t namesLen) -> int {
		switch (kind)
		{
			case 1: {
				auto nzbInfo = std::make_unique<NzbInfo>();
				nzbInfo->SetKind(NzbInfo::nkUrl);
				nzbInfo->SetUrl(body);
				nzbInfo->SetFilename(s1);
				nzbInfo->SetCategory(s2);
				nzbInfo->SetAutoCategory((a1 & 4) != 0);
				nzbInfo->SetPriority(a2);
				nzbInfo->SetAddUrlPaused((a1 & 2) != 0);
				nzbInfo->SetDupeKey(s3);
				nzbInfo->SetDupeScore(a3);
				nzbInfo->SetDupeMode((EDupeMode)a4);
				nzbget_rs_url_coord_add(reinterpret_cast<uintptr_t>(nzbInfo.get()), reinterpret_cast<uintptr_t>(&nzbInfo), (a1 & 1) != 0 ? 1 : 0);
				return 1;
			}
			case 6: {
				IdList cIdList(ids, ids + idsLen);
				NameList cNameList;
				cNameList.reserve(namesLen);
				for (size_t i = 0; i < namesLen; i++) cNameList.emplace_back(names[i]);
				return DownloadQueue::Guard()->EditList(
					idsLen > 0 ? &cIdList : nullptr, namesLen > 0 ? &cNameList : nullptr,
					(DownloadQueue::EMatchMode)a2, (DownloadQueue::EEditAction)a1, s1) ? 1 : 0;
			}
			default: return 0;
		}
	};
	host.binrpc_list_cb = +[](int wantFiles, void* bCtx,
		NzbgetRsBinRpcListPushNzbFn pushNzb, NzbgetRsBinRpcListPushParamFn pushParam,
		NzbgetRsBinRpcListPushFileFn pushFile, int* outPostJobs, int64_t* outRemSize) {
		GuardedDownloadQueue dq = DownloadQueue::Guard();
		if (wantFiles)
		{
			uint32 ownerIndex = 0;
			for (NzbInfo* nzbInfo : dq->GetQueue())
			{
				ownerIndex++;
				pushNzb(bCtx, (uint32)nzbInfo->GetId(), (uint32)nzbInfo->GetKind(),
					nzbInfo->GetSize(), nzbInfo->GetRemainingSize(), nzbInfo->GetPausedSize(),
					(uint32)nzbInfo->GetPausedFileCount(), (uint32)nzbInfo->GetRemainingParCount(),
					(uint32)nzbInfo->GetPriority(), nzbInfo->GetFilename(), nzbInfo->GetName(),
					nzbInfo->GetDestDir(), nzbInfo->GetCategory(), nzbInfo->GetQueuedFilename());
				for (NzbParameter& p : nzbInfo->GetParameters())
					pushParam(bCtx, ownerIndex, p.GetName(), p.GetValue());
				for (FileInfo* fileInfo : nzbInfo->GetFileList())
				{
					uint32 nzbIndex = ownerIndex;
					if (fileInfo->GetNzbInfo() != nzbInfo)
					{
						nzbIndex = 0;
						for (uint32 i = 0; i < dq->GetQueue()->size(); i++)
						{
							nzbIndex++;
							if (dq->GetQueue()->at(i).get() == fileInfo->GetNzbInfo()) break;
						}
					}
					pushFile(bCtx, (uint32)fileInfo->GetId(), nzbIndex,
						fileInfo->GetSize(), fileInfo->GetRemainingSize(),
						(uint32)fileInfo->GetPaused(), (uint32)fileInfo->GetFilenameConfirmed(),
						(uint32)fileInfo->GetActiveDownloads(), fileInfo->GetNzbInfo()->GetName(),
						fileInfo->GetSubject(), fileInfo->GetFilename());
				}
			}
		}
		if (outPostJobs)
		{
			int postJobs = 0;
			for (NzbInfo* n : dq->GetQueue()) postJobs += n->GetPostInfo() ? 1 : 0;
			*outPostJobs = postJobs;
			dq->CalcRemainingSize(outRemSize, nullptr);
		}
	};
	host.binrpc_post_cb = +[](void* encCtx, NzbgetRsBinRpcPostPushFn pushPost) {
		GuardedDownloadQueue dq = DownloadQueue::Guard();
		time_t curTime = Util::CurrentTime();
		for (NzbInfo* nzbInfo : dq->GetQueue())
		{
			PostInfo* postInfo = nzbInfo->GetPostInfo();
			if (!postInfo) continue;
			pushPost(encCtx, (uint32)nzbInfo->GetId(), (uint32)postInfo->GetStage(),
				(uint32)postInfo->GetStageProgress(), (uint32)postInfo->GetFileProgress(),
				postInfo->GetStartTime() ? (uint32)(curTime - postInfo->GetStartTime()) : 0,
				postInfo->GetStageTime() ? (uint32)(curTime - postInfo->GetStageTime()) : 0,
				postInfo->GetNzbInfo()->GetFilename(), postInfo->GetNzbInfo()->GetName(),
				postInfo->GetNzbInfo()->GetDestDir(), postInfo->GetProgressLabel());
		}
	};
	host.binrpc_history_cb = +[](int showHidden, void* encCtx, NzbgetRsBinRpcHistPushFn pushHist) {
		GuardedDownloadQueue dq = DownloadQueue::Guard();
		for (HistoryInfo* h : dq->GetHistory())
		{
			if (h->GetKind() == HistoryInfo::hkDup && !showHidden) continue;
			int64 size = 0;
			uint32 fileCount = 0, parStatus = 0, scriptStatus = 0, urlStatus = 0;
			if (h->GetKind() == HistoryInfo::hkNzb)
			{
				NzbInfo* nzbInfo = h->GetNzbInfo();
				size = nzbInfo->GetSize();
				fileCount = nzbInfo->GetFileCount();
				parStatus = nzbInfo->GetParStatus();
				scriptStatus = nzbInfo->GetScriptStatuses()->CalcTotalStatus();
			}
			else if (h->GetKind() == HistoryInfo::hkDup) size = h->GetDupInfo()->GetSize();
			else if (h->GetKind() == HistoryInfo::hkUrl) urlStatus = h->GetNzbInfo()->GetUrlStatus();
			pushHist(encCtx, (uint32)h->GetId(), (uint32)h->GetKind(), (uint32)(int)h->GetTime(),
				size, fileCount, parStatus, scriptStatus, urlStatus, h->GetName());
		}
	};
	return host;
}

#endif
