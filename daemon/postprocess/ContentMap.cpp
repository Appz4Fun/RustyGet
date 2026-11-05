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

#include "ContentMap.h"
#include "RarReader.h"
#include "StreamCrypto.h"
#include "nzbget_rs.h"

bool DiskContentSource::Read(int64 offset, void* buffer, int64 size)
{
	return offset >= 0 && size >= 0 && offset <= m_size && size <= m_size - offset &&
		m_file.Seek(offset) && m_file.Read(buffer, size) == size;
}

CompositeSource::CompositeSource(ContentSourceSet& sources, std::vector<int> memberIndexes,
	std::vector<int64> memberSizes) :
	m_sources(sources), m_memberIndexes(std::move(memberIndexes)),
	m_memberSizes(std::move(memberSizes))
{
	m_memberBases.resize(m_memberSizes.size());
	m_totalSize = nzbget_rs_stream_composite_compute_member_bases(
		m_memberSizes.data(), m_memberBases.data(), m_memberSizes.size());
}

bool CompositeSource::Read(int64 offset, void* buffer, int64 size)
{
	if (!(offset >= 0 && size >= 0 && offset <= m_totalSize && size <= m_totalSize - offset))
	{
		return false;
	}

	for (const MemberRange& piece : ToMembers({offset, size}))
	{
		ContentSource* source = m_sources.GetSource(piece.MemberIndex);
		if (!source || !source->Read(piece.Range.Offset, buffer, piece.Range.Size))
		{
			return false;
		}
		buffer = (char*)buffer + piece.Range.Size;
	}
	return true;
}

std::vector<MemberRange> CompositeSource::ToMembers(const StreamRange& logicalRange) const
{
	std::vector<NzbgetRsMemberRange> rsPieces(m_memberIndexes.size());
	NzbgetRsByteRange lr{logicalRange.Offset, logicalRange.Size};
	size_t count = nzbget_rs_composite_to_members(
		m_memberIndexes.data(), m_memberBases.data(), m_memberSizes.data(),
		m_memberIndexes.size(), lr, rsPieces.data(), rsPieces.size());
	std::vector<MemberRange> pieces;
	pieces.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		pieces.push_back({rsPieces[i].member_index, {rsPieces[i].offset, rsPieces[i].size}});
	}
	return pieces;
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
	constexpr int64 blockSize = RarCryptoContext::CryptoBlockSize;
	int empty = 0;
	int64 cipherFrom = 0;
	int64 cipherTo = 0;
	if (!nzbget_rs_content_map_calc_decrypt_cipher_bounds(
		innerRange.Offset, innerRange.Size, m_innerSize, blockSize, GetCipherStreamSize(),
		&empty, &cipherFrom, &cipherTo))
	{
		return false;
	}
	if (empty)
	{
		return true;
	}
	const RunCrypto* runCrypto = GetRunCrypto(0);
	if (!runCrypto)
	{
		return false;
	}
	// CBC random access: the predecessor cipher block seeds the chain (it may
	// live in the previous member); the header IV seeds block 0
	uint8 prev[RarCryptoContext::CryptoBlockSize];
	if (cipherFrom > 0 &&
		!ReadCipherRange(sources, {cipherFrom - blockSize, blockSize}, (char*)prev))
	{
		return false;
	}
	std::vector<uint8> cipher((size_t)(cipherTo - cipherFrom));
	std::vector<uint8> plain(cipher.size());
	if (!ReadCipherRange(sources, {cipherFrom, cipherTo - cipherFrom}, (char*)cipher.data()) ||
		!runCrypto->Crypto->DecryptRange(cipherFrom > 0 ? prev : nullptr,
			cipher.data(), plain.data(), (cipherTo - cipherFrom) / blockSize))
	{
		return false;
	}
	memcpy(buffer, plain.data() + (innerRange.Offset - cipherFrom), innerRange.Size);
	return true;
}

void ContentMap::ExcludeMember(int memberIndex)
{
	// when the map is encrypted, m_runCryptos runs parallel to m_runs; filter
	// both by index so the per-run crypto stays aligned after the drop
	bool parallel = m_runCryptos.size() == m_runs.size();
	std::vector<ContentRun> keptRuns;
	std::vector<RunCrypto> keptCryptos;
	for (size_t i = 0; i < m_runs.size(); i++)
	{
		if (m_runs[i].MemberIndex == memberIndex)
		{
			continue;
		}
		keptRuns.push_back(m_runs[i]);
		if (parallel)
		{
			keptCryptos.push_back(m_runCryptos[i]);
		}
	}
	m_runs = std::move(keptRuns);
	if (parallel)
	{
		m_runCryptos = std::move(keptCryptos);
	}
}


bool ContentMapper::IsCompressibleArchive(const MemberSet& set)
{
	return nzbget_rs_content_mapper_is_compressible_archive(
		(int)set.Format, (int)MemberSet::mfRar, (int)MemberSet::mfZip, (int)MemberSet::mfSevenZip) != 0;
}

std::vector<MemberSet> ContentMapper::GroupSets(const std::vector<SetMember>& members)
{
	std::vector<const char*> names;
	names.reserve(members.size());
	for (const SetMember& m : members)
	{
		names.push_back(m.Name.c_str());
	}
	std::vector<MemberSet> sets;
	nzbget_rs_content_mapper_group_sets(
		names.data(), names.size(), &sets,
		[](void* ctx, int32_t format, const int32_t* memberIndices, size_t memberCount)
		{
			auto* out = static_cast<std::vector<MemberSet>*>(ctx);
			MemberSet s;
			s.Format = static_cast<MemberSet::EFormat>(format);
			s.Members.assign(memberIndices, memberIndices + memberCount);
			out->push_back(std::move(s));
		});
	return sets;
}

std::unique_ptr<ContentMap> ContentMapper::BuildMap(const std::vector<SetMember>& members,
	const MemberSet& set, ContentSourceSet& sources, std::string& skipReason,
	const char* password)
{
	skipReason.clear();
	switch (set.Format)
	{
		case MemberSet::mfBare:
			return BuildBareMap(members, set, sources, skipReason);
		case MemberSet::mfSplit:
			return BuildSplitMap(members, set, sources, skipReason);
		case MemberSet::mfRar:
			return BuildRarMap(members, set, sources, skipReason, password);
		case MemberSet::mfZip:
			return BuildZipMap(members, set, sources, skipReason);
		case MemberSet::mfSevenZip:
			return BuildSevenZipMap(members, set, sources, skipReason);
		default:
			skipReason = "format mapper not implemented";
			return nullptr;
	}
}

std::unique_ptr<ContentMap> ContentMapper::BuildBareMap(const std::vector<SetMember>& members,
	const MemberSet& set, ContentSourceSet& sources, std::string& skipReason)
{
	int memberIndex = set.Members[0];
	ContentSource* source = sources.GetSource(memberIndex);
	if (!source || source->Size() <= 0)
	{
		skipReason = "member unreadable";
		return nullptr;
	}

	std::unique_ptr<ContentMap> map = std::make_unique<ContentMap>();
	map->SetInnerName(members[memberIndex].Name.c_str());
	map->SetInnerSize(source->Size());
	map->GetRuns()->push_back({0, memberIndex, 0, source->Size()});
	return map;
}

std::unique_ptr<ContentMap> ContentMapper::BuildSplitMap(const std::vector<SetMember>& members,
	const MemberSet& set, ContentSourceSet& sources, std::string& skipReason)
{
	// inner name: the shared stem without the numeric suffix
	const std::string& firstName = members[set.Members[0]].Name;
	NzbgetRsBuf innerBuf = nzbget_rs_stream_content_mapper_split_inner_name(firstName.c_str());
	std::string innerName(innerBuf.data ? innerBuf.data : "", innerBuf.len);
	nzbget_rs_free(innerBuf);
	if (innerName.empty())
	{
		skipReason = "inner file is not a media file";
		return nullptr;
	}

	std::unique_ptr<ContentMap> map = std::make_unique<ContentMap>();
	map->SetInnerName(innerName.c_str());

	int64 innerOffset = 0;
	for (int memberIndex : set.Members)
	{
		ContentSource* source = sources.GetSource(memberIndex);
		if (!source || source->Size() <= 0)
		{
			// every member's size shapes the offsets after it - unlike rar
			// volumes, a split set with an unreadable member cannot map at all
			skipReason = "member " + members[memberIndex].Name + " unreadable";
			return nullptr;
		}
		map->GetRuns()->push_back({innerOffset, memberIndex, 0, source->Size()});
		innerOffset += source->Size();
	}
	map->SetInnerSize(innerOffset);
	return map;
}

std::unique_ptr<ContentMap> ContentMapper::BuildRarMap(const std::vector<SetMember>& members,
	const MemberSet& set, ContentSourceSet& sources, std::string& skipReason,
	const char* password)
{
	// parse every volume's headers; at most ONE unreadable volume is carried
	// as an unknown whose packed size is inferred from the inner size, so
	// the rest of the set still maps (its own holes stay for par2)
	std::vector<std::unique_ptr<RarVolume>> volumes(set.Members.size());
	int unknownVolume = -1;

	for (size_t i = 0; i < set.Members.size(); i++)
	{
		int memberIndex = set.Members[i];
		ContentSource* source = sources.GetSource(memberIndex);
		std::unique_ptr<RarVolume> volume =
			std::make_unique<RarVolume>(members[memberIndex].Name.c_str());
		// the archive password (a donor's, or the target's own threaded through
		// BuildRepairSets) unlocks header-encrypted (-hp) volumes: set it before
		// the parse so header decryption runs. Plaintext-header volumes ignore
		// it; a wrong -hp password fails the parse and skips as encrypted headers.
		// Target-side -hp is thus attempted too: it is safe because a holed
		// header region makes RarSourceCursor::Read return short/zero, failing
		// the parse to the same skip - so a successful parse means the headers
		// were fully primary (non-hole) bytes.
		if (password)
		{
			volume->SetPassword(password);
		}
		if (source && volume->ReadFrom(*source))
		{
			volumes[i] = std::move(volume);
			continue;
		}
		int unreadableClass = nzbget_rs_content_mapper_classify_unreadable_rar_vol(
			source != nullptr ? 1 : 0, volume->GetEncrypted() ? 1 : 0, unknownVolume);
		if (unreadableClass == 1)
		{
			skipReason = SkipEncryptedHeaders;
			return nullptr;
		}
		if (unreadableClass == 2)
		{
			skipReason = "unreadable headers in more than one volume";
			return nullptr;
		}
		unknownVolume = (int)i;
	}

	std::vector<uint8_t> volReadable(set.Members.size());
	std::vector<int32_t> volVersions(set.Members.size());
	std::vector<int32_t> volMemberIndices(set.Members.size());
	std::vector<uint8_t> volHasSource(set.Members.size());
	std::vector<int64_t> volSourceSizes(set.Members.size());
	std::vector<NzbgetRsRarFileEntry> entries;
	std::vector<RarFile*> entryFiles;

	for (size_t i = 0; i < set.Members.size(); i++)
	{
		int memberIndex = set.Members[i];
		ContentSource* source = sources.GetSource(memberIndex);
		volReadable[i] = volumes[i] != nullptr ? 1 : 0;
		volVersions[i] = volumes[i] ? volumes[i]->GetVersion() : 0;
		volMemberIndices[i] = memberIndex;
		volHasSource[i] = source != nullptr ? 1 : 0;
		volSourceSizes[i] = source ? source->Size() : 0;
		if (!volumes[i])
		{
			continue;
		}
		for (RarFile& innerFile : *volumes[i]->GetFiles())
		{
			NzbgetRsRarFileEntry e{};
			e.volume_index = (int32_t)i;
			e.filename = innerFile.GetFilename();
			e.size = innerFile.GetSize();
			e.packed_size = innerFile.GetPackedSize();
			e.data_offset = innerFile.GetDataOffset();
			e.stored = innerFile.GetStored() ? 1 : 0;
			e.encrypted_data = innerFile.GetEncryptedData() ? 1 : 0;
			if (innerFile.GetHasSalt())
			{
				e.has_salt = 1;
				memcpy(e.salt, innerFile.GetSalt(), 8);
			}
			if (const RarFile::Rar5Crypt* crypt = innerFile.GetCrypt())
			{
				e.has_crypt = 1;
				e.crypt_version = crypt->Version;
				e.crypt_kdf_count = crypt->KdfCount;
				memcpy(e.crypt_salt, crypt->Salt, 16);
				memcpy(e.crypt_iv, crypt->Iv, 16);
			}
			entries.push_back(e);
			entryFiles.push_back(&innerFile);
		}
	}

	struct RarBuildCtx
	{
		const std::vector<RarFile*>* EntryFiles;
		const char* Password;
		std::shared_ptr<RarCryptoContext> Crypto;
		std::unique_ptr<ContentMap> Map;
		std::string* SkipReason;
	} ctx{&entryFiles, password, nullptr, nullptr, &skipReason};

	nzbget_rs_content_mapper_build_rar_map(
		unknownVolume, password != nullptr ? 1 : 0,
		volReadable.data(), volVersions.data(), volMemberIndices.data(),
		volHasSource.data(), volSourceSizes.data(), set.Members.size(),
		entries.data(), entries.size(), &ctx,
		[](void* rawCtx, size_t entryIdx, int version) -> int
		{
			auto* c = static_cast<RarBuildCtx*>(rawCtx);
			if (entryIdx >= c->EntryFiles->size())
			{
				return 0;
			}
			RarFile* rf = (*c->EntryFiles)[entryIdx];
			if (version == 3)
			{
				c->Crypto = RarCryptoContext::MakeRar3(c->Password, rf->GetSalt());
			}
			else if (version == 5 && rf->GetCrypt())
			{
				c->Crypto = RarCryptoContext::MakeRar5(c->Password, *rf->GetCrypt());
			}
			return c->Crypto != nullptr ? 1 : 0;
		},
		[](void* rawCtx, const char* innerName, int64_t innerSize,
			const NzbgetRsContentRun* runs, size_t runCount,
			int encrypted, const int64_t* cipherDataOffsets, const int64_t* cipherSizes)
		{
			auto* c = static_cast<RarBuildCtx*>(rawCtx);
			c->Map = std::make_unique<ContentMap>();
			c->Map->SetInnerName(innerName);
			c->Map->SetInnerSize(innerSize);
			for (size_t i = 0; i < runCount; i++)
			{
				c->Map->GetRuns()->push_back({runs[i].inner_offset, runs[i].member_index,
					runs[i].member_offset, runs[i].size});
				if (encrypted && cipherDataOffsets && cipherSizes)
				{
					c->Map->GetRunCryptos()->push_back({c->Crypto, cipherDataOffsets[i], cipherSizes[i]});
				}
			}
		},
		[](void* rawCtx, const char* reason)
		{
			auto* c = static_cast<RarBuildCtx*>(rawCtx);
			*c->SkipReason = reason ? reason : "";
		});
	return std::move(ctx.Map);
}

std::unique_ptr<ContentMap> ContentMapper::BuildZipMap(const std::vector<SetMember>& members,
	const MemberSet& set, ContentSourceSet& sources, std::string& skipReason)
{
	(void)members;
	std::vector<int64_t> memberSizes;
	memberSizes.reserve(set.Members.size());
	for (int memberIndex : set.Members)
	{
		ContentSource* source = sources.GetSource(memberIndex);
		if (!source || source->Size() <= 0)
		{
			skipReason = "member unreadable";
			return nullptr;
		}
		memberSizes.push_back(source->Size());
	}
	struct MapBuildCtx
	{
		ContentSourceSet* Sources;
		std::unique_ptr<ContentMap> Map;
		std::string* SkipReason;
	} ctx{&sources, nullptr, &skipReason};

	nzbget_rs_content_mapper_build_zip_map(
		set.Members.data(), memberSizes.data(), set.Members.size(), &ctx,
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			auto* c = static_cast<MapBuildCtx*>(rawCtx);
			ContentSource* src = c->Sources->GetSource(memberIndex);
			return (src && src->Read(offset, buf, size)) ? 1 : 0;
		},
		[](void* rawCtx, const char* innerName, int64_t innerSize,
			const NzbgetRsContentRun* runs, size_t runCount)
		{
			auto* c = static_cast<MapBuildCtx*>(rawCtx);
			c->Map = std::make_unique<ContentMap>();
			c->Map->SetInnerName(innerName);
			c->Map->SetInnerSize(innerSize);
			for (size_t i = 0; i < runCount; i++)
			{
				c->Map->GetRuns()->push_back({runs[i].inner_offset, runs[i].member_index,
					runs[i].member_offset, runs[i].size});
			}
		},
		[](void* rawCtx, const char* reason)
		{
			auto* c = static_cast<MapBuildCtx*>(rawCtx);
			*c->SkipReason = reason ? reason : "";
		});
	return std::move(ctx.Map);
}

std::unique_ptr<ContentMap> ContentMapper::BuildSevenZipMap(const std::vector<SetMember>& members,
	const MemberSet& set, ContentSourceSet& sources, std::string& skipReason)
{
	(void)members;
	std::vector<int64_t> memberSizes;
	memberSizes.reserve(set.Members.size());
	for (int memberIndex : set.Members)
	{
		ContentSource* source = sources.GetSource(memberIndex);
		if (!source || source->Size() <= 0)
		{
			skipReason = "member unreadable";
			return nullptr;
		}
		memberSizes.push_back(source->Size());
	}
	struct MapBuildCtx
	{
		ContentSourceSet* Sources;
		std::unique_ptr<ContentMap> Map;
		std::string* SkipReason;
	} ctx{&sources, nullptr, &skipReason};

	nzbget_rs_content_mapper_build_seven_zip_map(
		set.Members.data(), memberSizes.data(), set.Members.size(), &ctx,
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			auto* c = static_cast<MapBuildCtx*>(rawCtx);
			ContentSource* src = c->Sources->GetSource(memberIndex);
			return (src && src->Read(offset, buf, size)) ? 1 : 0;
		},
		[](void* rawCtx, const char* innerName, int64_t innerSize,
			const NzbgetRsContentRun* runs, size_t runCount)
		{
			auto* c = static_cast<MapBuildCtx*>(rawCtx);
			c->Map = std::make_unique<ContentMap>();
			c->Map->SetInnerName(innerName);
			c->Map->SetInnerSize(innerSize);
			for (size_t i = 0; i < runCount; i++)
			{
				c->Map->GetRuns()->push_back({runs[i].inner_offset, runs[i].member_index,
					runs[i].member_offset, runs[i].size});
			}
		},
		[](void* rawCtx, const char* reason)
		{
			auto* c = static_cast<MapBuildCtx*>(rawCtx);
			*c->SkipReason = reason ? reason : "";
		});
	return std::move(ctx.Map);
}

std::vector<RepairSetData> ContentMapper::BuildRepairSets(const std::vector<SetMember>& members,
	const std::vector<StreamRangeList>& memberHoles, ContentSourceSet& sources,
	const char* password)
{
	std::vector<RepairSetData> repairSets;

	for (MemberSet& set : GroupSets(members))
	{
		bool anyHoles = false;
		for (int memberIndex : set.Members)
		{
			anyHoles |= !memberHoles[memberIndex].empty();
		}
		if (!anyHoles)
		{
			continue;
		}

		RepairSetData data;
		data.Set = set;
		data.Map = BuildMap(members, set, sources, data.SkipReason, password);

		if (data.Map)
		{
			std::vector<NzbgetRsContentRun> rsRuns;
			rsRuns.reserve(data.Map->GetRuns()->size());
			for (const ContentRun& run : *data.Map->GetRuns())
			{
				rsRuns.push_back({run.InnerOffset, run.MemberIndex, run.MemberOffset, run.Size});
			}
			std::vector<int32_t> holeMembers;
			std::vector<NzbgetRsByteRange> holeRanges;
			for (int memberIndex : set.Members)
			{
				for (const StreamRange& hole : memberHoles[memberIndex])
				{
					holeMembers.push_back(memberIndex);
					holeRanges.push_back({hole.Offset, hole.Size});
				}
			}
			nzbget_rs_content_mapper_collect_inner_holes(
				rsRuns.data(), rsRuns.size(),
				holeMembers.data(), holeRanges.data(), holeRanges.size(),
				&data.InnerHoles,
				[](void* ctx, const NzbgetRsByteRange* innerHoles, size_t len)
				{
					auto* dst = static_cast<StreamRangeList*>(ctx);
					dst->reserve(len);
					for (size_t i = 0; i < len; i++)
					{
						dst->push_back({innerHoles[i].offset, innerHoles[i].size});
					}
				});
			data.OriginalInnerHoles = data.InnerHoles;
			if (data.InnerHoles.empty())
			{
				data.Map.reset();
				data.SkipReason = "holes lie outside the mappable inner stream";
			}
		}

		repairSets.push_back(std::move(data));
	}

	return repairSets;
}

StreamRangeList ContentMapper::CoalesceRanges(std::vector<StreamRange> ranges)
{
	std::vector<NzbgetRsByteRange> out(ranges.size());
	size_t count = nzbget_rs_content_mapper_coalesce_ranges(
		reinterpret_cast<const NzbgetRsByteRange*>(ranges.data()), ranges.size(),
		out.data(), out.size());
	StreamRangeList merged;
	merged.reserve(count);
	for (size_t i = 0; i < count; i++)
	{
		merged.push_back({out[i].offset, out[i].size});
	}
	return merged;
}
