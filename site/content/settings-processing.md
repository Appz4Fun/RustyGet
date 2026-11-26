+++
title = "Repair and unpacking"
description = "Configure article integrity checks, par2 repair, and archive extraction with illustrated Settings controls."
eyebrow = "Settings guide"
+++

Pictures use an isolated demonstration service and example values. Credentials are blank, folders use variables, and extension defaults are sanitized. These pictures illustrate the controls. Choose values for your installation; the pictured status comes from the demonstration service. Your build and installed extensions can differ.

## Check and repair

Choose a `ParCheck` mode for par2 verification and use `ParRepair` to control repair. `ParScan` and the rename controls affect how the daemon finds and verifies files.

`HealthCheck` determines how the daemon handles a download that cannot complete from its current articles. The `dupe` mode can fail over to another copy; see [Duplicate fleets](duplicate-fleets.md) before enabling it.

Review `ParBuffer` and `ParThreads` against available memory and CPU capacity.

`ParBuffer` also decides which releases can be repaired during the download (live repair). A release needs about 16 × its par2 block size + 47 MB; the poster picks the block size, so it doesn't follow the release size. A release that doesn't fit is repaired after the download, as before. Shares below come from 400 random NZBs of a real library (2026-11-15):

| ParBuffer | Largest par2 block repaired live | Share of NZBs |
| --- | --- | --- |
| 128 MB | 5 MB | 60% |
| 256 MB | 13 MB | 89% |
| 320 MB | 17 MB | 93% |
| 500 MB (default) | 28 MB | 96% |
| 1024 MB | 61 MB | 100% |

A 100 GB release posted with 3,000 to 5,000 par2 blocks (20 to 33 MB each) needs about 370 to 580 MB; with 2,000 blocks (50 MB each), about 850 MB. A download with par2 files holds its live repair memory until it finishes, and running downloads share `ParBuffer`, so the table holds for one download at a time. When a release doesn't fit, the log line "par2 blocks need N MB of ParBuffer" says how much it needs. `ParPauseQueue` and `ParTimeLimit` control how repair work interacts with downloads. A successful check or repair appears in the download's History details.

Key options: [`ParCheck`](options.md#parcheck), [`ParRepair`](options.md#parrepair), [`HealthCheck`](options.md#healthcheck), [`ParBuffer`](options.md#parbuffer), [`ParThreads`](options.md#parthreads).

<figure class="settings-figure">
<a class="settings-shot" href="assets/settings/check-repair.jpg" aria-label="Open full-size picture: Checksum, par2, rename, and health-check settings"><img src="assets/settings/check-repair.jpg" width="1280" height="1244" loading="lazy" alt="Checksum, par2, rename, and health-check settings"></a>
<figcaption>Verification, repair, and duplicate failover are separate controls. Open the picture to inspect the full section.</figcaption>
</figure>

## Unpack

Turn `Unpack` on when you want archive extraction. RustyGet extracts RAR, 7-Zip, ZIP, and tar archives itself, so it needs no external tools. `DirectUnpack` extracts RAR archives while articles arrive and pauses at a damaged volume until it's repaired; `UseTempUnpackDir` controls temporary extraction folders. `FeedThreads` sets how many threads decode a 7-Zip LZMA2 archive during live extraction.

`UnpackCleanupDisk` removes archive files after successful extraction. Choose it according to what you want the completed folder to retain. `UnpackIgnoreExt` lists nonstandard RAR extensions, such as `.cbr`, that should keep their names during RAR renaming. It also suppresses the failure for finding only nonstandard RAR archives with those extensions. Recognized `.rar` and `.7z` archives still follow normal extraction. `UnpackPassFile` supplies a password-list file when needed.

A category can override the global unpack setting. Nested archives are unpacked too, up to five levels deep; see [Built in instead of extensions](post-processing.md#built-in-instead-of-extensions).

Every unpack is checked before each file or folder is created. Unpacked names are made ASCII: accents are dropped, other characters become `_`, and upper and lower case are kept. `../` and absolute paths are refused. Names and paths must fit the file systems of both the unpack folder and the final folder. A download may unpack at most `UnpackMaxFiles` files (default 100), `UnpackMaxFolders` folders (default 10) and `UnpackMaxDepth` levels of folders (default 2; `Release/Sample/sample.mkv` is 2 deep). The volumes of an archive inside an archive are unpacked further and, when `UnpackCleanupDisk` deletes them, do not count; only what comes out does. A download over a limit stops unpacking and fails post-processing, with a message that names the limit and the file. No par repair is tried. Set a limit to 0 to turn it off.

Key options: [`Unpack`](options.md#unpack), [`DirectUnpack`](options.md#directunpack), [`UseTempUnpackDir`](options.md#usetempunpackdir), [`FeedThreads`](options.md#feedthreads), [`UnpackCleanupDisk`](options.md#unpackcleanupdisk), [`UnpackIgnoreExt`](options.md#unpackignoreext), [`UnpackMaxFiles`](options.md#unpackmaxfiles), [`UnpackMaxDepth`](options.md#unpackmaxdepth), [`UnpackMaxFolders`](options.md#unpackmaxfolders).

<figure class="settings-figure">
<a class="settings-shot" href="assets/settings/unpack.jpg" aria-label="Open full-size picture: Archive extraction and cleanup settings"><img src="assets/settings/unpack.jpg" width="1280" height="908" loading="lazy" alt="Archive extraction and cleanup settings"></a>
<figcaption>These controls choose extraction behavior and retained files. Open the picture to inspect the full section.</figcaption>
</figure>
