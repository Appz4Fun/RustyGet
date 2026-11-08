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


#ifndef CONTENTMAP_H
#define CONTENTMAP_H

#include <memory>
#include <string>
#include <vector>
#include "DownloadInfo.h"
#include "FileSystem.h"

/*
 * Cross-packing ("container-aware") stream repair for option
 * <DupeArticleFallback> value "stream": a ContentMap locates the canonical
 * inner content stream (the media file's bytes) inside a posting's member
 * files, so two postings that pack the SAME inner file differently (bare
 * file, store-mode rar volumes, stored zip, 7z-copy, raw splits) can donate
 * to each other. Maps are built by parsing container headers through the
 * ContentSource byte-reader interface - the donor/target symmetry point:
 * the target reads assembled files on disk, the donor fetches articles on
 * demand. Recovery still never unpacks, decompresses or decrypts anything:
 * only store/copy-mode packings are mappable, identity is proven by probe
 * byte-compares before any write, and everything unmappable stays for par2.
 */

/* all-or-nothing absolute-offset byte access to one member file;
 * false = the bytes are unavailable (target: inside a captured hole;
 * donor: article not fetchable) */
class ContentSource
{
public:
	virtual ~ContentSource() {}
	virtual int64 Size() = 0;
	virtual bool Read(int64 offset, void* buffer, int64 size) = 0;
};

/* the member files of one posting, by index into its SetMember list */
class ContentSourceSet
{
public:
	virtual ~ContentSourceSet() {}
	virtual ContentSource* GetSource(int memberIndex) = 0;
};

/* reads an already-open disk file (the target side) */
class DiskContentSource : public ContentSource
{
public:
	DiskContentSource(DiskFile& file, int64 size) : m_file(file), m_size(size) {}
	virtual int64 Size() { return m_size; }
	virtual bool Read(int64 offset, void* buffer, int64 size);

private:
	DiskFile& m_file;
	int64 m_size;
};

/* a member file of a posting; Size 0 = not known yet (donor side before
 * the first article of that member was fetched) */
struct SetMember
{
	std::string Name;
	int64 Size = 0;
};

/* member files grouped into one container set, in data order */
struct MemberSet
{
	enum EFormat
	{
		mfBare,		// a directly posted media file (identity map)
		mfSplit,	// raw splits: name.ext.001, .002, ...
		mfRar,		// rar volumes: .partNN.rar or .rar/.rNN
		mfZip,		// zip, incl. spanned .z01... + final .zip
		mfSevenZip	// 7z, incl. .7z.001 splits
	};

	EFormat Format;
	std::vector<int> Members;
};

/* one contiguous piece of the inner stream inside a member file */
struct ContentRun
{
	int64 InnerOffset;
	int MemberIndex;
	int64 MemberOffset;
	int64 Size;
	int64 InnerEnd() const { return InnerOffset + Size; }
};

class RarCryptoContext;	// StreamCrypto.h - held by pointer only here

/* crypto annotation for a run whose member bytes are RAR-encrypted ciphertext
 * (password-assisted store-rar mapping, M3). The run lives in PLAINTEXT inner
 * space; its member bytes are the ciphertext. For an encrypted RAR the whole
 * inner file is ONE continuous AES-CBC stream that RAR splits across volumes
 * at ARBITRARY byte offsets (verified: per-volume cipher chunks are unpadded
 * and commonly non-16-aligned, only their total is padded to ceil16 of the
 * plaintext size; no per-volume re-keying), so every encrypted run of a map
 * shares ONE context. The member data areas concatenate to one contiguous
 * CIPHER SPACE in which position p carries the ciphertext byte of plaintext
 * position p: this run's slab covers cipher positions [run.InnerOffset,
 * run.InnerOffset + CipherSize) at member offsets [CipherDataOffset,
 * CipherDataOffset + CipherSize). CipherSize can exceed run.Size only in the
 * final slab (the CBC padding tail past the plaintext end). Cipher block 0
 * chains from the context's header IV, every later block from its predecessor
 * ciphertext block - which may live in the PREVIOUS member. */
struct RunCrypto
{
	std::shared_ptr<RarCryptoContext> Crypto;
	int64 CipherDataOffset = 0;
	int64 CipherSize = 0;
};

struct MemberRange
{
	int MemberIndex;
	StreamRange Range;
};

/* wraps another source, refusing reads that intersect captured holes: a
 * hole's bytes exist on disk (preallocated) but are garbage, and parsing
 * them would build maps out of noise */
class HoledSource : public ContentSource
{
public:
	HoledSource(ContentSource& inner, StreamRangeList holes) :
		m_inner(inner), m_holes(std::move(holes)) {}
	virtual int64 Size() { return m_inner.Size(); }
	virtual bool Read(int64 offset, void* buffer, int64 size);

private:
	ContentSource& m_inner;
	StreamRangeList m_holes;
};

/* per-member hole-aware view over another source set */
class HoledSourceSet : public ContentSourceSet
{
public:
	HoledSourceSet(ContentSourceSet& inner, const std::vector<StreamRangeList>& memberHoles) :
		m_inner(inner), m_memberHoles(memberHoles), m_wrapped(memberHoles.size()) {}
	virtual ContentSource* GetSource(int memberIndex);

private:
	ContentSourceSet& m_inner;
	const std::vector<StreamRangeList>& m_memberHoles;
	std::vector<std::unique_ptr<HoledSource>> m_wrapped;
};

/* where the inner content stream lives inside a member set */
class ContentMap
{
public:
	const char* GetInnerName() const { return m_innerName.c_str(); }
	void SetInnerName(const char* innerName) { m_innerName = innerName ? innerName : ""; }
	void SetInnerName(std::string innerName) { m_innerName = std::move(innerName); }
	int64 GetInnerSize() const { return m_innerSize; }
	void SetInnerSize(int64 innerSize) { m_innerSize = innerSize; }
	std::vector<ContentRun>* GetRuns() { return &m_runs; }

	/* crypto annotations, kept parallel to GetRuns() (index-for-index) when the
	 * map is encrypted, empty otherwise. The builder pushes one entry per run. */
	std::vector<RunCrypto>* GetRunCryptos() { return &m_runCryptos; }
	/* the crypto binding for a run, or nullptr for a plaintext run: the surface
	 * Task 4 decrypts holes through. Encrypted maps annotate every run. */
	const RunCrypto* GetRunCrypto(size_t runIndex) const
	{
		return runIndex < m_runCryptos.size() && m_runCryptos[runIndex].Crypto ?
			&m_runCryptos[runIndex] : nullptr;
	}
	/* true when any run carries ciphertext (a password-assisted store-rar map) */
	bool GetEncrypted() const { return !m_runCryptos.empty(); }
	/* total bytes of the contiguous cipher space (== ceil16(InnerSize) for a
	 * complete encrypted map), 0 for plaintext maps */
	int64 GetCipherStreamSize() const;

	/* where cipherRange (positions in the contiguous cipher space spanning all
	 * member data areas) lives in member coordinates. Fail closed: the result is
	 * empty unless every byte is covered exactly once, in order - a range
	 * touching an excluded member or the padding of a foreshortened map cannot
	 * be trusted for crypto work. Empty for plaintext maps. */
	std::vector<MemberRange> MapCipherRange(const StreamRange& cipherRange) const;

	/* assembles the raw ciphertext of cipherRange from the member sources
	 * (cross-member 16-byte blocks are the norm: RAR cuts volumes mid-block);
	 * false when any byte is unmappable or unreadable */
	bool ReadCipherRange(ContentSourceSet& sources, const StreamRange& cipherRange,
		char* buffer) const;

	/* reads innerRange of the PLAINTEXT inner stream by fetching the covering
	 * whole cipher blocks (plus the predecessor block as chain input; the
	 * header IV seeds block 0) and CBC-decrypting them. false when any needed
	 * cipher byte is unavailable - the caller must treat the range as missing */
	bool ReadInnerDecrypted(ContentSourceSet& sources, const StreamRange& innerRange,
		char* buffer) const;

	/* the parts of memberRange that carry inner bytes, in inner coordinates
	 * (framing inside memberRange drops out - donor-irreparable by design) */
	StreamRangeList MapToInner(int memberIndex, const StreamRange& memberRange) const;

	/* where innerRange lives in member coordinates, split across runs */
	std::vector<MemberRange> MapFromInner(const StreamRange& innerRange) const;

private:
	std::string m_innerName;
	int64 m_innerSize = 0;
	std::vector<ContentRun> m_runs;
	// parallel to m_runs when encrypted (one entry per run), empty when plain
	std::vector<RunCrypto> m_runCryptos;
};

/* one target-side set the repair stage can work on; Map is null exactly
 * when SkipReason names why the set cannot cross-map (par2 still applies) */
struct RepairSetData
{
	MemberSet Set;
	std::unique_ptr<ContentMap> Map;
	StreamRangeList InnerHoles;
	// InnerHoles as captured at build time: InnerHoles shrinks as donors
	// patch, but identity evidence must anchor only to bytes that were no
	// longer holes when the set was built - primary-downloaded or already
	// probe-verified M1 writes - so probe placement excludes these
	StreamRangeList OriginalInnerHoles;
	std::string SkipReason;
};

class ContentMapper
{
public:
	/* M3-eligible skip reasons: a set that skipped for one of these was blocked
	 * ONLY by encryption, so the controller may retry BuildMap with a password
	 * (the M3 ladder). Defined here - where BuildRarMap SETS them - and matched
	 * by StreamRepair, so a future reword cannot silently disable donor-side M3. */
	static constexpr const char* SkipEncryptedData = "encrypted archive data";
	static constexpr const char* SkipEncryptedHeaders = "encrypted archive headers";

	/* M4 (option <DupeStreamDecompress>): true when set's format is one an
	 * extractor (unrar/7z) can decompress (mfRar, mfZip, mfSevenZip) - a
	 * candidate for the decompression-donor path. False for mfBare/mfSplit:
	 * there is no compression to undo, the store-mode paths above already
	 * handle them. */
	static bool IsCompressibleArchive(const MemberSet& set);

	/* groups member files into container sets by their naming schemes;
	 * incomplete sets (numbering gaps) are dropped - their members stay
	 * un-mapped and par2 owns them. Bare singletons require a media
	 * extension (DupeStreamRepair::IsStreamEligible) or no extension at all
	 * (an obfuscated name). */
	static std::vector<MemberSet> GroupSets(const std::vector<SetMember>& members);

	/* builds the inner-content map for one set; nullptr + skipReason when
	 * the set is not store/copy-mappable (M1 and par2 still apply). A non-null
	 * password enables password-assisted mapping of encrypted store-rar archives
	 * (threaded to BuildRarMap only); without it encrypted archives skip as in
	 * M2. Donor call sites supply the donor's password; the target side threads
	 * its own through BuildRepairSets. */
	static std::unique_ptr<ContentMap> BuildMap(const std::vector<SetMember>& members,
		const MemberSet& set, ContentSourceSet& sources, std::string& skipReason,
		const char* password = nullptr);

	/* target side: group members into sets, keep those with holed members,
	 * build maps through (hole-aware) sources and translate member holes to
	 * inner coordinates. Framing holes and holes of excluded members drop
	 * out here - par2 owns them. A non-null password (the TARGET's own) lets
	 * encrypted store-rar target sets map; their runs carry RunCrypto. */
	static std::vector<RepairSetData> BuildRepairSets(const std::vector<SetMember>& members,
		const std::vector<StreamRangeList>& memberHoles, ContentSourceSet& sources,
		const char* password = nullptr);
};

#include <exception>
#include "Log.h"
#include "StreamCrypto.h"
#include "nzbget_rs.h"

namespace ContentMapDetail
{
	template <typename Ctx, typename R, typename F>
	inline R MapCall(void* rawCtx, R failed, F f) noexcept
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
	inline void MapCallVoid(void* rawCtx, F f) noexcept
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

	inline void LogDebugRarVolume(const char* filename, int version, bool multiVolume,
		uint32 volumeNo, bool newNaming, bool hasNextVolume, bool encrypted,
		size_t fileCount, const uint32* fileTimes, const int64* fileSizes,
		const uint32* fileAttrs, const uint8* fileSplitBefore,
		const uint8* fileSplitAfter, const char* const* fileNames)
	{
#ifdef DEBUG
		debug("Volume: version:%i, multi:%i, vol-no:%i, new-naming:%i, has-next:%i, encrypted:%i, file-count:%i, [%s]",
			(int)version, (int)multiVolume, volumeNo, (int)newNaming, (int)hasNextVolume,
			(int)encrypted, (int)fileCount, FileSystem::BaseFileName(filename ? filename : ""));

		for (size_t i = 0; i < fileCount; i++)
		{
			debug("  time:%i, size:%" PRIi64 ", attr:%i, split-before:%i, split-after:%i, [%s]",
				fileTimes ? fileTimes[i] : 0, fileSizes ? fileSizes[i] : (int64)0, fileAttrs ? fileAttrs[i] : 0,
				(int)(fileSplitBefore && fileSplitBefore[i] != 0),
				(int)(fileSplitAfter && fileSplitAfter[i] != 0),
				(fileNames && fileNames[i]) ? fileNames[i] : "");
		}
#else
		(void)filename;
		(void)version;
		(void)multiVolume;
		(void)volumeNo;
		(void)newNaming;
		(void)hasNextVolume;
		(void)encrypted;
		(void)fileCount;
		(void)fileTimes;
		(void)fileSizes;
		(void)fileAttrs;
		(void)fileSplitBefore;
		(void)fileSplitAfter;
		(void)fileNames;
#endif
	}
}

inline bool DiskContentSource::Read(int64 offset, void* buffer, int64 size)
{
	return offset >= 0 && size >= 0 && offset <= m_size && size <= m_size - offset &&
		m_file.Seek(offset) && m_file.Read(buffer, size) == size;
}

inline bool HoledSource::Read(int64 offset, void* buffer, int64 size)
{
	if (nzbget_rs_holed_source_intersects(
		reinterpret_cast<const NzbgetRsByteRange*>(m_holes.data()),
		m_holes.size(), offset, size) != 0)
	{
		return false;
	}
	return m_inner.Read(offset, buffer, size);
}

inline ContentSource* HoledSourceSet::GetSource(int memberIndex)
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

inline StreamRangeList ContentMap::MapToInner(int memberIndex, const StreamRange& memberRange) const
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

inline std::vector<MemberRange> ContentMap::MapFromInner(const StreamRange& innerRange) const
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

inline int64 ContentMap::GetCipherStreamSize() const
{
	std::vector<int64_t> cipherSizes;
	cipherSizes.reserve(m_runCryptos.size());
	for (const RunCrypto& runCrypto : m_runCryptos)
	{
		cipherSizes.push_back(runCrypto.CipherSize);
	}
	return nzbget_rs_stream_content_map_sum_cipher_sizes(cipherSizes.data(), cipherSizes.size());
}

inline std::vector<MemberRange> ContentMap::MapCipherRange(const StreamRange& cipherRange) const
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

inline bool ContentMap::ReadCipherRange(ContentSourceSet& sources, const StreamRange& cipherRange,
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

inline bool ContentMap::ReadInnerDecrypted(ContentSourceSet& sources, const StreamRange& innerRange,
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
			return ContentMapDetail::MapCall<DecryptCtx>(rawCtx, 0, [memberIndex](DecryptCtx* c)
			{
				return c->Sources->GetSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return ContentMapDetail::MapCall<DecryptCtx>(rawCtx, static_cast<int64_t>(0), [memberIndex](DecryptCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return ContentMapDetail::MapCall<DecryptCtx>(rawCtx, 0, [=](DecryptCtx* c)
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

inline bool ContentMapper::IsCompressibleArchive(const MemberSet& set)
{
	return nzbget_rs_content_mapper_is_compressible_archive(
		static_cast<int>(set.Format), static_cast<int>(MemberSet::mfRar),
		static_cast<int>(MemberSet::mfZip), static_cast<int>(MemberSet::mfSevenZip)) != 0;
}

inline std::vector<MemberSet> ContentMapper::GroupSets(const std::vector<SetMember>& members)
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
			ContentMapDetail::MapCallVoid<GroupCtx>(rawCtx, [=](GroupCtx* c)
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

inline std::unique_ptr<ContentMap> ContentMapper::BuildMap(const std::vector<SetMember>& members,
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
			return ContentMapDetail::MapCall<BuildMapCtx>(rawCtx, -1, [memberIndex](BuildMapCtx* c)
			{
				if (memberIndex < 0) return 0;
				return c->Sources->GetSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return ContentMapDetail::MapCall<BuildMapCtx>(rawCtx, static_cast<int64_t>(-1), [memberIndex](BuildMapCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return ContentMapDetail::MapCall<BuildMapCtx>(rawCtx, 0, [=](BuildMapCtx* c)
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
			ContentMapDetail::MapCallVoid<BuildMapCtx>(rawCtx, [=](BuildMapCtx*)
			{
				ContentMapDetail::LogDebugRarVolume(filename, version, multiVolume != 0, volumeNo,
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
			ContentMapDetail::MapCallVoid<BuildMapCtx>(rawCtx, [=](BuildMapCtx* c)
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
			ContentMapDetail::MapCallVoid<BuildMapCtx>(rawCtx, [reason](BuildMapCtx* c)
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

inline std::vector<RepairSetData> ContentMapper::BuildRepairSets(const std::vector<SetMember>& members,
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
			return ContentMapDetail::MapCall<BuildRepairSetsCtx>(rawCtx, -1, [memberIndex](BuildRepairSetsCtx* c)
			{
				if (memberIndex < 0) return 0;
				return c->Sources->GetSource(memberIndex) != nullptr ? 1 : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex) -> int64_t
		{
			return ContentMapDetail::MapCall<BuildRepairSetsCtx>(rawCtx, static_cast<int64_t>(-1), [memberIndex](BuildRepairSetsCtx* c)
			{
				ContentSource* src = c->Sources->GetSource(memberIndex);
				return src ? src->Size() : 0;
			});
		},
		[](void* rawCtx, int32_t memberIndex, int64_t offset, uint8_t* buf, int64_t size) -> int
		{
			return ContentMapDetail::MapCall<BuildRepairSetsCtx>(rawCtx, 0, [=](BuildRepairSetsCtx* c)
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
			ContentMapDetail::MapCallVoid<BuildRepairSetsCtx>(rawCtx, [=](BuildRepairSetsCtx*)
			{
				ContentMapDetail::LogDebugRarVolume(filename, version, multiVolume != 0, volumeNo,
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
			ContentMapDetail::MapCallVoid<BuildRepairSetsCtx>(rawCtx, [=](BuildRepairSetsCtx* c)
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

#endif
