#!/bin/sh
# Fails unless this machine really has the instruction-set tier a CI runner
# label claims (docs/CPU_TIERS.md, "CI tiers"). A runner that lacks a flag
# would silently run the fallback kernels and pass, so this is a failure,
# never a skip. Usage: scripts/ci/isa-assert.sh avx512|avx2|neon|apple-m
set -eu
tier=${1:?usage: isa-assert.sh avx512|avx2|neon|apple-m}

# x86-64-v3, the release floor (what rust/tiercheck.rs checks for the v3 build),
# plus what the dispatched kernels test for (popcnt, ssse3, sse4.1, pclmulqdq).
v3="sse2 ssse3 sse4_1 popcnt pclmulqdq avx avx2 bmi1 bmi2 fma abm movbe f16c"
case $tier in
avx512)
    # avx512f..vl: the x86-64-v4 build and its launcher check; avx512bw and
    # avx512_vbmi2: the nntp-rs yEnc kernel; gfni with avx512bw: par-rs GF(2^16).
    need="$v3 avx512f avx512bw avx512cd avx512dq avx512vl avx512_vbmi2 gfni"
    absent="" os=Linux arch=x86_64 ;;
avx2)
    # The Coffee Lake floor: AVX2 without AVX-512 or GFNI, so the AVX2 kernels
    # are the ones chosen and the x86-64-v4 build must refuse to start.
    need="$v3" absent="avx512f gfni" os=Linux arch=x86_64 ;;
neon)
    # aarch64 Linux: NEON kernels (nntp-rs, par-rs, zipfast BLAKE2sp) and the
    # crypto extensions par-rs and zipfast dispatch on (CRC-32, PMULL, AES, SHA-2).
    need="asimd aes pmull sha2 crc32" absent="" os=Linux arch=aarch64 ;;
apple-m)
    # Apple M4 or newer: the apple-m4 build's check (I8MM, BF16) plus the above.
    need="neon FEAT_AES FEAT_PMULL FEAT_SHA256 FEAT_CRC32 FEAT_I8MM FEAT_BF16"
    absent="" os=Darwin arch=arm64 ;;
*)
    echo "isa-assert: unknown tier '$tier' (avx512, avx2, neon, apple-m)" >&2
    exit 2 ;;
esac

fail=0
[ "$(uname -s)" = "$os" ] || { echo "isa-assert: tier $tier needs $os, this is $(uname -s)" >&2; fail=1; }
[ "$(uname -m)" = "$arch" ] || { echo "isa-assert: tier $tier needs $arch, this is $(uname -m)" >&2; fail=1; }
[ $fail -eq 0 ] || exit 1

has() {
    if [ "$os" = Darwin ]; then
        [ "$(sysctl -n "hw.optional.arm.$1" 2>/dev/null || sysctl -n "hw.optional.$1" 2>/dev/null)" = 1 ]
    else
        case " $flags " in *" $1 "*) return 0 ;; *) return 1 ;; esac
    fi
}
if [ "$os" = Linux ]; then
    # The kernel lists only the features it enabled (AVX-512 state included).
    flags=$(grep -m1 -E '^(flags|Features)[[:space:]]*:' /proc/cpuinfo | cut -d: -f2)
    model=$(grep -m1 -E '^model name' /proc/cpuinfo | cut -d: -f2 | sed 's/^ *//')
    [ -n "$model" ] || model="implementer $(grep -m1 '^CPU implementer' /proc/cpuinfo | cut -d: -f2) part $(grep -m1 '^CPU part' /proc/cpuinfo | cut -d: -f2)"
else
    model=$(sysctl -n machdep.cpu.brand_string)
fi

missing="" extra=""
for f in $need; do has "$f" || missing="$missing $f"; done
for f in $absent; do ! has "$f" || extra="$extra $f"; done
echo "isa-assert: tier=$tier cpu=$model"
if [ -n "$missing" ] || [ -n "$extra" ]; then
    [ -z "$missing" ] || echo "isa-assert: FAIL: tier $tier needs$missing, which this CPU lacks" >&2
    [ -z "$extra" ] || echo "isa-assert: FAIL: tier $tier must not have$extra (the label would test the wrong kernels)" >&2
    exit 1
fi
# Informational: features beyond the tier that change which kernel a library
# picks (Coffee Lake has none of these; Zen 3 and newer have SHA-NI and VAES).
for f in sha_ni vaes vpclmulqdq avx512vbmi sha3 sve2 i8mm bf16 FEAT_SHA3 FEAT_SME; do
    if has "$f"; then extra="$extra $f"; fi
done
echo "isa-assert: PASS: tier $tier has $need${absent:+ and lacks $absent}"
[ -z "$extra" ] || echo "isa-assert: also present:$extra"
