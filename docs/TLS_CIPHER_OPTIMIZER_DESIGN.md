# Rust TLS cipher optimizer

Status: proposed specification for human review; implementation has not started.

## Intent and acceptance

Add an **Optimize TLS ciphers** button to Settings > News Servers. On the
computer running NZBGet, measure local cipher throughput, discover which
candidates each configured encrypted news server accepts, and automatically
save the fastest measured supported cipher separately for each server. A server
that accepts the overall winner gets that winner; another gets its own fastest
supported candidate. Show the results, errors and whether a reload is required.

The user approved this approach in chat. The user additionally requires a
tested isolated local version and real screenshots before any pull request,
an audible readiness announcement, and their explicit **g2g** before opening
the PR and beginning the review process. This specification is the next review
artifact, followed by an implementation plan. No production installation,
provider test, integration, or release of an existing backend hold is implied.

Source baseline: `8cb2a02d46b6f596e75ff183050b740ed84c609e`, freshly fetched
from `origin/dupe-article-fallback`, canonical repository `Appz4Fun/nzbget`.
Existing boundaries are `rust/`, `daemon/remote/XmlRpc.cpp`,
`daemon/connect/TlsSocket.cpp`, `daemon/extension/ScriptConfig.cpp`, and
`webui/config.js`. The guide is `docs/FINDING_BEST_TLS_CIPHER.md`.

## Chosen approach and alternatives

Implement orchestration, measurement loops, selection, job state, cancellation
and validation in the existing Rust library. Add narrowly typed C ABI callbacks
for the existing daemon's OpenSSL, NNTP and configuration facilities. C++ remains
the adapter to existing native facilities; JavaScript renders the dialog and
polls the job. Do not introduce a second crypto library or an executable lookup.

A separate `openssl speed` process is simpler but could measure a different
OpenSSL build and requires an external executable. Downloading articles under
each cipher would include network and provider variability and need a content
fixture. Neither is the selected implementation. This tool estimates local
crypto throughput and validates compatibility; it does not claim to measure
end-to-end download speed or guarantee a unique fastest cipher under all loads.

## Cipher scope and measurement

Enumerate candidates from the linked OpenSSL implementation under its existing
security policy. Eligible suites use TLS 1.2 or TLS 1.3, authenticated encryption
with at least 128-bit security, and certificate authentication usable by NZBGet.
TLS 1.2 additionally requires ephemeral key exchange. Exclude anonymous,
NULL, export, legacy CBC/RC4/3DES, PSK and SRP suites; NNTP credentials are not
TLS PSK credentials. Report exclusions and unavailable algorithms explicitly.
Benchmark each distinct eligible bulk algorithm once and associate its results
with every matching suite. Cover AES-128-GCM, AES-256-GCM and
ChaCha20-Poly1305 when available, plus other eligible algorithms actually exposed
by the linked library with a validated authenticated-decryption adapter.
An algorithm without that adapter is reported as unmeasured and cannot win.

Measure authenticated decryption of valid synthetic 16,384-byte records with
fresh record nonces, matching Usenet's receive direction. Generate and validate
reference plaintext/ciphertext outside timing; consume decrypted outputs so work
cannot be removed. Use one worker and the same reusable-context lifecycle for
all algorithms, including per-record initialization and tag verification.
Do not include allocation or fixture generation in the timed region.

Freeze the job's candidate list and method before collection: a 50 ms warmup
per algorithm followed by five rounds, rotating candidate order each round.
Each sample runs until the integer elapsed duration is at least 200,000,000 ns.
Store every round's bytes, elapsed nanoseconds and correctness outcome; a failed,
missing, duplicate or under-floor sample invalidates that algorithm. No adaptive
retries or silent replacement samples. Rank valid algorithms by median bytes
per second. Exact ties prefer the existing single cipher if it is supported,
then TLS 1.3, then lexical suite name for a reproducible result. Show measured
rates as estimates, including sample spread, rather than a precision guarantee.

## Server compatibility and exact selection

Operate on the saved configuration, taking an immutable server snapshot at
start. Include configured servers with encryption enabled and a nonempty host,
including disabled backup servers; skip plaintext servers without modifying
their encryption setting. Reject a start from unsaved server edits, with a
message to save those edits first. Preserve unrelated unsaved browser edits.
Server credentials stay inside the daemon and never enter job responses.

Probe each eligible suite separately using the configured host, port, SNI,
certificate store and certificate-verification level. Reuse the actual NNTP
connection path and validate greeting/authentication without requesting articles.
Offer exactly one suite in exactly its TLS version, check the negotiated version
and suite, and close the probe connection. A successful handshake under a
different suite does not establish compatibility. Ordinary default cipher lists
must not leak into a supposedly pinned TLS 1.2 or TLS 1.3 probe.

Use sequential probes, at most one connection at a time, a maximum ten-second
connection/handshake/NNTP deadline per probe and a ten-minute overall job
deadline. Bound DNS resolution as part of the probe deadline. Check cancellation
between measurement batches and during network waits. Keep unsupported-suite
rejection separate from timeout, DNS, certificate and authentication errors;
ambiguous failures are unknown, not evidence of non-support. If any eligible
candidate for a server remains unknown or unmeasured, do not claim its optimum
is established and leave that server unchanged. Preserve successful compatibility
observations in the report. Other fully tested servers can still receive results.

Choose the highest ranked measured suite among each fully tested server's
accepted suites. Report the common supported suites for the complete fleet only
when all included servers have a complete compatibility result; otherwise mark
that intersection unknown. Do not force a slower common cipher onto a server
that supports a faster one. A server with no eligible supported suite is left
unchanged, with an explanation.

The persisted single-suite value must also constrain the normal download TLS
path to that suite and version. Add a targeted exact-suite path for optimizer
candidates in `TlsSocket`; preserve existing behavior for empty values and
custom colon-separated lists. Cover fallback/version isolation in integration
tests. Saving a cipher that the normal connection path can ignore is a failure.

## Jobs, RPC and configuration transaction

Expose control-account-only start, status and cancel RPCs through the existing
RPC transport. Deny add/restricted accounts and unsafe GET/JSONP entry paths
for all optimizer operations. The client sends no arbitrary host or command.
Use opaque job identifiers, bounded result sizes, one active job per daemon,
and one retained terminal result until a later run replaces it. Repeated status
requests have no side effects. A duplicate start returns the existing active
job, never another benchmark. Cancellation never creates a replacement run.
Admission caps are 256 encrypted servers, 128 eligible suites, 64 distinct bulk
algorithms and a 16 MiB configuration snapshot. Reject an over-cap job before
starting work rather than silently truncating the candidate or server list.
Status responses are at most 1 MiB, with bounded diagnostic strings and no
per-record payloads; per-algorithm five-round samples remain available there.

States are measuring, probing, saving, completed, cancelled and failed.
Report completed with per-server unchanged/error results for partial outcomes.
Daemon shutdown cancels and joins the owned worker before native TLS cleanup.
Never hold a configuration mutex while benchmarking or waiting on a provider.

At commit, compare the saved configuration with the original snapshot under
the same synchronization used by ordinary configuration writes. Any change
invalidates the transaction; save nothing and report the conflict. Serialize
ordinary `saveconfig` with this commit so compare-and-write is meaningful.
Rust prepares a patch containing only accepted `ServerN.Cipher` values.
Apply it to the original complete file, preserving all other bytes, comments,
credentials and permissions. Do not pass a sparse list to existing `saveconfig`,
which removes omitted options. Write a sibling temporary file, check complete
writes/flushes, then atomically replace the config through a platform-supported
operation. Preserve the original on any pre-replacement failure; report storage
and replacement failures as failures. Reject symlink/nonregular configuration
targets rather than silently replacing a different object. Keep file ownership
and platform behavior explicit in the implementation and tests.

Cancellation before the commit changes nothing. Once the atomic commit happens,
report saved even if cancellation arrives afterward. Saving does not restart the
daemon or reconnect active downloads. Use the existing reload-required notice,
and clearly distinguish saved values from active connections. Browser cipher
fields and saved baselines update only for values confirmed committed; unrelated
unsaved changes and their dirty-state indicators remain intact.

## WebUI and error presentation

Place the fleet button near the News Servers section controls. Open an accessible
dialog explaining the brief CPU work, provider connections, automatic cipher-only
save and later reload. Start from the button's explicit user action without a
second approval dialog. Show progress, algorithm rates, compatibility and each
server's old cipher, selected cipher and outcome. Provide Cancel while running
and Close after completion. Closing the dialog does not silently cancel or
restart a job; reopening resumes its status. Prevent misleading double starts.

Use text insertion for server names and native error messages. Keep credentials
out of diagnostics. Follow existing locale conventions, keyboard focus handling
and light/dark styling. Support a bounded horizontally scrollable result table
at phone widths, with access to its final column and final server row.
When Rust or TLS support is absent, disable the action and explain why.

## Validation and local handoff

Before test execution, create a fresh exclusive evidence directory, a frozen
test plan, source/fixture hashes and per-process command metadata with raw
stdout/stderr sinks. Record launch failures, timeout, native exit and all failed
attempts before interpreting output. Preserve evidence outside tracked source;
public reports and screenshots use sanitized examples only.

Rust tests cover candidate filtering, exact sample floors/keys, ranking and ties,
different server support sets, unknown outcomes, no common cipher, cancellation,
duplicate starts, bounded status and configuration conflicts. Native integration
tests use local TLS/NNTP fixture servers with controlled supported suites,
certificates and authentication outcomes. Assert the exact negotiated cipher,
normal-download pinning, version fallback prevention, and failure behavior.
Exercise config byte preservation, permissions, concurrent ordinary saves,
write failures, replacement failures and cancellation around commit.

WebUI checks retain the committed loopback-only guards and intercept mutations
outside the isolated end-to-end test. Cover desktop/phone, both themes, keyboard,
progress/cancel, partial failure, backend absence, escaping, unsaved edits,
reload-required messaging, duplicate starts and final-row/column visibility.
Capture and inspect real sanitized JPEGs from the candidate's actual dialog.
Run relevant existing Settings regressions after the new checks pass.

Heavy native/Rust builds and tests require the designated scheduling owner and
worker admission under the repository's fixed lock namespace and job caps.
Use fresh targets and retain actual build-to-binary lineage. This is new
functionality, not a replacement of an existing C++ implementation; there is no
prior C++ or Rust optimizer baseline to qualify. Existing migration and backend
release holds remain unchanged. Do not manufacture comparative speed claims.

Prepare an isolated loopback local NZBGet instance with its own configuration,
queue/state directories and candidate WebUI/backend. Synthetic providers prove
the feature; they do not prove the user's providers support a particular suite.
Give the user its URL, start/stop instructions, screenshots, evidence summary,
and exact tested source/binary identity. On readiness, speak a short macOS
announcement. Wait for the user's **g2g** before PR creation or the review process.

## TLS Cipher Optimizer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the Rust-backed Optimize TLS ciphers action and deliver a tested isolated local instance with screenshots before PR creation.

**Architecture:** Rust owns policy, benchmarking, jobs and byte-preserving config patches. Typed callbacks reuse the daemon's linked OpenSSL and NNTP implementation. The WebUI polls a control-account-only RPC service and renders its bounded results.

**Tech Stack:** Existing Rust 1.99 static library, C++20 daemon adapters, linked OpenSSL, existing JavaScript/Bootstrap WebUI, Boost tests, Python loopback TLS/NNTP fixtures, Playwright.

**Spec:** [Rust TLS cipher optimizer](#rust-tls-cipher-optimizer)

### Global Constraints

- Work only in the existing isolated `codex/tls-cipher-optimizer` worktree; preserve other owners' work.
- All commits: signed, author and committer `xbmc4lyfe <273732874+xbmc4lyfe@users.noreply.github.com>`.
- No PR or review process before the user's local-test handoff and g2g; the user has granted approval to proceed through implementation without further checkpoints.
- Eligible suites: certificate-authenticated TLS 1.2/1.3 AEAD, at least 128-bit security, ephemeral TLS 1.2 key exchange; no PSK/SRP/anonymous/legacy modes.
- Records: 16,384 bytes; warmup 50 ms; five balanced rotating rounds; each sample elapsed at least 200,000,000 ns; median throughput ranking; no replacement samples.
- Caps: 256 encrypted servers, 128 eligible suites, 64 algorithms, 16 MiB config snapshot, 1 MiB status response, ten seconds per probe, ten minutes per job, one active job and one probe connection.
- Exact ties: current supported single cipher, then TLS 1.3, then lexical suite name.
- Unknown or unmeasured candidates prevent saving that server; confirmed unsupported candidates do not.
- Apply only cipher values; byte-preserve unrelated config; reject concurrent edits and nonregular/symlink targets; atomic file replacement; no automatic reload.
- Heavy Rust/native stages use the designated scheduling owner, fixed global locks, at most three jobs and bounded admission (default 120 seconds). No unscheduled heavy local build.
- Fresh evidence directory and plan, raw precreated streams, command/completion records and hashes before each validation run. Preserve all failed attempts.
- Existing backend/migration holds remain independent; this is a new feature with no old optimizer baseline.

### Review Focus

- Mixed-case or duplicate Cipher lines must all resolve to the saved choice without changing any unrelated bytes (Task 2).
- DNS stalls and shutdown must leave no resolver process, probe connection or job touching cleaned-up TLS state (Task 3/4).
- A TLS 1.2-only pin against a TLS 1.3-capable provider must not silently use its TLS 1.3 defaults (Task 3).
- A completed old dialog must not overwrite server reordering or unsaved edits in another browser (Task 4/5).
- A delayed cancel after atomic replacement must report saved, never imply the file was left untouched (Task 2/4).

### File Structure and Shared Interfaces

- `rust/src/tlscipher/mod.rs`: policy types, sample validation, ranking, per-server decisions and config byte patching; no native dependencies, independently testable with `rustc --test`.
- `rust/src/tlscipher/job.rs`: one-job worker service, bounded status JSON, cancellation, measurements and probe orchestration using `Backend`.
- `rust/src/tlscipher/native.rs`: owned typed C callback adapter and self-executable bounded resolver subprocess.
- `rust/include/nzbget_tls_cipher.h`: ABI dedicated to this feature; consumed by the existing umbrella `nzbget_rs.h`.
- `daemon/connect/TlsCipherBackend.{h,cpp}`: OpenSSL/NNTP callback implementations and native lifecycle facade.
- `daemon/connect/Connection.{h,cpp}` and `TlsSocket.{h,cpp}`: resolved-address override, absolute probe deadline, negotiated-cipher inspection, exact-suite pinning; normal custom lists retain behavior.
- `daemon/extension/ScriptConfig.{h,cpp}`: common config write lock and checked snapshot/atomic patch commit.
- `daemon/remote/XmlRpc.cpp`: optimizer RPC registration/access/transport checks, small adapter command.
- `daemon/main/nzbget.cpp`: early internal resolver entry and worker shutdown before cleanup, including reload.
- `webui/tlscipher.js`, `webui/index.html`, `webui/config.js`, `webui/skin.css`, locale catalogs: dialog, section action, saved-result reconciliation and styling.
- `tests/connect/TlsCipher.cpp`, `tests/functional/tlscipher/fixture.py`, `tests/webui/tls-cipher.js`: native integration, loopback end-to-end and UI regression.

Rust policy interfaces:

```rust
enum Protocol { Tls12, Tls13 }
struct Suite { name: String, algorithm: String, protocol: Protocol, security_bits: u16, aead: bool, ephemeral: bool, certificate: bool }
struct Sample { round: u8, bytes: u64, elapsed_ns: u64, correct: bool }
enum Support { Supported, Unsupported, Unknown }
struct Choice { cipher: String, bytes_per_second: f64 }
fn eligible(suite: &Suite) -> bool;
fn throughput(samples: &[Sample]) -> Result<f64, Error>;
fn choose(suites: &[Suite], rates: &BTreeMap<String, f64>, support: &BTreeMap<String, Support>, current: &str) -> Result<Option<Choice>, Error>;
fn patch_config(original: &[u8], changes: &BTreeMap<u32, String>) -> Result<Vec<u8>, Error>;
```

Job `Backend: Send + Sync + 'static` exposes `snapshot() -> Result<Snapshot, Error>`,
`suites() -> Result<Vec<Suite>, Error>`, `prepare(algorithm: &str) -> Result<Box<dyn Decryptor>, Error>`,
`probe(server: &Server, suite: &Suite, cancel: &AtomicBool, deadline: Instant) -> Probe`,
and `commit(snapshot: &Snapshot, changes: &BTreeMap<u32, String>, cancel: &AtomicBool) -> Result<bool, Error>`.
`Decryptor::prepare_batch(&mut self, first_sequence: u64, count: u32) -> Result<(), Error>` generates 64 valid records with distinct sequence-derived nonces outside timing.
`Decryptor::record(&mut self, sequence: u64) -> Result<u64, Error>` processes one verified prepared record; setup stays outside timers. Samples accumulate only the timed decryption batch durations until the exact floor.
`Snapshot` owns original config bytes plus parsed `Server` values (id, name, host, port, username, password, cipher, IP version and verification level).
`Probe { support: Support, message: &'static str }` retains distinct normalized DNS, deadline, certificate, authentication and unsupported-suite outcomes without returning native credential-bearing text.
No credential-bearing `Debug` or status serialization derives.

`Service::start(Arc<dyn Backend>) -> Result<String, Error>`,
`Service::status(id: Option<&str>) -> Result<String, Error>`,
`Service::cancel(id: &str) -> Result<(), Error>`, and `Service::shutdown()`
are the only public job controls. An absent status id queries availability/current job.

ABI callbacks use borrowed length-tagged strings/buffers and explicit success/error
codes; every created native context has exactly one destroy callback. Rust copies
snapshots before returning from start and retains an owned native backend context
until its joined worker exits. All callbacks are nonthrowing; no borrowed RPC or
Options pointer crosses a thread lifetime. Publish exact ABI declarations beside
the matching Rust `#[repr(C)]` structs and test field/ownership correspondence.

---

#### Task 1: Cipher policy and measured selection

**Files:** Create `rust/src/tlscipher/mod.rs`; modify `rust/src/lib.rs`.
**Interfaces:** Produces the policy interfaces above; Task 4 consumes them.

- [ ] Write tests `filters_unsafe_suites`, `requires_five_unique_valid_rounds`, `rejects_underfloor_and_nonfinite_rates`, `selects_fastest_per_server`, `unknown_prevents_choice`, `ties_retain_current_then_tls13_then_name`.
  Assert elapsed `199_999_999` rejects and `200_000_000` accepts; samples with rounds `[0,1,2,3,3]` reject; algorithms at 300/200/100 choose the best in each independent support set; unsupported differs from unknown.
- [ ] Under scheduler admission, run `rustc --edition=2021 --test rust/src/tlscipher/mod.rs -o POLICY_TEST` then `POLICY_TEST`; retain the expected failing assertions before implementing behavior.
- [ ] Implement `eligible`, `throughput` and `choose` with the shared signatures. Require finite positive rates and exact sample keys; every candidate's support must be known before selection.
- [ ] Run the same admitted test commands; expected: all policy tests pass. Commit signed as `feat: add TLS cipher ranking policy` after reading the actual output.

#### Task 2: Cipher-only configuration transaction

**Files:** Modify `rust/src/tlscipher/mod.rs`, `daemon/extension/ScriptConfig.{h,cpp}`, `tests/extension/ScriptConfig.cpp`.
**Interfaces:** Consumes `patch_config`; produces `ScriptConfig::ReadCipherSnapshot(std::string&)` and `ScriptConfig::CommitCipherPatch(std::string_view expected, std::string_view replacement, const std::function<bool()>& cancel)` with typed result `Saved/Unchanged/Conflict/Cancelled/Failed`.

- [ ] Write Rust tests `patch_preserves_unrelated_bytes`, `patches_duplicate_case_insensitive_cipher_lines`, `appends_missing_cipher_with_matching_newline`, `rejects_invalid_patch_values`.
  Assert comments, CRLF, non-UTF8 unrelated bytes, credentials, no-final-newline and all non-Cipher fields remain byte-identical. Reject ids zero, unknown server ids and values containing line breaks/NUL or cipher-list syntax.
- [ ] Write native tests `CipherPatchConflictPreservesOriginal`, `CipherPatchRejectsSymlinkAndDirectory`, `CipherPatchWriteFailurePreservesOriginal`, `CipherPatchSerializesOrdinarySave`, `CipherPatchPreservesMode`, `CancelAfterCommitReportsSaved` using temporary files only.
- [ ] Run these through the admitted Rust and Boost `ExtensionTest` commands; expected: relevant tests fail against unimplemented APIs/old behavior.
- [ ] Implement parsing/patch construction in Rust. In native code use a shared ScriptConfig write mutex, descriptor-bound bounded snapshot, original-byte comparison, exclusive sibling temp creation, mode/ownership preservation, checked write/flush/close, cancellation check and atomic replacement. Use platform-native replacement semantics; unsupported ownership/ACL preservation rejects before replacement. Ordinary SaveConfig takes the same mutex.
- [ ] Run admitted policy and `nzbget_tests --run_test=ExtensionTest --log_level=message`; expected: all named tests and existing config regressions pass. Commit signed.

#### Task 3: Linked-library crypto and exact bounded NNTP probes

**Files:** Create ABI header and `TlsCipherBackend.{h,cpp}`; modify `TlsSocket.{h,cpp}`, `Connection.{h,cpp}`, `daemon/sources.cmake`, `rust/include/nzbget_rs.h`; create native tests/fixture and add CMake registration.
**Interfaces:** Produces the native callbacks consumed by Task 4, exact-suite pinning for normal downloads, and `Connection::SetResolvedAddresses(...)` plus `SetDeadline(...)`; ordinary connections have neither override by default.

- [ ] Write native/loopback tests for TLS 1.2/1.3 accepted/rejected suites, actual selected name/version, greeting/authentication, bad certificate, stalled greeting, shutdown/cancel, multiple IPv4/IPv6 answers and empty DNS result. Assert no ARTICLE/BODY/GROUP command is sent.
- [ ] Write EVP correctness tests for supported AEAD adapters, corrupt tags, malformed lengths, per-record distinct nonces and nonconstant plaintext; no invalid plaintext is accepted. Mark unsupported algorithm adapters unmeasured.
- [ ] Run admitted native tests against existing behavior; expected: exact TLS 1.2 pin test fails if TLS 1.3 defaults remain usable, and new probe/deadline tests fail until implemented.
- [ ] Implement metadata enumeration through the linked OpenSSL, reusable authenticated-decryption contexts and adapter callbacks. Rank/filter policy stays in Rust. Bound callbacks and clear OpenSSL errors locally without printing secrets.
- [ ] Add exact eligible single-suite recognition/version bounds and getters for negotiated version/name. Preserve empty/custom-list behavior. Keep certificate validation on the real existing TlsSocket path.
- [ ] Implement absolute monotonic probe deadline and cancellation-aware socket waits; deadline covers connect, handshake, greeting and login, not a fresh timeout for every phase. Use numeric pre-resolved addresses while retaining original hostname for SNI/certificate checks.
- [ ] Run admitted native tests plus unchanged NNTP/Remote/Extension suites; expected: all pass, including no-TLS/no-Rust compile branches. Commit signed.

#### Task 4: Rust job service, resolver custody and protected RPC

**Files:** Create `rust/src/tlscipher/job.rs`, `native.rs`; modify `ffi.rs`, ABI header, `XmlRpc.cpp`, `nzbget.cpp` and native facade; add tests to `tests/remote/XmlRpc.cpp`.
**Interfaces:** Consumes Tasks 1–3; produces RPCs `tlscipherstart([])`, `tlscipherstatus([optional_job_id])`, `tlsciphercancel([job_id])`, each returning a structured JSON object over authenticated JSON-RPC POST only.

- [ ] Write fake-backend job tests for rotating five rounds, one worker, duplicate starts, all admission caps, snapshot ownership, unknown/unmeasured handling, partial fleet success, exact common-set/unknown reporting, cancellation before/during/after commit, stale ids, status size and shutdown join.
- [ ] Write RPC tests for control versus restricted/add access, POST versus GET/JSONP, wrong parameter types/extra arguments, backend absence, wrong job id and bounded escaped responses. Assert denied requests cannot start crypto/provider work or mutate config.
- [ ] Write resolver tests with a synthetic self-executable mode: success, stalled resolver killed/reaped by deadline, cancellation, malformed/oversized output and launch failure. Assert no resolver thread survives shutdown.
- [ ] Run admitted failing tests before implementing the worker and RPC bodies.
- [ ] Implement Service, bounded serializable public models, one owned thread, cancellation and callback lifetimes; measurements use `Instant` and batches bounded for responsiveness. Require returned processed bytes equal record size.
- [ ] Implement bounded DNS through a fixed internal mode of the current executable (`--internal-tls-cipher-resolve`), entered before daemon initialization. Spawn with argument lists, private bounded stdout, no inherited credentials/stdin, null stderr; poll deadline/cancel, kill and reap the exact child on failure. Parse only numeric socket addresses, preserving IP-version filtering. This avoids uncancellable blocking getaddrinfo in the worker; it does not perform TLS or require an external executable.
- [ ] Register POST-only control RPCs with a disabled result when unavailable. Ensure reload/shutdown joins the worker before native objects or TLS are cleaned up. Commit only fully tested per-server choices via Task 2.
- [ ] Run admitted Rust job tests and native RPC/NNTP/config suites; expected: all pass with no secret strings in public responses. Commit signed.

#### Task 5: WebUI action, progress and saved-result reconciliation

**Files:** Create `webui/tlscipher.js`, `tests/webui/tls-cipher.js`; modify `index.html`, `config.js`, `skin.css`, locale source and generated catalogs, `tests/webui/README.md`.
**Interfaces:** Consumes Task 4 RPC schema; produces `TlsCipherDialog.init()`, `showModal()` and `Config.applyCipherResults(results)`.

- [ ] Write loopback-only Playwright tests exercising real committed templates/JS with intercepted RPC: button availability, unsaved-server guard, unrelated unsaved preservation, pending/cancel/reopen, duplicate clicks, stale job, escaped names/errors, results saved versus active, browser reorder conflict, keyboard focus and last row/column at 1280/390/320 in both themes.
- [ ] Run `node tests/webui/tls-cipher.js http://127.0.0.1:17689/` with prospective capture; expected: assertions fail before implementation, with no network mutations.
- [ ] Implement the dialog and News Servers action, reuse existing rendering/locale patterns, poll at a bounded interval and stop timers on terminal state/page unload. Reopening queries current state rather than starting another run.
- [ ] Disable action for unsaved server edits or missing backend; preserve unrelated dirty values. Match server snapshot fields before applying confirmed cipher values to browser state; conflicting browser changes are reported and retained. Add existing reload-required notice after saved results.
- [ ] Run the new check plus Settings density, lists, compact tiles, locales and phone navigation against the isolated fixture; expected: zero assertion/page errors. Commit signed.

#### Task 6: End-to-end validation, local instance and screenshots

**Files:** Complete `tests/functional/tlscipher/fixture.py`; document invocation and local handoff in `docs/FUNCTIONAL_TESTING.md`; retain private evidence outside Git.
**Interfaces:** Consumes the complete candidate; produces the local runnable artifact and handoff.

- [ ] Freeze source/fixtures/commands and get scheduling-owner admission for a fresh native TLS/Rust build and focused tests; no stale archive timestamps or borrowed build-lineage claims.
- [ ] Run the real button/RPC against synthetic loopback servers with different support sets. Verify measured rank, negotiated suite, per-server persisted cipher, untouched other config bytes, reload-required state, unknown/error preservation and cancellation. Inspect actual files and RPC outcomes, not just screenshots.
- [ ] Run relevant native and UI regressions; inspect full completion records and all failures. Unsupported platform coverage is a declared limitation, never a pass.
- [ ] Prepare a separate local loopback service serving the exact built backend and candidate assets, with separate config/queue/state and synthetic providers. Record executable and source hashes, URL, start/stop commands and process ownership.
- [ ] Capture real sanitized JPEGs of initial, running and completed dialog at desktop/phone and light/dark; verify viewport, final row/column, no clipping/private values and image dimensions/hashes. Open the local instance for the user.
- [ ] Announce readiness audibly with `/usr/bin/say`, deliver screenshots and local test instructions, and stop before PR/review pending the user's g2g. Preserve all evidence and the plan ledger at this handoff.

#### Task 7: Review and PR after g2g

**Files:** Candidate runtime/docs and retained evidence as required by findings.
**Interfaces:** Consumes explicit g2g, fresh source, evidence and current remote.

- [ ] Only after g2g, obtain the required fresh independent whole-branch review, fix actionable findings with meaningful tests and rerun affected validation. Follow existing integration-owner and final-head review procedures.
- [ ] Refresh canonical default, preserve signed history, verify exact final tree/identity/signature and current-head tests. Publish only the owned feature ref using the verified HTTPS helper; verify GitHub signing and create/attach the PR.
- [ ] Integration/deployment remain with designated owners and their applicable holds; do not equate PR creation with installation.
