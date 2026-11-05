/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "YEncoder.h"
#include "Util.h"
#include "FileSystem.h"
#include "Log.h"
#include "nzbget_rs.h"

bool YEncoder::OpenFile(CString& errmsg)
{
	if (m_size < 0)
	{
		errmsg = "Invalid segment size";
		return false;
	}

	if (!m_diskfile.Open(m_filename, DiskFile::omRead) || !m_diskfile.Seek(0, DiskFile::soEnd))
	{
		errmsg = "File not found";
		return false;
	}

	m_fileSize = m_diskfile.Position();
	int effSize = 0;
	if (!nzbget_rs_snh_yenc_validate_open_range(m_size, m_offset, m_fileSize, &effSize))
	{
		m_size = effSize;
		errmsg = "Invalid segment size";
		return false;
	}
	m_size = effSize;

	if (!m_diskfile.Seek(m_offset))
	{
		errmsg = "Invalid segment offset";
		return false;
	}

	return true;
}

void YEncoder::WriteSegment()
{
	StringBuilder outbuf;
	int reserveSize = 2048;
	int inbufSize = 0;
	nzbget_rs_snh_yenc_calc_segment_buffers(m_size, &reserveSize, &inbufSize);
	outbuf.Reserve(reserveSize);

	NzbgetRsBuf beginHdr = nzbget_rs_dqh_yenc_format_begin_headers(
		m_part, m_fileSize, FileSystem::BaseFileName(m_filename), m_offset, m_size);
	outbuf.Append(beginHdr.data ? beginHdr.data : "", (int)beginHdr.len);
	nzbget_rs_free(beginHdr);

	Crc32 crc;
	CharBuffer inbuf(inbufSize);
	int lnsz = 0;

	while (m_diskfile.Position() < m_offset + m_size)
	{
		int64 needBytes = std::min((int64)inbuf.Size(), m_offset + m_size - m_diskfile.Position());
		int64 readBytes = m_diskfile.Read(inbuf, needBytes);
		bool lastblock = m_diskfile.Position() == m_offset + m_size;
		if (readBytes == 0)
		{
			return; // error;
		}

		crc.Append((uchar*)(const char*)inbuf, (int)readBytes);

		size_t consumed = 0;
		while (consumed < (size_t)readBytes)
		{
			size_t outLen = (size_t)outbuf.Length();
			int needFlush = 0;
			size_t stepConsumed = nzbget_rs_dqh_yenc_encode_chunk_step(
				(const char*)inbuf + consumed,
				(size_t)readBytes - consumed,
				lastblock ? 1 : 0,
				&lnsz,
				(char*)outbuf,
				&outLen,
				(size_t)outbuf.Capacity(),
				&needFlush);
			outbuf.SetLength((int)outLen);
			consumed += stepConsumed;
			if (needFlush)
			{
				m_writeFunc(outbuf, outbuf.Length());
				outbuf.SetLength(0);
			}
		}
	}
	m_diskfile.Close();

	NzbgetRsBuf endTrl = nzbget_rs_dqh_yenc_format_end_trailer(m_size, (uint32_t)crc.Finish());
	outbuf.Append(endTrl.data ? endTrl.data : "", (int)endTrl.len);
	nzbget_rs_free(endTrl);
	m_writeFunc(outbuf, outbuf.Length());
}
