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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include "NzbReader.h"
#include "nzbget_rs.h"

std::string NzbSummary::Fingerprint() const
{
	std::vector<const char*> ptrs;
	std::vector<size_t> lens;
	ptrs.reserve(messageIds.size());
	lens.reserve(messageIds.size());
	for (const std::string& id : messageIds)
	{
		ptrs.push_back(id.data());
		lens.push_back(id.size());
	}
	char buffer[17] = {};
	nzbget_rs_dupesearch_fnv1a_fingerprint(ptrs.data(), lens.data(), ptrs.size(), buffer);
	return std::string(buffer, 16);
}

bool NzbReader::Parse(const std::string& data, NzbSummary& summary)
{
	summary = NzbSummary();
	RsNzbSummaryScalars scalars{};
	int ok = nzbget_rs_nzb_reader_parse(
		reinterpret_cast<const uint8_t*>(data.data()),
		data.size(),
		&scalars,
		&summary,
		[](void* ctx,
			const uint8_t* posterPtr, size_t posterLen,
			const uint8_t* mainNamePtr, size_t mainNameLen,
			const uint8_t* parNamePtr, size_t parNameLen,
			const uint8_t* parIdPtr, size_t parIdLen,
			int64_t parBytes)
		{
			NzbSummary* s = static_cast<NzbSummary*>(ctx);
			s->poster.assign(reinterpret_cast<const char*>(posterPtr), posterLen);
			s->mainName.assign(reinterpret_cast<const char*>(mainNamePtr), mainNameLen);
			s->parIndex.name.assign(reinterpret_cast<const char*>(parNamePtr), parNameLen);
			s->parIndex.messageId.assign(reinterpret_cast<const char*>(parIdPtr), parIdLen);
			s->parIndex.bytes = parBytes;
		},
		[](void* ctx, int kind,
			const uint8_t* ptr1, size_t len1,
			const uint8_t* ptr2, size_t len2,
			int64_t bytes)
		{
			NzbSummary* s = static_cast<NzbSummary*>(ctx);
			const char* c1 = reinterpret_cast<const char*>(ptr1);
			const char* c2 = reinterpret_cast<const char*>(ptr2);
			switch (kind)
			{
				case 1:
					s->groups.emplace_back(c1, len1);
					break;
				case 2:
					s->meta.emplace(std::string(c1, len1), std::string(c2, len2));
					break;
				case 3:
					s->filenames.emplace(c1, len1);
					break;
				case 4:
					s->messageIds.emplace_back(c1, len1);
					break;
				case 5:
					s->dataMessageIds.emplace_back(c1, len1);
					break;
				case 6:
					s->heads.push_back({std::string(c1, len1), std::string(c2, len2), bytes});
					break;
			}
		},
		[](void* ctx, const uint64_t* ptr, size_t len)
		{
			NzbSummary* s = static_cast<NzbSummary*>(ctx);
			s->recoveryBytes.assign(ptr, ptr + len);
		});
	if (ok == 0)
	{
		summary = NzbSummary();
		return false;
	}
	summary.files = scalars.files;
	summary.totalBytes = scalars.total_bytes;
	summary.dataFiles = scalars.data_files;
	summary.dataBytes = scalars.data_bytes;
	summary.listedParts = scalars.listed_parts;
	summary.declaredParts = scalars.declared_parts;
	summary.dataListedParts = scalars.data_listed_parts;
	summary.dataDeclaredParts = scalars.data_declared_parts;
	summary.articleBytes = scalars.article_bytes;
	return true;
}
