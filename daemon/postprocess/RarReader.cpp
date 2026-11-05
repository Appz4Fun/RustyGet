/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"

#include "RarReader.h"
#include <exception>
#include "nzbget_rs.h"
#include "StreamCrypto.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "ContentMap.h"

int64 RarSourceCursor::Read(void* buffer, int64 size)
{
	int eofInt = 0;
	int64 newPos = m_position;
	int64 available = nzbget_rs_rrd_rar_cursor_calc_read_window(
		m_source.Size(), m_position, size, &eofInt, &newPos);
	if (eofInt != 0)
	{
		m_eof = true;	// mimic feof: only a read past the end raises it
	}
	if (available <= 0)
	{
		return 0;
	}
	if (!m_source.Read(m_position, buffer, available))
	{
		return 0;	// source refused (donor article missing / target hole)
	}
	m_position = newPos;
	return available;
}

bool RarSourceCursor::Seek(int64 position, DiskFile::ESeekOrigin origin)
{
	// Header sizes can wrap the cursor. Use unsigned arithmetic explicitly:
	// signed overflow here is undefined even though the parser needs wrapping.
	int64 target = 0;
	if (!nzbget_rs_rrd_rar_cursor_calc_seek_target(
		m_position,
		origin == DiskFile::soEnd ? m_source.Size() : 0,
		position, (int)origin, (int)DiskFile::soCur, (int)DiskFile::soEnd,
		&target))
	{
		return false;
	}
	m_position = target;
	m_eof = false;	// like fseek clearing the eof indicator
	return true;
}

bool RarVolume::Read()
{
	debug("Checking file %s", m_filename.c_str());

	DiskFile file;
	if (!file.Open(m_filename.c_str(), DiskFile::omRead))
	{
		return false;
	}

	// size the source through the open handle: a path-based stat could race
	// a concurrent rename/replace and disagree with the handle's file
	int64 size = file.Seek(0, DiskFile::soEnd) ? file.Position() : -1;
	if (size < 0 || !file.Seek(0))
	{
		file.Close();
		return false;
	}

	DiskContentSource source(file, size);
	bool ok = ReadFrom(source);
	file.Close();
	return ok;
}

namespace
{
	// the parse's C++ side (rust/src/rarreader.rs calls back into it)
	struct RarContext
	{
		RarVolume* volume;
		ContentSource* source;
		std::exception_ptr error;
		bool signatureRead = false;
	};

	// a callback that throws makes the parse fail; the exception is kept and
	// rethrown once Rust has returned
	template <typename R, typename F>
	R RarCall(void* ctx, R failed, F f) noexcept
	{
		RarContext* c = static_cast<RarContext*>(ctx);
		if (c->error) return failed;
		try
		{
			return f(c);
		}
		catch (...)
		{
			c->error = std::current_exception();
			return failed;
		}
	}
}

bool RarVolume::ReadFrom(ContentSource& source)
{
	// rust/src/rarreader.rs parses the headers; the content source and the
	// cipher (OpenSSL) stay here. A callback that throws makes the parse fail;
	// the exception is rethrown once Rust has returned.
	RarContext context{this, &source, nullptr};

	NzbgetRsRarIo io{};
	io.ctx = &context;
	io.size = [](void* ctx) -> int64_t
	{
		return RarCall(ctx, (int64_t)-1, [](RarContext* c) { return (int64_t)c->source->Size(); });
	};
	io.read = [](void* ctx, int64_t offset, uint8_t* buffer, int64_t len) -> int
	{
		return RarCall(ctx, 0, [=](RarContext* c)
		{
			bool ok = c->source->Read(offset, buffer, len);
			c->signatureRead = true;
			return ok ? 1 : 0;
		});
	};
	io.rar3Prepare = [](void* ctx, const uint8_t* salt) -> int
	{
		return RarCall(ctx, 0, [=](RarContext* c) { return c->volume->DecryptRar3Prepare(salt) ? 1 : 0; });
	};
	io.rar5Prepare = [](void* ctx, uint8_t kdfCount, const uint8_t* salt) -> int
	{
		return RarCall(ctx, 0, [=](RarContext* c) { return c->volume->DecryptRar5Prepare(kdfCount, salt) ? 1 : 0; });
	};
	io.init = [](void* ctx, int keyBits, const uint8_t* iv) -> int
	{
		return RarCall(ctx, 0, [=](RarContext* c)
		{
			if (iv)
			{
				memcpy(c->volume->m_decryptIV, iv, sizeof(c->volume->m_decryptIV));
			}
			return c->volume->DecryptInit(keyBits) ? 1 : 0;
		});
	};
	io.decrypt = [](void* ctx, const uint8_t* in, uint8_t* out) -> int
	{
		return RarCall(ctx, 0, [=](RarContext* c) { return c->volume->DecryptBuf(in, out) ? 1 : 0; });
	};
	io.free = [](void* ctx)
	{
		RarCall(ctx, 0, [](RarContext* c) { c->volume->DecryptFree(); return 0; });
	};
	io.addFile = [](void* ctx, const NzbgetRsRarFile* f) -> int
	{
		return RarCall(ctx, 0, [f](RarContext* c)
		{
			RarFile file;
			file.m_filename.assign(f->filename, f->filenameLen);
			file.m_time = f->time;
			file.m_attr = f->attr;
			file.m_size = f->size;
			file.m_splitBefore = f->splitBefore;
			file.m_splitAfter = f->splitAfter;
			file.m_method = f->method;
			file.m_stored = f->stored;
			file.m_packedSize = f->packedSize;
			file.m_dataOffset = f->dataOffset;
			file.m_encryptedData = f->encryptedData;
			file.m_hasSalt = f->hasSalt;
			memcpy(file.m_salt, f->salt, sizeof(file.m_salt));
			file.m_hasCrypt = f->hasCrypt;
			if (f->hasCrypt)
			{
				file.m_crypt.Version = f->cryptVersion;
				file.m_crypt.Flags = f->cryptFlags;
				file.m_crypt.KdfCount = f->cryptKdfCount;
				memcpy(file.m_crypt.Salt, f->cryptSalt, sizeof(file.m_crypt.Salt));
				memcpy(file.m_crypt.Iv, f->cryptIv, sizeof(file.m_crypt.Iv));
				memcpy(file.m_crypt.CheckValue, f->cryptCheckValue, sizeof(file.m_crypt.CheckValue));
				file.m_crypt.HasCheck = f->cryptHasCheck;
			}
			c->volume->m_files.push_back(std::move(file));
			return 1;
		});
	};

	io.setDataOffset = [](void* ctx, int64_t position)
	{
		RarCall(ctx, 0, [=](RarContext* c)
		{
			RarFile& file = c->volume->m_files.back();
			// Preserve the two's-complement result without signed overflow.
			file.m_dataOffset = (int64)nzbget_rs_dqh_rar_volume_set_data_offset(
				position, (int64_t)file.m_packedSize);
			return 0;
		});
	};

	NzbgetRsRarVolume state{};
	state.version = m_version;
	state.volumeNo = m_volumeNo;
	state.newNaming = m_newNaming;
	state.hasNextVolume = m_hasNextVolume;
	state.multiVolume = m_multiVolume;
	state.encrypted = m_encrypted;
	state.hasPassword = !m_password.empty();
	memcpy(state.decryptBuf, m_decryptBuf, sizeof(m_decryptBuf));
	state.decryptPos = m_decryptPos;

	bool ok = nzbget_rs_rar_read(&io, &state) != 0;

	// DetectRarVersion never assigned m_version if Size/Read threw while
	// reading the signature. Preserve it when this is a repeated ReadFrom.
	if (!context.error || context.signatureRead) m_version = state.version;
	m_volumeNo = state.volumeNo;
	m_newNaming = state.newNaming;
	m_hasNextVolume = state.hasNextVolume;
	m_multiVolume = state.multiVolume;
	m_encrypted = state.encrypted;
	memcpy(m_decryptBuf, state.decryptBuf, sizeof(m_decryptBuf));
	m_decryptPos = state.decryptPos;

	if (context.error)
	{
		DecryptFree();
		std::rethrow_exception(context.error);
	}

	LogDebugInfo();

	return ok;
}

void RarVolume::LogDebugInfo()
{
#ifdef DEBUG
	debug("Volume: version:%i, multi:%i, vol-no:%i, new-naming:%i, has-next:%i, encrypted:%i, file-count:%i, [%s]",
		(int)m_version, (int)m_multiVolume, m_volumeNo, (int)m_newNaming, (int)m_hasNextVolume,
		(int)m_encrypted, (int)m_files.size(), FileSystem::BaseFileName(m_filename.c_str()));

	for (RarFile& file : m_files)
	{
		debug("  time:%i, size:%" PRIi64 ", attr:%i, split-before:%i, split-after:%i, [%s]",
			file.m_time, file.m_size, file.m_attr,
			file.m_splitBefore, file.m_splitAfter, file.m_filename.c_str());
	}
#endif
}

bool RarVolume::DecryptRar3Prepare(const uint8 salt[8])
{
	// key schedule lives in StreamCrypto so header decryption here and M3 stream
	// repair derive identical keys; writes m_decryptKey[0..15] and m_decryptIV
	return StreamCrypto::DeriveRar3(m_password.c_str(), salt, m_decryptKey, m_decryptIV);
}

bool RarVolume::DecryptRar5Prepare(uint8 kdfCount, const uint8 salt[16])
{
	return StreamCrypto::DeriveRar5(m_password.c_str(), kdfCount, salt, m_decryptKey);
}

bool RarVolume::DecryptInit(int keyLength)
{
#ifndef DISABLE_TLS
	m_context.reset(EVP_CIPHER_CTX_new());
	if (!m_context) 
		return false;

	if (!EVP_DecryptInit(m_context.get(),
		keyLength == 128 ? EVP_aes_128_cbc() : EVP_aes_256_cbc(),
		m_decryptKey, m_decryptIV))
		return false;
	// RAR encrypts headers/data as raw CBC with no PKCS#7 padding. Leaving
	// OpenSSL's default padding on makes EVP_DecryptUpdate buffer the final
	// block, so a single 16-byte call yields nothing (outlen 0) and the block
	// stream comes back garbage - which is why -hp header parsing never worked.
	EVP_CIPHER_CTX_set_padding(m_context.get(), 0);
	return true;
#else
	return false;
#endif
}

bool RarVolume::DecryptBuf(const uint8 in[16], uint8 out[16])
{
#ifndef DISABLE_TLS
	// padding is disabled in DecryptInit, so one 16-byte block in yields exactly
	// one 16-byte block out
	int outlen = 0;
	if (!EVP_DecryptUpdate(m_context.get(), out, &outlen, in, 16)) return false;
	if (outlen != 16) return false;
	return true;
#else
	return false;
#endif
}

void RarVolume::DecryptFree()
{
#ifndef DISABLE_TLS
	m_context.reset();
#endif
}

