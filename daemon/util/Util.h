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

// The queue model, its host operations and the queue's drop hooks all live
// in Rust now (rust/src/qqueue.rs, qnzb.rs, qfile.rs, queuesubsys_host.rs).
// What stays here hands Rust the build flags, the TLS library's version text
// and the stdout writer once at start-up.

#define VERSION "27.0"

#include <stdio.h>
#include <stddef.h>
#include <mutex>

#ifndef DISABLE_TLS
#include <openssl/opensslv.h> // version text reported by SystemInfo; TLS itself is in Rust
#endif

#include "nzbget_rs.h"

class Util
{
public:
	static const char* VersionRevision() { return VersionRevisionBuf; }
	static inline char VersionRevisionBuf[100];

	static void Init();
};

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
		nzbget_rs_build_env_init(flags, Util::VersionRevision(), sslLib, sslVer, nullptr,
			+[](const char* ptr, size_t len, int doFlush) {
				if (ptr && len > 0) fwrite(ptr, 1, len, stdout);
				if (doFlush) fflush(stdout);
			});
	});
}

inline void Util::Init()
{
	snprintf(VersionRevisionBuf, sizeof(VersionRevisionBuf), "%s", VERSION);
	InitRsBuildEnv();
}

#endif /* UTIL_H */
