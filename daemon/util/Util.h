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
	// NzbgetRsNzbScalars*; the host handles below live in its slots
	static void* operator new(size_t) { return nzbget_rs_nzb_scalars_new(); }
	static void operator delete(void* p) { nzbget_rs_nzb_scalars_free(static_cast<NzbgetRsNzbScalars*>(p)); }
	NzbInfo()
	{
		// the next id and an empty file list (rust/src/qnzb.rs)
		nzbget_rs_nzb_init(Scalars());
	}
	~NzbInfo()
	{
		delete reinterpret_cast<PostInfo*>(nzbget_rs_nzb_slot(Scalars(), 1, 1, 0));
		delete reinterpret_cast<FileList*>(nzbget_rs_nzb_slot(Scalars(), 0, 1, 0));
	}
	NzbInfo(const NzbInfo&) = delete;
	NzbInfo& operator=(const NzbInfo&) = delete;
	// numeric and status fields live in Rust (rust/src/qnzb.rs)
	NzbgetRsNzbScalars* Scalars() const { return reinterpret_cast<NzbgetRsNzbScalars*>(const_cast<NzbInfo*>(this)); }
	// strings live in Rust too: 0 url, 1 filename, 2 name, 3 dest dir, 4 final dir,
	// 5 category, 6 queued filename, 7 dupe key, 8 hard-link path, 9 meta name
	const char* NT(int id) { return nzbget_rs_nzb_text(Scalars(), id); }
	void NST(int id, const char* v) { nzbget_rs_nzb_set_text(Scalars(), id, v, -1); }
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

	int GetId() { return Scalars()->id; }
	void SetId(int id)
	{
		Scalars()->id = id;
		nzbget_rs_nzb_ids(3, id);
	}
	EKind GetKind() { return (EKind)Scalars()->kind; }
	void SetKind(EKind kind) { Scalars()->kind = kind; }
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
	int GetFileCount() { return Scalars()->fileCount; }
	void SetFileCount(int fileCount) { Scalars()->fileCount = fileCount; }
	int GetParkedFileCount() { return Scalars()->parkedFileCount; }
	void SetParkedFileCount(int parkedFileCount) { Scalars()->parkedFileCount = parkedFileCount; }
	int64 GetSize() { return Scalars()->size; }
	void SetSize(int64 size) { Scalars()->size = size; }
	int64 GetRemainingSize() { return Scalars()->remainingSize; }
	void SetRemainingSize(int64 remainingSize) { Scalars()->remainingSize = remainingSize; }
	int64 GetPausedSize() { return Scalars()->pausedSize; }
	void SetPausedSize(int64 pausedSize) { Scalars()->pausedSize = pausedSize; }
	int GetPausedFileCount() { return Scalars()->pausedFileCount; }
	void SetPausedFileCount(int pausedFileCount) { Scalars()->pausedFileCount = pausedFileCount; }
	int GetRemainingParCount() { return Scalars()->remainingParCount; }
	void SetRemainingParCount(int remainingParCount) { Scalars()->remainingParCount = remainingParCount; }
	int GetActiveDownloads() { return Scalars()->activeDownloads; }
	void SetActiveDownloads(int activeDownloads)
	{
		int64_t startTime = static_cast<int64_t>(Scalars()->downloadStartTime);
		int changed = 0;
		nzbget_rs_nzb_calc_active_downloads_sec(
			Scalars()->activeDownloads, activeDownloads, Scalars()->kind == NzbInfo::nkNzb ? 1 : 0,
			static_cast<int64_t>(Util::CurrentTime()), &Scalars()->downloadSec, &startTime,
			&Scalars()->downloadStartSec, &changed);
		Scalars()->downloadStartTime = static_cast<time_t>(startTime);
		if (changed) Scalars()->changed = true;
		Scalars()->activeDownloads = activeDownloads;
	}
	int64 GetSuccessSize() { return Scalars()->successSize; }
	void SetSuccessSize(int64 successSize) { Scalars()->successSize = successSize; }
	int64 GetFailedSize() { return Scalars()->failedSize; }
	void SetFailedSize(int64 failedSize) { Scalars()->failedSize = failedSize; }
	int64 GetCurrentSuccessSize() { return Scalars()->currentSuccessSize; }
	void SetCurrentSuccessSize(int64 currentSuccessSize) { Scalars()->currentSuccessSize = currentSuccessSize; }
	int64 GetCurrentFailedSize() { return Scalars()->currentFailedSize; }
	void SetCurrentFailedSize(int64 currentFailedSize) { Scalars()->currentFailedSize = currentFailedSize; }
	int64 GetParSize() { return Scalars()->parSize; }
	void SetParSize(int64 parSize) { Scalars()->parSize = parSize; }
	int64 GetParSuccessSize() { return Scalars()->parSuccessSize; }
	void SetParSuccessSize(int64 parSuccessSize) { Scalars()->parSuccessSize = parSuccessSize; }
	int64 GetParFailedSize() { return Scalars()->parFailedSize; }
	void SetParFailedSize(int64 parFailedSize) { Scalars()->parFailedSize = parFailedSize; }
	int64 GetParCurrentSuccessSize() { return Scalars()->parCurrentSuccessSize; }
	void SetParCurrentSuccessSize(int64 parCurrentSuccessSize) { Scalars()->parCurrentSuccessSize = parCurrentSuccessSize; }
	int64 GetParCurrentFailedSize() { return Scalars()->parCurrentFailedSize; }
	void SetParCurrentFailedSize(int64 parCurrentFailedSize) { Scalars()->parCurrentFailedSize = parCurrentFailedSize; }
	int GetTotalArticles() { return Scalars()->totalArticles; }
	void SetTotalArticles(int totalArticles) { Scalars()->totalArticles = totalArticles; }
	int GetSuccessArticles() { return Scalars()->successArticles; }
	void SetSuccessArticles(int successArticles) { Scalars()->successArticles = successArticles; }
	int GetFailedArticles() { return Scalars()->failedArticles; }
	void SetFailedArticles(int failedArticles) { Scalars()->failedArticles = failedArticles; }
	int GetCurrentSuccessArticles() { return Scalars()->currentSuccessArticles; }
	void SetCurrentSuccessArticles(int currentSuccessArticles) { Scalars()->currentSuccessArticles = currentSuccessArticles; }
	int GetCurrentFailedArticles() { return Scalars()->currentFailedArticles; }
	void SetCurrentFailedArticles(int currentFailedArticles) { Scalars()->currentFailedArticles = currentFailedArticles; }
	int GetDupeRecoveredArticles() { return Scalars()->dupeRecoveredArticles; }
	// duplicates whose articles failed the par2 checksums: no more articles from them (B40, not persisted)
	// failed articles of its own in a row, first articles of files aside (B49, not persisted)
	int GetDupeFailedRun() { return Scalars()->dupeFailedRun; }
	void SetDupeFailedRun(int dupeFailedRun) { Scalars()->dupeFailedRun = dupeFailedRun; }
	void SetDupeRecoveredArticles(int dupeRecoveredArticles) { Scalars()->dupeRecoveredArticles = dupeRecoveredArticles; }
	int64 GetDupeRecoveredBytes() { return Scalars()->dupeRecoveredBytes; }
	void SetDupeRecoveredBytes(int64 value) { Scalars()->dupeRecoveredBytes = value; }
	int GetDupeRecoveredHoles() { return Scalars()->dupeRecoveredHoles; }
	void SetDupeRecoveredHoles(int value) { Scalars()->dupeRecoveredHoles = value; }
	int GetDupeAttemptedArticles() { return Scalars()->dupeAttemptedArticles; }
	void SetDupeAttemptedArticles(int value) { Scalars()->dupeAttemptedArticles = value; }
	int GetDupeUnsourcedArticles() { return Scalars()->dupeUnsourcedArticles; }
	void SetDupeUnsourcedArticles(int value) { Scalars()->dupeUnsourcedArticles = value; }
	int GetDupeFailoverChecked() { return Scalars()->dupeFailoverChecked; }
	void SetDupeFailoverChecked(int value) { Scalars()->dupeFailoverChecked = value; }
	bool GetDeadPickProbed() { return Scalars()->deadPickProbed; }
	void SetDeadPickProbed(bool value) { Scalars()->deadPickProbed = value; }
	enum EDupeParDeferState { dpNone, dpDeferred, dpLifted };
	EDupeParDeferState GetDupeParDeferState() { return (EDupeParDeferState)Scalars()->dupeParDeferState; }
	void SetDupeParDeferState(EDupeParDeferState state) { Scalars()->dupeParDeferState = state; }
	bool GetDupeNoParNoted() { return Scalars()->dupeNoParNoted; }
	void SetDupeNoParNoted(bool noted) { Scalars()->dupeNoParNoted = noted; }
	// completed files and stream-repair jobs (ops in rust/src/qnzb.rs nzbget_rs_nzb_files)
	int64 NF(int op, int a2 = 0, NzbInfo* p2 = nullptr, const char* s1 = nullptr, const char* s2 = nullptr,
		int* ints = nullptr, int64_t* i64s = nullptr, const char** strs = nullptr, const int64_t* raw = nullptr)
	{
		return nzbget_rs_nzb_files(Scalars(), op, a2, p2 ? p2->Scalars() : nullptr, s1, s2, ints, i64s, strs, raw);
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
	int GetPriority() { return Scalars()->priority; }
	void SetPriority(int priority) { Scalars()->priority = priority; }
	int GetExtraPriority() { return Scalars()->extraPriority; }
	void SetExtraPriority(int extraPriority) { Scalars()->extraPriority = extraPriority; }
	bool HasExtraPriority() { return Scalars()->extraPriority > 0; }
	bool GetForcePriority() { return Scalars()->priority >= FORCE_PRIORITY; }
	time_t GetMinTime() { return Scalars()->minTime; }
	void SetMinTime(time_t minTime) { Scalars()->minTime = minTime; }
	time_t GetMaxTime() { return Scalars()->maxTime; }
	void SetMaxTime(time_t maxTime) { Scalars()->maxTime = maxTime; }
	CString BuildFinalDirName()
	{
		const char* categoryDestDir = nullptr;
		if (!Util::EmptyStr(NT(5)))
		{
			const char* strs[3]{};
			if (ActiveOptions::QueryCategory(NT(5), false, nullptr, strs) && !Util::EmptyStr(strs[1]))
			{
				categoryDestDir = strs[1];
			}
		}
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			2, ActiveOptions::GetDestDir(), categoryDestDir, NT(5),
			GetName(), nullptr, ActiveOptions::GetAppendCategoryDir() ? 1 : 0, GetId());
		CString out(buf.data, static_cast<int>(buf.len));
		nzbget_rs_free(buf);
		return out;
	}
	void BuildDestDirName()
	{
		const char* categoryDestDir = nullptr;
		if (Util::EmptyStr(ActiveOptions::GetInterDir()) && !Util::EmptyStr(NT(5)))
		{
			const char* strs[3]{};
			if (ActiveOptions::QueryCategory(NT(5), false, nullptr, strs) && !Util::EmptyStr(strs[1]))
			{
				categoryDestDir = strs[1];
			}
		}
		NzbgetRsBuf buf = nzbget_rs_nzb_format_name(
			3, ActiveOptions::GetInterDir(), ActiveOptions::GetDestDir(), categoryDestDir,
			NT(5), GetName(), ActiveOptions::GetAppendCategoryDir() ? 1 : 0, GetId());
		nzbget_rs_nzb_set_text(Scalars(), 3, buf.data, (intptr_t)buf.len);
		nzbget_rs_free(buf);
	}
	bool RenameCompletedFile(const char* oldName, const char* newAbsoluteOrRelativePath)
	{
		return NF(4, 0, nullptr, oldName, newAbsoluteOrRelativePath) != 0;
	}
	void SetDirectRenameStatus(EDirectRenameStatus renameStatus) { Scalars()->directRenameStatus = renameStatus; }
	EDirectRenameStatus GetDirectRenameStatus() { return (EDirectRenameStatus)Scalars()->directRenameStatus; }
	EPostRenameStatus GetParRenameStatus() { return (EPostRenameStatus)Scalars()->parRenameStatus; }
	void SetParRenameStatus(EPostRenameStatus renameStatus) { Scalars()->parRenameStatus = renameStatus; }
	EPostRenameStatus GetRarRenameStatus() { return (EPostRenameStatus)Scalars()->rarRenameStatus; }
	void SetRarRenameStatus(EPostRenameStatus renameStatus) { Scalars()->rarRenameStatus = renameStatus; }
	EParStatus GetParStatus() { return (EParStatus)Scalars()->parStatus; }
	void SetParStatus(EParStatus parStatus) { Scalars()->parStatus = parStatus; }
	EDirectUnpackStatus GetDirectUnpackStatus() { return (EDirectUnpackStatus)Scalars()->directUnpackStatus; }
	void SetDirectUnpackStatus(EDirectUnpackStatus directUnpackStatus) { Scalars()->directUnpackStatus = directUnpackStatus; }
	EPostUnpackStatus GetUnpackStatus() { return (EPostUnpackStatus)Scalars()->unpackStatus; }
	void SetUnpackStatus(EPostUnpackStatus unpackStatus) { Scalars()->unpackStatus = unpackStatus; }
	ECleanupStatus GetCleanupStatus() { return (ECleanupStatus)Scalars()->cleanupStatus; }
	void SetCleanupStatus(ECleanupStatus cleanupStatus) { Scalars()->cleanupStatus = cleanupStatus; }
	EMoveStatus GetMoveStatus() { return (EMoveStatus)Scalars()->moveStatus; }
	void SetPostUnpackRenamingStatus(PostUnpackRenamingStatus status) { Scalars()->postUnpackRenamingStatus = (int32_t)status; }
	PostUnpackRenamingStatus GetPostUnpackRenamingStatus() { return (PostUnpackRenamingStatus)Scalars()->postUnpackRenamingStatus; }
	void SetPostDownloadRenamingStatus(PostDownloadRenamingStatus status) { Scalars()->postDownloadRenamingStatus = (int32_t)status; }
	PostDownloadRenamingStatus GetPostDownloadRenamingStatus() { return (PostDownloadRenamingStatus)Scalars()->postDownloadRenamingStatus; }
	void SetMoveStatus(EMoveStatus moveStatus) { Scalars()->moveStatus = moveStatus; }
	EDeleteStatus GetDeleteStatus() { return (EDeleteStatus)Scalars()->deleteStatus; }
	void SetDeleteStatus(EDeleteStatus deleteStatus) { Scalars()->deleteStatus = deleteStatus; }
	EMarkStatus GetMarkStatus() { return (EMarkStatus)Scalars()->markStatus; }
	void SetMarkStatus(EMarkStatus markStatus) { Scalars()->markStatus = markStatus; }
	EUrlStatus GetUrlStatus() { return (EUrlStatus)Scalars()->urlStatus; }
	int GetExtraParBlocks() { return Scalars()->extraParBlocks; }
	void SetExtraParBlocks(int extraParBlocks) { Scalars()->extraParBlocks = extraParBlocks; }
	void SetUrlStatus(EUrlStatus urlStatus) { Scalars()->urlStatus = urlStatus; }
	const char* GetQueuedFilename() { return NT(6); }
	void SetQueuedFilename(const char* queuedFilename) { NST(6, queuedFilename); }
	bool GetDeleting() { return Scalars()->deleting; }
	void SetDeleting(bool deleting) { Scalars()->deleting = deleting; }
	bool GetParking() { return Scalars()->parking; }
	void SetParking(bool parking) { Scalars()->parking = parking; }
	bool GetDeletePaused() { return Scalars()->deletePaused; }
	void SetDeletePaused(bool deletePaused) { Scalars()->deletePaused = deletePaused; }
	bool GetManyDupeFiles() { return Scalars()->manyDupeFiles; }
	void SetManyDupeFiles(bool manyDupeFiles) { Scalars()->manyDupeFiles = manyDupeFiles; }
	bool GetAvoidHistory() { return Scalars()->avoidHistory; }
	void SetAvoidHistory(bool avoidHistory) { Scalars()->avoidHistory = avoidHistory; }
	bool GetHealthPaused() { return Scalars()->healthPaused; }
	void SetHealthPaused(bool healthPaused) { Scalars()->healthPaused = healthPaused; }
	bool GetCleanupDisk() { return Scalars()->cleanupDisk; }
	void SetCleanupDisk(bool cleanupDisk) { Scalars()->cleanupDisk = cleanupDisk; }
	bool GetUnpackCleanedUpDisk() { return Scalars()->unpackCleanedUpDisk; }
	void SetUnpackCleanedUpDisk(bool unpackCleanedUpDisk) { Scalars()->unpackCleanedUpDisk = unpackCleanedUpDisk; }
	bool GetAddUrlPaused() { return Scalars()->addUrlPaused; }
	void SetAddUrlPaused(bool addUrlPaused) { Scalars()->addUrlPaused = addUrlPaused; }
	FileList* GetFileList() { return reinterpret_cast<FileList*>(nzbget_rs_nzb_slot(Scalars(), 0, 0, 0)); }
	// parameters, script statuses, blocked donors and server stats live in Rust;
	// op codes as in rust/src/qnzb.rs nzbget_rs_nzb_coll
	int64 NC(int op, int a2 = 0, NzbInfo* other = nullptr, const char* s1 = nullptr, const char* s2 = nullptr,
		int* ints = nullptr, const char** strs = nullptr)
	{
		return nzbget_rs_nzb_coll(Scalars(), op, a2, other ? other->Scalars() : nullptr, s1, s2, ints, strs);
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
			0, Scalars()->size, Scalars()->parSize, Scalars()->currentFailedSize, Scalars()->parCurrentFailedSize, 0);
	}
	int CalcCriticalHealth(bool allowEstimation)
	{
		return nzbget_rs_nzb_calc_health(
			1, Scalars()->size, Scalars()->parSize, Scalars()->currentFailedSize, Scalars()->parCurrentFailedSize, allowEstimation ? 1 : 0);
	}
	const char* GetDupeKey() { return NT(7); }
	void SetDupeKey(const char* dupeKey) { NST(7, dupeKey ? dupeKey : ""); }
	int GetDupeScore() { return Scalars()->dupeScore; }
	void SetDupeScore(int dupeScore) { Scalars()->dupeScore = dupeScore; }
	EDupeMode GetDupeMode() { return (EDupeMode)Scalars()->dupeMode; }
	void SetDupeMode(EDupeMode dupeMode) { Scalars()->dupeMode = dupeMode; }
	EDupeHint GetDupeHint() { return (EDupeHint)Scalars()->dupeHint; }
	void SetDupeHint(EDupeHint dupeHint) { Scalars()->dupeHint = dupeHint; }
	uint32 GetFullContentHash() { return Scalars()->fullContentHash; }
	void SetFullContentHash(uint32 fullContentHash) { Scalars()->fullContentHash = fullContentHash; }
	uint32 GetFilteredContentHash() { return Scalars()->filteredContentHash; }
	void SetFilteredContentHash(uint32 filteredContentHash) { Scalars()->filteredContentHash = filteredContentHash; }
	int64 GetDownloadedSize() { return Scalars()->downloadedSize; }
	void SetDownloadedSize(int64 downloadedSize) { Scalars()->downloadedSize = downloadedSize; }
	int GetDownloadSec() { return Scalars()->downloadSec; }
	void SetDownloadSec(int downloadSec) { Scalars()->downloadSec = downloadSec; }
	int GetPostTotalSec() { return Scalars()->postTotalSec; }
	void SetPostTotalSec(int postTotalSec) { Scalars()->postTotalSec = postTotalSec; }
	int GetParSec() { return Scalars()->parSec; }
	void SetParSec(int parSec) { Scalars()->parSec = parSec; }
	int GetRepairSec() { return Scalars()->repairSec; }
	void SetRepairSec(int repairSec) { Scalars()->repairSec = repairSec; }
	int GetUnpackSec() { return Scalars()->unpackSec; }
	void SetUnpackSec(int unpackSec) { Scalars()->unpackSec = unpackSec; }
	time_t GetDownloadStartTime() { return Scalars()->downloadStartTime; }
	void SetDownloadStartTime(time_t downloadStartTime) { Scalars()->downloadStartTime = downloadStartTime; }
	bool GetChanged() { return Scalars()->changed; }
	void SetChanged(bool changed) { Scalars()->changed = changed; }
	void SetReprocess(bool reprocess) { Scalars()->reprocess = reprocess; }
	bool GetReprocess() { return Scalars()->reprocess; }
	time_t GetQueueScriptTime() { return Scalars()->queueScriptTime; }
	void SetQueueScriptTime(time_t queueScriptTime) { Scalars()->queueScriptTime = queueScriptTime; }
	void SetParFull(bool parFull) { Scalars()->parFull = parFull; }
	bool GetParFull() { return Scalars()->parFull; }
	int GetFeedId() { return Scalars()->feedId; }
	void SetFeedId(int feedId) { Scalars()->feedId = feedId; }
	void MoveFileList(NzbInfo* srcNzbInfo)
	{
		GetFileList()->clear();
		GetFileList()->TakeAll(*srcNzbInfo->GetFileList());
		for (FileInfo* fileInfo : GetFileList())
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
		times.reserve(GetFileList()->size());
		for (FileInfo* f : GetFileList()) times.push_back((int64_t)f->GetTime());
		int64_t minT = 0, maxT = 0;
		nzbget_rs_nzb_calc_min_max_time(times.data(), times.size(), &minT, &maxT);
		Scalars()->minTime = (time_t)minT;
		Scalars()->maxTime = (time_t)maxT;
	}
	bool GetParRepaired() { return Scalars()->parRepaired; }
	void SetParRepaired(bool parRepaired) { Scalars()->parRepaired = parRepaired; }
	bool GetUnpackTried() { return Scalars()->unpackTried; }
	void SetUnpackTried(bool unpackTried) { Scalars()->unpackTried = unpackTried; }
	int GetLastUnpackStatus() { return Scalars()->lastUnpackStatus; }
	void SetLastUnpackStatus(int lastUnpackStatus) { Scalars()->lastUnpackStatus = lastUnpackStatus; }

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

		// the object's storage is the Rust post job (rust/src/qnzb.rs Post): a
		// PostInfo* is an RsPost*. Rust creates and frees it (host op 32 sub-ops
		// 54 and 55) and answers the host ops on it (60 and 62).
		static void* operator new(size_t) { return nzbget_rs_post_new(); }
		static void operator delete(void* p) { nzbget_rs_post_free(static_cast<RsPost*>(p)); }
		PostInfo() = default;
		PostInfo(const PostInfo&) = delete;
		PostInfo& operator=(const PostInfo&) = delete;
		RsPost* Rs() const { return reinterpret_cast<RsPost*>(const_cast<PostInfo*>(this)); }

		NzbInfo* GetNzbInfo() { return reinterpret_cast<NzbInfo*>((uintptr_t)nzbget_rs_post_op(Rs(), 2, 0, 0, nullptr, nullptr)); }
		void SetNzbInfo(NzbInfo* nzbInfo) { nzbget_rs_post_op(Rs(), 3, 0, (int64)reinterpret_cast<uintptr_t>(nzbInfo), nullptr, nullptr); }
		EStage GetStage() { return (EStage)V(0); }
		void SetStage(EStage stage) { SetV(0, stage); }
		void SetProgressLabel(const char* progressLabel) { nzbget_rs_post_op(Rs(), 11, 0, 0, progressLabel, nullptr); }
		// a copy, valid on this thread until the next call
		const char* GetProgressLabel()
		{
			static thread_local CString label;
			NzbgetRsBuf buf = nzbget_rs_post_label(Rs());
			if (buf.data && buf.len > 0) label.Set(reinterpret_cast<const char*>(buf.data), (int)buf.len); else label = "";
			nzbget_rs_free(buf);
			return label;
		}
		int GetFileProgress() { return (int)V(13); }
		void SetFileProgress(int fileProgress) { SetV(13, fileProgress); }
		int GetStageProgress() { return (int)V(14); }
		void SetStageProgress(int stageProgress) { SetV(14, stageProgress); }
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
		// par repaired, unpack tried and last unpack status are the collection's
		// when there is one (Rust reads and writes it)
		bool GetParRepaired() { return V(9) != 0; }
		void SetParRepaired(bool parRepaired) { SetV(9, parRepaired); }
		bool GetUnpackTried() { return V(10) != 0; }
		void SetUnpackTried(bool unpackTried) { SetV(10, unpackTried); }
		bool GetPassListTried() { return V(11) != 0; }
		void SetPassListTried(bool passListTried) { SetV(11, passListTried); }
		int GetLastUnpackStatus() { return (int)V(12); }
		void SetLastUnpackStatus(int unpackStatus) { SetV(12, unpackStatus); }
		bool GetNeedParCheck() { return V(6) != 0; }
		void SetNeedParCheck(bool needParCheck) { SetV(6, needParCheck); }
		bool GetNeedUnpack() { return V(7) != 0; }
		void SetNeedUnpack(bool needUnpack) { SetV(7, needUnpack); }
		JobThread* GetPostThread() { return reinterpret_cast<JobThread*>((uintptr_t)V(19)); }
		void SetPostThread(JobThread* postThread) { SetV(19, (int64)reinterpret_cast<uintptr_t>(postThread)); }
		int GetParredFileCount() { return (int)nzbget_rs_post_op(Rs(), 20, 0, 0, nullptr, nullptr); }
		const char* GetParredFile(int index)
		{
			const char* s = "";
			nzbget_rs_post_op(Rs(), 21, index, 0, nullptr, &s);
			return s;
		}
		void AddParredFile(const char* filename) { nzbget_rs_post_op(Rs(), 22, 0, 0, filename ? filename : "", nullptr); }
		// extracted archives belong to the collection; a post job without one keeps its own
		int GetExtractedArchiveCount() { return (int)nzbget_rs_post_op(Rs(), 30, 0, 0, nullptr, nullptr); }
		const char* GetExtractedArchive(int index)
		{
			const char* s = "";
			nzbget_rs_post_op(Rs(), 31, index, 0, nullptr, &s);
			return s;
		}
		void ClearExtractedArchives() { nzbget_rs_post_op(Rs(), 33, 0, 0, nullptr, nullptr); }

	private:
		int64 V(int id) { return nzbget_rs_post_op(Rs(), 0, id, 0, nullptr, nullptr); }
		void SetV(int id, int64 v) { nzbget_rs_post_op(Rs(), 1, id, v, nullptr, nullptr); }
	};

	PostInfo* GetPostInfo() { return reinterpret_cast<PostInfo*>(nzbget_rs_nzb_slot(Scalars(), 1, 0, 0)); }
	// post-processing starts and ends in Rust (host op 32 sub-ops 54 and 55,
	// rust/src/qnzb.rs enter_post_process / leave_post_process)
	bool IsDupeSuccess()
	{
		return nzbget_rs_nzb_is_dupe_success(
			static_cast<int>(Scalars()->markStatus), static_cast<int>(Scalars()->deleteStatus),
			static_cast<int>(Scalars()->parStatus), static_cast<int>(Scalars()->unpackStatus),
			static_cast<int>(Scalars()->urlStatus), CalcHealth(), CalcCriticalHealth(true),
			CalcCriticalHealth(false), ActiveOptions::GetHealthCheck() == ActiveOptions::hcDupe ? 1 : 0) != 0;
	}
	const char* MakeTextStatus(bool ignoreScriptStatus)
	{
		ScriptStatus::EStatus scriptStatus = ignoreScriptStatus ?
			ScriptStatus::srSuccess : ScriptTotalStatus();
		return nzbget_rs_nzb_make_text_status(
			static_cast<int>(Scalars()->kind), static_cast<int>(Scalars()->markStatus),
			static_cast<int>(Scalars()->deleteStatus), static_cast<int>(Scalars()->parStatus),
			static_cast<int>(Scalars()->unpackStatus), static_cast<int>(Scalars()->moveStatus),
			static_cast<int>(Scalars()->urlStatus), static_cast<int>(scriptStatus),
			CalcHealth(), CalcCriticalHealth(false));
	}
	int GetMessageCount() { return (int)(intptr_t)nzbget_rs_nzb_slot(Scalars(), 4, 0, 0); }
	void SetMessageCount(int messageCount) { nzbget_rs_nzb_slot(Scalars(), 4, 1, (uintptr_t)(intptr_t)messageCount); }
	int GetCachedMessageCount() { return Scalars()->cachedMessageCount; }
	// message log ops in rust/src/qnzb.rs nzbget_rs_nzb_msgs
	int64 NM(int op, int a2 = 0, NzbInfo* p2 = nullptr, const char* s1 = nullptr, const int* ints = nullptr,
		void (*visit)(uintptr_t, int, int, int, const char*) = nullptr, uintptr_t ctx = 0)
	{
		return nzbget_rs_nzb_msgs(Scalars(), op, a2, p2 ? p2->Scalars() : nullptr, s1, ints, visit, ctx);
	}
	bool GetAllFirst() { return Scalars()->allFirst; }
	void SetAllFirst(bool allFirst) { Scalars()->allFirst = allFirst; }
	bool GetWaitingPar() { return Scalars()->waitingPar; }
	void SetWaitingPar(bool waitingPar) { Scalars()->waitingPar = waitingPar; }
	bool GetLoadingPar() { return Scalars()->loadingPar; }
	void SetLoadingPar(bool loadingPar) { Scalars()->loadingPar = loadingPar; }
	JobThread* GetUnpackThread() { return reinterpret_cast<JobThread*>(nzbget_rs_nzb_slot(Scalars(), 2, 0, 0)); }
	void SetUnpackThread(JobThread* unpackThread) { nzbget_rs_nzb_slot(Scalars(), 2, 1, reinterpret_cast<uintptr_t>(unpackThread)); }
	JobThread* GetLiveRepairThread() { return reinterpret_cast<JobThread*>(nzbget_rs_nzb_slot(Scalars(), 3, 0, 0)); }
	void SetLiveRepairThread(JobThread* liveRepairThread) { nzbget_rs_nzb_slot(Scalars(), 3, 1, reinterpret_cast<uintptr_t>(liveRepairThread)); }
	void UpdateCurrentStats()
	{
		RsDownloadCurrentStatsAcc acc{};
		acc.current_success_articles = Scalars()->successArticles;
		acc.current_failed_articles = Scalars()->failedArticles;
		acc.current_success_size = Scalars()->successSize;
		acc.current_failed_size = Scalars()->failedSize;
		acc.par_current_success_size = Scalars()->parSuccessSize;
		acc.par_current_failed_size = Scalars()->parFailedSize;
		NC(48);
		for (FileInfo* fileInfo : GetFileList())
		{
			nzbget_rs_nzb_update_current_stats_step(
				&acc, fileInfo->GetRemainingSize(), fileInfo->GetSuccessArticles(),
				fileInfo->GetFailedArticles(), fileInfo->GetSuccessSize(), fileInfo->GetFailedSize(),
				fileInfo->GetExtraPriority() ? 1 : 0, fileInfo->GetPaused() ? 1 : 0, fileInfo->GetParFile() ? 1 : 0);
			AddFileStats(fileInfo, 1, 1);
		}
		Scalars()->pausedFileCount = acc.paused_file_count;
		Scalars()->remainingParCount = acc.remaining_par_count;
		Scalars()->remainingSize = acc.remaining_size;
		Scalars()->pausedSize = acc.paused_size;
		Scalars()->currentSuccessArticles = acc.current_success_articles;
		Scalars()->currentFailedArticles = acc.current_failed_articles;
		Scalars()->currentSuccessSize = acc.current_success_size;
		Scalars()->currentFailedSize = acc.current_failed_size;
		Scalars()->parCurrentSuccessSize = acc.par_current_success_size;
		Scalars()->parCurrentFailedSize = acc.par_current_failed_size;
		Scalars()->extraPriority = acc.extra_priority;
	}
	void UpdateCompletedStats(FileInfo* fileInfo)
	{
		nzbget_rs_nzb_update_completed_stats(
			fileInfo->GetSuccessSize(), fileInfo->GetFailedSize(),
			fileInfo->GetFailedArticles(), fileInfo->GetSuccessArticles(),
			fileInfo->GetExtraPriority() ? 1 : 0, fileInfo->GetParFile() ? 1 : 0,
			fileInfo->GetPaused() ? 1 : 0,
			&Scalars()->successSize, &Scalars()->failedSize, &Scalars()->failedArticles, &Scalars()->successArticles,
			&Scalars()->extraPriority, &Scalars()->parSuccessSize, &Scalars()->parFailedSize,
			&Scalars()->remainingParCount, &Scalars()->pausedFileCount);
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
			&Scalars()->fileCount, &Scalars()->size, &Scalars()->currentSuccessSize, &Scalars()->failedSize,
			&Scalars()->failedArticles, &Scalars()->currentFailedSize, &Scalars()->totalArticles,
			&Scalars()->currentSuccessArticles, &Scalars()->currentFailedArticles, &Scalars()->remainingSize,
			&Scalars()->extraPriority, &Scalars()->remainingParCount, &Scalars()->parSize,
			&Scalars()->parCurrentSuccessSize, &Scalars()->parFailedSize, &Scalars()->parCurrentFailedSize,
			&Scalars()->pausedFileCount, &Scalars()->pausedSize);
		AddFileStats(fileInfo, 1, 2);
	}
	bool IsDownloadCompleted(bool ignorePausedPars)
	{
		if (Scalars()->activeDownloads != 0)
		{
			return false;
		}
		for (FileInfo* fileInfo : GetFileList())
		{
			if (((!fileInfo->GetPaused() || !ignorePausedPars) || !fileInfo->GetParFile()) &&
				!fileInfo->GetDeleted())
			{
				return false;
			}
		}
		return true;
	}
	void SetDesiredServerId(int id) { Scalars()->desiredServerId = id; }
	int GetDesiredServerId() { return Scalars()->desiredServerId; }
	bool GetSkipScriptProcessing() { return Scalars()->skipScriptProcessing; }
	void SetSkipScriptProcessing(bool skip) { Scalars()->skipScriptProcessing = skip; }
	void SetSkipDiskWrite(bool skipWrite) { Scalars()->skipDiskWrite = skipWrite; }
	bool GetSkipDiskWrite() { return Scalars()->skipDiskWrite; }
	void SetAutoCategory(bool autoCategory) { Scalars()->autoCategory = autoCategory; }
	bool GetAutoCategory() const { return Scalars()->autoCategory; }
	bool HasDesiredServer() const { return Scalars()->desiredServerId > 0; }

	static const int FORCE_PRIORITY = 900;

private:
	void ClearMessages() { NM(2); }

};

using PostInfo = NzbInfo::PostInfo;

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
static_assert(std::is_empty<PostInfo>::value && !std::is_polymorphic<PostInfo>::value);

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
		// other sub-ops are answered in Rust (rust/src/qnzb.rs host_coll_op)
		NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
		if (a1 == 3) { static thread_local CString s_finalDir; s_finalDir = n->BuildFinalDirName(); strs[0] = s_finalDir.Str(); return 0; }
		if (a1 == 16) { static thread_local CString s_niceUrl; s_niceUrl = n->MakeNiceUrlName(s1 ? s1 : "", s2 ? s2 : ""); strs[0] = s_niceUrl.Str(); return 0; }
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
		// plain setters are answered in Rust (rust/src/qnzb.rs host_coll_set)
		NzbInfo* n = reinterpret_cast<NzbInfo*>(p1);
		switch (a1)
		{
		case 0: n->SetId(a2); break;
		case 16: n->SetActiveDownloads(a2); break;
		case 41: n->SetFilename(s1); break;
		case 50: n->BuildDestDirName(); break;
		case 51: n->UpdateMinMaxTime(); break;
		case 52: n->UpdateCompletedStats(reinterpret_cast<FileInfo*>(p2)); break;
		case 53: n->UpdateDeletedStats(reinterpret_cast<FileInfo*>(p2)); break;
		case 96: n->SetUrl(s1); break;
		case 97: n->GetFileList()->clear(); break;
		case 98: n->UpdateCurrentStats(); break;
		default: break;
		}
		return 0;
	}
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
