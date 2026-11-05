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
    fn test_sap_article_writer_and_binrpc() {
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

        assert_eq!(sap_binrpc_log_slice_bounds(5, 0, 20, 100), (15, 5));
        assert_eq!(sap_binrpc_log_slice_bounds(-1, 0, 20, 100), (20, 0));
        assert_eq!(sap_binrpc_log_slice_bounds(0, 110, 20, 100), (10, 10));
        assert_eq!(sap_binrpc_log_slice_bounds(0, 90, 20, 100), (0, 20));

        assert!(sap_binrpc_edit_queue_use_offset_str(1, 1, 10));
        assert!(sap_binrpc_edit_queue_use_offset_str(10, 1, 10));
        assert!(!sap_binrpc_edit_queue_use_offset_str(2, 1, 10));
    }
}
