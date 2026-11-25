+++
title = "Why RustyGet"
description = "What duplicate fleets solve, why RustyGet is written in Rust, and how it fits the tools you already use."
eyebrow = "Overview"
lede = "A release with a few missing articles shouldn't fail when another posting of it has every one. RustyGet is built around that idea: every copy of a release works as one download."
+++

## The problem with spares

The same release usually exists as many postings: reposts, listings on several indexers, and other encodes. A downloader that keeps them only as spares tries them one after another, each from scratch, and a release with a handful of missing articles fails even when another posting has all of them.

RustyGet treats the copies as one **fleet**:

- A missing article is fetched from a matching file in another copy, and checked before it counts.
- Byte-identical copies, **twins**, lend articles and par2 recovery volumes; the same video in other packaging, an **alt**, lends verified bytes through every container around it.
- A copy that can't complete hands over to the best backup within seconds, and nothing already downloaded is thrown away.

See [Duplicate fleets](duplicate-fleets.md) for how it works.

## Why Rust

- **Memory safety.** Duplicate fleets juggle many postings, threads, and partial files at once. Rust rules out use-after-free crashes and overlapping buffer copies.
- **Speed where it counts.** par2 repair can run while the download arrives (`LiveParRepair`), and archives unpack as their bytes land, with SIMD paths where a benchmark proves them faster.
- **One toolchain, no helpers.** Archives, parity, yEnc, compression and XML are handled in-process by Rust libraries, with no external `unrar`, `7z` or `par2` programs.
- **Tested behavior.** Every change is checked by unit tests and a functional harness with more than 100 end-to-end scenarios.

See [Rust port status](rust-port.md) for details.

## Works with your tools

- RustyGet answers the widely supported NZBGet-compatible JSON-RPC API, so Sonarr, Radarr and the Kodi add-on connect to it unchanged; in those apps, add it as an **NZBGet** download client.
- It adds `appendfleet`, so a client can hand over every copy of a release in one call.
- The API may grow where the old behavior is unsafe or in the way, and RustyGet may add a SABnzbd-compatible API. [API and compatibility](api.md) tracks both.
- **Packages** come later. Until then, [build from source](install.md).
