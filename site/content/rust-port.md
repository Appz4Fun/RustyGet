+++
title = "Rust port status"
nav_title = "Rust port"
description = "How RustyGet moved from C++ to Rust one module at a time, which modules are in Rust, and the C++ that remains."
eyebrow = "Reference"
generated = "porting"
lede = "RustyGet replaces NZBGet's C++ with Rust one module at a time, while the program stays in daily use. Each step keeps the behavior the same, so the rewrite never trades correctness for progress. Almost all of it is done."
+++

## How the port works

The Rust code lives in `rust/` as a library crate, `nzbget_rs`, that links into the `rustyget` binary. The remaining C++ calls it through the C interface declared in `rust/include/nzbget_rs.h`. Module by module, the program moved behind that interface, and today the C++ is little more than glue.

<ol class="steps">
<li><strong>Pick a module.</strong> Hot paths first, measured with real downloads. Whole modules are preferred over single functions.</li>
<li><strong>Port it faithfully.</strong> The Rust code reproduces the old behavior, quirks included.</li>
<li><strong>Prove it.</strong> While the C++ still existed, differential tests ran the old C++ and the new Rust side by side on the same inputs. Those inputs included random and mutated data. Some of those harnesses remain in the crate's `tests` folder.</li>
<li><strong>Review it.</strong> Each port is a pull request with an automated review, fixed until it's clean.</li>
<li><strong>Ship it.</strong> The port lands as one signed commit and goes into daily use.</li>
</ol>

When a port finds a bug in the old code, the port keeps the bug, and the fix follows as a separate commit. Bugs found this way include an error message that glibc left empty, and a RAR header parser that kept going on a block that doesn't advance. New features, such as twin detection, exist only in Rust.

## What's left

On the main branch, about 217,000 lines of Rust in `rust/src/` do the work. What remains in C++:

| Code | Lines | What it is |
|---|---|---|
| `daemon/main/rustyget.cpp` | 320 | The program entry point, which hands over to Rust |
| `daemon/util/Util.h` | 4,950 | Glue: the C++ classes and callbacks that still wrap Rust objects |
| `rust/include/*.h` | 472 | C declarations of the Rust interface |
| `tests/` | 796 | The daemon test runner and its test suites |
| `rust/tests/*.cpp` | | Differential and ownership test harnesses |

So the build still needs a C++20 compiler. Several dependencies are already gone, among them CMake, Boost, PAR2-Turbo, libxml2, zlib, ncurses, and the external `unrar` and `7z` programs. The binary links OpenSSL for TLS, and the Rust `zstd` crate builds the C zstd library. The macOS app in `platforms/macos/` is Objective-C.

The detailed inventories in the repository, [the migration status](https://github.com/Appz4Fun/RustyGet/blob/main/docs/RUST_MIGRATION_STATUS.md), [the caller boundary map](https://github.com/Appz4Fun/RustyGet/blob/main/docs/rust-migration/BOUNDARY_MAP.md), and [the dependency retirement audit](https://github.com/Appz4Fun/RustyGet/blob/main/docs/rust-migration/DEPENDENCY_RETIREMENT.md), date from before most of the C++ was removed, so they list far more open work than remains.

## Modules in Rust

This table is generated from the crate's source on every build. Each module's first doc comment says what it does. The `ffi` module is the C interface that the C++ code calls.

<!-- modules -->

## Porting notes

The notes below come from `rust/PORTING.md` in the repository. They were written during the first ports, when the Rust code built through CMake and every C++ routine stayed as a fallback, so their rules describe that stage. Their table of finished ports lists the measured speed-ups; the module table on this page lists all current Rust modules.

<!-- porting -->
