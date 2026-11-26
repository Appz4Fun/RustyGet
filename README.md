<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="site/assets/wordmark-dark.svg">
    <img src="site/assets/wordmark-light.svg" alt="RustyGet" width="520">
  </picture>
</p>

<p align="center">
  <a href="https://rustyget.xbmc4lyfe.com/"><img alt="Documentation" src="https://img.shields.io/badge/docs-rustyget.xbmc4lyfe.com-a8400f"></a>
  <img alt="Rust 1.99.0" src="https://img.shields.io/badge/rust-1.99.0-1d6f60">
  <a href="COPYING"><img alt="License: GPL v2 or later" src="https://img.shields.io/badge/license-GPL--2.0--or--later-3d5a73"></a>
</p>

RustyGet is a Usenet downloader written in Rust. It treats every copy of a release as one **fleet**: an article that's missing on every news server comes from another copy instead of failing the download, and when a copy can't complete, the next best one takes over within seconds.

**[Read the documentation](https://rustyget.xbmc4lyfe.com/)**

## Contents

- [What RustyGet does](#what-rustyget-does)
- [How deduplication works](#how-deduplication-works)
- [Built in Rust](#built-in-rust)
- [Build and run](#build-and-run)
- [Works with your tools](#works-with-your-tools)
- [Documentation](#documentation)
- [Contribute](#contribute)
- [License](#license)

## What RustyGet does

| Area | RustyGet |
|---|---|
| An article missing on every server | Fetched from a matching file in another copy of the release. A borrowed yEnc article must fit its place in the file, and, where they're available, it's checked against its yEnc checksum and the download's par2 block checksums |
| Copies of a release | One fleet: copies lend each other articles, bytes, and par2 recovery volumes |
| Byte-identical copies | **Twins** proven by par2 checksums (MD5, 16 KB hash, and length of every file); the same video in other packaging is an **alt** |
| Not enough par2 blocks | Repairs with the fewest recovery volumes of a twin that cover the damage |
| Holes after the download | Stream repair patches verified bytes from other copies, even across RAR, ZIP, 7-Zip, raw splits, and encrypted RAR |
| PAR2 repair | With `LiveParRepair=yes`, runs while the download arrives and finishes as the last article lands |
| A download that can't complete | `HealthCheck=dupe` fails over within seconds, and a probe parks dead copies before they download |
| Clients with many copies | One `appendfleet` call: RustyGet checks every copy and downloads the most complete one |
| Unpacking | Built in, with no external programs |
| First start | Starts with no configuration file or a broken one, with a setup guide and Settings that check every value before saving |

## How deduplication works

The same release usually exists as many postings: reposts, listings on several indexers, and other encodes. RustyGet uses them together, and it pairs with the beta version of the [NeNeTeePee Stream](https://github.com/Appz4Fun/NeNeTeePee-Stream-Kodi) add-on for Kodi, which can use RustyGet as its download backbone.

### The add-on finds and deduplicates the fleet

1. **Search.** The add-on searches through [NZBHydra2](https://github.com/theotherp/nzbhydra2), the recommended Newznab front end, which puts all of your indexers behind one API. It searches by IMDb or TVDB ID first and by title as a fallback, alongside Prowlarr and direct indexers if you use them.
2. **Rank by quality.** It parses each release name and ranks by resolution, HDR format, remux, preferred release-group tiers, audio format, and size. You pick a release, or let the add-on pick the top one.
3. **Gather every copy.** It collects every row that's the same release as the pick: the same name, or a parsed name that agrees on title, year or episode, group, resolution, codec, HDR, audio, edition, and language. It also adds other uploads with the same title from NZBHydra2's answer, including ones your filters hid.
4. **Drop the same posting twice.** Rows with the same size, posted within 120 seconds of each other, are one listing seen through several indexers. After it downloads the NZB files, the add-on compares fingerprints of every article's Message-ID; NZB files that share more than 1% of their articles are the same posting, and only one stays. A re-upload that shares none is kept, because it's the strongest backup there is.
5. **Send the fleet.** It sends up to 50 copies in one [`appendfleet`](https://rustyget.xbmc4lyfe.com/api.html#appendfleet) call under a duplicate key built from the content ID and the release name.

### RustyGet picks the copy and keeps the rest working

1. **Check every copy on your servers.** RustyGet checks a sample of each posting's articles on all of your news servers at once, and downloads a few in full to verify their checksums. It marks copies of the same posting that the add-on couldn't see.
2. **Rank.** Copies that are alive come first, then unknown, then dead. Within a group, the most complete copy wins, then the easiest one to borrow from: plain or split files, then stored archives, then compressed, then encrypted.
3. **Queue the best, keep the rest.** The top copy downloads, and the others wait in Downloads as a collapsed fleet, in the order they'd take over, with their measured health.
4. **Borrow, repair, fail over.** During the download, a missing article comes from a matching file in another copy. Holes are patched from verified bytes. If the copy can't complete, the best backup takes over within seconds, and the add-on follows the duplicate key to the new download.

The add-on decides *which release* you get. RustyGet decides *which copy* of it downloads, because only your news servers know which copies still exist.

For more detail, see [Duplicate fleets](https://rustyget.xbmc4lyfe.com/duplicate-fleets.html), [Streaming with Kodi](https://rustyget.xbmc4lyfe.com/kodi.html), and [NZBHydra2](https://rustyget.xbmc4lyfe.com/nzbhydra2.html).

## Built in Rust

RustyGet has no external helper programs. Archives, parity, yEnc, compression and XML are handled in-process by Rust libraries:

- **[zipfast](https://github.com/xbmc4lyfe/zipfast) and zipview:** RAR (1.5 through 5.0, multi-volume, encrypted data and headers), 7-Zip (including split `.7z.001`), ZIP, TAR, Gzip, Bzip2, XZ and Zstandard, extracted while the download arrives, with no `unrar` or `7z` executables. Nested archives, obfuscated archive names and file permissions are handled in the unpack pipeline.
- **[par-rs](https://github.com/xbmc4lyfe/par-rs)** handles par2 verification, repair while downloading, Reed-Solomon repair and the renaming of obfuscated files.
- **SIMD yEnc, `crc32fast`, `flate2` (`zlib-rs`) and `quick-xml`.**

RustyGet targets macOS (Apple Silicon) and x86_64 Linux. Every build runs on an Intel Coffee Lake desktop and an Apple M4; newer CPU instructions are used where a benchmark proves them faster.

## Build and run

RustyGet runs on Linux and macOS. It has no release packages yet, so build it from source with `just` and Rust (plus a C compiler and linker, which Rust uses to link and to build the bundled zstd library). The repository pins Rust 1.99.0, and rustup installs it on the first build.

```bash
git clone https://github.com/Appz4Fun/RustyGet.git rustyget
cd rustyget
[ -e ~/.rustyget ] || install -m 600 rustyget.conf ~/.rustyget
just run ~/.rustyget
```

`just run <config>` builds the release binary and starts it with that configuration file, which the line before copies from the template on the first run. Then open `http://127.0.0.1:6789` on the same computer: the template listens on `127.0.0.1`, and the default login is `rustyget` with the password `tegbzn6789`. Change the password first, and set `ControlIP` to reach RustyGet from other devices. Started with no configuration file at all, `rustyget -D` creates one with a random password and prints it once. See [Install and build](https://rustyget.xbmc4lyfe.com/install.html) for the libraries each platform needs, and [Quick start](https://rustyget.xbmc4lyfe.com/quick-start.html) for the first settings.

## Works with your tools

RustyGet answers the widely supported NZBGet-compatible JSON-RPC API, so Sonarr, Radarr, Prowlarr and the Kodi add-on connect to it unchanged: in those apps, add RustyGet as an **NZBGet** download client. RustyGet adds `appendfleet` and new fields, runs methods that change something only by POST, and checks access in multicalls. An existing `nzbget.conf` and queue directory are read as they are. Future versions may also add a SABnzbd-compatible API. See [API and compatibility](https://rustyget.xbmc4lyfe.com/api.html).

## Documentation

The **[documentation site](https://rustyget.xbmc4lyfe.com/)** covers:

- [Feature highlights](https://rustyget.xbmc4lyfe.com/features.html)
- [Duplicate handling](https://rustyget.xbmc4lyfe.com/duplicates.html) and [Duplicate fleets](https://rustyget.xbmc4lyfe.com/duplicate-fleets.html)
- [Check, repair, and unpack](https://rustyget.xbmc4lyfe.com/post-processing.html), [Servers, RSS, and scheduling](https://rustyget.xbmc4lyfe.com/automation.html), and [Performance tuning](https://rustyget.xbmc4lyfe.com/performance.html)
- [Configuration options](https://rustyget.xbmc4lyfe.com/options.html), generated from `rustyget.conf`
- [API and compatibility](https://rustyget.xbmc4lyfe.com/api.html) and the [Design system](https://rustyget.xbmc4lyfe.com/design-system.html)

[RustyGet Observe](tools/nzbget-observe/README.md) is an opt-in Rust watcher that tracks job progress, records Linux resource observations, and saves bounded evidence when an active job stops showing progress.

The site's source is in [`site/`](site), with the prose checked by Vale against the Google developer documentation style.

## Contribute

Issues and pull requests are welcome in this repository. Write new code in Rust, keep ports byte-identical with the code they replace, and add tests that fail without your change. AI-assisted work is welcome if you say which tools you used and understand every line. See [Contributing](https://rustyget.xbmc4lyfe.com/contributing.html).

## License

RustyGet is free software under the [GNU General Public License](COPYING), version 2 or, at your option, any later version. The copyright notices of everyone whose work it contains stay in the source files.

The binaries also link libraries under other licences, listed with their authors on the License page of the web interface. Among them: nntp-rs (article decoding) under the GNU Affero General Public License, version 3 or later, which adds terms for software that users reach over a network; zipview under the GPL, version 3; and zipfast under the RPL-1.5.
