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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include <exception>
#include "ContentMap.h"
#include "RarReader.h"
#include "StreamCrypto.h"
#include "nzbget_rs.h"

namespace
{
	template <typename Ctx, typename R, typename F>
	R MapCall(void* rawCtx, R failed, F f) noexcept
	{
		Ctx* c = static_cast<Ctx*>(rawCtx);
		if (c->Error)
		{
			return failed;
		}
		try
		{
			return f(c);
		}
		catch (...)
		{
			c->Error = std::current_exception();
			return failed;
		}
	}

	template <typename Ctx, typename F>
	void MapCallVoid(void* rawCtx, F f) noexcept
	{
		Ctx* c = static_cast<Ctx*>(rawCtx);
		if (c->Error)
		{
			return;
		}
		try
		{
			f(c);
		}
		catch (...)
		{
			c->Error = std::current_exception();
		}
	}
}

bool DiskContentSource::Read(int64 offset, void* buffer, int64 size)
{
	return offset >= 0 && size >= 0 && offset <= m_size && size <= m_size - offset &&
		m_file.Seek(offset) && m_file.Read(buffer, size) == size;
}

bool HoledSource::Read(int64 offset, void* buffer, int64 size)
{
	if (nzbget_rs_holed_source_intersects(
		reinterpret_cast<const NzbgetRsByteRange*>(m_holes.data()),
		m_holes.size(), offset, size) != 0)
	{
		return false;
	}
	return m_inner.Read(offset, buffer, size);
}

ContentSource* HoledSourceSet::GetSource(int memberIndex)
{
	if (memberIndex < 0 || (size_t)memberIndex >= m_wrapped.size())
	{
		return nullptr;
	}
	if (!m_wrapped[memberIndex])
	{
		ContentSource* inner = m_inner.GetSource(memberIndex);
		if (!inner)
		{
			return nullptr;
		}
		m_wrapped[memberIndex] = std::make_unique<HoledSource>(
			*inner, m_memberHoles[memberIndex]);
	}
	return m_wrapped[memberIndex].get();
}

StreamRangeList ContentMap::MapToInner(int memberIndex, const StreamRange& memberRange) const
{
	std::vector<NzbgetRsContentRun> rsRuns;
	rsRuns.reserve(m_runs.size());
	for (const ContentRun& run : m_runs)
	{
		rsRuns.push_back({run.InnerOffset, run.MemberIndex, run.MemberOffset, run.Size});
	}
	std::vector<NzbgetRsByteRange> out(m_runs.size());
	NzbgetRsByteRange mr{memberRange.Offset, memberRange.Size};
	size_t count = nzbget_rs_content_map_to_inner(
		rsRuns.data(), rsRuns.size(), memberIndex, mr, out.data(), out.size());
	StreamRangeList innerRanges;
	innerRanges.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		innerRanges.push_back({out[i].offset, out[i].size});
	}
	return innerRanges;
}

std::vector<MemberRange> ContentMap::MapFromInner(const StreamRange& innerRange) const
{
	std::vector<NzbgetRsContentRun> rsRuns;
	rsRuns.reserve(m_runs.size());
	for (const ContentRun& run : m_runs)
	{
		rsRuns.push_back({run.InnerOffset, run.MemberIndex, run.MemberOffset, run.Size});
	}
	std::vector<NzbgetRsMemberRange> out(m_runs.size());
	NzbgetRsByteRange ir{innerRange.Offset, innerRange.Size};
	size_t count = nzbget_rs_content_map_from_inner(
		rsRuns.data(), rsRuns.size(), ir, out.data(), out.size());
	std::vector<MemberRange> pieces;
	pieces.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		pieces.push_back({out[i].member_index, {out[i].offset, out[i].size}});
	}
	return pieces;
}

int64 ContentMap::GetCipherStreamSize() const
{
	std::vector<int64_t> cipherSizes;
	cipherSizes.reserve(m_runCryptos.size());
	for (const RunCrypto& runCrypto : m_runCryptos)
	{
		cipherSizes.push_back(runCrypto.CipherSize);
	}
	return nzbget_rs_stream_content_map_sum_cipher_sizes(cipherSizes.data(), cipherSizes.size());
}

std::vector<MemberRange> ContentMap::MapCipherRange(const StreamRange& cipherRange) const
{
	if (cipherRange.Offset < 0 || cipherRange.Size <= 0 || m_runCryptos.size() != m_runs.size())
	{
		return {};
	}
	std::vector<NzbgetRsContentRun> rsRuns(m_runs.size());
	std::vector<int64_t> cipherOffsets(m_runs.size());
	std::vector<int64_t> cipherSizes(m_runs.size());
	std::vector<uint8_t> hasCrypto(m_runs.size());
	for (size_t i = 0; i < m_runs.size(); i++)
	{
		rsRuns[i] = {m_runs[i].InnerOffset, m_runs[i].MemberIndex, m_runs[i].MemberOffset, m_runs[i].Size};
		cipherOffsets[i] = m_runCryptos[i].CipherDataOffset;
		cipherSizes[i] = m_runCryptos[i].CipherSize;
		hasCrypto[i] = m_runCryptos[i].Crypto ? 1 : 0;
	}
	std::vector<NzbgetRsMemberRange> out(m_runs.size());
	NzbgetRsByteRange cr{cipherRange.Offset, cipherRange.Size};
	size_t count = nzbget_rs_content_map_cipher_range(
		rsRuns.data(), cipherOffsets.data(), cipherSizes.data(), hasCrypto.data(),
		m_runs.size(), cr, out.data(), out.size());
	std::vector<MemberRange> pieces;
	pieces.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		pieces.push_back({out[i].member_index, {out[i].offset, out[i].size}});
	}
	return pieces;
}

bool ContentMap::ReadCipherRange(ContentSourceSet& sources, const StreamRange& cipherRange,
	char* buffer) const
{
	if (cipherRange.Size == 0)
	{
		return true;
	}
	std::vector<MemberRange> pieces = MapCipherRange(cipherRange);
	if (pieces.empty())
	{
		return false;
	}
	int64 filled = 0;
	for (const MemberRange& piece : pieces)
	{
		ContentSource* source = sources.GetSource(piece.MemberIndex);
		if (!source || !source->Read(piece.Range.Offset, buffer + filled, piece.Range.Size))
		{
			return false;
		}
		filled += piece.Range.Size;
	}
	return true;
}

bool ContentMap::ReadInnerDecrypted(ContentSourceSet& sources, const StreamRange& innerRange,
	char* buffer) const
{
	if (innerRange.Size == 0)
	{
		return innerRange.Offset >= 0 && innerRange.Offset <= m_innerSize;
	}
	if (!buffer)
	{
		return false;
	}
	std::vector<NzbgetRsContentRun> rsRuns(m_runs.size());
	std::vector<int64_t> cipherOffsets(m_runs.size());
	std::vector<int64_t> cipherSizes(m_runs.size());
	std::vector<uint8_t> hasCrypto(m_runs.size());
	for (size_t i = 0; i < m_runs.size(); i++)
	{
		rsRuns[i] = {m_runs[i].InnerOffset, m_runs[i].MemberIndex, m_runs[i].MemberOffset, m_runs[i].Size};
		if (i < m_runCryptos.size())
		{
			cipherOffsets[i] = m_runCryptos[i].CipherDataOffset;
			cipherSizes[i] = m_runCryptos[i].CipherSize;
			hasCrypto[i] = m_runCryptos[i].Crypto ? 1 : 0;
		}
	}
	const RunCrypto* runCrypto = GetRunCrypto(0);
	const uint8* key = (runCrypto && runCrypto->Crypto) ? runCrypto->Crypto->GetKey() : nullptr;
	size_t keyLen = (runCrypto && runCrypto->Crypto) ? static_cast<size_t>(runCrypto->Crypto->GetKeyLength()) : 0;
	const uint8* iv = (runCrypto && runCrypto->Crypto) ? runCrypto->Crypto->GetIv() : nullptr;

	struct DecryptCtx
	{
		ContentSourceSet* Sources;
		std::exception_ptr Error;
	} ctx{&sources, nullptr};

	bool ok = nzbget_rs_content_map_read_inner_decrypted(
		m_innerSize,
		rsRuns.data(), cipherOffsets.data(), cipherSizes.data(), hasCrypto.data(), m_runs.size(),
		key, keyLen, iv,
		NzbgetRsByteRange{innerRange.Offset, innerRange.Size},
		reinterpret_cast<uint8_t*>(buffer),
		&ctx,
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return MapCall<DecryptCtx>(rawCtx, 0, [memberIndex](DecryptCtx* c)
			{
				return c->Sources->GetSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return MapCall<DecryptCtx>(rawCtx, static_cast<int64_t>(0), [memberIndex](DecryptCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return MapCall<DecryptCtx>(rawCtx, 0, [=](DecryptCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return (src && src->Read(offset, buf, size)) ? 1 : 0;
			});
		}) != 0;
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return ok;
}

bool ContentMapper::IsCompressibleArchive(const MemberSet& set)
{
	return nzbget_rs_content_mapper_is_compressible_archive(
		static_cast<int>(set.Format), static_cast<int>(MemberSet::mfRar),
		static_cast<int>(MemberSet::mfZip), static_cast<int>(MemberSet::mfSevenZip)) != 0;
}

std::vector<MemberSet> ContentMapper::GroupSets(const std::vector<SetMember>& members)
{
	std::vector<const char*> names;
	names.reserve(members.size());
	for (const SetMember& m : members)
	{
		names.push_back(m.Name.c_str());
	}
	struct GroupCtx
	{
		std::vector<MemberSet> Sets;
		std::exception_ptr Error;
	} ctx{{}, nullptr};
	nzbget_rs_content_mapper_group_sets(
		names.data(), names.size(), &ctx,
		[](void* rawCtx, int32_t format, const int32_t* memberIndices, size_t memberCount)
		{
			MapCallVoid<GroupCtx>(rawCtx, [=](GroupCtx* c)
			{
				MemberSet s;
				s.Format = static_cast<MemberSet::EFormat>(format);
				s.Members.assign(memberIndices, memberIndices + memberCount);
				c->Sets.push_back(std::move(s));
			});
		});
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return std::move(ctx.Sets);
}

std::unique_ptr<ContentMap> ContentMapper::BuildMap(const std::vector<SetMember>& members,
	const MemberSet& set, ContentSourceSet& sources, std::string& skipReason,
	const char* password)
{
#ifndef DISABLE_TLS
	constexpr int tlsAvailable = 1;
#else
	constexpr int tlsAvailable = 0;
#endif
	skipReason.clear();
	std::vector<const char*> memberNames(members.size());
	std::vector<int64_t> memberSizes(members.size());
	for (size_t i = 0; i < members.size(); i++)
	{
		memberNames[i] = members[i].Name.c_str();
		memberSizes[i] = members[i].Size;
	}
	struct BuildMapCtx
	{
		ContentSourceSet* Sources;
		std::unique_ptr<ContentMap> Map;
		std::string* SkipReason;
		std::exception_ptr Error;
	} ctx{&sources, nullptr, &skipReason, nullptr};

	nzbget_rs_content_mapper_build_map(
		memberNames.data(), memberSizes.data(), members.size(),
		static_cast<int32_t>(set.Format), set.Members.data(), set.Members.size(),
		password, tlsAvailable, &ctx,
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return MapCall<BuildMapCtx>(rawCtx, -1, [memberIndex](BuildMapCtx* c)
			{
				if (memberIndex < 0) return 0;
				return c->Sources->GetSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return MapCall<BuildMapCtx>(rawCtx, static_cast<int64_t>(-1), [memberIndex](BuildMapCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return MapCall<BuildMapCtx>(rawCtx, 0, [=](BuildMapCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return (src && src->Read(offset, buf, size)) ? 1 : 0;
			});
		},
#ifdef DEBUG
		[](void* rawCtx, const char* filename, int32_t version, int multiVolume,
			uint32_t volumeNo, int newNaming, int hasNextVolume, int encrypted,
			size_t fileCount, const uint32_t* fileTimes, const int64_t* fileSizes,
			const uint32_t* fileAttrs, const uint8_t* fileSplitBefore,
			const uint8_t* fileSplitAfter, const char* const* fileNames)
		{
			MapCallVoid<BuildMapCtx>(rawCtx, [=](BuildMapCtx*)
			{
				RarVolume::LogDebugVolume(filename, version, multiVolume != 0, volumeNo,
					newNaming != 0, hasNextVolume != 0, encrypted != 0, fileCount,
					fileTimes, fileSizes, fileAttrs, fileSplitBefore, fileSplitAfter, fileNames);
			});
		},
#else
		nullptr,
#endif
		[](void* rawCtx, const char* innerName, size_t innerNameLen, int64_t innerSize,
			const NzbgetRsContentRun* runs, size_t runCount,
			int encrypted, const uint8_t* key, size_t keyLen, const uint8_t* iv,
			const int64_t* cipherDataOffsets, const int64_t* cipherSizes)
		{
			MapCallVoid<BuildMapCtx>(rawCtx, [=](BuildMapCtx* c)
			{
				c->Map = std::make_unique<ContentMap>();
				c->Map->SetInnerName(std::string(innerName ? innerName : "", innerNameLen));
				c->Map->SetInnerSize(innerSize);
				std::shared_ptr<RarCryptoContext> sharedCrypto;
				if (encrypted && key && iv)
				{
					sharedCrypto = RarCryptoContext::MakeFromKeyIv(key, static_cast<int>(keyLen), iv);
				}
				for (size_t i = 0; i < runCount; i++)
				{
					c->Map->GetRuns()->push_back({runs[i].inner_offset, runs[i].member_index,
						runs[i].member_offset, runs[i].size});
					if (encrypted && cipherDataOffsets && cipherSizes)
					{
						c->Map->GetRunCryptos()->push_back({sharedCrypto, cipherDataOffsets[i], cipherSizes[i]});
					}
				}
			});
		},
		[](void* rawCtx, const char* reason)
		{
			MapCallVoid<BuildMapCtx>(rawCtx, [reason](BuildMapCtx* c)
			{
				*c->SkipReason = reason ? reason : "";
			});
		});
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return std::move(ctx.Map);
}

std::vector<RepairSetData> ContentMapper::BuildRepairSets(const std::vector<SetMember>& members,
	const std::vector<StreamRangeList>& memberHoles, ContentSourceSet& sources,
	const char* password)
{
#ifndef DISABLE_TLS
	constexpr int tlsAvailable = 1;
#else
	constexpr int tlsAvailable = 0;
#endif
	std::vector<const char*> memberNames(members.size());
	std::vector<int64_t> memberSizes(members.size());
	std::vector<size_t> holeCounts(members.size(), 0);
	std::vector<NzbgetRsByteRange> holesFlat;
	for (size_t i = 0; i < members.size(); i++)
	{
		memberNames[i] = members[i].Name.c_str();
		memberSizes[i] = members[i].Size;
		if (i < memberHoles.size())
		{
			holeCounts[i] = memberHoles[i].size();
			for (const StreamRange& hole : memberHoles[i])
			{
				holesFlat.push_back({hole.Offset, hole.Size});
			}
		}
	}

	struct BuildRepairSetsCtx
	{
		ContentSourceSet* Sources;
		std::vector<RepairSetData> Sets;
		std::exception_ptr Error;
	} ctx{&sources, {}, nullptr};

	nzbget_rs_content_mapper_build_repair_sets(
		memberNames.data(), memberSizes.data(), members.size(),
		holeCounts.data(), holesFlat.data(),
		password, tlsAvailable, &ctx,
		[](void* rawCtx, int32_t memberIndex) -> int
		{
			return MapCall<BuildRepairSetsCtx>(rawCtx, -1, [memberIndex](BuildRepairSetsCtx* c)
			{
				if (memberIndex < 0) return 0;
				return c->Sources->GetSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return MapCall<BuildRepairSetsCtx>(rawCtx, static_cast<int64_t>(-1), [memberIndex](BuildRepairSetsCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return MapCall<BuildRepairSetsCtx>(rawCtx, 0, [=](BuildRepairSetsCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return (src && src->Read(offset, buf, size)) ? 1 : 0;
			});
		},
#ifdef DEBUG
		[](void* rawCtx, const char* filename, int32_t version, int multiVolume,
			uint32_t volumeNo, int newNaming, int hasNextVolume, int encrypted,
			size_t fileCount, const uint32_t* fileTimes, const int64_t* fileSizes,
			const uint32_t* fileAttrs, const uint8_t* fileSplitBefore,
			const uint8_t* fileSplitAfter, const char* const* fileNames)
		{
			MapCallVoid<BuildRepairSetsCtx>(rawCtx, [=](BuildRepairSetsCtx*)
			{
				RarVolume::LogDebugVolume(filename, version, multiVolume != 0, volumeNo,
					newNaming != 0, hasNextVolume != 0, encrypted != 0, fileCount,
					fileTimes, fileSizes, fileAttrs, fileSplitBefore, fileSplitAfter, fileNames);
			});
		},
#else
		nullptr,
#endif
		[](void* rawCtx, int32_t format, const int32_t* setMembers, size_t setMemberCount,
			int hasMap, const char* innerName, size_t innerNameLen, int64_t innerSize,
			const NzbgetRsContentRun* runs, size_t runCount,
			int encrypted, const uint8_t* key, size_t keyLen, const uint8_t* iv,
			const int64_t* cipherDataOffsets, const int64_t* cipherSizes,
			const NzbgetRsByteRange* innerHoles, size_t innerHoleCount,
			const char* skipReason)
		{
			MapCallVoid<BuildRepairSetsCtx>(rawCtx, [=](BuildRepairSetsCtx* c)
			{
				RepairSetData data;
				data.Set.Format = static_cast<MemberSet::EFormat>(format);
				if (setMembers && setMemberCount > 0)
				{
					data.Set.Members.assign(setMembers, setMembers + setMemberCount);
				}
				if (hasMap)
				{
					data.Map = std::make_unique<ContentMap>();
					data.Map->SetInnerName(std::string(innerName ? innerName : "", innerNameLen));
					data.Map->SetInnerSize(innerSize);
					std::shared_ptr<RarCryptoContext> sharedCrypto;
					if (encrypted && key && iv)
					{
						sharedCrypto = RarCryptoContext::MakeFromKeyIv(key, static_cast<int>(keyLen), iv);
					}
					for (size_t i = 0; i < runCount; i++)
					{
						data.Map->GetRuns()->push_back({runs[i].inner_offset, runs[i].member_index,
							runs[i].member_offset, runs[i].size});
						if (encrypted && cipherDataOffsets && cipherSizes)
						{
							data.Map->GetRunCryptos()->push_back({sharedCrypto, cipherDataOffsets[i], cipherSizes[i]});
						}
					}
					data.InnerHoles.reserve(innerHoleCount);
					for (size_t i = 0; i < innerHoleCount; i++)
					{
						data.InnerHoles.push_back({innerHoles[i].offset, innerHoles[i].size});
					}
					data.OriginalInnerHoles = data.InnerHoles;
				}
				data.SkipReason = skipReason ? skipReason : "";
				c->Sets.push_back(std::move(data));
			});
		});
	if (ctx.Error)
	{
		std::rethrow_exception(ctx.Error);
	}
	return std::move(ctx.Sets);
}
