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

#include <algorithm>
#include "NzbReader.h"
#include "XmlReader.h"
#include "NzbFile.h"
#include "ParParser.h"
#include "nzbget_rs.h"

#include "DupeUtil.h"
#include "nzbget_rs_dupesearch_nzb.h"

using DupeUtil::Lower;
using DupeUtil::Trim;

namespace
{

// the file name of a subject: the first quoted string, else the whole subject
std::string NameOfSubject(const std::string& subject)
{
	NzbgetRsSubjectRange range = nzbget_rs_dupesearch_name_of_subject(
		(const uint8_t*)subject.data(), subject.size());
	return subject.substr(range.start, range.len);
}

bool ValidMessageId(const std::string& id)
{
	return nzbget_rs_dupesearch_valid_message_id(id.data(), id.size()) != 0;
}

bool IsParName(const std::string& name)
{
	return nzbget_rs_dupesearch_par_name((const uint8_t*)name.data(), name.size()) != 0;
}


class Reader : public XmlReader::Handler
{
public:
	void Start(const std::string& name, const XmlReader::Attrs& attrs) override
	{
		m_text.clear();
		int elemKind = nzbget_rs_dqh_nzb_reader_classify_start_element(
			name.c_str(), m_inFile ? 1 : 0);
		if (elemKind == 1 && !XmlReader::Attr(attrs, "type").empty())
		{
			m_metaType = Lower(XmlReader::Attr(attrs, "type"));
		}
		else if (elemKind == 2)
		{
			files++;
			m_inFile = true;
			if (poster.empty())
			{
				poster = XmlReader::Attr(attrs, "poster");
			}
			m_name = NameOfSubject(XmlReader::Attr(attrs, "subject"));
			m_fileBytes = 0;
			m_firstId.clear();
			m_subject = XmlReader::Attr(attrs, "subject");
			m_parts.clear();
			m_segments.clear();
			m_validParts = 0;
		}
		else if (elemKind == 3)
		{
			m_segmentBytes = nzbget_rs_dqh_parse_c_atoll(XmlReader::Attr(attrs, "bytes").c_str());
			m_segmentNumber = nzbget_rs_dqh_parse_c_atoll(XmlReader::Attr(attrs, "number").c_str());
			m_inSegment = true;
		}
	}

	void End(const std::string& name) override
	{
		int elemKind = nzbget_rs_dqh_nzb_reader_classify_end_element(
			name.c_str(),
			!m_metaType.empty() ? 1 : 0,
			m_inSegment ? 1 : 0,
			m_inFile ? 1 : 0);
		if (elemKind == 1)
		{
			meta[m_metaType] = Trim(m_text);
			m_metaType.clear();
		}
		else if (elemKind == 2)
		{
            // Keep the ID and size together until duplicate part numbers have
            // been resolved with the queue parser's keep-last policy.
            m_segments.push_back({ m_segmentNumber, m_segmentBytes, Trim(m_text) });
			m_inSegment = false;
		}
		else if (elemKind == 3)
		{
			std::string group = Trim(m_text);
			if (!group.empty())
			{
				groups.insert(group);
			}
		}
		else if (elemKind == 4)
		{
            std::stable_sort(m_segments.begin(), m_segments.end(),
                [](const Segment& a, const Segment& b) { return a.number < b.number; });
            for (size_t i = 0; i < m_segments.size(); i++)
            {
                const Segment& segment = m_segments[i];
                if (i + 1 < m_segments.size() && m_segments[i + 1].number == segment.number) continue;
                m_parts.push_back({ segment.number, segment.bytes });
                // Invalid retained data still makes geometry unknown. Never
                // resurrect an earlier valid duplicate in place of this ID.
                if (!ValidMessageId(segment.id))
                {
                    if (!IsParName(m_name)) geometryValid = false;
                    continue;
                }
                sizes[m_name] += segment.bytes;
                ids.insert(segment.id);
                if (!IsParName(m_name)) dataIds.insert(segment.id);
                m_validParts++;
                int geomValid = geometryValid ? 1 : 0;
                int pushArticleSize = 0;
                m_fileBytes = nzbget_rs_dupesearch_accum_segment_bytes(
                    m_fileBytes, segment.bytes, IsParName(m_name) ? 1 : 0,
                    geomValid, &geomValid, &pushArticleSize);
                geometryValid = geomValid != 0;
                if (pushArticleSize != 0) articleSizes.push_back((uint64_t)segment.bytes);
                if (segment.number == 1) m_firstId = segment.id;
            }
            uint64_t listed = m_parts.size(), declared = listed;
            std::vector<NzbgetRsDupePart> raw;
            raw.reserve(m_parts.size());
            for (const auto& part : m_parts) raw.push_back({ part.first, part.second });
            int count = NzbFile::DeclaredParts(m_subject.c_str());
            nzbget_rs_dupe_part_counts(raw.data(), raw.size(), count > 0 ? (uint32_t)count : 0,
                IsParName(m_name), &listed, &declared);
            listedParts += m_validParts;
            declaredParts += declared;
            if (!IsParName(m_name))
            {
                dataListedParts += m_validParts;
                dataDeclaredParts += declared;
            }
            int newDataFiles = dataFiles;
            int64_t newDataBytes = dataBytes;
            int newGeomValid = geometryValid ? 1 : 0;
            int pushParityHead = 0;
            int pushDataHead = 0;
            nzbget_rs_dupesearch_accum_file_data_totals(
                IsParName(m_name) ? 1 : 0, m_fileBytes, m_firstId.empty() ? 1 : 0,
                dataFiles, dataBytes, newGeomValid,
                &newDataFiles, &newDataBytes, &newGeomValid, &pushParityHead, &pushDataHead);
            dataFiles = newDataFiles;
            dataBytes = newDataBytes;
            geometryValid = newGeomValid != 0;
            if (pushParityHead != 0) parityHeads.push_back({ m_name, m_firstId, m_fileBytes });
            if (pushDataHead != 0) heads.push_back({ m_name, m_firstId, m_fileBytes });
			m_inFile = false;
		}
		m_text.clear();
	}

	void Text(const std::string& text) override
	{
		m_text += text;
	}

	int files = 0;
	std::string poster;
	std::map<std::string, long long> sizes;
	std::vector<NzbSummary::Head> heads;
	long long dataBytes = 0;
	int dataFiles = 0;
	bool geometryValid = true;
	uint64_t listedParts = 0, declaredParts = 0;
	uint64_t dataListedParts = 0, dataDeclaredParts = 0;
	std::vector<uint64_t> articleSizes;
	std::vector<NzbSummary::Head> parityHeads;
	std::set<std::string> ids;
	std::set<std::string> dataIds;
	std::set<std::string> groups;
	std::map<std::string, std::string> meta;

private:
	std::string m_text;
	std::string m_metaType;
	std::string m_name;
	long long m_segmentBytes = 0;
	long long m_segmentNumber = 0;
	long long m_fileBytes = 0;
	std::string m_firstId;
	std::string m_subject;
	uint64_t m_validParts = 0;
	std::vector<std::pair<long long,long long>> m_parts;
    struct Segment
    {
        long long number;
        long long bytes;
        std::string id;
    };
    std::vector<Segment> m_segments;
	bool m_inFile = false;
	bool m_inSegment = false;
};

}

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
	Reader reader;
	if (!XmlReader::Parse(data, reader) || !reader.files || reader.ids.empty())
	{
		return false;
	}

	summary.files = reader.files;
	summary.poster = reader.poster;
	summary.meta = reader.meta;
	summary.messageIds.assign(reader.ids.begin(), reader.ids.end());
	summary.dataMessageIds.assign(reader.dataIds.begin(), reader.dataIds.end());
	summary.groups.assign(reader.groups.begin(), reader.groups.end());
    summary.listedParts = reader.listedParts;
    summary.declaredParts = reader.declaredParts;
    summary.dataListedParts = reader.dataListedParts;
    summary.dataDeclaredParts = reader.dataDeclaredParts;
    if (!reader.articleSizes.empty())
    {
        std::sort(reader.articleSizes.begin(), reader.articleSizes.end());
        summary.articleBytes = reader.articleSizes[reader.articleSizes.size() / 2];
    }
    // File size does not identify a PAR2 index: an index can be larger than
    // a recovery volume. Use the shared filename parser and keep capacity
    // paired with the selected index's collection.
    for (const auto& head : reader.parityHeads)
    {
        int baseLen = 0;
        bool parsedOk = ParParser::ParseParFilename(head.name.c_str(), true, &baseLen, nullptr);
        if (parsedOk && baseLen >= 0 && (size_t)baseLen + 5 == head.name.size())
        {
            if (!summary.parIndex.messageId.empty() &&
                !ParParser::SameParCollection(summary.parIndex.name.c_str(), head.name.c_str(), true))
            {
                // Whole-NZB dataBytes cannot be paired with one independent
                // PAR collection's capacity. Keep feasibility unknown.
                summary.parIndex = NzbSummary::Head();
                break;
            }
            if (summary.parIndex.messageId.empty()) summary.parIndex = head;
        }
    }
    if (!summary.parIndex.messageId.empty())
    {
        for (const auto& head : reader.parityHeads)
        {
            int baseLen = 0;
            bool parsedOk = ParParser::ParseParFilename(head.name.c_str(), true, &baseLen, nullptr);
            if (parsedOk && baseLen >= 0 && (size_t)baseLen + 5 < head.name.size() &&
                ParParser::SameParCollection(summary.parIndex.name.c_str(), head.name.c_str(), true))
                summary.recoveryBytes.push_back((uint64_t)head.bytes);
        }
    }

	std::vector<const std::string*> sizeNames;
	std::vector<int64_t> sizeVals;
	std::vector<uint8_t> sizeIsPar;
	sizeNames.reserve(reader.sizes.size());
	sizeVals.reserve(reader.sizes.size());
	sizeIsPar.reserve(reader.sizes.size());
	for (const auto& entry : reader.sizes)
	{
		summary.filenames.insert(Lower(entry.first));
		sizeNames.push_back(&entry.first);
		sizeVals.push_back(entry.second);
		sizeIsPar.push_back(IsParName(entry.first) ? 1 : 0);
	}
	int64_t totalBytes = 0;
	ptrdiff_t mainIdx = nzbget_rs_dupesearch_select_main_file(
		sizeVals.data(), sizeIsPar.data(), sizeVals.size(), &totalBytes);
	summary.totalBytes = totalBytes;
	if (mainIdx >= 0 && (size_t)mainIdx < sizeNames.size())
	{
		summary.mainName = *sizeNames[(size_t)mainIdx];
	}
	if (reader.geometryValid)
	{
		summary.dataBytes = reader.dataBytes;
		summary.dataFiles = reader.dataFiles;
		std::vector<int64_t> headBytes;
		headBytes.reserve(reader.heads.size());
		for (const auto& h : reader.heads)
		{
			headBytes.push_back(h.bytes);
		}
		size_t topIndices[3] = {};
		size_t topCount = nzbget_rs_dupesearch_select_top_heads(
			headBytes.data(), headBytes.size(), topIndices, 3);
		summary.heads.reserve(topCount);
		for (size_t i = 0; i < topCount; i++)
		{
			summary.heads.push_back(std::move(reader.heads[topIndices[i]]));
		}
	}
	return true;
}
