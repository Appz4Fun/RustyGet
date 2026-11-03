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

#include <boost/test/unit_test.hpp>
#include <fstream>
#include "DupeCoordinator.h"
#include "FileSystem.h"
#include "Options.h"
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

BOOST_AUTO_TEST_SUITE(QueueTest)

namespace
{

class StaticDownloadQueue final : public DownloadQueue
{
public:
	StaticDownloadQueue() { Init(this); }
	~StaticDownloadQueue() { Final(); }
	bool EditEntry(int, EEditAction, const char*) override { return false; }
	bool EditList(IdList*, NameList*, EMatchMode, EEditAction, const char*) override { return false; }
	void HistoryChanged() override {}
	void Save() override {}
	void SaveChanged() override {}
};

std::unique_ptr<NzbInfo> MakeNzb(const char* name, const char* dupeKey, int dupeScore)
{
	std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
	nzbInfo->SetKind(NzbInfo::nkNzb);
	nzbInfo->SetName(name);
	nzbInfo->SetDupeKey(dupeKey);
	nzbInfo->SetDupeScore(dupeScore);
	nzbInfo->SetDupeMode(dmScore);
	return nzbInfo;
}

// source nzb-files of backups, as nzbget keeps them in NzbDir; removed at exit
struct KeptNzbFiles
{
	std::vector<std::string> paths;
	~KeptNzbFiles()
	{
		for (const std::string& path : paths)
		{
			FileSystem::DeleteFile(path.c_str());
		}
	}
} g_keptNzbFiles;

std::string KeepNzbFile(const char* name)
{
	std::string path = (fs::temp_directory_path() / fs::make_unique_filename()).string() + name + ".nzb.queued";
	std::ofstream(path) << "<nzb></nzb>";
	g_keptNzbFiles.paths.push_back(path);
	return path;
}

// a history item deleted as duplicate backup: healthy (nothing failed), not
// marked bad and with its source nzb-file kept, so ReturnBestDupe may move
// it back to the queue
NzbInfo* AddBackup(DownloadQueue& queue, const char* name, const char* dupeKey, int dupeScore,
	bool keepNzbFile = true)
{
	std::unique_ptr<NzbInfo> nzbInfo = MakeNzb(name, dupeKey, dupeScore);
	nzbInfo->SetDeleteStatus(NzbInfo::dsDupe);
	nzbInfo->SetQueuedFilename(keepNzbFile ? KeepNzbFile(name).c_str() : "/nonexistent/gone.nzb.queued");
	NzbInfo* raw = nzbInfo.get();
	queue.GetHistory()->Add(std::make_unique<HistoryInfo>(std::move(nzbInfo)), true);
	return raw;
}

} // namespace

BOOST_AUTO_TEST_CASE(DupeFailoverWarrantedTest)
{
	// a posting at 30% health warrants 30% of the score it was queued with
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(90, 300, 27));
	BOOST_CHECK(!DupeCoordinator::DupeFailoverWarranted(90, 300, 26));
	// a dead posting warrants nothing: any backup is better, even one scored below
	// zero - the one ReturnBestDupe would fetch after the failure anyway (B27)
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(90, 0, 0));
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(90, 0, -5));
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(-10, 500, -20));
	// at full health only an equal or better backup qualifies
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(90, 1000, 90));
	BOOST_CHECK(!DupeCoordinator::DupeFailoverWarranted(90, 1000, 89));
	// items queued without a score (0) warrant nothing, like ReturnBestDupe
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(0, 1000, 0));
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(-10, 500, 0));
	// health outside the permille range is clamped
	BOOST_CHECK(!DupeCoordinator::DupeFailoverWarranted(90, 1500, 89));
	BOOST_CHECK(DupeCoordinator::DupeFailoverWarranted(90, -7, 0));
}

BOOST_AUTO_TEST_CASE(DupeSuccessMatchesUnrepairedHealthFailure)
{
	struct RestoreOptions
	{
		Options* previous = g_Options;
		~RestoreOptions() { g_Options = previous; }
	} restoreOptions;
	Options::CmdOptList cmdOpts;
	cmdOpts.emplace_back("HealthCheck=dupe");
	cmdOpts.emplace_back("SevenZipCmd=7z");
	cmdOpts.emplace_back("UnrarCmd=unrar");
	Options options(&cmdOpts, nullptr);
	NzbInfo nzb;
	nzb.SetSize(1000);
	nzb.SetCurrentFailedSize(40);
	nzb.SetParStatus(NzbInfo::psSkipped);
	nzb.SetUnpackStatus(NzbInfo::usNone);
	BOOST_REQUIRE_EQUAL(nzb.CalcHealth(), 960);
	BOOST_REQUIRE_EQUAL(nzb.CalcCriticalHealth(true), 850);
	BOOST_REQUIRE_EQUAL(nzb.CalcCriticalHealth(false), 1000);
	BOOST_REQUIRE_EQUAL(std::string(nzb.MakeTextStatus(true)), "FAILURE/HEALTH");
	BOOST_CHECK(!nzb.IsDupeSuccess());
	// A verified repair or an explicit good mark still takes precedence.
	nzb.SetParStatus(NzbInfo::psSuccess);
	BOOST_CHECK(nzb.IsDupeSuccess());
	nzb.SetParStatus(NzbInfo::psSkipped);
	nzb.SetMarkStatus(NzbInfo::ksGood);
	BOOST_CHECK(nzb.IsDupeSuccess());
	nzb.SetMarkStatus(NzbInfo::ksNone);
	nzb.SetCurrentFailedSize(0);
	BOOST_CHECK(nzb.IsDupeSuccess());
}

BOOST_AUTO_TEST_CASE(DupeAlivePermilleTest)
{
	NzbInfo nzbInfo;
	BOOST_CHECK_EQUAL(DupeCoordinator::AlivePermille(&nzbInfo), -1);
	auto alive = [&](const char* value)
		{
			nzbInfo.GetParameters()->SetParameter("DupeAlive", value);
			return DupeCoordinator::AlivePermille(&nzbInfo);
		};
	BOOST_CHECK_EQUAL(alive("100"), 1000);
	BOOST_CHECK_EQUAL(alive("99.6"), 996);
	BOOST_CHECK_EQUAL(alive("0"), 0);
	BOOST_CHECK_EQUAL(alive(""), -1);
	BOOST_CHECK_EQUAL(alive("abc"), -1);
	BOOST_CHECK_EQUAL(alive("-5"), 0);
	BOOST_CHECK_EQUAL(alive("1000"), 1000);
	BOOST_CHECK_EQUAL(alive("nan"), -1);
}

BOOST_AUTO_TEST_CASE(DupeCoordinatorFindDupeBackupTest)
{
	Options::CmdOptList cmdOpts;
	Options options(&cmdOpts, nullptr);
	StaticDownloadQueue queue;
	DupeCoordinator coordinator;

	std::unique_ptr<NzbInfo> queued = MakeNzb("Rel.2026.1080p-A", "imdb:1", 10);
	NzbInfo* item = queued.get();
	queue.GetQueue()->Add(std::move(queued), false);

	// nothing in history
	BOOST_CHECK(coordinator.FindDupeBackup(&queue, item, item->GetName(), item->GetDupeKey()) == nullptr);

	// the highest-scored healthy backup of the same key wins; other keys are ignored
	AddBackup(queue, "Rel.2026.1080p-B", "imdb:1", 50);
	NzbInfo* best = AddBackup(queue, "Rel.2026.1080p-C", "imdb:1", 90);
	AddBackup(queue, "Other.2026-X", "imdb:2", 100);
	HistoryInfo* backup = coordinator.FindDupeBackup(&queue, item, item->GetName(), item->GetDupeKey());
	BOOST_REQUIRE(backup);
	BOOST_CHECK(backup->GetNzbInfo() == best);

	// a backup whose source nzb-file is gone can't be downloaded again: the
	// next one is picked instead
	NzbInfo* gone = AddBackup(queue, "Rel.2026.1080p-Z", "imdb:1", 95, false);
	backup = coordinator.FindDupeBackup(&queue, item, item->GetName(), item->GetDupeKey());
	BOOST_REQUIRE(backup);
	BOOST_CHECK(backup->GetNzbInfo() == best);
	gone->SetDupeKey("imdb:gone");

	// a backup marked bad, or one that failed on its own, is no candidate
	best->SetMarkStatus(NzbInfo::ksBad);
	backup = coordinator.FindDupeBackup(&queue, item, item->GetName(), item->GetDupeKey());
	BOOST_REQUIRE(backup);
	BOOST_CHECK_EQUAL(backup->GetNzbInfo()->GetDupeScore(), 50);

	// a queued duplicate with a higher score than every backup blocks them
	std::unique_ptr<NzbInfo> rival = MakeNzb("Rel.2026.1080p-D", "imdb:1", 60);
	NzbInfo* rivalRaw = rival.get();
	queue.GetQueue()->Add(std::move(rival), false);
	BOOST_CHECK(coordinator.FindDupeBackup(&queue, item, item->GetName(), item->GetDupeKey()) == nullptr);

	// the item itself never counts as its own rival
	item->SetDupeScore(99);
	queue.GetQueue()->Remove(rivalRaw);
	backup = coordinator.FindDupeBackup(&queue, item, item->GetName(), item->GetDupeKey());
	BOOST_REQUIRE(backup);
	BOOST_CHECK_EQUAL(backup->GetNzbInfo()->GetDupeScore(), 50);

	// a good-duplicate in history ends the search
	std::unique_ptr<NzbInfo> good = MakeNzb("Rel.2026.1080p-G", "imdb:1", 1);
	good->SetMarkStatus(NzbInfo::ksGood);
	queue.GetHistory()->Add(std::make_unique<HistoryInfo>(std::move(good)), true);
	BOOST_CHECK(coordinator.FindDupeBackup(&queue, item, item->GetName(), item->GetDupeKey()) == nullptr);
}

#ifdef NZBGET_USE_RUST
BOOST_AUTO_TEST_CASE(RustDupeCoordinatorFfiParity)
{
	BOOST_CHECK_NE(nzbget_rs_dupecoord_same_name_or_key("Movie.A", "imdb:123", "Movie.B", "IMDB:123"), 0);
	BOOST_CHECK_EQUAL(nzbget_rs_dupecoord_same_name_or_key("Movie.A", "imdb:123", "Movie.A", "imdb:456"), 0);
	BOOST_CHECK_NE(nzbget_rs_dupecoord_same_name_or_key("Movie.A", "", "movie.a", nullptr), 0);

	BOOST_CHECK_NE(nzbget_rs_dupecoord_failover_warranted(1000, 300, 300), 0);
	BOOST_CHECK_EQUAL(nzbget_rs_dupecoord_failover_warranted(1000, 300, 299), 0);
	BOOST_CHECK_EQUAL(nzbget_rs_dupecoord_parse_alive_permille("96.5"), 965);
	BOOST_CHECK_EQUAL(nzbget_rs_dupecoord_parse_alive_permille("invalid"), -1);

	BOOST_CHECK_EQUAL(nzbget_rs_dupecoord_status_from_text("SUCCESS/ALL"), (int)DupeCoordinator::dsSuccess);
	BOOST_CHECK_EQUAL(nzbget_rs_dupecoord_status_from_text("FAILURE/PAR"), (int)DupeCoordinator::dsFailure);
	BOOST_CHECK_EQUAL(nzbget_rs_dupecoord_status_from_text("WARNING/SCRIPT"), (int)DupeCoordinator::dsWarning);

	NzbgetRsBuf reason = nzbget_rs_dupecoord_format_no_backup_reason(5, 1, 1, 1, 1, 0, 1);
	std::string text(reason.data ? reason.data : "", reason.len);
	nzbget_rs_free(reason);
	BOOST_CHECK(text.find("5 other item(s) of the key in history:") != std::string::npos);

	BOOST_CHECK_EQUAL(nzbget_rs_qhp_calc_health(1000, 0, 100, 0), 900);
	BOOST_CHECK_EQUAL(nzbget_rs_qhp_calc_critical_health(1100, 100, 0, 1), 900);
	BOOST_CHECK_EQUAL(nzbget_rs_qhp_calc_write_buffer_size(500, 1024 * 1024), 500 * 1024);
	BOOST_CHECK_NE(nzbget_rs_qhp_is_data_filename("Show.S01E01.mkv"), 0);
	BOOST_CHECK_EQUAL(nzbget_rs_qhp_is_data_filename("Show.S01E01.par2"), 0);
	BOOST_CHECK_NE(nzbget_rs_qhp_size_near(10000, 10025), 0);
	BOOST_CHECK_EQUAL(nzbget_rs_qhp_size_near(10000, 10035), 0);
	BOOST_CHECK_NE(nzbget_rs_qhp_download_hopeless(1000, 5, 95), 0);
	int parsedMode = -1;
	BOOST_CHECK_NE(nzbget_rs_qhp_parse_dupe_mode("score", &parsedMode), 0);
	BOOST_CHECK_EQUAL(parsedMode, (int)dmScore);
	BOOST_CHECK_NE(nzbget_rs_qhp_parse_dupe_mode("all", &parsedMode), 0);
	BOOST_CHECK_EQUAL(parsedMode, (int)dmAll);
	BOOST_CHECK_NE(nzbget_rs_qhp_parse_dupe_mode("force", &parsedMode), 0);
	BOOST_CHECK_EQUAL(parsedMode, (int)dmForce);
	BOOST_CHECK_EQUAL(nzbget_rs_qhp_parse_dupe_mode("bad", &parsedMode), 0);

	BOOST_CHECK_EQUAL(nzbget_rs_dsp_parse_format_version("nzbget diskstate file version 64"), 64);
	int nzbId = 0, fileId = 0;
	BOOST_CHECK_NE(nzbget_rs_dsp_parse_ids_file("nzb 17 file 309\n", &nzbId, &fileId), 0);
	BOOST_CHECK_EQUAL(nzbId, 17);
	BOOST_CHECK_EQUAL(fileId, 309);
	int isNzbLog = 0;
	BOOST_CHECK_EQUAL(nzbget_rs_dsp_classify_queue_dir_id("n42.log", &isNzbLog), 42);
	BOOST_CHECK_NE(isNzbLog, 0);
	BOOST_CHECK_EQUAL(nzbget_rs_dsp_classify_queue_dir_id("108s", &isNzbLog), 108);
	BOOST_CHECK_EQUAL(isNzbLog, 0);
	BOOST_CHECK_EQUAL(nzbget_rs_dsp_normalize_post_stage(6, 58, 12), 7);
	BOOST_CHECK_EQUAL(nzbget_rs_dsp_normalize_post_stage(14, 64, 12), 12);
	BOOST_CHECK_NE(nzbget_rs_dsp_can_process_extension("Show.S01E01.nzb"), 0);
	BOOST_CHECK_EQUAL(nzbget_rs_dsp_can_process_extension("Show.S01E01.nzb.queued"), 0);
	BOOST_CHECK_EQUAL(nzbget_rs_dsp_classify_missed_articles(2, 50, 0), 1);
	BOOST_CHECK_EQUAL(nzbget_rs_dsp_classify_missed_articles(10, 50, 0), 2);
}
#endif

BOOST_AUTO_TEST_SUITE_END()
