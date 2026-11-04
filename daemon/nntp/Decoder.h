/*
 *  This file is part of nzbget. See <https://nzbget.com>.
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef DECODER_H
#define DECODER_H

#include <string>
#include "NString.h"
#include "Util.h"

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
	Decoder();
	~Decoder();
	Decoder(const Decoder&) = delete;
	Decoder& operator=(const Decoder&) = delete;
	EStatus Check();
	void Clear();
	int DecodeBuffer(char* buffer, int len);
	void SetCrcCheck(bool crcCheck);
	void SetRawMode(bool rawMode);
	EFormat GetFormat();
	int64 GetBeginPos();
	int64 GetEndPos();
	int64 GetSize();
	uint32 GetExpectedCrc();
	uint32 GetCalculatedCrc();
	bool GetEof();
	const char* GetArticleFilename();

private:
	struct NzbgetRsDecoder* m_decoder;
};

#endif
