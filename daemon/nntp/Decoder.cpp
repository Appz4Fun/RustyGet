/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "Decoder.h"
#include "Log.h"
#include "Util.h"

#include "nzbget_rs.h"

namespace
{
	int YencDecode(const void** src, void** dst, size_t len, int* state)
	{
		auto yencState = static_cast<RapidYencDecoderState>(*state);
		int result = rapidyenc_decode_incremental(src, dst, len, &yencState);
		*state = static_cast<int>(yencState);
		return result;
	}

	unsigned int YencCrc(const void* src, size_t len, unsigned int init)
	{
		return rapidyenc_crc(src, len, init);
	}
}

Decoder::Decoder() : m_decoder(nzbget_rs_decoder_new(YencDecode, YencCrc)) {}
Decoder::~Decoder() { nzbget_rs_decoder_free(m_decoder); }
Decoder::EStatus Decoder::Check() { return static_cast<EStatus>(nzbget_rs_decoder_check(m_decoder)); }
void Decoder::Clear() { nzbget_rs_decoder_clear(m_decoder); }
int Decoder::DecodeBuffer(char* buffer, int len) { return nzbget_rs_decoder_decode(m_decoder, buffer, len); }
void Decoder::SetCrcCheck(bool crcCheck) { nzbget_rs_decoder_set(m_decoder, 0, crcCheck); }
void Decoder::SetRawMode(bool rawMode) { nzbget_rs_decoder_set(m_decoder, 1, rawMode); }
Decoder::EFormat Decoder::GetFormat() { return static_cast<EFormat>(nzbget_rs_decoder_get(m_decoder, 0)); }
int64 Decoder::GetBeginPos() { return nzbget_rs_decoder_get(m_decoder, 1); }
int64 Decoder::GetEndPos() { return nzbget_rs_decoder_get(m_decoder, 2); }
int64 Decoder::GetSize() { return nzbget_rs_decoder_get(m_decoder, 3); }
uint32 Decoder::GetExpectedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 4)); }
uint32 Decoder::GetCalculatedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 5)); }
bool Decoder::GetEof() { return nzbget_rs_decoder_get(m_decoder, 6) != 0; }
const char* Decoder::GetArticleFilename() { return nzbget_rs_decoder_filename(m_decoder); }
