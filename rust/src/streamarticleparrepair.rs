// Copyright (C) 2026 Denis <denis@nzbget.com>
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::dupefallback::NzbgetRsByteRange;

/// Resolves the estimated donor part index for `offset` corrected by `drift`.
/// Mirrors `DonorMemberSource::PartForOffset` and `DonorMemberSource::Prefetch` (`indexOf`)
/// in `StreamRepair.cpp`.
pub fn sap_stream_repair_donor_index_for_offset(
    offset: i64,
    drift: i64,
    ranges: &[NzbgetRsByteRange],
) -> i32 {
    let target = offset.saturating_sub(drift);
    let mut index: usize = 0;
    while index + 1 < ranges.len()
        && ranges[index].offset.saturating_add(ranges[index].size) <= target
    {
        index += 1;
    }
    index as i32
}

/// Checks whether the donor part cache has reached capacity and can evict its oldest entry.
/// Mirrors `DonorMemberSource::StoreFetched` in `StreamRepair.cpp`.
pub fn sap_stream_repair_can_evict_donor_cache(
    cache_len: usize,
    max_cached_parts: i32,
    order_len: usize,
) -> bool {
    (cache_len as i32) >= max_cached_parts && order_len > 0
}

/// Returns the missing byte count contributed by a repair target to the watchdog total.
/// Mirrors `StreamRepairController::StartWatchdog` in `StreamRepair.cpp`.
pub fn sap_stream_repair_target_missing_bytes(
    decoded_file_size: i64,
    holes_total_size: i64,
    encoded_size: i64,
) -> i64 {
    if decoded_file_size > 0 {
        holes_total_size
    } else {
        encoded_size
    }
}

/// Computes the average download rate (bytes/sec) for the stream repair watchdog.
/// Mirrors `StreamRepairController::StartWatchdog` in `StreamRepair.cpp`.
pub fn sap_stream_repair_watchdog_calc_download_rate(
    download_time_sec: i32,
    all_bytes: i64,
) -> i64 {
    if download_time_sec > 0 {
        all_bytes / i64::from(download_time_sec)
    } else {
        0
    }
}

/// Checks whether an existing on-disk file for a whole-file target is an interrupted
/// earlier recreation of the same size (or empty) and can be replaced.
/// Mirrors `StreamRepairController::RepairWholeFile` in `StreamRepair.cpp`.
pub fn sap_stream_repair_whole_file_existing_reusable(
    existing_size: i64,
    fetched_file_size: i64,
) -> bool {
    existing_size == fetched_file_size || existing_size == 0
}

/// Checks whether enough overlap probe parts have been pooled into `probeParts`
/// and remaining candidates should go to `reserve`.
/// Mirrors `StreamRepairController::VerifyDonor` in `StreamRepair.cpp`.
pub fn sap_stream_repair_verify_donor_pool_stop(
    probe_count: usize,
    min_probe_count: i32,
    pooled: i64,
    base_floor: i64,
) -> bool {
    let pc = probe_count as i32;
    pc >= min_probe_count && (pooled >= base_floor || pc >= min_probe_count.saturating_mul(2))
}

/// Checks whether `PatchFromDonor` should stop after pass 1 because no donor part
/// was fetched or the measured drift was zero.
/// Mirrors `StreamRepairController::PatchFromDonor` in `StreamRepair.cpp`.
pub fn sap_stream_repair_patch_should_break_pass2(fetched_any: bool, drift: i64) -> bool {
    !fetched_any || drift == 0
}

/// Computes the article-fetch budget for building a donor `ContentMap` of `member_count` members.
/// Mirrors `StreamRepairController::ExecCrossPackRepair` in `StreamRepair.cpp`.
pub fn sap_stream_repair_mapping_budget(member_count: usize) -> i32 {
    8i32.saturating_add((member_count as i32).saturating_mul(4))
}

/// Computes the achievable comparable ciphertext bytes for a probe window clipped by
/// `last_computable` and `chain_start`.
/// Mirrors `StreamRepairController::VerifyEncryptedFromSource` in `StreamRepair.cpp`.
pub fn sap_stream_repair_encrypted_achievable_bytes(
    window_offset: i64,
    window_size: i64,
    last_computable: i64,
    chain_start: i64,
) -> i64 {
    let window_end = window_offset.saturating_add(window_size).min(last_computable);
    let from = window_offset.max(chain_start);
    if from < window_end {
        window_end - from
    } else {
        0
    }
}

/// Computes the per-mille stage progress `(completed_articles * 1000 / total_articles)`
/// during donor set materialization.
/// Mirrors `StreamRepairController::MaterializeDonorSet` in `StreamRepair.cpp`.
pub fn sap_stream_repair_materialize_stage_progress(
    completed_articles: i64,
    total_articles: i64,
) -> i32 {
    if total_articles > 0 {
        (completed_articles.saturating_mul(1000) / total_articles) as i32
    } else {
        0
    }
}

/// Determines whether a run at `run_member_index` should be kept when excluding
/// `exclude_member_index`, and whether its parallel `RunCrypto` entry should also be kept.
/// Mirrors `ContentMap::ExcludeMember` in `ContentMap.cpp`.
pub fn sap_content_map_exclude_member_step(
    run_member_index: i32,
    exclude_member_index: i32,
    cryptos_len: usize,
    runs_len: usize,
) -> (bool, bool) {
    let keep_run = run_member_index != exclude_member_index;
    let keep_crypto = keep_run && cryptos_len == runs_len;
    (keep_run, keep_crypto)
}

/// Selects the log message mode for `ArticleWriter::LogStartMessage`:
/// `0` = Moving, `1` = Writing, `2` = Checking, `3` = Joining.
/// Mirrors `ArticleWriter::LogStartMessage` in `ArticleWriter.cpp`.
pub fn sap_article_writer_log_start_mode(
    raw_article: bool,
    direct_write: bool,
    cached: bool,
) -> i32 {
    if raw_article {
        0
    } else if direct_write && cached {
        1
    } else if direct_write {
        2
    } else {
        3
    }
}

/// Determines whether `ArticleWriter::FlushCache` should write directly into the target output file.
/// Mirrors `ArticleWriter::FlushCache` in `ArticleWriter.cpp`.
pub fn sap_article_writer_flush_is_direct_write(
    direct_write_opt: bool,
    force_direct_write: bool,
    output_initialized: bool,
) -> bool {
    (direct_write_opt || force_direct_write) && output_initialized
}

/// Determines whether `ArticleWriter::FlushCache` should abort immediately because the file
/// was deleted and is not being parked.
/// Mirrors `ArticleWriter::FlushCache` in `ArticleWriter.cpp`.
pub fn sap_article_writer_flush_should_abort_deleted(
    file_deleted: bool,
    nzb_parking: bool,
) -> bool {
    file_deleted && !nzb_parking
}

/// Computes the updated allocated byte total and whether `DeleteCacheFlag()` should be called
/// when freeing a cached article segment.
/// Mirrors `ArticleCache::Free` in `ArticleWriter.cpp`.
pub fn sap_article_cache_free_eval(
    segment_size: i32,
    allocated_before: usize,
    server_mode: bool,
    continue_partial: bool,
) -> (usize, bool) {
    let new_allocated = allocated_before.wrapping_sub(segment_size as usize);
    let delete_cache_flag = new_allocated == 0 && server_mode && continue_partial;
    (new_allocated, delete_cache_flag)
}

/// Checks whether `ParChecker::SignalFilename` should transition `m_stage` from
/// `ptRepairing` to `ptVerifyingRepaired`.
/// Mirrors `ParChecker::SignalFilename` in `ParChecker.cpp`.
pub fn sap_parchecker_should_mark_verifying_repaired(stage: i32, pt_repairing: i32) -> bool {
    stage == pt_repairing
}

/// Checks whether `ParChecker::SignalFilename` should record the filename in `m_processedFiles`.
/// Mirrors `ParChecker::SignalFilename` in `ParChecker.cpp`.
pub fn sap_parchecker_should_record_processed_file(
    stage: i32,
    pt_loading_pars: i32,
    pt_verifying_sources: i32,
) -> bool {
    stage == pt_loading_pars || stage == pt_verifying_sources
}

/// Checks whether an NZB entry is a valid `.par2` candidate smaller than the current best
/// and within `max_index_size`.
/// Mirrors `RepairController::PostParChecker::RequestTwinIndex` in `Repair.cpp`.
pub fn sap_repair_ctrl_is_better_twin_par2_entry(
    is_par2: bool,
    entry_size: i64,
    has_current_best: bool,
    current_best_size: i64,
    max_index_size: i64,
) -> bool {
    is_par2
        && entry_size <= max_index_size
        && (!has_current_best || entry_size < current_best_size)
}

/// Computes `(start, nr_entries)` for a binary RPC log slice request.
/// Mirrors `LogBinCommand::Execute` in `BinRpc.cpp`.
pub fn sap_binrpc_log_slice_bounds(
    lines: i32,
    id_from: u32,
    messages_len: usize,
    first_msg_id: u32,
) -> (i32, i32) {
    let msg_count = messages_len as i32;
    let mut nr_entries = if lines < 0 { 0 } else { lines };
    let mut start = msg_count;
    if nr_entries > 0 {
        if nr_entries > msg_count {
            nr_entries = msg_count;
        }
        start = msg_count - nr_entries;
    }
    if id_from > 0 && messages_len > 0 {
        start = id_from.wrapping_sub(first_msg_id) as i32;
        if start < 0 {
            start = 0;
        }
        nr_entries = msg_count.wrapping_sub(start);
        if nr_entries < 0 {
            nr_entries = 0;
        }
    }
    (start, nr_entries)
}

/// Checks whether `EditQueueBinCommand::Execute` should format `offset` as the edit text
/// instead of using the trailing text buffer.
/// Mirrors `EditQueueBinCommand::Execute` in `BinRpc.cpp`.
pub fn sap_binrpc_edit_queue_use_offset_str(
    action: i32,
    ea_file_move_offset: i32,
    ea_group_move_offset: i32,
) -> bool {
    action == ea_file_move_offset || action == ea_group_move_offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sap_stream_repair_donor_index_for_offset() {
        let ranges = [
            NzbgetRsByteRange {
                offset: 0,
                size: 1000,
            },
            NzbgetRsByteRange {
                offset: 1000,
                size: 1000,
            },
            NzbgetRsByteRange {
                offset: 2000,
                size: 1000,
            },
        ];
        assert_eq!(sap_stream_repair_donor_index_for_offset(0, 0, &ranges), 0);
        assert_eq!(sap_stream_repair_donor_index_for_offset(999, 0, &ranges), 0);
        assert_eq!(
            sap_stream_repair_donor_index_for_offset(1000, 0, &ranges),
            1
        );
        assert_eq!(
            sap_stream_repair_donor_index_for_offset(2500, 600, &ranges),
            1
        );
        assert_eq!(
            sap_stream_repair_donor_index_for_offset(9000, 0, &ranges),
            2
        );
    }

    #[test]
    fn test_sap_stream_repair_can_evict_donor_cache() {
        assert!(sap_stream_repair_can_evict_donor_cache(16, 16, 1));
        assert!(!sap_stream_repair_can_evict_donor_cache(15, 16, 1));
        assert!(!sap_stream_repair_can_evict_donor_cache(16, 16, 0));
    }

    #[test]
    fn test_sap_stream_repair_target_missing_bytes_and_rate() {
        assert_eq!(sap_stream_repair_target_missing_bytes(5000, 1200, 6000), 1200);
        assert_eq!(sap_stream_repair_target_missing_bytes(0, 1200, 6000), 6000);
        assert_eq!(sap_stream_repair_watchdog_calc_download_rate(10, 50_000), 5000);
        assert_eq!(sap_stream_repair_watchdog_calc_download_rate(0, 50_000), 0);
    }

    #[test]
    fn test_sap_stream_repair_whole_file_and_verify_and_patch() {
        assert!(sap_stream_repair_whole_file_existing_reusable(0, 1024));
        assert!(sap_stream_repair_whole_file_existing_reusable(1024, 1024));
        assert!(!sap_stream_repair_whole_file_existing_reusable(512, 1024));

        assert!(!sap_stream_repair_verify_donor_pool_stop(2, 3, 10_000, 4096));
        assert!(sap_stream_repair_verify_donor_pool_stop(3, 3, 4096, 4096));
        assert!(!sap_stream_repair_verify_donor_pool_stop(3, 3, 2048, 4096));
        assert!(sap_stream_repair_verify_donor_pool_stop(6, 3, 2048, 4096));

        assert!(sap_stream_repair_patch_should_break_pass2(false, 100));
        assert!(sap_stream_repair_patch_should_break_pass2(true, 0));
        assert!(!sap_stream_repair_patch_should_break_pass2(true, -50));

        assert_eq!(sap_stream_repair_mapping_budget(5), 28);
    }

    #[test]
    fn test_sap_stream_repair_encrypted_and_materialize() {
        assert_eq!(
            sap_stream_repair_encrypted_achievable_bytes(100, 500, 450, 150),
            300
        );
        assert_eq!(
            sap_stream_repair_encrypted_achievable_bytes(500, 100, 450, 150),
            0
        );
        assert_eq!(sap_stream_repair_materialize_stage_progress(25, 100), 250);
        assert_eq!(sap_stream_repair_materialize_stage_progress(25, 0), 0);
    }

    #[test]
    fn test_sap_content_map_and_article_writer() {
        assert_eq!(
            sap_content_map_exclude_member_step(1, 2, 4, 4),
            (true, true)
        );
        assert_eq!(
            sap_content_map_exclude_member_step(2, 2, 4, 4),
            (false, false)
        );
        assert_eq!(
            sap_content_map_exclude_member_step(1, 2, 0, 4),
            (true, false)
        );

        assert_eq!(sap_article_writer_log_start_mode(true, true, true), 0);
        assert_eq!(sap_article_writer_log_start_mode(false, true, true), 1);
        assert_eq!(sap_article_writer_log_start_mode(false, true, false), 2);
        assert_eq!(sap_article_writer_log_start_mode(false, false, true), 3);

        assert!(sap_article_writer_flush_is_direct_write(true, false, true));
        assert!(sap_article_writer_flush_is_direct_write(false, true, true));
        assert!(!sap_article_writer_flush_is_direct_write(true, true, false));

        assert!(sap_article_writer_flush_should_abort_deleted(true, false));
        assert!(!sap_article_writer_flush_should_abort_deleted(true, true));

        assert_eq!(
            sap_article_cache_free_eval(512, 512, true, true),
            (0, true)
        );
        assert_eq!(
            sap_article_cache_free_eval(256, 512, true, true),
            (256, false)
        );
    }

    #[test]
    fn test_sap_par_repair_and_binrpc() {
        assert!(sap_parchecker_should_mark_verifying_repaired(2, 2));
        assert!(!sap_parchecker_should_mark_verifying_repaired(1, 2));

        assert!(sap_parchecker_should_record_processed_file(0, 0, 1));
        assert!(sap_parchecker_should_record_processed_file(1, 0, 1));
        assert!(!sap_parchecker_should_record_processed_file(2, 0, 1));

        assert!(sap_repair_ctrl_is_better_twin_par2_entry(
            true, 40_000, false, 0, 512_000
        ));
        assert!(sap_repair_ctrl_is_better_twin_par2_entry(
            true, 20_000, true, 40_000, 512_000
        ));
        assert!(!sap_repair_ctrl_is_better_twin_par2_entry(
            true, 600_000, false, 0, 512_000
        ));

        assert_eq!(sap_binrpc_log_slice_bounds(5, 0, 20, 100), (15, 5));
        assert_eq!(sap_binrpc_log_slice_bounds(-1, 0, 20, 100), (20, 0));
        assert_eq!(sap_binrpc_log_slice_bounds(0, 110, 20, 100), (10, 10));
        assert_eq!(sap_binrpc_log_slice_bounds(0, 90, 20, 100), (0, 20));

        assert!(sap_binrpc_edit_queue_use_offset_str(1, 1, 10));
        assert!(sap_binrpc_edit_queue_use_offset_str(10, 1, 10));
        assert!(!sap_binrpc_edit_queue_use_offset_str(2, 1, 10));
    }
}
