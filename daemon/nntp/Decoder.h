/*
 *  This file is part of RustyGet.
 *
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


#ifndef DECODER_H
#define DECODER_H

#include <string>
#include "NString.h"
#include "Util.h"
#include "nzbget_rs.h"

class Decoder
{
public:
	enum EStatus
	{
		dsUnknownError,
		dsFinished,
		dsArticleIncomplete,
		dsCrcError,
		dsInvalidSize,
		dsNoBinaryData
	};

	enum EFormat
	{
		efUnknown,
		efYenc,
		efUx,
	};

	// rust/src/decoder.rs
	Decoder() : m_decoder(nzbget_rs_decoder_new(nullptr, nullptr)) {}
	~Decoder() { nzbget_rs_decoder_free(m_decoder); }
	Decoder(const Decoder&) = delete;
	Decoder& operator=(const Decoder&) = delete;
	EStatus Check() { return static_cast<EStatus>(nzbget_rs_decoder_check(m_decoder)); }
	void Clear() { nzbget_rs_decoder_clear(m_decoder); }
	int DecodeBuffer(char* buffer, int len) { return nzbget_rs_decoder_decode(m_decoder, buffer, len); }
	void SetCrcCheck(bool crcCheck) { nzbget_rs_decoder_set(m_decoder, 0, crcCheck); }
	void SetRawMode(bool rawMode) { nzbget_rs_decoder_set(m_decoder, 1, rawMode); }
	EFormat GetFormat() { return static_cast<EFormat>(nzbget_rs_decoder_get(m_decoder, 0)); }
	int64 GetBeginPos() { return nzbget_rs_decoder_get(m_decoder, 1); }
	int64 GetEndPos() { return nzbget_rs_decoder_get(m_decoder, 2); }
	int64 GetSize() { return nzbget_rs_decoder_get(m_decoder, 3); }
	uint32 GetExpectedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 4)); }
	uint32 GetCalculatedCrc() { return static_cast<uint32>(nzbget_rs_decoder_get(m_decoder, 5)); }
	bool GetEof() { return nzbget_rs_decoder_get(m_decoder, 6) != 0; }
	const char* GetArticleFilename() { return nzbget_rs_decoder_filename(m_decoder); }

private:
	struct NzbgetRsDecoder* m_decoder;
};

#endif
