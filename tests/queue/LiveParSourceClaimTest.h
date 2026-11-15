// Exercise the actual native claim path; claims are not cache completion metadata.
namespace {
ArticleInfo* LiveParClaimArticle(FileInfo& file)
{
    auto article = std::make_unique<ArticleInfo>();
    article->SetPartNumber((int)file.GetArticles()->size() + 1);
    article->SetStatus(ArticleInfo::aiRunning);
    ArticleInfo* ptr = article.get();
    file.GetArticles()->push_back(std::move(article));
    return ptr;
}
}

BOOST_AUTO_TEST_CASE(LiveParRunningHeadersAndCacheGeometry)
{
    FileInfo file;
    ArticleInfo* first = LiveParClaimArticle(file);
    ArticleInfo* second = LiveParClaimArticle(file);
    BOOST_REQUIRE(live_par_claim_source_header(&file, first, 0, 1024));
    // A non-owned stand-in keeps destructor accounting unchanged; only the
    // existing cache pointer/geometry preservation is being tested here.
    char content[8] = {};
    first->AttachSegment(content, 0, 123, 7);
    BOOST_CHECK(!live_par_claim_source_header(&file, second, 512, 1536));
    BOOST_CHECK(!live_par_claim_source_header(&file, first, 2048, 3072));
    BOOST_CHECK_EQUAL(first->GetSegmentOffset(), 123);
    BOOST_CHECK_EQUAL(first->GetSegmentSize(), 7);
    BOOST_CHECK(first->GetSegmentContent() == content);
}

BOOST_AUTO_TEST_CASE(LiveParOutOfOrderHeadersAndGaps)
{
    FileInfo file;
    ArticleInfo* first = LiveParClaimArticle(file);
    ArticleInfo* second = LiveParClaimArticle(file);
    ArticleInfo* third = LiveParClaimArticle(file);
    BOOST_CHECK(live_par_claim_source_header(&file, third, 20, 30));
    BOOST_CHECK(live_par_claim_source_header(&file, first, 0, 10));
    BOOST_CHECK(live_par_claim_source_header(&file, second, 10, 20));
    BOOST_CHECK(!live_par_claim_source_header(&file, first, 0, 10));
}

BOOST_AUTO_TEST_CASE(LiveParHeaderHistorySurvivesSegmentReset)
{
    FileInfo file;
    ArticleInfo* first = LiveParClaimArticle(file);
    BOOST_REQUIRE(live_par_claim_source_header(&file, first, 0, 10));
    first->SetStatus(ArticleInfo::aiUndefined);
    first->SetSegmentOffset(0);
    first->SetSegmentSize(0);
    first->DiscardSegment();
    BOOST_CHECK(!live_par_claim_source_header(&file, first, 0, 10));
    BOOST_CHECK_EQUAL(first->GetLiveParClaimBegin(), 0);
    BOOST_CHECK_EQUAL(first->GetLiveParClaimEnd(), 10);
}

BOOST_AUTO_TEST_CASE(LiveParInvalidAndRestoredHeaderRanges)
{
    FileInfo file;
    ArticleInfo* first = LiveParClaimArticle(file);
    ArticleInfo* second = LiveParClaimArticle(file);
    BOOST_CHECK(!live_par_claim_source_header(&file, first, -1, 10));
    BOOST_CHECK(!live_par_claim_source_header(&file, first, 5, 5));
    // A persisted completed segment is already owned even without a new claim.
    first->SetStatus(ArticleInfo::aiFinished);
    first->SetSegmentOffset(0);
    first->SetSegmentSize(10);
    BOOST_CHECK(!live_par_claim_source_header(&file, first, 0, 10));
    BOOST_CHECK(!live_par_claim_source_header(&file, second, 0, 5));
    BOOST_CHECK(live_par_claim_source_header(&file, second, 10, 20));
}

BOOST_AUTO_TEST_CASE(LiveParRestoredHistoryCannotHideEarlierOverlap)
{
    FileInfo file;
    ArticleInfo* first = LiveParClaimArticle(file);
    ArticleInfo* second = LiveParClaimArticle(file);
    ArticleInfo* third = LiveParClaimArticle(file);
    first->SetStatus(ArticleInfo::aiFinished);
    first->SetSegmentOffset(0);
    first->SetSegmentSize(300);
    second->SetStatus(ArticleInfo::aiFinished);
    second->SetSegmentOffset(100);
    second->SetSegmentSize(100);
    BOOST_CHECK(!live_par_claim_source_header(&file, third, 200, 300));
}

BOOST_AUTO_TEST_CASE(LiveParOrderedRestoredHistoryRemainsUsable)
{
    FileInfo file;
    ArticleInfo* first = LiveParClaimArticle(file);
    ArticleInfo* second = LiveParClaimArticle(file);
    ArticleInfo* third = LiveParClaimArticle(file);
    first->SetStatus(ArticleInfo::aiFinished);
    first->SetSegmentOffset(0);
    first->SetSegmentSize(100);
    second->SetStatus(ArticleInfo::aiFinished);
    second->SetSegmentOffset(100);
    second->SetSegmentSize(100);
    BOOST_CHECK(live_par_claim_source_header(&file, third, 200, 300));
}

BOOST_AUTO_TEST_CASE(LiveParMalformedRestoredHistoryIsAmbiguous)
{
    for (bool overflow : {false, true})
    {
        FileInfo file;
        ArticleInfo* first = LiveParClaimArticle(file);
        ArticleInfo* second = LiveParClaimArticle(file);
        first->SetStatus(ArticleInfo::aiFinished);
        first->SetSegmentOffset(overflow ? INT64_MAX - 10 : 0);
        first->SetSegmentSize(overflow ? 30 : -1);
        BOOST_CHECK(!live_par_claim_source_header(&file, second, 100, 200));
    }
}

// Borrowed namespace visitor: 0 refused, 1 complete, 2 transient busy.
namespace {
struct LiveNamespaceTestRow {
    int id;
    unsigned flags;
    std::string name, original, hash, output;
};
bool LiveNamespaceCollect(void* context, int id, unsigned flags, const char* name,
    const char* original, const char* hash, const char* output)
{
    auto& rows = *static_cast<std::vector<LiveNamespaceTestRow>*>(context);
    rows.push_back({id, flags, name, original, hash, output});
    return true;
}
bool LiveNamespaceStop(void* context, int, unsigned, const char*,
    const char*, const char*, const char*)
{
    ++*static_cast<int*>(context);
    return false;
}
}
BOOST_AUTO_TEST_CASE(LiveParNamespaceBorrowedRowsCoverQueueAndCompleted)
{
    NzbInfo nzb;
    auto source = std::make_unique<FileInfo>(40);
    source->SetNzbInfo(&nzb);
    source->SetFilename("part40.rar");
    source->SetOrigname("original-part40.rar");
    source->SetHash16k("00112233445566778899aabbccddeeff");
    source->SetOutputFilename("/fixture/40.out.tmp");
    source->SetActiveDownloads(1);
    nzb.GetFileList()->Add(std::move(source));
    auto parity = std::make_unique<FileInfo>(99);
    parity->SetNzbInfo(&nzb);
    parity->SetFilename("rel.par2");
    parity->SetParFile(true);
    nzb.GetFileList()->Add(std::move(parity));
    nzb.GetCompletedFiles()->emplace_back(39, "part39.rar", "part39.rar",
        CompletedFile::cfSuccess, 0, false, "", "");
    std::vector<LiveNamespaceTestRow> rows;
    BOOST_REQUIRE_EQUAL(live_par_visit_namespace(&nzb, LiveNamespaceCollect, &rows), 1);
    BOOST_REQUIRE_EQUAL(rows.size(), 3);
    BOOST_CHECK_EQUAL(rows[0].id, 40);
    BOOST_CHECK_EQUAL(rows[0].name, "part40.rar");
    BOOST_CHECK_EQUAL(rows[0].original, "original-part40.rar");
    BOOST_CHECK_EQUAL(rows[0].hash, "00112233445566778899aabbccddeeff");
    BOOST_CHECK_EQUAL(rows[0].output, "/fixture/40.out.tmp");
    BOOST_CHECK_EQUAL(rows[1].id, 99);
    BOOST_CHECK((rows[1].flags & 1) != 0);
    BOOST_CHECK_EQUAL(rows[1].hash, "");
    BOOST_CHECK_EQUAL(rows[2].id, 39);
    BOOST_CHECK_EQUAL(rows[2].name, "part39.rar");
    BOOST_CHECK((rows[2].flags & 4) != 0);
    rows.clear();
    BOOST_REQUIRE_EQUAL(NzbgetCQueueHost(nullptr, 43, 15, 0,
        reinterpret_cast<uintptr_t>(&nzb), reinterpret_cast<uintptr_t>(&rows),
        reinterpret_cast<const char*>(reinterpret_cast<uintptr_t>(&LiveNamespaceCollect)),
        nullptr, nullptr, nullptr, nullptr), 1);
    BOOST_REQUIRE_EQUAL(rows.size(), 3);
    BOOST_CHECK_EQUAL(rows[0].output, "/fixture/40.out.tmp");
}
BOOST_AUTO_TEST_CASE(LiveParNamespaceSizingRefusalStopsBeforeCopy)
{
    NzbInfo nzb;
    for (int id : {40, 41}) {
        auto file = std::make_unique<FileInfo>(id);
        file->SetNzbInfo(&nzb);
        file->SetFilename("part.rar");
        nzb.GetFileList()->Add(std::move(file));
    }
    int visits = 0;
    BOOST_CHECK_EQUAL(live_par_visit_namespace(&nzb, LiveNamespaceStop, &visits), 0);
    BOOST_CHECK_EQUAL(visits, 1);
    visits = 0;
    BOOST_CHECK_EQUAL(NzbgetCQueueHost(nullptr, 43, 15, 0,
        reinterpret_cast<uintptr_t>(&nzb), reinterpret_cast<uintptr_t>(&visits),
        reinterpret_cast<const char*>(reinterpret_cast<uintptr_t>(&LiveNamespaceStop)),
        nullptr, nullptr, nullptr, nullptr), 0);
    BOOST_CHECK_EQUAL(visits, 1);
    // Assembly may still have its final downloader registered. Neither active
    // count is proof that borrowing its changing output string is safe.
    auto* first = nzb.GetFileList()->at(0).get();
    first->SetTotalArticles(1);
    first->SetCompletedArticles(1);
    for (int active : {0, 1}) {
        first->SetActiveDownloads(active);
        visits = 0;
        BOOST_CHECK_EQUAL(live_par_visit_namespace(&nzb, LiveNamespaceStop, &visits), 2);
        BOOST_CHECK_EQUAL(visits, 0);
        BOOST_CHECK_EQUAL(NzbgetCQueueHost(nullptr, 43, 15, 0,
            reinterpret_cast<uintptr_t>(&nzb), reinterpret_cast<uintptr_t>(&visits),
            reinterpret_cast<const char*>(reinterpret_cast<uintptr_t>(&LiveNamespaceStop)),
            nullptr, nullptr, nullptr, nullptr), 2);
        BOOST_CHECK_EQUAL(visits, 0);
    }
    first->SetActiveDownloads(0);
    first->SetCompletedArticles(0);
    nzb.SetParking(true);
    BOOST_CHECK_EQUAL(live_par_visit_namespace(&nzb, LiveNamespaceStop, &visits), 2);
    nzb.SetParking(false);
    nzb.SetDeleting(true);
    BOOST_CHECK_EQUAL(live_par_visit_namespace(&nzb, LiveNamespaceStop, &visits), 2);
    BOOST_CHECK_EQUAL(visits, 0);

}
