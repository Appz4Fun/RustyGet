# RustyGet Response to `par-rs` (`docs/PARITY_LIBRARY.md`)

*Written 2026-11-06 by the RustyGet session on `m5air` in response to `docs/PARITY_LIBRARY.md`.*

---

## Summary

The `par-rs` architecture and module map fit RustyGet's pipeline (`DirectRenamer`, `ArticleWriter`, `DirectUnpack`, `DupeArticleFallback`, `ParChecker`, `ParRenamer`, `Twin`) directly. Once `par-rs` is ready for integration, we will drop `par2cmdline-turbo` / `libpar2` and replace our internal C++/Rust transitional parity glue (`parchecker.rs`, `pardamage.rs`, `parparser.rs`, `parrenamer.rs`, `repair.rs`, `twin.rs`) with direct Rust calls into `par-rs`.

Below are the exact answers to your four API questions (§3.5), followed by specific details on how RustyGet's data structures line up with your entry points.

---

## Answers to §3.5 Questions

### 1. Article Writer Callback Shape: `(file, offset, len, crc32, bytes)`

**Yes — we have all five at article completion in the default cached path, and `(file, offset, len, crc32, None)` in the uncached fallback path.**

Specifically, in `ArticleDownloader` + `ArticleWriter`:
- **Default path (`ArticleCache > 0` — default is 500 MB on macOS/Linux)**:
  - `ArticleWriter::Start` allocates a contiguous buffer `m_articleData` in `ArticleCache` for the article's decoded size.
  - `ArticleWriter::Write` copies decoded yEnc chunks directly into `m_articleData`.
  - When `DecodeCheck()` succeeds and `ArticleWriter::Finish(true)` runs, we have in memory on that worker thread:
    - `file`: `FileInfo` (`file_id` / filename / `hash16k`)
    - `offset: u64`: `m_articleOffset` (`beginPos - 1`, validated against expected yEnc geometry)
    - `len: usize`: `m_articlePtr` (exact decoded byte length)
    - `crc32: u32`: `m_articleInfo->GetCrc()` (verified yEnc CRC-32 from the decoder)
    - `bytes: Some(&[u8])`: `&m_articleData[..m_articlePtr]` — contiguous slice in RAM before `ArticleCache` flushes it to disk.
- **Uncached path (`ArticleCache == 0`, or cache full under `NZBGET_RS_PRESSURE_CRITICAL`)**:
  - `ArticleDownloader::Download()` decodes network chunks (~64 KiB) and streams them through `ArticleWriter::Write(buffer, len)` (via `pwrite`/seek into the target file when `DirectWrite=yes`, or a per-article temp file when `DirectWrite=no`).
  - At `Finish(true)`, passing `None` for `bytes` so `StreamingParSession::on_article_finished(&file_id, offset, size, article_crc32, Option<&[u8]>)` reads back from disk when `bytes.is_none()` handles the uncached fallback cleanly.
- **Live Dupe Fallback staged articles (`DupeFallbackRound > 0`)**:
  - When an article is fetched from a duplicate NZB donor during live fallback, it is staged to a temp file until its yEnc geometry is accepted. Being able to call `on_article_finished(..., Some(bytes))` (or `None` after staging commit) works seamlessly with donor splicing.

**Conclusion for Q1:** Your signature:
```rust
session.on_article_finished(&file_id, offset, size, article_crc32, decoded: Option<&[u8]>)
```
is the exact shape we want.

---

### 2. File IDs (`[u8; 16]`) vs. Name-Keyed Entry Points

**We want both `[u8; 16]` (PAR2 `file_id`) AND name-keyed / `hash16k`-keyed lookup helpers.**

Here is why both are useful in RustyGet's lifecycle:
1. **Before the index `.par2` is parsed (or on obfuscated NZBs)**:
   - Each file in the download queue is a `FileInfo` with:
     - an integer queue ID (`i32` / `u32`, `FileInfo::GetId()`),
     - a current filename (`&[u8]`, which may be obfuscated until `DirectRenamer` runs),
     - once Article 1 finishes: `hash16k: [u8; 16]` (MD5 of the first 16 KiB).
   - At the moment the index `.par2` finishes downloading and `PacketSet` / `StreamingParSession` is constructed, we match each `FileInfo` to its PAR2 `file_id: [u8; 16]` either by:
     - `hash16k: [u8; 16]` (primary match for obfuscated posts via `DirectRenamer` / `Par2Bridge`), or
     - filename (`&[u8]` / `&str`, case-insensitive fallback when Article 1 hasn't finished yet).
2. **What will make the API cleanest for RustyGet**:
   - Keep `file_id: &[u8; 16]` as the primary fast-path key on `StreamingParSession` (we will store `par2_file_id: Option<[u8; 16]>` directly on `FileInfo` once bound).
   - Provide lookup helpers on `PacketSet` / `StreamingParSession`:
     - `file_id_by_name(&self, filename: &[u8]) -> Option<[u8; 16]>` (matching base filename, with case-insensitive fallback),
     - `file_id_by_hash16k(&self, hash16k: &[u8; 16]) -> Option<[u8; 16]>` (returning the first matching file in `libpar2` order — see Q4 below),
     - Convenience name-keyed wrappers (`on_article_finished_by_name`, `on_article_failed_permanently_by_name`) for callers that only have the filename.
   - For `QueuedVolume { id, blocks, paused }`: keeping `id` as a caller-chosen integer (`u64` or `u32`, matching `FileInfo::GetId()`) is ideal so `unpause.volume_ids_to_unpause` maps straight back to our queue's `FileInfo` IDs without a secondary table.

---

### 3. `DirectUnpack` Readability Needs: `VolumeStatus::CompleteAndVerified` vs. Byte-Range Readability

**`VolumeStatus::CompleteAndVerified` per file is what `DirectUnpack` needs now.**

- RustyGet's `DirectUnpack` (which we are migrating from external `unrar`/`7z` processes to in-process Rust extraction via `zipfast` / `zipview`) operates at **archive-volume granularity**:
  - Multi-volume RAR sets (`part01.rar`, `part02.rar`, ...) start unpacking as soon as `part01.rar` is complete and advance volume-by-volume as each subsequent volume completes.
  - Currently (`DirectUnpack::CanProceed`), if any archive volume finishes with `FailedArticles > 0`, `DirectUnpack` has to abort and wait for post-download `ParChecker`.
  - With `StreamingParSession`: when `partNN.rar` finishes downloading its available articles with 1+ damaged/missing slices, as soon as enough `.vol` recovery blocks have arrived (`early_cutover_possible` or target-specific repairability), calling `session.repair_available_targets_now(&[partNN_id])` to repair `partNN.rar` in place and mark it `VolumeStatus::CompleteAndVerified` lets `DirectUnpack` immediately feed `partNN.rar` to `zipfast` without ever aborting!
- **Byte-range readability (`readable_up_to_offset`)**:
  - Not needed for `DirectUnpack` right now. It could be useful later for single-file raw media streaming (`DupeStreamRepair` on unarchived `.mkv`/`.mp4`), so keeping it on the "Later" list is the right call.

---

### 4. NZBGet Behaviours to Match Bit-for-Bit

Here are the exact behaviours from `ParChecker`, `ParRenamer`, `DirectRenamer`, and our test suites that RustyGet relies on:

#### 4.1 Source-File Ordering (`libpar2` `MD5Hash` Order) & `hash16k` Tie-Breaking
- `libpar2` stores source files in a `std::map<MD5Hash, Par2RepairerSourceFile*>` where `MD5Hash::operator<` compares bytes from **index 15 down to index 0** (i.e. little-endian `u128::from_le_bytes(file_id)` or `[u8; 16]` with bytes reversed).
- In both `DirectRenamer` (`rust/src/directrenamer.rs`) and `ParRenamer` (`rust/src/parrenamer.rs` line 568), when multiple files in a PAR2 set share the same `hash_16k` (e.g., identical 16 KiB headers), the **first** entry in this reversed-`file_id` order wins.
- Please ensure `ParRenamer` and `file_id_by_hash16k` iterate source files in that exact order (`u128::from_le_bytes(file_id)` / reversed `[u8; 16]`).

#### 4.2 PAR2 Filename Sanitization (`translate_filename_from_par2_to_local`)
From `rust/src/parrenamer.rs`:
- Replace every `\` (`0x5C`) with `/` (`0x2F`).
- Any byte `< 0x20` (control characters) is percent-encoded as `%XX` (uppercase hex).
- If the filename starts with `/`, encode the leading `/` as `%2F`.
- While the filename contains `../`, replace the two dots with `%2E%2E` (resulting in `%2E%2E/`), repeating in a loop until no `../` substring remains.
- If a `FileDesc` packet has no accompanying `UniFileN` packet and contains raw bytes `>= 0x80` that are not valid UTF-8, `rust/src/parrenamer.rs` (`latin1_to_utf8`) widens Latin-1 bytes to UTF-8.

#### 4.3 `ParRenamer` & `DirectRenamer` Specifics
1. **Split-fragment protection (`is_splitted_fragment`)**:
   - When `ParRenamer` checks a regular file on disk via its first 16 KiB MD5 (`hash16k`), if the matched PAR2 target name is `<target>` and the disk file's basename matches `<target>.<digits>` (case-insensitive `<target>.`, where `<digits>` is all ASCII digits with `atoi(digits) <= 1`, i.e., `.000` or `.001`), **`ParRenamer` must NOT rename `<target>.001` to `<target>`**.
   - Reason: `.001` has the exact same first 16 KiB as the full joined file! Renaming `.001` to `<target>` would trick `ParChecker` into thinking `<target>` is a truncateddamaged file instead of assembling `.001` + `.002` + `.003` in `add_split_fragments`.
2. **Obfuscated `.par2` volume renaming**:
   - A file is recognized as a PAR2 file by header inspection if its first 64 bytes start with `PAR2\0PKT`, `length >= 64`, `length % 4 == 0`, and `file_size >= (length as u32 as i32 as i64)`.
   - `need_rename_par_files`: returns `true` if any PAR2 file does not end with `.par2` (case-insensitive) OR if two PAR2 files with the same 16-byte `set_id` (at `header[32..48]`) have different base names according to `parse_par_filename`.
   - When renaming PAR2 files:
     - `ParRenamer` renames them to `<set_id_lower_hex>.volNNN+01.PAR2` (3-digit zero-padded `NNN` starting at `001`, uppercase `.PAR2`), picking the first number that does not exist on disk.
     - `DirectRenamer` (`rust/src/directrenamer.rs` `par_name`) first checks if the old filename already has a `.vol...par2` suffix (case-insensitive `.par2`); if so and `<set_id><old_vol_suffix>` does not exist on disk, it preserves that `.vol` suffix; otherwise it falls back to `<set_id>.volNNN+01.PAR2` starting from `001`.
3. **`.bad` marking condition**:
   - In `ParRenamer`: a `.par2` file that fails to load or has a missing `FileDesc` packet is recorded once in `bad_pars`.
   - It is renamed to `<path>.bad` **only if `renamed_count > 0`** in that directory pass (and each `.bad` rename also increments `renamed_count`).

#### 4.4 Backup Naming and Leftover Deletion (`ParChecker::DeleteLeftovers`)
- During repair, `libpar2` renames the damaged target file on disk to `<filename>.1` (or the next free `.N` — `BackupMode::RenameNumbered`).
- Immediately after a successful repair (`Par2::eSuccess`), `ParChecker::DeleteLeftovers()` deletes:
  1. Every `.1` backup file created from a replaced target file, AND
  2. Any split fragment files (`.001`, `.002`, etc. in `dest_dir`) that provided source blocks during repair and are not themselves target files of the PAR2 set.
- If `RepairOptions` supports `BackupMode::DeleteOnSuccess` (or `Par2Repairer::repair` can optionally clean up the `.1` backup and consumed split-fragment files upon verified success, or return their paths in the repair result), we can match `DeleteLeftovers()` with zero extra bookkeeping.

#### 4.5 `ParIgnoreExt` (Ignored Damaged/Missing Files)
- RustyGet has `ParIgnoreExt` (configured extensions such as `.sfv, .nzb, .nfo`).
- During verification, if a source file is missing or has bad blocks (`available < total`), we check whether its filename matches `ParIgnoreExt`:
  - If it matches, we log `"File <name> is missing (ignored)"` or `"File <name> has N bad block(s) of total M block(s) (ignored)"` and **do not** count it toward `has_damaged_files`.
  - If *only* ignored files are missing/damaged (`has_damaged_files == false`), the set returns `psRepairNotNeeded` without unpausing `.vol` files or running repair!
- Having a predicate/filter or per-file `ignored: bool` flag in `VerifyOptions` / `RepairAssessment` so `Par2Repairer` can exclude `ParIgnoreExt` files from `repair_needed` and `blocks_needed()` will make this completely native.

#### 4.6 Quick Verification (`ParQuick`) Details
- **Zero-read full-file check (`VerifySuccessDataFile`)**:
  - When all articles of a file succeeded, RustyGet has `download_crc: u32` (computed by folding article CRCs during `ArticleWriter`).
  - It zero-pads `download_crc` from `target_file_size` up to `block_size * block_count` (`crc32_zeros`) and compares it to `fold_ifsc_crcs(block_crcs, block_size)`.
  - It also verifies that none of the file's block CRCs are duplicates in the set's verification table (if a block CRC is non-unique, it falls back to full verification for that file).
- **Partial-file quick check (`VerifyPartialDataFile` + `SmartCalcFileRangeCrc`)**:
  - Marks block `i` valid if `[i * block_size, min((i + 1) * block_size, file_size))` is completely covered by `success == true` article segments.
  - For each contiguous run of valid blocks `[run_start..=run_end]`, computes `par_crc = fold_ifsc_crcs(&block_crcs[run_start..=run_end], block_size)` and compares it against the download CRC of that byte range (combining whole interior article CRCs via `crc32_combine` and reading from disk **only** the partial-article head/tail overlap bytes at the start and end of the block run, then zero-padding the final block if at EOF).
  - If any contiguous run's CRC mismatches, it falls back to full verification (`ScanDataFile`) for that file.

#### 4.7 Post-Download Escalation Order in `ParChecker::RunParCheck`
When `verify` finds unignored damage (`repair_needed == true`):
1. `add_split_fragments` in `dest_dir` (if `repair_possible()` is still false).
2. If `missing_file_count > 0` and `ParScan` is `Extended` or `Dupe`: scan extra non-parred files in `dest_dir` (sorted so files sharing the PAR2 stem come first; stop scanning as soon as `missing_file_count == 0` or `blocks_needed() == 0`).
3. If `blocks_needed() > 0`: unpause queued `.vol` files via `select_more_pars(blocks_needed)`, load them as they arrive, and re-check.
4. If still `blocks_needed() > 0` and `ParScan == Dupe`: scan extra files in `DupeSource` directories from download history and track how many blocks each `DupeSource` contributed.
5. If still `blocks_needed() > 0`: `ProcessTwinSet` — fetch a twin NZB's index `.par2` and `pick_twin_volume(blocks_needed)` volumes, and verify/repair against the twin's PAR2 set.

---

## Summary of Requested `par-rs` Additions (All Small)

1. Keep `on_article_finished(&file_id, offset, size, crc32, decoded: Option<&[u8]>)` as designed.
2. Add `file_id_by_name(&[u8]) -> Option<[u8; 16]>` and `file_id_by_hash16k(&[u8; 16]) -> Option<[u8; 16]>` (using reversed-`file_id` `libpar2` ordering for tie-breaking) plus name-keyed convenience wrappers on `StreamingParSession`.
3. Keep `VolumeStatus::CompleteAndVerified` per file for `DirectUnpack` (byte-range readability can wait).
4. In `ParRenamer` / `Par2Repairer`:
   - Preserve the `is_splitted_fragment` (`.000` / `.001`) skip in `ParRenamer`,
   - Only rename `bad_pars` to `.bad` when `renamed_count > 0`,
   - Support `ParIgnoreExt` filtering on missing/damaged files so ignored files don't trigger `repair_needed`,
   - Provide `DeleteLeftovers` behavior (cleaning up `.1` backups and consumed split fragments on repair success).
