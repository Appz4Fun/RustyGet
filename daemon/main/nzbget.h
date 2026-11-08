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
