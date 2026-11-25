+++
title = "RustyGet"
nav_title = "Overview"
template = "home"
toc = false
description = "RustyGet is a Usenet downloader written in Rust. It treats every copy of a release as one fleet, so missing articles come from other copies instead of failing the download."
eyebrow = "Usenet downloader · written in Rust"
headline = "Every copy of a release,"
headline_accent = "working as one download."
lede = "RustyGet is a Usenet downloader written in Rust, built around duplicate fleets: when an article is missing on every server, it comes from another copy of the release, checked before it counts, instead of failing the download."
facts = ["Works with Sonarr, Radarr and Kodi", "Live par2 repair and unpacking", "Borrowed articles checked", "GPL v2 or later"]
+++

## Start here

<div class="cards">
<a class="card" href="install.html"><p class="eyebrow">Get started</p><h3>Install and build</h3><p>Compile from source with Just and the pinned Rust toolchain, then run it as a service.</p></a>
<a class="card" href="quick-start.html"><p class="eyebrow">Get started</p><h3>Quick start</h3><p>Five settings to start downloading, and five more to turn on duplicate fleets.</p></a>
<a class="card" href="duplicate-fleets.html"><p class="eyebrow">Features</p><h3>Duplicate fleets</h3><p>Article fallback, twins and alts, par2 from a twin, stream repair, and early failover.</p></a>
<a class="card" href="kodi.html"><p class="eyebrow">Features</p><h3>Streaming with Kodi</h3><p>How the NeNeTeePee Stream add-on finds every copy through NZBHydra2 and hands the fleet to RustyGet.</p></a>
<a class="card" href="why.html"><p class="eyebrow">Overview</p><h3>Why RustyGet</h3><p>What duplicate fleets solve, why Rust, and how RustyGet fits your tools.</p></a>
</div>

## What it does

RustyGet downloads files from Usenet using NZB files: it fetches articles from your news servers, decodes them, repairs the result with par2 while the download arrives, and unpacks archives as they come in. It runs on anything from a small NAS to a large server, and media managers such as Sonarr and Radarr connect to it unchanged.

What's new is how it treats duplicates. The same release usually exists as several postings: reposts, listings on other indexers, and other encodes. RustyGet uses them together:

- An article missing on every server is fetched from a matching file in another copy. A yEnc article must fit its place in the file, and, where they're available, it's checked against its checksum (with the default `CrcCheck=yes`) and the download's par2 data.
- Copies with byte-identical files, called **twins**, are proven by par2 checksums and can lend articles and recovery volumes. Other encodes, called **alts**, mostly serve as failover backups.
- Holes that articles can't fill are patched with verified bytes, even from copies that are split, packed, or encrypted differently.
- A copy that can't complete hands over to the best backup within seconds.
- RustyGet checks each copy a fleet brings on your servers before it counts on it.

See [Feature highlights](features.md) for the full list.

## Status

RustyGet is in active development and runs in daily use. It is written entirely in Rust. There are no release packages yet, and the API may change as development goes on. Follow progress in the [Rust port status](rust-port.md) and the [repository](https://github.com/Appz4Fun/RustyGet).
