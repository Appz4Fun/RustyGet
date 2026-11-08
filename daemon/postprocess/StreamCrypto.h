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
#include "nzbget_rs.h"

class RarCryptoContext
{
public:
	static constexpr int CryptoBlockSize = 16;

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
