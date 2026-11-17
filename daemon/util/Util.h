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


#include "nzbget_rs.h"

/* An ordered list owning T objects, kept in Rust (rust/src/qlist.rs). The
   caller holds the lock that guards the list's owner, as with the queue. */
template <typename T>
class RsList
{
public:
	// the object's storage is the Rust list: an RsList<T>* is an RsPtrList*
	static void* operator new(size_t)
	{
		return nzbget_rs_ptrlist_new([](uintptr_t p) { delete reinterpret_cast<T*>(p); });
	}
	static void operator delete(void* p) { nzbget_rs_ptrlist_free(static_cast<RsPtrList*>(p)); }
	RsList() = default;
	RsList(const RsList&) = delete;
	RsList& operator=(const RsList&) = delete;
	~RsList() = default;
	RsPtrList* Rs() const { return reinterpret_cast<RsPtrList*>(const_cast<RsList*>(this)); }

	size_t size() const { return (size_t)Op(0); }
	bool empty() const { return size() == 0; }
	T* at(size_t index) const { return reinterpret_cast<T*>((uintptr_t)Op(1, (int64_t)index)); }
	// -1 when not in the list
	int64_t IndexOf(T* p) const { return Op(2, 0, p); }
	void Add(std::unique_ptr<T> uptr, bool addTop = false) { Op(addTop ? 4 : 3, 0, uptr.release()); }
	// before the item at index (at the back when out of range)
	void Insert(int64_t index, std::unique_ptr<T> uptr) { Op(5, index, uptr.release()); }
	std::unique_ptr<T> Remove(T* p) { return std::unique_ptr<T>(reinterpret_cast<T*>((uintptr_t)Op(6, 0, p))); }
	void EraseAt(size_t index) { Op(7, (int64_t)index); }
	void ReplaceAt(size_t index, std::unique_ptr<T> uptr) { Op(8, (int64_t)index, uptr.release()); }
	void clear() { Op(9); }
	// order: every current item once, in the new order (ignored otherwise)
	void Reorder(const uintptr_t* order, size_t count)
	{
		nzbget_rs_ptrlist_op(Rs(), 10, (int64_t)count, 0, order, nullptr);
	}
	// appends and takes every item of other
	void TakeAll(RsList& other) { nzbget_rs_ptrlist_op(Rs(), 11, 0, 0, nullptr, other.Rs()); }
	T* Find(int id) const
	{
		for (size_t i = 0, n = size(); i < n; i++)
		{
			T* item = at(i);
			if (item && item->GetId() == id) return item;
		}
		return nullptr;
	}

	struct iterator
	{
		const RsList* list;
		size_t index;
		T* operator*() const { return list->at(index); }
		iterator& operator++() { ++index; return *this; }
		bool operator!=(const iterator& other) const { return index != other.index; }
	};
	iterator begin() const { return iterator{this, 0}; }
	iterator end() const { return iterator{this, size()}; }

private:
	int64_t Op(int op, int64_t a = 0, T* p = nullptr) const
	{
		return nzbget_rs_ptrlist_op(Rs(), op, a, reinterpret_cast<uintptr_t>(p), nullptr, nullptr);
	}
};
template <typename T> typename RsList<T>::iterator begin(RsList<T>* c) { return c->begin(); }
template <typename T> typename RsList<T>::iterator end(RsList<T>* c) { return c->end(); }

#endif

#ifndef THREAD_H
#define THREAD_H

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


// The daemon's options live in Rust (rust/src/options.rs). These read the
// active set: the daemon's, or in tests the newest one created.
namespace ActiveOptions
{
	enum EHealthCheck { hcPause, hcDelete, hcPark, hcNone, hcDupe };

	inline const NzbgetRsOptions* Handle() { return nzbget_rs_options_active(); }

	inline const NzbgetRsParsedOptions& Scalars()
	{
		static const NzbgetRsParsedOptions none{};
		const NzbgetRsParsedOptions* scalars = nzbget_rs_options_scalars(Handle());
		return scalars ? *scalars : none;
	}

	inline bool QueryCategory(const char* name, bool searchAliases, int* outUnpack, const char** outStrs)
	{
		return nzbget_rs_options_query(
			Handle(), 0, name, searchAliases ? 1 : 0, outUnpack, outStrs, 0, nullptr) != 0;
	}

	inline const char* GetDestDir() { return nzbget_rs_options_str(Handle(), 3); }
	inline const char* GetInterDir() { return nzbget_rs_options_str(Handle(), 4); }
	inline bool GetRawArticle() { return Scalars().raw_article != 0; }
	inline bool GetSkipWrite() { return Scalars().skip_write != 0; }
	inline bool GetAppendCategoryDir() { return Scalars().append_category_dir != 0; }
	inline EHealthCheck GetHealthCheck() { return (EHealthCheck)Scalars().health_check; }
	inline bool GetDirectWrite() { return Scalars().direct_write != 0; }
	inline int GetWriteBuffer() { return Scalars().write_buffer; }
}

#endif /* OPTIONS_H */

// Messages go to the daemon's active log (rust/src/log.rs).
inline void LogEmitV(Message::EKind kind, const char* msg, va_list ap)
{
	char text[1024];
	vsnprintf(text, 1024, msg, ap);
	text[1024 - 1] = '\0';
	nzbget_rs_log_active_emit(static_cast<int>(kind), text);
}

#ifdef DEBUG
inline void debug(const char* filename, const char* funcname, int lineNr, const char* msg, ...)
{
	char tmp1[1024];
	va_list ap;
	va_start(ap, msg);
	vsnprintf(tmp1, 1024, msg, ap);
	tmp1[1024 - 1] = '\0';
	va_end(ap);
	BString<1024> tmp2;
	if (funcname) tmp2.Format("%s (%s:%i:%s)", tmp1, FileSystem::BaseFileName(filename), lineNr, funcname);
	else tmp2.Format("%s (%s:%i)", tmp1, FileSystem::BaseFileName(filename), lineNr);
	nzbget_rs_log_active_emit(static_cast<int>(Message::mkDebug), *tmp2);
}
#endif

inline void error(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	LogEmitV(Message::mkError, msg, ap);
	va_end(ap);
}

inline void warn(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	LogEmitV(Message::mkWarning, msg, ap);
	va_end(ap);
}

inline void info(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	LogEmitV(Message::mkInfo, msg, ap);
	va_end(ap);
}

inline void detail(const char* msg, ...)
{
	va_list ap;
	va_start(ap, msg);
	LogEmitV(Message::mkDetail, msg, ap);
	va_end(ap);
}

#ifdef DEBUG
#define debug(...) debug(__FILE__, FUNCTION_MACRO_NAME, __LINE__, __VA_ARGS__)
#else
#define debug(...) do { } while(0)
#endif

#endif /* LOG_H */


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

inline void RegisterRsQueueHooks();

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
		RegisterRsQueueHooks();
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

	// the object's storage is the Rust file (rust/src/qfile.rs): a FileInfo* is an RsFile*
	static void* operator new(size_t) { return nzbget_rs_file_new(-1); }
	static void operator delete(void* p) { nzbget_rs_file_free(static_cast<RsFile*>(p)); }
	FileInfo(int id = 0) { S(0, id != 0 ? id : nzbget_rs_file_ids(4, 0)); }
	~FileInfo() = default;
	FileInfo(const FileInfo&) = delete;
	FileInfo& operator=(const FileInfo&) = delete;
	RsFile* Rs() { return reinterpret_cast<RsFile*>(this); }
	void SetId(int id) { S(0, id); }
	static void RaiseIdFloor(int id) { nzbget_rs_file_ids(3, id); }
	NzbInfo* GetNzbInfo() { return reinterpret_cast<NzbInfo*>((uintptr_t)I(35)); }
	void SetNzbInfo(NzbInfo* nzbInfo) { S(35, (int64)reinterpret_cast<uintptr_t>(nzbInfo)); }
	RsArticleList* GetArticleList() { return nzbget_rs_file_articles(Rs()); }
	size_t GroupCount() { return (size_t)nzbget_rs_file_op(Rs(), 0, 0, nullptr, nullptr, nullptr); }
	const char* GroupAt(size_t i) { const char* g = nullptr; nzbget_rs_file_op(Rs(), 1, (int)i, nullptr, nullptr, &g); return g; }
	void AddGroup(const char* group) { nzbget_rs_file_op(Rs(), 2, 0, group, nullptr, nullptr); }
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
	FileOutputGuard GuardOutputFile() { return FileOutputGuard(Rs()); }
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
	size_t StatCount() { return (size_t)nzbget_rs_file_op(Rs(), 10, 0, nullptr, nullptr, nullptr); }
	void StatAt(size_t i, int* out3) { nzbget_rs_file_op(Rs(), 11, (int)i, nullptr, out3, nullptr); }
	void StatOp(int serverId, int success, int failed, ServerStatList::EStatOperation op)
	{
		int v[3] = { serverId, success, failed };
		nzbget_rs_file_op(Rs(), 12, (int)op, nullptr, v, nullptr);
	}
	void ClearStats() { nzbget_rs_file_op(Rs(), 13, 0, nullptr, nullptr, nullptr); }

private:
	int64 I(int id) { return nzbget_rs_file_get(Rs(), id); }
	void S(int id, int64 v) { nzbget_rs_file_set(Rs(), id, v); }
	const char* T(int id) { return nzbget_rs_file_text(Rs(), id); }
	void ST(int id, const char* v) { nzbget_rs_file_set_text(Rs(), id, v); }

};


typedef RsList<FileInfo> FileList;

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
	// the object's storage is the Rust collection (rust/src/qnzb.rs): a NzbInfo* is an
	// NzbgetRsNzbScalars*; the host handles below live in its slots. Its host ops
	// (30-34) are answered in Rust; what is left here serves the remaining C++.
	static void* operator new(size_t) { return nzbget_rs_nzb_scalars_new(); }
	static void operator delete(void* p) { nzbget_rs_nzb_scalars_free(static_cast<NzbgetRsNzbScalars*>(p)); }
	NzbInfo()
	{
		// the next id and an empty file list (rust/src/qnzb.rs)
		nzbget_rs_nzb_init(Scalars());
	}
	~NzbInfo()
	{
		// the post job is a Rust allocation (rust/src/qnzb.rs Post)
		nzbget_rs_post_free(reinterpret_cast<RsPost*>(nzbget_rs_nzb_slot(Scalars(), 1, 1, 0)));
		delete reinterpret_cast<FileList*>(nzbget_rs_nzb_slot(Scalars(), 0, 1, 0));
	}
	NzbInfo(const NzbInfo&) = delete;
	NzbInfo& operator=(const NzbInfo&) = delete;
	// numeric and status fields live in Rust (rust/src/qnzb.rs)
	NzbgetRsNzbScalars* Scalars() const { return reinterpret_cast<NzbgetRsNzbScalars*>(const_cast<NzbInfo*>(this)); }
	enum EKind
	{
		nkNzb,
		nkUrl
	};

	int GetId() { return Scalars()->id; }
	EKind GetKind() { return (EKind)Scalars()->kind; }
	// text fields as in rust/src/qnzb.rs: 2 name, 3 destination dir
	const char* GetName() { return nzbget_rs_nzb_text(Scalars(), 2); }
	const char* GetDestDir() { return nzbget_rs_nzb_text(Scalars(), 3); }
	int64 GetPausedSize() { return Scalars()->pausedSize; }
	void SetPausedSize(int64 pausedSize) { Scalars()->pausedSize = pausedSize; }
	int GetPausedFileCount() { return Scalars()->pausedFileCount; }
	void SetPausedFileCount(int pausedFileCount) { Scalars()->pausedFileCount = pausedFileCount; }
	int GetExtraPriority() { return Scalars()->extraPriority; }
	void SetExtraPriority(int extraPriority) { Scalars()->extraPriority = extraPriority; }
	bool GetSkipDiskWrite() { return Scalars()->skipDiskWrite; }
	// parameters, script statuses, blocked donors and server stats live in Rust;
	// op codes as in rust/src/qnzb.rs nzbget_rs_nzb_coll
	int64 NC(int op, int a2 = 0, NzbInfo* other = nullptr, const char* s1 = nullptr, const char* s2 = nullptr,
		int* ints = nullptr, const char** strs = nullptr)
	{
		return nzbget_rs_nzb_coll(Scalars(), op, a2, other ? other->Scalars() : nullptr, s1, s2, ints, strs);
	}
};

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

// these objects are Rust allocations (see their operator new): no data members of
// their own, and no vtable that would be written into the Rust storage
static_assert(std::is_empty<FileInfo>::value && !std::is_polymorphic<FileInfo>::value);
static_assert(std::is_empty<NzbInfo>::value && !std::is_polymorphic<NzbInfo>::value);
static_assert(std::is_empty<FileList>::value && !std::is_polymorphic<FileList>::value);

typedef RsList<NzbInfo> NzbList;

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

	// the object's storage is the Rust record (rust/src/qdup.rs): a DupInfo* is an RsDup*
	static void* operator new(size_t) { return nzbget_rs_dup_new(); }
	static void operator delete(void* p) { nzbget_rs_dup_free(static_cast<RsDup*>(p)); }
	// user-provided, so value-initialisation never zeroes the Rust storage
	DupInfo() {}
	~DupInfo() = default;
	DupInfo(const DupInfo&) = delete;
	DupInfo& operator=(const DupInfo&) = delete;
	RsDup* Rs() { return reinterpret_cast<RsDup*>(this); }

	int GetId() { return (int)Get(5); }
	void SetId(int id)
	{
		nzbget_rs_dup_set(Rs(), 5, id, nullptr);
		nzbget_rs_nzb_ids(3, id);
	}
	const char* GetName() { return Text(0); }
	void SetName(const char* name) { nzbget_rs_dup_set(Rs(), 7, 0, name); }
	const char* GetDupeKey() { return Text(1); }
	void SetDupeKey(const char* dupeKey) { nzbget_rs_dup_set(Rs(), 8, 0, dupeKey); }
	int GetDupeScore() { return (int)Get(1); }
	void SetDupeScore(int dupeScore) { nzbget_rs_dup_set(Rs(), 1, dupeScore, nullptr); }
	EDupeMode GetDupeMode() { return (EDupeMode)Get(0); }
	void SetDupeMode(EDupeMode dupeMode) { nzbget_rs_dup_set(Rs(), 0, dupeMode, nullptr); }
	int64 GetSize() { return Get(6); }
	void SetSize(int64 size) { nzbget_rs_dup_set(Rs(), 6, size, nullptr); }
	uint32 GetFullContentHash() { return (uint32)Get(3); }
	void SetFullContentHash(uint32 fullContentHash) { nzbget_rs_dup_set(Rs(), 3, fullContentHash, nullptr); }
	uint32 GetFilteredContentHash() { return (uint32)Get(4); }
	void SetFilteredContentHash(uint32 filteredContentHash) { nzbget_rs_dup_set(Rs(), 4, filteredContentHash, nullptr); }
	EStatus GetStatus() { return (EStatus)Get(2); }
	void SetStatus(EStatus Status) { nzbget_rs_dup_set(Rs(), 2, Status, nullptr); }
	// every field at once (see nzbget_rs_dup_get)
	void Read(int* ints, int64_t* i64s, const char** strs) { nzbget_rs_dup_get(Rs(), ints, i64s, strs); }

private:
	int64 Get(int field)
	{
		int ints[6]; int64_t size = 0; const char* strs[2];
		nzbget_rs_dup_get(Rs(), ints, &size, strs);
		return field == 6 ? size : ints[field];
	}
	const char* Text(int which)
	{
		int ints[6]; const char* strs[2];
		nzbget_rs_dup_get(Rs(), ints, nullptr, strs);
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

	// the object's storage is the Rust entry (rust/src/qhist.rs): a HistoryInfo* is an
	// RsHist* holding the kind, the time and the inner NzbInfo* or DupInfo*
	static void* operator new(size_t) { return nzbget_rs_hist_new(); }
	static void operator delete(void* p) { nzbget_rs_hist_free(static_cast<RsHist*>(p)); }
	HistoryInfo(std::unique_ptr<NzbInfo> nzbInfo)
	{
		NzbInfo* nzb = nzbInfo.release();
		SetInfo(nzb);
		Set(0, nzb->GetKind() == NzbInfo::nkNzb ? hkNzb : hkUrl);
	}
	HistoryInfo(std::unique_ptr<DupInfo> dupInfo)
	{
		SetInfo(dupInfo.release());
		Set(0, hkDup);
	}
	// deletes the inner record (unless DiscardNzbInfo took it) before operator delete
	// frees the entry
	~HistoryInfo()
	{
		EKind kind = GetKind();
		if ((kind == hkNzb || kind == hkUrl) && Info())
		{
			delete GetNzbInfo();
		}
		else if (kind == hkDup && Info())
		{
			delete GetDupInfo();
		}
	}
	HistoryInfo(const HistoryInfo&) = delete;
	HistoryInfo& operator=(const HistoryInfo&) = delete;
	RsHist* Rs() { return reinterpret_cast<RsHist*>(this); }
	EKind GetKind() { return (EKind)Get(0); }
	int GetId()
	{
		if (GetKind() == hkNzb || GetKind() == hkUrl)
		{
			return GetNzbInfo()->GetId();
		}
		else // if (GetKind() == hkDup)
		{
			return GetDupInfo()->GetId();
		}
	}
	NzbInfo* GetNzbInfo() { return reinterpret_cast<NzbInfo*>(Info()); }
	DupInfo* GetDupInfo() { return reinterpret_cast<DupInfo*>(Info()); }
	void DiscardNzbInfo() { SetInfo(nullptr); }
	time_t GetTime() { return (time_t)Get(1); }
	void SetTime(time_t time) { Set(1, (int64)time); }
	const char* GetName()
	{
		if (GetKind() == hkNzb || GetKind() == hkUrl)
		{
			return GetNzbInfo()->GetName();
		}
		else if (GetKind() == hkDup)
		{
			return GetDupInfo()->GetName();
		}
		else
		{
			return "<unknown>";
		}
	}

private:
	int64 Get(int field) { return nzbget_rs_hist_get(Rs(), field); }
	void Set(int field, int64 v) { nzbget_rs_hist_set(Rs(), field, v); }
	uintptr_t Info() { return (uintptr_t)Get(2); }
	void SetInfo(void* info) { Set(2, (int64)reinterpret_cast<uintptr_t>(info)); }
};

// Rust allocations as well (see their operator new)
static_assert(std::is_empty<DupInfo>::value && !std::is_polymorphic<DupInfo>::value);
static_assert(std::is_empty<HistoryInfo>::value && !std::is_polymorphic<HistoryInfo>::value);

typedef RsList<HistoryInfo> HistoryList;

// holds the current queue's lock (rust/src/qqueue.rs) while it lives
class GuardedDownloadQueue
{
public:
	explicit GuardedDownloadQueue(DownloadQueue* queue) : m_ptr(queue)
	{
		if (m_ptr) nzbget_rs_queue_op(reinterpret_cast<uintptr_t>(m_ptr), 3);
	}
	GuardedDownloadQueue(GuardedDownloadQueue&& other) : m_ptr(other.m_ptr), m_locked(other.m_locked) { other.m_locked = false; }
	GuardedDownloadQueue(const GuardedDownloadQueue&) = delete;
	~GuardedDownloadQueue() { if (m_ptr && m_locked) nzbget_rs_queue_op(reinterpret_cast<uintptr_t>(m_ptr), 4); }
	DownloadQueue* operator->() { return m_ptr; }
	operator DownloadQueue*() { return m_ptr; }

private:
	DownloadQueue* m_ptr;
	bool m_locked = true;
};

// lets Rust delete a NzbInfo (its destructor still frees host objects)
inline void RegisterRsQueueHooks()
{
	nzbget_rs_queue_hooks([](uintptr_t p) { delete reinterpret_cast<NzbInfo*>(p); });
}

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

	// the daemon's queue (rust/src/qqueue.rs), locked while the guard lives
	static GuardedDownloadQueue Guard();
	NzbList* GetQueue() { return reinterpret_cast<NzbList*>(nzbget_rs_queue_op(Key(), 1)); }
	HistoryList* GetHistory() { return reinterpret_cast<HistoryList*>(nzbget_rs_queue_op(Key(), 2)); }

	// the object's storage is the Rust queue that host op 25 creates: a
	// DownloadQueue* is an RsQueue*, never made here
	DownloadQueue() = delete;
	DownloadQueue(const DownloadQueue&) = delete;
	DownloadQueue& operator=(const DownloadQueue&) = delete;

private:
	uintptr_t Key() const { return reinterpret_cast<uintptr_t>(this); }
};

static_assert(std::is_empty<DownloadQueue>::value && !std::is_polymorphic<DownloadQueue>::value);

inline GuardedDownloadQueue DownloadQueue::Guard()
{
	return GuardedDownloadQueue(reinterpret_cast<DownloadQueue*>(nzbget_rs_queue_op(0, 9)));
}


class DiskState {};
extern DiskState* g_DiskState;



#ifndef SERVERPOOL_H
#define SERVERPOOL_H


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

#endif

class Scanner {};
extern Scanner* g_Scanner;
class UrlCoordinator;

#endif /* SCANNER_H */

#ifndef DUPESEARCH_H
#define DUPESEARCH_H
#include <ctime>
#endif



inline uintptr_t NzbgetCQueueHost(void*, int op, int a1, int a2, uintptr_t p1, uintptr_t p2,
	const char* s1, const char* s2, int* ints, int64_t* i64s, const char** strs)
{
	switch (op)
	{
	// 30 (collection snapshot), 31 (collection lists and texts) and 32
	// (collection setters) are answered in Rust (rust/src/qnzb.rs
	// host_coll_snapshot, host_coll_op and host_coll_set)
	case 43: {
		// article ops, and the live-par namespace walk (15), are answered in Rust
		// (rust/src/queuesubsys_host.rs native_op)
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
				if (!ActiveOptions::GetDirectWrite() && !f->GetForceDirectWrite()) f->SetOutputInitialized(true);
			}
			if (ActiveOptions::GetDirectWrite() || f->GetForceDirectWrite())
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
				ints[0] = ActiveOptions::GetDirectWrite() ? 1 : 0; ints[1] = f->GetForceDirectWrite() ? 1 : 0;
				ints[2] = f->GetOutputInitialized() ? 1 : 0; ints[3] = f->GetCachedArticles();
				ints[4] = f->GetSuccessArticles(); ints[5] = f->GetFailedArticles();
				ints[6] = f->GetMissedArticles(); ints[7] = f->GetTotalArticles();
				ints[8] = f->IsHardLinked() ? 1 : 0; ints[9] = Util::EmptyStr(ActiveOptions::GetInterDir()) ? 1 : 0;
				ints[10] = ActiveOptions::GetRawArticle() ? 1 : 0; ints[11] = (nzb->GetSkipDiskWrite() || ActiveOptions::GetSkipWrite()) ? 1 : 0;
				ints[12] = ActiveOptions::GetWriteBuffer(); ints[15] = (int)ArtOp(f->GetArticleList(), 0, 100);
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
				ints[0] = f->IsHardLinked() ? 1 : 0; ints[1] = Util::EmptyStr(ActiveOptions::GetInterDir()) ? 1 : 0;
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
	// 60 (post-processing state) and 62 (post and direct-unpack job handles) are
	// answered in Rust (rust/src/qnzb.rs host_post_op / host_post_job_op)
	default: return 0;
	}
}

#endif

class FeedCoordinator;
namespace SystemHealth { class Service; }
class Maintenance;
#include <system_error>

#endif
