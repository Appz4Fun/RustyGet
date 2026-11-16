#!/bin/sh
# /usr/bin/rustyget: runs the newest CPU tier build of RustyGet this host supports.
# x86-64-v4 (AVX-512: Zen 4/5, Xeon Sapphire Rapids and newer) when present and
# supported, otherwise the floor build. RUSTYGET_CPU_LEVEL=<tier> forces a tier.
dir=/usr/lib/rustyget
level=${RUSTYGET_CPU_LEVEL:-}
if [ -z "$level" ]; then
    level=default
    if [ -x "$dir/rustyget-x86-64-v4" ] && [ -r /proc/cpuinfo ]; then
        flags=" $(grep -m1 '^flags' /proc/cpuinfo | cut -d: -f2) "
        level=x86-64-v4
        for f in avx512f avx512bw avx512cd avx512dq avx512vl; do
            case "$flags" in
                *" $f "*) ;;
                *) level=default ;;
            esac
        done
    fi
fi
exec "$dir/rustyget-$level" "$@"
