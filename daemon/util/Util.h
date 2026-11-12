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
			--m_threadCount;
		}
		return terminated;
	}

	bool IsStopped() { return m_stopped; };
	bool IsRunning() const { return m_running; }
	void SetAutoDestroy(bool autoDestroy) { m_autoDestroy = autoDestroy; }
	static int GetThreadCount() { return m_threadCount; }
	static bool IsCurrentThreadStopped() { return m_currentThread && m_currentThread->IsStopped(); }

protected:
	virtual void Run() {} // Virtual function - override in derivatives

private:
	inline static std::mutex m_threadMutex;
	inline static std::atomic<int> m_threadCount{1};
	inline static thread_local Thread* m_currentThread = nullptr;
	std::thread::native_handle_type m_threadObj = 0;
	std::atomic<bool> m_running{false};
	std::atomic<bool> m_stopped{false};
	std::atomic<bool> m_autoDestroy{false};

	void thread_handler()
	{
		++m_threadCount;
		struct CurrentGuard
		{
			CurrentGuard(Thread* t) { m_currentThread = t; }
			~CurrentGuard() { m_currentThread = nullptr; }
		} currentGuard(this);

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

	template <typename From, typename To,
		typename std::enable_if_t<std::is_integral_v<From> && std::is_integral_v<To>, bool> = true>
	static constexpr To SafeIntCast(From num) noexcept
	{
		if constexpr (std::is_unsigned_v<From> && std::is_signed_v<To>)
		{
			return static_cast<uint64_t>(num) > static_cast<uint64_t>(std::numeric_limits<To>::max())
				? 0 : static_cast<To>(num);
		}
		else if constexpr (std::is_signed_v<From> && std::is_unsigned_v<To>)
		{
			return (num < 0 || static_cast<uint64_t>(num) > static_cast<uint64_t>(std::numeric_limits<To>::max()))
				? 0 : static_cast<To>(num);
		}
		else if constexpr (std::is_signed_v<From> && std::is_signed_v<To>)
		{
			return (static_cast<int64_t>(num) < static_cast<int64_t>(std::numeric_limits<To>::min()) ||
				static_cast<int64_t>(num) > static_cast<int64_t>(std::numeric_limits<To>::max()))
				? 0 : static_cast<To>(num);
		}
		else
		{
			return static_cast<uint64_t>(num) > static_cast<uint64_t>(std::numeric_limits<To>::max())
				? 0 : static_cast<To>(num);
		}
	}

	static void SetStandByMode([[maybe_unused]] bool standBy) {}
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

	static CString FormatSize(int64 fileSize)
	{
		NzbgetRsBuf buf = nzbget_rs_format_size(fileSize);
		CString result(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return result;
	}

	static const char* VersionRevision() { return VersionRevisionBuf; }
	static inline char VersionRevisionBuf[100];

	static void Init()
	{
		snprintf(VersionRevisionBuf, sizeof(VersionRevisionBuf), "%s", VERSION);
		CurrentTicks();
	}
};

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
	struct DiskState
	{
		int64 available;
		int64 total;
	};

public:
	static char* BaseFileName(const char* filename)
	{
		return const_cast<char*>(filename) + nzbget_rs_path_position(0, filename, strlen(filename));
	}

	static bool SameFilename(const char* filename1, const char* filename2)
	{
		return !strcmp(filename1 ? filename1 : "", filename2 ? filename2 : "");
	}

	static void NormalizePathSeparators(char* path) { nzbget_rs_normalize_path_separators(path); }

	static bool LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull)
	{
		return nzbget_rs_fs_load_file_into_buffer(
			filename, addTrailingNull ? 1 : 0, &buffer,
			[](void* ctx, int sz) -> uint8_t* {
				auto* buf = static_cast<CharBuffer*>(ctx);
				buf->Reserve(sz);
				return reinterpret_cast<uint8_t*>(static_cast<char*>(*buf));
			}) != 0;
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

	static bool ForceDirectories(const char* path, CString& errmsg)
	{
		NzbgetRsBuf msg{};
		bool ok = nzbget_rs_force_directories(path, &msg) != 0;
		errmsg.Clear();
		if (!ok)
		{
			errmsg = msg.data;
		}
		nzbget_rs_free(msg);
		return ok;
	}

	static std::optional<DiskState> GetDiskState(const char* path)
	{
		int64 avail = 0, total = 0;
		if (nzbget_rs_fs_get_disk_state(path, &avail, &total))
		{
			return DiskState{ avail, total };
		}
		return std::nullopt;
	}

	static inline mode_t uMask;
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
	enum EWriteLog { wlNone, wlAppend, wlReset, wlRotate };
	enum EMessageTarget { mtNone, mtScreen, mtLog, mtBoth };
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
	enum EPostStrategy { ppSequential, ppBalanced, ppAggressive, ppRocket };
	enum EFileNaming { nfAuto, nfArticle, nfNzb };
	enum ECertVerifLevel { cvNone, cvMinimal, cvStrict, Count };
	enum ENzbDirArchiveAction { Delete, Move };

	typedef std::vector<const char*> CmdOptList;

	class Extender
	{
	public:
		virtual void AddNewsServer(int id, bool active, const char* name, const char* host,
			int port, int ipVersion, const char* user, const char* pass, bool joinGroup,
			bool tls, const char* cipher, int maxConnections, int retention,
			int level, int group, bool optional, unsigned int certVerificationfLevel) = 0;
		virtual ~Extender() = default;
		virtual void AddFeed(
			int id,
			const char* name,
			const char* url,
			int interval,
			const char* filter,
			bool backlog,
			bool pauseNzb,
			const char* category,
			int categorySource,
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
	};

	Options(const char* exeName, const char* configFilename, bool noConfig,
		CmdOptList* commandLineOptions, Extender* extender)
	{
		Init(exeName, configFilename, noConfig, commandLineOptions, false, extender);
	}

	Options(CmdOptList* commandLineOptions, Extender* extender)
	{
		Init("rustyget/rustyget", nullptr, true, commandLineOptions, true, extender);
	}

	~Options()
	{
		nzbget_rs_options_free(m_rs);
		g_Options = nullptr;
	}

	const NzbgetRsOptions* RsHandle() const { return m_rs; }
	bool GetFatalError() { return m_fatalError; }

	bool QueryCategory(const char* name, bool searchAliases, int* outUnpack, const char** outStrs) const
	{
		return nzbget_rs_options_query(
			m_rs, 0, name, searchAliases ? 1 : 0, outUnpack, outStrs, 0, nullptr) != 0;
	}

	void CreateSchedulerTask(int id, const char* time, const char* weekDays,
		ESchedulerCommand command, const char* param)
	{
		m_configErrors = nzbget_rs_options_task(
			m_rs, id, time, weekDays, static_cast<int>(command), param, this, &HostCb) != 0;
	}

	const char* GetConfigFilename() const { return Str(0); }
	bool GetConfigErrors() const { return m_configErrors; }
	const char* GetAppDir() const { return Str(1); }
	const char* GetMainDir() const { return Str(2); }
	const char* GetDestDir() const { return Str(3); }
	const char* GetInterDir() const { return Str(4); }
	const char* GetTempDir() const { return Str(5); }
	const char* GetQueueDir() const { return Str(6); }
	const char* GetNzbDir() const { return Str(7); }
	const char* GetWebDir() const { return Str(8); }
	const char* GetConfigTemplate() const { return Str(9); }
	const char* GetScriptDir() const { return Str(10); }
	const char* GetRequiredDir() const { return Str(11); }
	bool GetSystemHealthCheck() const { return m_scalars.system_health_check != 0; }
	bool GetNzbLog() const { return m_scalars.nzb_log != 0; }
	EMessageTarget GetInfoTarget() const { return (EMessageTarget)m_scalars.info_target; }
	EMessageTarget GetWarningTarget() const { return (EMessageTarget)m_scalars.warning_target; }
	EMessageTarget GetErrorTarget() const { return (EMessageTarget)m_scalars.error_target; }
	EMessageTarget GetDebugTarget() const { return (EMessageTarget)m_scalars.debug_target; }
	EMessageTarget GetDetailTarget() const { return (EMessageTarget)m_scalars.detail_target; }
	int GetArticleTimeout() const { return m_scalars.article_timeout; }
	int GetArticleReadChunkSize() const { return m_scalars.article_read_chunk_size; }
	int GetUrlTimeout() const { return m_scalars.url_timeout; }
	int GetRemoteTimeout() const { return m_scalars.remote_timeout; }
	bool GetRawArticle() const { return m_scalars.raw_article != 0; }
	bool GetSkipWrite() const { return m_scalars.skip_write != 0; }
	bool GetAppendCategoryDir() const { return m_scalars.append_category_dir != 0; }
	bool GetContinuePartial() const { return m_scalars.continue_partial != 0; }
	int GetArticleRetries() const { return m_scalars.article_retries; }
	int GetArticleInterval() const { return m_scalars.article_interval; }
	int GetUrlRetries() const { return m_scalars.url_retries; }
	int GetUrlInterval() const { return m_scalars.url_interval; }
	bool GetFlushQueue() const { return m_scalars.flush_queue != 0; }
	bool GetDupeCheck() const { return m_scalars.dupe_check != 0; }
	EDupeArticleFallback GetDupeArticleFallback() const { return (EDupeArticleFallback)m_scalars.dupe_article_fallback; }
	bool GetDupeStreamDecompress() const { return m_scalars.dupe_stream_decompress != 0; }
	int GetDupeStreamTimeout() const { return m_scalars.dupe_stream_timeout; }
	bool GetDupeSearch() const { return m_scalars.dupe_search != 0; }
	bool GetDupeSearchDryRun() const { return m_scalars.dupe_search_dry_run != 0; }
	const char* GetDupeSearchUrl() const { return Str(12); }
	const char* GetDupeSearchApiKey() const { return Str(13); }
	int GetDupeSearchDelay() const { return m_scalars.dupe_search_delay; }
	int GetDupeSearchMaxDonors() const { return m_scalars.dupe_search_max_donors; }
	int GetDupeHealthPercent() const { return m_scalars.dupe_health_percent; }
	int GetDupeHealthMin() const { return m_scalars.dupe_health_min; }
	int GetDupeHealthMax() const { return m_scalars.dupe_health_max; }
	int GetDupeBodyChecks() const { return m_scalars.dupe_body_checks; }
	int GetDupeHealthBudget() const { return m_scalars.dupe_health_budget; }
	int GetDupeMinAlive() const { return m_scalars.dupe_min_alive; }
	int GetDupeFastDonors() const { return m_scalars.dupe_fast_donors; }
	const char* GetControlIp() const { return Str(14); }
	const char* GetControlUsername() const { return Str(15); }
	const char* GetControlPassword() const { return Str(16); }
	const char* GetRestrictedUsername() const { return Str(17); }
	const char* GetRestrictedPassword() const { return Str(18); }
	const char* GetAddUsername() const { return Str(19); }
	const char* GetAddPassword() const { return Str(20); }
	int GetControlPort() const { return m_scalars.control_port; }
	bool GetFormAuth() const { return m_scalars.form_auth != 0; }
	bool GetJsonP() const { return m_scalars.json_p != 0; }
	bool GetSecureControl() const { return m_scalars.secure_control != 0; }
	int GetSecurePort() const { return m_scalars.secure_port; }
	const char* GetSecureCert() const { return Str(21); }
	const char* GetSecureKey() const { return Str(22); }
	const char* GetCertStore() const { return Str(23); }
	bool GetCertCheck() const { return m_scalars.cert_check != 0; }
	const char* GetAuthorizedIp() const { return Str(24); }
	const char* GetLockFile() const { return Str(25); }
	const char* GetDaemonUsername() const { return Str(26); }
	EOutputMode GetOutputMode() const { return (EOutputMode)m_scalars.output_mode; }
	int GetUrlConnections() const { return m_scalars.url_connections; }
	int GetLogBuffer() const { return m_scalars.log_buffer; }
	EWriteLog GetWriteLog() const { return (EWriteLog)m_scalars.write_log; }
	const char* GetLogFile() const { return Str(27); }
	int GetRotateLog() const { return m_scalars.rotate_log; }
	EParCheck GetParCheck() const { return (EParCheck)m_scalars.par_check; }
	bool GetParRepair() const { return m_scalars.par_repair != 0; }
	EParScan GetParScan() const { return (EParScan)m_scalars.par_scan; }
	bool GetParQuick() const { return m_scalars.par_quick != 0; }
	EPostStrategy GetPostStrategy() const { return (EPostStrategy)m_scalars.post_strategy; }
	bool GetParRename() const { return m_scalars.par_rename != 0; }
	int GetParBuffer() const { return m_scalars.par_buffer; }
	int GetParThreads() const { return m_scalars.par_threads; }
	bool GetRarRename() const { return m_scalars.rar_rename != 0; }
	EHealthCheck GetHealthCheck() const { return (EHealthCheck)m_scalars.health_check; }
	const char* GetScriptOrder() const { return Str(28); }
	const char* GetExtensions() const { return Str(29); }
	int GetUMask() const { return m_scalars.umask; }
	int GetUpdateInterval() const { return m_scalars.update_interval; }
	bool GetCursesNzbName() const { return m_scalars.curses_nzb_name != 0; }
	bool GetCursesTime() const { return m_scalars.curses_time != 0; }
	bool GetCursesGroup() const { return m_scalars.curses_group != 0; }
	bool GetCrcCheck() const { return m_scalars.crc_check != 0; }
	bool GetDirectWrite() const { return m_scalars.direct_write != 0; }
	int GetWriteBuffer() const { return m_scalars.write_buffer; }
	int GetNzbDirInterval() const { return m_scalars.nzb_dir_interval; }
	int GetNzbDirFileAge() const { return m_scalars.nzb_dir_file_age; }
	bool GetNzbDirArchiveScan() const { return m_scalars.nzb_dir_archive_scan != 0; }
	ENzbDirArchiveAction GetNzbDirArchiveAction() const { return (ENzbDirArchiveAction)m_scalars.nzb_dir_archive_action; }
	int GetDiskSpace() const { return m_scalars.disk_space; }
	bool GetCrashTrace() const { return m_scalars.crash_trace != 0; }
	bool GetCrashDump() const { return m_scalars.crash_dump != 0; }
	bool GetParPauseQueue() const { return m_scalars.par_pause_queue != 0; }
	bool GetScriptPauseQueue() const { return m_scalars.script_pause_queue != 0; }
	bool GetNzbCleanupDisk() const { return m_scalars.nzb_cleanup_disk != 0; }
	int GetParTimeLimit() const { return m_scalars.par_time_limit; }
	int GetKeepHistory() const { return m_scalars.keep_history; }
	bool GetUnpack() const { return m_scalars.unpack != 0; }
	bool GetDirectUnpack() const { return m_scalars.direct_unpack != 0; }
	bool GetUseTempUnpackDir() const { return m_scalars.use_temp_unpack_dir != 0; }
	bool GetUnpackCleanupDisk() const { return m_scalars.unpack_cleanup_disk != 0; }
	const char* GetUnrarCmd() const { return Str(30); }
	const char* GetUnpackPassFile() const { return Str(31); }
	bool GetUnpackPauseQueue() const { return m_scalars.unpack_pause_queue != 0; }
	const char* GetExtCleanupDisk() const { return Str(32); }
	const char* GetParIgnoreExt() const { return Str(33); }
	bool GetPostDownloadRename() const { return m_scalars.post_download_rename != 0; }
	bool GetRenameAfterUnpack() const { return m_scalars.rename_after_unpack != 0; }
	const char* GetRenameIgnoreExt() const { return Str(34); }
	const char* GetUnpackIgnoreExt() const { return Str(35); }
	int GetFeedHistory() const { return m_scalars.feed_history; }
	bool GetUrlForce() const { return m_scalars.url_force != 0; }
	int GetTimeCorrection() const { return m_scalars.time_correction; }
	int GetPropagationDelay() const { return m_scalars.propagation_delay; }
	int GetArticleCache() const { return m_scalars.article_cache; }
	int GetEventInterval() const { return m_scalars.event_interval; }
	int GetMemoryPressureInterval() const { return m_scalars.memory_pressure_interval; }
	const char* GetMemoryPressurePath() const { return Str(36); }
	const char* GetShellOverride() const { const char* s = Str(40); return s ? s : ""; }
	int GetMonthlyQuota() const { return m_scalars.monthly_quota; }
	int GetQuotaStartDay() const { return m_scalars.quota_start_day; }
	int GetDailyQuota() const { return m_scalars.daily_quota; }
	bool GetDirectRename() const { return m_scalars.direct_rename != 0; }
	bool GetHardLinking() const { return m_scalars.hard_linking != 0; }
	const char* GetHardLinkingIgnoreExt() const { return Str(37); }
	bool GetReorderFiles() const { return m_scalars.reorder_files != 0; }
	EFileNaming GetFileNaming() const { return (EFileNaming)m_scalars.file_naming; }
	int GetDownloadRate() const { return m_scalars.download_rate; }

	void SetServerMode(bool serverMode) { m_serverMode = serverMode; }
	bool GetServerMode() const { return m_serverMode; }
	void SetDaemonMode(bool daemonMode) { m_daemonMode = daemonMode; }
	bool GetDaemonMode() const { return m_daemonMode; }
	void SetRemoteClientMode(bool remoteClientMode) { m_remoteClientMode = remoteClientMode; }
	bool GetRemoteClientMode() const { return m_remoteClientMode; }
	void SetDirectClientMode(bool directClientMode) { m_directClientMode = directClientMode; }
	bool GetDirectClientMode() const { return m_directClientMode; }

	const char* GetUnrarPath() const { const char* s = Str(38); return s ? s : ""; }
	const char* GetSevenZipPath() const { const char* s = Str(39); return s ? s : ""; }

private:
	NzbgetRsOptions* m_rs = nullptr;
	Extender* m_extender = nullptr;
	NzbgetRsParsedOptions m_scalars{};
	bool m_fatalError = false;
	bool m_configErrors = false;
	bool m_serverMode = false;
	bool m_daemonMode = false;
	bool m_remoteClientMode = false;
	bool m_directClientMode = false;

	const char* Str(int id) const { return nzbget_rs_options_str(m_rs, id); }

	void Init(const char* exeName, const char* configFilename, bool noConfig,
		CmdOptList* commandLineOptions, bool noDiskAccess, Extender* extender)
	{
		g_Options = this;
		m_extender = extender;
		int flags = 0;
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
			exeName, configFilename, noConfig ? 1 : 0, noDiskAccess ? 1 : 0,
			commandLineOptions && !commandLineOptions->empty() ? commandLineOptions->data() : nullptr,
			commandLineOptions ? commandLineOptions->size() : 0,
			flags, Util::VersionRevision(), this, &HostCb, &m_scalars, &stateFlags);
		m_fatalError = (stateFlags & 1) != 0;
		m_configErrors = (stateFlags & 2) != 0;
	}

	static void HostCb(void* ctx, int op, const int* ints, const char* const* strs, size_t)
	{
		auto* self = static_cast<Options*>(ctx);
		if (op == 0) { printf("%s\n", strs[0]); return; }
		if (op == 1) { printf("%s\n", strs[0]); error("%s", strs[0]); return; }
		if (op == 2) { printf("%s\n", strs[0]); warn("%s", strs[0]); return; }
		if (op == 3) { warn("%s", strs[0]); return; }
		if (!self->m_extender) return;
		if (op == 7)
		{
			self->m_extender->AddNewsServer(ints[0], ints[1] != 0, strs[0], strs[1], ints[2], ints[3],
				strs[2], strs[3], ints[4] != 0, ints[5] != 0, strs[4], ints[6], ints[7],
				ints[8], ints[9], ints[10] != 0, static_cast<unsigned int>(ints[11]));
		}
		else if (op == 8)
		{
			self->m_extender->AddFeed(ints[0], strs[0], strs[1], ints[1], strs[2], ints[2] != 0,
				ints[3] != 0, strs[3], ints[4], ints[5], strs[4], static_cast<unsigned int>(ints[6]));
		}
		else if (op == 9)
		{
			self->m_extender->AddTask(ints[0], ints[1], ints[2], ints[3],
				static_cast<ESchedulerCommand>(ints[4]), strs[0]);
		}
	}
};

#endif /* OPTIONS_H */

class Debuggable;
class Log;
extern Log* g_Log;

class Log
{
public:
	Log()
	{
		g_Log = this;
#ifdef DEBUG
		m_rs = nzbget_rs_log_new(1);
#else
		m_rs = nzbget_rs_log_new(0);
#endif
	}

	~Log()
	{
		nzbget_rs_log_free(m_rs);
		g_Log = nullptr;
	}

	const NzbgetRsLog* RsHandle() const { return m_rs; }

	void Clear() { nzbget_rs_log_ctl(m_rs, 0, 0, 0, OptRs(), nullptr, nullptr); }

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

	void LogDebugInfo();

	void IntervalCheck()
	{
		nzbget_rs_log_ctl(m_rs, 2, 0, 0, OptRs(), nullptr, nullptr);
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

inline void Log::LogDebugInfo()
{
	nzbget_rs_log_ctl(m_rs, 7, 0, 0, OptRs(), nullptr,
		+[](uintptr_t p) { reinterpret_cast<Debuggable*>(p)->LogDebugInfo(); });
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

private:
	time_t m_lastWork = 0;

	friend class ServiceCoordinator;
};

class ServiceCoordinator : public Thread
{
public:
	using ServiceList = std::vector<Service*>;

	ServiceCoordinator() { debug("Creating ServiceCoordinator"); }
	~ServiceCoordinator() override { debug("Destroying ServiceCoordinator"); }

	void Run() override
	{
		debug("Entering ServiceCoordinator-loop");

		while (!IsStopped())
		{
			for (Service* service : m_services)
			{
				int serviceInterval = service->ServiceInterval();
				if (serviceInterval >= Service::Now &&
					std::abs(Util::CurrentTime() - service->m_lastWork) >= serviceInterval)
				{
					service->ServiceWork();
					service->m_lastWork = Util::CurrentTime();
				}
			}

			int waitInterval = 60 * 60 * 24;
			time_t curTime = Util::CurrentTime();
			for (Service* service : m_services)
			{
				int serviceInterval = service->ServiceInterval();
				int remaining = Service::Sleep;
				if (serviceInterval >= Service::Now)
				{
					int elapsed = static_cast<int>(curTime - service->m_lastWork);
					remaining = std::min(serviceInterval, serviceInterval - elapsed);
					waitInterval = std::min(waitInterval, remaining);
				}
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

		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_waitCond.notify_all();
	}

private:
	ServiceList m_services;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	bool m_workenUp = false;

	void RegisterService(Service* service) { m_services.push_back(service); }

	void WakeUp()
	{
		debug("Waking up ServiceCoordinator");
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_workenUp = true;
		m_waitCond.notify_all();
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

			if (suppressable && m_suppressSlot != 0 &&
				nzbget_rs_conn_ctl(nullptr, 23, 0, m_suppressSlot, nullptr, nullptr, nullptr, 0) != 0)
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


class Connection
{
public:
	enum EStatus
	{
		csConnected,
		csDisconnected,
		csListening,
		csCancelled,
		csBroken
	};

	enum EIPVersion
	{
		ipAuto = 0,
		ipV4 = 4,
		ipV6 = 6
	};

	static void Init() { debug("Initializing global connection data"); }
	static void Final() {}

	Connection(const char* host, int port, bool tls, bool registerCancel = true) :
		m_rs(nzbget_rs_conn_new(host ? host : "", port, tls ? 1 : 0, registerCancel ? 1 : 0, RsDbg(), this, &HostCb)) {}

	Connection(SOCKET socket, bool tls) :
		m_rs(nzbget_rs_conn_from_fd(socket, tls ? 1 : 0, 60, RsDbg(), this, &HostCb)) {}

	explicit Connection(NzbgetRsConn* rs) : m_rs(rs)
	{
		nzbget_rs_conn_ctl(m_rs, 22, 0, reinterpret_cast<uintptr_t>(this), nullptr, nullptr, nullptr, 0);
	}

	virtual ~Connection() { nzbget_rs_conn_free(m_rs); }

	NzbgetRsConn* Handle() const { return m_rs; }
	void SetSuppressErrors(bool s) { Ctl(5, s ? 1 : 0); }
	virtual bool Connect() { return Ctl(0) != 0; }
	virtual bool Disconnect() { return Ctl(1) != 0; }
	bool Bind() { return Ctl(2) != 0; }
	int WriteLine(const char* buffer) { return nzbget_rs_conn_io(m_rs, 0, const_cast<char*>(buffer), 0, nullptr); }
	bool Send(const char* buffer, int size) { return nzbget_rs_conn_io(m_rs, 1, const_cast<char*>(buffer), size, nullptr) != 0; }
	char* ReadLine(char* buffer, int size, int* bytesReadOut)
	{
		return nzbget_rs_conn_io(m_rs, 2, buffer, size, bytesReadOut) != 0 ? buffer : nullptr;
	}
	std::unique_ptr<Connection> Accept()
	{
		NzbgetRsConn* acc = nzbget_rs_conn_accept(m_rs);
		return acc ? std::make_unique<Connection>(acc) : nullptr;
	}
	int TryRecv(char* buffer, int size) { return nzbget_rs_conn_io(m_rs, 4, buffer, size, nullptr); }
	bool Recv(char* buffer, int size) { return nzbget_rs_conn_io(m_rs, 3, buffer, size, nullptr) != 0; }
	const char* GetHost() const { return reinterpret_cast<const char*>(Ctl(19)); }
	void SetCipher(const char* cipher) { nzbget_rs_conn_ctl(m_rs, 7, 0, 0, cipher, nullptr, nullptr, 0); }
	void SetTimeout(int timeout) { Ctl(6, timeout); }
	void SetResolvedAddresses(const std::vector<std::string>& addresses)
	{
		std::vector<const char*> ptrs;
		ptrs.reserve(addresses.size());
		for (const auto& s : addresses) ptrs.push_back(s.c_str());
		nzbget_rs_conn_ctl(m_rs, 13, 0, 0, nullptr, nullptr, ptrs.empty() ? nullptr : ptrs.data(), ptrs.size());
	}
	void SetDeadline(std::shared_ptr<const SocketDeadline> deadline)
	{
		m_deadline = std::move(deadline);
		nzbget_rs_conn_ctl(m_rs, 12, 0, m_deadline ? reinterpret_cast<uintptr_t>(&m_deadline) : 0, nullptr, nullptr, nullptr, 0);
	}
#ifndef DISABLE_TLS
	const char* GetNegotiatedCipher() const { return reinterpret_cast<const char*>(Ctl(21)); }
	int GetNegotiatedVersion() const { return static_cast<int>(Ctl(18)); }
	int GetTlsProbeFailure() const { return static_cast<int>(Ctl(17)); }
	void SetCertVerifLevel(unsigned int level) { Ctl(9, static_cast<int>(level)); }
	bool StartTls(bool isClient, const char* certFile, const char* keyFile)
	{
		return nzbget_rs_conn_ctl(m_rs, 4, isClient ? 1 : 0, 0, certFile, keyFile, nullptr, 0) != 0;
	}
#endif
	void SetIPVersion(EIPVersion ipVersion) { Ctl(8, static_cast<int>(ipVersion)); }
	EStatus GetStatus() { return static_cast<EStatus>(Ctl(14)); }
	bool GetAcceptBusy() { return Ctl(15) != 0; }
	void SetBroken() { Ctl(11); }
	void SetGracefull(bool gracefull) { Ctl(10, gracefull ? 1 : 0); }
	const char* GetRemoteAddr() { return reinterpret_cast<const char*>(Ctl(20)); }
	int FetchTotalBytesRead() { return static_cast<int>(Ctl(16)); }
	void Cancel() { Ctl(3); }
	void ReadBuffer(char** buffer, int* bufLen) { nzbget_rs_conn_io(m_rs, 5, reinterpret_cast<char*>(buffer), 0, bufLen); }

	static int RsDbg()
	{
#ifdef DEBUG
		return 1;
#else
		return 0;
#endif
	}

	static intptr_t HostCb(void* ctx, int op, int a1, int a2, uintptr_t p1,
		const char* s1, const char* s2, const char* s3, char* buf, int* outInt)
	{
		if (op == 0) { debug("%s", s1); return 0; }
		if (op == 1)
		{
			if (a1 != 0) debug("%s", s1);
			else if (ctx) static_cast<Connection*>(ctx)->PrintError(s1);
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

protected:
	std::shared_ptr<const SocketDeadline> m_deadline;
	NzbgetRsConn* m_rs = nullptr;

	intptr_t Ctl(int op, int a1 = 0) const
	{
		return nzbget_rs_conn_ctl(m_rs, op, a1, 0, nullptr, nullptr, nullptr, 0);
	}

	virtual void PrintError(const char* errMsg) { error("%s", errMsg); }
};

#ifndef SYSTEM_INFO_H
#define SYSTEM_INFO_H

#include <ostream>

namespace System
{
	class SystemInfo final
	{
	public:
		SystemInfo()
		{
#ifndef DISABLE_TLS
#ifdef LIBRESSL_VERSION_TEXT
			m_sslLibName = "LibreSSL";
			m_sslRawVersion = LIBRESSL_VERSION_TEXT;
			m_sslExtractAfterSpace = true;
#elif defined(OPENSSL_FULL_VERSION_STR)
			m_sslLibName = "OpenSSL";
			m_sslRawVersion = OPENSSL_FULL_VERSION_STR;
			m_sslExtractAfterSpace = false;
#else
			m_sslLibName = "OpenSSL";
			m_sslRawVersion = OPENSSL_VERSION_TEXT;
			m_sslExtractAfterSpace = true;
#endif
#endif
		}

		NzbgetRsBuf FormatBuf(int mode, bool withNetwork) const
		{
#ifndef DISABLE_TLS
			int useTls = 1;
#else
			int useTls = 0;
#endif
#ifndef DISABLE_GZIP
			int includeGzip = 1;
#else
			int includeGzip = 0;
#endif
			return nzbget_rs_system_info_format(
				mode, withNetwork ? 1 : 0, useTls, includeGzip, Util::VersionRevision(),
				m_sslLibName, m_sslRawVersion, m_sslExtractAfterSpace ? 1 : 0,
				&Connection::HostCb, g_Log ? g_Log->RsHandle() : nullptr,
				g_Options ? g_Options->RsHandle() : nullptr);
		}

		friend std::ostream& operator<<(std::ostream& os, const SystemInfo& sysinfo)
		{
			NzbgetRsBuf buf = sysinfo.FormatBuf(0, false);
			if (buf.data && buf.len > 0) os.write(buf.data, static_cast<std::streamsize>(buf.len));
			nzbget_rs_free(buf);
			return os;
		}

	private:
		const char* m_sslLibName = "";
		const char* m_sslRawVersion = "";
		bool m_sslExtractAfterSpace = false;
	};
}

extern System::SystemInfo* g_SystemInfo;

#endif

class WebDownloader : public Thread, public Subject
{
public:
	enum EStatus
	{
		adUndefined,
		adRunning,
		adFinished,
		adFailed,
		adRetry,
		adNotFound,
		adRedirect,
		adConnectError,
		adFatalError
	};

	WebDownloader() : m_rs(nzbget_rs_webdl_new(Connection::RsDbg(), &Connection::HostCb, this, &HostCb)) {}
	~WebDownloader() override { nzbget_rs_webdl_free(m_rs); }
	EStatus GetStatus() { return static_cast<EStatus>(nzbget_rs_webdl_ctl(m_rs, 3, 0, nullptr, nullptr, nullptr)); }
	void Run() override { nzbget_rs_webdl_ctl(m_rs, 1, 0, nullptr, nullptr, nullptr); }
	void Stop() override { Thread::Stop(); nzbget_rs_webdl_ctl(m_rs, 2, 0, nullptr, nullptr, nullptr); }
	EStatus DownloadWithRedirects(int maxRedirects)
	{
		return static_cast<EStatus>(nzbget_rs_webdl_ctl(m_rs, 0, maxRedirects, nullptr, nullptr, nullptr));
	}
	void SetInfoName(const char* infoName) { nzbget_rs_webdl_ctl(m_rs, 11, 0, infoName ? infoName : "", nullptr, nullptr); }
	const char* GetInfoName() { return reinterpret_cast<const char*>(nzbget_rs_webdl_ctl(m_rs, 11, 0, nullptr, nullptr, nullptr)); }
	void SetUrl(const char* url) { nzbget_rs_webdl_ctl(m_rs, 10, 0, url ? url : "", nullptr, nullptr); }
	const char* GetOutputFilename() { return reinterpret_cast<const char*>(nzbget_rs_webdl_ctl(m_rs, 12, 0, nullptr, nullptr, nullptr)); }
	void SetOutputFilename(const char* outputFilename) { nzbget_rs_webdl_ctl(m_rs, 12, 0, outputFilename ? outputFilename : "", nullptr, nullptr); }
	time_t GetLastUpdateTime()
	{
		int64_t t = 0;
		nzbget_rs_webdl_ctl(m_rs, 5, 0, nullptr, &t, nullptr);
		return static_cast<time_t>(t);
	}
	void SetLastUpdateTimeNow() { nzbget_rs_webdl_ctl(m_rs, 6, 0, nullptr, nullptr, nullptr); }
	int GetHttpStatus() const { return static_cast<int>(nzbget_rs_webdl_ctl(m_rs, 4, 0, nullptr, nullptr, nullptr)); }
	const char* GetOriginalFilename()
	{
		NzbgetRsBuf buf{};
		nzbget_rs_webdl_ctl(m_rs, 13, 0, nullptr, nullptr, &buf);
		m_originalFilename.Set(buf.data ? buf.data : "", static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return m_originalFilename;
	}
	void SetForce(bool force) { nzbget_rs_webdl_ctl(m_rs, 7, force ? 1 : 0, nullptr, nullptr, nullptr); }
	void SetRetry(bool retry) { nzbget_rs_webdl_ctl(m_rs, 8, retry ? 1 : 0, nullptr, nullptr, nullptr); }
#ifndef DISABLE_TLS
	void SetCertVerifLevel(unsigned int level) { nzbget_rs_webdl_ctl(m_rs, 9, static_cast<int>(level), nullptr, nullptr, nullptr); }
#endif
	void LogDebugInfo() { nzbget_rs_webdl_ctl(m_rs, 14, 0, nullptr, nullptr, nullptr); }

	static int HostCb(void* ctx, int op, int a1, const char* s1, int* outInts, const char** outStr)
	{
		auto* self = static_cast<WebDownloader*>(ctx);
		if (op == 0)
		{
			outInts[0] = g_Options ? g_Options->GetWriteBuffer() : 0;
			outInts[1] = g_Options ? g_Options->GetUrlRetries() : 1;
			outInts[2] = g_Options ? g_Options->GetUrlInterval() : 0;
			outInts[3] = g_Options ? g_Options->GetUrlTimeout() : 60;
#ifdef DISABLE_TLS
			outInts[4] = 1;
#else
			outInts[4] = 0;
#endif
#ifdef DISABLE_GZIP
			outInts[5] = 1;
#else
			outInts[5] = 0;
#endif
			*outStr = Util::VersionRevision();
			return 0;
		}
		if (op == 1) return ((self && self->IsStopped()) || Thread::IsCurrentThreadStopped()) ? 1 : 0;
		if (op == 2) return (g_WorkState && g_WorkState->GetPauseDownload()) ? 1 : 0;
		if (op == 3) { if (self) self->Notify(nullptr); return 0; }
		if (op == 4) { if (self) self->ProcessHeader(s1); return 0; }
		if (op == 5)
		{
			if (a1 == 0) debug("%s", s1);
			else if (a1 == 1) detail("%s", s1);
			else if (a1 == 2) warn("%s", s1);
			else if (a1 == 3) error("%s", s1);
			else info("%s", s1);
		}
		return 0;
	}

protected:
	virtual void ProcessHeader([[maybe_unused]] const char* line) {}

private:
	NzbgetRsWebDl* m_rs = nullptr;
	CString m_originalFilename;
};

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

protected:
	DownloadQueue() {}
	// a reload creates the queue anew: until it is loaded again, threads
	// waiting for IsLoaded() (the post-processor's queue sanitising, for
	// one) must not take the previous queue's state for it
	static void Init(DownloadQueue* globalInstance) { g_DownloadQueue = globalInstance; g_Loaded = false; }
	static void Final() { g_DownloadQueue = nullptr; }
	static void Loaded() { g_Loaded = true; }

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

	bool LoadDownloadQueue(DownloadQueue* downloadQueue, Servers* servers)
	{
		ExecCtx ctx;
		ctx.queue = downloadQueue;
		ctx.servers = servers;
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

	bool LoadFileState(FileInfo* fileInfo, Servers* servers, bool completed)
	{
		ExecCtx ctx;
		ctx.targetFile = fileInfo;
		ctx.servers = servers;
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

	void WriteCacheFlag()
	{
		ExecCtx ctx;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		nzbget_rs_diskstate_exec(4, &env);
	}

	void DeleteCacheFlag()
	{
		ExecCtx ctx;
		NzbgetRsDiskStateEnv env = MakeEnv(&ctx);
		nzbget_rs_diskstate_exec(3, &env);
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
		Servers* servers = nullptr;
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

	static void ApplyServerStats(ServerStatList* list, Servers* servers, const NzbgetRsDiskServerStat* stats, size_t len)
	{
		if (!servers) return;
		for (size_t i = 0; i < len; i++)
		{
			for (NewsServer* ns : servers)
			{
				if (ns->GetStateId() == stats[i].server_id)
				{
					list->StatOp(ns->GetId(), stats[i].success_articles, stats[i].failed_articles, ServerStatList::soSet);
				}
			}
		}
	}

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
		ApplyServerStats(n->GetServerStats(), c->servers, stats, statsLen);
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
			ApplyServerStats(f->GetServerStats(), c->servers, stats, statsLen);
			f->SetCompletedArticles(extraI32);
			if (idxOrFlags & 16) f->SetOutputInitialized((bool)fstate->output_initialized);
			if (strArg) f->SetOutputFilename(strArg);
		}
		return 0;
	}

	static void AuxCb(void* rawCtx, int op, int, int, uint32_t, const char*, const char*,
		const void*, void* sinkCtx,
		void (*sinkFn)(void*, int, int, int, uint32_t, const char*, const NzbgetRsServerMatchItem*, size_t, const void*, size_t))
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

#ifndef NNTPCONNECTION_H
#define NNTPCONNECTION_H

class NntpConnection : public Connection
{
public:
	NntpConnection(NewsServer* newsServer, bool registerCancel = true) :
		Connection(newsServer->GetHost(), newsServer->GetPort(), newsServer->GetTls(), registerCancel), m_newsServer(newsServer)
	{
		SetCipher(newsServer->GetCipher());
		SetIPVersion(newsServer->GetIpVersion() == 4 ? Connection::ipV4 :
			(newsServer->GetIpVersion() == 6 ? Connection::ipV6 : Connection::ipAuto));
#ifndef DISABLE_TLS
		SetCertVerifLevel(newsServer->GetCertVerificationLevel());
#endif
	}

	virtual bool Connect() override { return CallNntp(0, nullptr) != nullptr; }
	virtual bool Disconnect() override { return CallNntp(1, nullptr) != nullptr; }
	NewsServer* GetNewsServer() { return m_newsServer; }
	const char* Request(const char* req) { return CallNntp(2, req); }
	const char* JoinGroup(const char* grp) { return CallNntp(3, grp); }
	bool GetAuthError() { return CallNntp(4, nullptr) != nullptr; }
	// the server refused the login for good (502), not for now (481: too many connections)
	bool GetAuthRejected() { return CallNntp(5, nullptr) != nullptr; }

private:
	NewsServer* m_newsServer;

	const char* CallNntp(int op, const char* arg)
	{
		return nzbget_rs_conn_nntp(m_rs, op, arg,
			m_newsServer->GetUser(), m_newsServer->GetPassword(),
			m_newsServer->GetName(), m_newsServer->GetHost());
	}
};

#endif

inline uintptr_t NzbgetCArticleDlHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

class ServerPool : public Debuggable
{
public:
	typedef std::vector<NewsServer*> RawServerList;

	void SetTimeout(int timeout) { m_timeout = timeout; }
	void SetRetryInterval(int retryInterval) { m_retryInterval = retryInterval; }

	void AddServer(std::unique_ptr<NewsServer> newsServer)
	{
		debug("Adding server to ServerPool");

		m_servers.push_back(std::move(newsServer));
		m_sortedServers.push_back(m_servers.back().get());
	}

	void InitConnections()
	{
		debug("Initializing connections in ServerPool");
		Guard guard(m_connectionsMutex);
		nzbget_rs_article_dl_exec(nullptr, 120, 0, 0, 0, 0, nullptr, NzbgetCArticleDlHost, nullptr);
	}

	int GetMaxNormLevel() { return m_maxNormLevel; }
	Servers* GetServers() { return &m_servers; }

	NewsServer* GetServerById(int id)
	{
		auto serverIt = std::find_if(
			begin(m_servers),
			end(m_servers),
			[&](const std::unique_ptr<NewsServer>& server)
			{
				return server->GetId() == id;
			}
		);

		if (serverIt != end(m_servers))
		{
			return serverIt->get();
		}

		return nullptr;
	}

	NntpConnection* GetConnection(int level, NewsServer* wantServer, RawServerList* ignoreServers)
	{
		Guard guard(m_connectionsMutex);
		return reinterpret_cast<NntpConnection*>(nzbget_rs_article_dl_exec(
			nullptr, 121, level, 0, reinterpret_cast<uintptr_t>(wantServer),
			reinterpret_cast<uintptr_t>(ignoreServers), nullptr, NzbgetCArticleDlHost, nullptr));
	}

	void FreeConnection(NntpConnection* connection, bool used)
	{
		if (used)
		{
			debug("Freeing used connection");
		}
		Guard guard(m_connectionsMutex);
		nzbget_rs_article_dl_exec(nullptr, 122, 0, 0, reinterpret_cast<uintptr_t>(connection),
			used ? 1 : 0, nullptr, NzbgetCArticleDlHost, nullptr);
	}

	void CloseUnusedConnections()
	{
		Guard guard(m_connectionsMutex);
		nzbget_rs_article_dl_exec(nullptr, 123, 0, 0, 0, 0, nullptr, NzbgetCArticleDlHost, nullptr);
	}

	void Changed()
	{
		debug("Server config has been changed");

		InitConnections();
		CloseUnusedConnections();
	}

	int GetGeneration() { return m_generation; }

	void BlockServer(NewsServer* newsServer)
	{
		nzbget_rs_article_dl_exec(nullptr, 124, 0, 0, reinterpret_cast<uintptr_t>(newsServer),
			0, nullptr, NzbgetCArticleDlHost, nullptr);
	}

	bool IsServerBlocked(NewsServer* newsServer)
	{
		return nzbget_rs_article_dl_exec(nullptr, 125, m_retryInterval, 0,
			(uintptr_t)(int64)newsServer->GetBlockTime(), (uintptr_t)(int64)Util::CurrentTime(),
			nullptr, nullptr, nullptr) != 0;
	}

protected:
	virtual void LogDebugInfo()
	{
		Guard guard(m_connectionsMutex);
		nzbget_rs_article_dl_exec(nullptr, 126, 0, 0, 0, 0, nullptr, NzbgetCArticleDlHost, nullptr);
	}

private:
	class PooledConnection : public NntpConnection
	{
	public:
		explicit PooledConnection(NewsServer* newsServer) : NntpConnection(newsServer, false) {}
		bool GetInUse() { return m_inUse; }
		void SetInUse(bool inUse) { m_inUse = inUse; }
		time_t GetFreeTime() { return m_freeTime; }
		void SetFreeTimeNow() { m_freeTime = Util::CurrentTime(); }
	private:
		bool m_inUse = false;
		time_t m_freeTime = 0;
	};

	typedef std::vector<int> Levels;
	typedef std::vector<std::unique_ptr<PooledConnection>> Connections;

	Servers m_servers;
	RawServerList m_sortedServers;
	Connections m_connections;
	Levels m_levels;
	int m_maxNormLevel = 0;
	Mutex m_connectionsMutex;
	int m_timeout = 60;
	int m_retryInterval = 0;
	int m_generation = 0;

	friend uintptr_t NzbgetCArticleDlHost(void*, int, int, int, uintptr_t, uintptr_t,
		const char*, const char*, int*, int64_t*, const char**);
};

extern ServerPool* g_ServerPool;

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
				auto& value = static_cast<Context*>(context)->snapshot;
				if (!g_ScriptConfig->ReadCipherSnapshot(value)) return 0;
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

		class ProbeConnection final : public NntpConnection
		{
		public:
			explicit ProbeConnection(NewsServer* server) : NntpConnection(server) {}
		protected:
			void PrintError(const char*) override {}
		};
		inline int Probe(void*, const NzbgetTlsServer* server, const char* cipher, int protocol, const char* numeric,
			uint64_t milliseconds, NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			try
			{
				if (cancel(cancelContext)) return -6;
				if (milliseconds == 0) return -3;
				if (milliseconds > 10000) return -1;
				if (server->verification && !TlsSocket::HasCertificateStore()) return -4;
				auto deadline = std::make_shared<SocketDeadline>();
				deadline->end = std::chrono::steady_clock::now() + std::chrono::milliseconds(milliseconds);
				deadline->cancelled = [=] { return cancel(cancelContext) != 0; };
				NewsServer value(server->id, true, "TLS cipher probe", server->host, server->port, server->ip_version,
					server->username, server->password, false, true, cipher, 1, 0, 0, 0, false, server->verification);
				ProbeConnection connection(&value);
				std::vector<std::string> addresses;
				std::string remaining(numeric);
				for (size_t pos = 0; pos < remaining.size();)
				{
					size_t end = remaining.find('\n', pos);
					addresses.push_back(remaining.substr(pos, end == std::string::npos ? end : end - pos));
					if (end == std::string::npos) break; pos = end + 1;
				}
				if (addresses.empty() || addresses.size() > 16) return -2;
				connection.SetResolvedAddresses(addresses); connection.SetDeadline(deadline);
				connection.SetSuppressErrors(true); connection.SetTimeout(10);
				bool connected = connection.Connect();
				int expectedVer = protocol == 13 ? TLS1_3_VERSION : TLS1_2_VERSION;
				int result = (connected && strcmp(connection.GetNegotiatedCipher(), cipher) == 0 &&
					connection.GetNegotiatedVersion() == expectedVer) ? 1 :
					connection.GetAuthError() ? -5 : connection.GetTlsProbeFailure();
				if (cancel(cancelContext)) result = -6;
				else if (std::chrono::steady_clock::now() >= deadline->end) result = -3;
				connection.SetBroken(); connection.Disconnect();
				return result;
			}
			catch (...) { ERR_clear_error(); return -1; }
		}
		inline int Commit(void*, const unsigned char* original, size_t originalLength, const unsigned char* replacement, size_t replacementLength,
			NzbgetTlsCancelled cancel, void* cancelContext) noexcept
		{
			try
			{
				auto result = g_ScriptConfig->CommitCipherPatch(
					std::string_view(reinterpret_cast<const char*>(original), originalLength),
					std::string_view(reinterpret_cast<const char*>(replacement), replacementLength), [=] { return cancel(cancelContext) != 0; });
				switch (result)
				{
					case ScriptConfig::CipherCommit::Saved: return 1;
					case ScriptConfig::CipherCommit::Unchanged: return 0;
					case ScriptConfig::CipherCommit::Conflict: return -2;
					case ScriptConfig::CipherCommit::Cancelled: return -3;
					default: return -1;
				}
			}
			catch (...) { return -1; }
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

class StatMeter : public Debuggable
{
public:
	StatMeter() : m_rs(nzbget_rs_statmeter_new(LogRs(), OptRs())) {}
	~StatMeter() override { nzbget_rs_statmeter_free(m_rs); }
	StatMeter(const StatMeter&) = delete;
	StatMeter& operator=(const StatMeter&) = delete;

	const NzbgetRsStatMeter* RsHandle() const { return m_rs; }

	void Init()
	{
		int totalOffset = 0;
		nzbget_rs_statmeter_ctl(m_rs, 0, (int)g_ServerPool->GetServers()->size(), 0, 0, 0, 0,
			OptRs(), LogRs(), &totalOffset, nullptr);
		g_WorkState->SetLocalTimeOffset(totalOffset);
	}

	int64 CalcCurrentDownloadSpeed() { return nzbget_rs_statmeter_speed(m_rs, 0, 0); }
	int64 CalcMomentaryDownloadSpeed() { return nzbget_rs_statmeter_speed(m_rs, 1, 0); }
	void AddSpeedReading(int bytes) { nzbget_rs_statmeter_speed(m_rs, 2, bytes); }

	void AddServerStats(uint32 bytes, uint32 failed, uint32 success, int serverId)
	{
		nzbget_rs_statmeter_ctl(m_rs, 1, serverId, g_WorkState->GetLocalTimeOffset(),
			bytes, failed, success, nullptr, nullptr, nullptr, nullptr);
	}

	void CalcTotalStat(int* upTimeSec, int* dnTimeSec, int64* allBytes, bool* standBy)
	{
		int out[3]{};
		nzbget_rs_statmeter_ctl(m_rs, 2, 0, 0, 0, 0, 0, nullptr, nullptr, out, allBytes);
		if (upTimeSec) *upTimeSec = out[0];
		if (dnTimeSec) *dnTimeSec = out[1];
		if (standBy) *standBy = out[2] != 0;
	}

	void CalcQuotaUsage(int64& monthBytes, int64& dayBytes)
	{
		int64 out[2]{};
		nzbget_rs_statmeter_ctl(
			m_rs, 3, g_WorkState->GetLocalTimeOffset(), 0, 0, 0, 0, OptRs(), LogRs(), nullptr, out);
		monthBytes = out[0];
		dayBytes = out[1];
	}

	void IntervalCheck()
	{
		int out[2]{};
		int mask = nzbget_rs_statmeter_ctl(
			m_rs, 4, g_WorkState->GetLocalTimeOffset(), g_WorkState->GetQuotaReached() ? 1 : 0,
			0, 0, 0, OptRs(), LogRs(), out, nullptr);
		if (mask & 1) g_WorkState->SetLocalTimeOffset(out[0]);
		if (mask & 2) g_WorkState->SetQuotaReached(out[1] != 0);
		if (mask & 4) Save();
	}

	void EnterLeaveStandBy(bool enter)
	{
		nzbget_rs_statmeter_ctl(m_rs, 5, enter ? 1 : 0, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr);
	}

	void Save()
	{
		if (!g_Options || !g_Options->GetServerMode()) return;
		std::vector<NzbgetRsServerMatchItem> srvs = BuildServers();
		nzbget_rs_statmeter_disk(
			m_rs, 0, 1, g_WorkState->GetLocalTimeOffset(), OptRs(), LogRs(),
			srvs.data(), srvs.size(), nullptr, nullptr);
	}

	bool Load(bool* perfectServerMatch)
	{
		Servers* servers = g_ServerPool->GetServers();
		std::vector<NzbgetRsServerMatchItem> srvs = BuildServers();
		std::vector<int> stateIds(srvs.size(), -1);
		int pm = (perfectServerMatch && *perfectServerMatch) ? 1 : 0;
		bool ok = nzbget_rs_statmeter_disk(
			m_rs, 1, g_Options ? (g_Options->GetServerMode() ? 1 : 0) : 0,
			g_WorkState->GetLocalTimeOffset(), OptRs(), LogRs(),
			srvs.data(), srvs.size(), stateIds.data(), perfectServerMatch ? &pm : nullptr) != 0;
		for (size_t i = 0; i < srvs.size(); i++)
			if (stateIds[i] >= 0) (*servers)[i]->SetStateId(stateIds[i]);
		if (perfectServerMatch) *perfectServerMatch = pm != 0;
		return ok;
	}

protected:
	void LogDebugInfo() override
	{
		nzbget_rs_statmeter_ctl(m_rs, 6, 0, 0, 0, 0, 0, OptRs(), LogRs(), nullptr, nullptr);
	}

private:
	NzbgetRsStatMeter* m_rs = nullptr;

	static const NzbgetRsOptions* OptRs() { return g_Options ? g_Options->RsHandle() : nullptr; }
	static const NzbgetRsLog* LogRs() { return g_Log ? g_Log->RsHandle() : nullptr; }

	static std::vector<NzbgetRsServerMatchItem> BuildServers()
	{
		Servers* servers = g_ServerPool->GetServers();
		std::vector<NzbgetRsServerMatchItem> out;
		out.reserve(servers->size());
		for (NewsServer* ns : servers)
			out.push_back({ ns->GetId(), ns->GetPort(), ns->GetName(), ns->GetHost(), ns->GetUser() });
		return out;
	}
};

extern StatMeter* g_StatMeter;

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
	CachedSegmentData& operator=(CachedSegmentData&& other);
	~CachedSegmentData() override;
	char* GetData() override { return m_data; }

private:
	char* m_data = nullptr;
	int m_size = 0;

	friend class ArticleCache;
};

class ArticleCache;
extern ArticleCache* g_ArticleCache;

class ArticleCache final : public Thread
{
public:
	void Run() override;

	void Stop() override
	{
		Thread::Stop();

		// Resume Run() to exit it
		std::lock_guard<std::mutex> guard(m_allocMutex);
		m_allocCond.notify_all();
	}

	CachedSegmentData Alloc(int size);

	bool Realloc(CachedSegmentData* segment, int newSize)
	{
		std::lock_guard<std::mutex> guard(m_allocMutex);

		void* p = realloc(segment->m_data, newSize);
		if (p)
		{
			m_allocated += newSize - segment->m_size;
			segment->m_size = newSize;
			segment->m_data = (char*)p;
		}

		return p;
	}

	void Free(CachedSegmentData* segment)
	{
		if (segment->m_size)
		{
			free(segment->m_data);

			std::lock_guard<std::mutex> guard(m_allocMutex);
			size_t sub = segment->m_size > 0 ? (size_t)segment->m_size : 0;
			m_allocated = m_allocated >= sub ? (m_allocated - sub) : 0;
			if (m_allocated == 0 && g_Options->GetServerMode() && g_Options->GetContinuePartial())
			{
				g_DiskState->DeleteCacheFlag();
			}
		}
	}

	void LockFlush()
	{
		m_flushMutex.Lock();
		m_flushing.store(true, std::memory_order_release);
	}

	void UnlockFlush()
	{
		m_flushing.store(false, std::memory_order_release);
		m_flushMutex.Unlock();
	}

	Guard GuardContent() { return Guard(m_contentMutex); }
	bool GetFlushing() { return m_flushing.load(std::memory_order_acquire); }
	size_t GetAllocated() { return m_allocated; }
	bool FileBusy(FileInfo* fileInfo) { return fileInfo == m_fileInfo; }
	void WakeupFlush()
	{
		std::lock_guard<std::mutex> guard(m_allocMutex);
		m_allocCond.notify_all();
	}

private:
	std::atomic<size_t> m_allocated{0};
	FileInfo* m_fileInfo = nullptr;
	std::mutex m_allocMutex;
	std::condition_variable m_allocCond;
	Mutex m_flushMutex;
	Mutex m_contentMutex;
	std::atomic<bool> m_flushing{false};
	std::vector<ArticleInfo*> m_flushArticles;

	size_t GetEffectiveLimit() const;
	friend uintptr_t NzbgetCArticleDlHost(void*, int, int, int, uintptr_t, uintptr_t,
		const char*, const char*, int*, int64_t*, const char**);
};

inline CachedSegmentData::~CachedSegmentData()
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
}

inline CachedSegmentData& CachedSegmentData::operator=(CachedSegmentData&& other)
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
	m_data = other.m_data;
	m_size = other.m_size;
	other.m_data = nullptr;
	other.m_size = 0;
	return *this;
}

class MemoryMonitor final : public Thread
{
public:
	MemoryMonitor()
	{
		nzbget_rs_article_dl_exec(nullptr, 100, 1, 0, 0, 0, nullptr, NzbgetCArticleDlHost, this);
	}

	~MemoryMonitor() override
	{
		if (IsRunning())
		{
			Stop();
		}
		while (IsRunning())
		{
			Util::Sleep(10);
		}
	}

	void InitOptions()
	{
		nzbget_rs_article_dl_exec(nullptr, 100, 0, 0, 0, 0, nullptr, NzbgetCArticleDlHost, this);
	}

	void Run() override
	{
		nzbget_rs_article_dl_exec(nullptr, 101, 0, 0, 0, 0, nullptr, NzbgetCArticleDlHost, this);
	}

	void Stop() override
	{
		Thread::Stop();
	}

	int GetTier() const { return (int)nzbget_rs_article_dl_exec(nullptr, 102, 0, 0, 0, 0, nullptr, nullptr, nullptr); }
	const char* GetTierName() const
	{
		return reinterpret_cast<const char*>(nzbget_rs_article_dl_exec(nullptr, 103, 0, 0, 0, 0, nullptr, nullptr, nullptr));
	}

	size_t GetEffectiveArticleCacheBytes() const
	{
		return (size_t)std::max(0, GetEffectiveArticleCacheMB()) * 1024 * 1024;
	}
	int GetEffectiveArticleCacheMB() const
	{
		return (int)nzbget_rs_article_dl_exec(nullptr, 104, 0, 0, 0, 0, nullptr, nullptr, nullptr);
	}
	int GetEffectiveParBufferMB(int configuredMB) const
	{
		return (int)nzbget_rs_article_dl_exec(nullptr, 105, configuredMB, 0, 0, 0, nullptr, nullptr, nullptr);
	}
	int GetEffectiveDownloadsLimit(int configuredLimit) const
	{
		return (int)nzbget_rs_article_dl_exec(nullptr, 106, configuredLimit, 0, 0, 0, nullptr, nullptr, nullptr);
	}
};

inline MemoryMonitor* g_MemoryMonitor = nullptr;

inline size_t ArticleCache::GetEffectiveLimit() const
{
	return g_MemoryMonitor ?
		g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
		(size_t)std::max(0, g_Options->GetArticleCache()) * 1024 * 1024;
}

inline CachedSegmentData ArticleCache::Alloc(int size)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = nullptr;
	size_t effectiveLimit = GetEffectiveLimit();

	bool wasEmpty = m_allocated == 0;
	size_t avail = effectiveLimit > m_allocated ? effectiveLimit - m_allocated : 0;
	if (effectiveLimit > 0 && (size_t)size <= avail)
	{
		p = malloc(size);
		if (p)
		{
			if (wasEmpty && g_Options->GetServerMode() &&
				g_Options->GetContinuePartial() && !g_Options->GetSkipWrite())
			{
				g_DiskState->WriteCacheFlag();
			}
			if (wasEmpty)
			{
				// Resume Run(), the notification arrives later, after releasing m_allocMutex
				m_allocCond.notify_all();
			}
			m_allocated += size;
		}
	}

	return CachedSegmentData((char*)p, p ? size : 0);
}

inline void ArticleCache::Run()
{
	nzbget_rs_article_dl_exec(nullptr, 110, 0, 0, 0, 0, nullptr, NzbgetCArticleDlHost, nullptr);
}

#endif

inline uintptr_t NzbgetCArticleFetchHost(int op, int a1, int a2, uintptr_t p1, const char* s1,
	int* ints, int64_t* i64s, const char** strs, uint8_t* buf, size_t bufLen)
{
	switch (op)
	{
	case 0: {
		ints[0] = g_ServerPool->GetMaxNormLevel();
		ints[1] = g_WorkState->GetQuotaReached() ? 1 : 0;
		ints[2] = g_Options->GetArticleReadChunkSize();
		ints[3] = (int)g_ServerPool->GetServers()->size();
		int64 speedLimit = g_WorkState->GetSpeedLimit();
		i64s[0] = speedLimit;
		i64s[1] = speedLimit > 0 ? g_StatMeter->CalcCurrentDownloadSpeed() : 0;
		i64s[2] = speedLimit > 0 ? g_StatMeter->CalcMomentaryDownloadSpeed() : 0;
		return 0;
	}
	case 1: {
		NewsServer* s = g_ServerPool->GetServers()->at((size_t)a1).get();
		ints[0] = s->GetActive() ? 1 : 0;
		ints[1] = s->GetNormLevel();
		ints[2] = s->GetGroup();
		ints[3] = s->GetId();
		ints[4] = s->GetMaxConnections();
		ints[5] = s->GetJoinGroup() ? 1 : 0;
		strs[0] = s->GetName();
		return reinterpret_cast<uintptr_t>(s);
	}
	case 2: {
		ServerPool::RawServerList failed;
		if (buf && a2 > 0)
		{
			const uintptr_t* ptrs = reinterpret_cast<const uintptr_t*>(buf);
			failed.reserve((size_t)a2);
			for (int i = 0; i < a2; i++) failed.push_back(reinterpret_cast<NewsServer*>(ptrs[i]));
		}
		NntpConnection* conn = g_ServerPool->GetConnection(a1, nullptr, &failed);
		if (!conn) return 0;
		NewsServer* s = conn->GetNewsServer();
		ints[0] = conn->GetStatus() == Connection::csConnected ? 1 : 0;
		ints[1] = s->GetId();
		ints[2] = s->GetMaxConnections();
		ints[3] = s->GetJoinGroup() ? 1 : 0;
		ints[4] = s->GetGroup();
		i64s[0] = (int64_t)reinterpret_cast<uintptr_t>(s);
		strs[0] = s->GetName();
		return reinterpret_cast<uintptr_t>(conn);
	}
	case 3: {
		NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
		int bytesRead = conn->FetchTotalBytesRead();
		if (bytesRead > 0)
		{
			g_StatMeter->AddServerStats(
				Util::SafeIntCast<int, uint32>(bytesRead), 0, 0, conn->GetNewsServer()->GetId());
		}
		if (!a1 || conn->GetStatus() == Connection::csCancelled) conn->Disconnect();
		g_ServerPool->FreeConnection(conn, true);
		return 0;
	}
	case 4: {
		NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
		strs[0] = conn->GetNewsServer()->GetName();
		ints[0] = conn->GetNewsServer()->GetJoinGroup() ? 1 : 0;
		return conn->Connect() ? 1 : 0;
	}
	case 5: {
		const char* resp = reinterpret_cast<NntpConnection*>(p1)->JoinGroup(s1 ? s1 : "");
		strs[0] = resp;
		return resp ? 1 : 0;
	}
	case 6: {
		const char* resp = reinterpret_cast<NntpConnection*>(p1)->Request(s1 ? s1 : "");
		strs[0] = resp;
		return resp ? 1 : 0;
	}
	case 7: {
		NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
		char* buffer = nullptr;
		int len = 0;
		conn->ReadBuffer(&buffer, &len);
		if (len == 0)
		{
			len = conn->TryRecv(reinterpret_cast<char*>(buf), (int)bufLen);
			buffer = reinterpret_cast<char*>(buf);
		}
		if (len <= 0) { ints[0] = len; strs[0] = nullptr; return 0; }
		g_StatMeter->AddSpeedReading(len);
		ints[0] = len;
		strs[0] = buffer;
		return 1;
	}
	case 8:
		reinterpret_cast<NntpConnection*>(p1)->Disconnect();
		return 0;
	case 9: {
		NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
		conn->SetSuppressErrors(true);
		conn->Cancel();
		return 0;
	}
	case 10:
		detail("%s", s1 ? s1 : "");
		return 0;
	default:
		return 0;
	}
}

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
class TwinCheck : public Service
{
public:
	static constexpr const char* FilesParam = "DupeFiles";
	static constexpr const char* KindParam = "DupeKind";
	static constexpr const char* SampledParam = "DupeSampled";
	static constexpr const char* SampledOfParam = "DupeSampledOf";

	static void StopAll() { nzbget_rs_twin_exec(4, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, nullptr, &HostCb, nullptr); }

	TwinCheck()
	{
		nzbget_rs_twin_exec(6, 0, 0, reinterpret_cast<uintptr_t>(this), 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
		m_observer.m_owner = this;
		DownloadQueue::Guard()->Attach(&m_observer);
	}

	~TwinCheck() override
	{
		DownloadQueue::Guard()->Detach(&m_observer);
		nzbget_rs_twin_exec(7, 0, 0, reinterpret_cast<uintptr_t>(this), 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
	}

protected:
	int ServiceInterval() override { return nzbget_rs_twin_exec(9, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr); }
	void ServiceWork() override { nzbget_rs_twin_exec(10, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, &NzbgetCArticleFetchHost, &HostCb, nullptr); }

private:
	class TwinCheckJob : public Thread
	{
	public:
		explicit TwinCheckJob(uintptr_t token) : m_token(token) {}
	protected:
		void Run() override { nzbget_rs_twin_exec(12, 0, 0, m_token, 0, nullptr, nullptr, nullptr, nullptr, nullptr, &HostCb, nullptr); }
	private:
		uintptr_t m_token;
	};

	static NzbInfo* FindNzbInQueueOrHistory(DownloadQueue* dq, int nzbId)
	{
		if (NzbInfo* queued = dq->GetQueue()->Find(nzbId)) return queued;
		for (std::unique_ptr<HistoryInfo>& h : *dq->GetHistory())
			if (h->GetKind() == HistoryInfo::hkNzb && h->GetNzbInfo()->GetId() == nzbId) return h->GetNzbInfo();
		return nullptr;
	}

	static uintptr_t HostCb(int op, int a1, int a2, uintptr_t p1, const char* s1, const char* s2,
		const char* s3, const char* s4, int* ints, int64_t* i64s, const char** strs)
	{
		switch (op)
		{
		case 0:
			ints[0] = (g_Options && g_Options->GetDupeCheck()) ? 1 : 0;
			i64s[0] = (int64_t)Util::CurrentTime();
			strs[0] = (g_Options && !Util::EmptyStr(g_Options->GetQueueDir())) ? g_Options->GetQueueDir() : nullptr;
			return 0;
		case 1: reinterpret_cast<TwinCheckJob*>(p1)->Stop(); return 0;
		case 2: reinterpret_cast<TwinCheck*>(p1)->WakeUp(); return 0;
		case 3: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			nzbget_rs_twin_exec(11, 0, 0, p1, reinterpret_cast<uintptr_t>((DownloadQueue*)dq), nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
			return 0;
		}
		case 4: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			ints[0] = (int)dq->GetQueue()->size();
			ints[1] = (int)dq->GetHistory()->size();
			ints[2] = (int)NzbInfo::dsDupe;
			ints[3] = (int)NzbInfo::dsCopy;
			return 0;
		}
		case 5:
		case 6: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			NzbInfo* nzb = nullptr;
			if (op == 5) nzb = (*dq->GetQueue())[(size_t)a1].get();
			else { HistoryInfo* h = (*dq->GetHistory())[(size_t)a1].get(); if (h->GetKind() != HistoryInfo::hkNzb) return 0; nzb = h->GetNzbInfo(); }
			NzbParameter* pFiles = nzb->GetParameters()->Find(FilesParam);
			NzbParameter* pSampled = nzb->GetParameters()->Find(SampledParam);
			NzbParameter* pSampledOf = nzb->GetParameters()->Find(SampledOfParam);
			NzbParameter* pKind = nzb->GetParameters()->Find(KindParam);
			ints[0] = nzb->GetId();
			ints[1] = (int)nzb->GetDeleteStatus();
			ints[2] = nzb->GetDupeScore();
			ints[3] = (int)(uint32_t)nzb->GetFullContentHash();
			ints[4] = (nzb->GetParameters()->Find("DupeSameSize") != nullptr || nzb->GetParameters()->Find("DupeSameInner") != nullptr) ? 1 : 0;
			ints[5] = nzb->GetKind() == NzbInfo::nkNzb ? 1 : 0;
			ints[6] = (pSampled ? 1 : 0) | (pSampledOf ? 2 : 0);
			i64s[0] = nzb->GetRemainingSize();
			i64s[1] = nzb->GetPausedSize();
			strs[0] = nzb->GetName();
			strs[1] = nzb->GetDupeKey();
			strs[2] = nzb->GetQueuedFilename();
			strs[3] = pFiles ? pFiles->GetValue() : nullptr;
			strs[4] = pSampled ? pSampled->GetValue() : nullptr;
			strs[5] = pSampledOf ? pSampledOf->GetValue() : nullptr;
			strs[6] = pKind ? pKind->GetValue() : nullptr;
			return reinterpret_cast<uintptr_t>(nzb);
		}
		case 7: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p1);
			nzb->GetParameters()->SetParameter(KindParam, s1 ? s1 : "");
			if (s2 && *s2) nzb->PrintMessage(Message::mkInfo, "%s", s2);
			return 0;
		}
		case 8: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			dq->HistoryChanged();
			dq->Save();
			return 0;
		}
		case 9: {
			auto* job = new TwinCheckJob(p1);
			job->SetAutoDestroy(true);
			return reinterpret_cast<uintptr_t>(job);
		}
		case 10: reinterpret_cast<TwinCheckJob*>(p1)->Start(); return 0;
		case 11: return reinterpret_cast<TwinCheckJob*>(p1)->IsStopped() ? 1 : 0;
		case 12: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			NzbInfo* nzb = FindNzbInQueueOrHistory(dq, a1);
			if (!nzb) return 0;
			nzb->GetParameters()->SetParameter(s1, s2);
			if (s3) nzb->GetParameters()->SetParameter(s3, s4);
			dq->HistoryChanged();
			dq->Save();
			return 1;
		}
		case 13: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			NzbInfo* nzb = FindNzbInQueueOrHistory(dq, a1);
			if (!nzb) return 0;
			nzb->GetParameters()->SetParameter(FilesParam, s1 ? s1 : "");
			nzb->PrintMessage(Message::mkDetail, "Fingerprint of %s: %s (%i file(s) in its par2 set)", nzb->GetName(), s1 ? s1 : "", a2);
			dq->HistoryChanged();
			dq->Save();
			return 1;
		}
		default: return 0;
		}
	}

	class DownloadQueueObserver final : public Observer
	{
	public:
		TwinCheck* m_owner;
		void Update(Subject*, void* aspect) override
		{
			DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
			if (queueAspect->action == DownloadQueue::eaNzbAdded &&
				queueAspect->nzbInfo && !Util::EmptyStr(queueAspect->nzbInfo->GetDupeKey()))
			{
				nzbget_rs_twin_exec(8, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr);
				m_owner->WakeUp();
			}
		}
	};

	DownloadQueueObserver m_observer;
};

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

	ArticleDownloader(FileInfo* fileInfo, ArticleInfo* articleInfo, NntpConnection* connection,
		bool hasAnalyzer, const char* infoName)
		: m_fileInfo(fileInfo), m_articleInfo(articleInfo), m_lastUpdateTime(Util::CurrentTime())
	{
		debug("Creating ArticleDownloader");
#ifndef DISABLE_PARCHECK
		constexpr int parEnabled = 1;
#else
		constexpr int parEnabled = 0;
#endif
		m_rs = nzbget_rs_article_dl_new(hasAnalyzer ? 1 : 0, parEnabled,
			reinterpret_cast<uintptr_t>(connection), infoName ? infoName : "");
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
	EStatus GetStatus() { return m_status.load(); }
	time_t GetLastUpdateTime() { return m_lastUpdateTime.load(); }

	uintptr_t ExecRs(int op, int a1 = 0, uintptr_t p1 = 0, uintptr_t p2 = 0, const char* s1 = nullptr)
	{
		return nzbget_rs_article_dl_exec(m_rs, op, a1, 0, p1, p2, s1, NzbgetCArticleDlHost, this);
	}

	void Run() override { ExecRs(0); }

	void Stop() override
	{
		debug("Trying to stop ArticleDownloader");
		Thread::Stop();
		ExecRs(16);
		debug("ArticleDownloader stopped successfully");
	}

private:
	FileInfo* m_fileInfo = nullptr;
	ArticleInfo* m_articleInfo = nullptr;
	std::atomic<EStatus> m_status{adUndefined};
	std::atomic<time_t> m_lastUpdateTime{0};
	CachedSegmentData m_cachedData;
	NzbgetRsArticleDownloader* m_rs = nullptr;

	friend uintptr_t NzbgetCArticleDlHost(void*, int, int, int, uintptr_t, uintptr_t,
		const char*, const char*, int*, int64_t*, const char**);
};

inline uintptr_t NzbgetCArticleDlHost(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
{
	switch (op)
	{
	case 1: {
		if (a1 == 10) { i64s[0] = (int64_t)Util::CurrentTime(); return 0; }
		if (a1 == 11) { Util::Sleep(a2); return 0; }
		if (a1 == 12)
		{
			info("      Download: status=%i, LastUpdateTime=%s, InfoName=%s",
				a2, *Util::FormatTime((time_t)i64s[0]), s1 ? s1 : "");
			return 0;
		}
		const char* msg = s1 ? s1 : "";
		if (p1 != 0) reinterpret_cast<NzbInfo*>(p1)->PrintMessage((Message::EKind)a1, "%s", msg);
		else if (a1 == Message::mkInfo) info("%s", msg);
		else if (a1 == Message::mkWarning) warn("%s", msg);
		else if (a1 == Message::mkError) error("%s", msg);
		else if (a1 == Message::mkDebug) debug("%s", msg);
		else if (a1 == Message::mkDetail) detail("%s", msg);
		return 0;
	}
	case 2: {
		if (a1 == 0)
		{
			if (!g_Options) { ints[0] = 0; ints[1] = 500; strs[0] = nullptr; return 0; }
			ints[0] = g_Options->GetArticleCache();
			ints[1] = g_Options->GetMemoryPressureInterval();
			strs[0] = g_Options->GetMemoryPressurePath();
			return 1;
		}
		if (a1 == 1) return static_cast<MemoryMonitor*>(ctx)->IsStopped() ? 1 : 0;
		if (a1 == 2) { if (g_ArticleCache) g_ArticleCache->WakeupFlush(); return 0; }
		if (a1 == 3)
		{
			i64s[0] = (int64_t)g_ArticleCache->m_allocated.load();
			i64s[1] = (int64_t)g_ArticleCache->GetEffectiveLimit();
			ints[0] = g_ArticleCache->IsStopped() ? 1 : 0;
			ints[1] = (g_Options && g_Options->GetDirectWrite()) ? 1 : 0;
			ints[2] = g_MemoryMonitor ? g_MemoryMonitor->GetTier() : 0;
			return 0;
		}
		if (a1 == 4)
		{
			std::unique_lock<std::mutex> lk(g_ArticleCache->m_allocMutex);
			g_ArticleCache->m_allocCond.wait(lk, [] {
				return g_ArticleCache->IsStopped() || g_ArticleCache->m_allocated > 0;
			});
			return 0;
		}
		if (a1 == 5)
		{
			bool flushEverything = a2 != 0;
			static thread_local CString s_infoName;
			{
				GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
				for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
				{
					if (g_ArticleCache->m_fileInfo) break;
					for (FileInfo* fileInfo : nzbInfo->GetFileList())
					{
						if (fileInfo->GetCachedArticles() > 0 &&
							(fileInfo->GetActiveDownloads() == 0 || flushEverything) &&
							(!fileInfo->GetNzbInfo()->GetSkipDiskWrite() || flushEverything))
						{
							g_ArticleCache->m_fileInfo = fileInfo;
							s_infoName.Format("%s%c%s",
								fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, fileInfo->GetFilename());
							break;
						}
					}
				}
			}
			if (!g_ArticleCache->m_fileInfo) return 0;
			strs[0] = *s_infoName;
			return reinterpret_cast<uintptr_t>(g_ArticleCache->m_fileInfo);
		}
		if (a1 == 6) { g_ArticleCache->m_fileInfo = nullptr; return 0; }
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
			g_ArticleCache->LockFlush();
			{
				Guard cg = g_ArticleCache->GuardContent();
				if (f->GetFlushLocked())
				{
					g_ArticleCache->UnlockFlush();
					return 0;
				}
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
			g_ArticleCache->UnlockFlush();
			ints[0] = ((f->GetNzbInfo() && f->GetNzbInfo()->GetSkipDiskWrite()) || g_Options->GetSkipWrite()) ? 1 : 0;
			return 0;
		}
		return 0;
	}
	case 3: {
		if (a1 == 0)
		{
			ints[0] = (int)g_ServerPool->m_sortedServers.size();
			ints[1] = (int)g_ServerPool->m_connections.size();
			ints[2] = (int)g_ServerPool->m_levels.size();
			ints[3] = g_ServerPool->m_maxNormLevel;
			ints[4] = g_ServerPool->m_retryInterval;
			ints[5] = g_ServerPool->m_timeout;
			ints[6] = (int)g_ServerPool->m_servers.size();
			i64s[0] = (int64_t)Util::CurrentTime();
			return 0;
		}
		if (a1 == 1)
		{
			NewsServer* s = a2 < 0 ? reinterpret_cast<NewsServer*>(p1) :
				(p2 != 0 ? g_ServerPool->m_servers[(size_t)a2].get() : g_ServerPool->m_sortedServers[(size_t)a2]);
			ints[0] = s->GetId();
			ints[1] = s->GetLevel();
			ints[2] = s->GetNormLevel();
			ints[3] = s->GetActive() ? 1 : 0;
			ints[4] = s->GetOptional() ? 1 : 0;
			ints[5] = s->GetGroup();
			ints[6] = s->GetMaxConnections();
			i64s[0] = (int64_t)s->GetBlockTime();
			strs[0] = s->GetName();
			strs[1] = s->GetHost();
			return reinterpret_cast<uintptr_t>(s);
		}
		if (a1 == 2)
		{
			if (a2 == 0) g_ServerPool->m_sortedServers[p1]->SetNormLevel((int)p2);
			else if (a2 == 1)
			{
				if (p1 > 0)
				{
					ServerPool::RawServerList newSorted(p1);
					for (size_t i = 0; i < p1; i++) newSorted[i] = g_ServerPool->m_sortedServers[(size_t)ints[i]];
					g_ServerPool->m_sortedServers = std::move(newSorted);
					g_ServerPool->m_maxNormLevel = (int)p2;
				}
				g_ServerPool->m_levels.clear();
			}
			else if (a2 == 2)
			{
				NewsServer* ns = g_ServerPool->m_sortedServers[p1];
				ns->SetBlockTime(0);
				int normLevel = ints[0];
				if (normLevel > -1)
				{
					if ((int)g_ServerPool->m_levels.size() <= normLevel) g_ServerPool->m_levels.push_back(0);
					if (ints[2] != 0)
					{
						for (size_t k = 0; k < p2; k++)
						{
							auto conn = std::make_unique<ServerPool::PooledConnection>(ns);
							conn->SetTimeout(g_ServerPool->m_timeout);
							g_ServerPool->m_connections.push_back(std::move(conn));
						}
						g_ServerPool->m_levels[(size_t)normLevel] += ints[1];
					}
				}
			}
			else if (a2 == 3) g_ServerPool->m_generation++;
			return 0;
		}
		if (a1 == 3)
		{
			auto* c = g_ServerPool->m_connections[(size_t)a2].get();
			NewsServer* s = c->GetNewsServer();
			ints[0] = c->GetInUse() ? 1 : 0;
			ints[1] = c->GetStatus() == Connection::csConnected ? 1 : 0;
			ints[2] = s->GetId();
			ints[3] = s->GetLevel();
			ints[4] = s->GetNormLevel();
			i64s[0] = (int64_t)c->GetFreeTime();
			strs[0] = s->GetName();
			strs[1] = s->GetHost();
			return reinterpret_cast<uintptr_t>(s);
		}
		if (a1 == 4)
		{
			auto* lst = reinterpret_cast<ServerPool::RawServerList*>(p1);
			if (a2 < 0) return lst ? lst->size() : 0;
			NewsServer* s = (*lst)[(size_t)a2];
			ints[0] = s->GetId();
			ints[1] = s->GetGroup();
			ints[2] = s->GetNormLevel();
			return 0;
		}
		if (a1 == 5)
		{
			if (a2 == 0) { g_ServerPool->m_sortedServers[p1]->SetBlockTime((time_t)i64s[0]); return 0; }
			if (a2 == 1)
			{
				auto* c = g_ServerPool->m_connections[p1].get();
				c->SetInUse(true);
				g_ServerPool->m_levels[p2]--;
				return reinterpret_cast<uintptr_t>(static_cast<NntpConnection*>(c));
			}
			if (a2 == 2)
			{
				auto* c = static_cast<ServerPool::PooledConnection*>(reinterpret_cast<NntpConnection*>(p1));
				c->SetInUse(false);
				if (p2 != 0) c->SetFreeTimeNow();
				NewsServer* s = c->GetNewsServer();
				if (s->GetNormLevel() > -1 && s->GetActive()) g_ServerPool->m_levels[(size_t)s->GetNormLevel()]++;
				return 0;
			}
			if (a2 == 3)
			{
				g_ServerPool->m_connections.erase(std::remove_if(
					g_ServerPool->m_connections.begin(), g_ServerPool->m_connections.end(),
					[](std::unique_ptr<ServerPool::PooledConnection>& connection)
					{
						if (!connection->GetInUse() &&
							(connection->GetNewsServer()->GetNormLevel() == -1 || !connection->GetNewsServer()->GetActive()))
						{
							debug("Closing (and deleting) unused connection to server%i", connection->GetNewsServer()->GetId());
							if (connection->GetStatus() == Connection::csConnected) connection->Disconnect();
							return true;
						}
						return false;
					}),
					g_ServerPool->m_connections.end());
				return 0;
			}
			if (a2 == 4) { g_ServerPool->m_connections[p1]->Disconnect(); return 0; }
			if (a2 == 5)
			{
				NewsServer* s = reinterpret_cast<NewsServer*>(p1);
				Guard guard(g_ServerPool->m_connectionsMutex);
				time_t curTime = Util::CurrentTime();
				i64s[0] = (int64_t)s->GetBlockTime();
				i64s[1] = (int64_t)curTime;
				s->SetBlockTime(curTime);
				ints[0] = g_ServerPool->m_retryInterval;
				strs[0] = s->GetName();
				strs[1] = s->GetHost();
				return 0;
			}
			if (a2 == 6) return (uintptr_t)g_ServerPool->m_levels[p1];
		}
		return 0;
	}
	case 4: {
		auto* ad = static_cast<ArticleDownloader*>(ctx);
		if (a1 == 0)
		{
			ints[0] = g_Options->GetArticleRetries() > 0 ? g_Options->GetArticleRetries() : 1;
			ints[1] = g_Options->GetArticleInterval() > 0 ? g_Options->GetArticleInterval() : 0;
			ints[2] = g_ServerPool->GetGeneration();
			ints[3] = (int)g_ServerPool->GetServers()->size();
			ints[4] = ad->m_fileInfo->GetNzbInfo()->GetForcePriority() ? 1 : 0;
			ints[5] = ad->m_fileInfo->GetExtraPriority() ? 1 : 0;
			ints[6] = ad->m_fileInfo->GetNzbInfo()->HasDesiredServer() ? 1 : 0;
			ints[7] = ad->m_articleInfo->GetDupeFallbackRound();
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
				ints[2] = g_Options->GetDirectWrite() ? 1 : 0;
				ints[3] = ad->m_fileInfo->GetForceDirectWrite() ? 1 : 0;
				ints[4] = ad->m_fileInfo->GetOutputInitialized() ? 1 : 0;
				strs[0] = g_Options->GetTempDir();
				strs[1] = ad->m_fileInfo->GetNzbInfo()->GetDestDir();
				strs[2] = ad->m_fileInfo->GetOutputFilename().c_str();
				strs[3] = ad->m_fileInfo->GetFilename();
				return 0;
			}
			if (a2 == 2) { strs[0] = ad->m_articleInfo->GetResultFilename(); return 0; }
			return 0;
		}
		if (a1 == 2)
		{
			ints[0] = ad->IsStopped() ? 1 : 0;
			ints[1] = (g_WorkState && (g_WorkState->GetPauseDownload() || g_WorkState->GetQuotaReached())) ? 1 : 0;
			ints[2] = (g_WorkState && g_WorkState->GetDownloadHeld()) ? 1 : 0;
			ints[3] = g_ServerPool->GetGeneration();
			ints[4] = g_ServerPool->GetMaxNormLevel();
			ints[5] = (int)g_ServerPool->GetServers()->size();
			i64s[0] = (int64_t)Util::CurrentTime();
			return 0;
		}
		if (a1 == 3)
		{
			if (p1 & 1) ad->m_lastUpdateTime = (time_t)i64s[0];
			ad->m_status.store((ArticleDownloader::EStatus)a2);
			if (p1 & 2) ad->Notify(nullptr);
			return 0;
		}
		if (a1 == 4)
		{
			ServerPool::RawServerList ign;
			if (i64s && p2 > 0)
			{
				ign.reserve(p2);
				for (size_t i = 0; i < p2; i++) ign.push_back(reinterpret_cast<NewsServer*>((uintptr_t)i64s[i]));
			}
			return reinterpret_cast<uintptr_t>(g_ServerPool->GetConnection(
				a2, reinterpret_cast<NewsServer*>(p1), &ign));
		}
		if (a1 == 5)
		{
			NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
			NewsServer* srv = conn->GetNewsServer();
			conn->SetSuppressErrors(false);
#ifndef DISABLE_TLS
			conn->SetCertVerifLevel(srv->GetCertVerificationLevel());
#endif
			ints[0] = srv->GetNormLevel();
			ints[1] = srv->GetRetention();
			ints[2] = srv->GetId();
			ints[3] = 0;
			ints[4] = srv->GetJoinGroup() ? 1 : 0;
			strs[0] = srv->GetName();
			strs[1] = conn->GetHost();
			return reinterpret_cast<uintptr_t>(srv);
		}
		if (a1 == 6)
		{
			NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
			bool ok = conn->Connect();
			ints[0] = conn->GetAuthRejected() ? 1 : 0;
			return ok ? 1 : 0;
		}
		if (a1 == 7)
		{
			if (a2 == 0)
			{
				NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
				int bytesRead = conn->FetchTotalBytesRead();
				g_StatMeter->AddServerStats(
					Util::SafeIntCast<int, uint32>(bytesRead), 0, 0, conn->GetNewsServer()->GetId());
				ints[0] = bytesRead;
			}
			else
			{
				g_StatMeter->AddServerStats(
					0, static_cast<uint32>(ints[1]), static_cast<uint32>(ints[0]), (int)p1);
			}
			return 0;
		}
		if (a1 == 8)
		{
			NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
			if (!a2 || conn->GetStatus() == Connection::csCancelled) conn->Disconnect();
			int bytesRead = conn->FetchTotalBytesRead();
			g_StatMeter->AddServerStats(
				Util::SafeIntCast<int, uint32>(bytesRead), 0, 0, conn->GetNewsServer()->GetId());
			g_ServerPool->FreeConnection(conn, true);
			ints[0] = bytesRead;
			return 0;
		}
		if (a1 == 9)
		{
			NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
			conn->SetSuppressErrors(true);
			conn->Cancel();
			return 0;
		}
		if (a1 == 10)
		{
			NewsServer* srv = reinterpret_cast<NewsServer*>(p1);
			g_ServerPool->BlockServer(srv);
			return srv->GetOptional() ? 1 : 0;
		}
		if (a1 == 11)
		{
			NewsServer* s = g_ServerPool->GetServers()->at((size_t)a2).get();
			ints[0] = s->GetId();
			ints[1] = s->GetNormLevel();
			ints[2] = s->GetGroup();
			ints[3] = (s->GetActive() && s->GetMaxConnections() != 0 &&
				!(s->GetOptional() && g_ServerPool->IsServerBlocked(s))) ? 1 : 0;
			return reinterpret_cast<uintptr_t>(s);
		}
		if (a1 == 12)
		{
			if (a2 == 0)
			{
				ad->m_articleInfo->SetCrc(0);
				ints[0] = g_Options->GetRawArticle() ? 1 : 0;
				ints[1] = g_Options->GetCrcCheck() ? 1 : 0;
				ints[2] = g_Options->GetArticleReadChunkSize();
				ints[3] = (int)ad->m_fileInfo->GetGroups()->size();
				ints[4] = ((ad->m_fileInfo->GetNzbInfo() && ad->m_fileInfo->GetNzbInfo()->GetSkipDiskWrite()) ||
					g_Options->GetSkipWrite()) ? 1 : 0;
				strs[0] = ad->m_articleInfo->GetMessageId();
			}
			else if (a2 == 1)
			{
				NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
				strs[0] = conn->JoinGroup(ad->m_fileInfo->GetGroups()->at(p2));
				ints[0] = conn->GetAuthError() ? 1 : 0;
			}
			else if (a2 == 2)
			{
				NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
				strs[0] = conn->Request(s1);
				ints[0] = conn->GetAuthError() ? 1 : 0;
			}
			return 0;
		}
		if (a1 == 13)
		{
			NntpConnection* conn = reinterpret_cast<NntpConnection*>(p1);
			if (!ad->IsStopped() && !a2 &&
				g_MemoryMonitor && g_MemoryMonitor->GetTier() == 2 &&
				g_ArticleCache && g_ArticleCache->GetFlushing())
			{
				Util::Sleep(10);
			}
			while (!ad->IsStopped() && (g_WorkState->GetSpeedLimit() > 0.0f) &&
				(g_StatMeter->CalcCurrentDownloadSpeed() > g_WorkState->GetSpeedLimit() ||
				 g_StatMeter->CalcMomentaryDownloadSpeed() > g_WorkState->GetSpeedLimit()))
			{
				ad->m_lastUpdateTime = Util::CurrentTime();
				Util::Sleep(10);
			}
			char* buffer = nullptr;
			int len = 0;
			conn->ReadBuffer(&buffer, &len);
			if (len == 0)
			{
				char* lineBuf = const_cast<char*>(s2);
				len = conn->TryRecv(lineBuf, (int)p2);
				buffer = lineBuf;
			}
			if (len <= 0)
			{
				ints[1] = ad->IsStopped() ? 0 : 1;
				conn->SetBroken();
				conn->Disconnect();
				return 0;
			}
			g_StatMeter->AddSpeedReading(len);
			time_t oldTime = ad->m_lastUpdateTime.load();
			time_t nowTime = Util::CurrentTime();
			ad->m_lastUpdateTime = nowTime;
			ints[0] = len;
			ints[1] = (oldTime != nowTime) ? 1 : 0;
			strs[0] = buffer;
			return 1;
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
			ints[0] = g_Options->GetDupeCheck() ? 1 : 0;
			ints[1] = (int)nzb->GetDupeMode();
			ints[2] = nzb->GetManyDupeFiles() ? 1 : 0;
			ints[3] = g_Options->GetDirectWrite() ? 1 : 0;
			ints[4] = ad->m_fileInfo->GetForceDirectWrite() ? 1 : 0;
			ints[5] = (nzb->GetSkipDiskWrite() || g_Options->GetSkipWrite()) ? 1 : 0;
			ints[6] = g_Options->GetArticleCache();
			ints[7] = g_Options->GetRawArticle() ? 1 : 0;
			ints[8] = g_Options->GetWriteBuffer();
			ints[9] = ad->m_articleInfo->GetSize();
			ints[10] = ad->m_articleInfo->GetDupeFallbackRound();
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
			if (a2 == 0) ad->m_cachedData = CachedSegmentData();
			else if (a2 == 1)
			{
				ad->m_cachedData = g_ArticleCache->Alloc((int)p1);
				ints[0] = g_ArticleCache->GetFlushing() ? 1 : 0;
				return reinterpret_cast<uintptr_t>(ad->m_cachedData.GetData());
			}
			else if (a2 == 2)
			{
				if ((int)p2 != (int)p1) g_ArticleCache->Realloc(&ad->m_cachedData, (int)p1);
				Guard cg = g_ArticleCache->GuardContent();
				ad->m_articleInfo->AttachSegment(
					std::make_unique<CachedSegmentData>(std::move(ad->m_cachedData)), i64s[0], (int)p1);
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
			if (a2 == 3) { if (p1 != 0) g_ArticleCache->LockFlush(); else g_ArticleCache->UnlockFlush(); return 0; }
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

class DupeCoordinator
{
public:
	enum EDupeStatus
	{
		dsNone = 0,
		dsQueued = 1,
		dsDownloading = 2,
		dsSuccess = 4,
		dsWarning = 8,
		dsFailure = 16
	};

	HistoryInfo* FindDupeBackup(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey);
	void HistoryMark(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, NzbInfo::EMarkStatus markStatus);
	EDupeStatus GetDupeStatus(DownloadQueue* downloadQueue, const char* name, const char* dupeKey);
};

extern DupeCoordinator* g_DupeCoordinator;

class DupeProbe : public Thread
{
public:
	struct Sample { CString MessageId; std::shared_ptr<std::vector<CString>> Groups; };

	static void StartRecheck(int nzbId, std::vector<Sample> samples) { Launch(nzbId, true, 0, samples); }

	static void LaunchRs(NzbgetRsDupeProbe* rs)
	{
		auto* probe = new DupeProbe(rs);
		if (!nzbget_rs_dupe_probe_exec(probe->m_rs, 2, 0, 0, 0, reinterpret_cast<uintptr_t>(probe), nullptr, nullptr, nullptr, nullptr, &HostCb))
		{
			delete probe;
			return;
		}
		probe->SetAutoDestroy(true);
		probe->Thread::Start();
	}

protected:
	void Run() override { nzbget_rs_dupe_probe_exec(m_rs, 3, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, &HostCb); }

private:
	NzbgetRsDupeProbe* m_rs = nullptr;
	explicit DupeProbe(NzbgetRsDupeProbe* rs) : m_rs(rs) {}
	~DupeProbe() override { nzbget_rs_dupe_probe_free(m_rs); }

	static void Launch(int nzbId, bool countAll, int recoveredAtStart, const std::vector<Sample>& samples)
	{
		NzbgetRsDupeProbe* rs = nzbget_rs_dupe_probe_new(nzbId, countAll ? 1 : 0, recoveredAtStart);
		std::vector<const char*> grpPtrs;
		for (const Sample& s : samples)
		{
			grpPtrs.clear();
			if (s.Groups) for (const CString& g : *s.Groups) grpPtrs.push_back(*g);
			nzbget_rs_dupe_probe_exec(rs, 1, grpPtrs.size(), 0, 0, 0, *s.MessageId, grpPtrs.data(), nullptr, nullptr, nullptr);
		}
		LaunchRs(rs);
	}

	static void Abandon(int nzbId, int sampled, int missingServers, int activeServers, int recoveredAtStart);

public:
	static uintptr_t HostCb(void* ctx, int op, int a1, int a2, int a3, uintptr_t p1, uintptr_t p2, const char* s1, int* ints, const char** strs)
	{
		switch (op)
		{
		case 0: return g_ServerPool->GetServers()->size();
		case 1: {
			NewsServer* s = g_ServerPool->GetServers()->at((size_t)a1).get();
			ints[0] = s->GetActive() ? 1 : 0; ints[1] = s->GetGroup(); ints[2] = s->GetNormLevel(); ints[3] = s->GetId();
			return 0;
		}
		case 2: return g_ServerPool->GetServerById(a1) != nullptr ? 1 : 0;
		case 3: {
			NewsServer* wanted = g_ServerPool->GetServerById(a1);
			NntpConnection* conn = wanted ? g_ServerPool->GetConnection(wanted->GetNormLevel(), wanted, nullptr) : nullptr;
			if (!conn) return 0;
			ints[0] = conn->GetNewsServer()->GetId(); ints[1] = conn->GetNewsServer()->GetJoinGroup() ? 1 : 0;
			strs[0] = conn->GetNewsServer()->GetName();
			return reinterpret_cast<uintptr_t>(conn);
		}
		case 4: {
			NewsServer* wanted = g_ServerPool->GetServerById(a1);
			if (!wanted) return 0;
			time_t bt = wanted->GetBlockTime();
			return (g_ServerPool->IsServerBlocked(wanted) || (bt > 0 && Util::CurrentTime() - bt < 60)) ? 1 : 0;
		}
		case 5: return reinterpret_cast<NntpConnection*>(p1)->Connect() ? 1 : 0;
		case 6: strs[0] = reinterpret_cast<NntpConnection*>(p1)->JoinGroup(s1); return strs[0] ? 1 : 0;
		case 7: strs[0] = reinterpret_cast<NntpConnection*>(p1)->Request(s1); return strs[0] ? 1 : 0;
		case 8: reinterpret_cast<NntpConnection*>(p1)->Disconnect(); return 0;
		case 9: {
			auto* conn = reinterpret_cast<NntpConnection*>(p1);
			if (a1 || conn->GetStatus() == Connection::csCancelled) conn->Disconnect();
			g_ServerPool->FreeConnection(conn, true);
			return 0;
		}
		case 10:
			if (p1) reinterpret_cast<DupeProbe*>(p1)->Thread::Stop();
			if (p2) { auto* c = reinterpret_cast<NntpConnection*>(p2); c->SetSuppressErrors(true); c->Cancel(); }
			return 0;
		case 11: return (p1 && reinterpret_cast<DupeProbe*>(p1)->IsStopped()) ? 1 : 0;
		case 12: {
			if (a1 == 3) {
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				if (NzbInfo* n = dq->GetQueue()->Find(a2)) { n->SetDeadPickProbed(false); n->PrintMessage(Message::mkDetail, "%s", s1); }
				return 0;
			}
			{
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				if (NzbInfo* n = dq->GetQueue()->Find(a2)) n->PrintMessage(Message::mkInfo, "%s", s1);
				else detail("Dupe probe of download %i: %s", a2, s1);
			}
			if (a1 == 1) Abandon(a2, a3, ints[0], ints[1], ints[2]);
			return 0;
		}
		case 13: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			nzbget_rs_dupe_probe_exec(nullptr, 5, 0, a1, a2, reinterpret_cast<uintptr_t>(&*dq), nullptr, nullptr, reinterpret_cast<size_t*>(ints), ctx, &HostCb);
			return 0;
		}
		case 14: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			for (HistoryInfo* hi : dq->GetHistory())
			{
				if (hi->GetKind() == HistoryInfo::hkNzb && hi->GetNzbInfo()->GetId() == a1)
				{
					ints[0] = (int)dq->GetQueue()->size();
					strs[0] = hi->GetNzbInfo()->GetName(); strs[1] = hi->GetNzbInfo()->GetDupeKey();
					return reinterpret_cast<uintptr_t>(hi);
				}
			}
			return 0;
		}
		case 15: {
			NzbInfo* q = (*reinterpret_cast<DownloadQueue*>(p1)->GetQueue())[(size_t)a1].get();
			strs[0] = q->GetName(); strs[1] = q->GetDupeKey();
			return 0;
		}
		case 16: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* hi = reinterpret_cast<HistoryInfo*>(p2);
			NzbInfo* nzb = hi->GetNzbInfo();
			nzb->PrintMessage(Message::mkInfo, "%s", s1);
			if (a1 == 1) nzb->GetParameters()->SetParameter("*DupeRecheck", nullptr);
			else if (a1 == 3) { IdList ids = { hi->GetId() }; dq->EditList(&ids, nullptr, DownloadQueue::mmId, DownloadQueue::eaHistoryRetryFailed, nullptr); }
			return 0;
		}
		default: return 0;
		}
	}
};

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
		nzbget_rs_queue_coord_exec(m_rs, 0, 0, 0, 0, 0, 0, nullptr, nullptr, &HostCb, this);
	}

	~QueueCoordinator() override
	{
		nzbget_rs_queue_coord_exec(m_rs, 1, 0, 0, 0, 0, 0, nullptr, nullptr, &HostCb, this);
		nzbget_rs_queue_coord_free(m_rs);
	}

	void Run() override
	{
		nzbget_rs_queue_coord_exec(m_rs, 2, 0, 0, 0, 0, 0, nullptr, nullptr, &HostCb, this);
	}

	NzbInfo* AddNzbFileToQueue(std::unique_ptr<NzbInfo> nzbInfo, NzbInfo* urlInfo, bool addFirst)
	{
		static_assert(sizeof(std::unique_ptr<NzbInfo>) == sizeof(uintptr_t), "unique_ptr layout");
		return reinterpret_cast<NzbInfo*>(nzbget_rs_queue_coord_exec(m_rs, 3, addFirst ? 1 : 0, 0,
			reinterpret_cast<uintptr_t>(&nzbInfo), reinterpret_cast<uintptr_t>(urlInfo), 0,
			nullptr, nullptr, &HostCb, this));
	}

	void CheckDupeFileInfos(NzbInfo* nzbInfo)
	{
		nzbget_rs_queue_coord_exec(m_rs, 4, 0, 0, reinterpret_cast<uintptr_t>(nzbInfo), 0, 0,
			nullptr, nullptr, &HostCb, this);
	}

	void Stop() override
	{
		Thread::Stop();
		TwinCheck::StopAll();
		nzbget_rs_queue_coord_exec(m_rs, 5, 0, 0, 0, 0, 0, nullptr, nullptr, &HostCb, this);
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
				nullptr, nullptr, &HostCb, this);
		}
	}

	void FailOverDeadPick(DownloadQueue* downloadQueue, int nzbId, int samples, int missingServers, int activeServers, int recoveredAtStart = 0)
	{
		int ints[4] = { samples, missingServers, activeServers, recoveredAtStart };
		nzbget_rs_queue_coord_exec(m_rs, 7, nzbId, 0, reinterpret_cast<uintptr_t>(downloadQueue), 0, 0,
			nullptr, ints, &HostCb, this);
	}

	static uintptr_t HostCb(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs);

protected:
	void LogDebugInfo() override
	{
		nzbget_rs_queue_coord_exec(m_rs, 10, 0, 0, 0, 0, 0, nullptr, nullptr, &HostCb, this);
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
				args, nullptr, &HostCb, m_owner) != 0;
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
				args, idsPtr, &HostCb, m_owner) != 0;
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
	private:
		QueueCoordinator* m_owner;
		bool m_massEdit = false;
		bool m_wantSave = false;
		bool m_historyChanged = false;
		bool m_stateChanged = false;
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

inline void DupeProbe::Abandon(int nzbId, int sampled, int missingServers, int activeServers, int recoveredAtStart)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	g_QueueCoordinator->FailOverDeadPick(downloadQueue, nzbId, sampled, missingServers, activeServers, recoveredAtStart);
}

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

inline uintptr_t ScriptHostCb(
	void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, const char** strs);

class ScriptJobThread final : public Thread
{
public:
	explicit ScriptJobThread(uintptr_t jobPtr) : m_jobPtr(jobPtr) {}
	void Run() override { nzbget_rs_script_run_job(m_jobPtr, &ScriptHostCb); }

private:
	uintptr_t m_jobPtr;
};

namespace ExtensionManager
{
	class Manager final
	{
	public:
		Manager() : m_rs(nzbget_rs_ext_mgr_new()) {}
		~Manager() { nzbget_rs_ext_mgr_free(m_rs); }

		Manager(const Manager&) = delete;
		Manager& operator=(const Manager&) = delete;

		NzbgetRsExtManager* Handle() const { return m_rs; }

		std::optional<std::string>
		InstallOrUpdate(int isUpdate, const fs::path& file, const char* arg2)
		{
			std::string fileStr = fs::u8string(file);
			NzbgetRsBuf errBuf{};
			int ok = nzbget_rs_ext_mgr_op(
				m_rs, 3, isUpdate, 0, 0, fileStr.c_str(), arg2 ? arg2 : "",
				nullptr, nullptr, nullptr, &ScriptHostCb, &errBuf);
			std::optional<std::string> err;
			if (!ok && errBuf.data) err.emplace(errBuf.data, errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		std::optional<std::string>
		DeleteExtension(const std::string& name)
		{
			NzbgetRsBuf errBuf{};
			int ok = nzbget_rs_ext_mgr_op(
				m_rs, 2, 0, 0, 0, name.c_str(), nullptr, nullptr, nullptr, nullptr, nullptr, &errBuf);
			std::optional<std::string> err;
			if (!ok && errBuf.data) err.emplace(errBuf.data, errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		std::optional<std::string>
		LoadExtensions()
		{
			auto createTaskCb = +[](void*, const char* taskTime, const char* extName) {
				if (g_Options)
				{
					g_Options->CreateSchedulerTask(0, taskTime, nullptr, Options::scScript, extName);
				}
			};
			NzbgetRsBuf errBuf{};
			int ok = nzbget_rs_ext_mgr_op(
				m_rs, 0, PATH_SEPARATOR, 0, reinterpret_cast<uintptr_t>(createTaskCb),
				g_Options ? g_Options->GetScriptDir() : "",
				g_Options ? g_Options->GetScriptOrder() : "",
				g_Options ? g_Options->GetExtensions() : "",
				nullptr, nullptr, &ScriptHostCb, &errBuf);
			std::optional<std::string> err;
			if (!ok && errBuf.data) err.emplace(errBuf.data, errBuf.len);
			nzbget_rs_free(errBuf);
			return err;
		}

		NzbgetRsBuf SerializeList(bool isJson) const
		{
			NzbgetRsBuf out{};
			nzbget_rs_ext_mgr_op(
				m_rs, 1, isJson ? 1 : 0, 0, 0, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, &out);
			return out;
		}

		std::pair<WebDownloader::EStatus, fs::path>
		DownloadExtension(const std::string& url, const std::string& extName)
		{
			NzbgetRsBuf pathBuf{};
			nzbget_rs_ext_mgr_op(
				nullptr, 4, PATH_SEPARATOR, 0, 0,
				g_Options ? g_Options->GetTempDir() : "",
				nullptr, nullptr, nullptr, nullptr, nullptr, &pathBuf);
			std::string tmpFileNameStr(pathBuf.data ? pathBuf.data : "", pathBuf.len);
			nzbget_rs_free(pathBuf);

			std::unique_ptr<WebDownloader> downloader = std::make_unique<WebDownloader>();
			downloader->SetUrl(url.c_str());
			downloader->SetForce(true);
			downloader->SetRetry(false);
			downloader->SetOutputFilename(tmpFileNameStr.c_str());
			downloader->SetInfoName(extName.c_str());

			WebDownloader::EStatus status = downloader->DownloadWithRedirects(5);
			downloader.reset();

			return std::make_pair(status, fs::u8path(tmpFileNameStr));
		}

	private:
		NzbgetRsExtManager* m_rs = nullptr;
	};
}

extern ExtensionManager::Manager* g_ExtensionManager;

#ifndef COMMANDSCRIPT_H
#define COMMANDSCRIPT_H

class CommandScriptLog
{
public:
	CommandScriptLog()
		: m_rs(reinterpret_cast<NzbgetRsCmdScriptLog*>(nzbget_rs_cmd_script_log_op(0, nullptr, 0, 0, nullptr)))
	{
	}
	~CommandScriptLog() { nzbget_rs_cmd_script_log_op(1, m_rs, 0, 0, nullptr); }
	CommandScriptLog(const CommandScriptLog&) = delete;
	CommandScriptLog& operator=(const CommandScriptLog&) = delete;

	const NzbgetRsCmdScriptLog* RsHandle() const { return m_rs; }
	int Reset() { return static_cast<int>(nzbget_rs_cmd_script_log_op(2, m_rs, 0, 0, nullptr)); }
	void AddMessage(int scriptId, Message::EKind kind, const char* text)
	{
		nzbget_rs_cmd_script_log_op(3, m_rs, scriptId, static_cast<int>(kind), text);
	}

private:
	NzbgetRsCmdScriptLog* m_rs = nullptr;
};

extern CommandScriptLog* g_CommandScriptLog;

#endif

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

	QueueScriptCoordinator() : m_rs(nzbget_rs_queue_script_new()) {}
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

	void InitOptions()
	{
		NzbgetRsQueueScriptCoordEnv env{};
		env.ext_mgr = g_ExtensionManager ? g_ExtensionManager->Handle() : nullptr;
		nzbget_rs_queue_script_exec(m_rs, 1, &env);
	}

	void EnqueueScript(NzbInfo* nzbInfo, EEvent event)
	{
		if (!nzbInfo || nzbInfo->GetSkipScriptProcessing()) return;
		const char* postScript = g_Options ? g_Options->GetExtensions() : "";
		if (g_Options && !Util::EmptyStr(nzbInfo->GetCategory()))
		{
			const char* strs[3]{};
			if (g_Options->QueryCategory(nzbInfo->GetCategory(), false, nullptr, strs) &&
				!Util::EmptyStr(strs[2]))
			{
				postScript = strs[2];
			}
		}
		NzbgetRsQueueScriptCoordEnv env{};
		env.ext_mgr = g_ExtensionManager ? g_ExtensionManager->Handle() : nullptr;
		env.p1 = reinterpret_cast<uintptr_t>(nzbInfo);
		env.a1 = static_cast<int>(event);
		env.a3 = nzbInfo->GetId();
		env.a4 = g_Options ? g_Options->GetEventInterval() : 0;
		env.t1 = static_cast<int64_t>(Util::CurrentTime());
		env.t2 = static_cast<int64_t>(nzbInfo->GetQueueScriptTime());
		env.s1 = postScript;
		env.host_cb = &ScriptHostCb;
		env.action_cb = &QueueScriptCoordinator::CoordActionCb;
		nzbget_rs_queue_script_exec(m_rs, 4, &env);
	}

	void CheckQueue()
	{
		if (nzbget_rs_queue_script_exec(m_rs, 6, nullptr) != 0) return;
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbgetRsQueueScriptCoordEnv env{};
		env.p1 = reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(downloadQueue));
		env.host_cb = &ScriptHostCb;
		env.action_cb = &QueueScriptCoordinator::CoordActionCb;
		nzbget_rs_queue_script_exec(m_rs, 5, &env);
	}

	bool HasJob(int nzbId, bool* active)
	{
		int act = 0;
		NzbgetRsQueueScriptCoordEnv env{};
		env.a1 = nzbId;
		env.out_i32 = &act;
		bool working = nzbget_rs_queue_script_exec(m_rs, 2, &env) != 0;
		if (active) *active = act != 0;
		return working;
	}

	int GetQueueSize() { return nzbget_rs_queue_script_exec(m_rs, 3, nullptr); }

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
	void* /*ctx*/, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* /*ints*/, const char** strs)
{
	switch (op)
	{
	case 0: {
		const char* text = s1 ? s1 : "";
		if (a1 == (int)Message::mkDetail) detail("%s", text);
		else if (a1 == (int)Message::mkInfo) info("%s", text);
		else if (a1 == (int)Message::mkWarning) warn("%s", text);
		else if (a1 == (int)Message::mkError) error("%s", text);
		else debug("%s", text);
		return 0;
	}
	case 1:
		return g_EnvironmentVariables ? reinterpret_cast<uintptr_t>(*g_EnvironmentVariables) : 0;
	case 2:
		if (a1 == 0)
		{
			strs[0] = g_Options ? g_Options->GetShellOverride() : "";
		}
		else if (a1 == 1 && g_Options)
		{
			struct OptFwd { uintptr_t p1; void (*fn)(uintptr_t, const char*, const char*); }
				fwd{p1, reinterpret_cast<void(*)(uintptr_t, const char*, const char*)>(p2)};
			nzbget_rs_options_query(g_Options->RsHandle(), 1, nullptr, 0, nullptr, nullptr,
				reinterpret_cast<uintptr_t>(&fwd),
				+[](uintptr_t raw, const char* n, const char* v, int) {
					auto* f = reinterpret_cast<OptFwd*>(raw);
					f->fn(f->p1, n, v);
				});
		}
		return 0;
	case 3: {
		auto* t = new ScriptJobThread(p1);
		t->SetAutoDestroy(true);
		t->Start();
		return 0;
	}
	case 4:
		if (g_CommandScriptLog)
		{
			g_CommandScriptLog->AddMessage(a1, (Message::EKind)a2, s1 ? s1 : "");
		}
		return 0;
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
			ints[1] = g_MemoryMonitor != nullptr ? 1 : 0;
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

class RepairController : public Thread
{
public:
	RepairController()
	{
		debug("Creating RepairController");
	#ifndef DISABLE_PARCHECK
		m_rs = nzbget_rs_repair_ctrl_new(&NzbgetCArticleFetchHost);
	#endif
	}

	~RepairController() override
	{
	#ifndef DISABLE_PARCHECK
		nzbget_rs_repair_ctrl_free(m_rs);
	#endif
	}

	void Stop() override
	{
		debug("Stopping RepairController");
		Thread::Stop();
	#ifndef DISABLE_PARCHECK
		nzbget_rs_repair_ctrl_exec(m_rs, 0, 0, 0, nullptr, nullptr);
	#endif
	}

#ifndef DISABLE_PARCHECK
	static void StartJob(PostInfo* postInfo)
	{
		RepairController* repairController = new RepairController();
		repairController->m_postInfo = postInfo;
		repairController->SetAutoDestroy(false);
		postInfo->SetPostThread(repairController);
		repairController->Start();
	}

	void Run() override { nzbget_rs_repair_ctrl_exec(m_rs, 1, 0, 0, &HostCb, this); }
	bool AddPar(FileInfo* fileInfo, bool deleted) { return nzbget_rs_repair_ctrl_exec(m_rs, 2, deleted ? 1 : 0, reinterpret_cast<uintptr_t>(fileInfo), &HostCb, this) != 0; }

private:
	PostInfo* m_postInfo = nullptr;
	NzbgetRsRepairController* m_rs = nullptr;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
	{
		auto* c = static_cast<RepairController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			PostInfo* pi = c->m_postInfo;
			ints[0] = c->IsStopped() ? 1 : 0;
			ints[1] = g_Options->GetParQuick() ? 1 : 0;
			ints[2] = g_Options->GetParTimeLimit();
			ints[3] = g_WorkState->GetPausePostProcess() ? 1 : 0;
			ints[4] = pi->GetForceParFull() ? 1 : 0;
			ints[5] = pi->GetForceRepair() ? 1 : 0;
			ints[6] = (int)pi->GetStage();
			ints[7] = g_Options->GetParBuffer();
			ints[8] = g_MemoryMonitor != nullptr ? 1 : 0;
			ints[9] = g_Options->GetParThreads();
			ints[10] = (int)g_Options->GetParScan();
			ints[11] = g_Options->GetParRepair() ? 1 : 0;
			i64s[0] = (int64_t)Util::CurrentTime();
			i64s[1] = (int64_t)pi->GetStageTime();
			i64s[2] = (int64_t)pi->GetStartTime();
			strs[0] = g_Options->GetUnrarCmd();
			strs[1] = g_Options->GetParIgnoreExt();
			return reinterpret_cast<uintptr_t>(pi->GetNzbInfo());
		}
		case 2: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p1);
			NzbParameter* tf = nzb->GetParameters()->Find(TwinCheck::FilesParam);
			ints[0] = nzb->GetId();
			ints[1] = (int)nzb->GetKind();
			ints[2] = (int)NzbInfo::nkNzb;
			ints[3] = nzb->GetDownloadSec();
			ints[4] = nzb->GetParSec();
			ints[5] = nzb->GetRepairSec();
			ints[6] = (int)nzb->GetParStatus();
			ints[7] = nzb->GetReprocess() ? 1 : 0;
			ints[8] = nzb->GetForcePriority() ? 1 : 0;
			ints[9] = nzb->GetExtraParBlocks();
			ints[10] = (int)nzb->GetFileList()->size();
			ints[11] = (int)nzb->GetCompletedFiles()->size();
			ints[12] = (int)nzb->GetStreamRepairJobs()->size();
			ints[13] = (int)nzb->GetDupeMode();
			i64s[0] = nzb->GetDupeRecoveredBytes();
			i64s[1] = nzb->GetSize() - nzb->GetParSize();
			strs[0] = nzb->GetName();
			strs[1] = nzb->GetDestDir();
			strs[2] = nzb->GetDupeKey();
			strs[3] = nzb->GetQueuedFilename();
			strs[4] = tf ? tf->GetValue() : nullptr;
			return 0;
		}
		case 4: {
			auto* fi = reinterpret_cast<FileInfo*>(p1);
			bool same = fi->GetNzbInfo() == c->m_postInfo->GetNzbInfo();
			strs[0] = fi->GetNzbInfo()->GetDestDir();
			strs[1] = fi->GetFilename();
			return same ? 1 : 0;
		}
		case 5: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p1);
			PostInfo* pi = c->m_postInfo;
			nzb->SetParStatus((NzbInfo::EParStatus)ints[0]);
			if (ints[1]) pi->SetParRepaired(ints[2] != 0);
			pi->SetStartTime(pi->GetStartTime() + (time_t)ints[3]);
			nzb->SetParSec(nzb->GetParSec() + ints[4]);
			nzb->SetParFull(ints[5] != 0);
			pi->SetWorking(false);
			return 0;
		}
		case 6: {
			FileInfo* fi = (*reinterpret_cast<NzbInfo*>(p1)->GetFileList())[(size_t)a1].get();
			if (a2 == 0)
			{
				ints[0] = fi->GetFilenameConfirmed() ? 1 : 0;
				ints[1] = fi->GetPaused() ? 1 : 0;
				ints[2] = fi->GetParFile() ? 1 : 0;
				i64s[0] = fi->GetSize();
				strs[0] = fi->GetFilename();
				return reinterpret_cast<uintptr_t>(fi);
			}
			fi->SetPaused(false);
			fi->SetExtraPriority(true);
			return 0;
		}
		case 7:
			if (a1 <= 4) c->m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a1, s1 ? s1 : "");
			else if (a1 == 5) debug("%s", s1 ? s1 : "");
			else if (a1 == 6) c->Stop();
			else if (a1 == 7) info("%s", s1 ? s1 : "");
			else if (a1 == 8) detail("%s", s1 ? s1 : "");
			return 0;
		case 8: {
			PostInfo* pi = c->m_postInfo;
			if (a1 == 0)
			{
				if (s1) pi->SetProgressLabel(s1);
				pi->SetFileProgress(ints[0]);
				pi->SetStageProgress(ints[1]);
				if (ints[2] & 1) { pi->SetStage((PostInfo::EStage)ints[3]); pi->SetStageTime((time_t)i64s[0]); }
				if (ints[2] & 8) pi->GetNzbInfo()->SetRepairSec(ints[4]);
			}
			else
			{
				if (i64s[0] > 0) pi->SetStageTime((time_t)i64s[0]);
				if (i64s[1] > 0) pi->SetStartTime((time_t)i64s[1]);
			}
			return 0;
		}
		case 9:
			if (a1 == 0) { c->m_postInfo->GetParredFiles()->push_back(s1 ? s1 : ""); return 0; }
			if (a1 == 1) return c->m_postInfo->GetParredFiles()->size();
			if (a1 == 2) { strs[0] = (*c->m_postInfo->GetParredFiles())[(size_t)a2]; return 0; }
			if (a1 == 3)
			{
				CompletedFile& cf = (*reinterpret_cast<NzbInfo*>(p1)->GetCompletedFiles())[(size_t)a2];
				ints[0] = cf.GetId();
				ints[1] = (int)cf.GetStatus();
				ints[2] = (int)cf.GetCrc();
				strs[0] = cf.GetFilename();
				strs[1] = cf.GetOrigname();
				return 0;
			}
			if (a1 == 4)
			{
				StreamRepairJob& j = (*reinterpret_cast<NzbInfo*>(p1)->GetStreamRepairJobs())[(size_t)a2];
				ints[0] = j.GetFileId();
				strs[0] = j.GetFilename();
				return 0;
			}
			if (a1 == 5)
			{
				auto* nzb = reinterpret_cast<NzbInfo*>(p1);
				nzb->GetStreamRepairJobs()->erase(nzb->GetStreamRepairJobs()->begin() + a2);
				nzb->SetChanged(true);
				return 0;
			}
			{
				FileInfo tmpFileInfo(a2);
				if (!g_DiskState->LoadFileState(&tmpFileInfo, nullptr, true)) return 0;
				auto pushFn = reinterpret_cast<void(*)(uintptr_t, int, int64_t, int, uint32_t)>(reinterpret_cast<uintptr_t>(s2));
				bool any = false;
				for (ArticleInfo* pa : tmpFileInfo.GetArticles())
				{
					pushFn(p1, pa->GetStatus() == ArticleInfo::aiFinished ? 1 : 0, pa->GetSegmentOffset(), pa->GetSegmentSize(), pa->GetCrc());
					any = true;
				}
				return any ? 2 : 1;
			}
		case 10:
			if (a1 == 0)
			{
				auto* dq = reinterpret_cast<DownloadQueue*>(p1);
				ints[0] = (int)dq->GetQueue()->size();
				ints[1] = (int)dq->GetHistory()->size();
				return 0;
			}
			if (a1 == 1) return reinterpret_cast<uintptr_t>((*reinterpret_cast<DownloadQueue*>(p1)->GetQueue())[(size_t)a2].get());
			if (a1 == 2)
			{
				HistoryInfo* h = (*reinterpret_cast<DownloadQueue*>(p1)->GetHistory())[(size_t)a2].get();
				return h->GetKind() == HistoryInfo::hkNzb ? reinterpret_cast<uintptr_t>(h->GetNzbInfo()) : 0;
			}
			if (a1 == 7) { reinterpret_cast<NzbInfo*>(p1)->SetExtraParBlocks(a2); return 0; }
			return 0;
		default:
			return 0;
		}
	}
#endif
};

class StreamRepairController : public Thread
{
public:
	static void StartJob(PostInfo* postInfo)
	{
		StreamRepairController* ctrl = new StreamRepairController(false, 0);
		ctrl->m_postInfo = postInfo;
		ctrl->SetAutoDestroy(false);
		postInfo->SetPostThread(ctrl);
		ctrl->Start();
	}

	static void StartLive(NzbInfo* nzbInfo)
	{
		if (DupeArticleFallback::ShouldDeferToPar(nzbInfo))
		{
			return;
		}
		StreamRepairController* ctrl = new StreamRepairController(true, nzbInfo->GetId());
		ctrl->SetAutoDestroy(true);
		nzbInfo->SetLiveRepairThread(ctrl);
		ctrl->Start();
	}

	~StreamRepairController() override
	{
		nzbget_rs_stream_repair_ctrl_free(m_rs);
	}

	void Run() override
	{
		nzbget_rs_stream_repair_ctrl_exec(m_rs, 1, &HostCb, this);
	}

	void Stop() override
	{
		nzbget_rs_stream_repair_ctrl_exec(m_rs, 0, nullptr, nullptr);
		Thread::Stop();
	}

private:
	StreamRepairController(bool liveMode, int nzbId) : m_liveMode(liveMode), m_nzbId(nzbId)
	{
		static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange), "StreamRange ABI match");
	#ifndef DISABLE_TLS
		constexpr int tlsEnabled = 1;
	#else
		constexpr int tlsEnabled = 0;
	#endif
	#ifdef DEBUG
		constexpr int debugEnabled = 1;
	#else
		constexpr int debugEnabled = 0;
	#endif
		m_rs = nzbget_rs_stream_repair_ctrl_new(liveMode ? 1 : 0, nzbId,
			&NzbgetCArticleFetchHost, &DupeProbe::HostCb, tlsEnabled, debugEnabled);
	}

	PostInfo* m_postInfo = nullptr;
	bool m_liveMode = false;
	int m_nzbId = 0;
	NzbgetRsStreamRepairCtrl* m_rs = nullptr;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, int* ints, int64_t* i64s, const char** strs)
	{
		auto* self = static_cast<StreamRepairController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: return self->IsStopped() ? 1 : 0;
		case 2: self->Stop(); return 0;
		case 3: return 0;
		case 4: {
			const char* msg = s1 ? s1 : "";
			if (self->m_liveMode)
			{
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				if (NzbInfo* nzb = dq->GetQueue()->Find(self->m_nzbId))
					nzb->AddMessage((Message::EKind)a1, msg);
			}
			else if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
			{
				self->m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a1, msg);
			}
			return 0;
		}
		case 5:
			if (!s1) return 0;
			if (a1 == 0) info("%s", s1);
			else if (a1 == 1) error("%s", s1);
			else if (a1 == 2) warn("%s", s1);
			else if (a1 == 3) detail("%s", s1);
			else if (a1 == 4) debug("%s", s1);
			return 0;
		case 6: {
			int primaryConnections = 0;
			for (NewsServer* s : g_ServerPool->GetServers())
				if (s->GetActive() && s->GetNormLevel() == 0) primaryConnections += s->GetMaxConnections();
			int upTimeSec = 0, downloadTimeSec = 0;
			int64 allBytes = 0;
			bool standBy = false;
			g_StatMeter->CalcTotalStat(&upTimeSec, &downloadTimeSec, &allBytes, &standBy);
			ints[0] = g_Options->GetDupeStreamTimeout();
			ints[1] = g_Options->GetDupeStreamDecompress() ? 1 : 0;
			ints[2] = primaryConnections;
			ints[3] = (int)g_Options->GetParCheck();
			ints[4] = (int)Options::pcManual;
			i64s[0] = downloadTimeSec > 0 ? allBytes / downloadTimeSec : 0;
			strs[0] = g_Options->GetParIgnoreExt();
			return 0;
		}
		case 7: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			NzbInfo* nzb = nullptr;
			if (self->m_liveMode)
			{
				nzb = dq->GetQueue()->Find(self->m_nzbId);
				if (!nzb || nzb->GetLiveRepairThread() != self) return 0;
			}
			else
			{
				nzb = self->m_postInfo ? self->m_postInfo->GetNzbInfo() : nullptr;
				if (!nzb) return 0;
				if (a1) self->m_postInfo->SetProgressLabel("Repairing files from duplicates");
			}
			ints[0] = nzb->GetId();
			ints[1] = nzb->GetParSize() > 0 ? 1 : 0;
			ints[2] = (int)nzb->GetCompletedFiles()->size();
			ints[3] = (int)nzb->GetStreamRepairJobs()->size();
			ints[4] = (int)dq->GetQueue()->size();
			ints[5] = (int)dq->GetHistory()->size();
			i64s[0] = nzb->GetFullContentHash();
			strs[0] = nzb->GetName();
			strs[1] = nzb->GetDestDir();
			strs[2] = nzb->GetDupeKey();
			NzbParameter* pw = nzb->GetParameters()->Find("*Unpack:Password");
			strs[3] = pw ? pw->GetValue() : nullptr;
			return reinterpret_cast<uintptr_t>(nzb);
		}
		case 8: {
			CompletedFile& cf = (*reinterpret_cast<NzbInfo*>(p1)->GetCompletedFiles())[(size_t)a1];
			ints[0] = cf.GetId();
			ints[1] = cf.GetParFile() ? 1 : 0;
			ints[2] = (int)cf.GetStatus();
			strs[0] = cf.GetFilename();
			return 0;
		}
		case 9: {
			StreamRepairJob& job = (*reinterpret_cast<NzbInfo*>(p1)->GetStreamRepairJobs())[(size_t)a1];
			if (a2 == 1) { bool prev = job.GetLiveAttempted(); job.SetLiveAttempted(true); return prev ? 1 : 0; }
			ints[0] = job.GetFileId();
			ints[1] = job.GetFailedArticles();
			ints[2] = job.GetParFile() ? 1 : 0;
			ints[3] = job.GetLiveAttempted() ? 1 : 0;
			ints[4] = (int)job.GetHoles()->size();
			i64s[0] = job.GetDecodedFileSize();
			i64s[1] = job.GetFailedSize();
			i64s[2] = job.GetMissedSize();
			i64s[3] = (int64_t)job.GetStepsHash();
			strs[0] = job.GetFilename();
			strs[1] = job.GetHoles()->empty() ? nullptr : reinterpret_cast<const char*>(job.GetHoles()->data());
			return 0;
		}
		case 10: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			NzbInfo* cand = nullptr;
			if (a1 == 0)
			{
				cand = (*dq->GetQueue())[(size_t)a2].get();
				if (!cand || cand->GetKind() != NzbInfo::nkNzb) return 0;
			}
			else
			{
				HistoryInfo* h = (*dq->GetHistory())[(size_t)a2].get();
				if (!h || h->GetKind() != HistoryInfo::hkNzb) return 0;
				cand = h->GetNzbInfo();
			}
			ints[0] = cand->GetId();
			ints[1] = cand->GetDupeScore();
			ints[2] = (int)cand->GetDupeMode();
			i64s[0] = cand->GetFullContentHash();
			strs[0] = cand->GetName();
			strs[1] = cand->GetDupeKey();
			strs[2] = cand->GetQueuedFilename();
			NzbParameter* pw = cand->GetParameters()->Find("*Unpack:Password");
			strs[3] = pw ? pw->GetValue() : nullptr;
			return reinterpret_cast<uintptr_t>(cand);
		}
		case 11:
			if (self->m_postInfo)
			{
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				self->m_postInfo->SetProgressLabel(s1 ? s1 : "");
				self->m_postInfo->SetStageProgress(a1);
			}
			return 0;
		case 12: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p1);
			for (CompletedFile& cf : nzb->GetCompletedFiles())
			{
				if (cf.GetId() == a1)
				{
					if (a2 == 1 && cf.GetStatus() != CompletedFile::cfSuccess)
					{
						cf.SetStatus(CompletedFile::cfSuccess);
						g_DiskState->DiscardFile(a1, true, true, true);
					}
					else if (a2 == 2 && cf.GetStatus() == CompletedFile::cfFailure)
					{
						cf.SetStatus(CompletedFile::cfPartial);
					}
					break;
				}
			}
			return 0;
		}
		case 13: {
			auto* jobs = reinterpret_cast<NzbInfo*>(p1)->GetStreamRepairJobs();
			if (a2 == 0) { jobs->erase(jobs->begin() + a1); return 0; }
			StreamRepairJob& job = (*jobs)[(size_t)a1];
			if (i64s[0] >= 0) job.SetDecodedFileSize(i64s[0]);
			const auto* ranges = reinterpret_cast<const StreamRange*>(p2);
			job.SetHoles(ranges && ints[0] > 0 ? StreamRangeList(ranges, ranges + ints[0]) : StreamRangeList());
			return 0;
		}
		case 14: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			if (a2 == 0)
			{
				ints[0] = (int)nzb->GetParRenameStatus();
				ints[1] = (int)nzb->GetParStatus();
				i64s[0] = nzb->GetFailedSize();
				i64s[1] = nzb->GetSuccessSize();
				i64s[2] = nzb->GetCurrentFailedSize();
				i64s[3] = nzb->GetCurrentSuccessSize();
				i64s[4] = nzb->GetParFailedSize();
				i64s[5] = nzb->GetParSuccessSize();
				i64s[6] = nzb->GetParCurrentFailedSize();
				i64s[7] = nzb->GetParCurrentSuccessSize();
				return 0;
			}
			if (ints[0] > 0 || i64s[8] > 0 || ints[1] > 0)
			{
				nzb->SetDupeRecoveredArticles(nzb->GetDupeRecoveredArticles() + ints[0]);
				nzb->SetDupeRecoveredBytes(nzb->GetDupeRecoveredBytes() + i64s[8]);
				nzb->SetDupeRecoveredHoles(nzb->GetDupeRecoveredHoles() + ints[1]);
			}
			if (a1 != 0)
			{
				nzb->SetFailedSize(i64s[0]);
				nzb->SetSuccessSize(i64s[1]);
				nzb->SetCurrentFailedSize(i64s[2]);
				nzb->SetCurrentSuccessSize(i64s[3]);
				nzb->SetParFailedSize(i64s[4]);
				nzb->SetParSuccessSize(i64s[5]);
				nzb->SetParCurrentFailedSize(i64s[6]);
				nzb->SetParCurrentSuccessSize(i64s[7]);
			}
			PostInfo* pi = self->m_postInfo;
			pi->SetStreamRepairDone(true);
			pi->SetStreamRepairRecovered(i64s[8] > 0);
			int parFlags = ints[2];
			if (parFlags & 1) nzb->SetParRenameStatus(NzbInfo::rsNone);
			if (parFlags & 2) nzb->SetParStatus((NzbInfo::EParStatus)ints[3]);
			if (parFlags & 4) pi->SetForceParFull(true);
			if (parFlags & 8) pi->SetRequestParCheck(false);
			if (parFlags & 16) pi->SetRequestParCheck(true);
			dq->Save();
			pi->SetWorking(false);
			return 0;
		}
		case 15: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			if (a2 == 0)
			{
				NzbInfo* nzb = dq->GetQueue()->Find(self->m_nzbId);
				if (!nzb) return 0;
				if (nzb->GetLiveRepairThread() != self)
				{
					nzb->SetDupeRecoveredBytes(nzb->GetDupeRecoveredBytes() + i64s[0]);
					return 0;
				}
				if (i64s[0] > 0 || ints[1] > 0)
				{
					nzb->SetDupeRecoveredBytes(nzb->GetDupeRecoveredBytes() + i64s[0]);
					nzb->SetDupeRecoveredHoles(nzb->GetDupeRecoveredHoles() + ints[1]);
				}
				ints[0] = (int)nzb->GetStreamRepairJobs()->size();
				return reinterpret_cast<uintptr_t>(nzb);
			}
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			nzb->SetLiveRepairThread(nullptr);
			if (nzb->GetPostInfo() && !nzb->GetUnpackThread()) nzb->GetPostInfo()->SetWorking(false);
			dq->Save();
			return 0;
		}
		default:
			return 0;
		}
	}
};

#include <new>
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2018 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
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


#ifndef UNPACK_CONTROLLER_H
#define UNPACK_CONTROLLER_H

class UnpackController final : public Thread
{
public:
	UnpackController() : m_rs(nzbget_rs_unpack_ctrl_new()) {}

	~UnpackController() override
	{
		nzbget_rs_unpack_ctrl_free(m_rs);
#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}

	static void StartJob(PostInfo* postInfo)
	{
		UnpackController* unpackController = new UnpackController();
		unpackController->m_postInfo = postInfo;
		unpackController->SetAutoDestroy(false);
		postInfo->SetPostThread(unpackController);
		unpackController->Start();
	}

	void Run() override { nzbget_rs_unpack_ctrl_exec(m_rs, 0, &HostCb, this); }

	void Stop() override
	{
		debug("Stopping unpack");
		Thread::Stop();
		nzbget_rs_unpack_ctrl_exec(m_rs, 1, nullptr, nullptr);
	}

private:
	PostInfo* m_postInfo = nullptr;
	NzbgetRsUnpackController* m_rs = nullptr;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* /*s2*/, int* ints, int64_t* i64s, const char** strs)
	{
		auto* c = static_cast<UnpackController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			PostInfo* pi = c->m_postInfo;
			NzbInfo* n = pi->GetNzbInfo();
			if (a1 == 0)
			{
				NzbParameter* up = n->GetParameters()->Find("*Unpack:");
				NzbParameter* pw = n->GetParameters()->Find("*Unpack:Password");
				ints[0] = !(up && !strcasecmp(up->GetValue(), "no")) ? 1 : 0;
				ints[1] = pw ? 1 : 0;
				ints[2] = (int)n->GetCompletedFiles()->size();
				ints[3] = pi->GetExtractedArchives() ? (int)pi->GetExtractedArchives()->size() : 0;
				ints[4] = pi->GetUnpackTried() ? 1 : 0;
				ints[5] = pi->GetParRepaired() ? 1 : 0;
				ints[6] = pi->GetPassListTried() ? 1 : 0;
				ints[7] = pi->GetLastUnpackStatus();
				ints[8] = (int)n->GetDirectUnpackStatus();
				ints[9] = (int)n->GetParStatus();
				ints[10] = (int)n->GetParRenameStatus();
				ints[11] = 0;
				ints[12] = (g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0;
				ints[13] = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
				ints[14] = g_Options ? g_Options->GetWriteBuffer() : 0;
				ints[15] = (g_Options && g_Options->GetParRename()) ? 1 : 0;
				ints[16] = static_cast<int>(static_cast<uint32_t>(FileSystem::uMask));
			#ifndef DISABLE_PARCHECK
				ints[17] = 1;
			#else
				ints[17] = 0;
			#endif
				strs[0] = n->GetDestDir();
				strs[1] = n->GetFinalDir();
				strs[2] = n->GetName();
				strs[3] = pw ? pw->GetValue() : nullptr;
				strs[4] = g_Options ? g_Options->GetUnpackPassFile() : "";
				strs[5] = g_Options ? g_Options->GetUnpackIgnoreExt() : "";
				strs[6] = g_Options ? g_Options->GetInterDir() : "";
				return 0;
			}
			if (a1 == 1) { strs[0] = (*n->GetCompletedFiles())[(size_t)a2].GetFilename(); return 0; }
			if (a1 == 2) { strs[0] = *(*pi->GetExtractedArchives())[(size_t)a2]; return 0; }
			if (a1 == 3 && pi->GetExtractedArchives()) pi->GetExtractedArchives()->clear();
			return 0;
		}
		case 2:
			if (a1 == 0) return c->IsStopped() ? 1 : 0;
			if (a1 == 1) { c->Stop(); return 0; }
			if (a1 == 2) { c->Thread::Resume(); return 0; }
			if (a1 == 3) { if (c->m_postInfo) c->m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a2, s1 ? s1 : ""); return 0; }
			if (a1 == 4) { GuardedDownloadQueue dq = DownloadQueue::Guard(); c->m_postInfo->SetProgressLabel(s1 ? s1 : ""); return 0; }
			if (a1 == 5) { if (c->m_postInfo) c->m_postInfo->SetStageProgress(a2); return 0; }
			if (a1 == 6) { if (c->m_postInfo) c->m_postInfo->SetStageTime((time_t)i64s[0]); return 0; }
			if (a1 == 7) { if (a2 == 3) debug("%s", s1 ? s1 : ""); else if (a2 == 4) detail("%s", s1 ? s1 : ""); return 0; }
			if (a1 == 8) return (c->m_postInfo && c->m_postInfo->GetDeleted()) ? 1 : 0;
			return 0;
		case 3: {
			PostInfo* pi = c->m_postInfo;
			NzbInfo* n = pi->GetNzbInfo();
			if (a1 == 0) { n->SetUnpackStatus((NzbInfo::EPostUnpackStatus)a2); return 0; }
			if (a1 == 1)
			{
				n->SetUnpackStatus(NzbInfo::usSuccess);
				n->SetUnpackCleanedUpDisk(a2 != 0);
				if (p1) n->SetParRenameStatus(NzbInfo::rsNone);
				return 0;
			}
			if (a1 == 2)
			{
				pi->SetRequestParCheck(true);
				pi->SetForceRepair(ints[0] != 0);
				pi->SetUnpackTried(true);
				pi->SetPassListTried(ints[1] != 0);
				pi->SetLastUnpackStatus(ints[2]);
				return 0;
			}
			if (a1 == 3)
			{
				n->SetUnpackSec(n->GetUnpackSec() + a2);
				pi->SetWorking(false);
				return 0;
			}
			if (a1 == 4)
			{
				ints[0] = (int)n->GetParStatus();
				ints[1] = n->GetParFull() ? 1 : 0;
				return 0;
			}
			if (a1 == 5)
			{
				CString fin = n->BuildFinalDirName();
				reinterpret_cast<void(*)(uintptr_t, const char*)>(p2)(p1, fin ? *fin : "");
				return 0;
			}
			if (a1 == 6) n->SetFinalDir(s1 ? s1 : "");
			return 0;
		}
		default:
			return 0;
		}
	}
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

#endif

class MoveController : public Thread
{
public:
	static void StartJob(PostInfo* postInfo)
	{
		MoveController* moveController = new MoveController();
		moveController->m_postInfo = postInfo;
		moveController->SetAutoDestroy(false);
		postInfo->SetPostThread(moveController);
		moveController->Start();
	}

	void Run() override { nzbget_rs_post_job_exec(0, 0, 0, &HostCb, this); }

private:
	PostInfo* m_postInfo = nullptr;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* /*ints*/, int64_t* /*i64s*/, const char** strs)
	{
		auto* c = static_cast<MoveController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			if (a1 == 2) return (c->m_postInfo && c->m_postInfo->GetNzbInfo()) ? 1 : 0;
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			if (a1 == 1) { strs[0] = n->GetHardLinkPath().c_str(); return 0; }
			strs[0] = n->GetName();
			strs[1] = n->GetDestDir();
			strs[2] = n->GetFinalDir();
			if (Util::EmptyStr(strs[2]))
			{
				CString fin = n->BuildFinalDirName();
				reinterpret_cast<void(*)(uintptr_t, const char*)>(p2)(p1, fin ? *fin : "");
			}
			return 0;
		}
		case 2:
			if (a1 == 0) return c->IsStopped() ? 1 : 0;
			if (a1 == 1) { c->m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a2, s1 ? s1 : ""); return 0; }
			if (a1 == 2 && c->m_postInfo && c->m_postInfo->GetNzbInfo())
				c->m_postInfo->GetNzbInfo()->RenameCompletedFile(s1 ? s1 : "", s2 ? s2 : "");
			return 0;
		case 3:
			if (a1 == 0) c->m_postInfo->GetNzbInfo()->SetMoveStatus((NzbInfo::EMoveStatus)a2);
			else if (a1 == 1)
			{
				NzbInfo* n = c->m_postInfo->GetNzbInfo();
				n->SetDestDir(s1 ? s1 : "");
				n->SetFinalDir("");
				n->SetMoveStatus((NzbInfo::EMoveStatus)a2);
			}
			else if (a1 == 2) c->m_postInfo->SetWorking(false);
			return 0;
		default:
			return 0;
		}
	}
};

class CleanupController : public Thread
{
public:
	static void StartJob(PostInfo* postInfo)
	{
		CleanupController* cleanupController = new CleanupController();
		cleanupController->m_postInfo = postInfo;
		cleanupController->SetAutoDestroy(false);
		postInfo->SetPostThread(cleanupController);
		cleanupController->Start();
	}

	void Run() override { nzbget_rs_post_job_exec(1, 0, 0, &HostCb, this); }

private:
	PostInfo* m_postInfo = nullptr;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* /*s2*/, int* /*ints*/, int64_t* /*i64s*/, const char** strs)
	{
		auto* c = static_cast<CleanupController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			strs[0] = n->GetName();
			strs[1] = n->GetDestDir();
			strs[2] = n->GetFinalDir();
			strs[3] = g_Options ? g_Options->GetExtCleanupDisk() : "";
			return 0;
		}
		case 2:
			if (a1 == 1) c->m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a2, s1 ? s1 : "");
			return 0;
		case 3:
			c->m_postInfo->GetNzbInfo()->SetCleanupStatus((NzbInfo::ECleanupStatus)a2);
			c->m_postInfo->SetWorking(false);
			return 0;
		default:
			return 0;
		}
	}
};

class RenameController : public Thread
{
public:
	enum EJobKind
	{
		jkPar,
		jkRar
	};

	RenameController() { debug("Creating RenameController"); }

	static void StartJob(PostInfo* postInfo, EJobKind kind)
	{
		RenameController* renameController = new RenameController();
		renameController->m_postInfo = postInfo;
		renameController->m_kind = kind;
		renameController->SetAutoDestroy(false);
		postInfo->SetPostThread(renameController);
		renameController->Start();
	}

	void Run() override
	{
		int flags = 0;
	#ifndef DISABLE_PARCHECK
		flags |= 1;
	#endif
	#ifndef DISABLE_TLS
		flags |= 2;
	#endif
		nzbget_rs_post_job_exec(2, (int)m_kind, flags, &HostCb, this);
	}

private:
	PostInfo* m_postInfo = nullptr;
	EJobKind m_kind = jkPar;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
	{
		auto* c = static_cast<RenameController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			if (a1 == 1)
			{
				ints[0] = n->GetRemainingParCount();
				ints[1] = (int)n->GetParStatus();
				return 0;
			}
			NzbParameter* pw = n->GetParameters()->Find("*Unpack:Password");
			ints[0] = pw ? 1 : 0;
			ints[1] = (int)n->GetUnpackStatus();
			strs[0] = n->GetName();
			strs[1] = n->GetDestDir();
			strs[2] = n->GetFinalDir();
			strs[3] = pw ? pw->GetValue() : nullptr;
			strs[4] = g_Options ? g_Options->GetParIgnoreExt() : "";
			strs[5] = g_Options ? g_Options->GetUnpackIgnoreExt() : "";
			return 0;
		}
		case 2:
			if (a1 == 0) return c->IsStopped() ? 1 : 0;
			if (a1 == 1) { c->m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a2, s1 ? s1 : ""); return 0; }
			if (a1 == 2) return c->m_postInfo->GetNzbInfo()->RenameCompletedFile(s1 ? s1 : "", s2 ? s2 : "") ? 1 : 0;
			if (a1 == 3) { c->m_postInfo->SetProgressLabel(s1 ? s1 : ""); c->m_postInfo->SetStageProgress(a2); return 0; }
			if (a1 == 4) c->m_postInfo->GetParredFiles()->push_back(s1 ? s1 : "");
			return 0;
		case 3: {
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			if (a1 == 1)
			{
				auto* dq = reinterpret_cast<DownloadQueue*>(p1);
				n->SetParRenameStatus((NzbInfo::EPostRenameStatus)a2);
				dq->EditEntry(n->GetId(), DownloadQueue::eaGroupResume, nullptr);
				dq->EditEntry(n->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
				i64s[0] = n->GetRemainingSize();
				return 0;
			}
			if (a2 == (int)jkPar) n->SetParRenameStatus((NzbInfo::EPostRenameStatus)p1);
			else if (a2 == (int)jkRar) n->SetRarRenameStatus((NzbInfo::EPostRenameStatus)p1);
		#ifndef DISABLE_PARCHECK
			if (p2) c->m_postInfo->SetRequestParCheck(true);
		#endif
			c->m_postInfo->SetWorking(false);
			return 0;
		}
		default:
			return 0;
		}
	}
};

class PostRenamerController final : public Thread
{
public:
	static void StartJob(PostInfo* postInfo, bool postUnpack)
	{
		PostRenamerController* controller = new (std::nothrow) PostRenamerController();
		if (!controller)
		{
			error("Failed to allocate memory for PostRenamerController");
			return;
		}
		controller->m_postInfo = postInfo;
		controller->m_postUnpack = postUnpack;
		controller->SetAutoDestroy(false);
		postInfo->SetPostThread(controller);
		controller->Start();
	}

	void Run() override { nzbget_rs_post_job_exec(3, m_postUnpack ? 1 : 0, 0, &HostCb, this); }

private:
	PostInfo* m_postInfo = nullptr;
	bool m_postUnpack = false;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* /*ints*/, int64_t* /*i64s*/, const char** strs)
	{
		auto* c = static_cast<PostRenamerController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			strs[0] = n->GetMetaName().c_str();
			strs[1] = n->GetName();
			strs[2] = n->GetDestDir();
			strs[3] = g_Options ? g_Options->GetRenameIgnoreExt() : "";
			return 0;
		}
		case 2:
			if (a1 == 0) return c->IsStopped() ? 1 : 0;
			if (a1 == 1) { c->m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a2, s1 ? s1 : ""); return 0; }
			if (a1 == 2 && c->m_postInfo && c->m_postInfo->GetNzbInfo())
				c->m_postInfo->GetNzbInfo()->RenameCompletedFile(s1 ? s1 : "", s2 ? s2 : "");
			return 0;
		case 3:
			if (a1) c->m_postInfo->GetNzbInfo()->SetPostUnpackRenamingStatus((NzbInfo::PostUnpackRenamingStatus)a2);
			else c->m_postInfo->GetNzbInfo()->SetPostDownloadRenamingStatus((NzbInfo::PostDownloadRenamingStatus)a2);
			c->m_postInfo->SetWorking(false);
			return 0;
		default:
			return 0;
		}
	}
};

class PostScriptController : public Thread
{
public:
	PostScriptController() : m_runner(nzbget_rs_script_runner_new()) {}
	~PostScriptController() override { nzbget_rs_script_runner_free(m_runner); }

	static void StartJob(PostInfo* postInfo)
	{
		if (!postInfo || (postInfo->GetNzbInfo() && postInfo->GetNzbInfo()->GetSkipScriptProcessing()))
			return;
		PostScriptController* scriptController = new PostScriptController();
		scriptController->m_postInfo = postInfo;
		scriptController->SetAutoDestroy(false);
		postInfo->SetPostThread(scriptController);
		scriptController->Start();
	}

	void Run() override
	{
		int parcheckEnabled = 1;
#ifdef DISABLE_PARCHECK
		parcheckEnabled = 0;
#endif
		nzbget_rs_post_script_run(
			m_runner, g_ExtensionManager ? g_ExtensionManager->Handle() : nullptr,
			parcheckEnabled, this, &HostCb, &ScriptHostCb);
	}

	void Stop() override
	{
		debug("Stopping post-process-script");
		Thread::Stop();
		nzbget_rs_script_runner_terminate(m_runner, &ScriptHostCb);
	}

private:
	PostInfo* m_postInfo = nullptr;
	NzbgetRsScriptRunner* m_runner = nullptr;

	static uintptr_t HostCb(void* rawCtx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
	{
		auto* c = static_cast<PostScriptController*>(rawCtx);
		switch (op)
		{
		case 0: {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, reinterpret_cast<uintptr_t>((DownloadQueue*)dq));
			return 0;
		}
		case 1: {
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			if (a1 == 0) { n->GetScriptStatuses()->clear(); return n->GetParameters()->size(); }
			if (a1 == 1)
			{
				NzbParameter& p = (*n->GetParameters())[(size_t)a2];
				strs[0] = p.GetName();
				strs[1] = p.GetValue();
				return 0;
			}
			c->m_postInfo->SetStage(PostInfo::ptFinished);
			c->m_postInfo->SetWorking(false);
			return 0;
		}
		case 2:
			return (a1 == 0 && c->IsStopped()) ? 1 : 0;
		case 4: {
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			if (a1 == 0)
			{
				ints[0] = c->m_postInfo->GetRequestParCheck() ? 1 : 0;
				strs[0] = n->GetName();
				return 0;
			}
			if (a1 == 1) { c->m_postInfo->SetProgressLabel(s1 ? s1 : ""); return 0; }
			if (a1 == 3)
			{
				BString<1024> st = n->MakeTextStatus(true);
				reinterpret_cast<void(*)(uintptr_t, const char*)>(p2)(p1, *st);
				ints[0] = n->GetId();
				ints[1] = n->CalcHealth();
				ints[2] = n->CalcCriticalHealth(false);
				ints[3] = n->GetDupeScore();
				ints[4] = (int)n->GetDupeMode();
				ints[5] = (int)n->GetScriptStatuses()->CalcTotalStatus();
				ints[6] = (int)n->GetParStatus();
				ints[7] = (int)n->GetUnpackStatus();
				ints[8] = (int)n->GetDeleteStatus();
				ints[9] = (int)n->GetMarkStatus();
				ints[10] = n->GetTotalArticles();
				ints[11] = n->GetSuccessArticles();
				ints[12] = (int)n->GetServerStats()->size();
				ints[13] = (int)n->GetParameters()->size();
				strs[0] = n->GetName();
				strs[1] = n->GetDestDir();
				strs[2] = n->GetFilename();
				strs[3] = n->GetQueuedFilename();
				strs[4] = n->GetUrl();
				strs[5] = n->GetFinalDir();
				strs[6] = n->GetCategory();
				strs[7] = n->GetDupeKey();
				return 0;
			}
			if (a1 == 4)
			{
				ServerStat& ss = (*n->GetServerStats())[(size_t)a2];
				ints[0] = ss.GetServerId();
				ints[1] = ss.GetSuccessArticles();
				ints[2] = ss.GetFailedArticles();
				return 0;
			}
			if (a1 == 5) return (uintptr_t)n->GetFailedArticles();
			if (a1 == 9) return (uintptr_t)n->GetParStatus();
			if (a1 == 10)
			{
				if (p1) { c->m_postInfo->SetRequestParCheck(true); c->m_postInfo->SetForceRepair(true); }
				n->GetScriptStatuses()->emplace_back(s1 ? s1 : "", (ScriptStatus::EStatus)a2);
			}
			return 0;
		}
		case 5: {
			NzbInfo* n = c->m_postInfo->GetNzbInfo();
			if (a1 == 2)
			{
				if (a2 == 1) n->SetFinalDir(s1 ? s1 : "");
				else if (a2 == 2) n->SetDestDir(s1 ? s1 : "");
				else if (a2 == 3) n->GetParameters()->SetParameter(s1 ? s1 : "", s2 ? s2 : "");
				return 0;
			}
			if (a1 == 3)
			{
				n->AddMessage((Message::EKind)a2, s1 ? s1 : "");
				return 0;
			}
			if (a1 == 4)
			{
				if (a2 == 1) n->SetMarkStatus(NzbInfo::ksBad);
				else c->m_postInfo->SetProgressLabel(s1 ? s1 : "");
				return 0;
			}
			if (a1 == 5)
			{
				if (!g_WorkState->GetPausePostProcess() || n->GetForcePriority()) return 0;
				if (i64s)
				{
					i64s[0] = (int64_t)c->m_postInfo->GetStageTime();
					i64s[1] = (int64_t)c->m_postInfo->GetStartTime();
				}
				return 1;
			}
			if (a1 == 6)
			{
				if (i64s[0] > 0) c->m_postInfo->SetStageTime((time_t)i64s[0]);
				if (i64s[1] > 0) c->m_postInfo->SetStartTime((time_t)i64s[1]);
			}
			return 0;
		}
		default:
			return 0;
		}
	}
};

class PrePostProcessor : public Thread, public Observer
{
public:
	bool HasMoreJobs()
	{
		return nzbget_rs_prepost_exec(m_rs, 2, 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb) != 0;
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
		nzbget_rs_prepost_exec(m_rs, 0, 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb);
	}

	void Stop() override
	{
		Thread::Stop();
		nzbget_rs_prepost_exec(m_rs, 1, 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb);
	}

	bool HasActiveUnpack(bool includeWaitingVolume = true)
	{
		return nzbget_rs_prepost_exec(m_rs, 3, includeWaitingVolume ? 1 : 0, 0, 0, 0, 0, nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb) != 0;
	}

	void NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		nzbget_rs_prepost_exec(m_rs, 4, 0, 0, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), 0, nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb);
	}

	void CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		nzbget_rs_prepost_exec(m_rs, 5, 0, 0, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), 0, nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb);
	}

	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
	{
		nzbget_rs_prepost_exec(m_rs, 6, 0, 0, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo), 0, nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb);
	}

	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action, const char* args)
	{
		const int* ptr = (idList && !idList->empty()) ? idList->data() : nullptr;
		int len = idList ? (int)idList->size() : 0;
		return nzbget_rs_prepost_exec(m_rs, 7, len, (int)action, reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(ptr), 0, args, g_QueueCoordinator, &QueueCoordinator::HostCb) != 0;
	}

protected:
	void Update(Subject*, void* aspect) override
	{
		if (!aspect) return;
		DownloadQueue::Aspect* qa = static_cast<DownloadQueue::Aspect*>(aspect);
		nzbget_rs_prepost_exec(m_rs, 8, (int)qa->action, 0, reinterpret_cast<uintptr_t>(qa->downloadQueue), reinterpret_cast<uintptr_t>(qa->nzbInfo), reinterpret_cast<uintptr_t>(qa->fileInfo), nullptr, g_QueueCoordinator, &QueueCoordinator::HostCb);
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

	Scanner() : m_rsScanner(nzbget_rs_scanner_new()) {}
	~Scanner() override { nzbget_rs_scanner_free(m_rsScanner); }
	Scanner(const Scanner&) = delete;
	Scanner& operator=(const Scanner&) = delete;

	void InitOptions()
	{
		std::vector<const char*> catExts;
		if (g_Options)
		{
			nzbget_rs_options_query(g_Options->RsHandle(), 2, nullptr, 0, nullptr, nullptr,
				reinterpret_cast<uintptr_t>(&catExts),
				+[](uintptr_t raw, const char*, const char* ext, int) {
					reinterpret_cast<std::vector<const char*>*>(raw)->push_back(ext);
				});
		}
		int hasScripts = (g_ExtensionManager && g_Options)
			? nzbget_rs_ext_mgr_op(
				g_ExtensionManager->Handle(), 6, static_cast<int>(catExts.size()), 0, 0,
				g_Options->GetExtensions(), nullptr, nullptr,
				catExts.empty() ? nullptr : catExts.data(), nullptr, nullptr, nullptr)
			: 0;
		NzbgetRsScannerEnv env = MakeEnv();
		nzbget_rs_scanner_exec(m_rsScanner, 0, &env, hasScripts != 0 ? 1 : 0, 0, 0);
	}

	void InitPPParameters(const char* category, NzbParameterList* parameters, bool reset)
	{
		bool unpack = g_Options->GetUnpack();
		const char* extensions = g_Options->GetExtensions();
		if (!Util::EmptyStr(category))
		{
			int catUnpack = 0;
			const char* strs[3]{};
			if (g_Options->QueryCategory(category, false, &catUnpack, strs))
			{
				unpack = catUnpack != 0;
				if (!Util::EmptyStr(strs[2])) extensions = strs[2];
			}
		}
		auto paramCb = +[](void* raw, int op, const char* name, const char* val) -> int {
			auto* params = static_cast<NzbParameterList*>(raw);
			if (op == 0) return params->Find(name) != nullptr ? 1 : 0;
			params->SetParameter(name, val);
			return 0;
		};
		if (g_ExtensionManager)
		{
			nzbget_rs_ext_mgr_op(
				g_ExtensionManager->Handle(), 5, unpack ? 1 : 0, reset ? 1 : 0,
				reinterpret_cast<uintptr_t>(paramCb), extensions, nullptr, nullptr,
				nullptr, parameters, nullptr, nullptr);
		}
		else if (!parameters->Find("*Unpack:"))
		{
			parameters->SetParameter("*Unpack:", unpack ? "yes" : "no");
		}
	}

	void ScanNzbDir(bool syncMode)
	{
		NzbgetRsScannerEnv env = MakeEnv();
		nzbget_rs_scanner_exec(m_rsScanner, 3, &env, syncMode ? 1 : 0, 0, 0);
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
	int ServiceInterval() override
	{
		return nzbget_rs_scanner_exec(m_rsScanner, 1, nullptr, g_Options ? g_Options->GetNzbDirInterval() : 0, 0, 0);
	}

	void ServiceWork() override
	{
		NzbgetRsScannerEnv env = MakeEnv();
		nzbget_rs_scanner_exec(m_rsScanner, 2, &env, 0, 0, 0);
	}

private:
	NzbgetRsScanner* m_rsScanner = nullptr;
	NzbParameterList m_workParams;
	std::unique_ptr<NzbFile> m_activeNzb;

	NzbgetRsScannerEnv MakeEnv()
	{
		NzbgetRsScannerEnv env{};
		env.ctx = this;
		env.nzb_dir = g_Options ? g_Options->GetNzbDir() : nullptr;
		env.temp_dir = g_Options ? g_Options->GetTempDir() : nullptr;
		env.nzb_dir_interval = g_Options ? g_Options->GetNzbDirInterval() : 0;
		env.nzb_dir_file_age = g_Options ? g_Options->GetNzbDirFileAge() : 0;
		env.archive_scan = (g_Options && g_Options->GetNzbDirArchiveScan()) ? 1 : 0;
		env.archive_action = g_Options ? static_cast<int>(g_Options->GetNzbDirArchiveAction()) : 0;
		env.queue_loaded = DownloadQueue::IsLoaded() ? 1 : 0;
		env.pause_scan = (g_WorkState && g_WorkState->GetPauseScan()) ? 1 : 0;
		env.path_sep = PATH_SEPARATOR;
		env.wake_cb = [](void* ctx) { static_cast<Scanner*>(ctx)->WakeUp(); };
		env.log_cb = [](void*, int kind, const char* msg)
		{
			const char* text = msg ? msg : "";
			if (kind == 0) { debug("%s", text); }
			else if (kind == 1) { detail("%s", text); }
			else if (kind == 2) { info("%s", text); }
			else if (kind == 3) { warn("%s", text); }
			else { error("%s", text); }
		};
		env.find_category_cb = [](void*, const char* category, int searchAliases) -> const char*
		{
			const char* strs[3]{};
			return (g_Options && g_Options->QueryCategory(category ? category : "", searchAliases != 0, nullptr, strs))
				? strs[0] : nullptr;
		};
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
			const char* extensions = g_Options ? g_Options->GetExtensions() : "";
			if (g_Options && !sc.nzbCat.empty())
			{
				const char* strs[3]{};
				if (g_Options->QueryCategory(sc.nzbCat.c_str(), false, nullptr, strs) &&
					!Util::EmptyStr(strs[2]))
				{
					extensions = strs[2];
				}
			}
			nzbget_rs_scan_scripts_exec(
				g_ExtensionManager ? g_ExtensionManager->Handle() : nullptr,
				extensions, fullFilename, nzbInfo ? nzbInfo->GetUrl() : "", directory, &sc,
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
				},
				&ScriptHostCb);
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

class UrlCoordinator : public Thread, public Observer, public Debuggable
{
public:
	UrlCoordinator() : m_rs(nzbget_rs_url_coord_new())
	{
		m_downloadQueueObserver.m_owner = this;
		DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);
	}

	~UrlCoordinator() override
	{
		NzbgetRsUrlCoordEnv env = MakeEnv();
		nzbget_rs_url_coord_exec(m_rs, 9, &env);
		nzbget_rs_url_coord_free(m_rs);
	}

	void Run() override { NzbgetRsUrlCoordEnv env = MakeEnv(); nzbget_rs_url_coord_exec(m_rs, 0, &env); }

	void Stop() override
	{
		Thread::Stop();
		NzbgetRsUrlCoordEnv env = MakeEnv();
		nzbget_rs_url_coord_exec(m_rs, 1, &env);
	}

	void Update(Subject* caller, void*) override
	{
		NzbgetRsUrlCoordEnv env = MakeEnv();
		env.p1 = reinterpret_cast<uintptr_t>(static_cast<UrlDownloader*>(caller));
		nzbget_rs_url_coord_exec(m_rs, 4, &env);
	}

	bool HasMoreJobs() const { return nzbget_rs_url_coord_exec(m_rs, 7, nullptr) != 0; }

	bool DeleteQueueEntry(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool avoidHistory)
	{
		NzbgetRsUrlCoordEnv env = MakeEnv();
		env.p1 = reinterpret_cast<uintptr_t>(downloadQueue);
		env.p2 = reinterpret_cast<uintptr_t>(nzbInfo);
		env.a1 = avoidHistory ? 1 : 0;
		return nzbget_rs_url_coord_exec(m_rs, 5, &env) != 0;
	}

	void AddUrlToQueue(std::unique_ptr<NzbInfo> nzbInfo, bool addFirst)
	{
		NzbgetRsUrlCoordEnv env = MakeEnv();
		env.p1 = reinterpret_cast<uintptr_t>(nzbInfo.get());
		env.p2 = reinterpret_cast<uintptr_t>(&nzbInfo);
		env.a1 = addFirst ? 1 : 0;
		nzbget_rs_url_coord_exec(m_rs, 6, &env);
	}

protected:
	void LogDebugInfo() override { NzbgetRsUrlCoordEnv env = MakeEnv(); nzbget_rs_url_coord_exec(m_rs, 8, &env); }

private:
	class UrlDownloader final : public WebDownloader
	{
	public:
		UrlDownloader(UrlCoordinator* owner, NzbInfo* nzbInfo) : m_owner(owner), m_nzbInfo(nzbInfo) {}
		NzbInfo* GetNzbInfo() const { return m_nzbInfo; }
	protected:
		void ProcessHeader(const char* line) override { m_owner->OnUrlHeader(this, m_nzbInfo, line); }
	private:
		UrlCoordinator* m_owner;
		NzbInfo* m_nzbInfo;
	};

	class DownloadQueueObserver final : public Observer
	{
	public:
		UrlCoordinator* m_owner;
		void Update(Subject*, void* aspect) override
		{
			NzbgetRsUrlCoordEnv env = m_owner->MakeEnv();
			env.a1 = static_cast<int>(static_cast<DownloadQueue::Aspect*>(aspect)->action);
			nzbget_rs_url_coord_exec(m_owner->m_rs, 2, &env);
		}
	};

	NzbgetRsUrlCoord* m_rs;
	DownloadQueueObserver m_downloadQueueObserver;

	void OnUrlHeader(UrlDownloader* dl, NzbInfo* nzbInfo, const char* line)
	{
		NzbgetRsUrlCoordEnv env = MakeEnv();
		env.p1 = reinterpret_cast<uintptr_t>(dl);
		env.p2 = reinterpret_cast<uintptr_t>(nzbInfo);
		env.s1 = line;
		nzbget_rs_url_coord_exec(m_rs, 3, &env);
	}

	NzbgetRsUrlCoordEnv MakeEnv()
	{
		NzbgetRsUrlCoordEnv env{};
		env.ctx = this;
		env.with_queue_guard_cb = +[](void*, void* inner, void (*fnCb)(void*, uintptr_t)) {
			GuardedDownloadQueue dq = DownloadQueue::Guard();
			fnCb(inner, reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(dq)));
		};
		env.log_cb = +[](void*, int level, const char* msg) {
			if (level == 0) debug("%s", msg);
			else if (level == 1) detail("%s", msg);
			else if (level == 2) info("%s", msg);
			else if (level == 3) warn("%s", msg);
			else if (level == 4) error("%s", msg);
		};
		env.query_cb = +[](void* raw, int op, uintptr_t ptr, int* outInts, int64_t* outI64s, const char** outStrs) -> uintptr_t {
			if (op == 0) return DownloadQueue::IsLoaded() ? 1 : 0;
			if (op == 1) return static_cast<UrlCoordinator*>(raw)->IsStopped() ? 1 : 0;
			if (op == 2)
			{
				outInts[0] = g_Options->GetUrlConnections();
				outInts[1] = g_WorkState->GetPauseDownload() ? 1 : 0;
				outInts[2] = g_Options->GetUrlForce() ? 1 : 0;
				outInts[3] = g_Options->GetUrlTimeout();
				outInts[4] = static_cast<int>(PATH_SEPARATOR);
				outStrs[0] = g_Options->GetTempDir();
				return 1;
			}
			if (op == 3) return reinterpret_cast<DownloadQueue*>(ptr)->GetQueue()->size();
			if (op == 4)
			{
				NzbInfo* n = (*reinterpret_cast<DownloadQueue*>(ptr)->GetQueue())[static_cast<size_t>(outInts[0])].get();
				outInts[0] = static_cast<int>(n->GetKind());
				outInts[1] = static_cast<int>(n->GetUrlStatus());
				outInts[2] = static_cast<int>(n->GetDeleteStatus());
				outInts[3] = n->GetPriority();
				outInts[4] = n->GetId();
				return reinterpret_cast<uintptr_t>(n);
			}
			if (op == 5)
			{
				auto* dl = reinterpret_cast<UrlDownloader*>(ptr);
				outInts[0] = static_cast<int>(dl->GetStatus());
				if (outI64s) outI64s[0] = static_cast<int64_t>(dl->GetLastUpdateTime());
				outStrs[0] = dl->GetInfoName();
				outStrs[1] = dl->GetOriginalFilename();
				outStrs[2] = dl->GetOutputFilename();
				return reinterpret_cast<uintptr_t>(dl->GetNzbInfo());
			}
			if (op == 6)
			{
				auto* n = reinterpret_cast<NzbInfo*>(ptr);
				outInts[0] = static_cast<int>(n->GetUrlStatus());
				outInts[1] = static_cast<int>(n->GetDeleteStatus());
				outInts[2] = n->GetDeleting() ? 1 : 0;
				outInts[3] = n->GetActiveDownloads();
				outStrs[0] = n->GetUrl();
				outStrs[1] = n->GetFilename();
				outStrs[2] = n->GetCategory();
				outStrs[3] = n->GetName();
				return 1;
			}
			return 0;
		};
		env.action_cb = +[](void* raw, int op, uintptr_t p1, uintptr_t p2, int a1, int a2, const char* s1, const char* s2, const char* s3) -> uintptr_t {
			if (op == 0)
			{
				auto* self = static_cast<UrlCoordinator*>(raw);
				auto* n = reinterpret_cast<NzbInfo*>(p1);
				auto* dl = new UrlDownloader(self, n);
				dl->SetAutoDestroy(true);
				dl->Attach(self);
				dl->SetUrl(n->GetUrl());
				dl->SetForce(g_Options->GetUrlForce());
				dl->SetInfoName(n->MakeNiceUrlName(n->GetUrl(), n->GetFilename()));
				dl->SetOutputFilename(s1 ? s1 : "");
				n->SetActiveDownloads(1);
				n->SetUrlStatus(NzbInfo::lsRunning);
				return reinterpret_cast<uintptr_t>(dl);
			}
			if (op == 1)
			{
				auto* dl = reinterpret_cast<UrlDownloader*>(p1);
				if (a1 == 0) dl->Stop();
				else if (a1 == 1) dl->LogDebugInfo();
				else if (a1 == 2) delete dl;
				else if (a1 == 3) dl->Start();
				return 1;
			}
			if (op == 2) { reinterpret_cast<NzbInfo*>(p1)->GetParameters()->SetParameter(s1, s2); return 1; }
			if (op == 3)
			{
				auto* n = reinterpret_cast<NzbInfo*>(p1);
				if (a1 == 0) n->SetActiveDownloads(a2);
				else if (a1 == 1) n->SetUrlStatus(static_cast<NzbInfo::EUrlStatus>(a2));
				else if (a1 == 2) n->SetDeleteStatus(static_cast<NzbInfo::EDeleteStatus>(a2));
				else if (a1 == 3) n->SetDeleting(a2 != 0);
				else if (a1 == 4) n->SetAvoidHistory(a2 != 0);
				return 1;
			}
			if (op == 4)
			{
				auto* dq = reinterpret_cast<DownloadQueue*>(p1);
				DownloadQueue::Aspect aspect = {static_cast<DownloadQueue::EAspectAction>(a1), dq, reinterpret_cast<NzbInfo*>(p2), nullptr};
				dq->Notify(&aspect);
				return 1;
			}
			if (op == 5)
			{
				auto* n = reinterpret_cast<NzbInfo*>(p1);
				Scanner::EAddStatus st = a1 != 0
					? g_Scanner->AddArchive(s1, s2, n->GetAutoCategory(), n->GetPriority(), n->GetDupeKey(), n->GetDupeScore(), n->GetDupeMode(), n->GetParameters(), false, n->GetAddUrlPaused(), n, s3)
					: g_Scanner->AddExternalFile(s1, s2, n->GetAutoCategory(), n->GetPriority(), n->GetDupeKey(), n->GetDupeScore(), n->GetDupeMode(), n->GetParameters(), false, n->GetAddUrlPaused(), n, s3, nullptr, 0, nullptr);
				return static_cast<uintptr_t>(st);
			}
			if (op == 6) { g_QueueScriptCoordinator->EnqueueScript(reinterpret_cast<NzbInfo*>(p1), QueueScriptCoordinator::qeUrlCompleted); return 1; }
			if (op == 7)
			{
				auto* dq = reinterpret_cast<DownloadQueue*>(p1);
				if (a1 == 2) dq->Save();
				else if (a1 != 0)
				{
					auto* holder = reinterpret_cast<std::unique_ptr<NzbInfo>*>(p2);
					if (holder && *holder) dq->GetQueue()->Add(std::move(*holder), a2 != 0);
				}
				return 1;
			}
			return 0;
		};
		return env;
	}
};

extern UrlCoordinator* g_UrlCoordinator;


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


class DupeSearch final : public Thread
{
public:
	static constexpr const char* OwnedKeyParam = "DupeSearchOwnedKey";

	DupeSearch() : m_rs(nzbget_rs_dupesearch_new())
	{
		m_observer.m_owner = this;
		DownloadQueue::Guard()->Attach(&m_observer);
	}

	~DupeSearch() override
	{
		debug("Destroying DupeSearch");
		DownloadQueue::Guard()->Detach(&m_observer);
		nzbget_rs_dupesearch_free(m_rs);
	}

	void Stop() override
	{
		Thread::Stop();
		nzbget_rs_dupesearch_exec(m_rs, 0, 0, 0, 0, this, &HostCb);
	}

	void NotifyNameChanged(int nzbId)
	{
		nzbget_rs_dupesearch_exec(m_rs, 1, nzbId, 0, 0, this, &HostCb);
	}

	void Run() override
	{
		nzbget_rs_dupesearch_exec(m_rs, 3, 0, 0, 0, this, &HostCb);
	}

	static uintptr_t HostCb(
		void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
		const char* s1, const char* s2, int* ints, int64_t* i64s,
		const char** strs, const uint8_t* bytes, size_t len)
	{
		auto* self = static_cast<DupeSearch*>(ctx);
		switch (op)
		{
			case 0: {
				ints[0] = g_Options->GetDupeSearch() ? 1 : 0;
				ints[1] = g_Options->GetDupeCheck() ? 1 : 0;
				ints[2] = g_Options->GetDupeSearchDryRun() ? 1 : 0;
				ints[3] = g_Options->GetDupeSearchDelay();
				ints[4] = g_Options->GetDupeSearchMaxDonors();
				ints[5] = g_Options->GetDupeFastDonors();
				ints[6] = g_Options->GetDupeMinAlive();
				ints[7] = g_Options->GetDupeHealthPercent();
				ints[8] = g_Options->GetDupeHealthMin();
				ints[9] = g_Options->GetDupeHealthMax();
				ints[10] = g_Options->GetDupeBodyChecks();
				ints[11] = g_Options->GetDupeHealthBudget();
				ints[12] = (int)g_Options->GetHealthCheck();
				ints[13] = DownloadQueue::IsLoaded() ? 1 : 0;
				ints[14] = (self && self->IsStopped()) ? 1 : 0;
				ints[15] = (int)PATH_SEPARATOR;
				i64s[0] = (int64_t)Util::CurrentTime();
				strs[0] = g_Options->GetQueueDir() ? g_Options->GetQueueDir() : "";
				strs[1] = g_Options->GetTempDir() ? g_Options->GetTempDir() : "";
				strs[2] = g_Options->GetDupeSearchUrl() ? g_Options->GetDupeSearchUrl() : "";
				strs[3] = g_Options->GetDupeSearchApiKey() ? g_Options->GetDupeSearchApiKey() : "";
				strs[4] = g_Options->GetExtCleanupDisk() ? g_Options->GetExtCleanupDisk() : "";
				return 0;
			}
			case 1:
				if (a1 == 0) info("%s", s1 ? s1 : "");
				else if (a1 == 1) warn("%s", s1 ? s1 : "");
				else if (a1 == 2) error("%s", s1 ? s1 : "");
				else if (a1 == 3) debug("%s", s1 ? s1 : "");
				else detail("%s", s1 ? s1 : "");
				return 0;
			case 2: {
				auto* dl = new WebDownloader();
				dl->SetUrl(s1);
				dl->SetInfoName(s2);
				dl->SetOutputFilename(strs[0]);
				dl->SetForce(true);
				dl->SetRetry(false);
				return uintptr_t(dl);
			}
			case 3: {
				auto* dl = reinterpret_cast<WebDownloader*>(p1);
				if (a1 == 0) {
					bool ok = dl->DownloadWithRedirects(5) == WebDownloader::adFinished;
					ints[0] = dl->GetHttpStatus();
					return ok ? 1 : 0;
				}
				if (a1 == 1) { dl->Stop(); return 0; }
				delete dl;
				return 0;
			}
			case 4: {
				GuardedDownloadQueue dq = DownloadQueue::Guard();
				reinterpret_cast<void(*)(void*, uintptr_t)>(p2)(
					reinterpret_cast<void*>(p1), uintptr_t((DownloadQueue*)dq));
				return 0;
			}
			case 5: {
				if (a1 == 0) {
					auto* dq = reinterpret_cast<DownloadQueue*>(p1);
					ints[0] = (int)dq->GetQueue()->size();
					ints[1] = (int)dq->GetHistory()->size();
					return 0;
				}
				if (a1 == 1)
					return uintptr_t(reinterpret_cast<DownloadQueue*>(p1)->GetQueue()->at((size_t)a2).get());
				if (a1 == 2) {
					HistoryInfo* h = reinterpret_cast<DownloadQueue*>(p1)->GetHistory()->at((size_t)a2).get();
					ints[0] = h->GetId();
					ints[1] = (int)h->GetKind();
					i64s[0] = (int64_t)h->GetTime();
					return h->GetKind() == HistoryInfo::hkNzb ? uintptr_t(h->GetNzbInfo()) : 0;
				}
				if (a1 == 3) {
					auto* dq = reinterpret_cast<DownloadQueue*>(p1);
					if (NzbInfo* n = dq->GetQueue()->Find(a2)) { ints[0] = 0; return uintptr_t(n); }
					for (HistoryInfo* h : dq->GetHistory())
						if (h->GetKind() == HistoryInfo::hkNzb && h->GetNzbInfo()->GetId() == a2)
							{ ints[0] = 1; return uintptr_t(h->GetNzbInfo()); }
					return 0;
				}
				auto* n = reinterpret_cast<NzbInfo*>(p1);
				ints[0] = n->GetId();
				ints[1] = (int)n->GetKind();
				ints[2] = n->GetDeleting() ? 1 : 0;
				ints[3] = (int)n->GetDeleteStatus();
				ints[4] = (int)n->GetMarkStatus();
				ints[5] = (int)n->GetDupeMode();
				ints[6] = (int)n->GetDupeHint();
				ints[7] = n->GetDupeScore();
				ints[8] = n->GetPostInfo() != nullptr ? 1 : 0;
				ints[9] = n->IsDupeSuccess() ? 1 : 0;
				ints[10] = (int)n->GetFullContentHash();
				ints[11] = 0;
				ints[12] = (n->GetParameters()->Find("DupeSearch") || n->GetParameters()->Find("DupeAlive")) ? 1 : 0;
				ints[13] = n->GetParameters()->Find("DupeFleet") != nullptr ? 1 : 0;
				ints[14] = a2 ? n->CalcHealth() : 0;
				ints[15] = a2 ? n->CalcCriticalHealth(true) : 0;
				strs[0] = n->GetName() ? n->GetName() : "";
				strs[1] = n->GetDupeKey() ? n->GetDupeKey() : "";
				strs[2] = n->GetCategory() ? n->GetCategory() : "";
				strs[3] = n->GetQueuedFilename() ? n->GetQueuedFilename() : "";
				strs[4] = n->GetFinalDir() ? n->GetFinalDir() : "";
				strs[5] = n->GetDestDir() ? n->GetDestDir() : "";
				NzbParameter* alive = n->GetParameters()->Find("DupeAlive");
				NzbParameter* owned = n->GetParameters()->Find(OwnedKeyParam);
				strs[6] = alive ? alive->GetValue() : nullptr;
				strs[7] = owned ? owned->GetValue() : nullptr;
				return 0;
			}
			case 6: {
				auto* dq = reinterpret_cast<DownloadQueue*>(p1);
				if (a1 == 0) {
					auto* n = reinterpret_cast<NzbInfo*>(p2);
					n->SetDupeKey(s1);
					n->GetParameters()->SetParameter(OwnedKeyParam, s2);
					if ((a2 & 2) && n->GetDupeScore() < 1000000) n->SetDupeScore(1000000);
					n->SetChanged(true);
					if (a2 & 1) { dq->HistoryChanged(); dq->Save(); }
					else dq->SaveChanged();
					return 0;
				}
				if (a1 == 1) {
					reinterpret_cast<NzbInfo*>(p2)->AddMessage((Message::EKind)a2, s1);
					return 0;
				}
				if (a1 == 2) {
					IdList ids{ a2 };
					bool queued = dq->GetQueue()->Find(a2) != nullptr;
					dq->EditList(&ids, nullptr, DownloadQueue::mmId,
						queued ? DownloadQueue::eaGroupFinalDelete : DownloadQueue::eaHistoryFinalDelete, nullptr);
					return 0;
				}
				for (HistoryInfo* h : dq->GetHistory())
					if (h->GetId() == a2 && h->GetKind() == HistoryInfo::hkDup) return 0;
				IdList ids{ a2 };
				const std::pair<DownloadQueue::EEditAction, DownloadQueue::EEditAction> kinds[] = {
					{ DownloadQueue::eaHistorySetDupeScore, DownloadQueue::eaHistorySetParameter },
					{ DownloadQueue::eaGroupSetDupeScore, DownloadQueue::eaGroupSetParameter } };
				for (const auto& k : kinds)
				{
					if (dq->EditList(&ids, nullptr, DownloadQueue::mmId, k.first, s1))
					{
						for (size_t i = 0; i < (size_t)p2; i++)
							dq->EditList(&ids, nullptr, DownloadQueue::mmId, k.second, strs[i]);
						return 1;
					}
				}
				return 0;
			}
			case 7: {
				NzbParameterList parameters;
				for (size_t i = 0; i < (size_t)p1; i++)
					parameters.SetParameter(strs[1 + i * 2], strs[2 + i * 2]);
				ints[0] = 0;
				return g_Scanner->AddExternalFile(s1, s2, false, a1, strs[0], a2, dmScore,
					&parameters, false, false, nullptr, nullptr,
					reinterpret_cast<const char*>(bytes), (int)len, &ints[0]) == Scanner::asSuccess ? 1 : 0;
			}
			case 8:
				g_WorkState->HoldDownloadForFleet(a1 != 0);
				return 0;
			case 10: {
				if (a1 < 0) return g_ServerPool->GetServers()->size();
				if ((size_t)a1 >= g_ServerPool->GetServers()->size()) return 0;
				NewsServer* srv = g_ServerPool->GetServers()->at((size_t)a1).get();
				ints[0] = srv->GetId();
				ints[1] = srv->GetActive() ? 1 : 0;
				ints[2] = srv->GetMaxConnections();
				ints[3] = srv->GetGroup();
				return 1;
			}
			case 11: {
				NewsServer* srv = g_ServerPool->GetServerById(a1);
				ints[0] = srv ? 1 : 0;
				if (!srv) return 0;
				ints[1] = srv->GetJoinGroup() ? 1 : 0;
				return uintptr_t(g_ServerPool->GetConnection(srv->GetNormLevel(), srv, nullptr));
			}
			case 12: {
				auto* c = reinterpret_cast<NntpConnection*>(p1);
				if (a1) c->Disconnect();
				g_ServerPool->FreeConnection(c, true);
				return 0;
			}
			case 13: {
				auto* c = reinterpret_cast<NntpConnection*>(p1);
				if (!c->Connect()) { ints[0] = 1; return 0; }
				if (a1) {
					const char* joined = nullptr;
					for (int i = 0; i < a2; i++) {
						joined = c->JoinGroup(strs[i]);
						if (joined && joined[0] == '2') break;
					}
					if (!joined || joined[0] != '2') { ints[0] = !joined ? 1 : 0; return 0; }
				}
				if (!c->Send(reinterpret_cast<const char*>(bytes), (int)len)) { ints[0] = 1; return 0; }
				return 1;
			}
			case 14: {
				char line[1024];
				int length = 0;
				if (!reinterpret_cast<NntpConnection*>(p1)->ReadLine(line, sizeof(line), &length)) return 0;
				return !strncmp(line, "223", 3) ? 1 : !strncmp(line, "430", 3) ? 2 : 3;
			}
			case 15:
				return reinterpret_cast<uintptr_t>(&NzbgetCArticleFetchHost);
			default:
				return 0;
		}
	}

private:
	class DownloadQueueObserver final : public Observer
	{
	public:
		DupeSearch* m_owner = nullptr;
		void Update(Subject* caller, void* aspect) override
		{
			auto* ea = static_cast<DownloadQueue::Aspect*>(aspect);
			int kind = ea->action == DownloadQueue::eaNzbAdded ? 1
				: ea->action == DownloadQueue::eaNzbNamed ? 2 : 0;
			if (kind && ea->nzbInfo)
				nzbget_rs_dupesearch_exec(
					m_owner->m_rs, 2, kind, uintptr_t(static_cast<DownloadQueue*>(caller)),
					uintptr_t(ea->nzbInfo), m_owner, &HostCb);
		}
	};

	DownloadQueueObserver m_observer;
	NzbgetRsDupeSearch* m_rs = nullptr;
};

inline DupeSearch* g_DupeSearch = nullptr;

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
		case 7: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			CString ts = nzb->MakeTextStatus(true);
			snprintf(reinterpret_cast<char*>(bytes), len, "%s", *ts ? *ts : "");
			ints[0] = static_cast<int>(g_Options->GetDupeArticleFallback());
			ints[1] = static_cast<int>(g_Options->GetHealthCheck());
			ints[2] = static_cast<int>(nzb->GetKind());
			ints[3] = nzb->GetFailedArticles();
			ints[4] = nzb->GetParameters()->Find("*DupeRecheck") != nullptr ? 1 : 0;
			ints[5] = (g_Options->GetDupeCheck() && nzb->GetDupeMode() == dmScore) ? 1 : 0;
			ints[6] = static_cast<int>(dq->GetQueue()->size());
			strs[0] = nzb->GetName();
			strs[1] = nzb->GetDupeKey();
			return 0;
		}
		case 8: {
			NzbInfo* q = (*reinterpret_cast<DownloadQueue*>(p1)->GetQueue())[static_cast<size_t>(a1)].get();
			strs[0] = q->GetName();
			strs[1] = q->GetDupeKey();
			return 0;
		}
		case 9: {
			auto* dq = reinterpret_cast<DownloadQueue*>(p1);
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			return g_DupeCoordinator->FindDupeBackup(dq, nzb, nzb->GetName(), nzb->GetDupeKey()) ? 1 : 0;
		}
		case 10: {
			auto* nzb = reinterpret_cast<NzbInfo*>(p2);
			nzb->GetParameters()->SetParameter("*DupeRecheck", "1");
			auto* failed = new std::vector<DupeProbe::Sample>();
			for (CompletedFile& cf : nzb->GetCompletedFiles())
			{
				if ((cf.GetStatus() != CompletedFile::cfPartial && cf.GetStatus() != CompletedFile::cfFailure) || cf.GetId() <= 0) continue;
				bool partial = cf.GetStatus() == CompletedFile::cfPartial;
				FileInfo fi(cf.GetId());
				if (!g_DiskState->LoadFile(&fi, false, true) || (partial && !g_DiskState->LoadFileState(&fi, g_ServerPool->GetServers(), true))) continue;
				auto groups = std::make_shared<std::vector<CString>>();
				for (const CString& g : *fi.GetGroups()) groups->emplace_back(*g);
				for (ArticleInfo* art : fi.GetArticles())
				{
					if ((!partial || art->GetStatus() == ArticleInfo::aiFailed) && !Util::EmptyStr(art->GetMessageId()))
						failed->push_back({art->GetMessageId(), groups});
				}
			}
			if (failed->size() < 2) { delete failed; ints[0] = 0; return 0; }
			ints[0] = static_cast<int>(failed->size());
			return reinterpret_cast<uintptr_t>(failed);
		}
		case 11: {
			auto* failed = reinterpret_cast<std::vector<DupeProbe::Sample>*>(p1);
			std::vector<DupeProbe::Sample> samples;
			samples.reserve(len);
			for (size_t i = 0; i < len; ++i) samples.push_back(std::move((*failed)[static_cast<size_t>(ints[i])]));
			delete failed;
			DupeProbe::StartRecheck(reinterpret_cast<NzbInfo*>(p2)->GetId(), std::move(samples));
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
		case 19:
			g_DupeCoordinator->HistoryMark(reinterpret_cast<DownloadQueue*>(p1),
				reinterpret_cast<HistoryInfo*>(p2), static_cast<NzbInfo::EMarkStatus>(a1));
			return 0;
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
				g_DiskState->LoadFileState(fi.get(), g_ServerPool->GetServers(), true);
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
					if (hist->GetKind() == HistoryInfo::hkNzb && g_DupeSearch && prev != hist->GetNzbInfo()->GetName())
						g_DupeSearch->NotifyNameChanged(hist->GetNzbInfo()->GetId());
				}
				else if (hist->GetKind() == HistoryInfo::hkDup) hist->GetDupInfo()->SetName(s1 ? s1 : "");
			}
			else if (a1 == 3)
			{
				hist->GetNzbInfo()->GetParameters()->SetParameter(DupeSearch::OwnedKeyParam, nullptr);
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


inline uintptr_t QueueCoordinator::HostCb(void* ctx, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
{
	static_assert(Message::mkInfo == 0 && Message::mkDetail == 4, "Message::EKind");
	static_assert(Options::dafNone == 0 && Options::dafStream == 2 && Options::dafLive == 3, "EDupeArticleFallback");
	static_assert(Options::hcPause == 0 && Options::hcDupe == 4, "EHealthCheck");
	static_assert(ArticleDownloader::adRunning == 1 && ArticleDownloader::adFinished == 3 &&
		ArticleDownloader::adFailed == 4 && ArticleDownloader::adRetry == 5 &&
		ArticleDownloader::adFatalError == 10, "ArticleDownloader::EStatus");
	static_assert(DownloadQueue::eaGroupPause == 16 && DownloadQueue::eaGroupSortFiles == 34 &&
		DownloadQueue::eaPostDelete == 35, "DownloadQueue::EEditAction");

	auto* self = static_cast<QueueCoordinator*>(ctx);
	switch (op)
	{
	case 1: {
		ints[0] = (int)g_Options->GetDupeArticleFallback();
		ints[1] = (int)g_Options->GetParCheck();
		ints[2] = (int)g_Options->GetParScan();
		ints[3] = (int)g_Options->GetHealthCheck();
		ints[4] = g_Options->GetDupeCheck() ? 1 : 0;
		ints[5] = g_Options->GetServerMode() ? 1 : 0;
		ints[6] = g_Options->GetContinuePartial() ? 1 : 0;
		ints[7] = g_Options->GetDirectWrite() ? 1 : 0;
		ints[8] = g_Options->GetDirectRename() ? 1 : 0;
		ints[9] = g_Options->GetRawArticle() ? 1 : 0;
		ints[10] = g_Options->GetReorderFiles() ? 1 : 0;
		ints[11] = (int)g_Options->GetFileNaming();
		ints[12] = g_Options->GetPropagationDelay();
		ints[13] = g_Options->GetArticleTimeout();
		ints[14] = (g_WorkState && g_WorkState->GetPauseDownload()) ? 1 : 0;
		ints[15] = (g_WorkState && g_WorkState->GetTempPauseDownload()) ? 1 : 0;
		ints[16] = (g_WorkState && g_WorkState->GetQuotaReached()) ? 1 : 0;
		ints[17] = (g_WorkState && g_WorkState->GetDownloadHeld()) ? 1 : 0;
		ints[18] = (g_WorkState && g_WorkState->GetPausePostProcess()) ? 1 : 0;
		ints[19] = (g_WorkState && g_WorkState->GetPauseScan()) ? 1 : 0;
		ints[20] = (g_WorkState && g_WorkState->GetTempPausePostprocess()) ? 1 : 0;
		ints[21] = g_Options->GetKeepHistory();
		ints[22] = (int)g_Options->GetPostStrategy();
		ints[23] = g_Options->GetSkipWrite() ? 1 : 0;
		ints[24] = g_Options->GetParRename() ? 1 : 0;
		ints[25] = g_Options->GetRarRename() ? 1 : 0;
		ints[26] = g_Options->GetDirectUnpack() ? 1 : 0;
		ints[27] = g_Options->GetParPauseQueue() ? 1 : 0;
		ints[28] = g_Options->GetUnpackPauseQueue() ? 1 : 0;
		ints[29] = g_Options->GetScriptPauseQueue() ? 1 : 0;
		ints[30] = g_Options->GetPostDownloadRename() ? 1 : 0;
		ints[31] = g_Options->GetRenameAfterUnpack() ? 1 : 0;
		ints[32] = g_Options->GetHardLinking() ? 1 : 0;
		ints[33] = g_Options->GetUnpack() ? 1 : 0;
#ifndef DISABLE_PARCHECK
		ints[34] = 1;
#else
		ints[34] = 0;
#endif
		i64s[0] = (int64_t)Util::CurrentTime();
		strs[0] = g_Options->GetQueueDir();
		strs[1] = g_Options->GetParIgnoreExt();
		strs[2] = g_Options->GetExtCleanupDisk();
		strs[3] = g_Options->GetInterDir();
		strs[4] = g_Options->GetHardLinkingIgnoreExt();
		strs[5] = g_Options->GetExtensions();
		return 0;
	}
	case 2: {
		const char* msg = s1 ? s1 : "";
		if (p1 != 0)
		{
			NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
			if (a2 == 1) n->AddMessage((Message::EKind)a1, msg);
			else n->PrintMessage((Message::EKind)a1, "%s", msg);
		}
		else
		{
			if (a1 == Message::mkInfo) info("%s", msg);
			else if (a1 == Message::mkWarning) warn("%s", msg);
			else if (a1 == Message::mkError) error("%s", msg);
			else if (a1 == Message::mkDebug) debug("%s", msg);
			else if (a1 == Message::mkDetail) detail("%s", msg);
		}
		return 0;
	}
	case 3: {
		if (a1 == 0) Util::Sleep(a2);
		else if (a1 == 1) Util::SetStandByMode(a2 != 0);
		else if (a1 == 2 && g_StatMeter)
		{
			if (a2 == 0) { bool perfect = true; bool loaded = g_StatMeter->Load(&perfect); ints[0] = loaded ? 1 : 0; ints[1] = perfect ? 1 : 0; }
			else if (a2 == 1) g_StatMeter->Save();
			else if (a2 == 2) g_StatMeter->IntervalCheck();
			else if (a2 == 3) g_StatMeter->EnterLeaveStandBy(p1 != 0);
			else if (a2 == 4) g_StatMeter->AddSpeedReading(0);
		}
		else if (a1 == 3 && g_Log) g_Log->IntervalCheck();
		else if (a1 == 4 && g_WorkState)
		{
			if (a2 == 0) g_WorkState->SetDownloading(p1 != 0);
			else if (a2 == 1 && self) g_WorkState->Attach(self);
			else if (a2 == 2) g_WorkState->SetTempPauseDownload(p1 != 0);
		}
		else if (a1 == 5) return (uintptr_t)(g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveDownloadsLimit(a2) : a2);
		else if (a1 == 6)
		{
			const char* catStrs[3]{};
			if (!g_Options || !g_Options->QueryCategory(s1 ? s1 : "", false, ints, catStrs)) return 0;
			strs[0] = catStrs[2];
			return 1;
		}
		else if (a1 == 7) return DownloadQueue::IsLoaded() ? 1 : 0;
		return 0;
	}
	case 4: {
		if (!g_ServerPool) return 0;
		if (a1 == 0) return (uintptr_t)g_ServerPool->GetGeneration();
		if (a1 == 1) { g_ServerPool->CloseUnusedConnections(); return 0; }
		if (a1 == 2)
		{
			if (a2 < 0) return g_ServerPool->GetServers()->size();
			NewsServer* ns = g_ServerPool->GetServers()->at((size_t)a2).get();
			ints[0] = ns->GetNormLevel(); ints[1] = ns->GetActive() ? 1 : 0; ints[2] = ns->GetMaxConnections();
			return 0;
		}
		if (a1 == 3)
		{
			bool hasDesired = a2 != 0;
			NewsServer* desired = hasDesired ? g_ServerPool->GetServerById((int)p1) : nullptr;
			bool useSpecific = hasDesired && desired && desired->GetActive();
			int connLevel = useSpecific ? desired->GetNormLevel() : 0;
			return reinterpret_cast<uintptr_t>(g_ServerPool->GetConnection(connLevel, useSpecific ? desired : nullptr, nullptr));
		}
		return 0;
	}
	case 5: {
		if (!g_DiskState) return 0;
		if (a1 == 0) return g_DiskState->DownloadQueueExists() ? 1 : 0;
		if (a1 == 1) return g_DiskState->LoadDownloadQueue(reinterpret_cast<DownloadQueue*>(p1), g_ServerPool->GetServers()) ? 1 : 0;
		if (a1 == 2) { g_DiskState->DiscardDownloadQueue(); return 0; }
		if (a1 == 3) return g_DiskState->LegacyQueueRead() ? 1 : 0;
		if (a1 == 4) { g_DiskState->CleanupTempDir(reinterpret_cast<DownloadQueue*>(p1)); return 0; }
		if (a1 == 5) { g_DiskState->SaveAllFileInfos(reinterpret_cast<DownloadQueue*>(p1)); return 0; }
		if (a1 == 6) { g_DiskState->DiscardFiles(reinterpret_cast<NzbInfo*>(p1)); return 0; }
		if (a1 == 7) { g_DiskState->DiscardFile(a2, ints[0] != 0, ints[1] != 0, ints[2] != 0); return 0; }
		if (a1 == 8) { g_DiskState->SaveFile(reinterpret_cast<FileInfo*>(p1)); return 0; }
		if (a1 == 9) { g_DiskState->SaveFileState(reinterpret_cast<FileInfo*>(p1), a2 != 0); return 0; }
		if (a1 == 10) return g_DiskState->LoadFileState(reinterpret_cast<FileInfo*>(p1), g_ServerPool->GetServers(), a2 != 0) ? 1 : 0;
		if (a1 == 11) { g_DiskState->LoadArticles(reinterpret_cast<FileInfo*>(p1)); return 0; }
		if (a1 == 12)
		{
			FileInfo fi(a2);
			if (g_DiskState->LoadFileState(&fi, g_ServerPool->GetServers(), true)) g_DiskState->SaveFileState(&fi, true);
		}
		return 0;
	}
	case 6: {
		if (a1 == 0 && g_HistoryCoordinator) g_HistoryCoordinator->DeleteDiskFiles(reinterpret_cast<NzbInfo*>(p1));
		else if (a1 == 1 && g_HistoryCoordinator) g_HistoryCoordinator->Redownload(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<HistoryInfo*>(p2));
		else if (a1 == 2 && g_HistoryCoordinator) g_HistoryCoordinator->HistoryHide(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<HistoryInfo*>(p2), a2);
		else if (a1 == 3 && g_PrePostProcessor) g_PrePostProcessor->CheckDownloadCompleted(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<NzbInfo*>(p2));
		else if (a1 == 4 && g_QueueScriptCoordinator) g_QueueScriptCoordinator->EnqueueScript(reinterpret_cast<NzbInfo*>(p1), (QueueScriptCoordinator::EEvent)a2);
		else if (a1 == 5) StreamRepairController::StartLive(reinterpret_cast<NzbInfo*>(p1));
		else if (a1 == 6) return nzbget_rs_article_dl_exec(nullptr, 23, 0, 0, p1, 0, s1 ? s1 : "", NzbgetCArticleDlHost, nullptr);
		else if (a1 == 7) DupeProbe::LaunchRs(reinterpret_cast<NzbgetRsDupeProbe*>(p1));
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
		else if (a1 == 11) return g_UrlCoordinator ? (g_UrlCoordinator->DeleteQueueEntry(reinterpret_cast<DownloadQueue*>(p1), reinterpret_cast<NzbInfo*>(p2), a2 != 0) ? 1 : 0) : 0;
		else if (a1 == 12 && g_DupeSearch) g_DupeSearch->NotifyNameChanged(a2);
		else if (a1 == 13) return (g_QueueScriptCoordinator && g_QueueScriptCoordinator->HasJob(a2, nullptr)) ? 1 : 0;
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
			auto* urlInfo = reinterpret_cast<NzbInfo*>((uintptr_t)i64s[0]);
			if (urlInfo) dq->GetQueue()->insert(dq->GetQueue()->Find(urlInfo), std::move(*up));
			else dq->GetQueue()->Add(std::move(*up), a2 != 0);
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
		return 0;
	}
	case 25: {
		if (!self) return 0;
		if (a1 == 0) return reinterpret_cast<uintptr_t>(static_cast<DownloadQueue*>(&self->m_downloadQueue));
		if (a1 == 1) CoordinatorDownloadQueue::Init(&self->m_downloadQueue);
		else if (a1 == 2) CoordinatorDownloadQueue::Loaded();
		else if (a1 == 3) CoordinatorDownloadQueue::Final();
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
		i64s[0] = n->GetSize(); i64s[1] = n->GetRemainingSize(); i64s[2] = n->GetPausedSize();
		i64s[3] = n->GetSuccessSize(); i64s[4] = n->GetCurrentSuccessSize(); i64s[5] = n->GetFailedSize();
		i64s[6] = n->GetCurrentFailedSize(); i64s[7] = n->GetParSize(); i64s[8] = n->GetParSuccessSize();
		i64s[9] = n->GetParCurrentSuccessSize(); i64s[10] = n->GetParFailedSize();
		i64s[11] = n->GetParCurrentFailedSize(); i64s[12] = n->GetDownloadedSize();
		i64s[13] = (int64_t)n->GetMinTime(); i64s[14] = (int64_t)n->GetMaxTime();
		i64s[15] = (int64_t)n->GetFilteredContentHash(); i64s[16] = (int64_t)n->GetDownloadStartTime();
		i64s[17] = n->GetDupeRecoveredBytes(); i64s[18] = (int64_t)reinterpret_cast<uintptr_t>(pi);
		strs[0] = n->GetName(); strs[1] = n->GetFilename(); strs[2] = n->GetDestDir(); strs[3] = n->GetFinalDir();
		strs[4] = n->GetCategory(); strs[5] = n->GetDupeKey(); strs[6] = n->GetQueuedFilename();
		strs[7] = a1 != 0 ? n->MakeTextStatus(true) : nullptr;
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
		case 40: n->SetName(s1); break;
		case 41: n->SetFilename(s1); break;
		case 42: n->SetCategory(s1); break;
		case 43: n->SetDestDir(s1); break;
		case 44: n->SetDupeKey(s1); break;
		case 45: n->SetQueuedFilename(s1); break;
		case 46: n->SetHardLinkPath(s1 ? s1 : ""); break;
		case 50: n->BuildDestDirName(); break;
		case 51: n->UpdateMinMaxTime(); break;
		case 52: n->UpdateCompletedStats(reinterpret_cast<FileInfo*>(p2)); break;
		case 53: n->UpdateDeletedStats(reinterpret_cast<FileInfo*>(p2)); break;
		case 54: n->EnterPostProcess(); break;
		case 55: n->LeavePostProcess(); break;
		case 56: n->GetScriptStatuses()->clear(); break;
		case 57: n->SetUnpackThread(nullptr); n->SetLiveRepairThread(nullptr); break;
		case 60: n->SetCleanupDisk(a2 != 0); break;
		case 61: n->SetDupeMode((EDupeMode)a2); break;
		case 62: n->SetParStatus((NzbInfo::EParStatus)a2); break;
		case 63: n->SetUnpackStatus((NzbInfo::EPostUnpackStatus)a2); break;
		case 64: n->SetDirectUnpackStatus((NzbInfo::EDirectUnpackStatus)a2); break;
		case 65: n->SetPostTotalSec(a2); break;
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
		if (a1 == 0) return (g_ArticleCache && g_ArticleCache->FileBusy(f)) ? 1 : 0;
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
			ad->ExecRs(10, (int)ad->GetStatus(), reinterpret_cast<uintptr_t>(ints),
				reinterpret_cast<uintptr_t>(i64s), reinterpret_cast<const char*>(strs));
			return reinterpret_cast<uintptr_t>(ad);
		}
		if (a1 == 1) ad->Stop();
		else if (a1 == 2) ad->ExecRs(14, (int)ad->GetStatus(), (uintptr_t)(int64_t)ad->GetLastUpdateTime());
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
		auto* ad = new ArticleDownloader(f, a, reinterpret_cast<NntpConnection*>((uintptr_t)i64s[0]),
			a1 != 0, s1 ? s1 : "");
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
			i64s[0] = (int64_t)pi->GetStartTime(); i64s[1] = (int64_t)reinterpret_cast<uintptr_t>(pi->GetNzbInfo());
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
			return 0;
		}
		if (a1 == 2) { pi->SetStartTime((time_t)i64s[0]); return 0; }
		if (a1 == 3) { pi->SetStageTime((time_t)i64s[0]); pi->SetStageProgress(0); pi->SetFileProgress(0); pi->SetProgressLabel(""); return 0; }
		return 0;
	}
	case 61: {
		PrePostProcessor* pp = reinterpret_cast<PrePostProcessor*>(p1);
		return (pp && pp->IsStopped()) ? 1 : 0;
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
			if (pi && pi->GetPostThread()) return static_cast<RepairController*>(pi->GetPostThread())->AddPar(reinterpret_cast<FileInfo*>(p2), a2 != 0) ? 1 : 0;
#endif
			return 0;
		}
		if (a1 == 12)
		{
			PostInfo* pi = reinterpret_cast<PostInfo*>(p1);
			if (!pi) return 0;
			if (a2 == 0) RenameController::StartJob(pi, RenameController::jkPar);
			else if (a2 == 1) RenameController::StartJob(pi, RenameController::jkRar);
			else if (a2 == 2) StreamRepairController::StartJob(pi);
#ifndef DISABLE_PARCHECK
			else if (a2 == 3) RepairController::StartJob(pi);
#endif
			else if (a2 == 4) UnpackController::StartJob(pi);
			else if (a2 == 5) CleanupController::StartJob(pi);
			else if (a2 == 6) MoveController::StartJob(pi);
			else if (a2 == 7) PostRenamerController::StartJob(pi, false);
			else if (a2 == 8) PostRenamerController::StartJob(pi, true);
			else if (a2 == 9) PostScriptController::StartJob(pi);
			return 0;
		}
		return 0;
	}
	default: return 0;
	}
}

inline bool DupeArticleFallback::ShouldDeferToPar(NzbInfo* nzbInfo)
{
	return nzbget_rs_dupe_fallback_op(1, 0, reinterpret_cast<uintptr_t>(nzbInfo), &QueueCoordinator::HostCb) != 0;
}

inline HistoryInfo* DupeCoordinator::FindDupeBackup(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, const char* nzbName, const char* dupeKey)
{
	return reinterpret_cast<HistoryInfo*>(nzbget_rs_dupe_coord_exec(3, 0,
		reinterpret_cast<uintptr_t>(downloadQueue), reinterpret_cast<uintptr_t>(nzbInfo),
		nzbName, dupeKey, &QueueCoordinator::HostCb));
}

inline void DupeCoordinator::HistoryMark(DownloadQueue* downloadQueue, HistoryInfo* historyInfo, NzbInfo::EMarkStatus markStatus)
{
	nzbget_rs_dupe_coord_exec(4, (int)markStatus, reinterpret_cast<uintptr_t>(downloadQueue),
		reinterpret_cast<uintptr_t>(historyInfo), nullptr, nullptr, &QueueCoordinator::HostCb);
}

inline DupeCoordinator::EDupeStatus DupeCoordinator::GetDupeStatus(DownloadQueue* downloadQueue, const char* name, const char* dupeKey)
{
	return (EDupeStatus)nzbget_rs_dupe_coord_exec(5, 0, reinterpret_cast<uintptr_t>(downloadQueue),
		0, name, dupeKey, &QueueCoordinator::HostCb);
}
#endif
#endif

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2012-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef WEBSERVER_H
#define WEBSERVER_H

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef FEEDCOORDINATOR_H
#define FEEDCOORDINATOR_H

class FeedCoordinator : public Thread, public Observer, public Subject, public Debuggable
{
public:
	FeedCoordinator() : m_rs(nzbget_rs_feed_coord_new())
	{
		debug("Creating FeedCoordinator");
		m_downloadQueueObserver.m_owner = this;
		DownloadQueue::Guard()->Attach(&m_downloadQueueObserver);
		m_workStateObserver.m_owner = this;
		g_WorkState->Attach(&m_workStateObserver);
	}

	~FeedCoordinator() override
	{
		CallCtx cc{this, nullptr, nullptr};
		NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
		nzbget_rs_feed_coord_exec(m_rs, 12, &env);
		nzbget_rs_feed_coord_free(m_rs);
	}

	void Run() override { CallCtx cc{this, nullptr, nullptr}; NzbgetRsFeedCoordEnv env = MakeEnv(&cc); nzbget_rs_feed_coord_exec(m_rs, 1, &env); }

	void Stop() override
	{
		Thread::Stop();
		CallCtx cc{this, nullptr, nullptr};
		NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
		nzbget_rs_feed_coord_exec(m_rs, 2, &env);
	}

	void Update(Subject* caller, void*) override
	{
		CallCtx cc{this, nullptr, nullptr};
		NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
		env.p1 = reinterpret_cast<uintptr_t>(static_cast<WebDownloader*>(caller));
		nzbget_rs_feed_coord_exec(m_rs, 3, &env);
	}

	void AddFeed(int id, const char* name, const char* url, bool backlog, int interval,
		const char* filter, bool pauseNzb, const char* category, int categorySource,
		int priority, const char* extensions, unsigned int certVerifLevel)
	{
		CallCtx cc{this, nullptr, nullptr};
		NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
		env.a1 = id; env.a2 = interval; env.a3 = categorySource; env.a4 = priority;
		env.a5 = (backlog ? 1 : 0) | (pauseNzb ? 2 : 0);
		env.u1 = certVerifLevel;
		env.s1 = name; env.s2 = url; env.s3 = filter; env.s4 = category; env.s5 = extensions;
		nzbget_rs_feed_coord_exec(m_rs, 0, &env);
	}

	void FetchFeed(int id)
	{
		CallCtx cc{this, nullptr, nullptr};
		NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
		env.a1 = id;
		nzbget_rs_feed_coord_exec(m_rs, 6, &env);
	}

	void FetchFeedList(const char* feedList)
	{
		CallCtx cc{this, nullptr, nullptr};
		NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
		env.s1 = feedList;
		nzbget_rs_feed_coord_exec(m_rs, 7, &env);
	}

	bool HasActiveDownloads() const { return nzbget_rs_feed_coord_exec(m_rs, 8, nullptr) != 0; }

	int EmitViewOrPreview(int flags, int id, int categorySource, int priority, int interval,
		int cacheTimeSec, const char* name, const char* url, const char* filter,
		const char* category, const char* extensions, const char* cacheId,
		void* rpcUser, void (*onItem)(void*, const NzbgetRsRpcFeedItem*))
	{
		CallCtx cc{this, rpcUser, onItem};
		NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
		env.a1 = flags; env.a2 = id; env.a3 = categorySource;
		env.a4 = priority; env.a5 = interval; env.a6 = cacheTimeSec;
		env.s1 = name; env.s2 = url; env.s3 = filter;
		env.s4 = category; env.s5 = extensions; env.s6 = cacheId;
		env.rpc_emit_cb = +[](void* raw, const NzbgetRsRpcFeedItem* item) {
			auto* c = static_cast<CallCtx*>(raw);
			if (c->onItem) c->onItem(c->user, item);
		};
		return nzbget_rs_feed_coord_exec(m_rs, 9, &env);
	}

	const NzbgetRsFeedCoord* RsHandle() const { return m_rs; }

protected:
	void LogDebugInfo() override { CallCtx cc{this, nullptr, nullptr}; NzbgetRsFeedCoordEnv env = MakeEnv(&cc); nzbget_rs_feed_coord_exec(m_rs, 11, &env); }

private:
	struct CallCtx { FeedCoordinator* self; void* user; void (*onItem)(void*, const NzbgetRsRpcFeedItem*); };

	class DownloadQueueObserver final : public Observer
	{
	public:
		FeedCoordinator* m_owner;
		void Update(Subject*, void* aspect) override
		{
			auto* qa = static_cast<DownloadQueue::Aspect*>(aspect);
			CallCtx cc{m_owner, nullptr, nullptr};
			NzbgetRsFeedCoordEnv env = MakeEnv(&cc);
			env.a1 = static_cast<int>(qa->action);
			env.s1 = (qa->action == DownloadQueue::eaUrlCompleted && qa->nzbInfo) ? qa->nzbInfo->GetUrl() : nullptr;
			nzbget_rs_feed_coord_exec(m_owner->m_rs, 4, &env);
		}
	};

	class WorkStateObserver final : public Observer
	{
	public:
		FeedCoordinator* m_owner;
		void Update(Subject*, void*) override { nzbget_rs_feed_coord_exec(m_owner->m_rs, 5, nullptr); }
	};

	NzbgetRsFeedCoord* m_rs;
	DownloadQueueObserver m_downloadQueueObserver;
	WorkStateObserver m_workStateObserver;

	static NzbgetRsFeedCoordEnv MakeEnv(CallCtx* cc)
	{
		NzbgetRsFeedCoordEnv env{};
		env.ctx = cc;
		env.log_cb = +[](void*, int level, const char* msg) {
			if (level == 0) debug("%s", msg);
			else if (level == 1) detail("%s", msg);
			else if (level == 2) info("%s", msg);
			else if (level == 3) warn("%s", msg);
			else if (level == 4) error("%s", msg);
		};
		env.query_cb = +[](void* raw, int op, uintptr_t ptr, int* outInts, int64_t* outI64s, const char** outStrs) -> uintptr_t {
			if (op == 0) return DownloadQueue::IsLoaded() ? 1 : 0;
			if (op == 1) return static_cast<CallCtx*>(raw)->self->IsStopped() ? 1 : 0;
			if (op == 2)
			{
				outInts[0] = g_Options->GetServerMode() ? 1 : 0;
				outInts[1] = g_WorkState->GetPauseDownload() ? 1 : 0;
				outInts[2] = g_Options->GetUrlForce() ? 1 : 0;
				outInts[3] = g_Options->GetUrlConnections();
				outInts[4] = g_Options->GetUrlTimeout();
				outInts[5] = g_Options->GetFeedHistory();
				outInts[6] = g_Options->GetFlushQueue() ? 1 : 0;
				outInts[7] = static_cast<int>(PATH_SEPARATOR);
				outStrs[0] = g_Options->GetTempDir();
				outStrs[1] = g_Options->GetQueueDir();
				outStrs[2] = g_Options->GetExtensions();
				return 1;
			}
			if (op == 3)
			{
				auto* dl = reinterpret_cast<WebDownloader*>(ptr);
				outInts[0] = static_cast<int>(dl->GetStatus());
				if (outI64s) outI64s[0] = static_cast<int64_t>(dl->GetLastUpdateTime());
				outStrs[0] = dl->GetInfoName();
				outStrs[1] = dl->GetOutputFilename();
				return 1;
			}
			return 0;
		};
		env.action_cb = +[](void* raw, int op, uintptr_t p1, int a1, int a2, int a3, int64_t t1, int64_t t2, const char* s1, const char* s2, const char* s3, const char* s4) -> uintptr_t {
			auto* self = static_cast<CallCtx*>(raw)->self;
			if (op == 0)
			{
				auto* dl = new WebDownloader();
				dl->SetAutoDestroy(true);
				dl->Attach(self);
				dl->SetUrl(s1 ? s1 : "");
				dl->SetInfoName(s2 ? s2 : "");
				dl->SetForce(a1 != 0);
#ifndef DISABLE_TLS
				dl->SetCertVerifLevel(static_cast<unsigned int>(a2));
#endif
				dl->SetOutputFilename(s3 ? s3 : "");
				return reinterpret_cast<uintptr_t>(dl);
			}
			if (op == 1)
			{
				auto* dl = reinterpret_cast<WebDownloader*>(p1);
				if (a1 == 0) dl->Stop();
				else if (a1 == 1) dl->LogDebugInfo();
				else if (a1 == 2) delete dl;
				else if (a1 == 3) dl->Start();
				return 1;
			}
			if (op == 2) return static_cast<uintptr_t>(g_DupeCoordinator->GetDupeStatus(DownloadQueue::Guard(), s1 ? s1 : "", s2 ? s2 : ""));
			if (op == 3)
			{
				int ok = nzbget_rs_feed_scripts_exec(
					g_ExtensionManager ? g_ExtensionManager->Handle() : nullptr,
					s1 ? s1 : "", s2 ? s2 : "", a1, &ScriptHostCb);
				return (a2 == 0 || ok != 0) ? 1 : 0;
			}
			if (op == 4)
			{
				auto nzbInfo = std::make_unique<NzbInfo>();
				nzbInfo->SetKind(NzbInfo::nkUrl);
				nzbInfo->SetFeedId(static_cast<int>(p1));
				nzbInfo->SetUrl(s1 ? s1 : "");
				if (s2 && *s2) nzbInfo->SetFilename(s2);
				nzbInfo->SetCategory(s3 ? s3 : "");
				nzbInfo->SetPriority(a2);
				nzbInfo->SetAddUrlPaused((a1 & 1) != 0);
				nzbInfo->SetDupeKey(s4 ? s4 : "");
				nzbInfo->SetDupeScore(a3);
				nzbInfo->SetDupeMode(static_cast<EDupeMode>((a1 >> 1) & 0xff));
				nzbInfo->SetSize(t1);
				nzbInfo->SetMinTime(static_cast<time_t>(t2));
				nzbInfo->SetMaxTime(static_cast<time_t>(t2));
				g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), false);
				return 1;
			}
			return 0;
		};
		return env;
	}
};

extern FeedCoordinator* g_FeedCoordinator;

#
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2008-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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


#ifndef SCHEDULER_H
#define SCHEDULER_H

class Scheduler : public Service
{
public:
	enum ECommand
	{
		scPauseDownload,
		scUnpauseDownload,
		scPausePostProcess,
		scUnpausePostProcess,
		scDownloadRate,
		scExtensions,
		scProcess,
		scPauseScan,
		scUnpauseScan,
		scActivateServer,
		scDeactivateServer,
		scFetchFeed
	};

	Scheduler() : m_rs(nzbget_rs_scheduler_new()) {}
	~Scheduler() override { nzbget_rs_scheduler_free(m_rs); }
	Scheduler(const Scheduler&) = delete;
	Scheduler& operator=(const Scheduler&) = delete;

	void AddTask(int id, int hours, int minutes, int weekDaysBits, ECommand command, const char* param)
	{
		NzbgetRsSchedulerEnv env{};
		env.a1 = id;
		env.a2 = hours;
		env.a3 = minutes;
		env.a4 = weekDaysBits;
		env.a5 = static_cast<int>(command);
		env.s1 = param;
		nzbget_rs_scheduler_exec(m_rs, 0, &env);
	}

	const NzbgetRsScheduler* RsHandle() const { return m_rs; }

protected:
	int ServiceInterval() override { return nzbget_rs_scheduler_exec(m_rs, 1, nullptr); }

	void ServiceWork() override
	{
		NzbgetRsSchedulerEnv env{};
#ifdef DEBUG
		env.a5 = 1;
#endif
		env.log_cb = +[](void*, int level, const char* msg) {
			const char* s = msg ? msg : "";
			if (level == 0) debug("%s", s);
			else if (level == 2) info("%s", s);
			else if (level == 4) error("%s", s);
		};
		env.action_cb = +[](void*, int sub, int a1, int a2, const char* const* ptrs, size_t len, int* ints, int64_t* i64s, const char** strs) -> int {
			if (sub == 0)
			{
				if (!DownloadQueue::IsLoaded()) return 0;
				ints[0] = g_WorkState->GetLocalTimeOffset();
				ints[1] = g_WorkState->GetLocalTimeOffset();
				ints[2] = g_WorkState->GetSpeedLimit();
				ints[3] = g_WorkState->GetPauseDownload() ? 1 : 0;
				ints[4] = g_WorkState->GetPausePostProcess() ? 1 : 0;
				ints[5] = g_WorkState->GetPauseScan() ? 1 : 0;
				i64s[0] = static_cast<int64_t>(g_WorkState->GetResumeTime());
				return 1;
			}
			if (sub == 1)
			{
				if (a1 == 0) g_WorkState->SetSpeedLimit(a2);
				else if (a1 == 1) g_WorkState->SetPauseDownload(a2 != 0);
				else if (a1 == 2) g_WorkState->SetPausePostProcess(a2 != 0);
				else if (a1 == 3) g_WorkState->SetPauseScan(a2 != 0);
				else if (a1 == 4)
				{
					g_WorkState->SetResumeTime(0);
					g_WorkState->SetPauseDownload(false);
					g_WorkState->SetPausePostProcess(false);
					g_WorkState->SetPauseScan(false);
				}
				return 1;
			}
			if (sub == 2)
			{
				Servers* srvs = g_ServerPool->GetServers();
				if (a1 == 0) return static_cast<int>(srvs->size());
				if (a1 == 1)
				{
					NewsServer* srv = (*srvs)[static_cast<size_t>(a2)].get();
					ints[0] = srv->GetId();
					ints[1] = srv->GetActive() ? 1 : 0;
					strs[0] = srv->GetName();
					return 1;
				}
				if (a1 == 2) { (*srvs)[static_cast<size_t>(a2)]->SetActive(ints[0] != 0); return 1; }
				if (a1 == 3) { g_ServerPool->Changed(); return 1; }
			}
			if (sub == 3) { g_FeedCoordinator->FetchFeedList(ptrs[0]); return 1; }
			if (sub == 4)
			{
				nzbget_rs_scheduler_script_start(
					g_ExtensionManager ? g_ExtensionManager->Handle() : nullptr,
					ptrs[0], a1, a2, ptrs + 1, len > 0 ? len - 1 : 0, &ScriptHostCb);
				return 1;
			}
			if (sub == 6) { i64s[0] = static_cast<int64_t>(Util::CurrentTime()); return 1; }
			return 0;
		};
		nzbget_rs_scheduler_exec(m_rs, 2, &env);
	}

private:
	NzbgetRsScheduler* m_rs;
};

#ifndef SYSTEM_HEALTH_H
#define SYSTEM_HEALTH_H

namespace SystemHealth
{
class Service
{
public:
	Service(const Options& options, const Servers& servers, const ::FeedCoordinator& feedCoordinator,
			const ::Scheduler& scheduler, const ::Log& log)
		: m_options(options), m_servers(servers), m_feedCoordinator(feedCoordinator), m_scheduler(scheduler), m_log(log)
	{
	}

	void DiagnoseAndLog() const
	{
		NzbgetRsBuf buf = RunDiagnose(0);
		nzbget_rs_free(buf);
	}

	NzbgetRsBuf DiagnoseSerialized(bool isJson) const
	{
		return RunDiagnose(isJson ? 1 : 2);
	}

private:
	NzbgetRsBuf RunDiagnose(int formatMode) const
	{
		std::vector<NzbgetRsHealthServerInput> servers;
		servers.reserve(m_servers.size());
		for (const auto& s : m_servers)
		{
			servers.push_back(NzbgetRsHealthServerInput{
				s->GetId(), s->GetActive() ? 1 : 0, s->GetName(), s->GetLevel(),
				s->GetOptional() ? 1 : 0, s->GetGroup(), s->GetHost(), s->GetPort(),
				s->GetUser(), s->GetPassword(), s->GetTls() ? 1 : 0, s->GetCipher(),
				s->GetMaxConnections(), s->GetRetention(),
				static_cast<uint32_t>(s->GetCertVerificationLevel()), s->GetIpVersion()
			});
		}
		return nzbget_rs_system_health_diagnose(
			m_options.RsHandle(), servers.data(), servers.size(),
			m_feedCoordinator.RsHandle(), m_scheduler.RsHandle(), m_log.RsHandle(),
			m_options.GetDaemonMode() ? 1 : 0, formatMode);
	}

	const Options& m_options;
	const Servers& m_servers;
	const ::FeedCoordinator& m_feedCoordinator;
	const ::Scheduler& m_scheduler;
	const ::Log& m_log;
};
}  // namespace SystemHealth

extern SystemHealth::Service* g_SystemHealth;

#endif

#ifndef MAINTENANCE_H
#define MAINTENANCE_H


extern void ExitProc();
extern int g_ArgumentCount;
extern char* (*g_Arguments)[];

class Maintenance;
extern Maintenance* g_Maintenance;

class Maintenance
{
public:
	enum EBranch
	{
		brStable,
		brTesting,
		brDevel
	};

	Maintenance() : m_rs(nzbget_rs_maintenance_new()) {}
	~Maintenance()
	{
		NzbgetRsMaintenanceEnv env{};
		env.script_host_cb = &ScriptHostCb;
		nzbget_rs_maintenance_exec(m_rs, 0, &env);
		nzbget_rs_maintenance_free(m_rs);
	}
	Maintenance(const Maintenance&) = delete;
	Maintenance& operator=(const Maintenance&) = delete;

	void CollectMessages(int idFrom, int nrEntries, void* emitCtx, NzbgetRsXmlRpcEmitFn emitCb)
	{
		NzbgetRsMaintenanceEnv env{};
		env.a3 = idFrom;
		env.a4 = nrEntries;
		env.emit_ctx = emitCtx;
		env.emit_cb = emitCb;
		nzbget_rs_maintenance_exec(m_rs, 3, &env);
	}

	bool StartUpdate(EBranch branch)
	{
		NzbgetRsMaintenanceEnv env{};
		env.a1 = static_cast<int>(branch);
		env.a2 = PATH_SEPARATOR;
		env.a3 = g_Options->GetDaemonMode() ? 1 : 0;
		env.s1 = g_Options->GetWebDir();
		env.s2 = g_Options->GetAppDir();
		env.script_host_cb = &ScriptHostCb;
		return nzbget_rs_maintenance_exec(m_rs, 4, &env) != 0;
	}

	bool CheckUpdates(CString& updateInfo)
	{
		NzbgetRsBuf buf{};
		NzbgetRsMaintenanceEnv env{};
		env.ctx = this;
		env.a2 = PATH_SEPARATOR;
		env.s1 = g_Options->GetWebDir();
		env.out_buf = &buf;
		env.script_host_cb = &ScriptHostCb;
		if (nzbget_rs_maintenance_exec(m_rs, 5, &env) != 0 && buf.len > 0)
			updateInfo.Set(buf.data ? buf.data : "", static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return !updateInfo.Empty();
	}

	static bool VerifySignature(const char* inFilename, const char* sigFilename, const char* pubKeyFilename)
	{
		NzbgetRsMaintenanceEnv env{};
		env.s1 = inFilename;
		env.s2 = sigFilename;
		env.s3 = pubKeyFilename;
		env.action_cb = &Maintenance::MaintActionCb;
		return nzbget_rs_maintenance_exec(nullptr, 8, &env) != 0;
	}

private:
	NzbgetRsMaintenance* m_rs;

	static uintptr_t MaintActionCb(void*, int sub, const char* s1, const char* s2, const uint8_t* bytes, size_t len)
	{
		if (sub == 6)
		{
#ifndef DISABLE_TLS
			BIO* mem = BIO_new_mem_buf(bytes, static_cast<int>(len));
			EVP_PKEY* pubKey = PEM_read_bio_PUBKEY(mem, nullptr, nullptr, nullptr);
			BIO_free(mem);
			if (!pubKey) return 0;
			EVP_PKEY_CTX* pctx = EVP_PKEY_CTX_new(pubKey, nullptr);
			bool ok = pctx &&
				EVP_PKEY_verify_init(pctx) == 1 &&
				EVP_PKEY_CTX_set_rsa_padding(pctx, RSA_PKCS1_PADDING) > 0 &&
				EVP_PKEY_CTX_set_signature_md(pctx, EVP_sha256()) > 0 &&
				EVP_PKEY_verify(pctx, reinterpret_cast<const uint8_t*>(s2), 256, reinterpret_cast<const uint8_t*>(s1), 32) == 1;
			if (pctx) EVP_PKEY_CTX_free(pctx);
			EVP_PKEY_free(pubKey);
			return ok ? 1 : 0;
#else
			(void)s1; (void)s2; (void)bytes; (void)len;
			return 0;
#endif
		}
		return 0;
	}
};

#endif

#endif

#endif
#include <system_error>

/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef FRONTEND_H
#define FRONTEND_H
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@users.sourceforge.net>
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
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


extern void ExitProc();

class Frontend : public Thread
{
public:
	explicit Frontend(int mode) : m_mode(mode)
	{
		if (m_mode != 3)
		{
			debug("Creating Frontend");
			m_workStateObserver.m_owner = this;
			if (g_WorkState)
			{
				g_WorkState->Attach(&m_workStateObserver);
			}
			if (g_Options)
			{
				m_updateInterval = g_Options->GetUpdateInterval();
			}
		}
		if (m_mode == 2)
		{
			m_useColor = nzbget_rs_frontend_op(0, nullptr) > 0;
		}
	}

	~Frontend() override
	{
		if (m_mode != 3 && g_WorkState)
		{
			g_WorkState->Detach(&m_workStateObserver);
		}
		if (m_mode == 2)
		{
			nzbget_rs_frontend_op(1, nullptr);
			printf("\n");
		}
	}

	void Stop() override
	{
		Thread::Stop();
		m_waitCond.notify_all();
	}

protected:
	void Run() override
	{
		if (m_mode == 0 || m_mode == 1)
		{
			debug("Entering LoggableFrontend-loop");
		}
		else if (m_mode == 2)
		{
			debug("Entering NCursesFrontend-loop");
		}

		NzbgetRsFrontendEnv env{};
		env.ctx = this;
		env.remote_mode = (m_mode != 3 && g_Options && g_Options->GetRemoteClientMode()) ? 1 : 0;
		env.update_interval = m_updateInterval;
		env.use_color = m_useColor ? 1 : 0;
		env.path_sep = (uint8_t)PATH_SEPARATOR;
		if (m_mode != 3 && g_Options)
		{
			env.show_nzbname = g_Options->GetCursesNzbName() ? 1 : 0;
			env.show_timestamp = g_Options->GetCursesTime() ? 1 : 0;
			env.group_files = g_Options->GetCursesGroup() ? 1 : 0;
			env.time_correction = g_Options->GetTimeCorrection();
			env.control_ip = g_Options->GetControlIp();
			env.control_port = g_Options->GetControlPort();
			env.control_username = g_Options->GetControlUsername();
			env.control_password = g_Options->GetControlPassword();
		}
		env.write_stdout_cb = [](void*, const char* ptr, size_t len, int doFlush) {
			if (ptr && len > 0)
			{
				fwrite(ptr, 1, len, stdout);
			}
			if (doFlush)
			{
				fflush(stdout);
			}
		};
		env.action_cb = [](void* raw, int op, int a1, int a2, int a3) -> int64_t {
			Frontend* self = static_cast<Frontend*>(raw);
			switch (op)
			{
			case 0:
				return self->IsStopped() ? 1 : 0;
			case 1:
				if (self->m_mode != 3 && g_WorkState && g_WorkState->GetPauseFrontend())
				{
					std::unique_lock<std::mutex> lk(self->m_waitMutex);
					self->m_waitCond.wait_for(lk, std::chrono::milliseconds(2000));
				}
				else
				{
					Util::Sleep(a1);
				}
				return 0;
			case 2:
				self->Stop();
				return 0;
			case 3:
				if (a2)
				{
					info("%s", a1 ? "Pausing download" : "Unpausing download");
				}
				g_WorkState->SetResumeTime(0);
				g_WorkState->SetPauseDownload(a1 != 0);
				return 0;
			case 4:
				g_WorkState->SetSpeedLimit(a1);
				return 0;
			case 5:
				return DownloadQueue::Guard()->EditEntry(
					a3, static_cast<DownloadQueue::EEditAction>(a1), BString<100>("%i", a2)) ? 1 : 0;
			case 6:
				ExitProc();
				return 0;
			case 7:
				return static_cast<int64_t>(Util::CurrentTime());
			default:
				return 0;
			}
		};
		env.local_summary_cb = [](void*, NzbgetRsFrontendSummary* s) {
			bool standBy = false;
			s->current_download_speed = g_StatMeter->CalcCurrentDownloadSpeed();
			s->pause_download = g_WorkState->GetPauseDownload() ? 1 : 0;
			s->download_limit = g_WorkState->GetSpeedLimit();
			s->thread_count = Thread::GetThreadCount();
			g_StatMeter->CalcTotalStat(&s->up_time_sec, &s->dn_time_sec, &s->all_bytes, &standBy);
			s->stand_by = standBy ? 1 : 0;
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			s->post_job_count = 0;
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				s->post_job_count += nzbInfo->GetPostInfo() ? 1 : 0;
			}
			downloadQueue->CalcRemainingSize(&s->remaining_size, nullptr);
		};
		env.local_messages_cb = [](void*, int tailMode, uint32_t arg, int* outSkipped, void* emitCtx,
			void (*emit)(void*, uint32_t, int, int64_t, const char*)) {
			nzbget_rs_log_collect(
				0, g_Log ? g_Log->RsHandle() : nullptr, nullptr, tailMode, 0, arg,
				outSkipped, emitCtx, reinterpret_cast<uintptr_t>(emit), 0);
		};
		env.local_queue_cb = [](void*, void* emitCtx,
			void (*emitNzb)(void*, int, int, int, int64_t, int64_t, int64_t, int, int, const char*),
			void (*emitFile)(void*, size_t, int, int, int, int64_t, int64_t, const char*)) {
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			size_t idx = 0;
			for (NzbInfo* n : downloadQueue->GetQueue())
			{
				emitNzb(emitCtx, n->GetId(), n->GetPriority(), n->GetActiveDownloads(),
					n->GetSize(), n->GetRemainingSize(), n->GetPausedSize(),
					n->GetPausedFileCount(), n->GetRemainingParCount(), n->GetName());
				for (FileInfo* f : n->GetFileList())
				{
					emitFile(emitCtx, idx, f->GetId(), f->GetActiveDownloads(), f->GetPaused() ? 1 : 0,
						f->GetSize(), f->GetRemainingSize(), f->GetFilename());
				}
				idx++;
			}
		};

		nzbget_rs_frontend_op(2 + m_mode, &env);

		if (m_mode == 0 || m_mode == 1)
		{
			debug("Exiting LoggableFrontend-loop");
		}
		else if (m_mode == 2)
		{
			debug("Exiting NCursesFrontend-loop");
		}
	}

private:
	class WorkStateObserver : public Observer
	{
	public:
		Frontend* m_owner = nullptr;
		void Update(Subject*, void*) override { m_owner->m_waitCond.notify_all(); }
	};

	int m_mode;
	int m_updateInterval = 100;
	bool m_useColor = true;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	WorkStateObserver m_workStateObserver;
};

#endif

extern void Reload();

class RequestProcessor : public Thread, public Subject
{
public:
	~RequestProcessor()
	{
		m_connection->Disconnect();

#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
		nzbget_rs_cancel_reg_free(m_cancelRegistry);
	}

	void Run() override
	{
#ifdef DISABLE_TLS
		int tlsDisabled = 1;
#else
		int tlsDisabled = 0;
#endif
		nzbget_rs_req_proc_exec(
			m_connection->Handle(), m_cancelRegistry, m_tls ? 1 : 0, tlsDisabled,
			g_Options->GetSecurePort(), g_Options->GetControlPort(),
			g_Options->GetSecureCert(), g_Options->GetSecureKey(),
			this,
			+[](void* ctx) { static_cast<RequestProcessor*>(ctx)->ServBinRpc(); },
			+[](void* ctx, const char* sig) -> int {
				return static_cast<RequestProcessor*>(ctx)->ServWebRequest(sig) ? 1 : 0;
			},
			+[](void*, int lvl, const char* msg) {
				if (lvl == 0) debug("%s", msg); else warn("%s", msg);
			});
		Notify(nullptr);
	}

	void Stop() override
	{
		Thread::Stop();
		m_connection->Cancel();
		nzbget_rs_cancel_reg_stop(m_cancelRegistry);
	}

	void SetTls(bool tls) { m_tls = tls; }
	void SetConnection(std::unique_ptr<Connection>&& connection) { m_connection = std::move(connection); }

private:
	bool m_tls = false;
	std::unique_ptr<Connection> m_connection;
	NzbgetRsCancelReg* m_cancelRegistry = nzbget_rs_cancel_reg_new();

	bool ServWebRequest(const char* signature)
	{
		std::vector<const char*> scriptDirPtrs;
		nzbget_rs_options_query(g_Options->RsHandle(), 3, nullptr, 0, nullptr, nullptr,
			reinterpret_cast<uintptr_t>(&scriptDirPtrs),
			+[](uintptr_t raw, const char* p, const char*, int) {
				reinterpret_cast<std::vector<const char*>*>(raw)->push_back(p);
			});

		NzbgetRsWebServeEnv env{};
		env.ctx = m_connection.get();
		env.signature = signature;
		env.remote_addr = m_connection->GetRemoteAddr();
		env.version = Util::VersionRevision();
		env.web_dir = g_Options->GetWebDir();
		env.script_dirs = scriptDirPtrs.empty() ? nullptr : scriptDirPtrs.data();
		env.script_dirs_count = scriptDirPtrs.size();
		env.control_username = g_Options->GetControlUsername();
		env.control_password = g_Options->GetControlPassword();
		env.restricted_username = g_Options->GetRestrictedUsername();
		env.restricted_password = g_Options->GetRestrictedPassword();
		env.add_username = g_Options->GetAddUsername();
		env.add_password = g_Options->GetAddPassword();
		env.authorized_ip = g_Options->GetAuthorizedIp();
		env.config_filename = g_Options->GetConfigFilename();
		env.shell_override = g_Options->GetShellOverride();
		env.control_port = g_Options->GetControlPort();
		env.form_auth = g_Options->GetFormAuth() ? 1 : 0;
		env.json_p = g_Options->GetJsonP() ? 1 : 0;
#ifndef DISABLE_GZIP
		env.gzip_enabled = 1;
#endif
#ifdef DEBUG
		env.debug_build = 1;
#endif
		env.read_line_cb = [](void* c, char* buf, int sz) -> int {
			return static_cast<Connection*>(c)->ReadLine(buf, sz, nullptr) != nullptr ? 1 : 0;
		};
		env.recv_cb = [](void* c, char* buf, int sz) -> int {
			return static_cast<Connection*>(c)->Recv(buf, sz) ? 1 : 0;
		};
		env.send_cb = [](void* c, const char* buf, int sz) -> int {
			return static_cast<Connection*>(c)->Send(buf, sz) ? 1 : 0;
		};
		env.log_cb = [](void*, int lvl, const char* msg) {
			if (lvl == 0) debug("%s", msg);
			else if (lvl == 1) warn("%s", msg);
			else if (lvl == 2) error("%s", msg);
			else if (lvl == 3) info("%s", msg);
			else detail("%s", msg);
		};
		env.rpc_action_cb = [](void*, int op, int a1, int a2, int a3, int a4,
			const char* s1, const char* s2, const char* s3, const char* s4,
			const char* body, size_t bodyLen, const int32_t* ids, size_t idsLen,
			const NzbgetRsRpcParamItem* params, size_t paramsLen,
			uint32_t* outU32, double* outF64, NzbgetRsBuf* outBuf) -> int {
			auto setBuf = [&](const char* p, size_t n) {
				if (outBuf) *outBuf = nzbget_rs_alloc_buf(p, n);
			};
			switch (op)
			{
				case 1:
					if (a1 == 0) { int prev = g_WorkState->GetPauseDownload() ? 1 : 0; g_WorkState->SetPauseDownload(true); return prev; }
					g_WorkState->SetPauseDownload(a2 != 0);
					return 0;
				case 2:
					g_WorkState->SetResumeTime(0);
					if (a2 == 0) g_WorkState->SetPauseDownload(a1 != 0);
					else if (a2 == 1) g_WorkState->SetPausePostProcess(a1 != 0);
					else if (a2 == 2) g_WorkState->SetPauseScan(a1 != 0);
					else return 0;
					return 1;
				case 3: g_WorkState->SetResumeTime(Util::CurrentTime() + (time_t)a1); return 1;
				case 4: ExitProc(); return 1;
				case 5: Reload(); return 1;
				case 6: g_Log->LogDebugInfo(); return 1;
				case 7: g_WorkState->SetSpeedLimit(a1); return 1;
				case 8: if (outBuf) *outBuf = g_SystemInfo->FormatBuf(a1 ? 1 : 2, true); return 1;
				case 9:
					if (!g_Options->GetSystemHealthCheck()) return 0;
					if (outBuf) *outBuf = g_SystemHealth->DiagnoseSerialized(a1 != 0);
					return 1;
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
						g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), addTop);
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
				case 12: g_Log->Clear(); return 1;
				case 13: g_Scanner->ScanNzbDir(a1 != 0); return 1;
				case 14: {
					if (a1)
					{
						if (const auto& err = g_ExtensionManager->LoadExtensions())
						{
							setBuf(err->data(), err->size());
							return 0;
						}
					}
					if (outBuf) *outBuf = g_ExtensionManager->SerializeList(a2 != 0);
					return 1;
				}
				case 15: {
					if (a1 == 2)
					{
						if (const auto err = g_ExtensionManager->DeleteExtension(s1 ? s1 : "")) { setBuf(err->data(), err->size()); return 0; }
						return 1;
					}
					if (a1 == 0 && Util::EmptyStr(g_Options->GetTempDir())) { const char* m = "\"TempDir\" is not specified"; setBuf(m, strlen(m)); return 0; }
					const auto [status, filename] = g_ExtensionManager->DownloadExtension(s1 ? s1 : "", s2 ? s2 : "");
					auto rmDl = [&]() { fs::error_code ec; fs::remove(filename, ec); };
					if (status != WebDownloader::adFinished) { rmDl(); const char* m = "Failed to read URL"; setBuf(m, strlen(m)); return 0; }
					if (a1 == 0)
					{
						const char* firstDir = nullptr;
						if (nzbget_rs_options_query(g_Options->RsHandle(), 3, nullptr, 0, nullptr, &firstDir, 0, nullptr) == 0 || !firstDir)
						{ rmDl(); const char* m = "\"ScriptDir\" is not specified"; setBuf(m, strlen(m)); return 0; }
						if (const auto err = g_ExtensionManager->InstallOrUpdate(0, filename, firstDir))
						{
							rmDl();
							setBuf(err->data(), err->size());
							return 0;
						}
						return 1;
					}
					if (const auto err = g_ExtensionManager->InstallOrUpdate(1, filename, s2 ? s2 : "")) { setBuf(err->data(), err->size()); return 0; }
					return 1;
				}
				case 16: g_FeedCoordinator->FetchFeed(a1); return 1;
				case 17: {
					bool ok = false;
					for (size_t i = 0; i + 1 < idsLen; i += 2)
					{
						for (NewsServer* srv : g_ServerPool->GetServers())
						{
							if (srv->GetId() == ids[i]) { srv->SetActive(ids[i + 1] != 0); ok = true; }
						}
					}
					if (ok) g_ServerPool->Changed();
					return ok ? 1 : 0;
				}
				case 18: {
#ifdef DEBUG
					int dbg = 1;
#else
					int dbg = 0;
#endif
					NzbgetRsBuf buf{};
					int ok = nzbget_rs_webdl_read_temp_url(
						s1 ? s1 : "", s2 ? s2 : "", g_Options->GetTempDir(), dbg,
						&Connection::HostCb, nullptr, &WebDownloader::HostCb, &buf);
					if (ok && outBuf) *outBuf = buf; else nzbget_rs_free(buf);
					return ok ? 1 : 0;
				}
				case 19: {
					CString updateInfo;
					if (!g_Maintenance->CheckUpdates(updateInfo)) return 0;
					const char* p = *updateInfo ? *updateInfo : "";
					setBuf(p, strlen(p));
					return 1;
				}
				case 20: return g_Maintenance->StartUpdate((Maintenance::EBranch)a1) ? 1 : 0;
				case 21:
					return nzbget_rs_statmeter_rpc(
						g_StatMeter->RsHandle(), 0, g_WorkState->GetLocalTimeOffset(),
						a1, a2, 0, 0, 0, nullptr, nullptr);
				case 22: {
					std::string r = a1 == 0 ? TlsCipherBackend::Start() :
						a1 == 1 ? TlsCipherBackend::Status(s1) : TlsCipherBackend::Cancel(s1 ? s1 : "");
					setBuf(r.data(), r.size());
					return 1;
				}
				case 23: {
					NewsServer server(0, true, "test server", s1 ? s1 : "", a1, 0,
						s2 ? s2 : "", s3 ? s3 : "", false, a2 != 0, s4 ? s4 : "", 1, 0, 0, 0, false, a4);
					CString errText;
					class TestConn final : public NntpConnection
					{
					public:
						TestConn(NewsServer* ns, CString* out) : NntpConnection(ns), m_out(out) {}
					protected:
						CString* m_out;
						void PrintError(const char* m) override { if (m_out->Empty() && m) *m_out = m; }
					} conn(&server, &errText);
					conn.SetTimeout(a3 > 0 ? a3 : g_Options->GetArticleTimeout());
					conn.SetSuppressErrors(false);
					bool ok = conn.Connect();
					if (ok)
					{
						BString<1024> id;
						while (id.Length() < 30) id.Append(BString<100>("%i", rand()));
						const char* resp = conn.Request(BString<1024>("ARTICLE <%s@nzbget.com>\r\n", *id));
						ok = resp && (*resp == '4' || *resp == '2');
					}
					if (!ok && !errText.Empty()) setBuf(*errText, strlen(*errText));
					return ok ? 1 : 0;
				}
				case 24: {
					NewsServer* server = g_ServerPool->GetServerById(a1);
					if (!server || !server->GetActive() || server->GetMaxConnections() == 0)
					{
						const char* m = !server ? "Server not found" :
							!server->GetActive() ? "Server is not active" : "Server has no connections configured";
						setBuf(m, strlen(m));
						return 0;
					}
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
					g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), true);
					return 1;
				}
				case 25: {
#ifdef DISABLE_TLS
					int tlsDis = 1;
#else
					int tlsDis = 0;
#endif
#ifdef DEBUG
					int dbg = 1;
#else
					int dbg = 0;
#endif
					return nzbget_rs_net_speedtest_iter(
						body, bodyLen, a1, tlsDis, dbg, &Connection::HostCb, outU32, outF64);
				}
				case 26:
					return nzbget_rs_command_script_start(
						g_ExtensionManager ? g_ExtensionManager->Handle() : nullptr,
						s1 ? s1 : "", s2 ? s2 : "", g_CommandScriptLog->Reset(),
						params, paramsLen, &ScriptHostCb) != 0 ? 1 : 0;
				default: return 0;
			}
		};
		env.rpc_collect_cb = [](void*, int kind, int a1, int a2, int a3, int a4, int a5,
			const char* s1, const char* s2, const char* s3, const char* s4, const char* s5, const char* s6,
			const void* p1, size_t n1, void* emitCtx, NzbgetRsXmlRpcEmitFn emitCb) -> int {
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
					int postJobCount = 0, urlCount = 0;
					int64 remainingSize, forcedSize;
					{
						GuardedDownloadQueue dq = DownloadQueue::Guard();
						for (NzbInfo* n : dq->GetQueue())
						{
							postJobCount += n->GetPostInfo() != nullptr ? 1 : 0;
							urlCount += n->GetKind() == NzbInfo::nkUrl ? 1 : 0;
						}
						dq->CalcRemainingSize(&remainingSize, &forcedSize);
					}
					int upTimeSec, downloadTimeSec;
					int64 allBytes, monthBytes, dayBytes;
					bool serverStandBy;
					g_StatMeter->CalcTotalStat(&upTimeSec, &downloadTimeSec, &allBytes, &serverStandBy);
					g_StatMeter->CalcQuotaUsage(monthBytes, dayBytes);
					int64 freeDisk = 0, totalDisk = 0, freeInter = 0, totalInter = 0;
					if (!Util::EmptyStr(g_Options->GetDestDir()))
						if (auto r = FileSystem::GetDiskState(g_Options->GetDestDir())) { freeDisk = r->available; totalDisk = r->total; }
					bool interEmpty = Util::EmptyStr(g_Options->GetInterDir());
					if (!interEmpty)
						if (auto r = FileSystem::GetDiskState(g_Options->GetInterDir())) { freeInter = r->available; totalInter = r->total; }
					NzbgetRsRpcStatusInput sf{};
					sf.remaining_size = remainingSize; sf.forced_size = forcedSize;
					sf.article_cache = g_ArticleCache->GetAllocated();
					sf.all_bytes = allBytes; sf.month_bytes = monthBytes; sf.day_bytes = dayBytes;
					sf.memory_pressure = g_MemoryMonitor ? g_MemoryMonitor->GetTierName() : "NORMAL";
					sf.effective_article_cache_mb = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveArticleCacheMB() : g_Options->GetArticleCache();
					sf.effective_par_buffer_mb = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveParBufferMB(g_Options->GetParBuffer()) : g_Options->GetParBuffer();
					sf.download_rate = g_StatMeter->CalcCurrentDownloadSpeed();
					sf.download_limit = (int)g_WorkState->GetSpeedLimit();
					sf.thread_count = Thread::GetThreadCount() - 1;
					sf.post_job_count = postJobCount; sf.url_count = urlCount;
					sf.up_time_sec = upTimeSec; sf.download_time_sec = downloadTimeSec;
					sf.download_paused = g_WorkState->GetPauseDownload() ? 1 : 0;
					sf.server_stand_by = serverStandBy ? 1 : 0;
					sf.post_paused = g_WorkState->GetPausePostProcess() ? 1 : 0;
					sf.scan_paused = g_WorkState->GetPauseScan() ? 1 : 0;
					sf.quota_reached = g_WorkState->GetQuotaReached() ? 1 : 0;
					sf.free_disk_space = freeDisk; sf.total_disk_space = totalDisk;
					sf.inter_dir_empty = interEmpty ? 1 : 0;
					sf.free_inter_disk_space = freeInter; sf.total_inter_disk_space = totalInter;
					sf.server_time = (int)Util::CurrentTime(); sf.resume_time = (int)g_WorkState->GetResumeTime();
					sf.feed_active = g_FeedCoordinator->HasActiveDownloads() ? 1 : 0;
					sf.queued_scripts = g_QueueScriptCoordinator->GetQueueSize();
					std::vector<NzbgetRsRpcServerStatus> srvs;
					srvs.reserve(g_ServerPool->GetServers()->size());
					for (NewsServer* srv : g_ServerPool->GetServers()) srvs.push_back({srv->GetId(), srv->GetActive() ? 1 : 0});
					emitCb(emitCtx, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr,
						&sf, 0, srvs.data(), srvs.size(), nullptr, 0, nullptr, 0);
					return 1;
				}
				case 2: {
					if (a1 == 1) { g_Maintenance->CollectMessages(a3, a4, emitCtx, emitCb); return 1; }
					if (a1 == 2)
					{
						nzbget_rs_log_collect(
							1, nullptr, g_CommandScriptLog->RsHandle(), a3, a4, 0, nullptr,
							emitCtx, reinterpret_cast<uintptr_t>(emitCb), 0);
						return 1;
					}
					if (a1 != 3)
					{
						nzbget_rs_log_collect(
							1, g_Log->RsHandle(), nullptr, a3, a4, 0, nullptr,
							emitCtx, reinterpret_cast<uintptr_t>(emitCb), 0);
						return 1;
					}
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
						bool qsActive = false;
						bool hasQs = hasPost && postStage == (int)PostInfo::ptQueued &&
							g_QueueScriptCoordinator->HasJob(nzbInfo->GetId(), &qsActive);
						int flags = (hasPost ? 1 : 0) | (hasQs ? 2 : 0) | (qsActive ? 4 : 0) |
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
				case 8:
					return nzbget_rs_fleet_append(
						nullptr, &DupeSearch::HostCb, s1, s2, a1, a2,
						static_cast<const NzbgetRsRpcFleetReqMember*>(p1), n1, emitCtx, emitCb);
				case 9: {
					std::vector<NzbgetRsRpcConfigItem> entries;
					nzbget_rs_options_query(g_Options->RsHandle(), 1, nullptr, 0, nullptr, nullptr,
						reinterpret_cast<uintptr_t>(&entries),
						+[](uintptr_t raw, const char* n, const char* v, int r) {
							reinterpret_cast<std::vector<NzbgetRsRpcConfigItem>*>(raw)->push_back({n, v, r});
						});
					emitCb(emitCtx, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr,
						entries.data(), entries.size(), nullptr, 0, nullptr, 0, nullptr, 0);
					return 1;
				}
				case 10: {
					ScriptConfig::ConfigTemplates* tmpls = g_ScriptConfig->GetConfigTemplates();
					ScriptConfig::ConfigTemplates loaded;
					if (a1)
					{
						if (!g_ScriptConfig->LoadConfigTemplates(&loaded)) return 0;
						tmpls = &loaded;
					}
					std::vector<const char*> ptrs;
					ptrs.reserve(tmpls->size());
					for (ScriptConfig::ConfigTemplate& ct : tmpls) ptrs.push_back(ct.GetTemplate());
					emitCb(emitCtx, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr,
						ptrs.data(), ptrs.size(), nullptr, 0, nullptr, 0, nullptr, 0);
					return 1;
				}
				case 11: {
					struct RpcFeedEmit { decltype(emitCb) cb; void* ctx; } rctx{emitCb, emitCtx};
					return g_FeedCoordinator->EmitViewOrPreview(a1, a2, a3, a4, a5, (int)n1, s1, s2, s3, s4, s5, s6, &rctx,
						+[](void* raw, const NzbgetRsRpcFeedItem* item) {
							auto* r = static_cast<RpcFeedEmit*>(raw);
							r->cb(r->ctx, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0, nullptr, nullptr, nullptr, nullptr,
								item, 0, nullptr, 0, nullptr, 0, nullptr, 0);
						});
				}
				case 12:
					return nzbget_rs_statmeter_rpc(
						g_StatMeter->RsHandle(), 1, g_WorkState->GetLocalTimeOffset(),
						a1, a2, a3, a4, a5, emitCtx, emitCb);
				default: return 0;
			}
		};
		return nzbget_rs_web_serve(1, &env) != 0;
	}

	void ServBinRpc()
	{
		NzbgetRsBinRpcServeEnv env{};
		env.ctx = m_connection.get();
		env.remote_addr = m_connection->GetRemoteAddr();
		env.control_user = g_Options->GetControlUsername();
		env.control_password = g_Options->GetControlPassword();
		env.version = Util::VersionRevision();
		env.control_port = g_Options->GetControlPort();
		env.recv_cb = [](void* c, char* buf, int sz) -> int {
			return static_cast<Connection*>(c)->Recv(buf, sz) ? 1 : 0;
		};
		env.send_cb = [](void* c, const char* buf, int sz) -> int {
			return static_cast<Connection*>(c)->Send(buf, sz) ? 1 : 0;
		};
		env.log_cb = [](void*, int kind, const char* msg) {
			switch ((Message::EKind)kind)
			{
				case Message::mkInfo: info("%s", msg); break;
				case Message::mkWarning: warn("%s", msg); break;
				case Message::mkError: error("%s", msg); break;
				case Message::mkDebug: debug("%s", msg); break;
				case Message::mkDetail: detail("%s", msg); break;
			}
		};
		env.action_cb = [](void*, int kind, int a1, int a2, int a3, int a4,
			const char* s1, const char* s2, const char* s3, const char* body, int bodyLen,
			const int32_t* ids, size_t idsLen, const char* const* names, size_t namesLen) -> int {
			switch (kind)
			{
				case 1:
				{
					bool addTop = (a1 & 1) != 0;
					bool addPaused = (a1 & 2) != 0;
					bool autoCategory = (a1 & 4) != 0;
					bool isUrl = (a1 & 8) != 0;
					if (isUrl)
					{
						std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
						nzbInfo->SetKind(NzbInfo::nkUrl);
						nzbInfo->SetUrl(body);
						nzbInfo->SetFilename(s1);
						nzbInfo->SetCategory(s2);
						nzbInfo->SetAutoCategory(autoCategory);
						nzbInfo->SetPriority(a2);
						nzbInfo->SetAddUrlPaused(addPaused);
						nzbInfo->SetDupeKey(s3);
						nzbInfo->SetDupeScore(a3);
						nzbInfo->SetDupeMode((EDupeMode)a4);
						g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), addTop);
						return 1;
					}
					return g_Scanner->AddExternalFile(
						s1, s2, autoCategory, a2, s3, a3, (EDupeMode)a4,
						nullptr, addTop, addPaused, nullptr, nullptr, body, bodyLen, nullptr) != Scanner::asFailed ? 1 : 0;
				}
				case 2:
					g_WorkState->SetResumeTime(0);
					if (a2 == 1) g_WorkState->SetPauseDownload(a1 != 0);
					else if (a2 == 2) g_WorkState->SetPausePostProcess(a1 != 0);
					else if (a2 == 3) g_WorkState->SetPauseScan(a1 != 0);
					return 1;
				case 4:
					g_WorkState->SetSpeedLimit(a1);
					return 1;
				case 5:
					g_Log->LogDebugInfo();
					return 1;
				case 6:
				{
					IdList cIdList(ids, ids + idsLen);
					NameList cNameList;
					cNameList.reserve(namesLen);
					for (size_t i = 0; i < namesLen; i++)
					{
						cNameList.emplace_back(names[i]);
					}
					return DownloadQueue::Guard()->EditList(
						idsLen > 0 ? &cIdList : nullptr,
						namesLen > 0 ? &cNameList : nullptr,
						(DownloadQueue::EMatchMode)a2, (DownloadQueue::EEditAction)a1, s1) ? 1 : 0;
				}
				case 8:
					ExitProc();
					return 1;
				case 9:
					Reload();
					return 1;
				case 13:
					g_Scanner->ScanNzbDir(a1 != 0);
					return 1;
			}
			return 0;
		};
		env.collect_list_cb = [](void*, int wantFiles, int wantState, void* bCtx,
			NzbgetRsBinRpcListPushNzbFn pushNzb, NzbgetRsBinRpcListPushParamFn pushParam,
			NzbgetRsBinRpcListPushFileFn pushFile, NzbgetRsBinRpcListSetStateFn setState) {
			if (wantFiles)
			{
				GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
				uint32 ownerIndex = 0;
				for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
				{
					ownerIndex++;
					pushNzb(bCtx, (uint32)nzbInfo->GetId(), (uint32)nzbInfo->GetKind(),
						nzbInfo->GetSize(), nzbInfo->GetRemainingSize(), nzbInfo->GetPausedSize(),
						(uint32)nzbInfo->GetPausedFileCount(), (uint32)nzbInfo->GetRemainingParCount(),
						(uint32)nzbInfo->GetPriority(), nzbInfo->GetFilename(), nzbInfo->GetName(),
						nzbInfo->GetDestDir(), nzbInfo->GetCategory(), nzbInfo->GetQueuedFilename());
					for (NzbParameter& p : nzbInfo->GetParameters())
					{
						pushParam(bCtx, ownerIndex, p.GetName(), p.GetValue());
					}
					for (FileInfo* fileInfo : nzbInfo->GetFileList())
					{
						uint32 nzbIndex = ownerIndex;
						if (fileInfo->GetNzbInfo() != nzbInfo)
						{
							nzbIndex = 0;
							for (uint32 i = 0; i < downloadQueue->GetQueue()->size(); i++)
							{
								nzbIndex++;
								if (downloadQueue->GetQueue()->at(i).get() == fileInfo->GetNzbInfo())
								{
									break;
								}
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
			if (wantState)
			{
				int postJobCount = 0;
				int64 remainingSize = 0;
				{
					GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
					for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
					{
						postJobCount += nzbInfo->GetPostInfo() ? 1 : 0;
					}
					downloadQueue->CalcRemainingSize(&remainingSize, nullptr);
				}
				uint32 downloadRate = Util::SafeIntCast<int64, uint32>(g_StatMeter->CalcCurrentDownloadSpeed());
				int upTimeSec, dnTimeSec;
				int64 allBytes;
				bool standBy;
				g_StatMeter->CalcTotalStat(&upTimeSec, &dnTimeSec, &allBytes, &standBy);
				setState(bCtx, remainingSize, downloadRate, g_WorkState->GetSpeedLimit(),
					g_WorkState->GetPauseDownload() ? 1 : 0, g_WorkState->GetPausePostProcess() ? 1 : 0,
					g_WorkState->GetPauseScan() ? 1 : 0, Thread::GetThreadCount() - 1, postJobCount,
					upTimeSec, dnTimeSec, standBy ? 1 : 0, allBytes);
			}
		};
		env.collect_log_cb = [](void*, int, uint32_t, void* encCtx,
			NzbgetRsBinRpcLogBeginFn beginLog, NzbgetRsBinRpcLogPushFn pushLog) {
			nzbget_rs_log_collect(
				2, g_Log ? g_Log->RsHandle() : nullptr, nullptr, 0, 0, 0, nullptr, encCtx,
				reinterpret_cast<uintptr_t>(beginLog), reinterpret_cast<uintptr_t>(pushLog));
		};
		env.collect_post_cb = [](void*, void* encCtx, NzbgetRsBinRpcPostPushFn pushPost) {
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			time_t curTime = Util::CurrentTime();
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
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
		env.collect_history_cb = [](void*, int showHidden, void* encCtx, NzbgetRsBinRpcHistPushFn pushHist) {
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			for (HistoryInfo* h : downloadQueue->GetHistory())
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
				else if (h->GetKind() == HistoryInfo::hkDup)
				{
					size = h->GetDupInfo()->GetSize();
				}
				else if (h->GetKind() == HistoryInfo::hkUrl)
				{
					urlStatus = h->GetNzbInfo()->GetUrlStatus();
				}
				pushHist(encCtx, (uint32)h->GetId(), (uint32)h->GetKind(), (uint32)(int)h->GetTime(),
					size, fileCount, parStatus, scriptStatus, urlStatus, h->GetName());
			}
		};
		nzbget_rs_binrpc_serve(&env);
	}
};

class RemoteServer : public Thread, public Observer
{
public:
	explicit RemoteServer(bool tls) : m_tls(tls)
	{
#ifdef DISABLE_TLS
		int tlsDis = 1;
#else
		int tlsDis = 0;
#endif
#ifdef DEBUG
		int dbg = 1;
#else
		int dbg = 0;
#endif
		m_rs = nzbget_rs_remote_srv_new(tls ? 1 : 0, tlsDis, dbg, &Connection::HostCb, this, &RemoteServer::HostCb);
	}

	~RemoteServer()
	{
		nzbget_rs_remote_srv_op(m_rs, 3, 0);
#ifndef DISABLE_TLS
		OpenSSL::StopSSLThread();
#endif
	}

	void Run() override { nzbget_rs_remote_srv_op(m_rs, 0, 0); }

	void Stop() override
	{
		Thread::Stop();
		nzbget_rs_remote_srv_op(m_rs, 1, 0);
	}

	void Update(Subject* caller, void*) override
	{
		nzbget_rs_remote_srv_op(m_rs, 2, reinterpret_cast<uintptr_t>(static_cast<RequestProcessor*>(caller)));
	}

private:
	bool m_tls;
	NzbgetRsRemoteServer* m_rs = nullptr;

	static uintptr_t HostCb(void* ctx, int op, int a1, uintptr_t p1, const char* s1, int* outInts, const char** outStrs)
	{
		auto* self = static_cast<RemoteServer*>(ctx);
		if (op == 0)
		{
			outInts[0] = self->m_tls ? g_Options->GetSecurePort() : g_Options->GetControlPort();
			outInts[1] = g_Options->GetRemoteTimeout();
			outStrs[0] = g_Options->GetControlIp();
			outStrs[1] = g_Options->GetSecureCert();
			outStrs[2] = g_Options->GetSecureKey();
			return 0;
		}
		if (op == 1) return self->IsStopped() ? 1 : 0;
		if (op == 2)
		{
			auto* commandThread = new RequestProcessor();
			commandThread->SetAutoDestroy(true);
			commandThread->SetConnection(std::make_unique<Connection>(reinterpret_cast<NzbgetRsConn*>(p1)));
#ifndef DISABLE_TLS
			commandThread->SetTls(a1 != 0);
#endif
			commandThread->Attach(self);
			try
			{
				commandThread->Start();
				return reinterpret_cast<uintptr_t>(commandThread);
			}
			catch (const std::system_error& e)
			{
				commandThread->Detach(self);
				delete commandThread;
				warn("Could not start a thread for a web interface/API connection: %s", e.what());
				return 0;
			}
		}
		if (op == 3)
		{
			reinterpret_cast<RequestProcessor*>(p1)->Stop();
			return 0;
		}
		if (op == 4)
		{
			if (a1 == 0) debug("%s", s1);
			else if (a1 == 1) warn("%s", s1);
			else error("%s", s1);
		}
		return 0;
	}
};

#endif
