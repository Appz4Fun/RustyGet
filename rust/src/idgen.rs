//! Id generators for queued downloads and their files (NZBGet's
//! `NzbInfo::m_idGen`/`m_idMax` and `FileInfo::m_idGen`/`m_idMax`).
//!
//! Each generator hands out increasing ids and remembers the highest id
//! seen, so that after loading a saved queue it can continue above every
//! id already in use.

use std::sync::atomic::{AtomicI32, Ordering::SeqCst};

/// An id generator with a floor: the highest id seen so far.
#[derive(Debug, Default)]
pub struct IdGen {
    last: AtomicI32,
    floor: AtomicI32,
}

impl IdGen {
    pub const fn new() -> Self {
        Self {
            last: AtomicI32::new(0),
            floor: AtomicI32::new(0),
        }
    }

    /// The last id handed out.
    pub fn last(&self) -> i32 {
        self.last.load(SeqCst)
    }

    /// Hands out the next id.
    pub fn next(&self) -> i32 {
        self.last.fetch_add(1, SeqCst) + 1
    }

    /// Records `id` as in use: the floor rises to it if it is higher.
    pub fn raise_floor(&self, id: i32) {
        self.floor.fetch_max(id, SeqCst);
    }

    /// Continues generating from the floor (after loading a saved queue).
    pub fn reset_to_floor(&self) {
        self.last.store(self.floor.load(SeqCst), SeqCst);
    }
}

/// Ids of queued downloads (collections).
pub static NZB_IDS: IdGen = IdGen::new();
/// Ids of the files in queued downloads.
pub static FILE_IDS: IdGen = IdGen::new();

#[cfg(test)]
mod tests {
    use super::IdGen;

    #[test]
    fn hands_out_ids_and_resumes_above_the_floor() {
        let g = IdGen::new();
        assert_eq!(g.last(), 0);
        assert_eq!(g.next(), 1);
        assert_eq!(g.next(), 2);
        g.raise_floor(10);
        g.raise_floor(4);
        assert_eq!(g.last(), 2);
        g.reset_to_floor();
        assert_eq!(g.last(), 10);
        assert_eq!(g.next(), 11);
    }
}
