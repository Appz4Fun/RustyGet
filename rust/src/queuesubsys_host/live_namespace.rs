//! Synchronous borrowed queue transport. The caller holds the global queue.
use super::*;
use crate::parchecker::live_namespace::{NamespaceFacts, NamespaceRow};
use crate::parchecker::RepairError;
use std::mem::size_of;
use std::path::Path;

const MAX_ROWS: usize = 32768;
const MAX_FIELD: usize = 65536;

fn refused(message: &'static str) -> RepairError {
    RepairError::Io {
        context: "live namespace capture".to_owned(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    }
}

pub(crate) struct BorrowedNamespaceRow<'a> {
    pub queue_id: i32,
    pub flags: u32,
    pub fields: [&'a [u8]; 4],
}

struct Visit<F> {
    callback: F,
    error: Option<RepairError>,
}

unsafe extern "C" fn visit<F>(
    context: *mut c_void,
    queue_id: c_int,
    flags: u32,
    name: *const c_char,
    original: *const c_char,
    hash: *const c_char,
    output: *const c_char,
) -> bool
where
    F: FnMut(BorrowedNamespaceRow<'_>) -> Result<(), RepairError>,
{
    let state = &mut *context.cast::<Visit<F>>();
    let pointers = [name, original, hash, output];
    let result = (|| {
        if queue_id <= 0 || flags & !7 != 0 || pointers.iter().any(|p| p.is_null()) {
            return Err(refused("invalid borrowed row"));
        }
        let fields = pointers.map(|p| CStr::from_ptr(p).to_bytes());
        if fields.iter().any(|s| s.len() > MAX_FIELD) {
            return Err(refused("borrowed field exceeds bound"));
        }
        (state.callback)(BorrowedNamespaceRow {
            queue_id,
            flags,
            fields,
        })
    })();
    match result {
        Ok(()) => true,
        Err(error) => {
            state.error = Some(error);
            false
        }
    }
}

impl QueueSubsysHost {
    /// Existing completed-record ABI, inspected without owning any strings.
    pub(crate) fn live_completed_matches(
        &self,
        nzb: crate::queuesubsys_host::NzbRef,
        id: u64,
        name: &[u8],
        successful: bool,
    ) -> bool {
        let count = self.nzb_completed_files_len(nzb);
        if count > MAX_ROWS {
            return false;
        }
        for index in 0..count {
            let mut ints = [0; 4];
            let mut strings = [std::ptr::null(); 4];
            let x = OpArgs::NONE
                .a2(index as i32)
                .ints(ints.as_mut_ptr())
                .strs(strings.as_mut_ptr());
            self.filelist_op(nzb, 5, x);
            if ints[0] as u64 != id {
                continue;
            }
            return !strings[0].is_null()
                && unsafe { CStr::from_ptr(strings[0]).to_bytes() } == name
                && (!successful || ints[1] == super::CF_SUCCESS);
        }
        false
    }

    /// Pointers are valid only during their callback; no queue reentry or GF work.
    pub(crate) fn live_namespace_visit<F>(
        &self,
        nzb_ptr: crate::queuesubsys_host::NzbRef,
        callback: F,
    ) -> Result<(), RepairError>
    where
        F: FnMut(BorrowedNamespaceRow<'_>) -> Result<(), RepairError>,
    {
        let mut state = Visit {
            callback,
            error: None,
        };
        let status = if self.use_active {
            unsafe {
                live_par_visit_namespace(
                    nzb_ptr.addr() as *const crate::qnzb::NzbScalars,
                    Some(visit::<F>),
                    std::ptr::from_mut(&mut state).cast(),
                )
            }
        } else {
            self.raw(
                43,
                15,
                0,
                nzb_ptr.addr(),
                std::ptr::from_mut(&mut state) as usize,
                visit::<F> as *const () as *const c_char,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if let Some(error) = state.error {
            return Err(error);
        }
        match status {
            1 => Ok(()),
            2 => Err(RepairError::Io {
                context: "live namespace busy".to_owned(),
                source: std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "source assembly is active",
                ),
            }),
            0 => Err(refused("native producer refused namespace")),
            _ => Err(refused("invalid native namespace status")),
        }
    }

    /// Both passes run under the same caller-held queue guard. The sizing pass
    /// owns no source strings; the existing control pool admits all later copies.
    pub(crate) fn live_namespace_capture(
        &self,
        nzb_ptr: crate::queuesubsys_host::NzbRef,
        nzb_id: i32,
        root: &Path,
        catalog_bytes: usize,
        pool: u64,
    ) -> Result<NamespaceFacts, RepairError> {
        let mut count = 0usize;
        let mut string_bytes = 0usize;
        self.live_namespace_visit(nzb_ptr, |row| {
            count = count
                .checked_add(1)
                .ok_or_else(|| refused("row count overflow"))?;
            if count > MAX_ROWS {
                return Err(refused("row count exceeds bound"));
            }
            for field in row.fields {
                string_bytes = string_bytes
                    .checked_add(field.len())
                    .ok_or_else(|| refused("field sizes overflow"))?;
            }
            Ok(())
        })?;
        let required = count
            .checked_mul(
                size_of::<NamespaceRow>()
                    + size_of::<Option<usize>>()
                    + size_of::<usize>()
                    + size_of::<u8>(),
            )
            .and_then(|n| n.checked_add(string_bytes))
            .and_then(|n| n.checked_add(root.as_os_str().len()))
            .and_then(|n| n.checked_add(catalog_bytes))
            .ok_or_else(|| refused("capture size overflow"))?;
        let reservation = crate::parlive::reserve_control(nzb_id, required.max(256) as u64, pool)?;
        let mut facts = NamespaceFacts {
            root: std::path::PathBuf::new(),
            rows: Vec::new(),
            reservation,
        };
        facts
            .root
            .try_reserve_exact(root.as_os_str().len())
            .map_err(|_| refused("root allocation refused"))?;
        facts
            .rows
            .try_reserve_exact(count)
            .map_err(|_| refused("row allocation refused"))?;
        if facts.root.capacity() > root.as_os_str().len() || facts.rows.capacity() > count {
            return Err(refused("capture allocation exceeds preflight"));
        }
        facts.root.push(root);
        let mut copied = 0usize;
        self.live_namespace_visit(nzb_ptr, |row| {
            if facts.rows.len() >= count {
                return Err(refused("namespace grew between passes"));
            }
            let mut fields = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            for (owned, field) in fields.iter_mut().zip(row.fields) {
                copied = copied
                    .checked_add(field.len())
                    .ok_or_else(|| refused("copy sizes overflow"))?;
                if copied > string_bytes {
                    return Err(refused("namespace strings grew between passes"));
                }
                owned
                    .try_reserve_exact(field.len())
                    .map_err(|_| refused("field allocation refused"))?;
                if owned.capacity() > field.len() {
                    return Err(refused("field allocation exceeds preflight"));
                }
                owned.extend_from_slice(field);
            }
            let [name, original, hash16k, output] = fields;
            facts.rows.push(NamespaceRow {
                queue_id: row.queue_id,
                name,
                original,
                hash16k,
                output,
                par_file: row.flags & 1 != 0,
                deleted: row.flags & 2 != 0,
                completed: row.flags & 4 != 0,
            });
            Ok(())
        })?;
        if facts.rows.len() != count || copied != string_bytes {
            return Err(refused("namespace geometry changed between passes"));
        }
        Ok(facts)
    }
}

/// Borrowed-row visitor: (context, queue id, flags, name, original, hash16k, output).
/// Returning false stops the walk.
pub(crate) type NamespaceVisitor = unsafe extern "C" fn(
    *mut c_void,
    c_int,
    u32,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
) -> bool;

/// Host op 43 sub-op 15: walks a collection's queued files, then its completed
/// files, lending each row's strings to `visit` for the duration of the call.
/// The caller holds the queue throughout. Returns 0 on a callback or limit
/// refusal, 1 when complete, 2 when the collection is transiently busy
/// (parking, deleting, or a file whose final assembly may be clearing its
/// output name). Flags: 1 par file, 2 deleted, 4 completed record.
pub(crate) unsafe fn live_par_visit_namespace(
    nzb: *const crate::qnzb::NzbScalars,
    visit: Option<NamespaceVisitor>,
    context: *mut c_void,
) -> usize {
    use crate::qfile::{
        field, nzbget_rs_file_get, nzbget_rs_file_op, nzbget_rs_file_text, text, File,
    };
    let Some(visit) = visit else { return 0 };
    if nzb.is_null() {
        return 0;
    }
    let scalars = &*nzb;
    if scalars.parking || scalars.deleting {
        return 2;
    }
    let list = crate::qnzb::file_list(nzb) as *const crate::qlist::PtrList;
    let op = |op: c_int, a: i64| {
        crate::qlist::nzbget_rs_ptrlist_op(list, op, a, 0, std::ptr::null(), std::ptr::null())
    };
    let queued = op(0, 0).max(0) as usize;
    let none = std::ptr::null_mut();
    let nl: *mut i64 = std::ptr::null_mut();
    let ns: *mut *const c_char = std::ptr::null_mut();
    let completed_count = crate::qnzb::nzbget_rs_nzb_files(
        nzb,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
        none,
        nl,
        ns,
        std::ptr::null(),
    )
    .max(0) as usize;
    if queued > MAX_ROWS || completed_count > MAX_ROWS - queued {
        return 0;
    }
    let files: Vec<*const File> = (0..queued)
        .map(|i| op(1, i as i64) as usize as *const File)
        .collect();
    // Final assembly can clear the output name outside the queue, even while
    // its last downloader is still registered. Do not borrow in that window.
    for &f in &files {
        let total = nzbget_rs_file_get(f, field::TOTAL_ARTICLES) as i32;
        let completed = nzbget_rs_file_get(f, field::COMPLETED_ARTICLES) as i32;
        let missed = nzbget_rs_file_get(f, field::MISSED_ARTICLES) as i32;
        if total < 0 || completed < 0 || missed < 0 {
            return 0;
        }
        if total > 0 && i64::from(completed) + i64::from(missed) >= i64::from(total) {
            return 2;
        }
    }
    let empty = c"".as_ptr();
    let or_empty = |s: *const c_char| if s.is_null() { empty } else { s };
    for &f in &files {
        let taken = nzbget_rs_file_op(f, 20, 0, std::ptr::null(), none, ns) != 0;
        let flags = u32::from(nzbget_rs_file_get(f, field::PAR_FILE) != 0)
            | (u32::from(nzbget_rs_file_get(f, field::DELETED) != 0) << 1);
        let kept = visit(
            context,
            nzbget_rs_file_get(f, field::ID) as c_int,
            flags,
            or_empty(nzbget_rs_file_text(f, text::FILENAME)),
            or_empty(nzbget_rs_file_text(f, text::ORIGNAME)),
            or_empty(nzbget_rs_file_text(f, text::HASH16K)),
            or_empty(nzbget_rs_file_text(f, text::OUTPUT_FILENAME)),
        );
        if taken {
            nzbget_rs_file_op(f, 21, 0, std::ptr::null(), none, ns);
        }
        if !kept {
            return 0;
        }
    }
    for i in 0..completed_count {
        let mut ints = [0 as c_int; 4];
        let mut strs = [std::ptr::null::<c_char>(); 4];
        if crate::qnzb::nzbget_rs_nzb_files(
            nzb,
            1,
            i as c_int,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            nl,
            strs.as_mut_ptr(),
            std::ptr::null(),
        ) != 0
        {
            return 0;
        }
        let flags = 4 | u32::from(ints[2] != 0);
        if !visit(
            context,
            ints[0],
            flags,
            or_empty(strs[0]),
            or_empty(strs[1]),
            or_empty(strs[2]),
            empty,
        ) {
            return 0;
        }
    }
    1
}

/// Ported from tests/queue/LiveParSourceClaimTest.h (the retired C++ harness).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::qfile::{
        field, nzbget_rs_file_new, nzbget_rs_file_set, nzbget_rs_file_set_text, text,
    };
    use crate::qnzb::{
        nzbget_rs_nzb_scalars_free, nzbget_rs_nzb_scalars_new, nzbget_rs_nzb_slot, NzbScalars,
    };

    struct Row {
        id: i32,
        flags: u32,
        name: String,
        original: String,
        hash: String,
        output: String,
    }

    unsafe extern "C" fn drop_file(p: usize) {
        crate::qfile::nzbget_rs_file_free(p as *mut crate::qfile::File);
    }

    /// A collection with a Rust file list, as NzbInfo's constructor builds it.
    struct Nzb(*mut NzbScalars);
    impl Nzb {
        fn new() -> Self {
            let p = nzbget_rs_nzb_scalars_new();
            let list = crate::qlist::nzbget_rs_ptrlist_new(drop_file);
            unsafe { nzbget_rs_nzb_slot(p, 0, 1, list as usize) };
            Nzb(p)
        }
        fn add(&self, id: i32, name: &str) -> *mut crate::qfile::File {
            let f = nzbget_rs_file_new(id);
            let n = std::ffi::CString::new(name).unwrap();
            unsafe {
                nzbget_rs_file_set(f, field::NZB, self.0 as i64);
                nzbget_rs_file_set_text(f, text::FILENAME, n.as_ptr());
                let list = crate::qnzb::file_list(self.0) as *const crate::qlist::PtrList;
                crate::qlist::nzbget_rs_ptrlist_op(
                    list,
                    3,
                    0,
                    f as usize,
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            f
        }
        fn walk(&self, ctx: *mut c_void, v: NamespaceVisitor) -> usize {
            unsafe { live_par_visit_namespace(self.0, Some(v), ctx) }
        }
    }
    impl Drop for Nzb {
        fn drop(&mut self) {
            unsafe {
                let list = nzbget_rs_nzb_slot(self.0, 0, 1, 0);
                crate::qlist::nzbget_rs_ptrlist_free(list as *mut crate::qlist::PtrList);
                nzbget_rs_nzb_scalars_free(self.0);
            }
        }
    }

    unsafe fn s(p: *const c_char) -> String {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
    unsafe extern "C" fn collect(
        ctx: *mut c_void,
        id: c_int,
        flags: u32,
        name: *const c_char,
        original: *const c_char,
        hash: *const c_char,
        output: *const c_char,
    ) -> bool {
        let rows = &mut *ctx.cast::<Vec<Row>>();
        rows.push(Row {
            id,
            flags,
            name: s(name),
            original: s(original),
            hash: s(hash),
            output: s(output),
        });
        true
    }
    unsafe extern "C" fn stop(
        ctx: *mut c_void,
        _: c_int,
        _: u32,
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
        _: *const c_char,
    ) -> bool {
        *ctx.cast::<i32>() += 1;
        false
    }

    #[test]
    fn live_par_namespace_borrowed_rows_cover_queue_and_completed() {
        let nzb = Nzb::new();
        let source = nzb.add(40, "part40.rar");
        unsafe {
            nzbget_rs_file_set_text(source, text::ORIGNAME, c"original-part40.rar".as_ptr());
            nzbget_rs_file_set_text(
                source,
                text::HASH16K,
                c"00112233445566778899aabbccddeeff".as_ptr(),
            );
            nzbget_rs_file_set_text(
                source,
                text::OUTPUT_FILENAME,
                c"/fixture/40.out.tmp".as_ptr(),
            );
            nzbget_rs_file_set(source, field::ACTIVE_DOWNLOADS, 1);
        }
        let parity = nzb.add(99, "rel.par2");
        unsafe { nzbget_rs_file_set(parity, field::PAR_FILE, 1) };
        let mut ints = [39, 1 /* cfSuccess */, 0, 0];
        let mut strs = [
            c"part39.rar".as_ptr(),
            c"part39.rar".as_ptr(),
            c"".as_ptr(),
            c"".as_ptr(),
        ];
        unsafe {
            crate::qnzb::nzbget_rs_nzb_files(
                nzb.0,
                2,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                strs.as_mut_ptr(),
                std::ptr::null(),
            );
        }
        let mut rows: Vec<Row> = Vec::new();
        let ctx = std::ptr::from_mut(&mut rows).cast::<c_void>();
        assert_eq!(nzb.walk(ctx, collect), 1);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, 40);
        assert_eq!(rows[0].name, "part40.rar");
        assert_eq!(rows[0].original, "original-part40.rar");
        assert_eq!(rows[0].hash, "00112233445566778899aabbccddeeff");
        assert_eq!(rows[0].output, "/fixture/40.out.tmp");
        assert_eq!(rows[1].id, 99);
        assert_ne!(rows[1].flags & 1, 0);
        assert_eq!(rows[1].hash, "");
        assert_eq!(rows[2].id, 39);
        assert_eq!(rows[2].name, "part39.rar");
        assert_ne!(rows[2].flags & 4, 0);
        rows.clear();
        let ctx = std::ptr::from_mut(&mut rows).cast::<c_void>();
        assert_eq!(nzb.walk(ctx, collect), 1);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].output, "/fixture/40.out.tmp");
    }

    #[test]
    fn live_par_namespace_sizing_refusal_stops_before_copy() {
        let nzb = Nzb::new();
        let first = nzb.add(40, "part.rar");
        nzb.add(41, "part.rar");
        let mut visits = 0i32;
        let ctx = std::ptr::from_mut(&mut visits).cast::<c_void>();
        assert_eq!(nzb.walk(ctx, stop), 0);
        assert_eq!(visits, 1);
        visits = 0;
        assert_eq!(nzb.walk(ctx, stop), 0);
        assert_eq!(visits, 1);
        // Assembly may still have its final downloader registered. Neither
        // active count makes borrowing its changing output string safe.
        unsafe {
            nzbget_rs_file_set(first, field::TOTAL_ARTICLES, 1);
            nzbget_rs_file_set(first, field::COMPLETED_ARTICLES, 1);
        }
        for active in [0, 1] {
            unsafe { nzbget_rs_file_set(first, field::ACTIVE_DOWNLOADS, active) };
            visits = 0;
            assert_eq!(nzb.walk(ctx, stop), 2);
            assert_eq!(visits, 0);
            assert_eq!(nzb.walk(ctx, stop), 2);
            assert_eq!(visits, 0);
        }
        unsafe {
            nzbget_rs_file_set(first, field::ACTIVE_DOWNLOADS, 0);
            nzbget_rs_file_set(first, field::COMPLETED_ARTICLES, 0);
            (*nzb.0).parking = true;
        }
        assert_eq!(nzb.walk(ctx, stop), 2);
        unsafe {
            (*nzb.0).parking = false;
            (*nzb.0).deleting = true;
        }
        assert_eq!(nzb.walk(ctx, stop), 2);
        assert_eq!(visits, 0);
    }
}
