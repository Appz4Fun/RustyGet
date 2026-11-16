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
   Example: the yEnc break scan in `rust/src/decoder/scan.rs` (AVX-512BW, AVX2, NEON,
   scalar). `RUSTYGET_SIMD=scalar` or `RUSTYGET_SIMD=avx2` caps the x86 kernel, for
   benchmarks or to rule a kernel out on a misbehaving host. Tests call every version
   directly and compare it with the scalar one.
2. **Release builds per CPU tier.** `just release` builds for the floor
   (`-C target-cpu=x86-64-v3` on x86_64, `apple-m4` on Apple silicon; other architectures
   use the compiler default). `just release-tiers` also builds the newer tiers into
   `build/release-<cpu>/`: today `x86-64-v4` (AVX-512: AMD Zen 4/5 on AM5, Xeon Sapphire
   Rapids and newer). `just build release 3 <cpu>` builds any single tier. Debug and test
   builds keep the compiler default, so they run on any CI machine.

The `rustyget` .deb ships `/usr/lib/rustyget/rustyget-default` (the floor build) and, on
amd64, `/usr/lib/rustyget/rustyget-x86-64-v4`. `/usr/bin/rustyget` is a small launcher
that execs the v4 build when `/proc/cpuinfo` lists AVX-512 F/BW/CD/DQ/VL and the floor
build otherwise. `RUSTYGET_CPU_LEVEL=default` (or another installed tier name) forces one.

A tier binary started on a CPU below its tier stops at once with a message naming the
build to use (`rust/src/cpulevel.rs`), instead of crashing on an illegal instruction.

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

## Measurements (2026-11-16)

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
