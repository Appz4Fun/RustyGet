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
#include "nzbget_rs.h"

YEncoder::~YEncoder()
{
	nzbget_rs_yencoder_free(m_handle);
}

bool YEncoder::OpenFile(CString& errmsg)
{
	// rust/src/nserv.rs
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

void YEncoder::WriteSegment()
{
	// rust/src/nserv.rs
	auto writeCb = [](void* ctx, const char* buf, int size)
	{
		(*static_cast<WriteFunc*>(ctx))(buf, size);
	};
	nzbget_rs_yencoder_write_segment(
		m_handle, m_filename, m_part, m_offset, m_size, m_fileSize, writeCb, &m_writeFunc);
}
