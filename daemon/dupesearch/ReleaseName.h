/*
 *  This file is part of RustyGet.
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef RELEASENAME_H
#define RELEASENAME_H

#include <algorithm>
#include <cstdint>
#include <set>
#include <string>
#include <vector>
#include "nzbget_rs.h"

class ReleaseName
{
public:
	static constexpr size_t MaxNameLength = 512;
	struct Attrs
	{
		std::set<std::string> resolution;
		std::set<std::string> quality;
		std::set<std::string> codec;
		std::set<std::string> bitDepth;
		std::set<std::string> audio;
		std::set<std::string> channels;
		std::set<std::string> network;
		std::set<std::string> edition;
		std::set<std::string> hdr;
		std::string title;
		std::string group;
		std::vector<int> seasons;
		std::vector<int> episodes;
		int year = 0;
		bool repack = false;
		bool proper = false;
	};

	static std::string Clean(const std::string& name)
	{
		return Take(nzbget_rs_releasename_clean(name.data(), name.size()));
	}

	static std::string Normalize(const std::string& name)
	{
		return Take(nzbget_rs_releasename_normalize(name.data(), name.size()));
	}

	static std::string ShortQuery(const std::string& title)
	{
		return Take(nzbget_rs_releasename_short_query(title.data(), title.size()));
	}

	static Attrs Parse(const std::string& name)
	{
		std::string raw = Take(nzbget_rs_releasename_parse(name.data(), name.size()));
		struct Reader
		{
			const char* p;
			const char* end;
			uint32_t U32()
			{
				if (end - p < 4) { p = end; return 0; }
				const unsigned char* b = (const unsigned char*)p;
				p += 4;
				return (uint32_t)b[0] | (uint32_t)b[1] << 8 | (uint32_t)b[2] << 16 | (uint32_t)b[3] << 24;
			}
			std::string Str() { uint32_t n = U32(); n = std::min<uint32_t>(n, end - p); std::string s(p, n); p += n; return s; }
			int I32() { return (int)U32(); }
			bool Byte() { return p < end ? *p++ != 0 : false; }
		} in{raw.data(), raw.data() + raw.size()};

		Attrs attrs;
		for (std::set<std::string>* set : {&attrs.resolution, &attrs.quality, &attrs.codec, &attrs.bitDepth, &attrs.audio,
			&attrs.channels, &attrs.network, &attrs.edition, &attrs.hdr})
		{
			for (uint32_t n = in.U32(); n > 0 && in.p < in.end; n--)
			{
				set->insert(in.Str());
			}
		}
		attrs.title = in.Str();
		attrs.group = in.Str();
		for (std::vector<int>* list : {&attrs.seasons, &attrs.episodes})
		{
			for (uint32_t n = in.U32(); n > 0 && in.p < in.end; n--)
			{
				list->push_back(in.I32());
			}
		}
		attrs.year = in.I32();
		attrs.repack = in.Byte();
		attrs.proper = in.Byte();
		return attrs;
	}

	static bool Readable(const std::string& name)
	{
		return nzbget_rs_releasename_readable(name.data(), name.size()) != 0;
	}

	static bool SameRelease(const std::string& aName, const std::string& bName)
	{
		return nzbget_rs_releasename_same_release(aName.data(), aName.size(), bName.data(), bName.size()) != 0;
	}

private:
	static std::string Take(NzbgetRsBuf buf)
	{
		std::string out(buf.data, buf.len);
		nzbget_rs_free(buf);
		return out;
	}
};

#endif
