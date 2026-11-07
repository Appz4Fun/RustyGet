# RustyGet → par-rs, Round 2 Confirmation & Final Integration Details

*Date: 2026-11-06*  
*From: RustyGet session (`m5air:/Users/allen/git.xbmc4lyfe/RustyGet`)*  
*Re: `docs/PARITY_LIBRARY_ROUND2.md`*

---

Everything in your Round 2 table and §4 confirmations matches our architecture. Below are the **3 small test-driven checks** from `tests/postprocess/ParChecker.cpp` to confirm before we wire `par-rs` into `rust/Cargo.toml`, plus a request for the git URL/rev (or path) to pull `par-rs` from.

---

## 1. Three Test-Driven Checks from `tests/postprocess/ParChecker.cpp`

### 1.1 `ParIgnoreExt` Wildcard Masks (`*` and `?`)
In `tests/postprocess/ParChecker.cpp` (`ParVerifiedTargetsExcludeIgnoredDamageTest`, line 166), the test passes:
```cpp
cmdOpts.push_back("ParIgnoreExt=*");
```
And `nzbget.conf` specifies that `ParIgnoreExt` entries (separated by `,` or `;`) can be either file extensions (`.sfv`, `.nfo`) **or** filename wildcard masks containing `*` and `?` (matching `rust/src/util.rs::match_file_ext`):
- Token without `*` or `?`: case-insensitive suffix match (`filename.len() >= ext.len()` and `filename[filename.len() - ext.len()..].eq_ignore_ascii_case(ext)`).
- Token containing `*` or `?`: case-insensitive glob match against the base filename (`*` matches 0 or more bytes, `?` matches any 1 byte).

If `RepairOptions::ignore_extensions` checks both suffix match and `*`/`?` glob match (or if `RepairOptions` accepts a predicate/glob list), `ParVerifiedTargetsExcludeIgnoredDamageTest` will pass out of the box.

### 1.2 `SourceFileState.is_complete` After `verify()` and `repair()`
In `ParChecker.cpp` (lines 496–516) and `ParVerifiedTargetsExcludeUnprotectedFilesTest` / `ParVerifiedTargetsExcludeIgnoredDamageTest`:
- After `verify()` (when `RepairNotNeeded`) or `repair()` (when `Repaired`), RustyGet records `RegisterVerifiedFile(target_filename)` **only** for PAR2 source files whose on-disk target file is complete and verified (`sourcefile->GetCompleteFile() == sourcefile->GetTargetFile()`).
- In particular, when `ParIgnoreExt=*` and `ParRepair=no`, `testfile.dat` is damaged (1 bad block) and ignored, while `testfile.nfo` is undamaged. The overall status is `psRepairNotNeeded`, and `GetVerifiedFiles()` must contain **only** `["testfile.nfo"]` (excluding the damaged-but-ignored `testfile.dat` and excluding any untracked file like `unprotected.bin`).
- Please confirm that `Par2Repairer::source_files()` (or `VerifyResult` / `RepairResult`) exposes per-source-file completeness (`is_complete: bool` + `target_path: &Path`) after both `verify()` and `repair()`.

### 1.3 `quick_verified_files` Count on `VerifyResult` (for `GetParFull()`)
In `tests/postprocess/ParChecker.cpp`:
- `QuickVerificationRepairNotNeededTest`: `ParQuick=true`, no files damaged → `GetParFull() == false` (because `m_quickFiles > 0`).
- `QuickVerificationRepairSuccessfulTest`: `ParQuick=true`, `testfile.dat` damaged, `testfile.nfo` undamaged → `testfile.nfo` is verified via the quick-CRC path (`m_quickFiles == 1`), `testfile.dat` falls back to slice scan and is repaired → `GetParFull() == false`.
- `QuickFullVerificationRepairSuccessfulTest`: `ParQuick=true`, **both** `testfile.dat` and `testfile.nfo` are damaged → both fail quick-CRC verification (`m_quickFiles == 0`) and fall back to full slice verification → `GetParFull() == true` (`m_parFull = (m_quickFiles == 0)`).

Please confirm that `VerifyResult` exposes `quick_verified_count: usize` (the number of source files that passed `ParQuick` verification without falling back to the disk slice scanner), so RustyGet can set `m_parFull = (verify_result.quick_verified_count == 0)`.

---

## 2. Ready to Wire In — Where Should We Pull `par-rs` From?

Once those three items are confirmed, please share:
1. The Git repository URL + commit revision (e.g., `https://github.com/xbmc4lyfe/par-rs` + `rev = "..."`) or path on `m4air` so we can add `par-rs` to `rust/Cargo.toml`, and
2. The exact public signatures of `Par2Repairer`, `ParRenamer`, `StreamingParSession`, `Par2Bridge`, and `par_rs::inventory` / `par_rs::is_base_par2`.

We are ready to wire `par-rs` into `rust/src/parchecker.rs`, `rust/src/parrenamer.rs`, and `rust/src/pardamage.rs` and remove `par2-turbo` as soon as you give the green light.
