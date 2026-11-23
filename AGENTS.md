# Agent working instructions

Read [the evidence workflow](docs/EVIDENCE_WORKFLOW.md) before changing or validating
this project. Human instructions and current coordination ownership take priority.

## Preserve work and identity

- Preserve dirty work and earlier evidence. Use isolated work when necessary;
  do not create unsigned stash commits or reset another owner's checkout.
- Refresh the actual remote default before integration. This fork uses
  `main` (named `dupe-article-fallback` until 2026-11-01); verify it with
  `git ls-remote --symref origin HEAD` rather than silently substituting another ref.
- Every new Git commit must have author and committer `xbmc4lyfe`, email
  `273732874+xbmc4lyfe@users.noreply.github.com`, and the configured signing key.
  Verify GitHub reports the pushed commit as verified before integration.
- GitHub writes must use the `xbmc4lyfe` account. Tools that generate commits or
  publish automatically require an identity/signing audit before use.
- Verify the canonical HTTPS remote and effective Git credentials as well as
  the API actor. Use the verified GitHub credential-helper override for fetches
  and authorized Git mutations; an API identity check alone is insufficient.
- Preserve signed working history before a source-equivalent squash. Verify
  the reviewed tree, fresh sole-parent base and actual final-head reviews.
  Never substitute a GitHub-generated merge or squash commit for the locally
  signed canonical commit; lease only an owned feature ref when rewriting.
- Coordinate canonical default-branch writes and backend hold release with their
  designated owners. The PR owner remains responsible for deployment as described
  below. A source fix, tool installation, or documentation change does not release
  a performance or deployment hold.

## Own your PR through deployment

- The agent that opens a PR owns delivery through fixes, review, integration,
  redeployment, and verification. Do not leave deployment to a central redeployer
  or report the work complete at merge.
- Fix actionable review findings and request review again after changes. Pull in
  the actual upstream default, resolve conflicts, and repeat affected validation
  and review. Before a signed squash merge into the default branch, require g2g
  review of the actual final head, resolved actionable discussions, and passing
  pre-merge CI (GitHub Actions, or the local runs below when Actions is broken).
  Older-head approval does not cover later changes.
- After merge, the same agent follows default-branch CI/CD to completion when it runs,
  fixes failures, and performs the authorized redeployment of the exact merged
  artifact. An automated deployment pipeline may perform installation; the PR
  owner triggers it when needed, monitors it, and verifies the deployed result.
- Preserve shared scheduling, locks, authorization, release holds, backups, and
  rollback requirements. Request your execution slot from the scheduling owner;
  that coordination does not transfer deployment responsibility. Carry existing
  human authorization forward without asking for it again.
- Verify the installed revision or artifact, service health, preservation checks,
  and the changed behavior. Retain deployment and recovery evidence. A green
  build or successful merge alone is not proof of a successful deployment.
- For a change with no deployable artifact, record why deployment does not apply.
  Published documentation still requires its applicable publishing checks.
  Keep blocked deployment work owned and report the exact blocker. Do not archive
  the owning chat until delivery is verified or the human explicitly accepts a
  handoff or an undeployed outcome.

## Build and test before every push

- Before pushing any branch, run on the machine you are on, and paste the
  pass/fail lines into the PR description:
  `(cd rust && CARGO_BUILD_JOBS=3 RUSTFLAGS="-D warnings" cargo test --tests -- --test-threads=3)`,
  `CARGO_BUILD_JOBS=3 just test-daemon 3`, and the functional scenarios that
  cover what you touched (`tests/functional/dupefallback/harness.py`).
- A change must also build and pass on Linux. macOS-only validation is not
  enough: `libc` constants, `c_char` signedness and linker flags differ. Build
  and test in a Linux machine (for example an OrbStack VM with a copy of the
  tree) or wait for the `build` workflow, which builds and tests on Linux
  x86_64, Linux arm64 and macOS for every pull request and push to `main`.
- When GitHub Actions is broken (billing, outage, or private dependencies it
  cannot fetch), running the same checks locally replaces it. Run the commands
  above on the exact head on macOS, Linux x86_64 and Linux arm64 (OrbStack VMs
  are fine), and paste each platform's pass/fail lines and the head commit into
  the PR description. Say in the PR that Actions was unavailable.
- Reviewers and review agents: do not approve a head that lacks either a green
  `build` workflow on all three platforms or, when Actions is broken, the local
  results for all three platforms on that head. Ask for them instead of assuming.

## Capture evidence before interpreting it

- Freeze the question, baseline, fixtures, controls, analysis, and stopping rule
  before an experiment. A changed plan gets a new identifier.
- Use a fresh exclusive run directory. Create command metadata and raw stdout /
  stderr files before spawning; record exit, timeout, interruption, or launch
  failure before parsing. Retain failed, malformed, and inconclusive attempts.
- Check storage and tool readiness before admission. For uploads and asynchronous
  commands, inspect completion and exit status, then verify file hashes before
  running dependent work. An incomplete upload is never executable evidence.
- Record source, executable, fixture and dependency hashes, tool versions,
  machine configuration, process boundaries, and limitations. Preserve original
  raw evidence; derived analysis and redacted exports are separate artifacts.
- Hash only bounded regular files through validated descriptors; reject
  incomplete reads and protect inputs, including traversed symlink entries,
  before replacing or cleaning outputs. Checkout identity is descriptive,
  not proof that a particular executable was built from that source.
- Link conclusions and decision records to the evidence that supports them.
  Mark retrospective recovery explicitly; never describe recovered output as
  prospectively captured. Supersede old conclusions instead of rewriting history.
- Report observed, reproduced, verified, merged, and deployed states separately.
  Use repository-relative labels in user-facing text and published metadata.

## Measure the real behavior

Read [the benchmark procedures](docs/BENCHMARK_AGENT_PROCEDURES.md) when the
reviewed migration infrastructure is present. Framework-specific examples are
conditional, not worker admission or proof that the tooling is installed.

- Use offline fixture providers while retaining the actual caller, cache,
  mapping, crypto and ABI boundary being evaluated. Verify bytes, failure
  semantics and active-boundary hit counts before timing.
- Compare original C++, equivalent optimized C++ and Rust. Separate ownership /
  algorithm improvements from language effects. Include unchanged-code controls.
- Preserve independent-process trials and every sample; do not pool nested
  samples as independent executions or silently discard unfavorable results.
- Preserve an existing Rust implementation independently when applicable.
  Declare and measure that baseline, or obtain source-bound review of an
  explicit nonapplicability rationale. A default regression alert threshold
  is not permission to weaken migration or preservation requirements.
- Calibration failure or an interval admitting regression is inconclusive.
  Profiling, instruction counts, signatures and reproducibility do not prove
  native speed or release readiness. Do not retune a frozen plan to make it pass;
  method changes require a new identified plan and preservation of old results.
- Inspect the qualification artifact, not just a summarizer's exit code.
  A clean collection or analysis command can legitimately produce HOLD.
- Keep measured PR/evidence processes separate from authenticated publication.
  Recheck live same-repository PR/base/head identity before writes; missing
  configuration or incomplete publication must invalidate stale success.

## CPU tiers and SIMD

- Every build must run on an Intel Coffee Lake desktop (AVX2, no AVX-512) and an Apple
  M4 Air/Pro; anything older is unsupported. Newer instructions (AVX-512, GFNI, ARM
  crypto, M4/M5 extras) are welcome where a benchmark proves them faster, behind runtime
  dispatch with the scalar version kept as the reference. See
  [CPU tiers and SIMD](docs/CPU_TIERS.md) for the release tiers and the test rules.

## Runtime replacements and output contracts

Read [the Rust conversion procedures](docs/RUST_CONVERSION_PROCEDURES.md) for
reviewed-branch commands and build-lineage pitfalls.

- Match the actual caller's bytes, fields, partial output, failure, ownership
  and cleanup behavior. Include malformed inputs, overlap, boundary sizes,
  canaries, exceptions and allocation failure where the API permits them.
- Preserve existing text/logging emission, buffering, flush, and error behavior.
  A raw byte-write replacement can differ from historical formatted stdio even
  when a formatting helper returns equal text. Validate the real emission path,
  including embedded NUL, non-UTF8 and quiet-mode behavior where applicable.
- Prove the optimized boundary is reached under the live build configuration.
  A helper bypassed by the enabled Rust caller cannot qualify that caller.
- Snapshot held Cargo archives before reusing targets. Preserved archive
  timestamps can cause false freshness; use isolated targets or discard those
  timestamps and retain proof of required recompilation.
- Enforce the plan's exact integer elapsed-duration floor and unique balanced
  fixture/round keys. Invalid or under-floor rows reject the packet; keep raw
  data instead of filtering, replaying, or adaptively collecting replacements.

## WebUI and illustrated documentation

Read [the WebUI procedures](docs/WEBUI_AGENT_PROCEDURES.md) for tested commands.

- Exercise editor mutations only in an isolated loopback fixture; intercept
  saves and preserve the committed tests' non-loopback rejection guards.
- Test native keyboard and programmatic/search navigation separately. Native
  focus can conceal menu-scroll bugs. Cover desktop/phone, light/dark themes,
  repeated localized headings, editor payloads, and unsaved summaries.
- After each UI action, refresh the cheapest relevant current state. If a
  snapshot disagrees with visible content, check fresh accessibility state and
  direct selected/expanded attributes before diagnosing a defect or toggling
  the control again.
- Capture real JPEGs from sanitized examples, never private production values.
  Wait for asynchronous content and verify the actual active-tab viewport.
  Full-page capture does not expand internally scrolling content; inspect the
  final row/option as well as the first screen.
- Track picture dimensions/hashes and every relevant UI/config source asset.
  Keep accessible full-size links and complete print pictures. Validate the
  locked site build, links, and prose before claiming publication.
- For an explicitly authorized UI-only deployment, preserve backend, process,
  configuration and providers; the PR owner installs exact assets atomically
  with recovery evidence. Post-deployment UI checks are read-only. UI approval
  never clears a backend rollout hold.

## Observers and future investigations

Read [the observer procedures](docs/OBSERVER_AGENT_PROCEDURES.md) for daemon
contract and privacy checks, and [the investigation options](docs/CONVERSION_INVESTIGATION.md)
for researched methods that have not been executed or installed by this work.

- Require actual progress signals for a stall detector. Distinguish scheduling
  flags from work already in progress, and article/post behavior from URL fetching.
- Restricted RPC credentials are not inherently read-only. Limit methods at an
  enforced boundary and keep private raw RPC content out of shared evidence.
- Keep invalid or missing values unknown. Validate bounds and stages against
  daemon contracts, and label sequential replies as non-atomic observations.
- Separate observer collection cost, daemon observer effect, and migration
  qualification. PSI or cumulative counters alone do not identify a cause.
- State instrumentation coverage precisely: an ordinary Rust archive linked
  into a C++ sanitizer harness is not Rust sanitizer coverage; a width model
  is not native cross-platform execution. Research permission is not execution
  permission, and diagnostic counts are not native latency.

## Twins and alts

These two words have one meaning each in code, logs, tests, and documentation.

- A **twin** is the exact same post uploaded again: the same files, sizes,
  article counts, and checksums. Twins are proven, never assumed from a
  matching size or name: par2 file lists must agree, or sampled articles at
  the same positions must be byte-identical. Only a twin's articles may be
  swapped in one for one.
- An **alt** is the same video file in a different container: other volume
  sizes, another archive format, an archive inside an archive, or another
  password. Container bytes of alts never match and must never be compared.
- Matching and borrowing for alts go by the bytes of the video file itself,
  reached through every stored container around it. The one payload map is
  `ContentMapper::build_map_with_tls` in `rust/src/streamrepair.rs`. Extend
  it for a new container or layer; don't build a second map or compare
  container bytes.
- A copy with a different video file, such as another encode, is neither.
  In a fleet (`appendfleet`) it is dropped at intake (the user's rule: the
  fleet is one video; spot-check the payload, discard a mismatch). Added any
  other way, it is a plain backup for failover.
- Other par2 checksums (or other sampled articles) prove only another
  container, never another video or an alt. TwinCheck labels such a copy
  `container` (video inside not verified yet) unless the fleet intake read
  the video inside both copies (`DupeInner` digests): equal makes it `alt`,
  different makes it `other`. Never word it "another encode" from par2 alone.
- A **re-RAR** is an alt made by packing the same files again (another
  modification time): only the first and last article of each volume
  differ. TwinCheck's no-par2 sample always reads those header articles
  (`rust/src/twinfields.rs`), labels such a copy `alt` (`DupeSampled=rerar:<tail>`),
  and lets it lend its middle articles one for one, never a header article.
  Every member header is rewritten too, so this holds only while one member
  runs from the first volume into the last (`twinfields::rerar_member_end`);
  what follows it in the last volume counts as tail.
- Prefer donors in this order: twins, then alts that can donate by range
  (stored containers, known password), then alts that must be downloaded and
  extracted whole.

## Respect execution ownership

Read [the deployment procedures](docs/DEPLOYMENT_AGENT_PROCEDURES.md) for
identity, preservation, rollback, and exact-artifact handoff requirements.

- Coordinate remote work through the single worker scheduling owner. Heavy
  stages take `i7-heavy.lock` exclusively, then
  `nzbget-rust-benchmark.lock` shared at the fixed worker-global lock root.
  Per-job `TMPDIR`, build/target folders and NVMe destinations must never
  relocate or split the shared locks.
  Admission waits default to 120 seconds unless the coordinating owner has
  explicitly assigned another bounded cap. Respect the assigned stage's job
  cap, never exceeding three, including `CARGO_BUILD_JOBS`.
- Prebuilt timing takes only the benchmark lock exclusively and releases it
  after each bounded process. Never acquire the heavy lock afterward.
- Tracing, packaging and backup work must also be scheduled according to cost
  and available storage. No default writes, deployment, host tuning, production
  traffic or repeated benchmark runs follow merely from installing a tool.
- The designated NVMe build destination is already created and verified. Use
  unique owner/job output folders for already-approved work; do not add another
  folder-setup approval process or migrate/delete existing evidence implicitly.
