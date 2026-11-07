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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef STREAMCRYPTO_H
#define STREAMCRYPTO_H

#include <cstring>
#include <memory>
#include "RarReader.h"	// RarFile::Rar5Crypt
#include "nzbget_rs.h"

/* Shared RAR key-derivation functions extracted from RarVolume so that both
 * archive-header decryption (RarReader) and, in later M3 tasks, stream repair
 * derive byte-identical keys from the same password + salt. Task 1 provides the
 * KDF layer; Task 2 adds the rar5 password-check derivation plus the
 * random-access CBC codecs exposed through RarCryptoContext below. */
class StreamCrypto
{
public:
	static bool DeriveRar3(const char* password, const uint8 salt[8],
		uint8 keyOut[16], uint8 ivOut[16])
	{
#ifndef DISABLE_TLS
		return nzbget_rs_streamcrypto_derive_rar3(password ? password : "", salt, keyOut, ivOut) != 0;
#else
		(void)password;
		(void)salt;
		(void)keyOut;
		(void)ivOut;
		return false;
#endif
	}

	static bool DeriveRar5(const char* password, uint8 kdfCount,
		const uint8 salt[16], uint8 keyOut[32])
	{
#ifndef DISABLE_TLS
		return nzbget_rs_streamcrypto_derive_rar5(password ? password : "", kdfCount, salt, keyOut) != 0;
#else
		(void)password;
		(void)kdfCount;
		(void)salt;
		(void)keyOut;
		return false;
#endif
	}

	static bool DeriveRar5PswCheck(const char* password, uint8 kdfCount,
		const uint8 salt[16], uint8 checkOut[8])
	{
#ifndef DISABLE_TLS
		return nzbget_rs_streamcrypto_derive_rar5_psw_check(password ? password : "", kdfCount, salt, checkOut) != 0;
#else
		(void)password;
		(void)kdfCount;
		(void)salt;
		(void)checkOut;
		return false;
#endif
	}
};

class RarCryptoContext
{
public:
	static constexpr int CryptoBlockSize = 16;

	static std::unique_ptr<RarCryptoContext> MakeRar3(const char* password,
		const uint8 salt[8])
	{
		if (!password || !*password) return nullptr;

		std::unique_ptr<RarCryptoContext> ctx(new RarCryptoContext());
		if (!StreamCrypto::DeriveRar3(password, salt, ctx->m_key, ctx->m_iv))
		{
			return nullptr;
		}
		ctx->m_keyLength = 16;
		return ctx;
	}

	static std::unique_ptr<RarCryptoContext> MakeRar5(const char* password,
		const RarFile::Rar5Crypt& crypt)
	{
		if (!password || !*password) return nullptr;
		if (crypt.Version != 0) return nullptr;	// only AES-256 (crypt version 0)

		std::unique_ptr<RarCryptoContext> ctx(new RarCryptoContext());
		if (!StreamCrypto::DeriveRar5(password, crypt.KdfCount, crypt.Salt, ctx->m_key))
		{
			return nullptr;
		}

		if (crypt.HasCheck)
		{
			uint8 check[8];
			if (!StreamCrypto::DeriveRar5PswCheck(password, crypt.KdfCount, crypt.Salt, check))
			{
				return nullptr;
			}
			if (!nzbget_rs_dqh_mem_slices_equal(check, crypt.CheckValue, 8))
			{
				return nullptr;	// wrong password
			}
		}

		ctx->m_keyLength = 32;
		memcpy(ctx->m_iv, crypt.Iv, 16);	// crypt-record IV is the chain start
		return ctx;
	}

	static std::unique_ptr<RarCryptoContext> MakeFromKeyIv(const uint8* key,
		int keyLength, const uint8 iv[16])
	{
		if (!key || !iv || (keyLength != 16 && keyLength != 32))
		{
			return nullptr;
		}
		std::unique_ptr<RarCryptoContext> ctx(new RarCryptoContext());
		ctx->m_keyLength = keyLength;
		memcpy(ctx->m_key, key, (size_t)keyLength);
		memcpy(ctx->m_iv, iv, 16);
		return ctx;
	}

	const uint8* GetKey() const { return m_key; }
	int GetKeyLength() const { return m_keyLength; }
	const uint8* GetIv() const { return m_iv; }

	bool DecryptRange(const uint8* prevCipherBlock, const uint8* cipher,
		uint8* plain, int64 blocks) const
	{
		return Crypt(false, prevCipherBlock, cipher, plain, blocks);
	}

	bool EncryptRange(const uint8* prevCipherBlock, const uint8* plain,
		uint8* cipher, int64 blocks) const
	{
		return Crypt(true, prevCipherBlock, plain, cipher, blocks);
	}

private:
	RarCryptoContext() = default;

	bool Crypt(bool encrypt, const uint8* prevCipherBlock, const uint8* in,
		uint8* out, int64 blocks) const
	{
		if (blocks < 0) return false;
		if (blocks == 0) return true;	// nothing to do
		if (!in || !out) return false;

#ifndef DISABLE_TLS
		const uint8* iv = prevCipherBlock ? prevCipherBlock : m_iv;
		return nzbget_rs_streamcrypto_crypt_cbc(
			encrypt ? 1 : 0, m_key, (size_t)m_keyLength, iv, in, out, blocks) != 0;
#else
		(void)encrypt;
		(void)prevCipherBlock;
		return false;
#endif
	}

	int m_keyLength = 0;	// 16 (rar3/AES-128) or 32 (rar5/AES-256)
	uint8 m_key[32] = {};
	uint8 m_iv[16] = {};	// header IV: chain start for block 0
};

#endif
