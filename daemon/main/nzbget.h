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

#include "config.h"

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

#include <libxml/parser.h>
#include <libxml/xmlerror.h>
#include <libxml/entities.h>
#include <libxml/tree.h>

#include <rapidyenc.h>

#include <boost/asio.hpp>
#include <boost/json.hpp>
#ifndef DISABLE_TLS
#include <boost/asio/ssl.hpp>
#include "OpenSSL.h"
#endif

// NOTE: do not include <iostream> in "nzbget.h". <iostream> contains objects requiring
// initialization, causing every unit in nzbget to have initialization routine. This in particular
// is causing fatal problems in SIMD units which must not have static initialization because
// they contain code with runtime CPU dispatching.
//#include <iostream>

#ifdef HAVE_REGEX_H
#include <regex.h>
#endif

#ifndef DISABLE_GZIP
#include <zlib.h>
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
