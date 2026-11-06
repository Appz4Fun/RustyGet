/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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

#include <vector>
#include <cstring>
#include <stdexcept>
#include <boost/test/unit_test.hpp>
#include "RarReader.h"
#include "ContentMap.h"

BOOST_AUTO_TEST_SUITE(PostprocessTest)

namespace
{

class MemoryRarSource : public ContentSource
{
public:
	MemoryRarSource(std::vector<char> data) : m_data(std::move(data)) {}
	virtual int64 Size() { return (int64)m_data.size(); }
	virtual bool Read(int64 offset, void* buffer, int64 size)
	{
		if (offset < 0 || size < 0 || offset + size > (int64)m_data.size())
		{
			return false;
		}
		memcpy(buffer, m_data.data() + offset, size);
		return true;
	}
private:
	std::vector<char> m_data;
};

}

// All RAR3/RAR5 volume parsing, KDF, cursor, and large-block tests have been
// ported to pure Rust in rust/src/rarreader.rs (rar_reader_full_suite_port_from_cpp).
// This test exercises the C++ exception rethrow boundary on ContentSource callbacks.
BOOST_AUTO_TEST_CASE(RarSourceExceptionStateTest)
{
	class ThrowingSource : public MemoryRarSource
	{
	public:
		ThrowingSource(const std::vector<char>& data, int failAt, bool sizeThrows = false)
			: MemoryRarSource(data), m_failAt(failAt), m_sizeThrows(sizeThrows) {}
		int64 Size() override
		{
			if (m_sizeThrows) throw std::runtime_error("size failure");
			return MemoryRarSource::Size();
		}
		bool Read(int64 offset, void* buffer, int64 size) override
		{
			if (m_reads++ == m_failAt) throw std::runtime_error("read failure");
			return MemoryRarSource::Read(offset, buffer, size);
		}
	private:
		int m_failAt;
		bool m_sizeThrows;
		int m_reads = 0;
	};
	std::vector<char> data = {'R','a','r','!',0x1a,7,1,0};
	const char file[] = {0,0,0,0,9,2,0,0,0,0,0,0,1,'a'};
	data.insert(data.end(), file, file + sizeof(file));
	data.insert(data.end(), file, file + sizeof(file));
	ThrowingSource source(data, 12);
	RarVolume volume("memory");
	BOOST_CHECK_THROW(volume.ReadFrom(source), std::runtime_error);
	BOOST_CHECK_EQUAL(volume.GetVersion(), 5);
	BOOST_REQUIRE_EQUAL(volume.GetFiles()->size(), 1);
	BOOST_CHECK_EQUAL(volume.GetFiles()->front().GetFilename(), "a");
	BOOST_CHECK_EQUAL(volume.GetFiles()->front().GetDataOffset(), 22);
	ThrowingSource signatureFailure(data, 0);
	BOOST_CHECK_THROW(volume.ReadFrom(signatureFailure), std::runtime_error);
	BOOST_CHECK_EQUAL(volume.GetVersion(), 5);
	BOOST_CHECK_EQUAL(volume.GetFiles()->size(), 1);
	ThrowingSource sizeFailure(data, -1, true);
	BOOST_CHECK_THROW(volume.ReadFrom(sizeFailure), std::runtime_error);
	BOOST_CHECK_EQUAL(volume.GetVersion(), 5);
	BOOST_CHECK_EQUAL(volume.GetFiles()->size(), 1);
}

BOOST_AUTO_TEST_SUITE_END()
