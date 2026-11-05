// Hot-path Rust implementations for `ArticleWriter`, `ArticleCache`, `MemoryMonitor`,
// `Extension`, `SectionValidator`, `SectionGroupValidator`, `SchedulerTasksValidator`,
// `FeedsValidator`, `CategoriesValidator`, `Unpack`, `Thread`, `Xml`, `Json`,
// `Network`, `Observer`, `WorkState`, `NewsServer`, `OpenSSL`, and `Utf8`.

use std::ffi::CStr;

// ============================================================================
// ArticleWriter, ArticleCache & MemoryMonitor (`daemon/nntp/ArticleWriter.cpp`)
// ============================================================================

/// Formats `<tmp_dir><sep><file_id>.<part_num:03>` as in `ArticleWriter::BuildOutputFilename`.
pub fn article_writer_format_result_filename(
    tmp_dir: &[u8],
    sep: u8,
    file_id: i32,
    part_num: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(tmp_dir.len() + 20);
    out.extend_from_slice(tmp_dir);
    out.push(sep);
    out.extend_from_slice(format!("{file_id}.{part_num:03}").as_bytes());
    out
}

/// Formats `<dest_dir><sep><file_id>.out.tmp` as in `ArticleWriter::BuildOutputFilename`.
pub fn article_writer_format_direct_out_tmp(dest_dir: &[u8], sep: u8, file_id: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(dest_dir.len() + 24);
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(format!("{file_id}.out.tmp").as_bytes());
    out
}

/// Matches `!strncmp(nzbDestDir.data(), m_outputFilename.c_str(), len) && (m_outputFilename[len] == PATH_SEPARATOR || m_outputFilename[len] == ALT_PATH_SEPARATOR)`
/// in `ArticleWriter::CleanupOldData`.
pub fn article_writer_is_within_dest_dir(
    nzb_dest_dir: &[u8],
    output_filename: &[u8],
    sep1: u8,
    sep2: u8,
) -> bool {
    let len = nzb_dest_dir.len();
    if output_filename.len() <= len {
        return false;
    }
    output_filename.starts_with(nzb_dest_dir)
        && (output_filename[len] == sep1 || output_filename[len] == sep2)
}

/// Classifies completion message kind in `ArticleWriter::ReportCompletionStatus`:
/// - `1`: `total_articles == success_articles` ("Successfully downloaded ...")
/// - `2`: `missed_articles + failed_articles > 0` ("%i of %i article downloads failed ...")
/// - `0`: partial ("Partially downloaded ...")
pub fn article_writer_classify_completion_status(
    total_articles: i32,
    success_articles: i32,
    missed_articles: i32,
    failed_articles: i32,
) -> i32 {
    if total_articles == success_articles {
        1
    } else if missed_articles.saturating_add(failed_articles) > 0 {
        2
    } else {
        0
    }
}

/// Matches `fileInfo->GetCachedArticles() > 0 && (fileInfo->GetActiveDownloads() == 0 || flushEverything) && (!fileInfo->GetNzbInfo()->GetSkipDiskWrite() || flushEverything)`
/// in `ArticleCache::CheckFlush`.
pub fn article_cache_can_flush_file(
    cached_articles: i32,
    active_downloads: i32,
    skip_disk_write: bool,
    flush_everything: bool,
) -> bool {
    cached_articles > 0
        && (active_downloads == 0 || flush_everything)
        && (!skip_disk_write || flush_everything)
}

/// Matches `MemoryMonitor::GetTierName`.
pub fn memory_monitor_tier_name(tier: i32, constrained: i32, critical: i32) -> &'static CStr {
    if tier == critical {
        c"CRITICAL"
    } else if tier == constrained {
        c"CONSTRAINED"
    } else {
        c"NORMAL"
    }
}

/// Matches `MemoryMonitor::GetEffectiveDownloadsLimit`.
pub fn memory_monitor_effective_downloads_limit(
    tier: i32,
    configured_limit: i32,
    constrained: i32,
    critical: i32,
) -> i32 {
    if tier == critical {
        1.max(configured_limit / 4)
    } else if tier == constrained {
        2.max(configured_limit / 2)
    } else {
        configured_limit
    }
}

// ============================================================================
// Extension & Xml (`daemon/extension/Extension.cpp`, `daemon/util/Xml.cpp`)
// ============================================================================

/// Matches `Xml::BoolToStr(bool value)` (`"1"` or `"0"`).
pub fn xml_bool_to_str(value: bool) -> &'static CStr {
    if value {
        c"1"
    } else {
        c"0"
    }
}

/// Selects one of the 5 script kind flags (`0=post, 1=scan, 2=queue, 3=scheduler, 4=feed`).
pub fn extension_select_kind_flag(
    post: bool,
    scan: bool,
    queue: bool,
    scheduler: bool,
    feed: bool,
    which: i32,
) -> bool {
    match which {
        0 => post,
        1 => scan,
        2 => queue,
        3 => scheduler,
        4 => feed,
        _ => false,
    }
}

// ============================================================================
// SystemHealth Validators (`SectionValidator`, `SectionGroupValidator`,
// `SchedulerTasksValidator`, `FeedsValidator`, `CategoriesValidator`)
// ============================================================================

// ============================================================================
// Unpack (`daemon/util/Unpack.cpp`)
// ============================================================================

/// Matches `ExtractorBase::MakePassword`: `"-p-"` when empty, else `"-p<password>"`.
pub fn unpack_make_password_arg(password: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        b"-p-".to_vec()
    } else {
        let mut out = Vec::with_capacity(2 + password.len());
        out.extend_from_slice(b"-p");
        out.extend_from_slice(password);
        out
    }
}

/// Classifies tool path check in `Unpack::MakeExtractor::validateTool`:
/// - `1`: `tool_empty` (not configured)
/// - `2`: `has_err` (failed to access)
/// - `3`: `!exists` (doesn't exist)
/// - `0`: valid
pub fn unpack_validate_tool_status(tool_empty: bool, has_err: bool, exists: bool) -> i32 {
    if tool_empty {
        1
    } else if has_err {
        2
    } else if !exists {
        3
    } else {
        0
    }
}

// ============================================================================
// Thread, Json, Network, Observer, WorkState, NewsServer, OpenSSL, Utf8
// ============================================================================

/// Matches `if (terminated) { --m_threadCount; }` in `Thread::Kill`.
pub fn thread_adjust_count_on_kill(terminated: bool, cur_count: i32) -> i32 {
    if terminated {
        cur_count - 1
    } else {
        cur_count
    }
}

/// Matches `if (ec) return std::nullopt;` in `Json::Deserialize`.
pub fn json_has_error(err_val: i32) -> bool {
    err_val != 0
}

/// Matches `result.statusCode == 200` in `System::GetNetwork`.
pub fn network_is_http_status_ok(status_code: i32) -> bool {
    status_code == 200
}

/// Finds the index of `target` in `observers` for `Subject::Detach`.
pub fn observer_find_index(observers: &[usize], target: usize) -> isize {
    for (idx, &obs) in observers.iter().enumerate() {
        if obs == target {
            return idx as isize;
        }
    }
    -1
}

/// Returns `true` when `WorkState::Changed` should notify observers with `nullptr`.
pub fn work_state_should_notify() -> bool {
    true
}

/// Formats default `NewsServer` name (`server<id>`) when `name` is empty.
pub fn news_server_format_default_name(name: &[u8], id: i32) -> Vec<u8> {
    if !name.is_empty() {
        name.to_vec()
    } else {
        format!("server{id}").into_bytes()
    }
}

/// Returns `true` when `OpenSSL::StopSSLThread` should invoke `OPENSSL_thread_stop()`.
pub fn openssl_should_stop_thread(is_libressl: bool) -> bool {
    !is_libressl
}

/// Returns `true` when a UTF-8 / wide input string view is empty in `Utf8::Utf8ToWide` / `WideToUtf8`.
pub fn utf8_is_empty_len(len: usize) -> bool {
    len == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_article_writer_and_cache_helpers() {
        assert_eq!(
            article_writer_format_result_filename(b"/tmp", b'/', 42, 7),
            b"/tmp/42.007"
        );
        assert_eq!(
            article_writer_format_direct_out_tmp(b"/dest", b'/', 42),
            b"/dest/42.out.tmp"
        );
        assert!(article_writer_is_within_dest_dir(
            b"/dest/job",
            b"/dest/job/file.rar",
            b'/',
            b'\\'
        ));
        assert!(!article_writer_is_within_dest_dir(
            b"/dest/job",
            b"/dest/job2/file.rar",
            b'/',
            b'\\'
        ));
        assert_eq!(article_writer_classify_completion_status(10, 10, 0, 0), 1);
        assert_eq!(article_writer_classify_completion_status(10, 8, 2, 0), 2);
        assert_eq!(article_writer_classify_completion_status(10, 8, 0, 0), 0);
        assert!(article_cache_can_flush_file(3, 0, false, false));
        assert!(!article_cache_can_flush_file(3, 1, false, false));
        assert!(article_cache_can_flush_file(3, 1, true, true));
        assert_eq!(
            memory_monitor_tier_name(2, 1, 2).to_bytes(),
            b"CRITICAL"
        );
        assert_eq!(memory_monitor_effective_downloads_limit(2, 20, 1, 2), 5);
        assert_eq!(memory_monitor_effective_downloads_limit(1, 20, 1, 2), 10);
        assert_eq!(memory_monitor_effective_downloads_limit(0, 20, 1, 2), 20);
    }

    #[test]
    fn test_validators_and_utility_helpers() {
        assert_eq!(xml_bool_to_str(true).to_bytes(), b"1");
        assert_eq!(xml_bool_to_str(false).to_bytes(), b"0");
        assert!(extension_select_kind_flag(true, false, false, false, false, 0));
        assert!(!extension_select_kind_flag(true, false, false, false, false, 1));
        assert_eq!(unpack_make_password_arg(b""), b"-p-");
        assert_eq!(unpack_make_password_arg(b"secret"), b"-psecret");
        assert_eq!(unpack_validate_tool_status(false, false, true), 0);
        assert_eq!(unpack_validate_tool_status(true, false, false), 1);
        assert_eq!(thread_adjust_count_on_kill(true, 4), 3);
        assert!(json_has_error(1));
        assert!(network_is_http_status_ok(200));
        assert_eq!(observer_find_index(&[10, 20, 30], 20), 1);
        assert_eq!(observer_find_index(&[10, 20, 30], 99), -1);
        assert!(work_state_should_notify());
        assert_eq!(news_server_format_default_name(b"", 3), b"server3");
        assert!(openssl_should_stop_thread(false));
        assert!(utf8_is_empty_len(0));
    }
}
