//! Posting (daemon/dupesearch/Posting.cpp): which search results are one
//! posting, the order to fetch postings in, and sketches of postings' articles
//! (including `sketch_of_file` and its mtime+size cache).

/// Listings of one size posted this close together are one posting on several indexers.
pub const RELIST_WINDOW_SEC: i64 = 120;
/// Share of the articles two nzb-files may have in common before they count as one posting.
pub const SAME_POSTING_SHARE: f64 = 0.01;
/// Message-id hashes kept in a posting's sketch.
pub const SKETCH_SIZE: usize = 64;
/// An nzb-file this share off the listed size is another nzb-file than the listing's.
pub const LISTING_MISMATCH_SHARE: f64 = 0.02;

/// zlib's crc32 (the reflected IEEE polynomial), as `crc32(0, data, len)`.
pub fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                k += 1;
            }
            table[i] = c;
            i += 1;
        }
        table
    };
    let mut crc = !0u32;
    for &b in data {
        crc = TABLE[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

/// MakeSketch: the SKETCH_SIZE smallest distinct CRC32s of the message-ids, ascending.
pub fn make_sketch<'a>(message_ids: impl IntoIterator<Item = &'a [u8]>) -> Vec<u32> {
    let mut hashes: Vec<u32> = message_ids.into_iter().map(crc32).collect();
    hashes.sort_unstable();
    hashes.dedup();
    hashes.truncate(SKETCH_SIZE);
    hashes
}

/// Elements both sorted sequences hold (std::set_intersection's count, duplicates matched pairwise).
fn common<T: Ord>(a: &[T], b: &[T]) -> usize {
    let (mut i, mut j, mut n) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                n += 1;
                i += 1;
                j += 1;
            }
        }
    }
    n
}

/// SameSketch: at least an eighth (at least one) of the smaller sketch in common.
pub fn same_sketch(a: &[u32], b: &[u32]) -> bool {
    let k = a.len().min(b.len());
    k != 0 && common(a, b) >= 1.max(k / 8)
}

/// SamePosting: more than SAME_POSTING_SHARE of the articles in common (both sorted).
pub fn same_posting(a: &[&[u8]], b: &[&[u8]]) -> bool {
    common(a, b) as f64 > SAME_POSTING_SHARE * a.len().min(b.len()) as f64
}

/// The fields of a search result the grouping and ordering look at.
#[derive(Clone, Copy, Debug)]
pub struct Listing<'a> {
    pub link: &'a [u8],
    pub size: i64,
    pub grabs: i32,
    pub date: i64,
}

/// GroupListings: indices into `results`, one group per posting (same size,
/// posted within RELIST_WINDOW_SEC of the group's first; without a size or a
/// date a listing stays alone), groups in (size, date) order.
pub fn group_listings(results: &[Listing]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..results.len()).collect();
    order.sort_by(|&a, &b| (results[a].size, results[a].date).cmp(&(results[b].size, results[b].date)));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in order {
        let r = &results[i];
        if let Some(group) = groups.last_mut() {
            let first = &results[group[0]];
            if r.size != 0 && r.date != 0 && first.date != 0 && r.size == first.size
                && r.date.wrapping_sub(first.date) <= RELIST_WINDOW_SEC
            {
                group.push(i);
                continue;
            }
        }
        groups.push(vec![i]);
    }
    groups
}

/// ListingMismatch: the nzb-file is more than LISTING_MISMATCH_SHARE off the listed size.
pub fn listing_mismatch(listing_size: i64, nzb_bytes: i64) -> bool {
    listing_size > 0 && (nzb_bytes.wrapping_sub(listing_size)).wrapping_abs() as f64 > LISTING_MISMATCH_SHARE * listing_size as f64
}

/// OrderPostings: groups of indices into `candidates`, each group's listings by
/// rank (closest to `pick_bytes`, most grabbed, newest), groups by their first
/// listing's rank, one posting of each distinct size before the rest, at most
/// 3 * `max_postings` groups when that is above 0.
pub fn order_postings(candidates: &[Listing], pick_bytes: i64, max_postings: i32) -> Vec<Vec<usize>> {
    let mut by_rank: Vec<usize> = (0..candidates.len()).collect();
    let dist = |i: usize| candidates[i].size.wrapping_sub(pick_bytes).wrapping_abs();
    by_rank.sort_by(|&a, &b| {
        dist(a)
            .cmp(&dist(b))
            .then_with(|| candidates[b].grabs.cmp(&candidates[a].grabs))
            .then_with(|| candidates[b].date.cmp(&candidates[a].date))
    });
    let mut rank = vec![0usize; candidates.len()];
    for (r, &i) in by_rank.iter().enumerate() {
        rank[i] = r;
    }
    // the C++ looked a listing up by (link, size, date): the first candidate that matches gives its rank
    let rank_of = |i: usize| {
        let c = &candidates[i];
        let first = candidates
            .iter()
            .position(|o| o.link == c.link && o.size == c.size && o.date == c.date)
            .unwrap_or(i);
        rank[first]
    };
    let mut groups = group_listings(candidates);
    for group in &mut groups {
        group.sort_by_key(|&i| rank_of(i));
    }
    groups.sort_by_key(|g| rank_of(g[0]));
    let mut seen = std::collections::BTreeSet::new();
    let (mut firsts, mut rest) = (Vec::new(), Vec::new());
    for group in groups {
        if seen.insert(candidates[group[0]].size) {
            firsts.push(group);
        } else {
            rest.push(group);
        }
    }
    firsts.extend(rest);
    if max_postings > 0 && firsts.len() > 3 * max_postings as usize {
        firsts.truncate(3 * max_postings as usize);
    }
    firsts
}

type SketchCacheMap = std::collections::HashMap<
    std::path::PathBuf,
    ((std::time::SystemTime, u64), Vec<u32>),
>;

static SKETCH_CACHE: std::sync::Mutex<Option<SketchCacheMap>> = std::sync::Mutex::new(None);

/// `Posting::SketchOfFile`: reads an NZB file, parses its summary, and returns
/// its 64-hash article sketch, caching by `(path, mtime, file_size)`.
pub fn sketch_of_file(path: &std::path::Path) -> Option<Vec<u32>> {
    if path.as_os_str().is_empty() {
        return None;
    }
    let stamp = std::fs::metadata(path).ok().and_then(|m| {
        if m.is_file() {
            Some((m.modified().ok()?, m.len()))
        } else {
            None
        }
    });
    if let Some(st) = stamp {
        if let Ok(mut guard) = SKETCH_CACHE.lock() {
            let map = guard.get_or_insert_with(std::collections::HashMap::new);
            if let Some((cached_st, cached_sketch)) = map.get(path) {
                if *cached_st == st {
                    return Some(cached_sketch.clone());
                }
            }
        }
    }
    let bytes = std::fs::read(path).ok()?;
    let summary = crate::dupesearch_nzb::parse_nzb_summary(&bytes)?;
    let sketch = make_sketch(summary.message_ids.iter().map(|v| v.as_slice()));
    if let Some(st) = stamp {
        if let Ok(mut guard) = SKETCH_CACHE.lock() {
            let map = guard.get_or_insert_with(std::collections::HashMap::new);
            if map.len() > 4096 {
                map.clear();
            }
            map.insert(path.to_path_buf(), (st, sketch.clone()));
        }
    }
    Some(sketch)
}

#[cfg(unix)]
unsafe fn c_path_to_path(path: *const std::ffi::c_char) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    if path.is_null() {
        return None;
    }
    let bytes = std::ffi::CStr::from_ptr(path).to_bytes();
    if bytes.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
unsafe fn c_path_to_path(path: *const std::ffi::c_char) -> Option<std::path::PathBuf> {
    if path.is_null() {
        return None;
    }
    let s = std::ffi::CStr::from_ptr(path).to_string_lossy();
    if s.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(s.as_ref()))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_posting_sketch_of_file(
    path: *const std::ffi::c_char,
    out: *mut u32,
    cap: usize,
    out_len: *mut usize,
) -> std::ffi::c_int {
    let Some(path_buf) = c_path_to_path(path) else {
        return 0;
    };
    let Some(sketch) = sketch_of_file(&path_buf) else {
        return 0;
    };
    let n = sketch.len().min(cap);
    if !out.is_null() && n > 0 {
        std::ptr::copy_nonoverlapping(sketch.as_ptr(), out, n);
    }
    if !out_len.is_null() {
        *out_len = n;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(link: &'static [u8], size: i64, grabs: i32, date: i64) -> Listing<'static> {
        Listing { link, size, grabs, date }
    }

    #[test]
    fn crc32_is_zlibs() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn sketches_keep_the_smallest_distinct_hashes() {
        let ids: Vec<Vec<u8>> = (0..200).map(|i| format!("<{i}@x>").into_bytes()).collect();
        let s = make_sketch(ids.iter().map(|v| v.as_slice()));
        assert_eq!(s.len(), SKETCH_SIZE);
        assert!(s.windows(2).all(|w| w[0] < w[1]));
        assert!(same_sketch(&s, &s) && !same_sketch(&s, &[]));
    }

    #[test]
    fn groups_relistings_and_orders_by_rank() {
        let c = [l(b"a", 100, 0, 1000), l(b"b", 100, 5, 1060), l(b"c", 300, 0, 1000), l(b"d", 100, 0, 5000)];
        assert_eq!(group_listings(&c), vec![vec![0, 1], vec![3], vec![2]]);
        assert_eq!(order_postings(&c, 100, 0), vec![vec![1, 0], vec![2], vec![3]]);
        assert_eq!(order_postings(&c, 100, 1).len(), 3);
        assert!(listing_mismatch(100, 103) && !listing_mismatch(100, 102) && !listing_mismatch(0, 5));
    }

    #[test]
    fn posting_full_suite_port_from_cpp() {
        let make_ids = |prefix: &str, count: usize, from: usize| -> Vec<Vec<u8>> {
            let mut ids: Vec<Vec<u8>> = (from..from + count)
                .map(|i| format!("{prefix}{i}@x").into_bytes())
                .collect();
            ids.sort();
            ids
        };

        // PostingSketchTest
        let a = make_ids("a", 6000, 0);
        let mut reupload = make_ids("a", 5996, 2);
        reupload.push(b"z1@x".to_vec());
        reupload.push(b"z2@x".to_vec());
        reupload.sort();
        let other = make_ids("b", 6000, 0);

        let sk_a = make_sketch(a.iter().map(|v| v.as_slice()));
        let sk_re = make_sketch(reupload.iter().map(|v| v.as_slice()));
        let sk_other = make_sketch(other.iter().map(|v| v.as_slice()));
        assert_eq!(sk_a.len(), 64);
        assert!(same_sketch(&sk_a, &sk_re));
        assert!(!same_sketch(&sk_a, &sk_other));

        let tiny = [b"t1@x".as_slice(), b"t2@x".as_slice()];
        let sk_tiny = make_sketch(tiny);
        assert!(same_sketch(&sk_tiny, &sk_tiny));
        assert!(!same_sketch(&sk_tiny, &[]));
        assert!(!same_sketch(&[], &[]));

        // PostingSamePostingTest
        let a1000 = make_ids("a", 1000, 0);
        let a980 = make_ids("a", 980, 20);
        let mut partial = make_ids("a", 5, 0);
        partial.extend(make_ids("c", 995, 0));
        partial.sort();
        let b1000 = make_ids("b", 1000, 0);

        fn refs(v: &[Vec<u8>]) -> Vec<&[u8]> {
            v.iter().map(|s| s.as_slice()).collect()
        }
        assert!(same_posting(&refs(&a1000), &refs(&a1000)));
        assert!(same_posting(&refs(&a1000), &refs(&a980)));
        assert!(!same_posting(&refs(&a1000), &refs(&partial)));
        assert!(!same_posting(&refs(&a1000), &refs(&b1000)));

        // PostingSketchOfFileTest
        let path = std::env::temp_dir().join(format!(
            "nzbget-sketch-of-file-rust-{}.nzb",
            std::process::id()
        ));
        let write_nzb = |prefix: &str| {
            let mut xml = String::from(
                "<?xml version=\"1.0\"?><nzb><file poster=\"p\" subject=\"&quot;a.mkv&quot;\"><segments>",
            );
            for i in 0..30 {
                xml.push_str(&format!(
                    "<segment bytes=\"10\" number=\"{}\">{prefix}{i}@x</segment>",
                    i + 1
                ));
            }
            xml.push_str("</segments></file></nzb>");
            std::fs::write(&path, xml).unwrap();
        };
        write_nzb("a");
        let first = sketch_of_file(&path).unwrap();
        assert_eq!(first.len(), 30);
        let a30 = make_ids("a", 30, 0);
        assert!(same_sketch(
            &first,
            &make_sketch(a30.iter().map(|v| v.as_slice()))
        ));

        write_nzb("bb");
        let second = sketch_of_file(&path).unwrap();
        assert!(!same_sketch(&first, &second));

        let _ = std::fs::remove_file(&path);
        assert!(sketch_of_file(&path).is_none());
        std::fs::write(&path, "<html>not an nzb").unwrap();
        assert!(sketch_of_file(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }
}
