# CPU tiers and SIMD

RustyGet supports every CPU from an Intel Coffee Lake desktop (8th gen, e.g. i7-8600K /
i7-8700K: AVX2, FMA, BMI2, no AVX-512) and an Apple M4 MacBook Air/Pro upwards. Anything
older is not supported. Code may use newer instruction sets (AVX-512, GFNI, VPCLMULQDQ,
ARMv8 crypto, Apple M4/M5 extras) when that is proven faster, but every build must still
run on that floor.

## Two layers

1. **Runtime dispatch inside every binary.** Hot loops have a scalar reference version and
   vector versions chosen once at runtime from what the CPU reports
   (`is_x86_feature_detected!`, NEON on aarch64). This is where the measured gains are.
   Example: the yEnc body decoder from nntp-rs (`yenc_decode_piece`): a vector break scan
   finds runs free of line starts and escapes at line ends (AVX-512BW, AVX2, NEON, scalar),
   a payload kernel decodes each run (AVX-512 VBMI2, AVX2, SSSE3, NEON, scalar), and a line
   kernel decodes short lines (SSSE3, SSE2, NEON, or the state machine alone). The x86
   payload kernels need POPCNT (without it, the scalar one), and BMI1/BMI2 for AVX2 and
   AVX-512 (without them, at most SSSE3). `RUSTYGET_SIMD` (or nntp-rs's own `NNTP_RS_SIMD`, which wins when both
   are set) caps them, for benchmarks or to rule a kernel out on a misbehaving host:
   `scalar` runs the scalar break scan and payload kernel and the state machine alone (no
   line kernel); `sse2` runs the scalar payload kernel and, on x86, the SSE2 line kernel (on
   arm the state machine alone) but leaves the break scan alone; `avx2` stops the x86 break scan and payload kernel at AVX2 and
   leaves the line kernel alone. On arm, nntp-rs v0.4.0's NEON break scan ignores the cap.
   It is read once, at the first decode; `decoder::tests::simd_cap_is_honoured` checks it in
   a process of its own. nntp-rs tests and fuzzes every kernel against the state machine.
2. **Release builds per CPU tier.** `just release` builds for the floor
   (`-C target-cpu=x86-64-v3` on x86_64, `apple-m4` on Apple silicon; other architectures
   use the compiler default). `just release-tiers` also builds the newer tiers into
   `build/release-<cpu>/`: today `x86-64-v4` (AVX-512: AMD Zen 4/5 on AM5, Xeon Sapphire
   Rapids and newer). `just build release 3 <cpu>` builds any single tier; only `x86-64-v3`, `x86-64-v4` and
   `apple-m4` get the startup tier check below. Debug and test
   builds keep the compiler default, so they run on any CI machine.

The `rustyget` .deb ships `/usr/lib/rustyget/rustyget-default` (the floor build) and, on
amd64, `/usr/lib/rustyget/rustyget-x86-64-v4`. `/usr/bin/rustyget` is a small launcher
that execs the v4 build when `/proc/cpuinfo` lists AVX-512 F/BW/CD/DQ/VL and the floor
build otherwise. `RUSTYGET_CPU_LEVEL=default` (or another installed tier name) forces one.
The process name is therefore `rustyget-default` or `rustyget-x86-64-v4` (use
`systemctl` or `pgrep -f rustyget`, not `pidof rustyget`), and `AppDir` is `/usr/lib/rustyget`.

A tier binary started on a CPU below its tier stops at once with a message naming the
build to use, instead of crashing on an illegal instruction. The check lives in
`rust/tiercheck.rs`, which `rust/build.rs` compiles on its own for the compiler's default
CPU, telling it the tier only as `--cfg rustyget_tier="..."`, and links into the binary
that `rust/entry.rs` starts: with `target-cpu` set, std's feature-detection macros answer
"yes" at compile time and a check could never fire.

No `apple-m5` tier yet: in the current LLVM it adds only MTE over `apple-m4`, which
RustyGet does not use.

## Adding a fast path

- Keep the scalar version as the reference and the fallback.
- Pick the vector version once at runtime; never require a feature above the floor
  without a check.
- Test every version against the scalar one on inputs that put each interesting byte
  pattern at every lane and block boundary.
- Benchmark through the real caller (for example `cargo bench --bench yenc_decode`).
  A path you can only run under an emulator (Intel SDE, qemu) is proven correct there,
  not faster: give the expected gain from instruction counts or published throughput and
  flag it for a timing run on real hardware.
- par2 math and archive code belong to par-rs and zipfast; ask those libraries for
  their kernels rather than adding copies here.

## Measurements (2026-11-26): nntp-rs v0.4.0 decoder

Same bench and method as below (plan nya-bench-3): `cargo bench --bench yenc_decode`,
750,000-byte part, MB/s of encoded input, each process the best of 5 inner rounds, median of
5 independent processes per build and variant, order rotated. "Before" is main dbe9a86b,
"nntp-rs" is Step A on nntp-rs v0.4.0. Host: an Intel Xeon Platinum 8581C farm host
(AVX-512 BW and VBMI2) that other jobs shared (1-minute load 40-50), so single samples
spread; raw output, plan, input and bench-binary hashes on the host under
`~/nya-bench-3/20261126T122540Z`.

| Read size | x86 before | x86 nntp-rs | x86 before, cap avx2 | x86 nntp-rs, cap avx2 |
|---|---|---|---|---|
| 4 KiB | 2,766 | 4,208 | 2,550 | 2,850 |
| 16 KiB | 3,024 | 4,347 | 2,578 | 3,104 |
| 64 KiB | 2,760 | 3,619 | 2,778 | 2,890 |
| whole part | 1,133 | 1,271 | 1,155 | 1,155 |

On x86 nntp-rs v0.4.0 runs at about 1.3-1.5x of the path it replaced at 4-64 KiB reads
(1.1x on the whole part). With both capped to AVX2 it is 1.0-1.2x (1.0x on the whole part):
most of the AVX-512 host's gain comes from its AVX-512 kernels (the cap drops both the
AVX-512BW break scan and the VBMI2 payload kernel, so this run cannot tell them apart). The arm64 run of
the same plan (`~/nya-bench-3/20261126T122541Z` on an arm64 farm host at load ~35) spread too
widely to conclude anything (4 KiB samples from 844 to 1,662 MB/s); a run on a quiet arm64
host is still to do.

On AVX2-only CPUs, where the floor is, two runs outside plan nya-bench-3 found more:
nntp-rs reports +40-70% at 4-64 KiB reads on an i7-8700K for v0.4.0 (its own run of this
bench), and a native build of this bench on a Zen 3 AVX2 farm host (rustyget-avx2-1) measured
+53-57% at 4/16/64 KiB (+27% on the whole part) for v0.4.0 (rev d475ec1) against main
dbe9a86b (recorded in the 2026-11-26 coordination status log; no raw samples kept with this
document). Not measured on an M4 here.

## Measurements (2026-11-26, superseded): nntp-rs v0.3.2 decoder

yEnc article decode through the real `Decoder` (framing, dot-unstuffing, body kernel,
CRC-32), `cargo bench --bench yenc_decode`, 750,000-byte part, MB/s of encoded input. Each
process reports the best of 5 inner rounds; the table gives the median of 5 independent
processes per build and variant (plan nya-bench-2; farm hosts shared with other jobs; raw
per-process output, plan, input and bench-binary hashes kept on the farm hosts under
`~/nya-bench-2/20261126T111729Z` (x86) and `~/nya-bench-2/20261126T111730Z` (arm64)).
"Before" is the nntmux-yenc path with RustyGet's break scan (main dbe9a86b), "nntp-rs" is
nntp-rs v0.3.2 (`yenc_decode_piece`).

| Read size | x86 before, AVX-512 | x86 before, capped to AVX2 | x86 nntp-rs (SSSE3) | arm64 before | arm64 nntp-rs (NEON) |
|---|---|---|---|---|---|
| 4 KiB | 4,798 | 4,507 | 2,963 | 1,851 | 2,299 |
| 16 KiB | 5,091 | 4,740 | 3,069 | 1,899 | 2,340 |
| 64 KiB | 4,378 | 4,187 | 2,868 | 1,812 | 2,185 |
| whole part | 1,859 | 1,802 | 1,516 | 1,072 | 1,147 |

x86: Intel Xeon Platinum 8581C. arm64: CPU part 0xd8e. On x86 nntp-rs runs at about
0.6-0.7x of the path it replaced at 4-64 KiB reads (0.8x on the whole part); on arm64 about
1.2x at 4-64 KiB reads (1.07x on the whole part).
Not measured on the floor CPUs (an i7-8700K, an M4) yet.

## Measurements (2026-11-16, superseded)

These describe RustyGet's own break scan (AVX2, AVX-512BW), which the nntp-rs decoder
replaced; kept for the record.

yEnc article decode through the real `Decoder` (framing, dot-unstuffing, body kernel,
CRC-32), 750,000-byte part, one core of a 2.3 GHz Xeon with AVX-512, MB/s of encoded input:

| Read size | Before (scalar scan) | AVX2 scan | AVX-512BW scan |
|---|---|---|---|
| 4 KiB (the default `ArticleReadChunkSize`) | 1,214 | 3,474 | 3,962–4,256 |
| 16 KiB | 1,227 | 3,916 | 4,376–4,524 |
| 64 KiB | 1,198 | 3,774 | 3,922–3,934 |

Building the same code for `x86-64-v3` or `x86-64-v4` instead of the generic target made no
difference outside noise on this decode bench or on the `criterion_ports` JSON/Base64
kernels: the gain comes from the dispatched kernel, not from the target-cpu flag.
