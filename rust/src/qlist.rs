//! An ordered list that owns host objects: the download queue, the history
//! and each collection's file list. The host gives a drop function when it
//! creates a list; the list calls it for every item it erases, replaces or
//! still holds when freed, always after releasing its own lock.

use std::collections::VecDeque;
use std::ffi::c_int;
use std::sync::Mutex;

pub type DropFn = unsafe extern "C" fn(usize);

pub struct PtrList {
    items: Mutex<VecDeque<usize>>,
    drop_fn: DropFn,
}

impl PtrList {
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<usize>> {
        self.items.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn drop_all(drop_fn: DropFn, items: impl IntoIterator<Item = usize>) {
    for p in items {
        if p != 0 {
            unsafe { drop_fn(p) };
        }
    }
}

pub extern "C" fn nzbget_rs_ptrlist_new(drop_fn: DropFn) -> *mut PtrList {
    Box::into_raw(Box::new(PtrList {
        items: Mutex::new(VecDeque::new()),
        drop_fn,
    }))
}

pub unsafe extern "C" fn nzbget_rs_ptrlist_free(l: *mut PtrList) {
    if l.is_null() {
        return;
    }
    let list = unsafe { Box::from_raw(l) };
    let items = std::mem::take(&mut *list.lock());
    drop_all(list.drop_fn, items);
}

/// Every item of the list, in order, read under one lock. A caller walking a
/// whole list (a collection's files) takes it once this way rather than once
/// per index with ops 0 and 1, which made a snapshot of a large queue cost a
/// lock round trip per file (6790: the queue coordinator spun on them).
pub(crate) fn items(l: *const PtrList) -> Vec<usize> {
    if l.is_null() {
        return Vec::new();
    }
    let list = unsafe { &*l };
    list.lock().iter().copied().collect()
}

/// Ops (index a, item p):
///  0 count; 1 item at a (0 when out of range); 2 index of p (-1);
///  3 add p at the back; 4 add p at the front; 5 insert p before index a
///    (at the back when a is out of range);
///  6 release p: take it out without dropping, returns p or 0;
///  7 erase at a, dropping it; 8 replace at a with p, dropping the old one;
///  9 clear, dropping all;
///  10 reorder: `order` holds a items, a permutation of the current ones
///     (ignored otherwise);
///  11 append and release every item of list `other` (returns how many).
pub unsafe extern "C" fn nzbget_rs_ptrlist_op(
    l: *const PtrList,
    op: c_int,
    a: i64,
    p: usize,
    order: *const usize,
    other: *const PtrList,
) -> i64 {
    let list = unsafe { &*l };
    let idx = usize::try_from(a).ok();
    if op == 11 {
        if other.is_null() || std::ptr::eq(other, l) {
            return 0;
        }
        let taken = std::mem::take(&mut *unsafe { &*other }.lock());
        let n = taken.len() as i64;
        list.lock().extend(taken);
        return n;
    }
    let mut v = list.lock();
    match op {
        0 => v.len() as i64,
        1 => idx.and_then(|i| v.get(i)).copied().unwrap_or(0) as i64,
        2 => v.iter().position(|&x| x == p).map_or(-1, |i| i as i64),
        3 => {
            v.push_back(p);
            0
        }
        4 => {
            v.push_front(p);
            0
        }
        5 => {
            match idx.filter(|&i| i <= v.len()) {
                Some(i) => v.insert(i, p),
                None => v.push_back(p),
            }
            0
        }
        6 => match v.iter().position(|&x| x == p) {
            Some(i) => {
                v.remove(i);
                p as i64
            }
            None => 0,
        },
        7 => {
            let gone = idx.and_then(|i| v.remove(i));
            drop(v);
            drop_all(list.drop_fn, gone);
            0
        }
        8 => {
            let old = idx
                .and_then(|i| v.get_mut(i))
                .map(|slot| std::mem::replace(slot, p));
            drop(v);
            drop_all(list.drop_fn, old);
            0
        }
        9 => {
            let all = std::mem::take(&mut *v);
            drop(v);
            drop_all(list.drop_fn, all);
            0
        }
        10 => {
            let n = v.len();
            if order.is_null() || idx != Some(n) {
                return 0;
            }
            let new: Vec<usize> = unsafe { std::slice::from_raw_parts(order, n) }.to_vec();
            let mut a: Vec<usize> = v.iter().copied().collect();
            let mut b = new.clone();
            a.sort_unstable();
            b.sort_unstable();
            if a == b {
                *v = new.into();
            }
            0
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn count_drop(p: usize) {
        DROPPED.fetch_add(p, Ordering::SeqCst);
    }

    #[test]
    fn owns_and_drops() {
        let n: *const usize = std::ptr::null();
        let none: *const PtrList = std::ptr::null();
        unsafe {
            let l = nzbget_rs_ptrlist_new(count_drop);
            let op = |op, a, p| nzbget_rs_ptrlist_op(l, op, a, p, n, none);
            op(3, 0, 10);
            op(3, 0, 20);
            op(4, 0, 5);
            op(5, 1, 7);
            assert_eq!(
                (0..4).map(|i| op(1, i, 0)).collect::<Vec<_>>(),
                [5, 7, 10, 20]
            );
            assert_eq!(op(2, 0, 10), 2);
            assert_eq!(op(6, 0, 7), 7);
            assert_eq!(op(6, 0, 7), 0);
            op(7, 0, 0);
            assert_eq!(DROPPED.load(Ordering::SeqCst), 5);
            op(8, 0, 11);
            assert_eq!(DROPPED.load(Ordering::SeqCst), 15);
            let order = [20usize, 11];
            nzbget_rs_ptrlist_op(l, 10, 2, 0, order.as_ptr(), none);
            assert_eq!(op(1, 0, 0), 20);
            let bad = [20usize, 99];
            nzbget_rs_ptrlist_op(l, 10, 2, 0, bad.as_ptr(), none);
            assert_eq!(op(1, 1, 0), 11);
            let m = nzbget_rs_ptrlist_new(count_drop);
            nzbget_rs_ptrlist_op(m, 3, 0, 100, n, none);
            assert_eq!(nzbget_rs_ptrlist_op(l, 11, 0, 0, n, m), 1);
            assert_eq!(nzbget_rs_ptrlist_op(m, 0, 0, 0, n, none), 0);
            nzbget_rs_ptrlist_free(m);
            assert_eq!(DROPPED.load(Ordering::SeqCst), 15);
            nzbget_rs_ptrlist_free(l);
            assert_eq!(DROPPED.load(Ordering::SeqCst), 15 + 20 + 11 + 100);
        }
    }
}
