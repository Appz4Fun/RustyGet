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

#endif /* NZBGET_H */
