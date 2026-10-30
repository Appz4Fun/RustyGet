# Deferred PR validation

On 2026-10-12 the maintainer explicitly requested signed squash integration of
the open PRs before complete validation, with unfinished work recorded here.
These integrations do not assert review approval, correctness completion,
performance qualification, or deployment readiness. Preserve all earlier failed
checks and raw measurements. Do not mark unresolved review findings as fixed
solely because their PR was merged.

## PR #86 — caller-owned Latin1 conversion

Source snapshot: `bced81e17ab9df87abd7567e999ecd4ca41a1337`.

- [ ] Revisit production reachability: the active Rust-enabled filename and
  header callers bypass the optimized C++ compatibility wrapper. Existing
  wrapper benchmarks do not qualify those production callers.
- [ ] Complete matched whole-caller C++, fair optimized C++, and applicable
  current-Rust measurements before claiming a production speed improvement.
- [ ] Validate the final integrated source; this squash received source merge
  and whitespace checks only, with the full validation deferred by request.
- [ ] Keep backend deployment and performance qualification held.

Benchmarks and earlier evidence remain in `rust/tests/latin1_caller/`.

## PR #93 — optional progress observer

Source snapshot: `31e197612b3570e55ca9ea525a358e8ccdc06da7`.

- [ ] Run the prepared 37-test, strict Clippy and formatting checks on the
  consolidated repairs. Previous passing tests and source reviews apply to
  earlier snapshots; the five new regression checks failed on old code.
- [ ] Review the final transport failure metadata, unknown stages, progress
  bounds, PAR pause uncertainty, process counter units and formatting-only CI
  fixes, then respond to the still-open GitHub findings without claiming
  unexecuted fixes are validated.
- [ ] Follow `tools/nzbget-observe/TODO.md` for the remaining evidence and
  deployment checks. The source merge does not start the observer, exercise
  production RPC, or qualify its production overhead.
- [ ] Normalize the extra trailing blank line in the observer Cargo manifest
  during follow-up; the integration whitespace check reported it.

## PR #80 — native proxy discovery and watcher

Published source: `59f3408a32dc1ba9a6c4f6d72a9cad427be29fae`, with the
frozen held-member eligibility repair tree
`14fad1a2ecf575e780b2753d266cbf6798d6818d`.

- [ ] Fix the three unresolved Codex findings: infer whole size per file, pass
  inferred full data extent to PAR feasibility, and reschedule eligible old
  history picks after an interrupted SEARCHING checkpoint. These repairs are
  not implemented in this snapshot.
- [ ] Validate the included placement-eligibility checks before and after probe
  work. Only the original held-BAD failure was executed; the repaired code and
  FORCE, healthy and asynchronous BAD controls remain unexecuted.
- [ ] Complete the excluded size/PAR regression fixtures alongside their
  matching implementation. Private proof drivers and unfinished tests were
  preserved in the owner workspace, not added to this squash.
- [ ] Revisit slower and inconclusive complete-caller results and incomplete
  per-call I/O instrumentation. This latest source has not been timed.
- [ ] Complete integrated correctness checks and current-head review; leave
  outstanding review discussions truthful. Backend deployment remains held.

## PR #87 — stream repair prototypes

Published source: `5838c33b16d2c35aaa77b457d8500df51d1c7b5d`, updated
with the coherent hybrid-floor snapshot
`cdcf844ed22e4df71810b37696a53619e4df4569`.

- [ ] Follow [PR 87 deferred validation](#pr-87-deferred-validation). Dense PatchParts was about
  24.6% slower than fair C++; the old floor had 12 losing cases. The revised
  hybrid floor is not qualified: its first process had 741 of 8,784 retained
  rows below the required 5 ms, invalidating the packet and halting remaining
  runs. Preserve all results and do not replay that frozen plan.
- [ ] Validate the integrated hybrid, PatchParts, FFI and all registered
  benchmarks together. The benchmark-registration merge conflict was resolved
  by retaining both proxy and stream-compare cases; this composition is unbuilt.
- [ ] Complete integrated correctness, current-head review and new prospectively
  approved performance work. Do not infer qualification from source inclusion.
- [ ] Keep backend deployment held. The separate PAR proposal and private proof
  tools were not included in this immediate squash.

## PR #91 — benchmark infrastructure and Criterion

Latest composed source: `0a61ae913d29cb3220a73b78a672a5e5d870a2f4`,
including the publisher and both authored adapter retention repairs.

- [ ] Follow [PR91 validation and review follow-up](#pr91-validation-and-review-follow-up). Validate retained raw evidence against
  lexical, symlink traversal, target and inode aliases before cleanup or HOLD
  writes. The final two adapter repairs have syntax and source checks only.
- [ ] Run the paired adapter regressions and full combined Python suite. The
  attempted paired run stopped before runner launch when storage fell below
  its floor; zero tests ran. The earlier full run had 337 passes and two errors
  out of 339; the subsequent publisher/pipeline component run passed 111 tests.
- [ ] Review the complete integrated source and adjudicate open GitHub findings
  honestly. Historical whole-tree review defects and failed runs are retained;
  no final whole-tree CLEAN or exact-head approval is claimed for this squash.
- [ ] Validate the inherited runtime Rust changes and Criterion together. The
  older 502 snapshot's Rust results are historical and cannot qualify newly
  combined Latin1, proxy and stream-repair code.
- [ ] Supply the existing Bencher project identity and API key through the
  approved credential route before activating publishing. Activation remains
  incomplete; source merge does not imply publishing is enabled.
- [ ] Preserve backend deployment holds and do not treat unvalidated benchmark
  infrastructure as proof of a performance improvement.

## PR 87 deferred validation

The maintainer requested squash integration on 2026-10-12 before complete
validation. This records unresolved work; merge does not establish correctness,
performance qualification, review approval, or deployment readiness.

The source preserves PR 87 PatchParts and the corrected SelectProbeParts,
upgrades RequiredCompareFloor to the prepared hybrid/scratch revision 0d72a146,
and preserves existing module, ABI, and benchmark registrations. The floor uses
C++ for small inputs and Rust for nonempty inputs with at least 4096 holes.
PAR CRC production migration and private proof runners are outside this PR.

- [ ] Validate the final combined integrated source with remote Rust, daemon,
  CTest, sanitizer, differential, and mutation checks. Earlier component checks
  passed, but this exact composed tree has not been built or executed.
- [ ] Complete exact-head independent and GitHub review; prior component review
  does not approve the final combined squash. Preserve unresolved findings.
- [ ] Resolve PatchParts performance: the retained dense case was 24.6% slower
  than the matched C++ control. Do not claim a Rust speedup from this integration.
- [ ] Qualify the hybrid floor against production and improved C++ controls.
  The old all-Rust floor had 12 fair-control losses; its performance claims do
  not apply to this hybrid revision. The first new native process retained
  8784 rows, including 741 below the required 5ms duration. Its complete packet
  is INVALID/HOLD. Preserve raw evidence; no replay, filtering, recalibration,
  or replacement timing is authorized by this merge.
- [ ] Keep backend deployment held pending a separate release decision and
  source/artifact/install identity, configuration/queue preservation, and
  relevant live regression checks. No production deployment is asserted.

Historical component tests and reports remain in rust/tests/streamcompare.md
and the retained owner evidence. Unsupported platforms and final integrated
behavior remain unqualified until the appropriate checks actually execute.

## PR91 validation and review follow-up

The maintainer authorized squash integration before complete validation on
2026-10-12. This records remaining work rather than claiming final qualification.
Bencher activation, measured runtime performance, and backend deployment remain
subject to their existing holds.

### Evidence already available

- The earlier combined Python run at `f5f630ae` ran 339 tests: 337 passed and
  two helper-output contract checks errored. Its logs and source-bound receipts
  remain preserved. It is not a passing result for the integrated revision.
- The signed publisher repair at `10f30215` passed all 111 publisher and pipeline
  tests with zero skips, unchanged pipeline assertions, and stable captured
  inputs. Its existing publication peer completed a scoped source review with
  no findings. This is component evidence, not whole-tree clearance.
- The final earlier whole-tree review found retained-archive alias data loss in
  both RPC and postprocessing cleanup, plus the publisher helper-output issue.
  Candidate adapter repairs are included for follow-up verification; their
  presence alone does not establish remediation.
- The single retained-archive regression admission attempt stopped at its
  capacity check before runner or payload launch. No tests ran in that attempt;
  it is neither behavioral RED nor repaired GREEN evidence. The earlier Git
  bundle setup failure and the capacity failure remain separately preserved.
- Earlier runner and workflow repairs have scoped reviews and validation.
  Rust/Cargo validation at `502da6a6` remains historical evidence. The integrated
  default can include other runtime Rust changes, so those results do not clear
  the final integrated build. No new production speedup is established here.

### TODO after integration

- [ ] Collect the admitted retained-archive baseline regression run and bind its
  actual result, source/test hashes, raw logs and terminal lock-custody receipt.
  Keep setup failures separate from reproduced behavioral failures.
- [ ] Run the repaired RPC and postprocessing suites on the coordinated Linux
  worker. Verify every retained raw filename and lexical, intermediate symlink,
  resolved-target and inode alias before cleanup, derived writes and HOLD paths.
  Preserve existing cleanup of unaliased derived exports.
- [ ] Complete the existing adapter peers' reviews of the integrated repairs.
- [ ] Run `python3 -m unittest discover -s scripts/bench/tests -v` through the
  coordinated runner on the exact integrated source and record actual outcomes.
  Preserve the earlier failing evidence; do not substitute component passes.
- [ ] Complete a whole-tree review and a fresh GitHub Codex review of the signed
  integrated revision. Keep unresolved review findings visible until verified.
- [ ] Validate the final integrated Rust build on the coordinated worker. Recheck
  executable input equality before reusing any prior native results; inherited
  runtime changes require renewed affected validation.
- [ ] Verify workflow raw-artifact retention with an actual trusted workflow run.
  The existing artifact-selector check used a synthetic transport and model.
- [ ] Configure and observe trusted Bencher publication before declaring it
  active. Keep incomplete or failed qualification in HOLD.

The five original GitHub review threads cover descendant lock custody, actual
source-content binding, Cargo configuration overrides, explicit Rust
applicability, and bounded source capture. Source repairs and earlier checks
exist, but those threads must not be represented as current-head review approval
or resolved merely because integration was authorized.

## Fallback validation, 2026-10-13

This integration merges upstream `develop` at
`f81d3f4de58fbfae9e0657a7eb3041ff6ab5adfd` into the fork's
`dupe-article-fallback` head `cbda2e4f3f50d7e70c5eeaa25b2b37d779ca026c`.
The historical upstream PR 850 is closed; updating its source branch does not
reopen it.

### Completion regression

Healthy live downloads reproduced a queue stall after direct rename paused
the remaining PAR volumes. The name notification could arrive while their
last article requests were still active. Those requests subsequently drained
without completing a file, leaving no event to start post-processing.

The completion check now runs on both the name notification and the last
downloader's removal. It requires successful direct rename, completed output,
no active download work, no deletion, and no existing post-processing job.
Raw and skipped writes remain excluded. The queue lock protects both callers.
Six unit cases cover completion, remaining data, active requests, unfinished
rename, health deletion, and request draining.

The broad harness also reproduced a healthy backup never being downloaded
after a primary ended `FAILURE/HEALTH` at 96% health without usable parity.
`IsDupeSuccess` compared that result with the estimated 85% download threshold,
whereas the reported status used the actual parity threshold. With
`HealthCheck=dupe`, the success decision now uses that same actual threshold.
A red/green unit test covers the mismatch and retains successful PAR repair,
explicit good marks, and complete downloads as controls. The same-posting
functional case now downloads the healthy backup successfully.

An isolated fleet test repeatedly abandoned both a slow URL and a healthy URL
immediately. An instrumented diagnostic traced this to a size watchdog reading
a huge bogus size for an output file that did not exist yet. The POSIX
`FileSize` helper ignored `stat`'s return value and read its output on failure.
It now returns `-1` when `stat` fails. A red/green regression covers nonexistent
paths, nonexistent parents, `ENOTDIR`, and a known-size regular file. The fleet
test now honors its time budget and queues the healthy member.

The restricted JSON-P configuration test now owns its Options instance rather
than reading a global cleared by an earlier test. Two parked-file retry
fixtures use one connection so the retained volume finishes before health
cancellation. A history-recheck fixture disables early health cancellation
so both failed files reach history; it still requires all five failed articles
to be sampled without state-file errors. Their integrity assertions are unchanged.
Search fixtures check exact logical query/page coverage and at most two
transport attempts per page, matching Newznab's existing single retry. They
still require one logical search and no new requests after a cached result.
The empty-search restart fixture now requires another search after restart:
discovery without a usable backup intentionally remains retryable. Completed
backup memo preservation continues to be checked by the memo scenarios.

### Verified live behavior

The isolated live matrix passed 12/12 cases. Healthy controls and every
positive repair compared complete output SHA-256 hashes with the healthy
reference downloads. The reference payloads were approximately 521 MB and
681 MB. The tests exercised:

- Complementary missing articles with a valid PAR index: article borrowing
  followed by successful PAR verification.
- Missing NZB segment entries and failed articles: stream repair restored
  28,672,000 bytes across 40 articles.
- Live stream repair: repair started before the primary download completed;
  both complete output files matched their controls.
- Local PAR recovery with `ParQuick=yes` and `ParQuick=no`.
- Duplicate PAR recovery: recovery blocks came from a twin posting, with no
  borrowed data articles, and the reconstructed payload matched its control.
- Disabled fallback, an unrelated donor, and insufficient parity: all
  retained failure status rather than reporting success with damaged output.

An additional decompression pair used genuine downloaded reference bytes in
a locally generated deflated ZIP donor served by the real nserv implementation.
The damaged primary still downloaded through the live NNTP proxy. With
`DupeStreamDecompress=yes`, 40 articles were recovered and the entire output
matched the reference. With decompression disabled, recovery remained zero
and the download failed. This is a hybrid test, not an independently found
compressed repost on the public provider.

The live fixtures came from NZBHydra NZBs. Selected message IDs or NZB entries
were deliberately removed, and donor segment-size metadata was adjusted where
needed to exercise byte-range rather than structural article matching.
These controls prove recovery against real provider data; they do not imply
that every historical release has a compatible available donor.

### Configuration and scope

The isolated instances enabled duplicate checking, retained source NZBs,
PAR repair, and duplicate PAR scanning. Fallback was varied explicitly between
off, article, stream, and live; decompression was varied explicitly too.
The production daemon, queue, and configuration were not replaced.

A retained large-release replay did try stream recovery and restored roughly
600 MB before its watchdog stopped the pass: repair throughput was below one
third of the original download average. Its remaining damage exceeded the
available parity, and it correctly ended in failure. A test timeout of zero
disabled that watchdog for controlled correctness experiments; this is not a
production performance recommendation. Article borrowing without a PAR index
remains deliberately rejected by the safety check.

The final runtime SHA-256 is
`2bf30ed5aac96f4129df79068a783c50a9eedec853a8d6c98dd6e7f650df9f1d`.
The build used Rust 1.99 and Linux amd64 containers under macOS arm64 emulation.
These are correctness results, not native performance qualification or a
production deployment claim. Private raw fixtures, provider configuration,
command receipts, logs, and output hashes were retained outside the repository.

### Offline regression results

The unit suite passed 586/586 cases and 23,124/23,124 assertions. PAR-first
validation passed 14/14 scenarios with quick verification enabled and 14/14
with it disabled. History retry preserved failure and bytes without redundant
requests. Four archive-stall cases verified unavailable/incomplete donor
rejection, cleanup, later healthy donors, and password retry.

All 354 broad functional scenarios were exercised on the final runtime. The
three concurrent shards initially passed 352/354. One failure was the old
empty-search restart assertion; the corrected fixture passed with exactly two
searches and two empty results. The other produced an inconclusive multi-server
probe under concurrent execution, then downloaded the healthy backup with
matching bytes. Its original assertions passed in an isolated run: four
definitive servers, two unreachable servers excluded, a dead-posting verdict,
and successful byte-identical backup output. These targeted results are
reported separately from the initial shard results.

Earlier raw failures and instrumented diagnostic runs are retained. Their
executables are distinguished from the final candidate above; none is
substituted for final verification. The final source reviews found no issues
in the success threshold, bounded transport-retry assertions, or failed-stat
handling. A claimed duplicate completion-script finding was rejected because
the queue-locked outer guard excludes existing post-processing jobs before
script enqueue.
