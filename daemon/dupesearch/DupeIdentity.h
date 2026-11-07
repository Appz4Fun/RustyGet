#ifndef DUPEIDENTITY_H
#define DUPEIDENTITY_H

#include <atomic>
#include <ctime>
#include <string>
#include <thread>
#include <vector>
#include <functional>
#include "ArticleFetcher.h"
#include "DonorHealth.h"
#include "HttpGet.h"
#include "NzbReader.h"
#include "ReleaseName.h"
#include "Util.h"
#include "nzbget_rs.h"

// Rust decides identity; the daemon owns article transport and queue objects.
class DupeIdentity
{
public:
	struct Identity
	{
		std::string name;
		std::string extension;
		unsigned long long bytes = 0;
		explicit operator bool() const { return bytes != 0 && !extension.empty(); }
	};
	using FetchFunc = std::function<ArticleFetcher::FetchedArticle(const std::string&, const std::vector<CString>&)>;

	static Identity FromArticle(const std::vector<char>& data, const std::string& name,
		unsigned long long total, unsigned long long minimum)
	{
		Identity identity;
		NzbgetRsBuf raw = nzbget_rs_dupe_inner(data.data(), data.size(), name.data(), name.size(), total, minimum);
		uint64_t bytes = 0;
		size_t extensionSize = 0;
		size_t nameOffset = 0;
		size_t nameLen = 0;
		if (nzbget_rs_dupe_identity_decode_inner_buf(
			raw.data, raw.len, &bytes, &extensionSize, &nameOffset, &nameLen) != 0)
		{
			identity.bytes = bytes;
			identity.extension.assign(raw.data + 12, extensionSize);
			identity.name.assign(raw.data + nameOffset, nameLen);
		}
		nzbget_rs_free(raw);
		return identity;
	}

	static ArticleFetcher::FetchedArticle FetchArticle(const NzbSummary::Head& head,
		const std::vector<std::string>& names, time_t deadline, const FetchFunc& fetch = FetchFunc())
	{
		struct FetchCtx
		{
			const std::vector<std::string>* names;
			const FetchFunc* fetch;
			ArticleFetcher fetcher;
			ArticleFetcher::FetchedArticle article;
		} ctx{&names, &fetch, {}, {}};

		int ok = nzbget_rs_dupe_identity_fetch_article(
			head.messageId.data(), head.messageId.size(), (int64_t)deadline, &ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void* raw, const char* bracketedId, size_t bracketedLen,
			   int* outSuccess, int64_t* outOffset, int64_t* outFileSize) -> int {
				auto* c = static_cast<FetchCtx*>(raw);
				std::vector<CString> groups;
				for (const std::string& name : *c->names) groups.emplace_back(name.c_str());
				std::string id(bracketedId ? bracketedId : "", bracketedLen);
				try
				{
					c->article = (*c->fetch) ? (*c->fetch)(id, groups) : c->fetcher.Fetch(id.c_str(), groups);
				}
				catch (...)
				{
					c->article = ArticleFetcher::FetchedArticle();
					return 0;
				}
				*outSuccess = c->article.Success ? 1 : 0;
				*outOffset = c->article.Offset;
				*outFileSize = c->article.FileSize;
				return 1;
			},
			[](void* raw) {
				static_cast<FetchCtx*>(raw)->fetcher.Stop();
			});
		if (!ok)
			return ArticleFetcher::FetchedArticle();
		return ctx.article;
	}

	static Identity Read(const NzbSummary& info, time_t deadline, const FetchFunc& fetch = FetchFunc())
	{
		std::vector<const char*> headNames(info.heads.size());
		std::vector<size_t> headNameLens(info.heads.size());
		std::vector<const char*> headMsgIds(info.heads.size());
		std::vector<size_t> headMsgIdLens(info.heads.size());
		std::vector<int64_t> headBytes(info.heads.size());
		for (size_t i = 0; i < info.heads.size(); ++i)
		{
			headNames[i] = info.heads[i].name.data();
			headNameLens[i] = info.heads[i].name.size();
			headMsgIds[i] = info.heads[i].messageId.data();
			headMsgIdLens[i] = info.heads[i].messageId.size();
			headBytes[i] = info.heads[i].bytes;
		}
		struct ReadCtx
		{
			const NzbSummary* info;
			const FetchFunc* fetch;
			ArticleFetcher::FetchedArticle lastArticle;
		} ctx{&info, &fetch, {}};

		NzbgetRsBuf nameBuf{nullptr, 0, 0};
		NzbgetRsBuf extBuf{nullptr, 0, 0};
		uint64_t bytes = 0;
		int found = nzbget_rs_dupe_identity_read(
			info.dataBytes,
			headNames.empty() ? nullptr : headNames.data(),
			headNameLens.empty() ? nullptr : headNameLens.data(),
			headMsgIds.empty() ? nullptr : headMsgIds.data(),
			headMsgIdLens.empty() ? nullptr : headMsgIdLens.data(),
			headBytes.empty() ? nullptr : headBytes.data(),
			info.heads.size(), (int64_t)deadline, &ctx,
			[](void*) -> int64_t { return (int64_t)Util::CurrentTime(); },
			[](void*) -> int { return HttpGet::Stopped() ? 1 : 0; },
			[](void* raw, const char* msgId, size_t msgIdLen, int64_t dl,
			   int* outSuccess, int64_t* outFileSize,
			   const char** outDataPtr, size_t* outDataLen) {
				auto* c = static_cast<ReadCtx*>(raw);
				NzbSummary::Head head;
				head.messageId.assign(msgId ? msgId : "", msgIdLen);
				c->lastArticle = FetchArticle(head, c->info->groups, (time_t)dl, *c->fetch);
				*outSuccess = c->lastArticle.Success ? 1 : 0;
				*outFileSize = c->lastArticle.FileSize;
				*outDataPtr = c->lastArticle.Data.empty() ? nullptr : c->lastArticle.Data.data();
				*outDataLen = c->lastArticle.Data.size();
			},
			&nameBuf, &extBuf, &bytes);
		Identity identity;
		if (found)
		{
			if (nameBuf.data && nameBuf.len > 0) identity.name.assign(nameBuf.data, nameBuf.len);
			if (extBuf.data && extBuf.len > 0) identity.extension.assign(extBuf.data, extBuf.len);
			identity.bytes = bytes;
		}
		nzbget_rs_free(nameBuf);
		nzbget_rs_free(extBuf);
		return identity;
	}

	static unsigned long long WholeBytes(const NzbSummary& info)
	{
		if (info.totalBytes <= 0) return 0;
		return nzbget_rs_dupe_whole_bytes((uint64_t)info.totalBytes, info.listedParts, info.declaredParts);
	}

	static double Alive(const DonorHealth::Health& health, const NzbSummary& info)
	{
		return nzbget_rs_dupe_effective_alive(health.present, health.missing, health.error, info.listedParts, info.declaredParts);
	}

	static bool IsDead(const DonorHealth::Health& health, const NzbSummary& info, int minimum)
	{
		return nzbget_rs_dupe_is_dead(health.present, health.missing, health.error, info.listedParts, info.declaredParts, minimum) != 0;
	}

	static int UnlistedMissing(const DonorHealth::Health& health, const NzbSummary& info)
	{
		return nzbget_rs_dupe_unlisted_missing(health.Answered(), info.listedParts, info.declaredParts);
	}

	static bool ParDoomed(const NzbSummary& info, double alive, time_t deadline)
	{
		if (info.parIndex.messageId.empty() || info.recoveryBytes.empty() || info.dataBytes <= 0 || info.articleBytes == 0) return false;
		auto article = FetchArticle(info.parIndex, info.groups, deadline);
		if (!article.Success) return false;
		uint64_t block = nzbget_rs_dupe_par_block(article.Data.data(), article.Data.size());
		return nzbget_rs_dupe_par_doomed((uint64_t)info.dataBytes, info.articleBytes,
			info.recoveryBytes.data(), info.recoveryBytes.size(), alive, block) != 0;
	}

	static bool OuterCompatible(const std::string& title, const std::string& name)
	{
		return nzbget_rs_dupe_outer_ok(title.data(), title.size(), name.data(), name.size()) != 0;
	}

	static bool SameData(const NzbSummary& a, const NzbSummary& b)
	{
		if (a.dataBytes <= 0 || b.dataBytes <= 0 || a.dataFiles <= 0 || b.dataFiles <= 0) return false;
		return nzbget_rs_dupe_same_data((uint64_t)a.dataBytes, (uint32_t)a.dataFiles,
			(uint64_t)b.dataBytes, (uint32_t)b.dataFiles) != 0;
	}

	static bool Candidate(const std::string& title, long long bytes, const std::string& name, long long size)
	{
		return nzbget_rs_dupe_candidate(title.data(), title.size(), bytes > 0 ? bytes : 0,
			name.data(), name.size(), size > 0 ? size : 0) != 0;
	}

	static bool Matches(const std::string& title, const std::string& mainName,
		const Identity& mine, const Identity& theirs)
	{
		NzbgetRsDupeIdentityInput a{ mine.bytes, mine.name.data(), mine.name.size(), mine.extension.data(), mine.extension.size() };
		NzbgetRsDupeIdentityInput b{ theirs.bytes, theirs.name.data(), theirs.name.size(), theirs.extension.data(), theirs.extension.size() };
		return nzbget_rs_dupe_lenient_ok(title.data(), title.size(), mainName.data(), mainName.size(),
			mine ? &a : nullptr, theirs ? &b : nullptr) != 0;
	}

	static std::vector<std::string> Hints(const NzbSummary& pick, const NzbSummary& donor, bool sameInner)
	{
		std::vector<std::string> hints;
		if (SameData(pick, donor)) hints.emplace_back("DupeSameSize=1");
		if (sameInner) hints.emplace_back("DupeSameInner=1");
		return hints;
	}
};

#endif
