+++
title = "Downloads and diagnostics"
description = "Guide to incoming NZBs, duplicate recovery, queue storage, connection limits, and diagnostic logging."
eyebrow = "Settings guide"
+++

Pictures use an isolated demonstration service and example values. Credentials are blank, folders use variables, and extension defaults are sanitized. These pictures illustrate the controls. Choose values for your installation; the pictured status comes from the demonstration service. Your build and installed extensions can differ.

## Incoming NZBs

Set the scan interval and file age for the watched NZB folder. `AppendCategoryDir` controls category subfolders. Review archive scanning options if another program submits packed NZBs.

Duplicate handling belongs here too. `DupeCheck` groups backups by duplicate key. `DupeArticleFallback` enables article or live byte recovery from other copies. Duplicates come only from the app that sends a fleet; `DupeWatch` looks after the backups.

Follow [Duplicate fleets](duplicate-fleets.md) for the complete setup.

Key options: [`NzbDirInterval`](options.md#nzbdirinterval), [`NzbDirFileAge`](options.md#nzbdirfileage), [`DupeCheck`](options.md#dupecheck), [`DupeArticleFallback`](options.md#dupearticlefallback), [`DupeWatch`](options.md#dupewatch).

<figure class="settings-figure">
<a class="settings-shot" href="assets/settings/incoming-nzbs.jpg" aria-label="Open full-size picture: Incoming NZB and duplicate-recovery controls with blank indexer credentials"><img src="assets/settings/incoming-nzbs.jpg" width="1280" height="2093" loading="lazy" alt="Incoming NZB and duplicate-recovery controls with blank indexer credentials"></a>
<figcaption>Use option help for the recovery and health-check controls. Open the picture to inspect the full section.</figcaption>
</figure>

## Download queue

Review queue persistence before changing cache or disk behavior. `ArticleCache` and `WriteBuffer` affect memory and write buffering. `DiskSpace` controls the free-space pause threshold.

`KeepHistory` sets history retention. `NzbCleanupDisk` controls whether the original NZB remains on disk. Duplicate fleets can need those original NZBs as sources, so follow the fleet guide when choosing cleanup behavior.

Use [Performance](performance.md) for measured tuning guidance. Keep `SkipWrite` and `RawArticle` for their documented diagnostic uses.

Key options: [`ArticleCache`](options.md#articlecache), [`WriteBuffer`](options.md#writebuffer), [`DiskSpace`](options.md#diskspace), [`KeepHistory`](options.md#keephistory), [`NzbCleanupDisk`](options.md#nzbcleanupdisk).

<figure class="settings-figure">
<a class="settings-shot" href="assets/settings/download-queue.jpg" aria-label="Open full-size picture: Download queue persistence, memory, disk, and cleanup settings"><img src="assets/settings/download-queue.jpg" width="1280" height="1410" loading="lazy" alt="Download queue persistence, memory, disk, and cleanup settings"></a>
<figcaption>Cache and cleanup values depend on the workload and available storage. Open the picture to inspect the full section.</figcaption>
</figure>

## Connection

Set timeouts and retries for your network. `ArticleRetries` and `ArticleInterval` govern article retries; URL downloads have separate retry controls. `DownloadRate` sets a rate limit, and zero removes that limit.

Monthly and daily quotas count download volume. Review quota units and the billing-period start day in option help. After changing retries or timeouts, check Messages for recurring failures rather than changing several controls at once.

Key options: [`ArticleRetries`](options.md#articleretries), [`ArticleTimeout`](options.md#articletimeout), [`UrlRetries`](options.md#urlretries), [`UrlTimeout`](options.md#urltimeout), [`DownloadRate`](options.md#downloadrate), [`MonthlyQuota`](options.md#monthlyquota), [`DailyQuota`](options.md#dailyquota).

<figure class="settings-figure">
<a class="settings-shot" href="assets/settings/connection.jpg" aria-label="Open full-size picture: Article and URL connection controls, rate limit, and quotas"><img src="assets/settings/connection.jpg" width="1280" height="1107" loading="lazy" alt="Article and URL connection controls, rate limit, and quotas"></a>
<figcaption>Article and URL downloads have separate controls. Open the picture to inspect the full section.</figcaption>
</figure>

## Logging

Enable `SystemHealthCheck` to expose configuration notices. `LogToFile` writes `LogFile`; every 10 MB the file is moved aside as `rustyget-2026-11-15.1.log` (`.2` and so on) and gzipped to `.log.gz`. `LogRotations` sets how many of those are kept (default 500, `0` keeps all; with DETAIL messages from hundreds of connections a file fills in about a minute, so 10 kept only minutes); the oldest are deleted first. A device such as `/dev/stdout` is written as it is. Messages of each download go into the same file. The log targets control file and screen output. `LogBuffer` sets how many messages stay in memory for the web interface and each download's Log tab, and `0` keeps every message.

Use debug output when investigating a specific issue, then return to the level you normally use. Before sharing logs, remove credentials, private URLs, and identifying download details. The Messages tab and a download's History details provide context for failures.

Key options: [`SystemHealthCheck`](options.md#systemhealthcheck), [`LogToFile`](options.md#logtofile), [`LogRotations`](options.md#logrotations), [`LogBuffer`](options.md#logbuffer), [`TimeCorrection`](options.md#timecorrection).

<figure class="settings-figure">
<a class="settings-shot" href="assets/settings/logging.jpg" aria-label="Open full-size picture: System health, log targets, rotation, and message-buffer settings"><img src="assets/settings/logging.jpg" width="1280" height="1040" loading="lazy" alt="System health, log targets, rotation, and message-buffer settings"></a>
<figcaption>Health checks and logging serve different diagnostic purposes. Open the picture to inspect the full section.</figcaption>
</figure>
