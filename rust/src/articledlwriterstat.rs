// This file is part of nzbget. See <https://nzbget.com>.
//
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

//! Hot-path helpers for `ArticleDownloader`, `ArticleWriter`, `MemoryMonitor`,
//! `StatMeter`, `ServerPool`, and `XmlReader` (batch 46).

/// Returns `true` if `ArticleDownloader::Run` should keep waiting for a pool connection.
pub fn aws_article_dl_should_wait_for_connection(
    has_conn: bool,
    is_stopped: bool,
    config_gen: i32,
    pool_gen: i32,
) -> bool {
    !has_conn && !(is_stopped || config_gen != pool_gen)
}

/// Returns `true` if a failed connection attempt was rejected due to permanent auth failure.
pub fn aws_article_dl_eval_auth_rejected(
    connected: bool,
    has_conn: bool,
    conn_auth_rejected: bool,
) -> bool {
    !connected && has_conn && conn_auth_rejected
}

/// Returns `true` if `ArticleDownloader::Run` should block the server after a failed connect.
pub fn aws_article_dl_should_block_failed_server(
    connected: bool,
    has_conn: bool,
    is_stopped: bool,
) -> bool {
    !connected && has_conn && !is_stopped
}

/// Returns `true` if `FreeConnection` should keep the socket connected when not retrying the same server.
pub fn aws_article_dl_keep_conn_on_no_retry(
    status: i32,
    ad_finished: i32,
    ad_not_found: i32,
) -> bool {
    status == ad_finished || status == ad_not_found
}

/// Evaluates level exhaustion in `ArticleDownloader::Run`.
/// Returns `(advance_level, new_level, break_all_failed)`.
pub fn aws_article_dl_eval_level_exhaustion(
    all_failed: bool,
    level: i32,
    max_norm_level: i32,
) -> (bool, i32, bool) {
    if !all_failed {
        return (false, level, false);
    }
    if level < max_norm_level {
        (true, level.saturating_add(1), false)
    } else {
        (false, level, true)
    }
}

/// Returns `true` if `ArticleWriter::Start` needs to enter the direct-write output file initialization block.
pub fn aws_article_writer_should_init_direct_write_file(
    direct_write: bool,
    is_dupe_fallback: bool,
    output_initialized: bool,
    skip_disk_write: bool,
    file_exists: bool,
) -> bool {
    direct_write && !is_dupe_fallback && (!output_initialized || (!skip_disk_write && !file_exists))
}

/// Returns `true` if `ArticleWriter::Start` should keep waiting while the article cache is flushing.
pub fn aws_article_writer_should_wait_for_cache_flush(
    has_data: bool,
    cache_flushing: bool,
    is_critical: bool,
) -> bool {
    !has_data && cache_flushing && !is_critical
}

/// Returns `true` if `ArticleWriter::Write` should copy into the in-memory article cache buffer.
pub fn aws_article_writer_should_memcpy_cached_segment(
    raw_article: bool,
    has_cached_data: bool,
) -> bool {
    !raw_article && has_cached_data
}

/// Returns `true` if `ArticleWriter::Finish` should move the temporary file into the result file.
pub fn aws_article_writer_should_move_temp_to_result(
    direct_write: bool,
    has_cached_data: bool,
    skip_disk_write: bool,
) -> bool {
    !direct_write && !has_cached_data && !skip_disk_write
}

/// Returns `true` if `ArticleWriter::Finish` should realloc the cached buffer to `article_ptr`.
pub fn aws_article_writer_should_realloc_cached_segment(
    article_size: i32,
    article_ptr: i32,
) -> bool {
    article_size != article_ptr
}

/// Classifies the output file setup scenario in `ArticleWriter::SetupOutputFile`:
/// `1` = standard assembly (`.tmp`), `2` = direct write (`omReadWrite`), `3` = raw mode (create dir), `0` = none.
pub fn aws_article_writer_classify_setup_output_scenario(
    raw_article: bool,
    direct_write: bool,
) -> i32 {
    if !raw_article && !direct_write {
        1
    } else if direct_write {
        2
    } else if raw_article {
        3
    } else {
        0
    }
}

/// Returns `true` if `ArticleWriter::ProcessArticles` should commit a staged duplicate fallback article.
pub fn aws_article_writer_is_staged_dupe_commit_needed(
    direct_write: bool,
    dupe_fallback_round: i32,
    has_result_filename: bool,
    skip_disk_write: bool,
) -> bool {
    direct_write && dupe_fallback_round > 0 && has_result_filename && !skip_disk_write
}

/// Returns `true` if `ArticleWriter::ProcessArticles` should append a standard non-direct-write part file.
pub fn aws_article_writer_is_standard_part_append_needed(
    raw_article: bool,
    direct_write: bool,
    skip_disk_write: bool,
) -> bool {
    !raw_article && !direct_write && !skip_disk_write
}

/// Returns `true` if a hard-linked temporary output file should be deleted when `InterDir` is empty.
pub fn aws_article_writer_should_cleanup_hardlinked_temp(
    final_path_empty: bool,
    is_hard_linked: bool,
    inter_dir_empty: bool,
) -> bool {
    final_path_empty && is_hard_linked && inter_dir_empty
}

/// Returns `true` if a `FileInfo` still has pending direct-write output in `MoveCompletedFiles`.
pub fn aws_article_writer_has_pending_file_writes(
    output_initialized: bool,
    output_filename_empty: bool,
    active_downloads: i32,
    guarded_output_initialized: bool,
) -> bool {
    if !output_initialized || output_filename_empty {
        return false;
    }
    if active_downloads > 0 {
        guarded_output_initialized
    } else {
        true
    }
}

/// Classifies the action for `MemoryMonitor::ApplyTierTransition`:
/// `1` = CRITICAL transition, `2` = CONSTRAINED transition, `3` = return to NORMAL, `0` = no-op.
pub fn aws_memory_monitor_classify_tier_transition_action(
    old_tier: i32,
    new_tier: i32,
    tier_normal: i32,
    tier_constrained: i32,
    tier_critical: i32,
) -> i32 {
    if new_tier == tier_critical {
        1
    } else if new_tier == tier_constrained {
        2
    } else if new_tier == tier_normal && old_tier != tier_normal {
        3
    } else {
        0
    }
}

/// Accumulates day slot and total/custom volume stats in `ServerVolume::AddStats`.
/// Returns `(update_day, new_day_bytes, new_day_failed, new_day_success, new_total_bytes, new_custom_bytes)`.
pub fn aws_statmeter_accum_day_and_total_volume(
    day_slot: i32,
    cur_day_bytes: i64,
    cur_day_failed: i32,
    cur_day_success: i32,
    cur_total_bytes: i64,
    cur_custom_bytes: i64,
    add_bytes: i64,
    add_failed: i32,
    add_success: i32,
) -> (bool, i64, i32, i32, i64, i64) {
    let new_total = cur_total_bytes.wrapping_add(add_bytes);
    let new_custom = cur_custom_bytes.wrapping_add(add_bytes);
    if day_slot >= 0 {
        (
            true,
            cur_day_bytes.wrapping_add(add_bytes),
            cur_day_failed.wrapping_add(add_failed),
            cur_day_success.wrapping_add(add_success),
            new_total,
            new_custom,
        )
    } else {
        (
            false,
            cur_day_bytes,
            cur_day_failed,
            cur_day_success,
            new_total,
            new_custom,
        )
    }
}

/// Computes the initial `m_speedStartTime` slot in `StatMeter::ResetSpeedStat`.
pub fn aws_statmeter_calc_initial_speed_start_slot(cur_time: i64, slot_size: i32) -> i32 {
    if slot_size <= 0 {
        0
    } else {
        (cur_time as i32) / slot_size
    }
}

/// Returns `true` if a pooled connection is eligible as a candidate in `ServerPool::LockedGetConnection`.
pub fn aws_server_pool_is_candidate_connection_eligible(
    in_use: bool,
    server_active: bool,
    server_norm_level: i32,
    level: i32,
    match_want_server: bool,
    is_connected: bool,
    server_blocked: bool,
) -> bool {
    !in_use
        && server_active
        && server_norm_level == level
        && match_want_server
        && (is_connected || !server_blocked)
}

/// Returns `true` if `XmlReader::Parse` succeeded without custom entity declarations.
pub fn aws_xml_reader_is_parse_success(sax_result: i32, entity_declared: bool) -> bool {
    sax_result == 0 && !entity_declared
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_article_dl_and_writer_decisions() {
        assert!(aws_article_dl_should_wait_for_connection(false, false, 3, 3));
        assert!(!aws_article_dl_should_wait_for_connection(true, false, 3, 3));
        assert!(!aws_article_dl_should_wait_for_connection(false, true, 3, 3));
        assert!(!aws_article_dl_should_wait_for_connection(false, false, 3, 4));

        assert!(aws_article_dl_eval_auth_rejected(false, true, true));
        assert!(!aws_article_dl_eval_auth_rejected(true, true, true));
        assert!(!aws_article_dl_eval_auth_rejected(false, false, true));

        assert!(aws_article_dl_should_block_failed_server(false, true, false));
        assert!(!aws_article_dl_should_block_failed_server(false, true, true));

        assert!(aws_article_dl_keep_conn_on_no_retry(1, 1, 2));
        assert!(aws_article_dl_keep_conn_on_no_retry(2, 1, 2));
        assert!(!aws_article_dl_keep_conn_on_no_retry(3, 1, 2));

        assert_eq!(aws_article_dl_eval_level_exhaustion(false, 0, 2), (false, 0, false));
        assert_eq!(aws_article_dl_eval_level_exhaustion(true, 0, 2), (true, 1, false));
        assert_eq!(aws_article_dl_eval_level_exhaustion(true, 2, 2), (false, 2, true));

        assert!(aws_article_writer_should_init_direct_write_file(true, false, false, false, true));
        assert!(aws_article_writer_should_init_direct_write_file(true, false, true, false, false));
        assert!(!aws_article_writer_should_init_direct_write_file(true, false, true, false, true));
        assert!(!aws_article_writer_should_init_direct_write_file(true, true, false, false, false));

        assert!(aws_article_writer_should_wait_for_cache_flush(false, true, false));
        assert!(!aws_article_writer_should_wait_for_cache_flush(false, true, true));
        assert!(!aws_article_writer_should_wait_for_cache_flush(true, true, false));

        assert!(aws_article_writer_should_memcpy_cached_segment(false, true));
        assert!(!aws_article_writer_should_memcpy_cached_segment(true, true));

        assert!(aws_article_writer_should_move_temp_to_result(false, false, false));
        assert!(!aws_article_writer_should_move_temp_to_result(true, false, false));

        assert!(aws_article_writer_should_realloc_cached_segment(100, 90));
        assert!(!aws_article_writer_should_realloc_cached_segment(100, 100));
    }

    #[test]
    fn test_writer_scenarios_memory_statmeter_pool_xml() {
        assert_eq!(aws_article_writer_classify_setup_output_scenario(false, false), 1);
        assert_eq!(aws_article_writer_classify_setup_output_scenario(false, true), 2);
        assert_eq!(aws_article_writer_classify_setup_output_scenario(true, true), 2);
        assert_eq!(aws_article_writer_classify_setup_output_scenario(true, false), 3);

        assert!(aws_article_writer_is_staged_dupe_commit_needed(true, 1, true, false));
        assert!(!aws_article_writer_is_staged_dupe_commit_needed(true, 0, true, false));

        assert!(aws_article_writer_is_standard_part_append_needed(false, false, false));
        assert!(!aws_article_writer_is_standard_part_append_needed(false, true, false));

        assert!(aws_article_writer_should_cleanup_hardlinked_temp(true, true, true));
        assert!(!aws_article_writer_should_cleanup_hardlinked_temp(false, true, true));

        assert!(aws_article_writer_has_pending_file_writes(true, false, 0, false));
        assert!(aws_article_writer_has_pending_file_writes(true, false, 2, true));
        assert!(!aws_article_writer_has_pending_file_writes(true, false, 2, false));
        assert!(!aws_article_writer_has_pending_file_writes(false, false, 0, true));

        assert_eq!(aws_memory_monitor_classify_tier_transition_action(0, 2, 0, 1, 2), 1);
        assert_eq!(aws_memory_monitor_classify_tier_transition_action(0, 1, 0, 1, 2), 2);
        assert_eq!(aws_memory_monitor_classify_tier_transition_action(1, 0, 0, 1, 2), 3);
        assert_eq!(aws_memory_monitor_classify_tier_transition_action(0, 0, 0, 1, 2), 0);

        assert_eq!(
            aws_statmeter_accum_day_and_total_volume(2, 100, 1, 5, 1000, 500, 50, 1, 2),
            (true, 150, 2, 7, 1050, 550)
        );
        assert_eq!(
            aws_statmeter_accum_day_and_total_volume(-1, 100, 1, 5, 1000, 500, 50, 1, 2),
            (false, 100, 1, 5, 1050, 550)
        );

        assert_eq!(aws_statmeter_calc_initial_speed_start_slot(1000, 1), 1000);
        assert_eq!(aws_statmeter_calc_initial_speed_start_slot(1000, 0), 0);

        assert!(aws_server_pool_is_candidate_connection_eligible(
            false, true, 0, 0, true, false, false
        ));
        assert!(aws_server_pool_is_candidate_connection_eligible(
            false, true, 0, 0, true, true, true
        ));
        assert!(!aws_server_pool_is_candidate_connection_eligible(
            false, true, 0, 0, true, false, true
        ));

        assert!(aws_xml_reader_is_parse_success(0, false));
        assert!(!aws_xml_reader_is_parse_success(0, true));
        assert!(!aws_xml_reader_is_parse_success(1, false));
    }
}
