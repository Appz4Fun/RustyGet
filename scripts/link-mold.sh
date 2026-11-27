#!/bin/sh
# Cargo's linker for Linux builds (see .cargo/config.toml): the system C compiler
# driver, told to link with mold. mold links the test binaries and the daemon several
# times faster than GNU ld, and faster than the rust-lld Rust uses on x86_64 by default.
#
# A missing mold is an error, never a silent fallback to another linker, so a build
# host without it is noticed and fixed instead of quietly building slower.
if ! command -v mold >/dev/null 2>&1; then
    echo "error: RustyGet links Linux builds with mold, which is not installed." >&2
    echo "       Debian/Ubuntu: sudo apt install mold    Fedora: sudo dnf install mold" >&2
    echo "       Arch: sudo pacman -S mold    (see site/content/install.md)" >&2
    exit 1
fi
# -fuse-ld comes last so it overrides the -fuse-ld=lld rustc passes on x86_64.
exec "${RUSTYGET_LINK_CC:-cc}" "$@" -fuse-ld=mold
