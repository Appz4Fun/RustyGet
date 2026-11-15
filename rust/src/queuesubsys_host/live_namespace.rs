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
        nzb: usize,
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
            self.raw(
                34,
                5,
                index as i32,
                nzb,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                strings.as_mut_ptr(),
            );
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
        nzb_ptr: usize,
        callback: F,
    ) -> Result<(), RepairError>
    where
        F: FnMut(BorrowedNamespaceRow<'_>) -> Result<(), RepairError>,
    {
        let mut state = Visit {
            callback,
            error: None,
        };
        let status = self.raw(
            43,
            15,
            0,
            nzb_ptr,
            std::ptr::from_mut(&mut state) as usize,
            visit::<F> as *const () as *const c_char,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
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
        nzb_ptr: usize,
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
