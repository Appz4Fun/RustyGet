/*
 *  This file is part of nzbget. See <https://nzbget.com>.
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
#include <map>
#include <set>
#include <climits>
#include "TwinCheck.h"
#include "ArticleFetcher.h"
#include "DupeCoordinator.h"
#include "Options.h"
#include "Thread.h"
#include "Util.h"
#include "FileSystem.h"
#include <mutex>
#include <atomic>
#include "nzbget_rs.h"

namespace
{

// the result of a Rust call, copied and released
std::string RsString(NzbgetRsBuf buf)
{
	std::string text(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return text;
}

// the store of par2 file lists (queue directory, file "twincheck"), opened on
// first use: the constructors run before the options are read
bool OpenStore()
{
	// no options (unit tests): no store, and every list unknown
	static std::string openedPath;
	static std::mutex mutex;
	if (!g_Options || Util::EmptyStr(g_Options->GetQueueDir()))
	{
		return false;
	}
	std::string path = RsString(nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "twincheck"));
	std::lock_guard<std::mutex> guard(mutex);
	if (openedPath != path)
	{
		nzbget_rs_twin_open(path.c_str());
		openedPath = std::move(path);
	}
	return true;
}

class TwinCheckJob;

Mutex g_mutex;
std::set<TwinCheckJob*> g_jobs;
std::set<int> g_checking;		// downloads whose fingerprint is being fetched
std::atomic<int> g_jobCount{0};
bool g_stopping = false;
// fetches of a posting's par2-file that came to nothing (its articles gone, or
// the servers busy): after MaxAttempts it's given up, not tried first forever
std::map<int, int> g_failedAttempts;
constexpr int MaxAttempts = 3;
// the service, for Nudge() (under g_mutex); the check waits for its interval
// unless nudged
TwinCheck* g_instance = nullptr;
std::atomic<bool> g_nudged{false};

int Attempts(int nzbId)
{
	Guard guard(g_mutex);
	auto it = g_failedAttempts.find(nzbId);
	return it != g_failedAttempts.end() ? it->second : 0;
}

class TwinCheckJob : public Thread
{
public:
	TwinCheckJob(int nzbId, std::string nzbFilename, std::string posting) :
		m_nzbId(nzbId), m_nzbFilename(std::move(nzbFilename)), m_posting(std::move(posting)) {}
	/* sampling mode: the posting against the primary's */
	TwinCheckJob(int nzbId, std::string nzbFilename, int primaryId, std::string primaryNzbFilename) :
		m_nzbId(nzbId), m_nzbFilename(std::move(nzbFilename)), m_primaryId(primaryId),
		m_primaryNzbFilename(std::move(primaryNzbFilename)) {}

	void Cancel() { m_fetcher.Stop(); }
	int GetNzbId() const { return m_nzbId; }

protected:
	void Run() override;

private:
	int m_nzbId;
	std::string m_nzbFilename;
	std::string m_posting;	// the posting's own id (TwinCheck::Posting)
	int m_primaryId = 0;
	std::string m_primaryNzbFilename;
	ArticleFetcher m_fetcher;
	// something was stored: the next round at once (a check that came to nothing
	// waits for the interval, not retried every second)
	bool m_stored = false;

	void Sample();
	void Store(const char* name, const char* value, const char* name2 = nullptr, const char* value2 = nullptr);

	/* the bytes of the posting's smallest par2-file; false: it has none, or
	 * none could be read (<fetched>: its articles were asked for) */
	bool IndexData(std::vector<char>& data, bool& fetched);
};

bool TwinCheckJob::IndexData(std::vector<char>& data, bool& fetched)
{
	fetched = false;
	std::vector<TwinCheck::NzbEntry> entries = TwinCheck::ReadNzbEntries(m_nzbFilename.c_str());

	// its smallest par2-file: every par2-file of a set holds the FileDesc packets
	const TwinCheck::NzbEntry* index = nullptr;
	for (const TwinCheck::NzbEntry& entry : entries)
	{
		if (nzbget_rs_tpr_twin_check_job_should_replace_min_index(
			entry.par2 ? 1 : 0,
			index != nullptr ? 1 : 0,
			entry.size,
			index ? index->size : 0) != 0)
		{
			index = &entry;
		}
	}
	if (!nzbget_rs_tpr_twin_check_job_is_index_size_within_max(
		index != nullptr ? 1 : 0,
		index ? index->size : 0,
		TwinCheck::MaxIndexSize))
	{
		return false;
	}

	fetched = true;
	return TwinCheck::FetchEntry(m_fetcher, *index, TwinCheck::MaxIndexSize, [this]() { return IsStopped(); }, data);
}

void TwinCheckJob::Run()
{
	// leaves the registry however the check ends
	struct Unregister
	{
		TwinCheckJob* job;
		~Unregister()
		{
			Guard guard(g_mutex);
			g_jobs.erase(job);
			g_checking.erase(job->GetNzbId());
			g_jobCount--;
			// the labels from what this job stored, and the next job, at once
			if (!g_stopping && job->m_stored)
			{
				TwinCheck::NudgeLocked();
			}
		}
	} unregister{this};

	if (m_primaryId)
	{
		Sample();
		return;
	}

	std::vector<char> data;
	bool fetched = false;
	bool readable = IndexData(data, fetched);
	if (IsStopped())
	{
		return;
	}
	// parsed, kept with its file list and fingerprinted in Rust (twin.rs)
	bool complete = false;
	std::string fingerprint = readable ?
		TwinCheck::PutPar2(m_nzbId, m_posting.c_str(), data.data(), data.size(), &complete) : "";
	bool known = !fingerprint.empty();
	if (fetched && (!known || !complete))
	{
		// the par2-file's articles weren't all there (gone, or the servers busy):
		// asked again at a later round, a few times; a partial list is kept
		// meanwhile, and stays when the articles don't come
		Guard guard(g_mutex);
		if (nzbget_rs_stq_twin_check_eval_index_fetch_retry(
			fetched ? 1 : 0, known ? 1 : 0, complete ? 1 : 0,
			++g_failedAttempts[m_nzbId], MaxAttempts) != 0)
		{
			return;
		}
	}
	int fallbackFp = nzbget_rs_tpr_twin_check_job_fallback_fingerprint(
		known ? 1 : 0, fetched ? 1 : 0);
	if (fallbackFp == 1)
	{
		fingerprint = "lost";
	}
	else if (fallbackFp == 2)
	{
		fingerprint = "none";
	}
	int files = known ? nzbget_rs_ccm_twin_check_parse_fingerprint_files(fingerprint.c_str()) : 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = nullptr;
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued->GetId() == m_nzbId)
		{
			nzbInfo = queued;
		}
	}
	for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
	{
		if (!nzbInfo && historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == m_nzbId)
		{
			nzbInfo = historyInfo->GetNzbInfo();
		}
	}
	if (nzbInfo)
	{
		nzbInfo->GetParameters()->SetParameter(TwinCheck::FilesParam, fingerprint.c_str());
		m_stored = true;
		nzbInfo->PrintMessage(Message::mkDetail, "Fingerprint of %s: %s (%i file(s) in its par2 set)",
			nzbInfo->GetName(), fingerprint.c_str(), files);
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

void TwinCheckJob::Store(const char* name, const char* value, const char* name2, const char* value2)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = nullptr;
	for (NzbInfo* queued : downloadQueue->GetQueue())
	{
		if (queued->GetId() == m_nzbId)
		{
			nzbInfo = queued;
		}
	}
	for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
	{
		if (!nzbInfo && historyInfo->GetKind() == HistoryInfo::hkNzb && historyInfo->GetNzbInfo()->GetId() == m_nzbId)
		{
			nzbInfo = historyInfo->GetNzbInfo();
		}
	}
	if (nzbInfo)
	{
		nzbInfo->GetParameters()->SetParameter(name, value);
		if (name2)
		{
			nzbInfo->GetParameters()->SetParameter(name2, value2);
		}
		m_stored = true;
		downloadQueue->HistoryChanged();
		downloadQueue->Save();
	}
}

void TwinCheckJob::Sample()
{
	// the data files of both, by name: the same count, and article by article
	// the same sizes, or there's no telling by samples
	auto dataFiles = [](const std::string& nzbFilename)
		{
			std::vector<TwinCheck::NzbEntry> files;
			for (TwinCheck::NzbEntry& entry : TwinCheck::ReadNzbEntries(nzbFilename.c_str()))
			{
				if (!entry.par2)
				{
					files.push_back(std::move(entry));
				}
			}
			std::sort(files.begin(), files.end(), [](const TwinCheck::NzbEntry& a, const TwinCheck::NzbEntry& b)
				{
					return nzbget_rs_tpr_twin_check_compare_entry_names(
						a.filename.c_str(), a.subject.c_str(),
						b.filename.c_str(), b.subject.c_str()) != 0;
				});
			return files;
		};
	std::vector<TwinCheck::NzbEntry> primary = dataFiles(m_primaryNzbFilename);
	std::vector<TwinCheck::NzbEntry> dupe = dataFiles(m_nzbFilename);
	std::string primaryId = RsString(nzbget_rs_dqh_conn_format_port_str(m_primaryId));

	std::vector<size_t> pSegCounts;
	std::vector<int64> pSegBytes;
	pSegCounts.reserve(primary.size());
	for (const TwinCheck::NzbEntry& e : primary)
	{
		pSegCounts.push_back(e.segments.size());
		for (const auto& seg : e.segments)
		{
			pSegBytes.push_back(seg.first);
		}
	}
	std::vector<size_t> dSegCounts;
	std::vector<int64> dSegBytes;
	dSegCounts.reserve(dupe.size());
	for (const TwinCheck::NzbEntry& e : dupe)
	{
		dSegCounts.push_back(e.segments.size());
		for (const auto& seg : e.segments)
		{
			dSegBytes.push_back(seg.first);
		}
	}
	std::vector<size_t> sampleFileIdx(TwinCheck::SampleCount, 0);
	std::vector<size_t> sampleSegIdx(TwinCheck::SampleCount, 0);
	if (!nzbget_rs_dsp_twin_sample_indices(
		pSegCounts.data(), pSegBytes.data(), pSegBytes.size(), primary.size(),
		dSegCounts.data(), dSegBytes.data(), dSegBytes.size(), dupe.size(),
		(size_t)TwinCheck::SampleCount,
		sampleFileIdx.data(), sampleSegIdx.data()))
	{
		Store(TwinCheck::SampledParam, "none", TwinCheck::SampledOfParam, primaryId.c_str());
		return;
	}

	int compared = 0;
	int differ = 0;
	for (int n = 0; n < TwinCheck::SampleCount && !IsStopped(); n++)
	{
		const TwinCheck::NzbEntry& a = primary[sampleFileIdx[(size_t)n]];
		const TwinCheck::NzbEntry& b = dupe[sampleFileIdx[(size_t)n]];
		size_t segIdx = sampleSegIdx[(size_t)n];
		ArticleFetcher::FetchedArticle partA = m_fetcher.Fetch(a.segments[segIdx].second.c_str(), a.groups);
		ArticleFetcher::FetchedArticle partB = m_fetcher.Fetch(b.segments[segIdx].second.c_str(), b.groups);
		nzbget_rs_tpr_twin_check_job_compare_sampled_pair(
			partA.Success ? 1 : 0,
			partB.Success ? 1 : 0,
			partA.Offset,
			partB.Offset,
			partA.Data == partB.Data ? 1 : 0,
			compared,
			differ,
			&compared,
			&differ);
	}
	int verdict = nzbget_rs_cdn_twin_check_eval_sample_verdict(
		IsStopped() ? 1 : 0, compared, differ, TwinCheck::SampleCount);
	if (verdict == 0)
	{
		return;	// too few arrived: asked again at a later round
	}
	Store(TwinCheck::SampledParam, nzbget_rs_dqh_twin_check_sample_verdict_label(verdict),
		TwinCheck::SampledOfParam, primaryId.c_str());
}

struct Item
{
	NzbInfo* nzbInfo;
	bool queued;
	int rank;	// the queue first, then history newest first: who needs it soonest
};

}

std::vector<TwinCheck::NzbEntry> TwinCheck::ReadNzbEntries(const char* filename)
{
	std::vector<NzbEntry> entries;
	nzbget_rs_twin_visit_nzb_entries(
		filename,
		&entries,
		[](void* ctx, int64_t size, int32_t par2,
		   const char* fnamePtr, size_t fnameLen,
		   const char* subjPtr, size_t subjLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			NzbEntry entry;
			entry.size = size;
			entry.par2 = par2 != 0;
			entry.filename.assign(fnamePtr, fnameLen);
			entry.subject.assign(subjPtr, subjLen);
			out->push_back(std::move(entry));
		},
		[](void* ctx, const char* groupPtr, size_t groupLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			if (!out->empty())
			{
				out->back().groups.emplace_back(std::string(groupPtr, groupLen).c_str());
			}
		},
		[](void* ctx, int64_t bytes, const char* idPtr, size_t idLen) noexcept
		{
			auto* out = static_cast<std::vector<NzbEntry>*>(ctx);
			if (!out->empty())
			{
				out->back().segments.emplace_back(bytes, std::string(idPtr, idLen));
			}
		});
	return entries;
}

uint64 TwinCheck::BlockSize(const char* data, size_t size)
{
	return nzbget_rs_par2_block_size((const unsigned char*)data, size);
}

int TwinCheck::VolumeBlocks(const std::string& filename)
{
	return nzbget_rs_par2_volume_blocks(filename.c_str());
}

std::vector<TwinCheck::FileSig> TwinCheck::ParsePar2(const char* data, size_t size)
{
	std::vector<FileSig> sigs;
	nzbget_rs_twin_visit_par2_sigs(
		(const uint8_t*)data, size, &sigs,
		[](void* ctx, uint64_t length, const char* md5Ptr, size_t md5Len,
		   const char* namePtr, size_t nameLen) noexcept
		{
			auto* out = static_cast<std::vector<FileSig>*>(ctx);
			FileSig sig;
			sig.length = length;
			sig.md5.assign(md5Ptr, md5Len);
			sig.name.assign(namePtr, nameLen);
			out->push_back(std::move(sig));
		});
	return sigs;
}

std::string TwinCheck::Posting(NzbInfo* nzbInfo)
{
	return RsString(nzbget_rs_dqh_twin_check_format_u32_str(nzbInfo->GetFullContentHash()));
}

std::string TwinCheck::Identity(NzbInfo* nzbInfo)
{
	NzbParameter* print = nzbInfo->GetParameters()->Find(FilesParam);
	if (!print)
	{
		return "";
	}
	return RsString(nzbget_rs_ccm_twin_check_format_identity(
		print->GetValue(), Posting(nzbInfo).c_str()));
}

std::string TwinCheck::PutPar2(int nzbId, const char* posting, const char* data, size_t size,
	bool* complete)
{
	if (complete)
	{
		*complete = false;
	}
	if (!data || !size || !OpenStore())
	{
		return "";
	}
	// "fingerprint\tcomplete" from Rust (twin.rs), which keeps the file list
	std::string result = RsString(nzbget_rs_twin_put_par2(nzbId, posting ? posting : "",
		(const unsigned char*)data, size));
	int32_t completeFlag = 0;
	std::string fp = RsString(nzbget_rs_tpr_twin_check_split_put_par2_result(
		result.c_str(), &completeFlag));
	if (complete)
	{
		*complete = completeFlag != 0;
	}
	return fp;
}

int TwinCheck::HasSigs(int nzbId, const char* print)
{
	return OpenStore() ? nzbget_rs_twin_has(nzbId, print) : 0;
}

void TwinCheck::Prune(const std::string& live)
{
	if (OpenStore())
	{
		nzbget_rs_twin_prune(live.c_str());
	}
}

std::string TwinCheck::Kind(int primaryId, const char* primaryPrint, int dupeId, const char* dupePrint)
{
	if (!OpenStore())
	{
		return "";
	}
	return RsString(nzbget_rs_twin_kind(primaryId, primaryPrint, dupeId, dupePrint));
}

std::string TwinCheck::MatchByContent(int ownId, const char* ownPrint, const char* target, const char* targetAlt,
	int donorId, const char* donorPrint)
{
	if (!OpenStore())
	{
		return "";
	}
	return RsString(nzbget_rs_twin_match(ownId, ownPrint, target, targetAlt, donorId, donorPrint));
}

std::string TwinCheck::Fingerprint(const std::vector<FileSig>& sigs)
{
	// the md5 texts and lengths: the names aren't part of it
	std::vector<const char*> md5s;
	std::vector<unsigned long long> lengths;
	md5s.reserve(sigs.size());
	lengths.reserve(sigs.size());
	for (const FileSig& sig : sigs)
	{
		md5s.push_back(sig.md5.c_str());
		lengths.push_back(sig.length);
	}
	return RsString(nzbget_rs_twin_fingerprint(md5s.data(), lengths.data(), sigs.size()));
}

bool TwinCheck::FetchEntry(ArticleFetcher& fetcher, const NzbEntry& entry, int64 maxSize,
	const std::function<bool()>& stopped, std::vector<char>& data)
{
	data.clear();
	for (const auto& segment : entry.segments)
	{
		if (stopped())
		{
			return false;
		}
		ArticleFetcher::FetchedArticle part = fetcher.Fetch(segment.second.c_str(), entry.groups);
		size_t newDataLen = data.size();
		if (!nzbget_rs_stq_twin_check_is_fetched_entry_part_valid(
			part.Success ? 1 : 0, part.Offset, part.Data.size(),
			maxSize, data.size(), &newDataLen))
		{
			continue;
		}
		if (newDataLen > data.size())
		{
			data.resize(newDataLen);
		}
		std::copy(part.Data.begin(), part.Data.end(), data.begin() + (size_t)part.Offset);
	}
	return !data.empty();
}

void TwinCheck::ServiceWork()
{
	g_nudged = false;
	if (!Available() || !g_Options->GetDupeCheck())
	{
		return;
	}

	struct Job
	{
		int nzbId;
		std::string nzbFilename;
		bool hinted;
		int primaryId = 0;			// sampling against this one
		std::string primaryNzbFilename;
		std::string posting;		// the posting's own id (fingerprint jobs)
		int rank = 0;				// of its dupe key: the queue first, then the newest history
	};
	std::vector<Job> jobs;
	// the stored file lists are pruned to the postings still there, hourly
	std::string live;
	// (not while a fingerprint job runs: it may store a list its posting doesn't
	// carry the fingerprint of yet)
	static time_t lastPrune = 0;
	bool prune = nzbget_rs_tpr_twin_check_should_prune_store(
		(int64)Util::CurrentTime(), (int64)lastPrune, 60 * 60, (int)g_jobCount) != 0;
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (prune)
		{
			lastPrune = Util::CurrentTime();
			auto add = [&live](NzbInfo* nzbInfo)
				{
					live += RsString(nzbget_rs_tpr_twin_check_format_live_identity_line(
						nzbInfo->GetId(), Identity(nzbInfo).c_str()));
				};
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				add(nzbInfo);
			}
			for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
			{
				if (historyInfo->GetKind() == HistoryInfo::hkNzb)
				{
					add(historyInfo->GetNzbInfo());
				}
			}
			// nothing in queue and history: the whole store goes
			if (live.empty())
			{
				live = "\n";
			}
		}
		bool changed = false;

		// every download with a dupe key, by key
		std::map<std::string, std::vector<Item>> keys;
		auto keyOf = [](NzbInfo* nzbInfo)
			{
				return RsString(nzbget_rs_dqh_twin_check_lowercase_key(nzbInfo->GetDupeKey()));
			};
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetKind() == NzbInfo::nkNzb && !keyOf(nzbInfo).empty())
			{
				keys[keyOf(nzbInfo)].push_back({nzbInfo, true, 0});
			}
		}
		int rank = 0;
		for (std::unique_ptr<HistoryInfo>& historyInfo : *downloadQueue->GetHistory())
		{
			rank++;
			if (historyInfo->GetKind() == HistoryInfo::hkNzb && !keyOf(historyInfo->GetNzbInfo()).empty())
			{
				keys[keyOf(historyInfo->GetNzbInfo())].push_back({historyInfo->GetNzbInfo(), false, rank});
			}
		}

		auto isDupe = [](const Item& item)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				return nzbget_rs_ccm_twin_check_is_dupe_item(
					item.queued ? 1 : 0,
					nzbInfo->GetRemainingSize(),
					nzbInfo->GetPausedSize(),
					(int)nzbInfo->GetDeleteStatus(),
					(int)NzbInfo::dsDupe,
					(int)NzbInfo::dsCopy) != 0;
			};
		auto fingerprintOf = [](NzbInfo* nzbInfo)
			{
				NzbParameter* files = nzbInfo->GetParameters()->Find(FilesParam);
				return std::string(files ? files->GetValue() : "");
			};

		for (auto& entry : keys)
		{
			std::vector<Item>& items = entry.second;
			// merged downloads (their nzb-file is two) aren't compared: no fingerprint
			// stands for them
			items.erase(std::remove_if(items.begin(), items.end(),
				[](const Item& item)
				{
					return nzbget_rs_tpr_twin_check_is_merged_queued_filename(
						item.nzbInfo->GetQueuedFilename()) != 0;
				}),
				items.end());
			if (items.size() < 2)
			{
				continue;
			}
			size_t firstJob = jobs.size();
			int keyRank = INT_MAX;
			for (Item& item : items)
			{
				keyRank = std::min(keyRank, item.rank);
			}

			// postings without a fingerprint yet get one
			for (Item& item : items)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				// and so do those whose file list isn't stored (a fingerprint from
				// before the lists were kept, or the store file lost)
				std::string print = fingerprintOf(nzbInfo);
				bool listless = nzbget_rs_stq_twin_check_is_print_usable(print.c_str()) != 0 &&
					nzbget_rs_stq_twin_check_is_listless_from_counts(
						HasSigs(nzbInfo->GetId(), Identity(nzbInfo).c_str()),
						Attempts(nzbInfo->GetId()), MaxAttempts) != 0;
				if ((print.empty() || listless) &&
					!nzbget_rs_tpr_twin_check_is_merged_queued_filename(nzbInfo->GetQueuedFilename()) &&
					FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
				{
					// the dupe tool's hints of a twin go first; they prove nothing
					bool hinted = nzbget_rs_tpr_twin_check_is_job_hinted(
						nzbInfo->GetParameters()->Find("DupeSameSize") != nullptr ? 1 : 0,
						nzbInfo->GetParameters()->Find("DupeSameInner") != nullptr ? 1 : 0) != 0;
					Job job{nzbInfo->GetId(), nzbInfo->GetQueuedFilename(), hinted};
					job.posting = Posting(nzbInfo);
					jobs.push_back(std::move(job));
				}
			}

			// the primary: the download running (the highest score of the queued
			// ones not held back as duplicates), else the newest one in history
			// that wasn't deleted as a duplicate, else the queued one to run first
			NzbInfo* primary = nullptr;
			{
				std::vector<uint8_t> itemQueued;
				std::vector<uint8_t> itemIsDupe;
				std::vector<int> itemScores;
				itemQueued.reserve(items.size());
				itemIsDupe.reserve(items.size());
				itemScores.reserve(items.size());
				for (Item& item : items)
				{
					itemQueued.push_back(item.queued ? 1 : 0);
					itemIsDupe.push_back(isDupe(item) ? 1 : 0);
					itemScores.push_back(item.nzbInfo->GetDupeScore());
				}
				int primaryIdx = nzbget_rs_dsp_twin_select_primary(
					itemQueued.data(), itemIsDupe.data(), itemScores.data(), items.size());
				if (primaryIdx >= 0 && (size_t)primaryIdx < items.size())
				{
					primary = items[(size_t)primaryIdx].nzbInfo;
				}
			}

			// the labels: of the primary's twins and alts, by their par2 fingerprints,
			// else by articles sampled at the same places (none without either)
			std::string primaryPrint = primary ? fingerprintOf(primary) : "";
			bool primaryOk = primary &&
				!nzbget_rs_tpr_twin_check_is_merged_queued_filename(primary->GetQueuedFilename()) &&
				FileSystem::FileExists(primary->GetQueuedFilename());
			std::string primaryId = primary ? RsString(nzbget_rs_dqh_conn_format_port_str(primary->GetId())) : "";
			for (Item& item : items)
			{
				NzbInfo* nzbInfo = item.nzbInfo;
				std::string print = fingerprintOf(nzbInfo);
				bool byPar2 = nzbget_rs_stq_twin_check_is_print_usable(primaryPrint.c_str()) != 0 &&
					nzbget_rs_stq_twin_check_is_print_usable(print.c_str()) != 0;
				NzbParameter* sampled = nzbInfo->GetParameters()->Find(SampledParam);
				NzbParameter* sampledOf = nzbInfo->GetParameters()->Find(SampledOfParam);
				int sampledNowInt = 0;
				bool sampleable = nzbget_rs_stq_twin_check_eval_sampleable(
					primaryOk ? 1 : 0,
					nzbInfo == primary ? 1 : 0,
					isDupe(item) ? 1 : 0,
					print.empty() ? 1 : 0,
					primaryPrint.empty() ? 1 : 0,
					byPar2 ? 1 : 0,
					sampled != nullptr ? 1 : 0,
					sampledOf ? sampledOf->GetValue() : nullptr,
					primaryId.c_str(),
					&sampledNowInt) != 0;
				bool sampledNow = sampledNowInt != 0;
				if (sampleable && !sampledNow &&
					!nzbget_rs_tpr_twin_check_is_merged_queued_filename(nzbInfo->GetQueuedFilename()) &&
					FileSystem::FileExists(nzbInfo->GetQueuedFilename()))
				{
					jobs.push_back({nzbInfo->GetId(), nzbInfo->GetQueuedFilename(), false,
						primary->GetId(), primary->GetQueuedFilename()});
				}
				std::string byFiles;
				bool listed = false;
				if (nzbInfo != primary && isDupe(item) && byPar2)
				{
					byFiles = Kind(primary->GetId(), Identity(primary).c_str(), nzbInfo->GetId(),
						Identity(nzbInfo).c_str());
					listed = HasSigs(primary->GetId(), Identity(primary).c_str()) > 0 ||
						HasSigs(nzbInfo->GetId(), Identity(nzbInfo).c_str()) > 0;
				}
				std::string kindResolved = RsString(nzbget_rs_dsp_twin_resolve_kind(
					nzbInfo == primary ? 1 : 0,
					isDupe(item) ? 1 : 0,
					byPar2 ? 1 : 0,
					byFiles.c_str(),
					listed ? 1 : 0,
					print == primaryPrint ? 1 : 0,
					sampleable ? 1 : 0,
					sampledNow ? 1 : 0,
					sampled ? sampled->GetValue() : ""));
				const char* kind = kindResolved.c_str();
				NzbParameter* old = nzbInfo->GetParameters()->Find(KindParam);
				if (nzbget_rs_dqh_bytes_equal(old ? old->GetValue() : "", kind) == 0)
				{
					nzbInfo->GetParameters()->SetParameter(KindParam, kind);
					changed = true;
					if (*kind)
					{
						std::string msg = RsString(nzbget_rs_twin_format_kind_message(
							nzbInfo->GetName(), primary->GetName(), kind, byPar2 ? 1 : 0));
						nzbInfo->PrintMessage(Message::mkInfo, "%s", msg.c_str());
					}
				}
			}

			for (size_t i = firstJob; i < jobs.size(); i++)
			{
				jobs[i].rank = keyRank;
			}
		}

		if (changed)
		{
			downloadQueue->HistoryChanged();
			downloadQueue->Save();
		}
	}

	if (!live.empty())
	{
		Guard guard(g_mutex);
		// a job started since the snapshot: next time (Prune runs under g_mutex,
		// which a new job registers under)
		if (g_jobs.empty())
		{
			Prune(live);
		}
		else
		{
			lastPrune = 0;
		}
	}

	std::stable_sort(jobs.begin(), jobs.end(), [](const Job& a, const Job& b)
		{
			return nzbget_rs_tpr_twin_check_job_less(
				a.rank, a.hinted ? 1 : 0, b.rank, b.hinted ? 1 : 0) != 0;
		});

	Guard guard(g_mutex);
	for (Job& job : jobs)
	{
		if (nzbget_rs_tpr_twin_check_should_stop_launching_jobs(
			g_stopping ? 1 : 0, (int)g_jobs.size(), MaxRunning) != 0)
		{
			break;
		}
		if (g_checking.count(job.nzbId))
		{
			continue;
		}
		TwinCheckJob* thread = job.primaryId ?
			new TwinCheckJob(job.nzbId, job.nzbFilename, job.primaryId, job.primaryNzbFilename) :
			new TwinCheckJob(job.nzbId, job.nzbFilename, job.posting);
		g_jobs.insert(thread);
		g_checking.insert(job.nzbId);
		g_jobCount++;
		thread->SetAutoDestroy(true);
		thread->Start();
	}
}

void TwinCheck::StopAll()
{
	Guard guard(g_mutex);
	g_stopping = true;
	for (TwinCheckJob* job : g_jobs)
	{
		job->Stop();
		job->Cancel();
	}
}

void TwinCheck::WaitAll()
{
	while (g_jobCount > 0)
	{
		Util::Sleep(20);
	}
}

void TwinCheck::Reset()
{
	Guard guard(g_mutex);
	g_stopping = false;
}

TwinCheck::TwinCheck()
{
	Reset();
	{
		Guard guard(g_mutex);
		g_instance = this;
	}
	m_observer.m_owner = this;
	DownloadQueue::Guard()->Attach(&m_observer);
}

TwinCheck::~TwinCheck()
{
	// the queue (QueueCoordinator's) outlives this, and would notify freed memory
	DownloadQueue::Guard()->Detach(&m_observer);
	Guard guard(g_mutex);
	g_instance = nullptr;
}

int TwinCheck::ServiceInterval()
{
	// nudged: within a second, not at once: a fetch that fails at once is retried
	// at most once a second
	return nzbget_rs_ccm_twin_check_service_interval(g_nudged ? 1 : 0);
}

void TwinCheck::Nudge()
{
	Guard guard(g_mutex);
	NudgeLocked();
}

void TwinCheck::NudgeLocked()
{
	g_nudged = true;
	if (g_instance)
	{
		g_instance->WakeUp();
	}
}

void TwinCheck::DownloadQueueObserver::Update(Subject* caller, void* aspect)
{
	// called with the queue locked: no g_mutex here (jobs ending take it, then
	// may wait for the queue)
	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (nzbget_rs_tpr_twin_check_should_nudge_on_queue_event(
		(int)queueAspect->action,
		(int)DownloadQueue::eaNzbAdded,
		queueAspect->nzbInfo != nullptr ? 1 : 0,
		queueAspect->nzbInfo ? queueAspect->nzbInfo->GetDupeKey() : nullptr) != 0)
	{
		g_nudged = true;
		m_owner->WakeUp();
	}
}
