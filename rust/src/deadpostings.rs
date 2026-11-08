//! End-to-end Rust implementation of `DeadPostings` (`daemon/dupesearch/DeadPostings.cpp`):
//! persisted dead-posting line parsing, TTL/clock-skew pruning, sketch matching,
//! and atomic state-file persistence (`DeadPostingsStore`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// strto* uses C-string termination, accepts leading space and a sign, and the
// caller ignores errno. Keep those rules without creating temporary C strings.
fn decimal(field: &[u8]) -> Option<(bool, u64, bool)> {
    if field.is_empty() { return None; }
    let text = field.split(|&b| b == 0).next()?;
    if text.is_empty() { return Some((false, 0, false)); }
    let mut i = 0;
    while let Some(&byte) = text.get(i) {
        if !matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) { break; }
        i += 1;
    }
    let negative = text.get(i) == Some(&b'-');
    if matches!(text.get(i), Some(b'+' | b'-')) { i += 1; }
    let start = i;
    let mut value = 0u64;
    let mut overflow = false;
    while let Some(&digit) = text.get(i) {
        if !digit.is_ascii_digit() { return None; }
        match value.checked_mul(10).and_then(|n| n.checked_add(u64::from(digit - b'0'))) {
            Some(n) => value = n,
            None => { value = u64::MAX; overflow = true; }
        }
        i += 1;
    }
    (i > start).then_some((negative, value, overflow))
}

/// Matches the two tab-delimited fields read by the C++ loader. Hashes are
/// converted with the target's unsigned-long width, then sorted and deduped.
/// `strtoll` overflow saturates, since the persisted C++ reader ignores errno.
pub fn parse_line(line: &[u8], ulong_bits: u32) -> Option<(i64, Vec<u32>)> {
    let tab = line.iter().position(|&b| b == b'\t')?;
    let (negative, magnitude, overflow) = decimal(&line[..tab])?;
    let limit = if negative { 1u64 << 63 } else { i64::MAX as u64 };
    let magnitude = if overflow { limit } else { magnitude.min(limit) };
    let stamp = if negative {
        if magnitude == 1u64 << 63 { i64::MIN } else { -(magnitude as i64) }
    } else { magnitude as i64 };
    let hashes = &line[tab + 1..];
    let hashes = hashes.split(|&b| b == b'\t').next()?;
    if hashes.is_empty() { return None; }
    let hashes = hashes.strip_suffix(b",").unwrap_or(hashes);
    let mut sketch = Vec::with_capacity(64);
    for field in hashes.split(|&b| b == b',') {
        let (negative, magnitude, overflow) = decimal(field)?;
        let max = if ulong_bits == 32 { u32::MAX as u64 } else { u64::MAX };
        let value = if overflow || magnitude > max { max }
            else if negative { magnitude.wrapping_neg() & max } else { magnitude };
        sketch.push(u32::try_from(value).ok()?);
    }
    sketch.sort_unstable();
    sketch.dedup();
    Some((stamp, sketch))
}

/// Default dead-posting TTL in seconds (`DeadPostings::DeadTtlSec = 3 * 24 * 3600`).
pub const DEAD_TTL_SEC: u64 = 3 * 24 * 3600;
/// Maximum allowed forward clock skew in seconds (`24 * 3600`).
pub const FUTURE_SKEW_SEC: u64 = 24 * 3600;

/// Returns whether a dead-posting timestamp `recorded` is stale at time `now`
/// (`DeadPostings::Prune` in `daemon/dupesearch/DeadPostings.cpp`).
pub fn is_stale_timestamp(now: i64, recorded: i64) -> bool {
    let current = now as u64;
    let rec = recorded as u64;
    if now >= recorded {
        current.wrapping_sub(rec) >= DEAD_TTL_SEC
    } else {
        rec.wrapping_sub(current) > FUTURE_SKEW_SEC
    }
}

/// Returns whether a dead-posting entry (`recorded`, `entry_sketch`) is still active at
/// time `now` and matches `query_sketch` (`DeadPostings::IsDead` in `DeadPostings.cpp`).
pub fn is_active_entry_match(
    now: i64,
    recorded: i64,
    query_sketch: &[u32],
    entry_sketch: &[u32],
) -> bool {
    let active = now < recorded || (now as u64).wrapping_sub(recorded as u64) < DEAD_TTL_SEC;
    active && crate::posting::same_sketch(query_sketch, entry_sketch)
}

/// Formats one persisted dead-posting line (`"{stamp}\t{h0},{h1},...\t\n"`,
/// `DeadPostings::Save` in `DeadPostings.cpp`).
pub fn format_deadpostings_line(stamp: i64, hashes: &[u32]) -> Vec<u8> {
    let mut out = format!("{stamp}\t");
    for (i, &h) in hashes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&h.to_string());
    }
    out.push_str("\t\n");
    out.into_bytes()
}

fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    if bytes.is_empty() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        Some(PathBuf::from(String::from_utf8_lossy(bytes).into_owned()))
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> bool {
    let mut temp_os = path.as_os_str().to_os_string();
    temp_os.push(".new");
    let temp = PathBuf::from(temp_os);
    let write_res = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&temp)?;
        f.write_all(data)?;
        f.flush()?;
        Ok(())
    })();
    if write_res.is_err() {
        let _ = std::fs::remove_file(&temp);
        return false;
    }
    match std::fs::rename(&temp, path) {
        Ok(()) => return true,
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            if std::fs::copy(&temp, path).is_ok() && std::fs::remove_file(&temp).is_ok() {
                return true;
            }
        }
        Err(_) => {}
    }
    let _ = std::fs::remove_file(&temp);
    false
}

#[derive(Default)]
struct DeadPostingsInner {
    dead: Vec<(i64, Vec<u32>)>,
    unsaved: Vec<(i64, Vec<u32>)>,
    state_path: Option<PathBuf>,
}

impl DeadPostingsInner {
    fn prune(&mut self, now: i64) {
        self.dead.retain(|(ts, _)| !is_stale_timestamp(now, *ts));
        self.unsaved.retain(|(ts, _)| !is_stale_timestamp(now, *ts));
    }

    fn save(&self) -> bool {
        let Some(ref path) = self.state_path else {
            return true;
        };
        let mut text = Vec::new();
        for (ts, sketch) in &self.dead {
            text.extend_from_slice(&format_deadpostings_line(*ts, sketch));
        }
        write_atomic(path, &text)
    }
}

/// Thread-safe, atomically persisted store of dead postings (`DeadPostings` in `daemon/dupesearch/DeadPostings.cpp`).
#[derive(Default)]
pub struct DeadPostingsStore {
    inner: Mutex<DeadPostingsInner>,
}

impl DeadPostingsStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_state_path(&self, path: &[u8]) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.state_path = path_from_bytes(path);
        }
    }

    pub fn set_state_path_buf(&self, path: impl Into<PathBuf>) {
        if let Ok(mut guard) = self.inner.lock() {
            let pb = path.into();
            guard.state_path = (!pb.as_os_str().is_empty()).then_some(pb);
        }
    }

    pub fn load(&self, now: i64) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        if let Some(ref path) = guard.state_path {
            if let Ok(bytes) = std::fs::read(path) {
                for line in bytes.split(|&b| b == b'\n') {
                    let line = line.strip_suffix(b"\r").unwrap_or(line);
                    if let Some((stamp, sketch)) = parse_line(line, std::ffi::c_ulong::BITS) {
                        guard.dead.push((stamp, sketch));
                    }
                }
            }
        }
        guard.prune(now);
    }

    pub fn is_dead(&self, sketch: &[u32], now: i64) -> bool {
        let Ok(guard) = self.inner.lock() else {
            return false;
        };
        guard
            .dead
            .iter()
            .chain(guard.unsaved.iter())
            .any(|(ts, entry)| is_active_entry_match(now, *ts, sketch, entry))
    }

    /// Adds a dead posting sketch at `now`. Returns `false` only when `save` is true,
    /// a `state_path` is configured, and the atomic write fails.
    pub fn add(&self, sketch: Vec<u32>, now: i64, save: bool) -> bool {
        let Ok(mut guard) = self.inner.lock() else {
            return false;
        };
        guard.prune(now);
        if save {
            guard.dead.push((now, sketch));
            guard.save()
        } else {
            guard.unsaved.push((now, sketch));
            true
        }
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_deadpostings_store_new() -> *mut DeadPostingsStore {
    Box::into_raw(Box::new(DeadPostingsStore::new()))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_deadpostings_store_free(store: *mut DeadPostingsStore) {
    if !store.is_null() {
        drop(Box::from_raw(store));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_deadpostings_store_set_state_path(
    store: *const DeadPostingsStore,
    path: *const u8,
    len: usize,
) {
    if store.is_null() {
        return;
    }
    let slice = if path.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(path, len)
    };
    (*store).set_state_path(slice);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_deadpostings_store_load(
    store: *const DeadPostingsStore,
    now: i64,
) {
    if !store.is_null() {
        (*store).load(now);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_deadpostings_store_is_dead(
    store: *const DeadPostingsStore,
    sketch_ptr: *const u32,
    sketch_len: usize,
    now: i64,
) -> i32 {
    if store.is_null() {
        return 0;
    }
    let sketch = if sketch_ptr.is_null() || sketch_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(sketch_ptr, sketch_len)
    };
    i32::from((*store).is_dead(sketch, now))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_deadpostings_store_add(
    store: *const DeadPostingsStore,
    sketch_ptr: *const u32,
    sketch_len: usize,
    now: i64,
    save: i32,
) -> i32 {
    if store.is_null() {
        return 0;
    }
    let sketch = if sketch_ptr.is_null() || sketch_len == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(sketch_ptr, sketch_len).to_vec()
    };
    i32::from((*store).add(sketch, now, save != 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_stale_active_and_format_line() {
        let now = 1_000_000i64;
        assert!(!is_stale_timestamp(now, now));
        assert!(!is_stale_timestamp(now + DEAD_TTL_SEC as i64 - 1, now));
        assert!(is_stale_timestamp(now + DEAD_TTL_SEC as i64, now));
        assert!(!is_stale_timestamp(now, now + FUTURE_SKEW_SEC as i64));
        assert!(is_stale_timestamp(now, now + FUTURE_SKEW_SEC as i64 + 1));

        let s1 = [1u32, 2, 3, 4, 5, 6, 7, 8];
        assert!(is_active_entry_match(now, now, &s1, &s1));
        assert!(!is_active_entry_match(now + DEAD_TTL_SEC as i64, now, &s1, &s1));
        assert_eq!(format_deadpostings_line(12345, &[1, 2, 9]), b"12345\t1,2,9\t\n");
    }
    #[test]
    fn sorted_unique_and_trailing_delimiters() {
        assert_eq!(parse_line(b"123\t9,2,9,1,\tignored",64),Some((123,vec![1,2,9])));
        assert_eq!(parse_line(b"123\t7",64),Some((123,vec![7])));
    }
    #[test]
    fn signed_timestamp_and_leading_space() {
        assert_eq!(parse_line(b" \x0b-123\t +7,\t",64),Some((-123,vec![7])));
    }
    #[test]
    fn nul_terminates_numbers_not_fields() {
        assert_eq!(parse_line(b"123\0junk\t5\0junk,2\t",64),Some((123,vec![2,5])));
        assert_eq!(parse_line(b"\0\t\0\t",64),Some((0,vec![0])));
        assert_eq!(parse_line(b" \0\t1\t",64),None);
    }
    #[test]
    fn timestamp_overflow_saturates_like_strtoll() {
        assert_eq!(parse_line(b"99999999999999999999999\t1\t",64),Some((i64::MAX,vec![1])));
        assert_eq!(parse_line(b"-99999999999999999999999\t1\t",64),Some((i64::MIN,vec![1])));
    }
    #[test]
    fn signed_hash_and_ulong_width() {
        assert_eq!(parse_line(b"1\t-18446744073709551615\t",64),Some((1,vec![1])));
        assert_eq!(parse_line(b"1\t-1\t",64),None);
        assert_eq!(parse_line(b"1\t-1\t",32),Some((1,vec![u32::MAX])));
        assert_eq!(parse_line(b"1\t4294967296\t",32),Some((1,vec![u32::MAX])));
        assert_eq!(parse_line(b"1\t4294967296\t",64),None);
    }
    #[test]
    fn invalid_fields_reject_whole_record() {
        for line in [b"".as_slice(),b"1",b"1\t",b"1\t\t",b"\t1\t",b"+\t1\t",b"1 \t1\t",b"1\t1,x,2\t",b"1\t,1\t",b"1\t1,,2\t",b"1\t1 \t",b"1\t18446744073709551616\t"] {
            assert_eq!(parse_line(line,64),None,"{line:?}");
        }
    }

    fn make_ids_sketch(prefix: &str, count: usize, from: usize) -> Vec<u32> {
        let ids: Vec<Vec<u8>> = (from..from + count)
            .map(|i| format!("{prefix}{i}@x").into_bytes())
            .collect();
        crate::posting::make_sketch(ids.iter().map(|v| v.as_slice()))
    }

    #[test]
    fn dead_postings_full_suite_port_from_cpp() {
        // DeadPostingsRememberedTest
        let dead = DeadPostingsStore::new();
        let now = 1_000_000i64;
        let posting = make_ids_sketch("a", 6000, 0);
        assert!(!dead.is_dead(&posting, now));
        assert!(dead.add(posting.clone(), now, true));
        assert!(dead.is_dead(&posting, now));

        let mut relisted_ids: Vec<Vec<u8>> = (2..6000)
            .map(|i| format!("a{i}@x").into_bytes())
            .collect();
        relisted_ids.push(b"z1@x".to_vec());
        relisted_ids.push(b"z2@x".to_vec());
        let relisted = crate::posting::make_sketch(relisted_ids.iter().map(|v| v.as_slice()));
        assert!(dead.is_dead(&relisted, now));
        assert!(!dead.is_dead(&make_ids_sketch("b", 6000, 0), now));

        // DeadPostingsExpireTest
        let exp = DeadPostingsStore::new();
        let p_short = make_ids_sketch("a", 100, 0);
        exp.add(p_short.clone(), now, true);
        assert!(exp.is_dead(&p_short, now + DEAD_TTL_SEC as i64 - 1));
        assert!(!exp.is_dead(&p_short, now + DEAD_TTL_SEC as i64));
        exp.add(
            make_ids_sketch("b", 100, 0),
            now + DEAD_TTL_SEC as i64 + 10,
            true,
        );
        assert!(!exp.is_dead(&p_short, now + 5));

        // DeadPostingsTimeBoundsTest
        let low = i64::MIN;
        let high = i64::MAX;
        for save in [false, true] {
            let expired = DeadPostingsStore::new();
            expired.add(vec![1], low, save);
            assert!(expired.is_dead(&[1], low + DEAD_TTL_SEC as i64 - 1));
            assert!(!expired.is_dead(&[1], low + DEAD_TTL_SEC as i64));
            assert!(!expired.is_dead(&[1], high));
            expired.add(vec![2], high, save);
            assert!(!expired.is_dead(&[1], low + 1));

            let future = DeadPostingsStore::new();
            future.add(vec![3], high, save);
            assert!(future.is_dead(&[3], low));
            future.add(vec![4], low, save);
            assert!(!future.is_dead(&[3], high));

            let exact = DeadPostingsStore::new();
            exact.add(vec![9], now, save);
            exact.add(vec![10], now + DEAD_TTL_SEC as i64, save);
            assert!(!exact.is_dead(&[9], now));

            let boundaries = DeadPostingsStore::new();
            boundaries.add(vec![5], now + 24 * 3600, save);
            boundaries.add(vec![6], now, save);
            assert!(boundaries.is_dead(&[5], now));
            boundaries.add(vec![7], now + 24 * 3600 + 1, save);
            boundaries.add(vec![8], now, save);
            assert!(!boundaries.is_dead(&[7], now + 24 * 3600 + 1));
        }

        // DeadPostingsPersistTest (real atomic file write + reload)
        let pid = std::process::id();
        let path_persist = std::env::temp_dir().join(format!("nzbget-dead-postings-persist-{pid}"));
        let path_persist_new = PathBuf::from(format!("{}.new", path_persist.display()));
        let _ = std::fs::remove_file(&path_persist);
        let _ = std::fs::remove_file(&path_persist_new);

        let p500 = make_ids_sketch("a", 500, 0);
        {
            let store = DeadPostingsStore::new();
            store.set_state_path_buf(&path_persist);
            assert!(store.add(p500.clone(), now, true));
        }
        assert!(path_persist.exists());
        assert!(!path_persist_new.exists());

        let reloaded = DeadPostingsStore::new();
        reloaded.set_state_path_buf(&path_persist);
        reloaded.load(now);
        assert!(reloaded.is_dead(&p500, now));
        assert!(!reloaded.is_dead(&make_ids_sketch("b", 500, 0), now));

        std::fs::write(&path_persist, b"garbage\n\t\nnot\ta\tnumber\n").unwrap();
        let broken = DeadPostingsStore::new();
        broken.set_state_path_buf(&path_persist);
        broken.load(now);
        assert!(!broken.is_dead(&p500, now));
        let _ = std::fs::remove_file(&path_persist);

        // DeadPostingsUnsavedTest (real file check)
        let path_unsaved = std::env::temp_dir().join(format!("nzbget-dead-postings-unsaved-{pid}"));
        let _ = std::fs::remove_file(&path_unsaved);
        let tentative = make_ids_sketch("t", 500, 0);
        let real = make_ids_sketch("r", 500, 0);
        {
            let store = DeadPostingsStore::new();
            store.set_state_path_buf(&path_unsaved);
            assert!(store.add(tentative.clone(), now, false));
            assert!(store.is_dead(&tentative, now));
            assert!(!path_unsaved.exists());
            assert!(store.add(real.clone(), now, true));
            assert!(path_unsaved.exists());
        }
        let reloaded_u = DeadPostingsStore::new();
        reloaded_u.set_state_path_buf(&path_unsaved);
        reloaded_u.load(now);
        assert!(reloaded_u.is_dead(&real, now));
        assert!(!reloaded_u.is_dead(&tentative, now));
        let _ = std::fs::remove_file(&path_unsaved);

        // DeadPostingsRobustLoadTest
        let path_robust = std::env::temp_dir().join(format!("nzbget-dead-postings-robust-{pid}"));
        let posting_u = make_ids_sketch("u", 500, 0);
        let future_f = make_ids_sketch("f", 500, 0);
        {
            let mut content = format!("{now}\t");
            for (i, h) in posting_u.iter().rev().enumerate() {
                if i > 0 {
                    content.push(',');
                }
                content.push_str(&h.to_string());
            }
            content.push_str("\t\n");
            let far_future = now + 10 * 365 * 24 * 3600;
            content.push_str(&format!("{far_future}\t"));
            for (i, h) in future_f.iter().enumerate() {
                if i > 0 {
                    content.push(',');
                }
                content.push_str(&h.to_string());
            }
            content.push_str("\t\n");
            content.push_str(&format!("{now}\t12,x7,99\t\n"));
            std::fs::write(&path_robust, content.as_bytes()).unwrap();
        }
        let loaded_r = DeadPostingsStore::new();
        loaded_r.set_state_path_buf(&path_robust);
        loaded_r.load(now);
        assert!(loaded_r.is_dead(&posting_u, now));
        assert!(!loaded_r.is_dead(&future_f, now));
        assert!(!loaded_r.is_dead(&[0, 12, 99], now));
        let _ = std::fs::remove_file(&path_robust);

        // DeadPostingsStateFieldsTest
        let path_fields = std::env::temp_dir().join(format!("nzbget-dead-postings-fields-{pid}"));
        let stamp = now.to_string();
        let mut raw = Vec::new();
        raw.extend_from_slice(format!("{stamp}\t3,1,3,2,\tignored\n").as_bytes());
        raw.extend_from_slice(stamp.as_bytes());
        raw.extend_from_slice(b"\0tail\t9\0tail,7\t\n");
        raw.extend_from_slice(format!("{stamp}\t12,x7,99\t\n").as_bytes());
        raw.extend_from_slice(format!("{stamp}\t,77\t\n").as_bytes());
        std::fs::write(&path_fields, &raw).unwrap();

        let state_store = DeadPostingsStore::new();
        state_store.set_state_path_buf(&path_fields);
        state_store.load(now);
        assert!(state_store.is_dead(&[1], now));
        assert!(state_store.is_dead(&[7], now));
        assert!(!state_store.is_dead(&[12], now));
        assert!(!state_store.is_dead(&[77], now));
        assert!(state_store.add(vec![42], now, true));
        let saved_on_disk = std::fs::read(&path_fields).unwrap();
        let expected_saved = format!("{stamp}\t1,2,3\t\n{stamp}\t7,9\t\n{stamp}\t42\t\n");
        assert_eq!(saved_on_disk, expected_saved.into_bytes());
        let _ = std::fs::remove_file(&path_fields);
    }
}
