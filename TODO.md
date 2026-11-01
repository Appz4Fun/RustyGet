# TODO

Open items left after the 2026-11-01 clean-up (PRs #115–#118). Each one is
independent; none blocks a release.

## Repository hygiene

- **Stale remote branches.** `origin` carries 160 branches. 124 are `codex/*`,
  of which 68 are preserved pre-squash histories (`*-preserved`,
  `*-signed-history-*`, `*-signed-v*`) that delivery records on their PRs cite;
  keep those unless the records are revised. The remaining `codex/*` branches
  and the older ones (`feature/*`, `fix/*`, `test/*`, `rust-*`, `ci-*`,
  `deb-build`, `fork-ci`, `pr811-sync-fixes`, `v26.1-rebuild`, `assets/*`,
  `claude/*`) predate the current workflow: check each against `main`
  (`git diff --quiet main <branch>` plus a look at any unmerged commits) and
  delete what is superseded.
- **`dupe-article-fallback`** on origin is the former default branch, frozen at
  the same commit as PR #115's merge. Delete once nothing external links to it.
- **Origin URL** is still `git@github.com:xbmc4lyfe/nzbget` (GitHub redirects
  to `Appz4Fun/RustyGet`). The identity checks in
  `docs/DEPLOYMENT_AGENT_PROCEDURES.md` and
  `docs/BENCHMARK_AGENT_PROCEDURES.md` accept only the canonical HTTPS URL, so
  run `git remote set-url origin https://github.com/Appz4Fun/RustyGet.git` and
  use the verified credential-helper override from AGENTS.md.

## Tests

- **Differential scripts on macOS.** `cmdline_differential` crashes in its
  unsanitized pass: the parser resets getopt with `optind = 0`, a glibc
  convention; BSD libc needs `optreset = 1`. `fsops_differential` stops at
  "creating collision file" on case-insensitive APFS. Both need a
  platform-aware harness (or a documented Linux-only scope). ASan's
  `detect_container_overflow` must be off when linking the uninstrumented
  `libnzbget.a` into an instrumented harness on libc++.
- **`paths_differential` in Docker** reports `tr_TR.ISO8859-9 unavailable`;
  the verification image lacks locale sources (`locales` package or
  `/usr/share/i18n/locales/tr_TR`).
- **Timing flake** in `deadpickafterreload` / `deaddownloadrestart`: the probe
  can lose to the failure count after a reload or restart. Did not reproduce
  in the 2026-11-01 runs (354/354 on both platforms) but is not fixed.

## Upstream items deliberately not ported

- nzbgetcom #1053 replaces the webui icon font with an SVG sprite. Our webui
  diverged; worth redoing on our tree to drop the 3 MB font.
- nzbgetcom #1056 (TLS minimum protocol) collides with the cipher optimizer;
  re-evaluate if the optimizer changes.
- Version-format change, FreeBSD/musl/Android/runner commits: retired
  platforms, skip for good.
