/*
 *  This file is part of RustyGet.
 *
 *  Copyright (C) 2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef YENCODER_H
#define YENCODER_H

#include "NString.h"
#include "nzbget_rs.h"

struct NzbgetRsYEncoderFile;

class YEncoder
{
public:
	typedef std::function<void(const char* buf, int size)> WriteFunc;

	YEncoder(const char* filename, int part, int64 offset, int size, WriteFunc writeFunc) :
		m_filename(filename), m_part(part), m_offset(offset), m_size(size), m_fileSize(0), m_writeFunc(writeFunc) {};
	~YEncoder()
	{
		nzbget_rs_yencoder_free(m_handle);
	}

	bool OpenFile(CString& errmsg)
	{
		nzbget_rs_yencoder_free(m_handle);
		const char* err = nullptr;
		m_handle = nzbget_rs_yencoder_open(m_filename, m_offset, &m_size, &m_fileSize, &err);
		if (!m_handle)
		{
			errmsg = err ? err : "";
			return false;
		}
		return true;
	}

	void WriteSegment()
	{
		auto writeCb = +[](void* ctx, const char* buf, int size)
		{
			(*static_cast<WriteFunc*>(ctx))(buf, size);
		};
		nzbget_rs_yencoder_write_segment(
			m_handle, m_filename, m_part, m_offset, m_size, m_fileSize, writeCb, &m_writeFunc);
	}

private:
	NzbgetRsYEncoderFile* m_handle = nullptr;
	CString m_filename;
	int m_part;
	int64 m_offset;
	int m_size;
	int64 m_fileSize;
	WriteFunc m_writeFunc;
};

#endif
