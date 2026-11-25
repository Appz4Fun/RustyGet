+++
title = "Rust port status"
nav_title = "Rust port"
description = "How RustyGet moved from C++ to Rust one module at a time, which modules are in Rust, and the C++ that remains."
eyebrow = "Reference"
generated = "porting"
lede = "RustyGet is written in Rust. This page records how the code moved to Rust one module at a time while the program stayed in daily use, each step keeping the behavior the same."
+++

## How the port works

The Rust code lives in `rust/`: a library crate and the `rustyget` binary built from it by Cargo. Module by module, the program moved from C++ into Rust behind a C interface, and once the last module moved, the C++ and that interface were deleted.

<ol class="steps">
<li><strong>Pick a module.</strong> Hot paths first, measured with real downloads. Whole modules are preferred over single functions.</li>
<li><strong>Port it faithfully.</strong> The Rust code reproduces the old behavior, quirks included.</li>
<li><strong>Prove it.</strong> While the C++ still existed, differential tests ran the old C++ and the new Rust side by side on the same inputs. Those inputs included random and mutated data. Some of those harnesses remain in the crate's `tests` folder.</li>
<li><strong>Review it.</strong> Each port is a pull request with an automated review, fixed until it's clean.</li>
<li><strong>Ship it.</strong> The port lands as one signed commit and goes into daily use.</li>
</ol>

When a port finds a bug in the old code, the port keeps the bug, and the fix follows as a separate commit. Bugs found this way include an error message that glibc left empty, and a RAR header parser that kept going on a block that doesn't advance. New features, such as twin detection, exist only in Rust.

## What's left

On the main branch, about 222,000 lines of Rust in `rust/src/` do the work, and no C++ is left: Cargo builds the `rustyget` binary from Rust alone, with `rust/entry.rs` as its entry point. The last C++ files, the entry shim `daemon/main/rustyget.cpp`, the glue header `daemon/util/Util.h` and the C headers in `rust/include/`, are gone.

The build needs no C++ compiler. Several dependencies are already gone, among them CMake, Boost, PAR2-Turbo, libxml2, zlib, ncurses, and the external `unrar` and `7z` programs. The binary links OpenSSL for TLS, and the Rust `zstd` crate builds the C zstd library.

The detailed inventories in the repository, [the migration status](https://github.com/Appz4Fun/RustyGet/blob/main/docs/RUST_MIGRATION_STATUS.md), [the caller boundary map](https://github.com/Appz4Fun/RustyGet/blob/main/docs/rust-migration/BOUNDARY_MAP.md), and [the dependency retirement audit](https://github.com/Appz4Fun/RustyGet/blob/main/docs/rust-migration/DEPENDENCY_RETIREMENT.md), date from before most of the C++ was removed, so they list far more open work than remains.

## Modules in Rust

This table is generated from the crate's source on every build. Each module's first doc comment says what it does. The `ffi` module began as the C interface that the C++ code called.

<!-- modules -->

## Porting notes

The notes below come from `rust/PORTING.md` in the repository. They were written during the first ports, when the Rust code built through CMake and every C++ routine stayed as a fallback, so their rules describe that stage. Their table of finished ports lists the measured speed-ups; the module table on this page lists all current Rust modules.

<!-- porting -->
