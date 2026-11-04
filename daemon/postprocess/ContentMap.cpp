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

#include <algorithm>
#include <cctype>
#include "ContentMap.h"
#include "DupeStreamRepair.h"
#include "RarReader.h"
#include "StreamCrypto.h"
#include "Util.h"
#include "nzbget_rs.h"

bool DiskContentSource::Read(int64 offset, void* buffer, int64 size)
{
	return nzbget_rs_cdn_content_source_range_valid(offset, size, m_size) != 0 &&
		m_file.Seek(offset) && m_file.Read(buffer, size) == size;
}

CompositeSource::CompositeSource(ContentSourceSet& sources, std::vector<int> memberIndexes,
	std::vector<int64> memberSizes) :
	m_sources(sources), m_memberIndexes(std::move(memberIndexes)),
	m_memberSizes(std::move(memberSizes))
{
	for (int64 memberSize : m_memberSizes)
	{
		m_memberBases.push_back(m_totalSize);
		m_totalSize += memberSize;
	}
}

bool CompositeSource::Read(int64 offset, void* buffer, int64 size)
{
	if (nzbget_rs_cdn_content_source_range_valid(offset, size, m_totalSize) == 0)
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
	if (!nzbget_rs_srp_stream_repair_is_member_index_in_bounds(memberIndex, m_wrapped.size()))
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
	int64 total = 0;
	for (const RunCrypto& runCrypto : m_runCryptos)
	{
		total += runCrypto.CipherSize;
	}
	return total;
}

std::vector<MemberRange> ContentMap::MapCipherRange(const StreamRange& cipherRange) const
{
	if (cipherRange.Offset < 0 || cipherRange.Size <= 0 ||
		m_runCryptos.size() != m_runs.size())
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
	if (!nzbget_rs_cdn_content_map_calc_decrypt_cipher_bounds(
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
		int keepCrypto = 0;
		if (!nzbget_rs_sap_content_map_exclude_member_step(
			m_runs[i].MemberIndex, memberIndex, m_runCryptos.size(), m_runs.size(), &keepCrypto))
		{
			continue;
		}
		keptRuns.push_back(m_runs[i]);
		if (keepCrypto)
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
	return nzbget_rs_cdn_content_mapper_is_compressible_archive(
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
	if (!nzbget_rs_src_content_map_can_build_bare_or_split_member(
		source != nullptr ? 1 : 0, source ? source->Size() : 0))
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
	std::string innerName = firstName.substr(0, firstName.rfind('.'));
	if (!DupeStreamRepair::IsStreamEligible(innerName.c_str()))
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
		if (!nzbget_rs_src_content_map_can_build_bare_or_split_member(
			source != nullptr ? 1 : 0, source ? source->Size() : 0))
		{
			// every member's size shapes the offsets after it - unlike rar
			// volumes, a split set with an unreadable member cannot map at all
			skipReason = std::string("member ") + members[memberIndex].Name + " unreadable";
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
		if (source && volume->GetEncrypted())
		{
			skipReason = SkipEncryptedHeaders;
			return nullptr;
		}
		if (unknownVolume >= 0)
		{
			skipReason = "unreadable headers in more than one volume";
			return nullptr;
		}
		unknownVolume = (int)i;
	}

	// the primary inner file: largest entry of the first readable volume
	RarFile* primary = nullptr;
	for (std::unique_ptr<RarVolume>& volume : volumes)
	{
		if (!volume)
		{
			continue;
		}
		for (RarFile& innerFile : *volume->GetFiles())
		{
			if (!primary || innerFile.GetSize() > primary->GetSize())
			{
				primary = &innerFile;
			}
		}
		break;
	}
	if (!primary)
	{
		skipReason = "no file entries in the first readable volume";
		return nullptr;
	}

	std::string innerName = FileSystem::BaseFileName(primary->GetFilename());
	int64 innerSize = primary->GetSize();
	if (!DupeStreamRepair::IsStreamEligible(innerName.c_str()))
	{
		skipReason = "inner file is not a media file";
		return nullptr;
	}

	struct VolumeRun
	{
		int64 PackedSize = 0;
		int64 DataOffset = -1;	// -1 = no mappable data in this volume
		int Version = 0;		// rar version of the carrying volume (3 or 5)
		bool Encrypted = false;	// this volume's primary entry is data-encrypted
		bool HasSalt = false;	// rar3 per-file data salt present
		uint8 Salt[8] = {};
		bool HasCrypt = false;	// rar5 crypt record present
		RarFile::Rar5Crypt Crypt;
	};
	std::vector<VolumeRun> runs(set.Members.size());
	int64 knownPacked = 0;
	bool anyEncrypted = false;	// at least one primary entry is data-encrypted
	bool anyPlain = false;		// at least one primary entry is NOT encrypted

	for (size_t i = 0; i < set.Members.size(); i++)
	{
		if (!volumes[i])
		{
			continue;
		}
		for (RarFile& innerFile : *volumes[i]->GetFiles())
		{
			if (strcasecmp(FileSystem::BaseFileName(innerFile.GetFilename()),
				innerName.c_str()))
			{
				continue;	// other inner files (nfo, srt) stay unmapped
			}
			if (!innerFile.GetStored())
			{
				skipReason = "compressed archive (only store mode is mappable)";
				return nullptr;
			}
			if (innerFile.GetEncryptedData())
			{
				// no password => M2 behavior: leave it for par2
				if (!password)
				{
					skipReason = SkipEncryptedData;
					return nullptr;
				}
				anyEncrypted = true;
				runs[i].Encrypted = true;
				runs[i].Version = volumes[i]->GetVersion();
				if (innerFile.GetHasSalt())
				{
					runs[i].HasSalt = true;
					memcpy(runs[i].Salt, innerFile.GetSalt(), sizeof(runs[i].Salt));
				}
				if (const RarFile::Rar5Crypt* crypt = innerFile.GetCrypt())
				{
					runs[i].HasCrypt = true;
					runs[i].Crypt = *crypt;
				}
			}
			else
			{
				anyPlain = true;
			}
			if (innerFile.GetSize() != innerSize)
			{
				skipReason = "inconsistent inner file size across volumes";
				return nullptr;
			}
			ContentSource* source = sources.GetSource(set.Members[i]);
			if (!nzbget_rs_src_content_map_rar_data_run_geometry_valid(
				innerFile.GetDataOffset(), innerFile.GetPackedSize(),
				source != nullptr ? 1 : 0, source ? source->Size() : 0))
			{
				skipReason = "implausible data run geometry";
				return nullptr;
			}
			runs[i].PackedSize = innerFile.GetPackedSize();
			runs[i].DataOffset = innerFile.GetDataOffset();
			knownPacked += innerFile.GetPackedSize();
		}
	}

	// encrypted store-rar: the packed bytes are ciphertext, so the plain
	// "sum of packed == inner size" gate does not apply - build the crypto
	// geometry instead. Verified against real WinRAR encrypted testdata: an
	// encrypted RAR file is ONE continuous AES-CBC stream that RAR splits across
	// volumes at ARBITRARY byte offsets (NOT re-keyed/16-aligned per volume).
	// The volumes' data areas therefore concatenate to one contiguous cipher
	// space of exactly ceil16(innerSize) bytes; the map records each volume's
	// slab (RunCrypto.CipherSize) so readers can assemble 16-byte blocks across
	// member boundaries. Reassembly uses ONE shared context (all volumes share
	// the file's key and chain-start IV); block 0 chains from the header IV,
	// every later block from its predecessor ciphertext block.
	if (anyEncrypted)
	{
		if (anyPlain)
		{
			// the same inner file cannot be encrypted in one volume and clear in
			// another; a mixed set is malformed for our continuous-stream model
			skipReason = "encrypted volume geometry does not fit store mode";
			return nullptr;
		}
		if (unknownVolume >= 0)
		{
			// an unreadable volume yields no crypto params or cipher offset for
			// its slice: we cannot decrypt around it, so fail closed
			skipReason = "encrypted archive without crypto parameters";
			return nullptr;
		}

		// volumes carrying the primary, in data order
		std::vector<int> encVols;
		for (size_t i = 0; i < set.Members.size(); i++)
		{
			if (runs[i].DataOffset >= 0 && runs[i].Encrypted)
			{
				encVols.push_back((int)i);
			}
		}
		if (encVols.empty())
		{
			skipReason = "encrypted volume geometry does not fit store mode";
			return nullptr;
		}

		// one shared crypto context from the first carrying volume's parameters;
		// every volume must agree (same key) for the continuous stream to hold
		int first = encVols[0];
		int version = runs[first].Version;
		std::shared_ptr<RarCryptoContext> crypto;
		if (version == 3)
		{
			if (!runs[first].HasSalt)
			{
				skipReason = "encrypted archive without crypto parameters";
				return nullptr;
			}
			crypto = RarCryptoContext::MakeRar3(password, runs[first].Salt);
			if (!crypto)
			{
				// rar3 has no password check: a null here means empty password
				// or no TLS, i.e. no usable key material
				skipReason = "encrypted archive without crypto parameters";
				return nullptr;
			}
		}
		else if (version == 5)
		{
			if (!runs[first].HasCrypt)
			{
				skipReason = "encrypted archive without crypto parameters";
				return nullptr;
			}
			if (runs[first].Crypt.Version != 0)	// only AES-256 (crypt version 0)
			{
				skipReason = "encrypted archive without crypto parameters";
				return nullptr;
			}
			crypto = RarCryptoContext::MakeRar5(password, runs[first].Crypt);
			if (!crypto)
			{
				// rar5 verifies the password against the stored check value
				skipReason = "archive password rejected";
				return nullptr;
			}
		}
		else
		{
			skipReason = "encrypted archive without crypto parameters";
			return nullptr;
		}

		// cross-volume parameter agreement: a differing salt/crypt would mean a
		// different key, breaking the one-stream assumption
		for (size_t k = 1; k < encVols.size(); k++)
		{
			const VolumeRun& vr = runs[encVols[k]];
			if (vr.Version != version)
			{
				skipReason = "encrypted volume geometry does not fit store mode";
				return nullptr;
			}
			if (version == 3 &&
				(!vr.HasSalt || memcmp(vr.Salt, runs[first].Salt, sizeof(vr.Salt))))
			{
				skipReason = "encrypted volume geometry does not fit store mode";
				return nullptr;
			}
			if (version == 5 &&
				(!vr.HasCrypt ||
					vr.Crypt.Version != runs[first].Crypt.Version ||
					vr.Crypt.KdfCount != runs[first].Crypt.KdfCount ||
					memcmp(vr.Crypt.Salt, runs[first].Crypt.Salt, 16) ||
					memcmp(vr.Crypt.Iv, runs[first].Crypt.Iv, 16)))
			{
				skipReason = "encrypted volume geometry does not fit store mode";
				return nullptr;
			}
		}

		// contiguous cipher-space geometry: volume cuts are arbitrary, but the
		// data areas must concatenate to exactly ceil16(innerSize) cipher bytes.
		// The gate is phrased with subtractions bounded by the source-checked
		// packed sizes so a lying innerSize cannot overflow the arithmetic.
		int64 cipherTotal = 0;
		for (int v : encVols)
		{
			if (runs[v].PackedSize <= 0)
			{
				skipReason = "encrypted volume geometry does not fit store mode";
				return nullptr;
			}
			cipherTotal += runs[v].PackedSize;
		}
		if (!nzbget_rs_cdn_content_mapper_rar_cipher_geometry_valid(innerSize, cipherTotal))
		{
			skipReason = "encrypted volume geometry does not fit store mode";
			return nullptr;
		}

		std::unique_ptr<ContentMap> map = std::make_unique<ContentMap>();
		map->SetInnerName(innerName.c_str());
		map->SetInnerSize(innerSize);

		int64 cipherBase = 0;
		for (int v : encVols)
		{
			// plaintext position p == cipher position p; the run covers this
			// volume's plaintext bytes, and only the LAST volume's slab may
			// run past them (the CBC padding tail carries no plaintext)
			int64 plainChunk = nzbget_rs_cdn_content_mapper_rar_calc_plain_chunk(
				runs[v].PackedSize, innerSize, cipherBase);
			if (plainChunk <= 0)
			{
				// a padding-only volume: no real RAR splits inside the padding
				skipReason = "encrypted volume geometry does not fit store mode";
				return nullptr;
			}
			map->GetRuns()->push_back({cipherBase, set.Members[v],
				runs[v].DataOffset, plainChunk});
			// all runs share the one stream context; the slab is CipherSize
			// ciphertext bytes at member offset CipherDataOffset, covering
			// cipher-space positions [InnerOffset, InnerOffset + CipherSize)
			map->GetRunCryptos()->push_back({crypto, runs[v].DataOffset,
				runs[v].PackedSize});
			cipherBase += runs[v].PackedSize;
		}
		return map;
	}

	// store mode means the packed bytes ARE the inner bytes: exact sum
	// required (this is the strong gate even if a method byte lied)
	int64 inferredPacked = 0;
	int inferStatus = nzbget_rs_cdn_content_mapper_rar_infer_unknown_packed(
		unknownVolume, innerSize, knownPacked, &inferredPacked);
	if (inferStatus == 1)
	{
		skipReason = "packed sizes exceed the inner file size";
		return nullptr;
	}
	if (inferStatus == 2)
	{
		skipReason = "packed sizes do not sum to the inner file size";
		return nullptr;
	}
	if (unknownVolume >= 0)
	{
		runs[unknownVolume].PackedSize = inferredPacked;
		runs[unknownVolume].DataOffset = -1;
	}

	std::unique_ptr<ContentMap> map = std::make_unique<ContentMap>();
	map->SetInnerName(innerName.c_str());
	map->SetInnerSize(innerSize);

	int64 innerOffset = 0;
	for (size_t i = 0; i < set.Members.size(); i++)
	{
		if (nzbget_rs_src_content_map_rar_can_emit_plain_run(
			runs[i].DataOffset, runs[i].PackedSize))
		{
			map->GetRuns()->push_back({innerOffset, set.Members[i],
				runs[i].DataOffset, runs[i].PackedSize});
		}
		innerOffset += runs[i].PackedSize;
	}

	return map;
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
			for (int memberIndex : set.Members)
			{
				for (const StreamRange& hole : memberHoles[memberIndex])
				{
					for (const StreamRange& innerHole : data.Map->MapToInner(memberIndex, hole))
					{
						data.InnerHoles.push_back(innerHole);
					}
				}
			}
			std::sort(data.InnerHoles.begin(), data.InnerHoles.end(),
				[](const StreamRange& range1, const StreamRange& range2)
				{
					return range1.Offset < range2.Offset;
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
