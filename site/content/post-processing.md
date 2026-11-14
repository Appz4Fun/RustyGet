+++
title = "Check, repair, and unpack"
nav_title = "Check, repair, unpack"
description = "How RustyGet verifies downloads with par2, repairs them on every core, restores obfuscated names, unpacks archives, including nested ones, and fixes file permissions."
eyebrow = "Features"
lede = "After the last article arrives, RustyGet checks the files, repairs damage with par2 recovery data, restores obfuscated names, unpacks archives, and cleans up. All of it runs inside the program, with no external par2, unrar, or 7-Zip tool."
+++

## The pipeline

<ol class="steps">
<li><strong>Health check during download.</strong> If so many articles are missing that repair can't succeed, <code>HealthCheck</code> acts before you waste more bandwidth.</li>
<li><strong>Rename.</strong> Par-rename and RAR-rename restore the real filenames of obfuscated postings.</li>
<li><strong>Verify.</strong> Quick verification checks only the files that had download errors.</li>
<li><strong>Repair.</strong> Multithreaded par2 repair rebuilds damaged files.</li>
<li><strong>Unpack.</strong> RAR, 7-Zip, ZIP, and tar archives are extracted. Passwords come from the NZB file or from a password file.</li>
<li><strong>Unpack nested archives and clean up.</strong> Archives inside archives are extracted too, permissions are normalized, and archives and par2 files are deleted.</li>
</ol>

`PostStrategy` controls how many downloads can be in these steps at once: `sequential`, `balanced` (the template's value), `aggressive`, or `rocket`.

## Health check

Health is the share of a download's articles that the news servers actually delivered. Critical health is the lowest health that the download's par2 recovery data can still repair; RustyGet computes it from the number and size of the par2 files.

When health drops below critical health, `HealthCheck` decides what happens:

| Value | Effect |
|---|---|
| `park` | NZBGet's default. Moves the download to history with its files kept, so you can add more par2 files or retry later. |
| `delete` | Deletes the download and moves it to history. With duplicate checks on, the best backup of the title starts. |
| `pause` | Pauses the download. You start a backup yourself. |
| `none` | Keeps downloading. Use it when you plan to repair by hand. This is the built-in value when the configuration file leaves `HealthCheck` out. |
| `dupe` <span class="tag tag-new">new</span> | The value in RustyGet's configuration template. Waits for a duplicate first. If a better copy of the same title waits in history, the download fails over to it; if article fallback can borrow the missing articles, the download keeps going. See [Duplicate fleets](duplicate-fleets.md). |

## Renaming obfuscated files

Many postings hide their real names, for example a 32-character hexadecimal base name with extensions such as `.01`, `.21`, and `.par2`. Archives with names like these can't be unpacked until the real names come back.

Par-rename
: The par2 set stores the original filenames. RustyGet reads them and renames the files in seconds, with no verification pass. The classic approach, running par2cmdline over every file, can take tens of minutes on a large download. Set by `ParRename`.

RAR-rename
: Restores the names of multi-volume RAR archives from the archive headers. It works when a posting has no par2 files, and when the files were obfuscated before the par2 files were made. Set by `RarRename`. RustyGet parses RAR headers and renames volume sets in Rust.

Direct rename
: Renames files while they download, as soon as the first article of each par2 file arrives. Set by `DirectRename`.

## Quick verification

Par2 verification reads and checksums every file, which takes minutes to tens of minutes. On a slow device with a large, lightly damaged download, verification can take longer than the repair itself.

RustyGet doesn't need that pass. It records which files had download errors and hands exactly those to the repair module, so repair starts right away. `ParQuick` controls this, and `ParScan` sets which files take part:

| `ParScan` | Files scanned |
|---|---|
| `limited` | Only files that belong to the par2 set. |
| `extended` | Default. Files of the set first, then other files, until enough recovery blocks are found. |
| `full` | Every file in the download directory. |
| `dupe` | Like `extended`, and also borrows par2 blocks from duplicate downloads of the same title in history. |

## Multicore repair

par2cmdline predates multicore CPUs and uses one core. A popular multicore fork depends on an Intel library and runs only on Intel-compatible CPUs, which leaves out the ARM chips in most NAS boxes and media players.

The repair module, the `par-rs` crate, has its own threading that works on every platform and CPU. Set the thread count with `ParThreads` and the memory with `ParBuffer`.

> [!NOTE] Repair from a twin
> When a download is short of recovery blocks, RustyGet can use the par2 volumes of a twin: a duplicate posting whose files are byte-identical, proven by par2 checksums. See [Duplicate fleets](duplicate-fleets.md#repair-with-a-twins-par2-files).

## Unpack

`Unpack` extracts RAR, 7-Zip, ZIP, and tar archives with the built-in `zipfast` and `zipview` crates, including encrypted and multi-volume archives. It needs no external program, and `UnrarCmd` and `SevenZipCmd` are no longer used. `DirectUnpack` starts extracting while the download is still running, which saves time when the download needs no repair.

- `UseTempUnpackDir` extracts into a temporary `_unpack` directory and moves the files when done.
- `UnpackPassFile` points to a file of passwords to try.
- `UnpackCleanupDisk` deletes archives after a successful unpack, and `ExtCleanupDisk` deletes other leftovers, such as `.par2` and `.sfv` files.
- `UnpackIgnoreExt` leaves some archives alone, for example `.cbr` comic books.

## Retry failed articles

A failed download goes to history, where you can download it again. Besides starting over, you can retry only the articles that failed. That keeps everything already downloaded and fetches just the gaps. It helps when:

- A posting hadn't finished propagating to your servers.
- An optional news server was unreachable during the first attempt.
- You added or turned on another news server.

## Built in instead of extensions

NZBGet left nested archives, file permissions, and finding more copies to extension scripts. RustyGet does them itself, so there's nothing to install:

Nested archives
: After unpacking, RustyGet looks for archives inside the extracted files, including obfuscated ones that it recognizes by their RAR or 7-Zip header, and unpacks them too, up to five levels deep. This replaces the Extended Unpacker and Extended Unrar extensions.

Permissions
: Unpacked files get mode `0644`, or `0755` when they're executable, and folders get `0755`, never more open than the folder they were unpacked into. This replaces the Permission Fix extension.

More copies
: [Donor search](dupesearch.md) finds, checks, and queues other postings of a download through NZBHydra2. This replaces the DupeDonors extension.

Code inherited from NZBGet's extension system, such as the Extension Manager in the web interface and the extension API methods, is left over and on its way out.
