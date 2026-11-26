//! A weak primary cut at its frontier: it stops fetching its own data articles and a held alt
//! of the same video lends the rest (failover-continue), instead of the alt being downloaded
//! and extracted from its first byte.
//!
//! The fleet's weak-primary switch (`queuecoord::weak_primary_switch`) fails a weak primary
//! over to a whole alt while little of it came. That threw away the payload the primary had
//! downloaded and extracted, and fetched it again from the alt. When the primary's extraction
//! runs and the alt can lend live (a stored or AES alt through its payload map, a compressed
//! one through a restart near the frontier, `restartlend`), the switch cuts the primary
//! instead: every data article it has not fetched yet is answered "not found" at once, without
//! a server, so its remaining volumes go dead and the alt lends them from the frontier to the
//! end, the way it lends the dead tail of a primary that died. Nothing before the frontier is
//! fetched again; the holes the primary had before it are borrowed as before.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Cut downloads, each with the alt it was cut for.
fn cut_set() -> MutexGuard<'static, HashMap<i32, i32>> {
    static SET: OnceLock<Mutex<HashMap<i32, i32>>> = OnceLock::new();
    SET.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Cuts download `nzb_id` at its frontier for alt `alt_id`, the copy proven to hold its video
/// (equal inner keys) that lends the rest. True when it was not cut already.
pub fn cut(nzb_id: i32, alt_id: i32) -> bool {
    if nzb_id <= 0 || alt_id <= 0 {
        return false;
    }
    let mut set = cut_set();
    if set.contains_key(&nzb_id) {
        return false;
    }
    set.insert(nzb_id, alt_id);
    true
}

/// Whether download `nzb_id` is cut: its data articles not fetched yet are not fetched.
pub fn is_cut(nzb_id: i32) -> bool {
    cut_set().contains_key(&nzb_id)
}

/// The alt download `nzb_id` was cut for: only that copy keeps the cut (another backup the
/// failover order picks later was never proven to hold the video).
pub fn cut_for(nzb_id: i32) -> Option<i32> {
    cut_set().get(&nzb_id).copied()
}

/// Forgets the cut of download `nzb_id` (it left the queue, failed over, or starts over).
pub fn forget(nzb_id: i32) {
    cut_set().remove(&nzb_id);
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_cut_holds_until_forgotten() {
        let id = 0x7ffe_0001;
        assert!(!super::is_cut(id));
        assert!(super::cut(id, 7));
        assert!(!super::cut(id, 8), "cut once");
        assert!(super::is_cut(id));
        assert_eq!(super::cut_for(id), Some(7), "for the alt it was cut for");
        super::forget(id);
        assert!(!super::is_cut(id));
        assert!(!super::cut(0, 7), "no download 0");
        assert!(!super::cut(id, 0), "no alt 0");
    }
}
