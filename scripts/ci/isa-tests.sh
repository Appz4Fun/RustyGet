#!/usr/bin/env bash
# The instruction-set tests for one CI tier (docs/CPU_TIERS.md, "CI tiers"):
# the `isa` workflow runs this on a runner labelled with the tier, and the same
# command proves a farm host of that tier. Every step runs; the summary at the
# end has one line per step and the exit status is nonzero if any failed.
#
#   scripts/ci/isa-tests.sh avx512|avx2|neon|apple-m [jobs]
#
# ISA_SKIP_FULL=1 skips the uncapped full `cargo test --tests` (when the same
# head already ran it on this machine), ISA_SKIP_DAEMON=1 the daemon smoke.
set -uo pipefail
tier=${1:?usage: isa-tests.sh avx512|avx2|neon|apple-m [jobs]}
jobs=${2:-3}
root=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_BUILD_JOBS=$jobs RUSTFLAGS="-D warnings"
unset NNTP_RS_SIMD RUSTYGET_SIMD PAR_RS_SIMD RUSTYGET_CPU_LEVEL

# The caps each tier runs, so every kernel level the hardware has runs somewhere:
# native = no cap (the best kernel), avx2 = at most AVX2, sse2/scalar = scalar.
case $tier in
avx512) caps="native avx2 scalar" ;;
avx2) caps="native scalar" ;;
neon | apple-m) caps="native scalar" ;;
*) echo "isa-tests: unknown tier '$tier'" >&2; exit 2 ;;
esac

# Test modules whose code reaches a dispatched kernel through the real caller:
# the yEnc Decoder (nntp-rs), CRC-32, Latin-1 (SSSE3), par2 verify/repair/create
# (par-rs GF(2^16), CRC-32, MD5) and archive reading and crypto (zipfast AES,
# SHA-256, CRC-32, BLAKE2sp, RAR5 filters).
filters=(decoder:: text:: crc:: articledl:: articlefetcher:: parchecker parlive:: repair::
    progressive_unpack archive_native rarreader:: unpack:: twin)

summary=()
status=0
step() {
    local name=$1 start rc
    shift
    start=$(date +%s)
    echo "::group::isa-tests $tier: $name"
    "$@"
    rc=$?
    echo "::endgroup::"
    local line
    line="isa-tests $tier: $name: $([ $rc -eq 0 ] && echo PASS || echo "FAIL (exit $rc)") ($(($(date +%s) - start))s)"
    echo "$line"
    summary+=("$line")
    [ $rc -eq 0 ] || status=1
}

with_cap() {
    local cap=$1
    shift
    (
        export RUSTYGET_CI_TIER="$tier"
        case $cap in
        native) ;;
        # par-rs has a single switch; the scalar run turns it off with the rest.
        scalar) export RUSTYGET_SIMD=scalar PAR_RS_SIMD=off ;;
        *) export RUSTYGET_SIMD="$cap" ;;
        esac
        "$@"
    )
}

cargo_t() { (cd "$root/rust" && cargo test --locked "$@"); }

# The tier binaries: a build for a CPU tier must start on a CPU of that tier and
# refuse, with the message from rust/tiercheck.rs, on one below it.
tier_binary() {
    local cpu=$1 expect=$2 bin out rc
    (cd "$root" && just build debug "$jobs" "$cpu") || return 1
    bin="$root/build/debug-$cpu/rustyget"
    out=$("$bin" --version 2>&1)
    rc=$?
    echo "$cpu --version: exit $rc: $out"
    case $expect in
    runs) [ "$rc" -eq 0 ] ;;
    refuses) [ "$rc" -eq 1 ] && [[ $out == *"built for newer CPUs: this CPU lacks avx512f"* ]] ;;
    esac
}

# The deb's /usr/bin/rustyget launcher picks the tier build from /proc/cpuinfo.
launcher() {
    local want=$1 dir got rc
    dir=$(mktemp -d "${TMPDIR:-/tmp}/isa-launcher.XXXXXX") || return 1
    sed "s|^dir=/usr/lib/rustyget\$|dir=$dir|" "$root/linux/pkg/deb-rustyget/rustyget-launcher.sh" >"$dir/launcher"
    grep -q "^dir=$dir\$" "$dir/launcher" || { echo "launcher: dir= line not found"; return 1; }
    for t in default x86-64-v4; do printf '#!/bin/sh\necho %s\n' "$t" >"$dir/rustyget-$t"; done
    chmod +x "$dir"/launcher "$dir"/rustyget-*
    got=$(sh "$dir/launcher")
    rc=0
    echo "launcher picked $got (want $want)"
    [ "$got" = "$want" ] || rc=1
    got=$(RUSTYGET_CPU_LEVEL=default sh "$dir/launcher")
    echo "launcher with RUSTYGET_CPU_LEVEL=default picked $got"
    [ "$got" = default ] || rc=1
    # The real builds behind it: whatever it picks must start here.
    cp "$root/build/debug-x86-64-v3/rustyget" "$dir/rustyget-default" &&
        cp "$root/build/debug-x86-64-v4/rustyget" "$dir/rustyget-x86-64-v4" &&
        sh "$dir/launcher" --version || rc=1
    rm -r "$dir"
    return "$rc"
}

smoke() {
    local s
    for s in complementary stream liveoverlap xpackzip; do
        python3 "$root/tests/functional/dupefallback/harness.py" \
            --nzbget "$root/build/debug/rustyget" --target local --scenario "$s" || return 1
    done
}

step "assert $tier flags" sh "$root/scripts/ci/isa-assert.sh" "$tier"
if [ $status -ne 0 ]; then
    printf '%s\n' "${summary[@]}"
    exit 1
fi
[ "${ISA_SKIP_FULL:-}" = 1 ] || step "cargo test --tests (no cap)" cargo_t --tests -- --test-threads="$jobs"
for cap in $caps; do
    step "kernels reached, cap=$cap" with_cap "$cap" cargo_t --test isa_tier -- --ignored --exact reaches_the_tier_kernels --nocapture
    step "kernel tests, cap=$cap" with_cap "$cap" cargo_t --lib --test parchecker -- --test-threads="$jobs" "${filters[@]}"
done
case $tier in
avx512)
    step "x86-64-v3 build starts" tier_binary x86-64-v3 runs
    step "x86-64-v4 build starts" tier_binary x86-64-v4 runs
    step "launcher picks x86-64-v4" launcher x86-64-v4 ;;
avx2)
    step "x86-64-v3 build starts" tier_binary x86-64-v3 runs
    step "x86-64-v4 build refuses" tier_binary x86-64-v4 refuses
    step "launcher picks default" launcher default ;;
apple-m)
    step "apple-m4 build starts" tier_binary apple-m4 runs ;;
esac
if [ "${ISA_SKIP_DAEMON:-}" != 1 ]; then
    step "build daemon" bash -c "cd '$root' && just debug '$jobs'"
    step "daemon smoke (no cap)" with_cap native smoke
    step "daemon smoke (cap=scalar)" with_cap scalar smoke
fi

echo
echo "== isa-tests $tier on $(uname -sm), head $(git -C "$root" rev-parse --short HEAD 2>/dev/null || echo unknown)"
printf '%s\n' "${summary[@]}"
[ -z "${GITHUB_STEP_SUMMARY:-}" ] || printf '    %s\n' "${summary[@]}" >>"$GITHUB_STEP_SUMMARY"
exit $status
