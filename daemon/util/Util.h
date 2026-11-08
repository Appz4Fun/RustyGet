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
#define SIGCHLD_HANDLER 1
#define FUNCTION_MACRO_NAME __FUNCTION__
#define HAVE_CTIME_R_2 1
#define HAVE_FDATASYNC 1
#define HAVE_FSEEKO 1
#ifdef __APPLE__
#define HAVE_FULLFSYNC 1
#endif
#define HAVE_GETADDRINFO 1
#define HAVE_GETOPT 1
#define HAVE_GETOPT_H 1
#define HAVE_GETOPT_LONG 1
#define HAVE_INTTYPES_H 1
#define HAVE_LOCKF 1
#define HAVE_NCURSES_H 1
#define HAVE_PTHREAD_CANCEL 1
#define HAVE_REGEX_H 1
#define HAVE_SC_NPROCESSORS_ONLN 1
#define HAVE_STDIO_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRINGS_H 1
#define HAVE_STRING_H 1
#ifdef __linux__
#define HAVE_SYS_PRCTL_H 1
#define HAVE_ENDIAN_H 1
#endif
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_UNISTD_H 1
#define HAVE_ALLOCA_H 1
#define HAVE_VARIADIC_MACROS 1
#define HAVE_X509_CHECK_HOST 1
#define SOCKLEN_T socklen_t
#ifndef NZBGET_USE_RUST
#define NZBGET_USE_RUST 1
#endif

/***************** GLOBAL INCLUDES *****************/

// POSIX INCLUDES

#include <unistd.h>
#include <pwd.h>
#include <grp.h>
#include <signal.h>
#include <netdb.h>
#include <pthread.h>
#include <getopt.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/resource.h>
#include <sys/statvfs.h>
#include <sys/wait.h>
#include <sys/un.h>
#include <sys/file.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <dirent.h>

#ifndef __linux__
#include <sys/sysctl.h>
#endif

#define __BSD__ (__FreeBSD__ || __NetBSD__ || __OpenBSD__ || __DragonFly__)

#ifdef HAVE_SYS_PRCTL_H
#include <sys/prctl.h>
#endif

#ifdef HAVE_ENDIAN_H
#include <endian.h>
#endif

#ifdef HAVE_BACKTRACE
#include <execinfo.h>
#endif

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
#include <future>
#include <mutex>
#include <shared_mutex>
#include <condition_variable>
#include <chrono>
#include <optional>
#include <variant>
#include <limits>
#include <type_traits>
#include <random>
#include <exception>
#include <iomanip>
#include <span>
#include <regex>

#ifndef DISABLE_TLS
#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/ec.h>
#include <openssl/x509v3.h>

namespace OpenSSL
{
	using X509StorePtr = std::unique_ptr<X509_STORE, decltype(&X509_STORE_free)>;
	using X509Ptr = std::unique_ptr<X509, decltype(&X509_free)>;
	using EVPMdCtxPtr = std::unique_ptr<EVP_MD_CTX, decltype(&EVP_MD_CTX_free)>;
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

#ifdef HAVE_REGEX_H
#include <regex.h>
#endif

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
#define MAX_PATH 1024
#define S_DIRMODE (S_IRWXU | S_IRWXG | S_IRWXO)
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
using int16 = int16_t;
using uint16 = uint16_t;
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

// Assume little endian if byte order is not defined
#ifndef __BYTE_ORDER
#define __LITTLE_ENDIAN 1234
#define __BIG_ENDIAN    4321
#define __BYTE_ORDER __LITTLE_ENDIAN
#endif

#ifdef __GNUC__
#define PRINTF_SYNTAX(strindex) __attribute__ ((format (printf, strindex, strindex+1)))
#define SCANF_SYNTAX(strindex) __attribute__ ((format (scanf, strindex, strindex+1)))
#else
#define PRINTF_SYNTAX(strindex)
#define SCANF_SYNTAX(strindex)
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

	friend class ConditionVar;
};

class Guard
{
public:
	Guard() : m_mutex(nullptr) {}
	Guard(Mutex& mutex) : m_mutex(&mutex) { if (m_mutex) m_mutex->Lock(); }
	Guard(Mutex* mutex) : m_mutex(mutex) { if (m_mutex) m_mutex->Lock(); }
	Guard(std::unique_ptr<Mutex>& mutex) : m_mutex(mutex.get()) { if (m_mutex) m_mutex->Lock(); }
	Guard(Guard&& other) : m_mutex(other.m_mutex) { other.m_mutex = nullptr; }
	Guard(const Guard&) = delete;
	~Guard() { Unlock(); }
	Guard& operator=(Guard&& other) { m_mutex = other.m_mutex; other.m_mutex = nullptr; return *this; }
	operator bool() { return m_mutex; }

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

class ConditionVar
{
public:
	ConditionVar() {}
	ConditionVar(const ConditionVar& other) = delete;
	void Wait(Mutex& mutex) { Lock l(mutex); m_condObj.wait(l); }
	template <typename Pred> void Wait(Mutex& mutex, Pred pred) { Lock l(mutex); m_condObj.wait(l, pred); }
	void WaitFor(Mutex& mutex, int msec) { Lock l(mutex); m_condObj.wait_for(l, std::chrono::milliseconds(msec)); }
	template <typename Pred> void WaitFor(Mutex& mutex, int msec, Pred pred)
		{ Lock l(mutex); m_condObj.wait_for(l, std::chrono::milliseconds(msec), pred); }
	void NotifyOne() { m_condObj.notify_one(); }
	void NotifyAll() { m_condObj.notify_all(); }

private:
	std::condition_variable m_condObj;

	struct Lock : public std::unique_lock<std::mutex>
	{
		Lock(Mutex& mutex) : std::unique_lock<std::mutex>(mutex.m_mutexObj, std::adopt_lock) {}
		~Lock() { release(); }
	};
};

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

#ifdef HAVE_PTHREAD_CANCEL
		bool terminated = pthread_cancel(m_threadObj) == 0;
#else
		bool terminated = false;
#endif

		if (terminated)
		{
			--m_threadCount;
		}
		return terminated;
	}

	bool IsStopped() { return m_stopped; };
	bool IsRunning() const { return m_running; }
	bool GetAutoDestroy() { return m_autoDestroy; }
	void SetAutoDestroy(bool autoDestroy) { m_autoDestroy = autoDestroy; }
	static int GetThreadCount() { return m_threadCount; }

protected:
	virtual void Run() {} // Virtual function - override in derivatives

private:
	inline static std::mutex m_threadMutex;
	inline static std::atomic<int> m_threadCount{1};
	std::thread::native_handle_type m_threadObj = 0;
	std::atomic<bool> m_running{false};
	std::atomic<bool> m_stopped{false};
	std::atomic<bool> m_autoDestroy{false};

	void thread_handler()
	{
		++m_threadCount;

		Run();

		// read before m_running goes false: an owner waiting for that may destroy
		// the object right away
		const bool autoDestroy = m_autoDestroy;

		--m_threadCount;

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

#include "nzbget_rs.h"

namespace nstring_detail
{
struct VsnprintfCtx
{
	const char* format;
	va_list* ap;
};

inline int RunVsnprintf(void* ctx, char* dst, size_t size)
{
	auto* v = static_cast<VsnprintfCtx*>(ctx);
	return vsnprintf(dst, size, v->format, *v->ap);
}
}

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
	int Length() const { return (int)strlen(m_data); }
	int Capacity() const { return size - 1; }
	bool Empty() const { return !*m_data; }
	void Clear() { m_data[0] = '\0'; }
	const char* Str() const { return m_data; }
	operator char*() const { return const_cast<char*>(m_data); }
	char* operator*() const { return const_cast<char*>(m_data); }
	void Set(const char* str, int len = 0)
	{
		nzbget_rs_bstring_set(m_data, size, str, len);
	}
	void Append(const char* str, int len = 0)
	{
		nzbget_rs_bstring_append(m_data, size, str, len);
	}
	void AppendFmt(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		AppendFmtV(format, ap);
		va_end(ap);
	}
	void AppendFmtV(const char* format, va_list ap)
	{
		va_list apCopy;
		va_copy(apCopy, ap);
		nstring_detail::VsnprintfCtx ctx{ format, &apCopy };
		nzbget_rs_bstring_append_fmt(m_data, size, &ctx, nstring_detail::RunVsnprintf);
		va_end(apCopy);
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
		va_list apCopy;
		va_copy(apCopy, ap);
		nstring_detail::VsnprintfCtx ctx{ format, &apCopy };
		int len = nzbget_rs_bstring_format(m_data, size, &ctx, nstring_detail::RunVsnprintf);
		va_end(apCopy);
		return len;
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
	bool operator==(const CString& other) const
	{
		return nzbget_rs_cstring_eq(m_data, other.m_data) != 0;
	}
	bool operator==(const char* other) const
	{
		return nzbget_rs_cstring_eq(m_data, other) != 0;
	}
	static CString FormatStr(const char* format, ...)
	{
		CString result;
		va_list ap;
		va_start(ap, format);
		result.FormatV(format, ap);
		va_end(ap);
		return result;
	}
	operator char*() const { return m_data; }
	char* operator*() const { return m_data; }
	const char* Str() const { return m_data ? m_data : ""; }
	int Length() const { return m_data ? (int)strlen(m_data) : 0; }
	bool Empty() const { return !m_data || !*m_data; }
	void Clear()
	{
		nzbget_rs_cstring_clear(&m_data);
	}
	void Reserve(int capacity)
	{
		nzbget_rs_cstring_reserve(&m_data, capacity);
	}
	void Bind(char* str)
	{
		nzbget_rs_cstring_bind(&m_data, str);
	}
	char* Unbind()
	{
		return nzbget_rs_cstring_unbind(&m_data);
	}
	void Set(const char* str, int len = 0)
	{
		nzbget_rs_cstring_set(&m_data, str, len);
	}
	void Append(const char* str, int len = 0)
	{
		nzbget_rs_cstring_append(&m_data, str, len);
	}
	void AppendFmt(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		AppendFmtV(format, ap);
		va_end(ap);
	}
	void AppendFmtV(const char* format, va_list ap)
	{
		va_list ap2;
		va_copy(ap2, ap);
		int addLen = vsnprintf(nullptr, 0, format, ap);
		if (addLen > 0)
		{
			nstring_detail::VsnprintfCtx ctx{ format, &ap2 };
			nzbget_rs_cstring_append_fmt(&m_data, addLen, &ctx, nstring_detail::RunVsnprintf);
		}
		va_end(ap2);
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
			nstring_detail::VsnprintfCtx ctx{ format, &ap2 };
			res = nzbget_rs_cstring_format(&m_data, newLen, &ctx, nstring_detail::RunVsnprintf);
		}
		va_end(ap2);
		return res;
	}
	int Find(const char* str, int pos = 0)
	{
		return nzbget_rs_cstring_find(m_data, str, pos);
	}
	void Replace(int pos, int len, const char* str, int strLen = 0)
	{
		nzbget_rs_cstring_replace_range(&m_data, pos, len, str, strLen);
	}
	void Replace(const char* from, const char* to)
	{
		nzbget_rs_cstring_replace_all(&m_data, from, to);
	}
	void TrimRight()
	{
		nzbget_rs_cstring_trim_right(m_data);
	}

protected:
	char* m_data = nullptr;
};

/*
StringBuilder preallocates storage space and is best suitable for often "Append"s.
 */
class StringBuilder
{
public:
	~StringBuilder()
	{
		Clear();
	}
	StringBuilder() = default;
	StringBuilder(const StringBuilder& other) = delete;
	StringBuilder& operator=(const StringBuilder& other) = delete;
	StringBuilder& operator=(StringBuilder&& other) = delete;
	operator const char*() const { return m_data ? m_data : ""; }
	explicit operator char*() { return m_data; }
	const char* operator*() const { return m_data; }
	int Length() const { return m_length; }
	void SetLength(int length) { m_length = length; }
	int Capacity() const { return m_capacity; }
	void Reserve(int capacity, bool exact = false)
	{
		nzbget_rs_string_builder_reserve(&m_data, &m_capacity, capacity, exact ? 1 : 0);
	}
	bool Empty() const { return m_length == 0; }
	void Clear()
	{
		nzbget_rs_string_builder_clear(&m_data, &m_length, &m_capacity);
	}
	void Append(const char* str, int len = 0)
	{
		nzbget_rs_string_builder_append(&m_data, &m_length, &m_capacity, str, len);
	}
	void AppendFmt(const char* format, ...) PRINTF_SYNTAX(2)
	{
		va_list ap;
		va_start(ap, format);
		AppendFmtV(format, ap);
		va_end(ap);
	}
	void AppendFmtV(const char* format, va_list ap)
	{
		va_list ap2;
		va_copy(ap2, ap);
		int addLen = vsnprintf(nullptr, 0, format, ap);
		if (addLen > 0)
		{
			nstring_detail::VsnprintfCtx ctx{ format, &ap2 };
			nzbget_rs_string_builder_append_fmt(
				&m_data, &m_length, &m_capacity, addLen, &ctx, nstring_detail::RunVsnprintf);
		}
		va_end(ap2);
	}
	char* Unbind()
	{
		return nzbget_rs_string_builder_unbind(&m_data, &m_length, &m_capacity);
	}

protected:
	char* m_data = nullptr;
	int m_length = 0;
	int m_capacity = 0;
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
		nzbget_rs_char_buffer_reserve(&m_data, &m_size, size);
	}
	void Clear()
	{
		nzbget_rs_char_buffer_clear(&m_data, &m_size);
	}
	operator char*() const { return m_data; }
	char* operator*() const { return m_data; }

protected:
	char* m_data = nullptr;
	int m_size = 0;
};

#ifdef DEBUG
// helper declarations to identify incorrect calls to "free" at compile time
void free(CString str);
void free(StringBuilder str);
void free(CharBuffer str);
#endif

#endif

#endif /* NZBGET_H */

#include <optional>
#include <type_traits>
#include <string_view>


class Util
{
public:
	static bool MatchFileExt(const char* filename, const char* extensionList, const char* listSeparator);
	static std::optional<std::string> 
	FindExecutorProgram(const std::string& filename, const std::string& customPath);

	/*
	* Split command line into arguments.
	* Uses spaces and single quotation marks as separators.
	* May return empty list if bad escaping was detected.
	*/
	static std::vector<CString> SplitCommandLine(const char* commandLine);

	static int64 JoinInt64(uint32 Hi, uint32 Lo);
	static void SplitInt64(int64 Int64, uint32* Hi, uint32* Lo);

	static std::optional<double> StrToFloatChecked(std::string_view str, double minVal, double maxVal);
	static std::optional<int64_t> StrToSignedChecked(std::string_view str, int64_t minVal, int64_t maxVal);
	static std::optional<uint64_t> StrToUnsignedChecked(std::string_view str, uint64_t maxVal);
	static int64_t SafeCastSignedToSigned(int64_t num, int64_t minTo, int64_t maxTo) noexcept;
	static int64_t SafeCastUnsignedToSigned(uint64_t num, uint64_t maxTo) noexcept;
	static uint64_t SafeCastSignedToUnsigned(int64_t num, uint64_t maxTo) noexcept;
	static uint64_t SafeCastUnsignedToUnsigned(uint64_t num, uint64_t maxTo) noexcept;

	template <typename T>
	static std::optional<T> StrToNum(std::string_view str)
	{
		static_assert(std::is_arithmetic_v<T>, "T must be an arithmetic type");

		if constexpr (std::is_floating_point_v<T>)
		{
			auto v = StrToFloatChecked(
				str,
				static_cast<double>(std::numeric_limits<T>::lowest()),
				static_cast<double>(std::numeric_limits<T>::max()));
			return v ? std::optional<T>(static_cast<T>(*v)) : std::nullopt;
		}
		else if constexpr (std::is_signed_v<T>)
		{
			auto v = StrToSignedChecked(
				str,
				static_cast<int64_t>(std::numeric_limits<T>::min()),
				static_cast<int64_t>(std::numeric_limits<T>::max()));
			return v ? std::optional<T>(static_cast<T>(*v)) : std::nullopt;
		}
		else
		{
			auto v = StrToUnsignedChecked(
				str,
				static_cast<uint64_t>(std::numeric_limits<T>::max()));
			return v ? std::optional<T>(static_cast<T>(*v)) : std::nullopt;
		}
	}

	static void TrimRight(char* str);
	static void TrimRight(std::string& str);
	static void TrimLeft(std::string& str);
	static char* Trim(char* str);
	static void Trim(std::string& str);
	static bool IsControlChar(char c) noexcept;
	static void SanitizeLine(std::string& str);
	static bool EmptyStr(const char* str) { return !str || !*str; }
	static std::vector<CString> SplitStr(const char* str, const char* separators);
	static bool EndsWith(std::string_view str, std::string_view suffix, bool caseSensitive);
	static bool EndsWith(const char* str, const char* suffix, bool caseSensitive);
	static bool AlphaNum(const char* str);

	template<typename T, typename U>
	static constexpr bool CmpEqual(T t, U u) noexcept
	{
		if constexpr (std::is_signed_v<T> == std::is_signed_v<U>)
		{
			return t == u;
		}
		else if constexpr (std::is_signed_v<T>)
		{
			return t >= 0 && std::make_unsigned_t<T>(t) == u;
		}
		else
		{
			return u >= 0 && std::make_unsigned_t<U>(u) == t;
		}
	}
	
	template<typename T, typename U>
	static constexpr bool CmpNotEqual(T t, U u) noexcept
	{
		return !CmpEqual(t, u);
	}
	
	template<typename T, typename U>
	static constexpr bool CmpLess(T t, U u) noexcept
	{
		if constexpr (std::is_signed_v<T> == std::is_signed_v<U>)
		{
			return t < u;
		}
		else if constexpr (std::is_signed_v<T>)
		{
			return t < 0 || std::make_unsigned_t<T>(t) < u;
		}
		else
		{
			return u >= 0 && t < std::make_unsigned_t<U>(u);
		}
	}
	
	template<typename T, typename U>
	static constexpr bool CmpGreater(T t, U u) noexcept
	{
		return CmpLess(u, t);
	}
	
	template<typename T, typename U>
	static constexpr bool CmpLessEqual(T t, U u) noexcept
	{
		return !CmpLess(u, t);
	}
	
	template<typename T, typename U>
	static constexpr bool CmpGreaterEqual(T t, U u) noexcept
	{
		return !CmpLess(t, u);
	}
	
	template <typename From, typename To,
	 	typename std::enable_if_t<std::is_integral_v<From> && std::is_integral_v<To>, bool> = true>
	static To SafeIntCast(From num) noexcept
	{
		if constexpr (std::is_unsigned_v<From> && std::is_signed_v<To>)
		{
			return static_cast<To>(SafeCastUnsignedToSigned(
				static_cast<uint64_t>(num),
				static_cast<uint64_t>(std::numeric_limits<To>::max())));
		}
		else if constexpr (std::is_signed_v<From> && std::is_unsigned_v<To>)
		{
			return static_cast<To>(SafeCastSignedToUnsigned(
				static_cast<int64_t>(num),
				static_cast<uint64_t>(std::numeric_limits<To>::max())));
		}
		else if constexpr (std::is_signed_v<From> && std::is_signed_v<To>)
		{
			return static_cast<To>(SafeCastSignedToSigned(
				static_cast<int64_t>(num),
				static_cast<int64_t>(std::numeric_limits<To>::min()),
				static_cast<int64_t>(std::numeric_limits<To>::max())));
		}
		else
		{
			return static_cast<To>(SafeCastUnsignedToUnsigned(
				static_cast<uint64_t>(num),
				static_cast<uint64_t>(std::numeric_limits<To>::max())));
		}
	}
	

	/* replace all occurences of szFrom to szTo in string szStr with a limitation that szTo must be shorter than szFrom */
	static char* ReduceStr(char* str, const char* from, const char* to);

	/* Calculate Hash using Bob Jenkins (1996) algorithm */
	static uint32 HashBJ96(const char* buffer, int bufSize, uint32 initValue);

	static void SetStandByMode(bool standBy);

	static time_t CurrentTime();
	static int64 CurrentTicks();
	static void Sleep(int milliseconds);

	/* cross platform version of GNU timegm, which is similar to mktime but takes an UTC time as parameter */
	static time_t Timegm(tm const *t);

	static bool StrCaseCmp(std::string_view a, std::string_view b);

	static void FormatTime(time_t timeSec, char* buffer, int bufsize);
	static CString FormatTime(time_t timeSec);

	static CString FormatSpeed(int64 bytesPerSecond);
	static CString FormatSize(int64 fileSize);
	static CString FormatBuffer(const char* buf, int len);

	/*
	* Returns program version and revision number as string formatted like "0.7.0-r295".
	* If revision number is not available only version is returned ("0.7.0").
	*/
	static const char* VersionRevision() { return VersionRevisionBuf; }

	static char VersionRevisionBuf[100];

	static void Init();

	/*
	* Returns number of available CPU cores or -1 if it could not be determined
	*/
	static int NumberOfCpuCores();
};

class WebUtil
{
public:
	static uint32 DecodeBase64(char* inputBuffer, int inputBufferLength, char* outputBuffer);

	/*
	* Encodes string to be used as content of xml-tag.
	*/
	static CString XmlEncode(const char* raw);

	/*
	* Decodes string from xml.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void XmlDecode(char* raw);

	/*
	* Returns pointer to tag-content and length of content in iValueLength
	* The returned pointer points to the part of source-string, no additional strings are allocated.
	*/
	static const char* XmlFindTag(const char* xml, const char* tag, int* valueLength);

	/*
	* Parses tag-content into szValueBuf.
	*/
	static bool XmlParseTagValue(const char* xml, const char* tag, char* valueBuf, int valueBufSize, const char** tagEnd);

	/*
	* Replaces all tags with spaces effectively providing the text content only.
	* The string is transformed in-place overwriting the previous content.
	*/
	static void XmlStripTags(char* xml);

	/*
	* Replaces all entities with spaces.
	* The string is transformed in-place overwriting the previous content.
	*/
	static void XmlRemoveEntities(char* raw);

	/*
	* Creates JSON-string by replace the certain characters with escape-sequences.
	*/
	static CString JsonEncode(const char* raw);

	/*
	* Decodes JSON-string.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void JsonDecode(char* raw);

	/*
	* Returns pointer to field-content and length of content in iValueLength
	* The returned pointer points to the part of source-string, no additional strings are allocated.
	*/
	static const char* JsonFindField(const char* jsonText, const char* fieldName, int* valueLength);

	/*
	* Returns pointer to field-content and length of content in iValueLength
	* The returned pointer points to the part of source-string, no additional strings are allocated.
	*/
	static const char* JsonNextValue(const char* jsonText, int* valueLength);

	/*
	* Unquote http quoted string.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void HttpUnquote(char* raw);

	/*
	* Extracts file name from the value of "Content-Disposition" HTTP header (RFC 6266).
	* The extended parameter "filename*" (RFC 5987) takes precedence over "filename".
	* Returns an empty string if the header doesn't contain a file name.
	*/
	static CString ParseContentDispositionFilename(const char* contentDisposition);

	/*
	* Decodes URL-string.
	* The string is decoded on the place overwriting the content of raw-data.
	*/
	static void UrlDecode(char* raw);

	/*
	* Makes valid URL by replacing of spaces with "%20".
	*/
	static CString UrlEncode(const char* raw);

	/*
	* Converts ISO-8859-1 (aka Latin-1) into UTF-8.
	*/
	static CString Latin1ToUtf8(const char* str);

	static time_t ParseRfc822DateTime(const char* dateTimeStr);
};

class URL
{
public:
	URL(const char* address);
	bool IsValid() { return m_valid; }
	const char* GetAddress() { return m_address; }
	const char* GetProtocol() { return m_protocol; }
	const char* GetUser() { return m_user; }
	const char* GetPassword() { return m_password; }
	const char* GetHost() { return m_host; }
	const char* GetResource() { return m_resource; }
	int GetPort() { return m_port; }
	bool GetTls() { return m_tls; }

private:
	CString m_address;
	CString m_protocol;
	CString m_user;
	CString m_password;
	CString m_host;
	CString m_resource;
	int m_port = 0;
	bool m_tls = false;
	bool m_valid = false;

	void ParseUrl();
};

class RegEx
{
public:
	RegEx(const char *pattern, int matchBufSize = 100);
	~RegEx();
	bool IsValid();
	bool Match(const char* str);
	int GetMatchCount();
	int GetMatchStart(int index);
	int GetMatchLen(int index);

private:
#ifdef HAVE_REGEX_H
	regex_t m_context;
#endif
	void* m_rsRegex = nullptr;
};

class WildMask
{
public:
	WildMask(const char* pattern, bool wantsPositions = false):
		m_pattern(pattern), m_wantsPositions(wantsPositions), m_wildCount(0) {}
	bool Match(const char* text);
	int GetMatchCount() { return m_wildCount; }
	int GetMatchStart(int index) { return m_wildStart[index]; }
	int GetMatchLen(int index) { return m_wildLen[index]; }

private:
	typedef std::vector<int> IntList;

	CString m_pattern;
	bool m_wantsPositions;
	int m_wildCount;
	IntList m_wildStart;
	IntList m_wildLen;

	void ExpandArray();
};

#ifndef DISABLE_GZIP
class ZLib
{
public:
	/*
	* calculates the size required for output buffer
	*/
	static uint32 GZipLen(int inputBufferLength);

	/*
	* compresses inputBuffer and returns the size of bytes written to
	* outputBuffer or 0 if the buffer is too small or an error occured.
	*/
	static uint32 GZip(const void* inputBuffer, int inputBufferLength, void* outputBuffer, int outputBufferLength);
};

class GUnzipStream
{
public:
	enum EStatus
	{
		zlError,
		zlFinished,
		zlOK
	};

	GUnzipStream(int BufferSize);
	~GUnzipStream();

	/*
	* set next memory block for uncompression
	*/
	void Write(const void *inputBuffer, int inputBufferLength);

	/*
	* get next uncompressed memory block.
	* iOutputBufferLength - the size of uncompressed block. if it is "0" the next compressed block must be provided via "Write".
	*/
	EStatus Read(const void **outputBuffer, int *outputBufferLength);

private:
	void* m_rsStream = nullptr;
};
#endif

class Tokenizer
{
public:
	Tokenizer(const char* dataString, const char* separators);
	Tokenizer(char* dataString, const char* separators, bool inplaceBuf);
	char* Next();

private:
	BString<1024> m_shortString;
	CString m_longString;
	char* m_dataString;
	const char* m_separators;
	char* m_savePtr = nullptr;
	bool m_working = false;
};

class Crc32
{
public:
	Crc32() { Reset(); }
	void Reset();
	void Append(uchar* block, uint32 length);
	uint32 Finish();
	static uint32 Combine(uint32 crc1, uint32 crc2, uint32 len2);

private:
	alignas(16) uint32_t m_state[4 * 5]; // = YEncode::crc_state
	void* State() { return &m_state; }
};

#include <optional>
#include <sstream>
#include <array>
#include <algorithm>
#include "nzbget_rs.h"
#include <climits>
#include <stdexcept>
#include <limits>
#include <memory>

// Keep the Rust allocation owned even if std::string's allocation throws.
static inline std::string CopyRustPathText(NzbgetRsBuf buf)
{
	const auto release = [buf](char*) { nzbget_rs_free(buf); };
	const std::unique_ptr<char, decltype(release)> owner(buf.data, release);
	return std::string(buf.data, buf.len);
}

inline char Util::VersionRevisionBuf[100];

inline void Util::Init()
{
	snprintf(VersionRevisionBuf, sizeof(VersionRevisionBuf), "%s", VERSION);
	// init static vars there
	CurrentTicks();
}

inline std::optional<std::string>
Util::FindExecutorProgram(const std::string& filename, const std::string& customPath)
{
	NzbgetRsBuf buf{};
	if (nzbget_rs_util_find_executor_program(
		filename.data(), filename.size(), customPath.data(), customPath.size(), &buf) == 0)
	{
		nzbget_rs_free(buf);
		return std::nullopt;
	}
	return CopyRustPathText(buf);
}

inline int64 Util::JoinInt64(uint32 Hi, uint32 Lo)
{
	return nzbget_rs_ufs_util_join_int64(Hi, Lo);
}

inline void Util::SplitInt64(int64 Int64, uint32* Hi, uint32* Lo)
{
	nzbget_rs_ufs_util_split_int64(Int64, Hi, Lo);
}

inline std::optional<double> Util::StrToFloatChecked(std::string_view str, double minVal, double maxVal)
{
	double out = 0.0;
	if (nzbget_rs_util_str_to_f64_checked(str.data(), str.size(), minVal, maxVal, &out) != 0)
	{
		return out;
	}
	return std::nullopt;
}

inline std::optional<int64_t> Util::StrToSignedChecked(std::string_view str, int64_t minVal, int64_t maxVal)
{
	int64_t out = 0;
	if (nzbget_rs_util_str_to_i64_checked(str.data(), str.size(), minVal, maxVal, &out) != 0)
	{
		return out;
	}
	return std::nullopt;
}

inline std::optional<uint64_t> Util::StrToUnsignedChecked(std::string_view str, uint64_t maxVal)
{
	uint64_t out = 0;
	if (nzbget_rs_util_str_to_u64_checked(str.data(), str.size(), maxVal, &out) != 0)
	{
		return out;
	}
	return std::nullopt;
}

inline int64_t Util::SafeCastSignedToSigned(int64_t num, int64_t minTo, int64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_i64_to_i64(num, minTo, maxTo);
}

inline int64_t Util::SafeCastUnsignedToSigned(uint64_t num, uint64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_u64_to_i64(num, maxTo);
}

inline uint64_t Util::SafeCastSignedToUnsigned(int64_t num, uint64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_i64_to_u64(num, maxTo);
}

inline uint64_t Util::SafeCastUnsignedToUnsigned(uint64_t num, uint64_t maxTo) noexcept
{
	return nzbget_rs_util_safe_int_cast_u64_to_u64(num, maxTo);
}

inline bool Util::IsControlChar(char c) noexcept
{
	return nzbget_rs_util_is_control_char(static_cast<uint8_t>(c)) != 0;
}

inline CString Util::FormatSize(int64 fileSize)
{
	// rust/src/util.rs
	NzbgetRsBuf buf = nzbget_rs_format_size(fileSize);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline CString Util::FormatSpeed(int64 bytesPerSecond)
{
	// rust/src/util.rs
	NzbgetRsBuf buf = nzbget_rs_format_speed(bytesPerSecond);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline void Util::FormatTime(time_t timeSec, char* buffer, int bufsize)
{
	nzbget_rs_util_format_time(static_cast<int64_t>(timeSec), buffer, bufsize);
}

inline CString Util::FormatTime(time_t timeSec)
{
	CString result;
	result.Reserve(50);
	FormatTime(timeSec, result, 50);
	return result;
}

inline CString Util::FormatBuffer(const char* buf, int len)
{
	// rust/src/util.rs
	NzbgetRsBuf out = nzbget_rs_format_buffer(buf, len);
	CString result(out.data, static_cast<int>(out.len));
	nzbget_rs_free(out);
	return result;
}

namespace
{
	// tolower as strcasecmp applies it (an unsigned byte), and as WildMask's
	// C++ code did (a char, signed or not)
	int FileExtCaseFold(int byte)
	{
		return tolower(byte);
	}

	int FileExtMaskFold(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		return tolower(ch);
#else
		return ch < 0 ? ch : tolower(ch);
#endif
	}
}

inline bool Util::MatchFileExt(const char* filename, const char* extensionList, const char* listSeparator)
{
	// rust/src/util.rs
#ifdef __GLIBC__
	const int* table = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
	const int* table = nullptr;
#endif
	return nzbget_rs_match_file_ext(filename, extensionList, listSeparator, table, CHAR_MIN < 0,
		FileExtCaseFold, FileExtMaskFold) != 0;
}

inline std::vector<CString> Util::SplitCommandLine(const char* commandLine)
{
	// rust/src/util.rs: the words back to back, each NUL-terminated
	NzbgetRsBuf words = nzbget_rs_split_command_line(commandLine);
	std::vector<CString> result;
	try
	{
		for (size_t i = 0; i < words.len; i += strlen(words.data + i) + 1)
		{
			result.emplace_back(words.data + i);
		}
	}
	catch (...)
	{
		nzbget_rs_free(words);
		throw;
	}
	nzbget_rs_free(words);
	return result;
}

inline void Util::TrimRight(char* str)
{
	// rust/src/util.rs
	nzbget_rs_trim_line(str, 1);
}

namespace
{
	int TextRightSpace(int byte)
	{
		// The caller can override char's signedness independently of Rust.
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		// glibc supports negative signed chars, keeping EOF distinct from 255.
		return std::isspace(ch);
#else
		// Negative chars other than EOF are outside the portable ctype domain.
		return ch < 0 ? 0 : std::isspace(ch);
#endif
	}
}

inline void Util::TrimRight(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	str.resize(nzbget_rs_trim_string(str.data(), str.size(), 0, 1, 0, &start, TextRightSpace));
}

inline void Util::TrimLeft(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	nzbget_rs_trim_string(str.data(), str.size(), 1, 0, 0, &start, TextRightSpace);
	str.erase(0, start);
}

inline char* Util::Trim(char* str)
{
	// rust/src/util.rs
	return nzbget_rs_trim_line(str, 0);
}

inline void Util::Trim(std::string& str)
{
	TrimLeft(str);
	TrimRight(str);
}

inline void Util::SanitizeLine(std::string& str)
{
	// rust/src/util.rs
	size_t start;
	str.resize(nzbget_rs_trim_string(str.data(), str.size(), 1, 1, 1, &start, TextRightSpace));
	str.erase(0, start);
}

inline char* Util::ReduceStr(char* str, const char* from, const char* to)
{
	// rust/src/util.rs
	nzbget_rs_reduce_str(str, from, to);
	return str;
}

inline std::vector<CString> Util::SplitStr(const char* str, const char* separators)
{
	std::vector<CString> result;
	Tokenizer tok(str, separators);
	while (const char* substr = tok.Next())
	{
		result.emplace_back(substr);
	}
	return result;
}

inline bool Util::EndsWith(std::string_view str, std::string_view suffix, bool caseSensitive)
{
	// rust/src/util.rs
	return nzbget_rs_ends_with(str.data(), str.size(), suffix.data(), suffix.size(), caseSensitive) != 0;
}

inline bool Util::EndsWith(const char* str, const char* suffix, bool caseSensitive)
{
	if (!str)
	{
		return false;
	}
	if (EmptyStr(suffix))
	{
		return true;
	}
	return EndsWith(std::string_view(str), std::string_view(suffix), caseSensitive);
}

inline bool Util::AlphaNum(const char* str)
{
	// rust/src/util.rs
	return nzbget_rs_alpha_num(str) != 0;
}

inline uint32 Util::HashBJ96(const char* buffer, int bufSize, uint32 initValue)
{
	// rust/src/util.rs
	return nzbget_rs_hash_bj96(buffer, bufSize, initValue);
}

inline time_t Util::CurrentTime()
{
	return ::time(nullptr);
}

inline time_t Util::Timegm(tm const *t)
{
	return (time_t)nzbget_rs_ufs_util_internal_timegm(
		t->tm_year, t->tm_mon, t->tm_mday, t->tm_hour, t->tm_min, t->tm_sec);
}

inline bool Util::StrCaseCmp(std::string_view a, std::string_view b)
{
	return nzbget_rs_dqh_slice_eq_ignore_case(
		a.data(), a.size(), b.data(), b.size()) != 0;
}

// prevent PC from going to sleep
inline void Util::SetStandByMode([[maybe_unused]] bool standBy)
{
}

inline int Util::NumberOfCpuCores()
{
	return std::thread::hardware_concurrency();
}

inline int64 Util::CurrentTicks()
{
	timeval t;
	gettimeofday(&t, nullptr);
	return (int64)(t.tv_sec) * 1000000ll + (int64)(t.tv_usec);
}

inline void Util::Sleep(int ms)
{
	std::this_thread::sleep_for(std::chrono::milliseconds(ms));
}

inline uint32 WebUtil::DecodeBase64(char* inputBuffer, int inputBufferLength, char* outputBuffer)
{
	// rust/src/decode.rs
	return nzbget_rs_decode_base64(inputBuffer, inputBufferLength, outputBuffer);
}

/* END - Base64
*/

inline CString WebUtil::XmlEncode(const char* raw)
{
	// rust/src/escape.rs
	NzbgetRsBuf buf = nzbget_rs_xml_encode(raw);
	// CString adds one to an int length before allocating.
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

namespace
{
	// Numeric XML references use the caller's locale-specific case mapping.
	int XmlDigitLower(int byte)
	{
		return tolower(byte);
	}
}

inline void WebUtil::XmlDecode(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_xml_decode(raw, XmlDigitLower);
}

inline const char* WebUtil::XmlFindTag(const char* xml, const char* tag, int* valueLength)
{
	// rust/src/webutil.rs
	return nzbget_rs_xml_find_tag(xml, tag, valueLength);
}

inline bool WebUtil::XmlParseTagValue(const char* xml, const char* tag, char* valueBuf, int valueBufSize, const char** tagEnd)
{
	int valueLen = 0;
	const char* value = XmlFindTag(xml, tag, &valueLen);
	if (!value)
	{
		return false;
	}
	int len = nzbget_rs_ufs_webutil_xml_calc_copy_len(valueLen, valueBufSize);
	strncpy(valueBuf, value, len);
	valueBuf[len] = '\0';
	if (tagEnd)
	{
		*tagEnd = value + valueLen;
	}
	return true;
}

inline void WebUtil::XmlStripTags(char* xml)
{
	// rust/src/text.rs
	nzbget_rs_xml_strip_tags(xml);
}

namespace
{
	// Classify the byte as the caller's char, preserving glibc's legacy
	// negative-byte entries (in particular, EOF is distinct from byte 255).
	int XmlEntityAlpha(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		return (*__ctype_b_loc())[ch] & _ISalpha;
#else
		// Darwin and musl do not classify negative chars as alphabetic.
		// Avoid passing them outside the standard isalpha domain.
		return ch < 0 ? 0 : isalpha(ch);
#endif
	}
}

inline void WebUtil::XmlRemoveEntities(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_xml_remove_entities(raw, XmlEntityAlpha);
}

inline CString WebUtil::JsonEncode(const char* raw)
{
	// rust/src/escape.rs
	NzbgetRsBuf buf = nzbget_rs_json_encode(raw);
	// CString adds one to an int length before allocating.
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline void WebUtil::JsonDecode(char* raw)
{
	// rust/src/decode.rs
	nzbget_rs_json_decode(raw);
}

inline const char* WebUtil::JsonFindField(const char* jsonText, const char* fieldName, int* valueLength)
{
	// rust/src/webutil.rs
	return nzbget_rs_json_find_field(jsonText, fieldName, valueLength);
}

inline const char* WebUtil::JsonNextValue(const char* jsonText, int* valueLength)
{
	// rust/src/decode.rs
	return nzbget_rs_json_next_value(jsonText, valueLength);
}

inline void WebUtil::HttpUnquote(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_http_unquote(raw);
}

namespace
{
	// tolower as strncasecmp applies it: to an unsigned byte
	int CaseFold(int byte)
	{
		return tolower(byte);
	}
}

inline CString WebUtil::ParseContentDispositionFilename(const char* contentDisposition)
{
	// rust/src/webutil.rs
#ifdef __GLIBC__
	// what strncasecmp reads: the tolower table of this thread's locale
	const int* table = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
	const int* table = nullptr;
#endif
	NzbgetRsBuf buf = nzbget_rs_content_disposition_filename(contentDisposition, table, CaseFold);
	if (!buf.data)
	{
		return CString();
	}
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline void WebUtil::UrlDecode(char* raw)
{
	// rust/src/text.rs
	nzbget_rs_url_decode(raw);
}

inline CString WebUtil::UrlEncode(const char* raw)
{
	// rust/src/text.rs
	NzbgetRsBuf buf = nzbget_rs_url_encode(raw);
	if (buf.len >= static_cast<size_t>(std::numeric_limits<int>::max()))
	{
		nzbget_rs_free(buf);
		std::abort();
	}
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

namespace
{
	// Caller-owned bounded encodings; Reserve allocates bound+1 bytes and
	// the final NUL supplies CString's logical Length().
	template <class Write>
	CString RustEncodedKnownResult(size_t inputLen, size_t expansion, Write write)
	{
		if (inputLen > (static_cast<size_t>(std::numeric_limits<int>::max()) - 1) / expansion)
		{
			std::abort();
		}
		size_t bound = inputLen * expansion;
		CString result;
		result.Reserve(static_cast<int>(bound));
		char* output = static_cast<char*>(result);
		if (!output || write(output, bound + 1) > bound)
		{
			// Includes SIZE_MAX rejection; never request an unbounded retry.
			std::abort();
		}
		return result;
	}

}

inline CString WebUtil::Latin1ToUtf8(const char* str)
{
	// rust/src/text.rs
	size_t inputLen = str ? strlen(str) : 0;
	return RustEncodedKnownResult(inputLen, 2, [str, inputLen](char* out, size_t cap) {
		return nzbget_rs_latin1_to_utf8_n(str, inputLen, out, cap);
	});
}

/*
 The date/time can be formatted according to RFC822 in different ways. Examples:
   Wed, 26 Jun 2013 01:02:54 -0600
   Wed, 26 Jun 2013 01:02:54 GMT
   26 Jun 2013 01:02:54 -0600
   26 Jun 2013 01:02 -0600
   26 Jun 2013 01:02 A
 The day name and the seconds are optional; the zone is an offset (+hhmm or
 -hhmm) or a name (UT, GMT, UTC, Z and the US zones). Military zones and
 unknown names count as UTC. Only the first format was read before: the others
 (GMT is common in feeds) gave no time at all.
*/
inline time_t WebUtil::ParseRfc822DateTime(const char* dateTimeStr)
{
	// rust/src/util.rs
	return static_cast<time_t>(nzbget_rs_parse_rfc822_date_time(dateTimeStr));
}


inline URL::URL(const char* address) :
	m_address(address)
{
	if (address)
	{
		ParseUrl();
	}
}

inline void URL::ParseUrl()
{
	// rust/src/url.rs
	NzbgetRsUrlParts r;
	nzbget_rs_parse_url(m_address, &r);
	if (!r.valid)
	{
		return;
	}
	CString* fields[] = {&m_protocol, &m_user, &m_password, &m_host, &m_resource};
	for (int i = 0; i < 5; i++)
	{
		if (r.parts[i][0] >= 0)
		{
			// a part of the address; an empty one (only the host can be) as ""
			fields[i]->Set(r.parts[i][1] > 0 ? m_address + r.parts[i][0] : "", static_cast<int>(r.parts[i][1]));
		}
	}
	if (r.parts[4][0] < 0)
	{
		m_resource = "/";
	}
	m_port = r.port;
	m_valid = true;
}


inline RegEx::RegEx(const char *pattern, int matchBufSize)
{
#ifdef HAVE_REGEX_H
	bool valid = regcomp(&m_context, pattern, REG_EXTENDED | REG_ICASE | (matchBufSize > 0 ? 0 : REG_NOSUB)) == 0;
#else
	bool valid = false;
#endif
	m_rsRegex = nzbget_rs_regex_new(valid ? 1 : 0, matchBufSize);
}

inline RegEx::~RegEx()
{
	nzbget_rs_regex_free(m_rsRegex);
	m_rsRegex = nullptr;
#ifdef HAVE_REGEX_H
	regfree(&m_context);
#endif
}

inline bool RegEx::IsValid()
{
	return nzbget_rs_regex_is_valid(m_rsRegex) != 0;
}

inline bool RegEx::Match(const char *str)
{
#ifdef HAVE_REGEX_H
	struct ExecCtx
	{
		regex_t* context;
		const char* str;
	} ctx{&m_context, str};

	return nzbget_rs_regex_match(
		m_rsRegex, &ctx,
		[](void* raw, int32_t* outSo, int32_t* outEo, size_t nmatch) -> int {
			auto* c = static_cast<ExecCtx*>(raw);
			std::vector<regmatch_t> matches(nmatch);
			regmatch_t* pmatch = nmatch > 0 ? matches.data() : nullptr;
			if (regexec(c->context, c->str, nmatch, pmatch, 0) != 0)
			{
				return 0;
			}
			for (size_t i = 0; i < nmatch; i++)
			{
				outSo[i] = static_cast<int32_t>(matches[i].rm_so);
				outEo[i] = static_cast<int32_t>(matches[i].rm_eo);
			}
			return 1;
		}) != 0;
#else
	(void)str;
	return false;
#endif
}

inline int RegEx::GetMatchCount()
{
	return nzbget_rs_regex_get_match_count(m_rsRegex);
}

inline int RegEx::GetMatchStart(int index)
{
	return nzbget_rs_regex_get_match_start(m_rsRegex, index);
}

inline int RegEx::GetMatchLen(int index)
{
	return nzbget_rs_regex_get_match_len(m_rsRegex, index);
}


namespace
{
	// tolower of a byte as WildMask's C++ code saw it: a char (signed or not, as
	// on the platform) in the current locale, EOF kept distinct from 255
	int WildMaskFold(int byte)
	{
		int ch = static_cast<char>(byte);
#ifdef __GLIBC__
		// glibc supports negative signed chars for legacy programs
		return tolower(ch);
#else
		// Darwin and musl leave negative characters unchanged; don't pass them
		// outside the standard tolower domain
		return ch < 0 ? ch : tolower(ch);
#endif
	}
}

inline bool WildMask::Match(const char* text)
{
	// rust/src/wildmask.rs
#ifdef __GLIBC__
	// what tolower reads: the table of this thread's locale
	const int* table = reinterpret_cast<const int*>(*__ctype_tolower_loc());
#else
	const int* table = nullptr;
#endif
	m_wildCount = 0;
	m_wildStart.clear();
	m_wildLen.clear();
	if (!m_wantsPositions)
	{
		return nzbget_rs_wild_match(m_pattern, text, nullptr, 0, table, CHAR_MIN < 0, WildMaskFold).matched != 0;
	}

	// Backtracking can retain more captures than there are pattern bytes.
	// Rust reports the total count even for failed or truncated matches.
	size_t capacity = 100;
	std::unique_ptr<int[][2]> positions(new int[capacity][2]);
	auto match = [&]() {
		return nzbget_rs_wild_match(m_pattern, text,
			positions.get(), capacity, table, CHAR_MIN < 0, WildMaskFold);
	};
	auto result = match();
	if (result.count > capacity)
	{
		capacity = result.count;
		positions.reset(new int[capacity][2]);
		result = match();
	}
	if (result.count > static_cast<size_t>(INT_MAX))
	{
		throw std::length_error("Too many wildcard captures");
	}
	m_wildCount = static_cast<int>(result.count);
	m_wildStart.resize(m_wildCount);
	m_wildLen.resize(m_wildCount);
	for (int i = 0; i < m_wildCount; i++)
	{
		m_wildStart[i] = positions[i][0];
		m_wildLen[i] = positions[i][1];
	}
	return result.matched != 0;
}


#ifndef DISABLE_GZIP
inline uint32 ZLib::GZipLen(int inputBufferLength)
{
	return nzbget_rs_gzip_len(inputBufferLength);
}

inline uint32 ZLib::GZip(const void* inputBuffer, int inputBufferLength, void* outputBuffer, int outputBufferLength)
{
	return nzbget_rs_gzip_compress(
		static_cast<const uint8_t*>(inputBuffer),
		inputBufferLength,
		static_cast<uint8_t*>(outputBuffer),
		outputBufferLength);
}

inline GUnzipStream::GUnzipStream(int BufferSize)
	: m_rsStream(nzbget_rs_gunzip_new(BufferSize))
{
}

inline GUnzipStream::~GUnzipStream()
{
	nzbget_rs_gunzip_free(static_cast<RsGUnzipStream*>(m_rsStream));
	m_rsStream = nullptr;
}

inline void GUnzipStream::Write(const void *inputBuffer, int inputBufferLength)
{
	nzbget_rs_gunzip_write(
		static_cast<RsGUnzipStream*>(m_rsStream),
		static_cast<const uint8_t*>(inputBuffer),
		inputBufferLength);
}

inline GUnzipStream::EStatus GUnzipStream::Read(const void **outputBuffer, int *outputBufferLength)
{
	int status = nzbget_rs_gunzip_read(
		static_cast<RsGUnzipStream*>(m_rsStream),
		outputBuffer,
		outputBufferLength);
	if (status == 1)
	{
		return zlFinished;
	}
	if (status == 2)
	{
		return zlOK;
	}
	return zlError;
}
#endif

inline Tokenizer::Tokenizer(const char* dataString, const char* separators) :
	m_separators(separators)
{
	// an optimization to avoid memory allocation for short data string
	int len = strlen(dataString);
	if (nzbget_rs_ufs_tokenizer_fits_short_string(len, m_shortString.Capacity()) != 0)
	{
		m_shortString.Set(dataString);
		m_dataString = m_shortString;
	}
	else
	{
		m_longString.Set(dataString);
		m_dataString = m_longString;
	}

}

inline Tokenizer::Tokenizer(char* dataString, const char* separators, bool inplaceBuf) :
	m_separators(separators)
{
	if (inplaceBuf)
	{
		m_dataString = dataString;
	}
	else
	{
		m_longString.Set(dataString);
		m_dataString = m_longString;
	}
}

inline char* Tokenizer::Next()
{
	char* token = nzbget_rs_dqh_tokenizer_next(
		m_dataString, m_separators, m_working ? 1 : 0, &m_savePtr);
	m_working = true;
	return token;
}

inline void Crc32::Reset()
{
	uint32* currentCrc = static_cast<uint32*>(State());
	*currentCrc = 0;
}

inline void Crc32::Append(uchar* block, uint32 length)
{
	uint32* currentCrc = static_cast<uint32*>(State());
	*currentCrc = nzbget_rs_crc32_append(block, length, *currentCrc);
}

inline uint32 Crc32::Finish()
{
	uint32* currentCrc = static_cast<uint32*>(State());
	return *currentCrc;
}

inline uint32 Crc32::Combine(uint32 crc1, uint32 crc2, uint32 len2)
{
	// rust/src/crc.rs
	return nzbget_rs_crc32_combine(crc1, crc2, len2);
}


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
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


#ifndef FILESYSTEM_H
#define FILESYSTEM_H

#include <optional>
#include <chrono>
#include <filesystem>

namespace fs
{
using namespace std::filesystem;
using error_code = std::error_code;
using errc = std::errc;

inline fs::path u8path(std::string_view pathStr)
{
	return fs::path(pathStr);
}

inline std::string u8string(const path& p)
{
	return p.string();
}
}



namespace fs
{
inline path make_unique_filename()
{
	return std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
}

fs::path make_unique_filename(const fs::path& targetPath);

void move_file(const fs::path& src, const fs::path& dest, fs::error_code& ec) noexcept;

inline void move_file(const fs::path& src, const fs::path& dest)
{
	error_code ec;
	move_file(src, dest, ec);
	if (ec) throw std::runtime_error(ec.message());
}
}

class FileSystem
{
	struct DiskState
	{
		int64 available;
		int64 total;
	};

public:
	static CString GetLastErrorMessage();
	static char* BaseFileName(const char* filename);
	static std::pair<std::string, std::string> SplitPathAndFilename(const std::string& fullPath);
	static bool SameFilename(const char* filename1, const char* filename2);
	static void NormalizePathSeparators(char* path);
	static std::optional<std::string> GetRealPath(const std::string& path);
	static bool LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull);
	static bool SaveBufferIntoFile(const char* filename, const char* buffer, int bufLen);
	static bool AllocateFile(const char* filename, int64 size, bool sparse, CString& errmsg);
	static CString MakeValidFilename(const char* filename, bool allowSlashes = false);
	static std::string SanitizePathSegment(std::string_view name);
	static std::string SanitizeRelativePath(std::string_view path);
	static bool ReservedChar(char ch);
	static CString MakeUniqueFilename(const char* destDir, const char* basename);
	static bool MoveFile(const char* srcFilename, const char* dstFilename);
	static bool CopyFile(const char* srcFilename, const char* dstFilename);
	static bool DeleteFile(const char* filename);
	static bool FileExists(const char* filename);
	static bool DirectoryExists(const char* dirFilename);
	// a symbolic link (or a reparse point on Windows), not followed
	static bool IsLink(const char* filename);
	static bool CreateDirectory(const char* dirFilename);
	static bool CreateDirectoryExclusive(const char* dirFilename);
	static std::string ExtractFilePathFromCmd(const std::string& path);
	static std::string EscapePathForShell(const std::string& path);
	static std::optional<std::string> GetFileExtension(std::string_view filename);

	/* Delete empty directory */
	static bool RemoveDirectory(const char* dirFilename);

	/* Delete directory which is empty or contains only hidden files or directories */
	static bool DeleteDirectory(const char* dirFilename);

	static bool DeleteDirectoryWithContent(const char* dirFilename, CString& errmsg);
	static bool ForceDirectories(const char* path, CString& errmsg);
	static bool CreateHardLink(const char *from, const char *to, CString& errmsg);
	static CString GetCurrentDirectory();
	static int64 FileSize(const char* filename);
	static std::optional<DiskState> GetDiskState(const char* path);
	static bool DirEmpty(const char* dirFilename);
	static bool RenameBak(const char* filename, const char* bakPart, bool removeOldExtension, CString& newName);
	static CString ExpandHomePath(const char* filename);
	static void FixExecPermission(const char* filename);
	static bool RestoreFileOrDirPermissions(const char* filename);
	static bool RestoreFilePermissions(const char* filename);
	static bool RestoreDirPermissions(const char* filename);
	static bool RestorePermissions(const char* filename, mode_t mode);
	static mode_t uMask;
	static CString ExpandFileName(const char* filename);
	static CString GetExeFileName(const char* argv0);

	/* Flush disk buffers for file with given descriptor */
	static bool FlushFileBuffers(int fileDescriptor, CString& errmsg);

	/* Flush disk buffers for file metadata (after file renaming) */
	static bool FlushDirBuffers(const char* filename, CString& errmsg);

	static CString MakeExtendedPath(const char* path, bool force);
};

class DirBrowser
{
public:
	DirBrowser(const char* path, bool snapshot = true);
	~DirBrowser();
	const char* Next();

private:
	DIR* m_dir = nullptr;
	struct dirent* m_findData;

	bool m_snapshot;
	typedef std::deque<CString> FileList;
	FileList m_snapshotFiles;
	FileList::iterator m_snapshotIter;

	const char* InternNext();
};

class DiskFile
{
public:
	enum EOpenMode
	{
		omRead, // file must exist
		omReadWrite, // file must exist
		omWrite, // create new or overwrite existing
		omAppend // create new or append to existing
	};

	enum ESeekOrigin
	{
		soSet,
		soCur,
		soEnd
	};

	DiskFile() = default;
	DiskFile(const DiskFile&) = delete;
	virtual ~DiskFile();
	bool Open(const char* filename, EOpenMode mode);
	bool Close();
	bool Active() { return m_file != nullptr; }
	int64 Read(void* buffer, int64 size);
	int64 Write(const void* buffer, int64 size);
	int64 Position();
	bool Seek(int64 position, ESeekOrigin origin = soSet);
	bool Eof();
	bool Error();
	int64 Print(const char* format, ...) PRINTF_SYNTAX(2);
	virtual char* ReadLine(char* buffer, int64 size);
	bool SetWriteBuffer(int size);
	bool Flush();
	bool Sync(CString& errmsg);
	int GetFileDescriptor() const { return m_file ? fileno(m_file) : -1; }

private:
	FILE* m_file = nullptr;
};

#include "nzbget_rs.h"
#include <exception>
#include <memory>

// Keep the platform's stat ABI and errno behavior (Rust metadata can use
// statx, whose availability probe changes errno, and different time widths).
static inline int RustStat(const char* path)
{
	struct stat buffer;
	if (stat(path, &buffer) != 0)
	{
		return -errno;
	}
	return S_ISREG(buffer.st_mode) ? 1 : S_ISDIR(buffer.st_mode) ? 2 : 0;
}

static inline int RustStatvfs(const char* path, uint64_t* bavail, uint64_t* blocks, uint64_t* frsize)
{
	struct statvfs diskdata;
	if (statvfs(path, &diskdata) != 0)
	{
		return 0;
	}
	*bavail = static_cast<uint64_t>(diskdata.f_bavail);
	*blocks = static_cast<uint64_t>(diskdata.f_blocks);
	*frsize = static_cast<uint64_t>(diskdata.f_frsize);
	return 1;
}

inline CString FileSystem::GetLastErrorMessage()
{
	// rust/src/fsops.rs: the XSI strerror_r (glibc's GNU strerror_r left the
	// buffer empty for every known error, so the messages had no reason)
	BString<1024> msg;
	nzbget_rs_error_message(errno, msg);
	return *msg;
}

inline void FileSystem::NormalizePathSeparators(char* path)
{
	// rust/src/paths.rs
	nzbget_rs_normalize_path_separators(path);
}

inline std::optional<std::string> FileSystem::GetRealPath(const std::string& path)
{
	NzbgetRsBuf buf{};
	if (nzbget_rs_fs_get_real_path(path.data(), path.size(), &buf) == 0)
	{
		nzbget_rs_free(buf);
		return std::nullopt;
	}
	return CopyRustPathText(buf);
}

inline bool FileSystem::ForceDirectories(const char* path, CString& errmsg)
{
	// rust/src/fsops.rs
	NzbgetRsBuf msg{};
	bool ok = nzbget_rs_force_directories(path, &msg, RustStat) != 0;
	errmsg.Clear();
	if (!ok)
	{
		errmsg = msg.data;
	}
	nzbget_rs_free(msg);
	return ok;
}

inline bool FileSystem::CreateHardLink(const char *from, const char *to, CString& errmsg)
{
	NzbgetRsBuf msg{};
	bool ok = nzbget_rs_fs_create_hard_link(from, to, RustStat, &msg) != 0;
	errmsg.Clear();
	if (!ok)
	{
		errmsg.Set(msg.data ? msg.data : "", static_cast<int>(msg.len));
	}
	nzbget_rs_free(msg);
	return ok;
}

inline CString FileSystem::GetCurrentDirectory()
{
	NzbgetRsBuf buf = nzbget_rs_fs_get_current_directory();
	CString res(buf.data ? buf.data : "", static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return res;
}

inline bool FileSystem::DirEmpty(const char* dirFilename)
{
	DirBrowser dir(dirFilename);
	return dir.Next() == nullptr;
}

inline bool FileSystem::LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull)
{
	return nzbget_rs_fs_load_file_into_buffer(
		filename,
		addTrailingNull ? 1 : 0,
		RustStat,
		&buffer,
		[](void* ctx, int bufSize) -> uint8_t* {
			auto* b = static_cast<CharBuffer*>(ctx);
			b->Reserve(bufSize);
			return reinterpret_cast<uint8_t*>(static_cast<char*>(*b));
		}) != 0;
}

inline bool FileSystem::SaveBufferIntoFile(const char* filename, const char* buffer, int bufLen)
{
	return nzbget_rs_fs_save_buffer_into_file(
		filename, buffer, bufLen > 0 ? static_cast<size_t>(bufLen) : 0) != 0;
}

inline bool FileSystem::AllocateFile(const char* filename, int64 size, [[maybe_unused]] bool sparse, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_allocate_file(filename, size, &errBuf) != 0;
	if (errBuf.len == 0)
	{
		errmsg.Clear();
	}
	else
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline char* FileSystem::BaseFileName(const char* filename)
{
	// rust/src/paths.rs
	return const_cast<char*>(filename) + nzbget_rs_path_position(0, filename, strlen(filename));
}

inline std::pair<std::string, std::string> FileSystem::SplitPathAndFilename(const std::string& fullPath)
{
	// rust/src/paths.rs
	size_t slash = nzbget_rs_path_position(1, fullPath.data(), fullPath.size());
	if (slash == SIZE_MAX)
	{
		return std::make_pair(fullPath, "");
	}
	return std::make_pair(fullPath.substr(0, slash), fullPath.substr(slash + 1));
}

inline bool FileSystem::ReservedChar(char ch)
{
	// rust/src/paths.rs
	return nzbget_rs_reserved_char(ch) != 0;
}

//replace bad chars in filename
inline CString FileSystem::MakeValidFilename(const char* filename, bool allowSlashes)
{
	// rust/src/paths.rs
	NzbgetRsBuf buf = nzbget_rs_path_text(0, filename, strlen(filename), allowSlashes);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline std::string FileSystem::SanitizePathSegment(std::string_view name)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(1, name.data(), name.size(), 0));
}

inline std::string FileSystem::SanitizeRelativePath(std::string_view path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(2, path.data(), path.size(), 0));
}

inline CString FileSystem::MakeUniqueFilename(const char* destDir, const char* basename)
{
	// rust/src/fsops.rs
	NzbgetRsBuf buf = nzbget_rs_make_unique_filename(destDir, basename, RustStat);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline fs::path fs::make_unique_filename(const fs::path& targetPath)
{
	return fs::u8path(CopyRustPathText(nzbget_rs_fs_make_unique_path(targetPath.c_str())));
}

inline void fs::move_file(const fs::path& src, const fs::path& dest, fs::error_code& ec) noexcept
{
	int err = nzbget_rs_fs_move_file_no_overwrite(src.c_str(), dest.c_str());
	if (err == 0)
	{
		ec.clear();
	}
	else
	{
		ec = std::make_error_code(static_cast<std::errc>(err));
	}
}

inline bool FileSystem::MoveFile(const char* srcFilename, const char* dstFilename)
{
	return nzbget_rs_fs_move_file(srcFilename, dstFilename) != 0;
}

inline bool FileSystem::CopyFile(const char* srcFilename, const char* dstFilename)
{
	return nzbget_rs_fs_copy_file(srcFilename, dstFilename) != 0;
}

inline bool FileSystem::DeleteFile(const char* filename)
{
	return nzbget_rs_fs_delete_file(filename) != 0;
}

inline bool FileSystem::FileExists(const char* filename)
{
	// rust/src/fsops.rs (stat, a regular file)
	return nzbget_rs_file_exists(filename, RustStat) != 0;
}

inline bool FileSystem::DirectoryExists(const char* dirFilename)
{
	// rust/src/fsops.rs (stat, a directory)
	return nzbget_rs_directory_exists(dirFilename, RustStat) != 0;
}

inline bool FileSystem::CreateDirectory(const char* dirFilename)
{
	nzbget_rs_fs_create_directory_exclusive(dirFilename);
	return DirectoryExists(dirFilename);
}

inline bool FileSystem::CreateDirectoryExclusive(const char* dirFilename)
{
	return nzbget_rs_fs_create_directory_exclusive(dirFilename) != 0;
}

inline bool FileSystem::RemoveDirectory(const char* dirFilename)
{
	return nzbget_rs_fs_remove_directory(dirFilename) != 0;
}


inline std::string FileSystem::ExtractFilePathFromCmd(const std::string& path)
{
	// rust/src/paths.rs
	return path.substr(0, nzbget_rs_path_position(2, path.data(), path.size()));
}

inline std::string FileSystem::EscapePathForShell(const std::string& path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(3, path.data(), path.size(), 0));
}

inline std::optional<std::string> FileSystem::GetFileExtension(std::string_view filename)
{
	// rust/src/fsdelete.rs
	ptrdiff_t extIdx = nzbget_rs_file_extension(filename.data(), filename.size());
	if (extIdx < 0)
		return std::nullopt;

	return std::string(filename.substr(extIdx));
}

namespace
{
enum class PathType
{
	Error,
	File,
	Directory,
	Link,
	DirectoryLink
};

PathType GetPathTypeNoFollow(const char* filename)
{
	struct stat buffer;
	if (lstat(filename, &buffer) != 0)
	{
		return PathType::Error;
	}
	if (S_ISLNK(buffer.st_mode))
	{
		return PathType::Link;
	}
	return S_ISDIR(buffer.st_mode) ? PathType::Directory : PathType::File;
}

bool DeleteLink(const char* filename, PathType pathType)
{
	(void)pathType;
	return FileSystem::DeleteFile(filename);
}

// The calls rust/src/fsdelete.rs makes. errno is kept between the calls as
// the C++ kept it (Rust's own allocations don't touch it) and restored when
// the operation returns; an exception ends the operation and is rethrown.
class RsFsOps
{
public:
	NzbgetRsFsOps ops{};

	RsFsOps() : m_errno(errno)
	{
		ops.ctx = this;
		ops.pathType = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return static_cast<int>(GetPathTypeNoFollow(path)); });
		};
		ops.removeDir = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::RemoveDirectory(path) ? 1 : 0; });
		};
		ops.deleteFile = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::DeleteFile(path) ? 1 : 0; });
		};
		ops.deleteLink = [](void* ctx, const char* path, int type) noexcept -> int
		{
			return Call(ctx, [&] { return DeleteLink(path, type == 4 ? PathType::DirectoryLink : PathType::Link) ? 1 : 0; });
		};
		ops.list = [](void* ctx, const char* dir, void* sink, void (*add)(void*, const char*)) noexcept -> int
		{
			return Call(ctx, [&]
			{
				DirBrowser browser(dir);
				while (const char* filename = browser.Next())
				{
					add(sink, filename);
				}
				return 0;
			});
		};
		ops.fileExists = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::FileExists(path) ? 1 : 0; });
		};
		ops.dirExists = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::DirectoryExists(path) ? 1 : 0; });
		};
		ops.moveFile = [](void* ctx, const char* from, const char* to) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::MoveFile(from, to) ? 1 : 0; });
		};
		ops.lastError = [](void* ctx, void* sink, void (*set)(void*, const char*)) noexcept -> int
		{
			return Call(ctx, [&] { set(sink, FileSystem::GetLastErrorMessage()); return 0; });
		};
		ops.tooLong = [](void* ctx) noexcept -> int
		{
			return Call(ctx, [&] { errno = ENAMETOOLONG; return 0; });
		};
	}

	int Finish(int result)
	{
		errno = m_errno;
		if (m_error)
		{
			std::rethrow_exception(m_error);
		}
		return result;
	}

private:
	int m_errno;
	std::exception_ptr m_error;

	template <class F>
	static int Call(void* ctx, F f) noexcept
	{
		RsFsOps& self = *static_cast<RsFsOps*>(ctx);
		errno = self.m_errno;
		try
		{
			int result = f();
			self.m_errno = errno;
			return result;
		}
		catch (...)
		{
			self.m_errno = errno;
			self.m_error = std::current_exception();
			return -1;
		}
	}
};
}

inline bool FileSystem::IsLink(const char* filename)
{
	PathType type = GetPathTypeNoFollow(filename);
	return type == PathType::Link || type == PathType::DirectoryLink;
}

/* Delete directory which is empty or contains only hidden files or directories (whose names start with dot) */
inline bool FileSystem::DeleteDirectory(const char* dirFilename)
{
	if (!dirFilename)
	{
		errno = EFAULT;
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	return ops.Finish(nzbget_rs_delete_directory(&ops.ops, dirFilename, PATH_SEPARATOR)) == 1;
}

inline bool FileSystem::DeleteDirectoryWithContent(const char* dirFilename, CString& errmsg)
{
	// Keep the POSIX NULL-path error distinct from an empty path, without
	// passing NULL to lstat or printf (their declarations require non-NULL).
	if (!dirFilename)
	{
		errmsg.Clear();
		errno = EFAULT;
		NzbgetRsBuf errBuf = nzbget_rs_dqh_fs_format_null_delete_errmsg(*GetLastErrorMessage());
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
		nzbget_rs_free(errBuf);
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	NzbgetRsBuf text{};
	int result = nzbget_rs_delete_directory_with_content(&ops.ops, dirFilename, PATH_SEPARATOR, &text);
	if (text.len == 0)
	{
		errmsg.Clear();
	}
	else
	{
		errmsg.Set(text.data, (int)text.len);
	}
	nzbget_rs_free(text);
	return ops.Finish(result) == 1;
}

inline int64 FileSystem::FileSize(const char* filename)
{
	return nzbget_rs_fs_file_size(filename);
}

inline std::optional<FileSystem::DiskState> FileSystem::GetDiskState(const char* path)
{
	int64 available = 0;
	int64 total = 0;
	if (nzbget_rs_fs_get_disk_state(path, RustStatvfs, &available, &total) != 0)
	{
		return FileSystem::DiskState{ available, total };
	}
	return std::nullopt;
}

inline bool FileSystem::RenameBak(const char* filename, const char* bakPart, bool removeOldExtension, CString& newName)
{
	if (!filename || !bakPart)
	{
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	NzbgetRsBuf name{};
	int result = nzbget_rs_rename_bak(&ops.ops, filename, bakPart, removeOldExtension, &name);
	newName.Set(name.data, (int)name.len);
	nzbget_rs_free(name);
	return ops.Finish(result) == 1;
}

inline CString FileSystem::ExpandHomePath(const char* filename)
{
	NzbgetRsBuf buf = nzbget_rs_fs_expand_home_path(filename);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline CString FileSystem::ExpandFileName(const char* filename)
{
	NzbgetRsBuf expBuf = nzbget_rs_fs_expand_file_name(filename);
	CString result(expBuf.data ? expBuf.data : "", static_cast<int>(expBuf.len));
	nzbget_rs_free(expBuf);
	return result;
}

inline CString FileSystem::GetExeFileName(const char* argv0)
{
	NzbgetRsBuf exeBuf = nzbget_rs_fs_get_exe_file_name(argv0);
	CString result(exeBuf.data ? exeBuf.data : "", static_cast<int>(exeBuf.len));
	nzbget_rs_free(exeBuf);
	return result;
}

inline bool FileSystem::SameFilename(const char* filename1, const char* filename2)
{
	return nzbget_rs_dqh_bytes_equal(filename1, filename2) != 0;
}

inline bool FileSystem::FlushFileBuffers(int fileDescriptor, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_flush_file_buffers(fileDescriptor, &errBuf) != 0;
	if (!ok)
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline bool FileSystem::FlushDirBuffers(const char* filename, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_flush_dir_buffers(filename, PATH_SEPARATOR, &errBuf) != 0;
	if (!ok)
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline mode_t FileSystem::uMask;

inline void FileSystem::FixExecPermission(const char* filename)
{
	nzbget_rs_fs_fix_exec_permission(filename);
}

inline bool FileSystem::RestoreFileOrDirPermissions(const char* filename)
{
	return nzbget_rs_fs_restore_file_or_dir_permissions(filename, static_cast<uint32_t>(FileSystem::uMask)) != 0;
}

inline bool FileSystem::RestoreFilePermissions(const char* filepath)
{
	return RestorePermissions(filepath, 0666);
}

inline bool FileSystem::RestoreDirPermissions(const char* filename)
{
	return RestorePermissions(filename, 0777);
}

inline bool FileSystem::RestorePermissions(const char* filename, mode_t mode)
{
	return nzbget_rs_fs_restore_permissions(
		filename, static_cast<uint32_t>(mode), static_cast<uint32_t>(FileSystem::uMask)) != 0;
}

inline CString FileSystem::MakeExtendedPath(const char* path, [[maybe_unused]] bool force)
{
	return path;
}


inline DirBrowser::DirBrowser(const char* path, bool snapshot) :
	m_snapshot(snapshot)
{
	if (m_snapshot)
	{
		DirBrowser dir(path, false);
		while (const char* filename = dir.Next())
		{
			m_snapshotFiles.emplace_back(filename);
		}
		m_snapshotIter = m_snapshotFiles.begin();
	}
	else
	{
		m_dir = opendir(path);
	}
}

inline DirBrowser::~DirBrowser()
{
	if (m_dir)
	{
		closedir(m_dir);
	}
}

inline const char* DirBrowser::InternNext()
{
	if (m_snapshot)
	{
		return m_snapshotIter == m_snapshotFiles.end() ? nullptr : **m_snapshotIter++;
	}
	else
	{
		if (m_dir)
		{
			m_findData = readdir(m_dir);
			if (m_findData)
			{
				return m_findData->d_name;
			}
		}
		return nullptr;
	}
}

inline const char* DirBrowser::Next()
{
	const char* filename = nullptr;
	for (filename = InternNext(); filename && nzbget_rs_ufs_dir_browser_is_dot_or_dotdot(filename) != 0; )
	{
		filename = InternNext();
	}
	return filename;
}


inline DiskFile::~DiskFile()
{
	if (m_file)
	{
		Close();
	}
}

inline bool DiskFile::Open(const char* filename, EOpenMode mode)
{
	const char* strmode = mode == omRead ? FOPEN_RB : mode == omReadWrite ?
		FOPEN_RBP : mode == omWrite ? FOPEN_WB : FOPEN_AB;
	int fd = nzbget_rs_fs_open_cloexec_fd(
		filename, (int)mode, (int)omRead, (int)omReadWrite, (int)omWrite);
	if (fd == -1)
	{
		return false;
	}
	m_file = fdopen(fd, strmode);
	if (!m_file)
	{
		int error = errno;
		close(fd);
		errno = error;
	}
	return m_file;
}

inline bool DiskFile::Close()
{
	if (m_file)
	{
		int ret = fclose(m_file);
		m_file = nullptr;
		return ret == 0;
	}
	else
	{
		return false;
	}
}

inline int64 DiskFile::Read(void* buffer, int64 size)
{
	return fread(buffer, 1, (size_t)size, m_file);
}

inline int64 DiskFile::Write(const void* buffer, int64 size)
{
	return fwrite(buffer, 1, (size_t)size, m_file);
}

inline int64 DiskFile::Print(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int ret = vfprintf(m_file, format, ap);
	va_end(ap);
	return ret;
}

inline char* DiskFile::ReadLine(char* buffer, int64 size)
{
	return fgets(buffer, (int)size, m_file);
}

inline int64 DiskFile::Position()
{
	return ftell(m_file);
}

inline bool DiskFile::Seek(int64 position, ESeekOrigin origin)
{
	return fseek(m_file, position,
		nzbget_rs_ufs_disk_file_select_seek_origin(
			(int)origin, (int)soCur, (int)soEnd, SEEK_SET, SEEK_CUR, SEEK_END)) == 0;
}

inline bool DiskFile::Eof()
{
	return feof(m_file) != 0;
}

inline bool DiskFile::Error()
{
	return ferror(m_file) != 0;
}

inline bool DiskFile::SetWriteBuffer(int size)
{
	return setvbuf(m_file, nullptr, _IOFBF, size) == 0;
}

inline bool DiskFile::Flush()
{
	return fflush(m_file) == 0;
}

inline bool DiskFile::Sync(CString& errmsg)
{
	return FileSystem::FlushFileBuffers(fileno(m_file), errmsg);
}


#endif /* FILESYSTEM_H */

#ifndef LOG_H
#define LOG_H
#include <mutex>
#include <string>


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


#ifndef OPTIONS_H
#define OPTIONS_H
#include <atomic>
#include <cstdint>
#include <string>
#include <string_view>
#include "nzbget_rs.h"


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

class FeedInfo
{
public:
	enum EStatus
	{
		fsUndefined,
		fsRunning,
		fsFinished,
		fsFailed
	};

	enum class CategorySource
	{
		Auto,
		NZBFile,
		FeedFile
	};

	FeedInfo(
		int id,
		const char* name,
		const char* url,
		bool backlog,
		int interval,
		const char* filter,
		bool pauseNzb,
		const char* category,
		CategorySource categorySource,
		int priority,
		const char* extensions,
		unsigned int certVerifLevel
	)
		: m_id{ id }
		, m_name{ name ? name : "" }
		, m_url{ url ? url : "" }
		, m_filter{ filter ? filter : "" }
		, m_category{ category ? category : "" }
		, m_categorySource{ categorySource }
		, m_extensions{ extensions ? extensions : "" }
		, m_backlog{ backlog }
		, m_interval{ interval }
		, m_pauseNzb{ pauseNzb }
		, m_priority{ priority }
		, m_certVerifLevel{ certVerifLevel }
	{
		NzbgetRsBuf nameBuf{};
		nzbget_rs_feed_info_init(m_id, m_name.c_str(), m_filter.c_str(), &nameBuf, &m_filterHash);
		if (nameBuf.data)
		{
			m_name.assign(nameBuf.data, nameBuf.len);
		}
		nzbget_rs_free(nameBuf);
	}
	int GetId() const { return m_id; }
	const char* GetName() const { return m_name.c_str(); }
	const char* GetUrl() const { return m_url.c_str(); }
	int GetInterval() const { return m_interval; }
	const char* GetFilter() const { return m_filter.c_str(); }
	uint32 GetFilterHash() { return m_filterHash; }
	bool GetPauseNzb() const { return m_pauseNzb; }
	const char* GetCategory() const { return m_category.c_str(); }
	int GetPriority() const { return m_priority; }
	const char* GetExtensions() const { return m_extensions.c_str(); }
	unsigned int GetCertVerificationLevel() const { return m_certVerifLevel; }
	time_t GetLastUpdate() { return m_lastUpdate; }
	void SetLastUpdate(time_t lastUpdate) { m_lastUpdate = lastUpdate; }
	time_t GetNextUpdate() { return m_nextUpdate; }
	void SetNextUpdate(time_t nextUpdate) { m_nextUpdate = nextUpdate; }
	int GetLastInterval() { return m_lastInterval; }
	void SetLastInterval(int lastInterval) { m_lastInterval = lastInterval; }
	bool GetPreview() { return m_preview; }
	void SetPreview(bool preview) { m_preview = preview; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }
	const char* GetOutputFilename() const { return m_outputFilename.c_str(); }
	void SetOutputFilename(const char* outputFilename) { m_outputFilename = outputFilename ? outputFilename : ""; }
	bool GetFetch() { return m_fetch; }
	void SetFetch(bool fetch) { m_fetch = fetch; }
	bool GetForce() { return m_force; }
	void SetForce(bool force) { m_force = force; }
	bool GetBacklog() { return m_backlog; }
	void SetBacklog(bool backlog) { m_backlog = backlog; }
	CategorySource GetCategorySource() const { return m_categorySource; }
	void SetCategorySource(CategorySource categorySource) { m_categorySource = categorySource; }

private:
	int m_id;
	std::string m_name;
	std::string m_url;
	std::string m_category;
	std::string m_extensions;
	std::string m_filter;
	std::string m_outputFilename;
	time_t m_lastUpdate = 0;
	time_t m_nextUpdate = 0;
	uint32 m_filterHash;
	std::atomic<EStatus> m_status{fsUndefined};
	CategorySource m_categorySource = CategorySource::NZBFile;
	int m_interval;
	int m_priority;
	int m_lastInterval = 0;
	bool m_backlog;
	bool m_pauseNzb;
	bool m_preview = false;
	bool m_fetch = false;
	bool m_force = false;
	unsigned int m_certVerifLevel;
};

typedef std::deque<std::unique_ptr<FeedInfo>> Feeds;

class FeedFilterHelper
{
public:
	virtual void CalcDupeStatus(const char* title, const char* dupeKey, char* statusBuf, int bufLen) = 0;
};

class FeedItemInfo
{
public:
	enum EStatus
	{
		isUnknown,
		isBacklog,
		isFetched,
		isNew
	};

	enum EMatchStatus
	{
		msIgnored,
		msAccepted,
		msRejected
	};

	class Attr
	{
	public:
		Attr(const char* name, const char* value)
			: m_name(name ? name : "")
			, m_value(value ? value : "") 
		{ }
		const char* GetName() const { return m_name.c_str(); }
		const char* GetValue() const { return m_value.c_str(); }
	private:
		std::string m_name;
		std::string m_value;
	};

	typedef std::deque<Attr> AttributesBase;

	class Attributes: public AttributesBase
	{
	public:
		Attr* Find(const char* name)
		{
			if (!name)
			{
				return nullptr;
			}
			for (Attr& attr : *this)
			{
				if (!strcasecmp(attr.GetName(), name))
				{
					return &attr;
				}
			}
			return nullptr;
		}
	};

	FeedItemInfo() {}
	FeedItemInfo(FeedItemInfo&&) = delete;
	void SetFeedFilterHelper(FeedFilterHelper* feedFilterHelper) { m_feedFilterHelper = feedFilterHelper; }
	const char* GetTitle() const { return m_title.c_str(); }
	void SetTitle(const char* title) { m_title = title ? title : ""; }
	const char* GetFilename() const { return m_filename.c_str(); }
	void SetFilename(const char* filename) { m_filename = filename ? filename : ""; }
	const char* GetUrl() const { return m_url.c_str(); }
	void SetUrl(const char* url) { m_url = url ? url : ""; }
	int64 GetSize() const { return m_size; }
	void SetSize(int64 size) { m_size = size; }
	const char* GetCategory() const { return m_category.c_str(); }
	void SetCategory(const char* category) { m_category = category ? category : ""; }
	int GetImdbId() { return m_imdbId; }
	void SetImdbId(int imdbId) { m_imdbId = imdbId; }
	int GetRageId() { return m_rageId; }
	void SetRageId(int rageId) { m_rageId = rageId; }
	int GetTvdbId() { return m_tvdbId; }
	void SetTvdbId(int tvdbId) { m_tvdbId = tvdbId; }
	int GetTvmazeId() { return m_tvmazeId; }
	void SetTvmazeId(int tvmazeId) { m_tvmazeId = tvmazeId; }
	const char* GetDescription() const { return m_description.c_str(); }
	void SetDescription(const char* description) { m_description = description ? description: ""; }
	const char* GetSeason() const { return m_season.c_str(); }
	void SetSeason(const char* season)
	{
		m_season = season ? season : "";
		m_seasonNum = season ? ParsePrefixedInt(season) : 0;
	}
	const char* GetEpisode() const { return m_episode.c_str(); }
	void SetEpisode(const char* episode)
	{
		m_episode = episode ? episode : "";
		m_episodeNum = episode ? ParsePrefixedInt(episode) : 0;
	}
	int GetSeasonNum()
	{
		if (m_season.empty() && !m_seasonEpisodeParsed)
		{
			ParseSeasonEpisode();
		}
		return m_seasonNum;
	}
	int GetEpisodeNum()
	{
		if (m_episode.empty() && !m_seasonEpisodeParsed)
		{
			ParseSeasonEpisode();
		}
		return m_episodeNum;
	}
	const std::string& GetAddCategory() const { return m_addCategory; }
	void SetAddCategory(const char* addCategory) { m_addCategory = addCategory ? addCategory : ""; }
	bool GetPauseNzb() const { return m_pauseNzb; }
	void SetPauseNzb(bool pauseNzb) { m_pauseNzb = pauseNzb; }
	int GetPriority() const { return m_priority; }
	void SetPriority(int priority) { m_priority = priority; }
	time_t GetTime() const { return m_time; }
	void SetTime(time_t time) { m_time = time; }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus status) { m_status = status; }
	EMatchStatus GetMatchStatus() { return m_matchStatus; }
	void SetMatchStatus(EMatchStatus matchStatus) { m_matchStatus = matchStatus; }
	int GetMatchRule() { return m_matchRule; }
	void SetMatchRule(int matchRule) { m_matchRule = matchRule; }
	const char* GetDupeKey() const { return m_dupeKey.c_str(); }
	void SetDupeKey(const char* dupeKey) { m_dupeKey = dupeKey ? dupeKey : ""; }
	void AppendDupeKey(const char* extraDupeKey)
	{
		if (!m_dupeKey.empty() && !Util::EmptyStr(extraDupeKey))
		{
			m_dupeKey += "-";
			m_dupeKey += extraDupeKey;
		}
	}
	void BuildDupeKey(const char* rageId, const char* tvdbId, const char* tvmazeId, const char* series)
	{
		bool anySeriesId = nzbget_rs_feed_has_any_series_id(
			series, rageId, tvdbId, tvmazeId, m_rageId, m_tvdbId, m_tvmazeId) != 0;
		int seasonNum = anySeriesId ? GetSeasonNum() : 0;
		int episodeNum = (anySeriesId && seasonNum != 0) ? GetEpisodeNum() : 0;
		NzbgetRsBuf buf = nzbget_rs_feed_build_dupe_key(
			rageId, tvdbId, tvmazeId, series,
			m_rageId, m_tvdbId, m_tvmazeId, m_imdbId,
			seasonNum, episodeNum, m_season.c_str(), m_episode.c_str());
		m_dupeKey.assign(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		m_dupeKey.shrink_to_fit();
	}
	int GetDupeScore() const { return m_dupeScore; }
	void SetDupeScore(int dupeScore) { m_dupeScore = dupeScore; }
	EDupeMode GetDupeMode() const { return m_dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { m_dupeMode = dupeMode; }
	const char* GetDupeStatus()
	{
		if (m_dupeStatus.empty())
		{
			BString<1024> statuses;
			m_feedFilterHelper->CalcDupeStatus(m_title.c_str(), m_dupeKey.c_str(), statuses, statuses.Capacity());
			m_dupeStatus = statuses;
		}
		return m_dupeStatus.c_str();
	}
	Attributes* GetAttributes() { return &m_attributes; }

private:
	std::string m_title;
	std::string m_filename;
	std::string m_url;
	std::string m_category;
	std::string m_description;
	std::string m_season;
	std::string m_episode;
	std::string m_addCategory;
	std::string m_dupeKey;
	std::string m_dupeStatus;
	time_t m_time = 0;
	int64 m_size = 0;
	int m_imdbId = 0;
	int m_rageId = 0;
	int m_tvdbId = 0;
	int m_tvmazeId = 0;
	int m_seasonNum = 0;
	int m_episodeNum = 0;
	bool m_seasonEpisodeParsed = false;
	bool m_pauseNzb = false;
	int m_priority = 0;
	EStatus m_status = isUnknown;
	EMatchStatus m_matchStatus = msIgnored;
	int m_matchRule = 0;
	int m_dupeScore = 0;
	EDupeMode m_dupeMode = dmScore;
	FeedFilterHelper* m_feedFilterHelper = nullptr;
	Attributes m_attributes;

	int ParsePrefixedInt(const char *value)
	{
		return nzbget_rs_feed_parse_prefixed_int(value);
	}

	void ParseSeasonEpisode()
	{
		if (!m_feedFilterHelper)
		{
			return;
		}

		m_seasonEpisodeParsed = true;

		NzbgetRsBuf seasonBuf{};
		NzbgetRsBuf episodeBuf{};
		if (nzbget_rs_feed_parse_season_episode(m_title.c_str(), &seasonBuf, &episodeBuf) != 0)
		{
			SetSeason(seasonBuf.data ? seasonBuf.data : "");
			SetEpisode(episodeBuf.data ? episodeBuf.data : "");
		}
		nzbget_rs_free(seasonBuf);
		nzbget_rs_free(episodeBuf);
	}
};

typedef std::deque<FeedItemInfo> FeedItemList;

class FeedHistoryInfo
{
public:
	enum EStatus
	{
		hsUnknown,
		hsBacklog,
		hsFetched
	};

	FeedHistoryInfo(const char* url, EStatus status, time_t lastSeen) 
		: m_url{ url ? url : "" }
		, m_status{ status }
		, m_lastSeen{ lastSeen } {}
	const char* GetUrl() const { return m_url.c_str(); }
	EStatus GetStatus() { return m_status; }
	void SetStatus(EStatus Status) { m_status = Status; }
	time_t GetLastSeen() { return m_lastSeen; }
	void SetLastSeen(time_t lastSeen) { m_lastSeen = lastSeen; }

private:
	std::string m_url;
	EStatus m_status;
	time_t m_lastSeen;
};

typedef std::deque<FeedHistoryInfo> FeedHistoryBase;

class FeedHistory : public FeedHistoryBase
{
public:
	void Remove(const char* url)
	{
		if (!url)
		{
			return;
		}
		for (iterator it = begin(); it != end(); ++it)
		{
			if (!strcmp(it->GetUrl(), url))
			{
				erase(it);
				break;
			}
		}
	}

	FeedHistoryInfo* Find(const char* url)
	{
		if (!url)
		{
			return nullptr;
		}
		for (FeedHistoryInfo& feedHistoryInfo : *this)
		{
			if (!strcmp(feedHistoryInfo.GetUrl(), url))
			{
				return &feedHistoryInfo;
			}
		}
		return nullptr;
	}
};

#endif


class Options
{
public:
 // Program options
	static constexpr std::string_view CONFIGFILE = "ConfigFile";
	static constexpr std::string_view APPBIN = "AppBin";
	static constexpr std::string_view APPDIR = "AppDir";
	static constexpr std::string_view APPVERSION = "Version";
	static constexpr std::string_view MAINDIR = "MainDir";
	static constexpr std::string_view DESTDIR = "DestDir";
	static constexpr std::string_view INTERDIR = "InterDir";
	static constexpr std::string_view TEMPDIR = "TempDir";
	static constexpr std::string_view QUEUEDIR = "QueueDir";
	static constexpr std::string_view NZBDIR = "NzbDir";
	static constexpr std::string_view WEBDIR = "WebDir";
	static constexpr std::string_view CONFIGTEMPLATE = "ConfigTemplate";
	static constexpr std::string_view SCRIPTDIR = "ScriptDir";
	static constexpr std::string_view REQUIREDDIR = "RequiredDir";
	static constexpr std::string_view LOGFILE = "LogFile";
	static constexpr std::string_view WRITELOG = "WriteLog";
	static constexpr std::string_view ROTATELOG = "RotateLog";
	static constexpr std::string_view APPENDCATEGORYDIR = "AppendCategoryDir";
	static constexpr std::string_view LOCKFILE = "LockFile";
	static constexpr std::string_view DAEMONUSERNAME = "DaemonUsername";
	static constexpr std::string_view OUTPUTMODE = "OutputMode";
	static constexpr std::string_view DUPECHECK = "DupeCheck";
	static constexpr std::string_view DUPEARTICLEFALLBACK = "DupeArticleFallback";
	static constexpr std::string_view DUPESTREAMDECOMPRESS = "DupeStreamDecompress";
	static constexpr std::string_view DUPESTREAMTIMEOUT = "DupeStreamTimeout";
	static constexpr std::string_view DUPESEARCH = "DupeSearch";
	static constexpr std::string_view DUPESEARCHDRYRUN = "DupeSearchDryRun";
	static constexpr std::string_view DUPESEARCHURL = "DupeSearchUrl";
	static constexpr std::string_view DUPESEARCHAPIKEY = "DupeSearchApiKey";
	static constexpr std::string_view DUPESEARCHDELAY = "DupeSearchDelay";
	static constexpr std::string_view DUPESEARCHMAXDONORS = "DupeSearchMaxDonors";
	static constexpr std::string_view DUPEHEALTHPERCENT = "DupeHealthPercent";
	static constexpr std::string_view DUPEHEALTHMIN = "DupeHealthMin";
	static constexpr std::string_view DUPEHEALTHMAX = "DupeHealthMax";
	static constexpr std::string_view DUPEBODYCHECKS = "DupeBodyChecks";
	static constexpr std::string_view DUPEHEALTHBUDGET = "DupeHealthBudget";
	static constexpr std::string_view DUPEMINALIVE = "DupeMinAlive";
	static constexpr std::string_view DUPEFASTDONORS = "DupeFastDonors";
	static constexpr std::string_view DOWNLOADRATE = "DownloadRate";
	static constexpr std::string_view CONTROLIP = "ControlIP";
	static constexpr std::string_view CONTROLPORT = "ControlPort";
	static constexpr std::string_view CONTROLUSERNAME = "ControlUsername";
	static constexpr std::string_view CONTROLPASSWORD = "ControlPassword";
	static constexpr std::string_view RESTRICTEDUSERNAME = "RestrictedUsername";
	static constexpr std::string_view RESTRICTEDPASSWORD = "RestrictedPassword";
	static constexpr std::string_view ADDUSERNAME = "AddUsername";
	static constexpr std::string_view ADDPASSWORD = "AddPassword";
	static constexpr std::string_view FORMAUTH = "FormAuth";
	static constexpr std::string_view JSONP = "JsonP";
	static constexpr std::string_view SECURECONTROL = "SecureControl";
	static constexpr std::string_view SECUREPORT = "SecurePort";
	static constexpr std::string_view SECURECERT = "SecureCert";
	static constexpr std::string_view SECUREKEY = "SecureKey";
	static constexpr std::string_view CERTSTORE = "CertStore";
	static constexpr std::string_view CERTCHECK = "CertCheck";
	static constexpr std::string_view AUTHORIZEDIP = "AuthorizedIP";
	static constexpr std::string_view ARTICLETIMEOUT = "ArticleTimeout";
	static constexpr std::string_view ARTICLEREADCHUNKSIZE = "ArticleReadChunkSize";
	static constexpr std::string_view URLTIMEOUT = "UrlTimeout";
	static constexpr std::string_view REMOTETIMEOUT = "RemoteTimeout";
	static constexpr std::string_view FLUSHQUEUE = "FlushQueue";
	static constexpr std::string_view SYSTEMHEALTHCHECK = "SystemHealthCheck";
	static constexpr std::string_view NZBLOG = "NzbLog";
	static constexpr std::string_view RAWARTICLE = "RawArticle";
	static constexpr std::string_view SKIPWRITE = "SkipWrite";
	static constexpr std::string_view ARTICLERETRIES = "ArticleRetries";
	static constexpr std::string_view ARTICLEINTERVAL = "ArticleInterval";
	static constexpr std::string_view URLRETRIES = "UrlRetries";
	static constexpr std::string_view URLINTERVAL = "UrlInterval";
	static constexpr std::string_view CONTINUEPARTIAL = "ContinuePartial";
	static constexpr std::string_view URLCONNECTIONS = "UrlConnections";
	static constexpr std::string_view LOGBUFFER = "LogBuffer";
	static constexpr std::string_view INFOTARGET = "InfoTarget";
	static constexpr std::string_view WARNINGTARGET = "WarningTarget";
	static constexpr std::string_view ERRORTARGET = "ErrorTarget";
	static constexpr std::string_view DEBUGTARGET = "DebugTarget";
	static constexpr std::string_view DETAILTARGET = "DetailTarget";
	static constexpr std::string_view PARCHECK = "ParCheck";
	static constexpr std::string_view PARREPAIR = "ParRepair";
	static constexpr std::string_view PARSCAN = "ParScan";
	static constexpr std::string_view PARQUICK = "ParQuick";
	static constexpr std::string_view POSTSTRATEGY = "PostStrategy";
	static constexpr std::string_view FILENAMING = "FileNaming";
	static constexpr std::string_view POSTDOWNLOADRENAME = "PostDownloadRename";
	static constexpr std::string_view RENAMEAFTERUNPACK = "RenameAfterUnpack";
	static constexpr std::string_view RENAMEIGNOREEXT = "RenameIgnoreExt";
	static constexpr std::string_view PARRENAME = "ParRename";
	static constexpr std::string_view PARBUFFER = "ParBuffer";
	static constexpr std::string_view PARTHREADS = "ParThreads";
	static constexpr std::string_view RARRENAME = "RarRename";
	static constexpr std::string_view HEALTHCHECK = "HealthCheck";
	static constexpr std::string_view DIRECTRENAME = "DirectRename";
	static constexpr std::string_view HARDLINKING = "HardLinking";
	static constexpr std::string_view HARDLINKINGIGNOREEXT = "HardLinkingIgnoreExt";
	static constexpr std::string_view UMASK = "UMask";
	static constexpr std::string_view UPDATEINTERVAL = "UpdateInterval";
	static constexpr std::string_view CURSESNZBNAME = "CursesNzbName";
	static constexpr std::string_view CURSESTIME = "CursesTime";
	static constexpr std::string_view CURSESGROUP = "CursesGroup";
	static constexpr std::string_view CRCCHECK = "CrcCheck";
	static constexpr std::string_view DIRECTWRITE = "DirectWrite";
	static constexpr std::string_view WRITEBUFFER = "WriteBuffer";
	static constexpr std::string_view NZBDIRINTERVAL = "NzbDirInterval";
	static constexpr std::string_view NZBDIRFILEAGE = "NzbDirFileAge";
	static constexpr std::string_view NZBDIRARCHIVESCAN = "NzbDirArchiveScan";
	static constexpr std::string_view NZBDIRARCHIVEACTION = "NzbDirArchiveAction";
	static constexpr std::string_view DISKSPACE = "DiskSpace";
	static constexpr std::string_view CRASHTRACE = "CrashTrace";
	static constexpr std::string_view CRASHDUMP = "CrashDump";
	static constexpr std::string_view PARPAUSEQUEUE = "ParPauseQueue";
	static constexpr std::string_view SCRIPTPAUSEQUEUE = "ScriptPauseQueue";
	static constexpr std::string_view NZBCLEANUPDISK = "NzbCleanupDisk";
	static constexpr std::string_view PARTIMELIMIT = "ParTimeLimit";
	static constexpr std::string_view KEEPHISTORY = "KeepHistory";
	static constexpr std::string_view UNPACK = "Unpack";
	static constexpr std::string_view DIRECTUNPACK = "DirectUnpack";
	static constexpr std::string_view USETEMPUNPACKDIR = "UseTempUnpackDir";
	static constexpr std::string_view UNPACKCLEANUPDISK = "UnpackCleanupDisk";
	static constexpr std::string_view UNRARCMD = "UnrarCmd";
	static constexpr std::string_view SEVENZIPCMD = "SevenZipCmd";
	static constexpr std::string_view UNPACKPASSFILE = "UnpackPassFile";
	static constexpr std::string_view UNPACKPAUSEQUEUE = "UnpackPauseQueue";
	static constexpr std::string_view SCRIPTORDER = "ScriptOrder";
	static constexpr std::string_view EXTENSIONS = "Extensions";
	static constexpr std::string_view EXTCLEANUPDISK = "ExtCleanupDisk";
	static constexpr std::string_view PARIGNOREEXT = "ParIgnoreExt";
	static constexpr std::string_view UNPACKIGNOREEXT = "UnpackIgnoreExt";
	static constexpr std::string_view FEEDHISTORY = "FeedHistory";
	static constexpr std::string_view URLFORCE = "UrlForce";
	static constexpr std::string_view TIMECORRECTION = "TimeCorrection";
	static constexpr std::string_view PROPAGATIONDELAY = "PropagationDelay";
	static constexpr std::string_view ARTICLECACHE = "ArticleCache";
	static constexpr std::string_view EVENTINTERVAL = "EventInterval";
	static constexpr std::string_view SHELLOVERRIDE = "ShellOverride";
	static constexpr std::string_view MONTHLYQUOTA = "MonthlyQuota";
	static constexpr std::string_view QUOTASTARTDAY = "QuotaStartDay";
	static constexpr std::string_view DAILYQUOTA = "DailyQuota";
	static constexpr std::string_view REORDERFILES = "ReorderFiles";
	static constexpr std::string_view UPDATECHECK = "UpdateCheck";
	static constexpr std::string_view MEMORYPRESSUREINTERVAL = "MemoryPressureInterval";
	static constexpr std::string_view MEMORYPRESSUREPATH = "MemoryPressurePath";

// obsolete options
	static constexpr std::string_view POSTLOGKIND = "PostLogKind";
	static constexpr std::string_view NZBLOGKIND = "NZBLogKind";
	static constexpr std::string_view RETRYONCRCERROR = "RetryOnCrcError";
	static constexpr std::string_view ALLOWREPROCESS = "AllowReProcess";
	static constexpr std::string_view POSTPROCESS = "PostProcess";
	static constexpr std::string_view LOADPARS = "LoadPars";
	static constexpr std::string_view THREADLIMIT = "ThreadLimit";
	static constexpr std::string_view PROCESSLOGKIND = "ProcessLogKind";
	static constexpr std::string_view APPENDNZBDIR = "AppendNzbDir";
	static constexpr std::string_view RENAMEBROKEN = "RenameBroken";
	static constexpr std::string_view MERGENZB = "MergeNzb";
	static constexpr std::string_view STRICTPARNAME = "StrictParName";
	static constexpr std::string_view RELOADURLQUEUE = "ReloadUrlQueue";
	static constexpr std::string_view RELOADPOSTQUEUE = "ReloadPostQueue";
	static constexpr std::string_view NZBPROCESS = "NZBProcess";
	static constexpr std::string_view NZBADDEDPROCESS = "NZBAddedProcess";
	static constexpr std::string_view CREATELOG = "CreateLog";
	static constexpr std::string_view RESETLOG = "ResetLog";
	static constexpr std::string_view PARCLEANUPQUEUE = "ParCleanupQueue";
	static constexpr std::string_view DELETECLEANUPDISK = "DeleteCleanupDisk";
	static constexpr std::string_view HISTORYCLEANUPDISK = "HistoryCleanupDisk";
	static constexpr std::string_view SCANSCRIPT = "ScanScript";
	static constexpr std::string_view QUEUESCRIPT = "QueueScript";
	static constexpr std::string_view FEEDSCRIPT = "FeedScript";
	static constexpr std::string_view DECODE = "Decode";
	static constexpr std::string_view SAVEQUEUE = "SaveQueue";
	static constexpr std::string_view RELOADQUEUE = "ReloadQueue";
	static constexpr std::string_view TERMINATETIMEOUT = "TerminateTimeout";
	static constexpr std::string_view ACCURATERATE = "AccurateRate";
	static constexpr std::string_view CREATEBROKENLOG = "CreateBrokenLog";
	static constexpr std::string_view BROKENLOG = "BrokenLog";

	enum EWriteLog
	{
		wlNone,
		wlAppend,
		wlReset,
		wlRotate
	};
	enum EMessageTarget
	{
		mtNone,
		mtScreen,
		mtLog,
		mtBoth
	};
	enum EOutputMode
	{
		omLoggable,
		omColored,
		omNCurses
	};
	enum EParCheck
	{
		pcAuto,
		pcAlways,
		pcForce,
		pcManual
	};
	enum EParScan
	{
		psLimited,
		psExtended,
		psFull,
		psDupe
	};
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
	enum EHealthCheck
	{
		hcPause,
		hcDelete,
		hcPark,
		hcNone,
		hcDupe
	};
	enum ESchedulerCommand
	{
		scPauseDownload,
		scUnpauseDownload,
		scPausePostProcess,
		scUnpausePostProcess,
		scDownloadRate,
		scScript,
		scProcess,
		scPauseScan,
		scUnpauseScan,
		scActivateServer,
		scDeactivateServer,
		scFetchFeed
	};
	enum EPostStrategy
	{
		ppSequential,
		ppBalanced,
		ppAggressive,
		ppRocket
	};
	enum EFileNaming
	{
		nfAuto,
		nfArticle,
		nfNzb
	};
	enum ECertVerifLevel
	{
		cvNone,
		cvMinimal,
		cvStrict,
		Count
	};
	
	enum ENzbDirArchiveAction
	{
		Delete,
		Move
	};

	class OptEntry
	{
	public:
		OptEntry(const char* name, const char* value) :
			m_name(name), m_value(value) {}
		void SetName(const char* name) { m_name = name; }
		const char* GetName() const { return m_name; }
		void SetValue(const char* value);
		const char* GetValue() const { return m_value; }
		const char* GetDefValue() const { return m_defValue; }
		int GetLineNo() const { return m_lineNo; }
		bool Restricted();

	private:
		CString m_name;
		CString m_value;
		CString m_defValue;
		int m_lineNo = 0;

		void SetLineNo(int lineNo) { m_lineNo = lineNo; }

		friend class Options;
	};

	typedef std::deque<OptEntry> OptEntriesBase;

	class OptEntries: public OptEntriesBase
	{
	public:
		OptEntry* FindOption(const char* name);
	};

	typedef GuardedPtr<OptEntries> GuardedOptEntries;

	typedef std::vector<CString> NameList;
	typedef std::vector<const char*> CmdOptList;

	class Category
	{
	public:
		Category(const char* name, const char* destDir, bool unpack, const char* extensions)
			: m_name(name ? name : ""),
			  m_destDir(destDir ? destDir : ""),
			  m_extensions(extensions ? extensions : ""),
			  m_destDirPath(m_destDir),
			  m_unpack(unpack)
		{
		}
		const char* GetName() const { return m_name.c_str(); }
		const char* GetDestDir() const { return m_destDir.c_str(); }
		const fs::path& GetDestDirPath() const { return m_destDirPath; }
		bool GetUnpack() const { return m_unpack; }
		const char* GetExtensions() const { return m_extensions.c_str(); }
		NameList* GetAliases() { return &m_aliases; }
		const NameList* GetAliases() const { return &m_aliases; }

	private:
		const std::string m_name;
		const std::string m_destDir;
		const std::string m_extensions;
		const fs::path m_destDirPath;
		NameList m_aliases;
		const bool m_unpack;
	};

	typedef std::deque<Category> CategoriesBase;

	class Categories: public CategoriesBase
	{
	public:
		Category* FindCategory(const char* name, bool searchAliases);
		const Category* FindCategory(const char* name, bool searchAliases) const;
	};

	class Extender
	{
	public:
		virtual void AddNewsServer(int id, bool active, const char* name, const char* host,
			int port, int ipVersion, const char* user, const char* pass, bool joinGroup,
			bool tls, const char* cipher, int maxConnections, int retention,
			int level, int group, bool optional, unsigned int certVerificationfLevel) = 0;
		virtual ~Extender();
		virtual void AddFeed(
			int id,
			const char* name,
			const char* url,
			int interval,
			const char* filter,
			bool backlog,
			bool pauseNzb,
			const char* category,
			FeedInfo::CategorySource categorySource,
			int priority,
			const char* extensions,
			unsigned int certVeriflevel
		) = 0;
		virtual void AddTask(
			int id,
			int hours,
			int minutes,
			int weekDaysBits,
			ESchedulerCommand command,
			const char* param
		) = 0;
		virtual void SetupFirstStart() {}
	};

	Options(const char* exeName, const char* configFilename, bool noConfig,
		CmdOptList* commandLineOptions, Extender* extender);
	Options(CmdOptList* commandLineOptions, Extender* extender);
	~Options();

	static bool SplitOptionString(const char* option, CString& optName, CString& optValue);
	static void ConvertOldOptions(OptEntries* optEntries);
	bool GetFatalError() { return m_fatalError; }
	GuardedOptEntries GuardOptEntries() { return GuardedOptEntries(&m_optEntries, &m_optEntriesMutex); }
	void CreateSchedulerTask(int id, const char* time, const char* weekDays,
		ESchedulerCommand command, const char* param);

	// Options
	const char* GetConfigFilename() const { return m_configFilename; }
	bool GetConfigErrors() const { return m_configErrors; }
	const char* GetMainDir() const { return m_mainDir; }
	const char* GetAppDir() const { return m_appDir; }
	const char* GetDestDir() const { return m_destDir; }
	const char* GetInterDir() const { return m_interDir; }
	const char* GetTempDir() const { return m_tempDir; }
	const char* GetQueueDir() const { return m_queueDir; }
	const char* GetNzbDir() const { return m_nzbDir; }
	const char* GetWebDir() const { return m_webDir; }
	const char* GetConfigTemplate() const { return m_configTemplate; }
	const char* GetScriptDir() const { return m_scriptDir; }
	const char* GetRequiredDir() const { return m_requiredDir; }
	bool GetSystemHealthCheck() const { return m_systemHealthCheck; }
	bool GetNzbLog() const { return m_nzbLog; }
	EMessageTarget GetInfoTarget() const { return m_infoTarget; }
	EMessageTarget GetWarningTarget() const { return m_warningTarget; }
	EMessageTarget GetErrorTarget() const { return m_errorTarget; }
	EMessageTarget GetDebugTarget() const { return m_debugTarget; }
	EMessageTarget GetDetailTarget() const { return m_detailTarget; }
	int GetArticleTimeout() const { return m_articleTimeout; }
	int GetArticleReadChunkSize() const { return m_articleReadChunkSize; }
	int GetUrlTimeout() const { return m_urlTimeout; }
	int GetRemoteTimeout() const { return m_remoteTimeout; }
	bool GetRawArticle() const { return m_rawArticle; }
	bool GetSkipWrite() const { return m_skipWrite; }
	bool GetAppendCategoryDir() const { return m_appendCategoryDir; }
	bool GetContinuePartial() const { return m_continuePartial; }
	int GetArticleRetries() const { return m_articleRetries; }
	int GetArticleInterval() const { return m_articleInterval; }
	int GetUrlRetries() const { return m_urlRetries; }
	int GetUrlInterval() const { return m_urlInterval; }
	bool GetFlushQueue() const { return m_flushQueue; }
	bool GetDupeCheck() const { return m_dupeCheck; }
	EDupeArticleFallback GetDupeArticleFallback() const { return m_dupeArticleFallback; }
	bool GetDupeStreamDecompress() const { return m_dupeStreamDecompress; }
	int GetDupeStreamTimeout() const { return m_dupeStreamTimeout; }
	bool GetDupeSearch() const { return m_dupeSearch; }
	bool GetDupeSearchDryRun() const { return m_dupeSearchDryRun; }
	const char* GetDupeSearchUrl() const { return m_dupeSearchUrl; }
	const char* GetDupeSearchApiKey() const { return m_dupeSearchApiKey; }
	int GetDupeSearchDelay() const { return m_dupeSearchDelay; }
	int GetDupeSearchMaxDonors() const { return m_dupeSearchMaxDonors; }
	int GetDupeHealthPercent() const { return m_dupeHealthPercent; }
	int GetDupeHealthMin() const { return m_dupeHealthMin; }
	int GetDupeHealthMax() const { return m_dupeHealthMax; }
	int GetDupeBodyChecks() const { return m_dupeBodyChecks; }
	int GetDupeHealthBudget() const { return m_dupeHealthBudget; }
	int GetDupeMinAlive() const { return m_dupeMinAlive; }
	int GetDupeFastDonors() const { return m_dupeFastDonors; }
	const char* GetControlIp() const { return m_controlIp; }
	const char* GetControlUsername() const { return m_controlUsername; }
	const char* GetControlPassword() const { return m_controlPassword; }
	const char* GetRestrictedUsername() const { return m_restrictedUsername; }
	const char* GetRestrictedPassword() const { return m_restrictedPassword; }
	const char* GetAddUsername() const { return m_addUsername; }
	const char* GetAddPassword() const { return m_addPassword; }
	int GetControlPort() const { return m_controlPort; }
	bool GetFormAuth() const { return m_formAuth; }
	bool GetJsonP() const { return m_jsonP; }
	bool GetSecureControl() const { return m_secureControl; }
	int GetSecurePort() const { return m_securePort; }
	const char* GetSecureCert() const { return m_secureCert; }
	const char* GetSecureKey() const { return m_secureKey; }
	const char* GetCertStore() const { return m_certStore; }
	bool GetCertCheck() const { return m_certCheck; }
	const char* GetAuthorizedIp() const { return m_authorizedIp; }
	const char* GetLockFile() const { return m_lockFile; }
	const char* GetDaemonUsername() const { return m_daemonUsername; }
	EOutputMode GetOutputMode() const { return m_outputMode; }
	int GetUrlConnections() const { return m_urlConnections; }
	int GetLogBuffer() const { return m_logBuffer; }
	EWriteLog GetWriteLog() const { return m_writeLog; }
	const char* GetLogFile() const { return m_logFile; }
	int GetRotateLog() const { return m_rotateLog; }
	EParCheck GetParCheck() const { return m_parCheck; }
	bool GetParRepair() const { return m_parRepair; }
	EParScan GetParScan() const { return m_parScan; }
	bool GetParQuick() const { return m_parQuick; }
	EPostStrategy GetPostStrategy() const { return m_postStrategy; }
	bool GetParRename() const { return m_parRename; }
	int GetParBuffer() const { return m_parBuffer; }
	int GetParThreads() const { return m_parThreads; }
	bool GetRarRename() const { return m_rarRename; }
	EHealthCheck GetHealthCheck() const { return m_healthCheck; }
	const char* GetScriptOrder() const { return m_scriptOrder; }
	const char* GetExtensions() const { return m_extensions; }
	int GetUMask() const { return m_umask; }
	int GetUpdateInterval() const { return m_updateInterval; }
	bool GetCursesNzbName() const { return m_cursesNzbName; }
	bool GetCursesTime() const { return m_cursesTime; }
	bool GetCursesGroup() const { return m_cursesGroup; }
	bool GetCrcCheck() const { return m_crcCheck; }
	bool GetDirectWrite() const { return m_directWrite; }
	int GetWriteBuffer() const { return m_writeBuffer; }
	int GetNzbDirInterval() const { return m_nzbDirInterval; }
	int GetNzbDirFileAge() const { return m_nzbDirFileAge; }
	bool GetNzbDirArchiveScan() const { return m_nzbDirArchiveScan; }
	ENzbDirArchiveAction GetNzbDirArchiveAction() const { return m_nzbDirArchiveAction; }
	int GetDiskSpace() const { return m_diskSpace; }
	bool GetTls() const { return m_tls; }
	bool GetCrashTrace() const { return m_crashTrace; }
	bool GetCrashDump() const { return m_crashDump; }
	bool GetParPauseQueue() const { return m_parPauseQueue; }
	bool GetScriptPauseQueue() const { return m_scriptPauseQueue; }
	bool GetNzbCleanupDisk() const { return m_nzbCleanupDisk; }
	int GetParTimeLimit() const { return m_parTimeLimit; }
	int GetKeepHistory() const { return m_keepHistory; }
	bool GetUnpack() const { return m_unpack; }
	bool GetDirectUnpack() const { return m_directUnpack; }
	bool GetUseTempUnpackDir() const { return m_useTempUnpackDir; }
	bool GetUnpackCleanupDisk() const { return m_unpackCleanupDisk; }
	const char* GetUnrarCmd() const { return m_unrarCmd; }
	const char* GetSevenZipCmd() const { return m_sevenZipCmd; }
	const char* GetUnpackPassFile() const { return m_unpackPassFile; }
	bool GetUnpackPauseQueue() const { return m_unpackPauseQueue; }
	bool GetUnpackMutexEnforced() const { return m_unpackMutexEnforced; }
	void SetUnpackMutexEnforced(bool unpackMutexEnforced) { m_unpackMutexEnforced = unpackMutexEnforced; }
	const char* GetExtCleanupDisk() const { return m_extCleanupDisk; }
	const char* GetParIgnoreExt() const { return m_parIgnoreExt; }
	bool GetPostDownloadRename() const { return m_postDownloadRename; }
	bool GetRenameAfterUnpack() const { return m_renameAfterUnpack; }
	const char* GetRenameIgnoreExt() const { return m_renameIgnoreExt; }
	const char* GetUnpackIgnoreExt() const { return m_unpackIgnoreExt; }
	int GetFeedHistory() const { return m_feedHistory; }
	bool GetUrlForce() const { return m_urlForce; }
	int GetTimeCorrection() const { return m_timeCorrection; }
	int GetPropagationDelay() const { return m_propagationDelay; }
	int GetArticleCache() const { return m_articleCache; }
	int GetEventInterval() const { return m_eventInterval; }
	int GetMemoryPressureInterval() const { return m_memoryPressureInterval; }
	const char* GetMemoryPressurePath() const { return m_memoryPressurePath; }
	const std::string& GetShellOverride() const { return m_shellOverride; }
	int GetMonthlyQuota() const { return m_monthlyQuota; }
	int GetQuotaStartDay() const { return m_quotaStartDay; }
	int GetDailyQuota() const { return m_dailyQuota; }
	bool GetDirectRename() const { return m_directRename; }
	bool GetHardLinking() const { return m_hardLinking; }
	const char* GetHardLinkingIgnoreExt() const { return m_hardLinkingIgnoreExt; }
	bool GetReorderFiles() const { return m_reorderFiles; }
	EFileNaming GetFileNaming() const { return m_fileNaming; }
	int GetDownloadRate() const { return m_downloadRate; }

	Categories* GetCategories() { return &m_categories; }
	const Categories* GetCategories() const { return &m_categories; }
	Category* FindCategory(const char* name, bool searchAliases) { return m_categories.FindCategory(name, searchAliases); }
	const char* GetOption(const char* optname);

	// Current state
	void SetServerMode(bool serverMode) { m_serverMode = serverMode; }
	bool GetServerMode() const { return m_serverMode; }
	void SetDaemonMode(bool daemonMode) { m_daemonMode = daemonMode; }
	bool GetDaemonMode() const { return m_daemonMode; }
	void SetRemoteClientMode(bool remoteClientMode) { m_remoteClientMode = remoteClientMode; }
	bool GetRemoteClientMode() const { return m_remoteClientMode; }
	void SetDirectClientMode(bool directClientMode) { m_directClientMode = directClientMode; }
	bool GetDirectClientMode() const { return m_directClientMode; }

	const fs::path& GetAppDirPath() const { return m_appDirPath; }
	const fs::path& GetConfigFilePath() const { return m_configFilePath; }
	const fs::path& GetMainDirPath() const { return m_mainDirPath; }
	const fs::path& GetDestDirPath() const { return m_destDirPath; }
	const fs::path& GetInterDirPath() const { return m_interDirPath; }
	const fs::path& GetTempDirPath() const { return m_tempDirPath; }
	const fs::path& GetQueueDirPath() const { return m_queueDirPath; }
	const fs::path& GetNzbDirPath() const { return m_nzbDirPath; }
	const fs::path& GetWebDirPath() const { return m_webDirPath; }
	const fs::path& GetConfigTemplatePath() const { return m_configTemplatePath; }
	const fs::path& GetSecureCertPath() const { return m_secureCertPath; }
	const fs::path& GetSecureKeyPath() const { return m_secureKeyPath; }
	const fs::path& GetCertStorePath() const { return m_certStorePath; }
	const fs::path& GetLockFilePath() const { return m_lockFilePath; }
	const fs::path& GetLogFilePath() const { return m_logFilePath; }
	const fs::path& GetUnpackPassFilePath() const { return m_unpackPassFilePath; }
	const fs::path& GetUnrarPath() const { return m_unrarPath; }
	const fs::path& GetSevenZipPath() const { return m_sevenZipPath; }
	const std::vector<fs::path>& GetScriptDirPaths() const { return m_scriptDirPaths; }

private:
	void CheckDirs();
	void CheckScriptDir();

	OptEntries m_optEntries;
	Mutex m_optEntriesMutex;
	Categories m_categories;
	bool m_noDiskAccess = false;
	bool m_noConfig = false;
	bool m_fatalError = false;
	Extender* m_extender;

	// Options
	fs::path m_appDirPath;
	fs::path m_configFilePath;
	fs::path m_mainDirPath;
	fs::path m_destDirPath;
	fs::path m_interDirPath;
	fs::path m_tempDirPath;
	fs::path m_queueDirPath;
	fs::path m_nzbDirPath;
	fs::path m_webDirPath;
	fs::path m_configTemplatePath;
	fs::path m_secureCertPath;
	fs::path m_secureKeyPath;
	fs::path m_certStorePath;
	fs::path m_lockFilePath;
	fs::path m_logFilePath;
	fs::path m_unpackPassFilePath;
	fs::path m_unrarPath;
	fs::path m_sevenZipPath;
	std::vector<fs::path> m_scriptDirPaths;

	bool m_configErrors = false;
	int m_configLine = 0;
	CString m_appDir;
	CString m_configFilename;
	CString m_mainDir;
	CString m_destDir;
	CString m_interDir;
	CString m_tempDir;
	CString m_queueDir;
	CString m_nzbDir;
	CString m_webDir;
	CString m_configTemplate;
	CString m_scriptDir;
	CString m_requiredDir;
	EMessageTarget m_infoTarget = mtScreen;
	EMessageTarget m_warningTarget = mtScreen;
	EMessageTarget m_errorTarget = mtScreen;
	EMessageTarget m_debugTarget = mtNone;
	EMessageTarget m_detailTarget = mtScreen;
	bool m_skipWrite = false;
	bool m_rawArticle = false;
	bool m_nzbLog = false;
	bool m_systemHealthCheck = true;
	int m_articleTimeout = 0;
	int m_articleReadChunkSize = 4;
	int m_urlTimeout = 0;
	int m_remoteTimeout = 0;
	bool m_appendCategoryDir = false;
	bool m_continuePartial = false;
	int m_articleRetries = 0;
	int m_articleInterval = 0;
	int m_urlRetries = 0;
	int m_urlInterval = 0;
	bool m_flushQueue = false;
	bool m_dupeCheck = false;
	EDupeArticleFallback m_dupeArticleFallback = dafNone;
	bool m_dupeStreamDecompress = false;
	int m_dupeStreamTimeout = 60;
	bool m_dupeSearch = false;
	bool m_dupeSearchDryRun = false;
	CString m_dupeSearchUrl;
	CString m_dupeSearchApiKey;
	int m_dupeSearchDelay = 20;
	int m_dupeSearchMaxDonors = 0;
	int m_dupeHealthPercent = 5;
	int m_dupeHealthMin = 50;
	int m_dupeHealthMax = 1000;
	int m_dupeBodyChecks = 20;
	int m_dupeHealthBudget = 120;
	int m_dupeMinAlive = 50;
	int m_dupeFastDonors = 5;
	CString m_controlIp;
	CString m_controlUsername;
	CString m_controlPassword;
	CString m_restrictedUsername;
	CString m_restrictedPassword;
	CString m_addUsername;
	CString m_addPassword;
	bool m_formAuth = false;
	bool m_jsonP = false;
	int m_controlPort = 0;
	bool m_secureControl = false;
	int m_securePort = 0;
	CString m_secureCert;
	CString m_secureKey;
	CString m_certStore;
	bool m_certCheck = false;
	CString m_authorizedIp;
	CString m_lockFile;
	CString m_daemonUsername;
	EOutputMode m_outputMode = omLoggable;
	int m_urlConnections = 0;
	int m_logBuffer = 0;
	EWriteLog m_writeLog = wlAppend;
	int m_rotateLog = 0;
	CString m_logFile;
	EParCheck m_parCheck = pcManual;
	bool m_parRepair = false;
	EParScan m_parScan = psLimited;
	bool m_parQuick = true;
	EPostStrategy m_postStrategy = ppSequential;
	bool m_parRename = false;
	int m_parBuffer = 0;
	int m_parThreads = 0;
	bool m_rarRename = false;
	bool m_directRename = false;
	bool m_hardLinking = false;
	CString m_hardLinkingIgnoreExt;
	EHealthCheck m_healthCheck = hcNone;
	CString m_extensions;
	CString m_scriptOrder;
	int m_umask = 0;
	int m_updateInterval = 0;
	bool m_cursesNzbName = false;
	bool m_cursesTime = false;
	bool m_cursesGroup = false;
	bool m_crcCheck = false;
	bool m_directWrite = false;
	int m_writeBuffer = 0;
	int m_nzbDirInterval = 0;
	int m_nzbDirFileAge = 0;
	bool m_nzbDirArchiveScan = true;
	ENzbDirArchiveAction m_nzbDirArchiveAction = ENzbDirArchiveAction::Move;
	int m_diskSpace = 0;
	bool m_tls = false;
	bool m_crashTrace = false;
	bool m_crashDump = false;
	bool m_parPauseQueue = false;
	bool m_scriptPauseQueue = false;
	bool m_nzbCleanupDisk = false;
	int m_parTimeLimit = 0;
	int m_keepHistory = 0;
	bool m_unpack = false;
	bool m_directUnpack = false;
	bool m_useTempUnpackDir = true;
	bool m_unpackCleanupDisk = false;
	CString m_unrarCmd;
	CString m_sevenZipCmd;
	CString m_unpackPassFile;
	bool m_unpackPauseQueue;
	bool m_unpackMutexEnforced = false;
	CString m_extCleanupDisk;
	CString m_parIgnoreExt;
	CString m_unpackIgnoreExt;
	int m_feedHistory = 0;
	bool m_urlForce = false;
	int m_timeCorrection = 0;
	int m_propagationDelay = 0;
	int m_articleCache = 0;
	int m_eventInterval = 0;
	int m_memoryPressureInterval = 500;
	CString m_memoryPressurePath;
	std::string m_shellOverride;
	int m_monthlyQuota = 0;
	int m_quotaStartDay = 0;
	int m_dailyQuota = 0;
	bool m_reorderFiles = false;
	EFileNaming m_fileNaming = nfArticle;
	bool m_postDownloadRename = true;
	bool m_renameAfterUnpack = true;
	CString m_renameIgnoreExt;
	int m_downloadRate = 0;

	// Application mode
	bool m_serverMode = false;
	bool m_daemonMode = false;
	bool m_remoteClientMode = false;
	bool m_directClientMode = false;

	void Init(const char* exeName, const char* configFilename, bool noConfig,
		CmdOptList* commandLineOptions, bool noDiskAccess, Extender* extender);
	void InitDefaults();
	void InitOptions();
	void InitOptFile();
	void InitServers();
	void InitCategories();
	void InitScheduler();
	void InitFeeds();
	void InitCommandLineOptions(CmdOptList* commandLineOptions);
	void CheckOptions();
	NzbgetRsOptionsHost MakeHost();
	NzbgetRsParsedOptions ExportParsedScalars() const;
	void ApplyParsedScalars(const NzbgetRsParsedOptions& s);
	OptEntry* FindOption(const char* optname);
	void SetOption(const char* optname, const char* value);
	void SetToolPathOption(fs::path& pathOpt, std::string_view value);
	bool SetOptionString(const char* option);
	bool ValidateOptionName(const char* optname, const char* optvalue);
	void LoadConfigFile();
	void CheckDir(CString& dir, const char* optionName, const char* parentDir,
		bool allowEmpty, bool create);
	void ConfigError(const char* msg, ...);
	void ConfigWarn(const char* msg, ...);
};

extern Options* g_Options;

class MemoryMonitor final : public Thread
{
public:
	MemoryMonitor();
	~MemoryMonitor() override;

	void InitOptions();
	void Run() override;
	void Stop() override;

	NzbgetRsPressureTier GetTier() const { return (NzbgetRsPressureTier)m_tier.load(std::memory_order_acquire); }
	const char* GetTierName() const
	{
		return nzbget_rs_awc_memory_monitor_tier_name(
			(int)GetTier(), (int)NZBGET_RS_PRESSURE_CONSTRAINED, (int)NZBGET_RS_PRESSURE_CRITICAL);
	}

	size_t GetEffectiveArticleCacheBytes() const
	{
		return nzbget_rs_tpr_article_cache_mb_to_bytes(
			(int)m_effectiveCacheMB.load(std::memory_order_acquire));
	}
	int GetEffectiveArticleCacheMB() const
	{
		return (int)m_effectiveCacheMB.load(std::memory_order_acquire);
	}
	int GetEffectiveParBufferMB(int configuredMB) const
	{
		int tier = m_tier.load(std::memory_order_acquire);
		int mb = 0;
		if (nzbget_rs_awc_memory_monitor_calc_critical_or_normal_par_buffer(
			tier, configuredMB, (int)NZBGET_RS_PRESSURE_CRITICAL, (int)NZBGET_RS_PRESSURE_NORMAL, &mb) != 0)
		{
			return mb;
		}
		return (int)nzbget_rs_memory_pressure_effective_par_buffer((uint32_t)configuredMB);
	}
	int GetEffectiveDownloadsLimit(int configuredLimit) const
	{
		int tier = m_tier.load(std::memory_order_acquire);
		return nzbget_rs_awc_memory_monitor_effective_downloads_limit(
			tier, configuredLimit, (int)NZBGET_RS_PRESSURE_CONSTRAINED, (int)NZBGET_RS_PRESSURE_CRITICAL);
	}

	static void TrimHeap()
	{
		nzbget_rs_memory_pressure_trim_heap();
	}

private:
	std::atomic<int> m_tier{NZBGET_RS_PRESSURE_NORMAL};
	std::atomic<uint32_t> m_effectiveCacheMB{0};
	std::atomic<uint32_t> m_effectiveParBufferMB{0};
	std::atomic<uint32_t> m_effectiveDownloadsLimit{0};

	int m_consecutiveSamples = 0;
	int m_candidateTier = NZBGET_RS_PRESSURE_NORMAL;

	void UpdateState();
	void ApplyTierTransition(NzbgetRsPressureTier oldTier, NzbgetRsPressureTier newTier);
};

inline MemoryMonitor* g_MemoryMonitor = nullptr;


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

			if (nzbget_rs_log_should_delete_rotated_file_by_name(
				filename,
				(size_t)mask.GetMatchStart(0),
				(size_t)mask.GetMatchStart(1),
				(size_t)mask.GetMatchStart(2),
				static_cast<int64_t>(curTime),
				g_Options->GetRotateLog()))
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

	NzbgetRsBuf rotBuf = nzbget_rs_log_format_rotated_path_for_time(
		*directory, PATH_SEPARATOR, baseName, static_cast<int64_t>(curTime), *baseExt);
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

#ifndef SERVICE_H
#define SERVICE_H

#include "nzbget_rs.h"

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

private:
	time_t m_lastWork = 0;

	friend class ServiceCoordinator;
};

class ServiceCoordinator : public Thread
{
public:
	using ServiceList = std::vector<Service*>;

	ServiceCoordinator()
	{
		debug("Creating ServiceCoordinator");
	}

	~ServiceCoordinator() override
	{
		debug("Destroying ServiceCoordinator");
	}

	void Run() override
	{
		debug("Entering ServiceCoordinator-loop");

		while (!IsStopped())
		{
			// perform service work
			for (Service* service : m_services)
			{
				int serviceInterval = service->ServiceInterval();
				if (nzbget_rs_euh_service_should_work_now(
						serviceInterval, Service::Now,
						static_cast<int64_t>(Util::CurrentTime()),
						static_cast<int64_t>(service->m_lastWork)) != 0)
				{
					service->ServiceWork();
					service->m_lastWork = Util::CurrentTime();
				}
			}

			// calculate wait interval
			int waitInterval = 60 * 60 * 24; // something very large
			time_t curTime = Util::CurrentTime();
			for (Service* service : m_services)
			{
				int serviceInterval = service->ServiceInterval();
				int remaining = Service::Sleep;
				nzbget_rs_euh_service_calc_remaining_wait(
					serviceInterval, Service::Now, Service::Sleep,
					static_cast<int64_t>(curTime),
					static_cast<int64_t>(service->m_lastWork),
					waitInterval, &remaining, &waitInterval);
				debug("serviceInterval: %i, remaining: %i", serviceInterval, remaining);
			}

			debug("Waiting in ServiceCoordinator: %i", waitInterval);
			if (waitInterval > 0)
			{
				std::unique_lock<std::mutex> lk(m_waitMutex);
				m_waitCond.wait_for(lk, std::chrono::milliseconds(waitInterval * 1000), [&] { return m_workenUp || IsStopped(); });
				m_workenUp = false;
			}
		}

		debug("Exiting ServiceCoordinator-loop");
	}

	void Stop() override
	{
		Thread::Stop();

		// Resume Run() to exit it
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_waitCond.notify_all();
	}

private:
	ServiceList m_services;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	bool m_workenUp = false;

	void RegisterService(Service* service)
	{
		m_services.push_back(service);
	}

	void WakeUp()
	{
		debug("Waking up ServiceCoordinator");
		// Resume Run()
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_workenUp = true;
		m_waitCond.notify_all();
	}

	friend class Service;
};

inline Service::Service()
{
	g_ServiceCoordinator->RegisterService(this);
}

inline void Service::WakeUp()
{
	g_ServiceCoordinator->WakeUp();
}

#endif

inline const char* PossibleConfigLocations[] =
	{
		"~/.rustyget",
		"~/.config/rustyget.conf",
		"~/.config/rustyget/rustyget.conf",
		"/etc/rustyget.conf",
		"/usr/etc/rustyget.conf",
		"/usr/local/etc/rustyget.conf",
		"/opt/etc/rustyget.conf",
		"~/usr/etc/rustyget.conf",
		"~/.nzbget",
		"~/.config/nzbget.conf",
		"~/.config/nzbget/nzbget.conf",
		"/etc/nzbget.conf",
		"/usr/etc/nzbget.conf",
		"/usr/local/etc/nzbget.conf",
		"/opt/etc/nzbget.conf",
		"~/usr/etc/nzbget.conf",
		nullptr
	};

inline void Options::OptEntry::SetValue(const char* value)
{
	m_value = value;
	if (!m_defValue)
	{
		m_defValue = value;
	}
}

inline bool Options::OptEntry::Restricted()
{
	// rust/src/options.rs
	return nzbget_rs_option_restricted(m_name, (char)-1 < 0) != 0;
}

inline Options::OptEntry* Options::OptEntries::FindOption(const char* name)
{
	if (!name)
	{
		return nullptr;
	}

	for (OptEntry& optEntry : *this)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(optEntry.GetName(), name) != 0)
		{
			return &optEntry;
		}
	}

	return nullptr;
}

inline Options::Category* Options::Categories::FindCategory(const char* name, bool searchAliases)
{
	if (!name)
	{
		return nullptr;
	}

	for (Category& category : *this)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(category.GetName(), name) != 0)
		{
			return &category;
		}
	}

	if (searchAliases)
	{
		for (Category& category : *this)
		{
			std::vector<const char*> aliasPtrs;
			aliasPtrs.reserve(category.GetAliases()->size());
			for (const CString& alias : *category.GetAliases())
			{
				aliasPtrs.push_back(alias ? *alias : "");
			}
			if (nzbget_rs_options_category_alias_list_matches(
				aliasPtrs.data(), aliasPtrs.size(), name, (char)-1 < 0) != 0)
			{
				return &category;
			}
		}
	}

	return nullptr;
}

inline Options::Extender::~Extender() = default;

inline Options::Options(const char* exeName, const char* configFilename, bool noConfig,
	CmdOptList* commandLineOptions, Extender* extender)
{
	Init(exeName, configFilename, noConfig, commandLineOptions, false, extender);
}

inline Options::Options(CmdOptList* commandLineOptions, Extender* extender)
{
	Init("rustyget/rustyget", nullptr, true, commandLineOptions, true, extender);
}

inline void Options::Init(const char* exeName, const char* configFilename, bool noConfig,
	CmdOptList* commandLineOptions, bool noDiskAccess, Extender* extender)
{
	g_Options = this;
	m_extender = extender;
	m_noDiskAccess = noDiskAccess;
	m_noConfig = noConfig;
	m_configFilename = configFilename;

	SetOption(CONFIGFILE.data(), "");

	CString filename;
	if (m_noDiskAccess)
	{
		filename = exeName;
	}
	else
	{
		filename = FileSystem::GetExeFileName(exeName);
	}
	FileSystem::NormalizePathSeparators(filename);
	SetOption(APPBIN.data(), filename);
	NzbgetRsBuf appDirBuf = nzbget_rs_dqh_fs_extract_parent_dir_path(filename, PATH_SEPARATOR);
	filename.Set(appDirBuf.data ? appDirBuf.data : "", (int)appDirBuf.len);
	nzbget_rs_free(appDirBuf);
	SetOption(APPDIR.data(), filename);
	m_appDir = *filename;
	m_appDirPath = fs::u8path(*m_appDir);

	SetOption(APPVERSION.data(), Util::VersionRevision());

	InitDefaults();

	InitOptFile();
	if (m_fatalError)
	{
		return;
	}

	if (commandLineOptions)
	{
		InitCommandLineOptions(commandLineOptions);
	}

	if (!m_configFilename && !noConfig)
	{
		printf("No configuration-file found\n");
		printf("Please use option \"-c\" or put configuration-file in one of the following locations:\n");
		int p = 0;
		while (const char* filename = PossibleConfigLocations[p++])
		{
			printf("%s\n", filename);
		}
		m_fatalError = true;
		return;
	}

	ConvertOldOptions(&m_optEntries);

	CheckDirs();
	InitOptions();
	CheckOptions();

	InitServers();
	InitCategories();
	InitScheduler();
	InitFeeds();
}

inline Options::~Options()
{
	g_Options = nullptr;
}

inline NzbgetRsOptionsHost Options::MakeHost()
{
	NzbgetRsOptionsHost host{};
	host.ctx = this;
	host.find_entry = [](void* ctx, const char* name, const char** outVal, const char** outDefVal, int* outLineNo) -> int
	{
		auto* self = static_cast<Options*>(ctx);
		OptEntry* optEntry = self->FindOption(name);
		if (!optEntry)
		{
			return 0;
		}
		if (outVal) *outVal = optEntry->GetValue();
		if (outDefVal) *outDefVal = optEntry->GetDefValue();
		if (outLineNo) *outLineNo = optEntry->GetLineNo();
		return 1;
	};
	host.set_entry_raw_value = [](void* ctx, const char* name, const char* value)
	{
		auto* self = static_cast<Options*>(ctx);
		if (OptEntry* optEntry = self->FindOption(name))
		{
			optEntry->SetValue(value);
		}
	};
	host.set_option = [](void* ctx, const char* name, const char* value)
	{
		static_cast<Options*>(ctx)->SetOption(name, value);
	};
	host.set_config_line = [](void* ctx, int line)
	{
		static_cast<Options*>(ctx)->m_configLine = line;
	};
	host.get_config_line = [](void* ctx) -> int
	{
		return static_cast<Options*>(ctx)->m_configLine;
	};
	host.config_error = [](void* ctx, const char* msg)
	{
		static_cast<Options*>(ctx)->ConfigError("%s", msg);
	};
	host.config_warn = [](void* ctx, const char* msg)
	{
		static_cast<Options*>(ctx)->ConfigWarn("%s", msg);
	};
	host.file_exists = [](void* /*ctx*/, const char* path) -> int
	{
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	host.ensure_directory = [](void* ctx, const char* dir, char* errBuf, size_t errCap) -> int
	{
		auto* self = static_cast<Options*>(ctx);
		CString errmsg;
		if (!FileSystem::ForceDirectories(dir, errmsg))
		{
			if (errBuf && errCap > 0)
			{
				snprintf(errBuf, errCap, "%s", errmsg ? *errmsg : "");
			}
			return 0;
		}
		if (getuid() == 0 || geteuid() == 0)
		{
			struct passwd* pwd = getpwnam(self->GetOption(DAEMONUSERNAME.data()));
			if (pwd != nullptr)
			{
				chown(dir, pwd->pw_uid, pwd->pw_gid);
			}
		}
		return 1;
	};
	host.add_news_server = [](void* ctx, int id, int active, const char* name, const char* host,
		int port, int ipVersion, const char* user, const char* pass, int joinGroup, int tls,
		const char* cipher, int maxConnections, int retention, int level, int group, int optional,
		uint32_t certVerifLevel)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddNewsServer(id, active != 0, name, host, port, ipVersion,
				user, pass, joinGroup != 0, tls != 0, cipher, maxConnections, retention,
				level, group, optional != 0, certVerifLevel);
		}
	};
	host.add_feed = [](void* ctx, int id, const char* name, const char* url, int interval,
		const char* filter, int backlog, int pauseNzb, const char* category, int categorySource,
		int priority, const char* extensions, uint32_t certVerifLevel)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddFeed(id, name, url, interval, filter, backlog != 0, pauseNzb != 0,
				category, static_cast<FeedInfo::CategorySource>(categorySource), priority,
				extensions, certVerifLevel);
		}
	};
	host.add_task = [](void* ctx, int id, int hours, int minutes, int weekDaysBits,
		int command, const char* param)
	{
		auto* self = static_cast<Options*>(ctx);
		if (self->m_extender)
		{
			self->m_extender->AddTask(id, hours, minutes, weekDaysBits,
				static_cast<ESchedulerCommand>(command), param);
		}
	};
	host.add_category = [](void* ctx, const char* name, const char* destDir, int unpack,
		const char* extensions, const char* const* aliases, size_t aliasesLen)
	{
		auto* self = static_cast<Options*>(ctx);
		self->m_categories.emplace_back(name, destDir, unpack != 0, extensions);
		Category& category = self->m_categories.back();
		for (size_t i = 0; i < aliasesLen; ++i)
		{
			category.GetAliases()->push_back(aliases[i]);
		}
	};
	return host;
}

inline NzbgetRsParsedOptions Options::ExportParsedScalars() const
{
	NzbgetRsParsedOptions s{};
	s.download_rate = m_downloadRate;
	s.article_timeout = m_articleTimeout;
	s.article_read_chunk_size = m_articleReadChunkSize;
	s.url_timeout = m_urlTimeout;
	s.remote_timeout = m_remoteTimeout;
	s.article_retries = m_articleRetries;
	s.article_interval = m_articleInterval;
	s.url_retries = m_urlRetries;
	s.url_interval = m_urlInterval;
	s.control_port = m_controlPort;
	s.secure_port = m_securePort;
	s.url_connections = m_urlConnections;
	s.log_buffer = m_logBuffer;
	s.rotate_log = m_rotateLog;
	s.umask = m_umask;
	s.update_interval = m_updateInterval;
	s.write_buffer = m_writeBuffer;
	s.nzb_dir_interval = m_nzbDirInterval;
	s.nzb_dir_file_age = m_nzbDirFileAge;
	s.nzb_dir_archive_scan = m_nzbDirArchiveScan ? 1 : 0;
	s.nzb_dir_archive_action = (int)m_nzbDirArchiveAction;
	s.disk_space = m_diskSpace;
	s.par_time_limit = m_parTimeLimit;
	s.keep_history = m_keepHistory;
	s.feed_history = m_feedHistory;
	s.time_correction = m_timeCorrection;
	s.propagation_delay = m_propagationDelay;
	s.article_cache = m_articleCache;
	s.memory_pressure_interval = m_memoryPressureInterval;
	s.event_interval = m_eventInterval;
	s.par_buffer = m_parBuffer;
	s.par_threads = m_parThreads;
	s.monthly_quota = m_monthlyQuota;
	s.quota_start_day = m_quotaStartDay;
	s.daily_quota = m_dailyQuota;
	s.system_health_check = m_systemHealthCheck ? 1 : 0;
	s.nzb_log = m_nzbLog ? 1 : 0;
	s.append_category_dir = m_appendCategoryDir ? 1 : 0;
	s.continue_partial = m_continuePartial ? 1 : 0;
	s.flush_queue = m_flushQueue ? 1 : 0;
	s.dupe_check = m_dupeCheck ? 1 : 0;
	s.par_repair = m_parRepair ? 1 : 0;
	s.par_quick = m_parQuick ? 1 : 0;
	s.par_rename = m_parRename ? 1 : 0;
	s.rar_rename = m_rarRename ? 1 : 0;
	s.direct_rename = m_directRename ? 1 : 0;
	s.hard_linking = m_hardLinking ? 1 : 0;
	s.curses_nzb_name = m_cursesNzbName ? 1 : 0;
	s.curses_time = m_cursesTime ? 1 : 0;
	s.curses_group = m_cursesGroup ? 1 : 0;
	s.crc_check = m_crcCheck ? 1 : 0;
	s.direct_write = m_directWrite ? 1 : 0;
	s.raw_article = m_rawArticle ? 1 : 0;
	s.skip_write = m_skipWrite ? 1 : 0;
	s.crash_trace = m_crashTrace ? 1 : 0;
	s.crash_dump = m_crashDump ? 1 : 0;
	s.par_pause_queue = m_parPauseQueue ? 1 : 0;
	s.script_pause_queue = m_scriptPauseQueue ? 1 : 0;
	s.nzb_cleanup_disk = m_nzbCleanupDisk ? 1 : 0;
	s.form_auth = m_formAuth ? 1 : 0;
	s.json_p = m_jsonP ? 1 : 0;
	s.secure_control = m_secureControl ? 1 : 0;
	s.unpack = m_unpack ? 1 : 0;
	s.direct_unpack = m_directUnpack ? 1 : 0;
	s.use_temp_unpack_dir = m_useTempUnpackDir ? 1 : 0;
	s.unpack_cleanup_disk = m_unpackCleanupDisk ? 1 : 0;
	s.unpack_pause_queue = m_unpackPauseQueue ? 1 : 0;
	s.unpack_mutex_enforced = m_unpackMutexEnforced ? 1 : 0;
	s.url_force = m_urlForce ? 1 : 0;
	s.cert_check = m_certCheck ? 1 : 0;
	s.reorder_files = m_reorderFiles ? 1 : 0;
	s.post_download_rename = m_postDownloadRename ? 1 : 0;
	s.rename_after_unpack = m_renameAfterUnpack ? 1 : 0;
	s.output_mode = (int)m_outputMode;
	s.par_check = (int)m_parCheck;
	s.par_scan = (int)m_parScan;
	s.dupe_article_fallback = (int)m_dupeArticleFallback;
	s.dupe_stream_decompress = m_dupeStreamDecompress ? 1 : 0;
	s.dupe_stream_timeout = m_dupeStreamTimeout;
	s.dupe_search = m_dupeSearch ? 1 : 0;
	s.dupe_search_dry_run = m_dupeSearchDryRun ? 1 : 0;
	s.dupe_search_delay = m_dupeSearchDelay;
	s.dupe_search_max_donors = m_dupeSearchMaxDonors;
	s.dupe_health_percent = m_dupeHealthPercent;
	s.dupe_health_min = m_dupeHealthMin;
	s.dupe_health_max = m_dupeHealthMax;
	s.dupe_body_checks = m_dupeBodyChecks;
	s.dupe_health_budget = m_dupeHealthBudget;
	s.dupe_min_alive = m_dupeMinAlive;
	s.dupe_fast_donors = m_dupeFastDonors;
	s.post_strategy = (int)m_postStrategy;
	s.file_naming = (int)m_fileNaming;
	s.health_check = (int)m_healthCheck;
	s.info_target = (int)m_infoTarget;
	s.warning_target = (int)m_warningTarget;
	s.error_target = (int)m_errorTarget;
	s.debug_target = (int)m_debugTarget;
	s.detail_target = (int)m_detailTarget;
	s.write_log = (int)m_writeLog;
	s.tls = m_tls ? 1 : 0;
	return s;
}

inline void Options::ApplyParsedScalars(const NzbgetRsParsedOptions& s)
{
	m_downloadRate = s.download_rate;
	m_articleTimeout = s.article_timeout;
	m_articleReadChunkSize = s.article_read_chunk_size;
	m_urlTimeout = s.url_timeout;
	m_remoteTimeout = s.remote_timeout;
	m_articleRetries = s.article_retries;
	m_articleInterval = s.article_interval;
	m_urlRetries = s.url_retries;
	m_urlInterval = s.url_interval;
	m_controlPort = s.control_port;
	m_securePort = s.secure_port;
	m_urlConnections = s.url_connections;
	m_logBuffer = s.log_buffer;
	m_rotateLog = s.rotate_log;
	m_umask = s.umask;
	m_updateInterval = s.update_interval;
	m_writeBuffer = s.write_buffer;
	m_nzbDirInterval = s.nzb_dir_interval;
	m_nzbDirFileAge = s.nzb_dir_file_age;
	m_nzbDirArchiveScan = s.nzb_dir_archive_scan != 0;
	m_nzbDirArchiveAction = (ENzbDirArchiveAction)s.nzb_dir_archive_action;
	m_diskSpace = s.disk_space;
	m_parTimeLimit = s.par_time_limit;
	m_keepHistory = s.keep_history;
	m_feedHistory = s.feed_history;
	m_timeCorrection = s.time_correction;
	m_propagationDelay = s.propagation_delay;
	m_articleCache = s.article_cache;
	m_memoryPressureInterval = s.memory_pressure_interval;
	m_eventInterval = s.event_interval;
	m_parBuffer = s.par_buffer;
	m_parThreads = s.par_threads;
	m_monthlyQuota = s.monthly_quota;
	m_quotaStartDay = s.quota_start_day;
	m_dailyQuota = s.daily_quota;
	m_systemHealthCheck = s.system_health_check != 0;
	m_nzbLog = s.nzb_log != 0;
	m_appendCategoryDir = s.append_category_dir != 0;
	m_continuePartial = s.continue_partial != 0;
	m_flushQueue = s.flush_queue != 0;
	m_dupeCheck = s.dupe_check != 0;
	m_parRepair = s.par_repair != 0;
	m_parQuick = s.par_quick != 0;
	m_parRename = s.par_rename != 0;
	m_rarRename = s.rar_rename != 0;
	m_directRename = s.direct_rename != 0;
	m_hardLinking = s.hard_linking != 0;
	m_cursesNzbName = s.curses_nzb_name != 0;
	m_cursesTime = s.curses_time != 0;
	m_cursesGroup = s.curses_group != 0;
	m_crcCheck = s.crc_check != 0;
	m_directWrite = s.direct_write != 0;
	m_rawArticle = s.raw_article != 0;
	m_skipWrite = s.skip_write != 0;
	m_crashTrace = s.crash_trace != 0;
	m_crashDump = s.crash_dump != 0;
	m_parPauseQueue = s.par_pause_queue != 0;
	m_scriptPauseQueue = s.script_pause_queue != 0;
	m_nzbCleanupDisk = s.nzb_cleanup_disk != 0;
	m_formAuth = s.form_auth != 0;
	m_jsonP = s.json_p != 0;
	m_secureControl = s.secure_control != 0;
	m_unpack = s.unpack != 0;
	m_directUnpack = s.direct_unpack != 0;
	m_useTempUnpackDir = s.use_temp_unpack_dir != 0;
	m_unpackCleanupDisk = s.unpack_cleanup_disk != 0;
	m_unpackPauseQueue = s.unpack_pause_queue != 0;
	m_unpackMutexEnforced = s.unpack_mutex_enforced != 0;
	m_urlForce = s.url_force != 0;
	m_certCheck = s.cert_check != 0;
	m_reorderFiles = s.reorder_files != 0;
	m_postDownloadRename = s.post_download_rename != 0;
	m_renameAfterUnpack = s.rename_after_unpack != 0;
	m_outputMode = (EOutputMode)s.output_mode;
	m_parCheck = (EParCheck)s.par_check;
	m_parScan = (EParScan)s.par_scan;
	m_dupeArticleFallback = (EDupeArticleFallback)s.dupe_article_fallback;
	m_dupeStreamDecompress = s.dupe_stream_decompress != 0;
	m_dupeStreamTimeout = s.dupe_stream_timeout;
	m_dupeSearch = s.dupe_search != 0;
	m_dupeSearchDryRun = s.dupe_search_dry_run != 0;
	m_dupeSearchDelay = s.dupe_search_delay;
	m_dupeSearchMaxDonors = s.dupe_search_max_donors;
	m_dupeHealthPercent = s.dupe_health_percent;
	m_dupeHealthMin = s.dupe_health_min;
	m_dupeHealthMax = s.dupe_health_max;
	m_dupeBodyChecks = s.dupe_body_checks;
	m_dupeHealthBudget = s.dupe_health_budget;
	m_dupeMinAlive = s.dupe_min_alive;
	m_dupeFastDonors = s.dupe_fast_donors;
	m_postStrategy = (EPostStrategy)s.post_strategy;
	m_fileNaming = (EFileNaming)s.file_naming;
	m_healthCheck = (EHealthCheck)s.health_check;
	m_infoTarget = (EMessageTarget)s.info_target;
	m_warningTarget = (EMessageTarget)s.warning_target;
	m_errorTarget = (EMessageTarget)s.error_target;
	m_debugTarget = (EMessageTarget)s.debug_target;
	m_detailTarget = (EMessageTarget)s.detail_target;
	m_writeLog = (EWriteLog)s.write_log;
	m_tls = s.tls != 0;
}

inline void Options::ConfigError(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	printf("%s(%i): %s\n", m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>", m_configLine, tmp2);
	error("%s(%i): %s", m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>", m_configLine, tmp2);

	m_configErrors = true;
}

inline void Options::ConfigWarn(const char* msg, ...)
{
	char tmp2[1024];

	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp2, 1024, msg, ap);
	tmp2[1024-1] = '\0';
	va_end(ap);

	const char* filename = m_configFilename ? FileSystem::BaseFileName(m_configFilename) : "<noconfig>";
	printf("%s(%i): %s\n", filename, m_configLine, tmp2);
	warn("%s(%i): %s", filename, m_configLine, tmp2);
}

inline void Options::InitDefaults()
{
#ifdef DISABLE_CURSES
	int disableCurses = 1;
#else
	int disableCurses = 0;
#endif
	nzbget_rs_options_init_defaults(
		disableCurses,
		[](void* ctx, const char* name, const char* value)
		{
			static_cast<Options*>(ctx)->SetOption(name, value);
		},
		this);
}

inline void Options::InitOptFile()
{
	if (!m_configFilename && !m_noConfig)
	{
		int found = 0;
		NzbgetRsBuf foundBuf = nzbget_rs_options_find_default_config_file(
			*m_appDir,
			'/',
			nullptr,
			[](void*, const char* path) -> int
			{
				return FileSystem::FileExists(path) ? 1 : 0;
			},
			&found);
		if (found)
		{
			m_configFilename.Set(foundBuf.data ? foundBuf.data : "", (int)foundBuf.len);
		}
		nzbget_rs_free(foundBuf);
	}

	if (m_configFilename)
	{
		// normalize path in filename
		CString filename = FileSystem::ExpandFileName(m_configFilename);

		// substitute HOME-variable
		filename = FileSystem::ExpandHomePath(filename);

		m_configFilename = *filename;
		m_configFilePath = fs::u8path(*m_configFilename);

		SetOption(CONFIGFILE.data(), m_configFilename);

		LoadConfigFile();
	}
}

inline void Options::CheckDir(CString& dir, const char* optionName,
	const char* parentDir, bool allowEmpty, bool create)
{
	NzbgetRsOptionsHost host = MakeHost();
	int hasDir = 0;
	NzbgetRsBuf buf = nzbget_rs_options_check_dir(
		&host, m_noDiskAccess ? 1 : 0, optionName, parentDir,
		allowEmpty ? 1 : 0, create ? 1 : 0, &hasDir);
	if (hasDir)
	{
		dir = buf.data ? buf.data : "";
	}
	else
	{
		dir = nullptr;
	}
	nzbget_rs_free(buf);
}

// ScriptDir may list several folders: each is made absolute and created on
// its own (as one path, "a;b" created junk folders such as "a;")
inline void Options::CheckScriptDir()
{
	NzbgetRsOptionsHost host = MakeHost();
	int hasDir = 0;
	NzbgetRsBuf buf = nzbget_rs_options_check_script_dir(
		&host, m_noDiskAccess ? 1 : 0, m_mainDir, &hasDir);
	if (hasDir)
	{
		m_scriptDir = buf.data ? buf.data : "";
	}
	else
	{
		m_scriptDir = nullptr;
	}
	nzbget_rs_free(buf);
}

inline void Options::CheckDirs()
{
	m_mainDir = GetOption(MAINDIR.data());

	CheckDir(m_destDir, DESTDIR.data(), m_mainDir, false, true);
	CheckDir(m_interDir, INTERDIR.data(), m_mainDir, true, true);
	CheckDir(m_tempDir, TEMPDIR.data(), m_mainDir, false, true);
	CheckDir(m_queueDir, QUEUEDIR.data(), m_mainDir, false, true);
	CheckDir(m_webDir, WEBDIR.data(), nullptr, true, false);
	CheckScriptDir();
	CheckDir(m_nzbDir, NZBDIR.data(), m_mainDir, false, true);

	m_mainDirPath = fs::u8path(*m_mainDir);
	m_destDirPath = fs::u8path(*m_destDir);
	m_interDirPath = fs::u8path(*m_interDir);
	m_tempDirPath = fs::u8path(*m_tempDir);
	m_queueDirPath = fs::u8path(*m_queueDir);
	m_webDirPath = fs::u8path(*m_webDir);
	m_nzbDirPath = fs::u8path(*m_nzbDir);

	Tokenizer tokDir(g_Options->GetScriptDir(), ",;");
	while (const char* scriptDir = tokDir.Next())
	{
		fs::path path;
		path = fs::u8path(scriptDir);
		m_scriptDirPaths.push_back(std::move(path));
	}
}

inline void Options::InitOptions()
{
	m_requiredDir = GetOption(REQUIREDDIR.data());

	m_configTemplate		= GetOption(CONFIGTEMPLATE.data());
	m_scriptOrder			= GetOption(SCRIPTORDER.data());
	m_extensions			= GetOption(EXTENSIONS.data());
	m_controlIp				= GetOption(CONTROLIP.data());
	m_controlUsername		= GetOption(CONTROLUSERNAME.data());
	m_controlPassword		= GetOption(CONTROLPASSWORD.data());
	m_restrictedUsername	= GetOption(RESTRICTEDUSERNAME.data());
	m_restrictedPassword	= GetOption(RESTRICTEDPASSWORD.data());
	m_addUsername			= GetOption(ADDUSERNAME.data());
	m_addPassword			= GetOption(ADDPASSWORD.data());
	m_secureCert			= GetOption(SECURECERT.data());
	m_secureKey				= GetOption(SECUREKEY.data());
	m_certStore				= GetOption(CERTSTORE.data());
	m_authorizedIp			= GetOption(AUTHORIZEDIP.data());
	m_lockFile				= GetOption(LOCKFILE.data());
	m_daemonUsername		= GetOption(DAEMONUSERNAME.data());
	m_logFile				= GetOption(LOGFILE.data());
	m_unrarCmd				= GetOption(UNRARCMD.data());
	m_sevenZipCmd			= GetOption(SEVENZIPCMD.data());
	m_unpackPassFile		= GetOption(UNPACKPASSFILE.data());
	m_extCleanupDisk		= GetOption(EXTCLEANUPDISK.data());
	m_parIgnoreExt			= GetOption(PARIGNOREEXT.data());
	m_unpackIgnoreExt		= GetOption(UNPACKIGNOREEXT.data());

	m_configTemplatePath = fs::u8path(*m_configTemplate);
	m_secureCertPath = fs::u8path(*m_secureCert);
	m_secureKeyPath = fs::u8path(*m_secureKey);
	m_certStorePath = fs::u8path(*m_certStore);
	m_lockFilePath = fs::u8path(*m_lockFile);
	m_logFilePath = fs::u8path(*m_logFile);
	m_unpackPassFilePath = fs::u8path(*m_unpackPassFile);
	SetToolPathOption(m_unrarPath, *m_unrarCmd);
	SetToolPathOption(m_sevenZipPath, *m_sevenZipCmd);

	const char* shellOverride = GetOption(SHELLOVERRIDE.data());
	m_shellOverride	= shellOverride ? shellOverride : "";

	m_renameIgnoreExt 	    = GetOption(RENAMEIGNOREEXT.data());

	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars{};
	int hasMemPath = 0, hasHlExt = 0, hasDupeUrl = 0, hasDupeKey = 0, warnDupeNoUrl = 0;
	NzbgetRsBuf memPathBuf{}, hlExtBuf{}, dupeUrlBuf{}, dupeKeyBuf{};
	nzbget_rs_options_init_scalars(
		&host, &scalars,
		&hasMemPath, &memPathBuf,
		&hasHlExt, &hlExtBuf,
		&hasDupeUrl, &dupeUrlBuf,
		&hasDupeKey, &dupeKeyBuf,
		&warnDupeNoUrl);
	m_memoryPressurePath = hasMemPath ? (memPathBuf.data ? memPathBuf.data : "") : nullptr;
	m_hardLinkingIgnoreExt = hasHlExt ? (hlExtBuf.data ? hlExtBuf.data : "") : nullptr;
	m_dupeSearchUrl = hasDupeUrl ? (dupeUrlBuf.data ? dupeUrlBuf.data : "") : nullptr;
	m_dupeSearchApiKey = hasDupeKey ? (dupeKeyBuf.data ? dupeKeyBuf.data : "") : nullptr;
	nzbget_rs_free(memPathBuf);
	nzbget_rs_free(hlExtBuf);
	nzbget_rs_free(dupeUrlBuf);
	nzbget_rs_free(dupeKeyBuf);
	ApplyParsedScalars(scalars);

	if (warnDupeNoUrl)
	{
		warn("Option <%s> is enabled but option <%s> is empty: no duplicates will be searched",
			DUPESEARCH.data(), DUPESEARCHURL.data());
	}
}

inline void Options::SetOption(const char* optname, const char* value)
{
	OptEntry* optEntry = FindOption(optname);
	if (!optEntry)
	{
		m_optEntries.emplace_back(optname, nullptr);
		optEntry = &m_optEntries.back();
	}

	CString curvalue;

	if (nzbget_rs_dqh_options_should_expand_home_value(value, m_noDiskAccess ? 1 : 0) != 0)
	{
		curvalue = FileSystem::ExpandHomePath(value);
	}
	else
	{
		curvalue = value;
	}

	optEntry->SetLineNo(m_configLine);

	if (curvalue)
	{
		NzbgetRsBuf expanded = nzbget_rs_options_expand_variables(
			curvalue,
			[](void* ctx, const char* name) -> const char*
			{
				return static_cast<Options*>(ctx)->GetOption(name);
			},
			this);
		curvalue.Set(expanded.data ? expanded.data : "", (int)expanded.len);
		nzbget_rs_free(expanded);
	}

	optEntry->SetValue(curvalue);
}

inline void Options::SetToolPathOption(fs::path& pathOpt, std::string_view value)
{
	if (value.empty()) return;

	NzbgetRsBuf resolved = nzbget_rs_options_resolve_tool_path(value.data(), value.size());
	std::string_view sv(resolved.data ? resolved.data : "", resolved.len);
	fs::path directPath = fs::u8path(sv);
	nzbget_rs_free(resolved);
	pathOpt.swap(directPath);
}

inline Options::OptEntry* Options::FindOption(const char* optname)
{
	OptEntry* optEntry = m_optEntries.FindOption(optname);

	// normalize option name in option list; for example "server1.joingroup" -> "Server1.JoinGroup"
	if (optEntry && nzbget_rs_dqh_bytes_equal(optEntry->GetName(), optname) == 0)
	{
		optEntry->SetName(optname);
	}

	return optEntry;
}

inline const char* Options::GetOption(const char* optname)
{
	OptEntry* optEntry = FindOption(optname);
	if (optEntry)
	{
		m_configLine = nzbget_rs_dqh_options_update_config_line_on_get(
			optEntry->GetLineNo(), m_configLine);
		return optEntry->GetValue();
	}
	return nullptr;
}

inline void Options::InitServers()
{
	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars = ExportParsedScalars();
#ifdef DISABLE_TLS
	int disableTls = 1;
#else
	int disableTls = 0;
#endif
	nzbget_rs_options_init_servers(&host, disableTls, &scalars);
	ApplyParsedScalars(scalars);
}

inline void Options::InitCategories()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_categories(&host, m_noDiskAccess ? 1 : 0, m_destDir);
}

inline void Options::InitFeeds()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_feeds(&host);
}

inline void Options::InitScheduler()
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_init_scheduler(&host);
}

inline void Options::CreateSchedulerTask(int id, const char* time, const char* weekDays,
	ESchedulerCommand command, const char* param)
{
	NzbgetRsOptionsHost host = MakeHost();
	nzbget_rs_options_create_scheduler_task(&host, id, time, weekDays, (int)command, param);
}

inline void Options::LoadConfigFile()
{
	SetOption(CONFIGFILE.data(), m_configFilename);

	m_configLine = 0;
	if (!nzbget_rs_options_load_config_file(
		m_configFilename,
		this,
		[](void* rawCtx, int lineNo, const char* line)
		{
			auto* self = static_cast<Options*>(rawCtx);
			self->m_configLine = lineNo;
			self->SetOptionString(line);
		}))
	{
		ConfigError("Could not open file %s", *m_configFilename);
		m_fatalError = true;
		return;
	}

	m_configLine = 0;
}

inline void Options::InitCommandLineOptions(CmdOptList* commandLineOptions)
{
	for (const char* option : *commandLineOptions)
	{
		SetOptionString(option);
	}
}

inline bool Options::SetOptionString(const char* option)
{
	CString optname;
	CString optvalue;

	if (!SplitOptionString(option, optname, optvalue))
	{
		ConfigError("Invalid option \"%s\"", option);
		return false;
	}

	bool ok = ValidateOptionName(optname, optvalue);
	if (ok)
	{
		SetOption(optname, optvalue);
	}
	else
	{
		ConfigError("Invalid option \"%s\"", *optname);
	}

	return ok;
}

/*
 * Splits option string into name and value;
 * Converts old names and values if necessary;
 * Returns true if the option string has name and value;
 */
inline bool Options::SplitOptionString(const char* option, CString& optName, CString& optValue)
{
	NzbgetRsBuf nameBuf{}, valBuf{};
	if (!nzbget_rs_options_split_and_convert(option, &nameBuf, &valBuf))
	{
		return false;
	}

	optName.Set(nameBuf.data ? nameBuf.data : "", (int)nameBuf.len);
	optValue.Set(valBuf.data ? valBuf.data : "", (int)valBuf.len);
	nzbget_rs_free(nameBuf);
	nzbget_rs_free(valBuf);

	return true;
}

inline bool Options::ValidateOptionName(const char* optname, const char* optvalue)
{
	// rust/src/options.rs: the name's class; the messages are logged here
	auto predefined = [](void* ctx, const char* name) -> int
	{
		return static_cast<Options*>(ctx)->GetOption(name) != nullptr;
	};
	switch (nzbget_rs_validate_option_name(optname, predefined, this))
	{
		case 1:
			return true;

		case 2:
			ConfigWarn("Option \"%s\" is obsolete, ignored", optname);
			return true;

		case 3:
			if (!Util::EmptyStr(optvalue))
			{
				ConfigError("Option \"%s\" is obsolete, ignored, use \"%s\" and \"%s\" instead",
					optname, SCRIPTDIR.data(), EXTENSIONS.data());
			}
			return true;

		case 4:
			ConfigWarn("Option \"%s\" is obsolete, ignored, use \"%s\" instead", optname, WRITELOG.data());
			return true;

		default:
			return false;
	}
}

inline void Options::CheckOptions()
{
	NzbgetRsOptionsHost host = MakeHost();
	NzbgetRsParsedOptions scalars = ExportParsedScalars();
#ifdef DISABLE_PARCHECK
	int disableParcheck = 1;
#else
	int disableParcheck = 0;
#endif
#ifdef DISABLE_CURSES
	int disableCurses = 1;
#else
	int disableCurses = 0;
#endif
#ifdef DISABLE_TLS
	int disableTls = 1;
#else
	int disableTls = 0;
#endif
#ifdef HAVE_X509_CHECK_HOST
	int haveX509CheckHost = 1;
#else
	int haveX509CheckHost = 0;
#endif
	int ctChanged = 0;
	NzbgetRsBuf ctBuf = nzbget_rs_options_check_options(
		&host, &scalars,
		m_noDiskAccess ? 1 : 0,
		sizeof(void*) == 4 ? 1 : 0,
		disableParcheck, disableCurses, disableTls, haveX509CheckHost,
		m_certStore.Empty() ? 1 : 0,
		m_webDir ? *m_webDir : "",
		m_configTemplate ? *m_configTemplate : nullptr,
		m_unpackPassFile ? *m_unpackPassFile : nullptr,
		&ctChanged);
	if (ctChanged)
	{
		m_configTemplate.Set(ctBuf.data ? ctBuf.data : "", (int)ctBuf.len);
	}
	nzbget_rs_free(ctBuf);
	ApplyParsedScalars(scalars);
}

inline void Options::ConvertOldOptions(OptEntries* optEntries)
{
	if (!optEntries) return;
	std::vector<const char*> names;
	std::vector<const char*> values;
	names.reserve(optEntries->size());
	values.reserve(optEntries->size());
	for (const OptEntry& opt : *optEntries)
	{
		names.push_back(opt.GetName() ? opt.GetName() : "");
		values.push_back(opt.GetValue() ? opt.GetValue() : "");
	}
	nzbget_rs_options_convert_old_options(
		names.data(),
		values.data(),
		names.size(),
		optEntries,
		[](void* ctx, size_t idx, const char* name, const char* value)
		{
			auto* entries = static_cast<OptEntries*>(ctx);
			if (idx < entries->size())
			{
				(*entries)[idx].m_value = value;
			}
			else
			{
				entries->emplace_back(name, value);
			}
		});
}

#ifndef DECODER_H
#define DECODER_H

class Decoder
{
public:
	enum EStatus
	{
		dsUnknownError,
		dsFinished,
		dsArticleIncomplete,
		dsCrcError,
		dsInvalidSize,
		dsNoBinaryData
	};

	enum EFormat
	{
		efUnknown,
		efYenc,
		efUx,
	};

	// rust/src/decoder.rs
	Decoder() : m_decoder(nzbget_rs_decoder_new(nullptr, nullptr)) {}
	~Decoder() { nzbget_rs_decoder_free(m_decoder); }
	Decoder(const Decoder&) = delete;
	Decoder& operator=(const Decoder&) = delete;
	EStatus Check() { return static_cast<EStatus>(nzbget_rs_decoder_check(m_decoder)); }
	void Clear() { nzbget_rs_decoder_clear(m_decoder); }
	int DecodeBuffer(char* buffer, int len) { return nzbget_rs_decoder_decode(m_decoder, buffer, len); }
	void SetCrcCheck(bool crcCheck) { nzbget_rs_decoder_set(m_decoder, 0, crcCheck); }
	void SetRawMode(bool rawMode) { nzbget_rs_decoder_set(m_decoder, 1, rawMode); }
	EFormat GetFormat() { return static_cast<EFormat>(nzbget_rs_decoder_get(m_decoder, 0)); }
	int64 GetBeginPos() { return nzbget_rs_decoder_get(m_decoder, 1); }
	int64 GetEndPos() { return nzbget_rs_decoder_get(m_decoder, 2); }
	int64 GetSize() { return nzbget_rs_decoder_get(m_decoder, 3); }
	uint32 GetExpectedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 4)); }
	uint32 GetCalculatedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 5)); }
	bool GetEof() { return nzbget_rs_decoder_get(m_decoder, 6) != 0; }
	const char* GetArticleFilename() { return nzbget_rs_decoder_filename(m_decoder); }

private:
	struct NzbgetRsDecoder* m_decoder;
};

#endif

#ifndef PARPARSER_H
#define PARPARSER_H

class ParParser
{
public:
	typedef std::vector<std::string> ParFileList;

	static bool FindMainPars(const char* path, ParFileList* fileList)
	{
		if (fileList)
		{
			fileList->clear();
		}
		return nzbget_rs_find_main_pars(
			path,
			fileList != nullptr ? 1 : 0,
			fileList ? +[](void* ctx, const char* name, size_t len)
			{
				static_cast<ParFileList*>(ctx)->emplace_back(name, len);
			} : nullptr,
			fileList) != 0;
	}

	static bool ParseParFilename(const char* parFilename, bool confirmedFilename, int* baseNameLen, int* blocks)
	{
		return nzbget_rs_parse_par_filename(parFilename, confirmedFilename, baseNameLen, blocks) != 0;
	}

	static bool SameParCollection(const char* filename1, const char* filename2, bool confirmedFilenames)
	{
		return nzbget_rs_same_par_collection(filename1, filename2, confirmedFilenames) != 0;
	}

	static constexpr int64 MaxIndexParSize = 16 * 1024 * 1024;

	static bool ListParredFiles(const char* parFilename, std::vector<std::string>& filenames)
	{
		return nzbget_rs_par_renamer_list_parred_files(
			parFilename,
			MaxIndexParSize,
			&filenames,
			+[](void* rawCtx, const char* name) noexcept
			{
				if (name)
				{
					static_cast<std::vector<std::string>*>(rawCtx)->emplace_back(name);
				}
			}) != 0;
	}
};

#endif

#ifndef WORKSTATE_H
#define WORKSTATE_H

class WorkState final : public Subject
{
public:
	void SetPauseDownload(bool pauseDownload) { m_pauseDownload = pauseDownload; Changed(); }
	bool GetPauseDownload() const { return m_pauseDownload; }
	void SetPausePostProcess(bool pausePostProcess) { m_pausePostProcess = pausePostProcess; Changed(); }
	bool GetPausePostProcess() const { return m_pausePostProcess; }
	void SetPauseScan(bool pauseScan) { m_pauseScan = pauseScan; Changed(); }
	bool GetPauseScan() const { return m_pauseScan; }
	void SetTempPauseDownload(bool tempPauseDownload) { m_tempPauseDownload = tempPauseDownload; Changed(); }
	bool GetTempPauseDownload() const { return m_tempPauseDownload; }
	void HoldDownloadForFleet(bool hold) { m_fleetHold += hold ? 1 : -1; Changed(); }
	bool GetDownloadHeld() const { return m_tempPauseDownload || m_fleetHold > 0; }
	void SetTempPausePostprocess(bool tempPausePostprocess) { m_tempPausePostprocess = tempPausePostprocess; Changed(); }
	bool GetTempPausePostprocess() const { return m_tempPausePostprocess; }
	void SetPauseFrontend(bool pauseFrontend) { m_pauseFrontend = pauseFrontend; Changed(); }
	bool GetPauseFrontend() const { return m_pauseFrontend; }
	void SetSpeedLimit(int speedLimit) { m_speedLimit = speedLimit; Changed(); }
	int GetSpeedLimit() const { return m_speedLimit; }
	void SetResumeTime(time_t resumeTime) { m_resumeTime = resumeTime; Changed(); }
	time_t GetResumeTime() const { return m_resumeTime; }
	void SetLocalTimeOffset(int localTimeOffset) { m_localTimeOffset = localTimeOffset; Changed(); }
	int GetLocalTimeOffset() { return m_localTimeOffset; }
	void SetQuotaReached(bool quotaReached) { m_quotaReached = quotaReached; Changed(); }
	bool GetQuotaReached() { return m_quotaReached; }
	void SetDownloading(bool downloading) { m_downloading = downloading; Changed(); }
	bool GetDownloading() { return m_downloading; }

private:
	std::atomic<time_t> m_resumeTime{0};
	std::atomic<int> m_localTimeOffset{0};
	std::atomic<int> m_speedLimit{0};
	std::atomic<bool> m_tempPauseDownload{true};
	std::atomic<int> m_fleetHold{0};
	std::atomic<bool> m_tempPausePostprocess{true};
	std::atomic<bool> m_pauseDownload{false};
	std::atomic<bool> m_pausePostProcess{false};
	std::atomic<bool> m_pauseScan{false};
	std::atomic<bool> m_pauseFrontend{false};
	std::atomic<bool> m_downloading{false};
	std::atomic<bool> m_quotaReached{false};

	void Changed() { Notify(nullptr); }
};

extern WorkState* g_WorkState;

#endif

#ifndef NEWSSERVER_H
#define NEWSSERVER_H

class NewsServer
{
public:
	NewsServer(int id, bool active, const char* name, const char* host, int port, int ipVersion,
		const char* user, const char* pass, bool joinGroup,
		bool tls, const char* cipher, int maxConnections, int retention,
		int level, int group, bool optional, unsigned int certVerificationfLevel) :
		m_id(id), m_active(active), m_name(name), m_host(host ? host : ""), m_port(port), m_ipVersion(ipVersion),
		m_user(user ? user : ""), m_password(pass ? pass : ""), m_joinGroup(joinGroup), m_tls(tls),
		m_cipher(cipher ? cipher : ""), m_maxConnections(maxConnections), m_retention(retention),
		m_level(level), m_normLevel(level), m_group(group), m_optional(optional), m_certVerificationfLevel(certVerificationfLevel)
	{
		if (m_name.Empty())
		{
			m_name.Format("Server%i", id);
		}
	}
	int GetId() const { return m_id; }
	int GetStateId() const { return m_stateId; }
	void SetStateId(int stateId) { m_stateId = stateId; }
	bool GetActive() const { return m_active; }
	void SetActive(bool active) { m_active = active; }
	const char* GetName() const { return m_name; }
	int GetGroup() const { return m_group; }
	const char* GetHost() const { return m_host; }
	int GetPort() const { return m_port; }
	int GetIpVersion() const { return m_ipVersion; }
	const char* GetUser() const { return m_user; }
	const char* GetPassword() const { return m_password; }
	int GetMaxConnections() const { return m_maxConnections; }
	int GetLevel() const { return m_level; }
	int GetNormLevel() const { return m_normLevel; }
	void SetNormLevel(int level) { m_normLevel = level; }
	bool GetJoinGroup() const { return m_joinGroup; }
	bool GetTls() const { return m_tls; }
	const char* GetCipher() const { return m_cipher; }
	int GetRetention() const { return m_retention; }
	bool GetOptional() const { return m_optional; }
	time_t GetBlockTime() const { return m_blockTime; }
	void SetBlockTime(time_t blockTime) { m_blockTime = blockTime; }
	unsigned int GetCertVerificationLevel() const { return m_certVerificationfLevel; }

private:
	int m_id;
	int m_stateId = 0;
	bool m_active;
	CString m_name;
	CString m_host;
	int m_port;
	int m_ipVersion;
	CString m_user;
	CString m_password;
	bool m_joinGroup;
	bool m_tls;
	CString m_cipher;
	int m_maxConnections;
	int m_retention;
	int m_level;
	int m_normLevel;
	int m_group;
	bool m_optional = false;
	time_t m_blockTime = 0;
	unsigned int m_certVerificationfLevel;
};

typedef std::vector<std::unique_ptr<NewsServer>> Servers;

#endif

#ifndef SCRIPTCONFIG_H
#define SCRIPTCONFIG_H

#include <functional>

class ScriptConfig
{
public:

	class ConfigTemplate
	{
	public:
		ConfigTemplate(const char* templ)
			: m_template(templ) {}
		const char* GetTemplate() { return m_template; }

	private:
		CString m_template;
	};

	using ConfigTemplates = std::deque<ConfigTemplate>;

	void InitOptions()
	{
		InitConfigTemplates();
	}

	bool LoadConfig(Options::OptEntries* optEntries)
	{
		if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
		{
			return false;
		}
		return nzbget_rs_script_config_load(
			g_Options->GetConfigFilename(),
			optEntries,
			[](void* ctx, const char* name, const char* value)
			{
				try
				{
					static_cast<Options::OptEntries*>(ctx)->emplace_back(name, value);
				}
				catch (...)
				{
				}
			}) != 0;
	}

	bool SaveConfig(Options::OptEntries* optEntries, const char* cipherBaseline = nullptr, bool* conflict = nullptr)
	{
		if (conflict) *conflict = false;
		if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
		{
			return false;
		}
		std::vector<const char*> names;
		std::vector<const char*> values;
		names.reserve(optEntries->size());
		values.reserve(optEntries->size());
		for (const auto& entry : *optEntries)
		{
			if (entry.GetName() && entry.GetValue())
			{
				names.push_back(entry.GetName());
				values.push_back(entry.GetValue());
			}
		}
		int conflictFlag = 0;
		int saved = nzbget_rs_script_config_save(
			g_Options->GetConfigFilename(),
			names.data(),
			values.data(),
			names.size(),
			cipherBaseline,
			&conflictFlag);
		if (conflict)
		{
			*conflict = conflictFlag != 0;
		}
		return saved != 0;
	}

	static std::string CipherBaseline(const Options::OptEntries& optEntries)
	{
		std::vector<const char*> names;
		std::vector<const char*> values;
		names.reserve(optEntries.size());
		values.reserve(optEntries.size());
		for (const auto& entry : optEntries)
		{
			if (!entry.GetName() || !entry.GetValue()) continue;
			names.push_back(entry.GetName());
			values.push_back(entry.GetValue());
		}
		NzbgetRsBuf buf = nzbget_rs_sus_build_cipher_baseline(names.data(), values.data(), names.size());
		std::string token(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return token;
	}

	enum class CipherCommit { Saved, Unchanged, Conflict, Cancelled, Failed };

	bool ReadCipherSnapshot(std::string& bytes)
	{
		if (!g_Options || !g_Options->GetConfigFilename())
		{
			return false;
		}
		NzbgetRsBuf buf{};
		if (!nzbget_rs_script_config_read_cipher_snapshot(g_Options->GetConfigFilename(), &buf))
		{
			return false;
		}
		bytes.assign(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return true;
	}

	CipherCommit CommitCipherPatch(std::string_view expected, std::string_view replacement, const std::function<bool()>& cancel)
	{
		if (!g_Options || !g_Options->GetConfigFilename())
		{
			return CipherCommit::Failed;
		}
		int res = nzbget_rs_script_config_commit_cipher_patch(
			g_Options->GetConfigFilename(),
			expected.data(),
			expected.size(),
			replacement.data(),
			replacement.size(),
			const_cast<std::function<bool()>*>(&cancel),
			[](void* ctx) -> int
			{
				try
				{
					const auto* fn = static_cast<const std::function<bool()>*>(ctx);
					return (*fn)() ? 1 : 0;
				}
				catch (...)
				{
					return 1;
				}
			},
			[](void*)
			{
				try
				{
					warn("TLS cipher settings were replaced, but the configuration directory could not be synchronized");
				}
				catch (...)
				{
				}
			});
		switch (res)
		{
			case 0: return CipherCommit::Saved;
			case 1: return CipherCommit::Unchanged;
			case 2: return CipherCommit::Conflict;
			case 3: return CipherCommit::Cancelled;
			default: return CipherCommit::Failed;
		}
	}

	bool LoadConfigTemplates(ConfigTemplates* configTemplates)
	{
		CharBuffer buffer;
		if (!FileSystem::LoadFileIntoBuffer(g_Options->GetConfigTemplate(), buffer, true))
		{
			return false;
		}
		configTemplates->emplace_back(ConfigTemplate(buffer));

		return true;
	}

	ConfigTemplates* GetConfigTemplates() { return &m_configTemplates; }

private:
	ConfigTemplates m_configTemplates;

	void InitConfigTemplates()
	{
		if (!LoadConfigTemplates(&m_configTemplates))
		{
			error("Could not read configuration templates");
		}
	}
};

extern ScriptConfig* g_ScriptConfig;

#endif

#endif /* OPTIONS_H */
#endif /* LOG_H */
#endif /* UTIL_H */
