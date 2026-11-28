#![no_main]
use libfuzzer_sys::fuzz_target;
use nzbget_rs::nntp::{self, Abort, Auth, Io};
use std::ffi::{c_char, CStr, CString};

struct Mock {
    lines: Vec<CString>,
    next: usize,
    cur: Vec<u8>,
    user: CString,
    pass: CString,
    name: CString,
    writes: usize,
}

// SAFETY: read_line hands out a pointer into `cur`, which only read_line writes.
unsafe impl Io for Mock {
    fn write_line(&mut self, _line: &CStr) -> Result<(), Abort> {
        self.writes += 1;
        if self.writes > 64 { Err(Abort) } else { Ok(()) }
    }
    fn read_line(&mut self) -> Result<*mut c_char, Abort> {
        let Some(l) = self.lines.get(self.next) else { return Ok(std::ptr::null_mut()) };
        self.next += 1;
        self.cur.clear();
        self.cur.extend_from_slice(l.as_bytes_with_nul());
        Ok(self.cur.as_mut_ptr() as *mut c_char)
    }
    fn report_error(&mut self, _p: &CStr, _a: &CStr) -> Result<(), Abort> { Ok(()) }
    fn debug(&mut self, _m: &CStr) -> Result<(), Abort> { Ok(()) }
    fn cancelled(&mut self) -> Result<bool, Abort> { Ok(false) }
    fn user(&self) -> &CStr { &self.user }
    fn password(&self) -> &CStr { &self.pass }
    fn name(&self) -> &CStr { &self.name }
    fn host(&self) -> &CStr { &self.name }
    fn conn_host(&self) -> &CStr { &self.name }
}

fuzz_target!(|data: &[u8]| {
    let Some((&sel, rest)) = data.split_first() else { return };
    let lines: Vec<CString> = rest
        .split(|&b| b == b'\n')
        .map(|l| CString::new(l.iter().copied().filter(|&b| b != 0).collect::<Vec<u8>>()).unwrap())
        .collect();
    let mk = |with_auth: bool| Mock {
        lines: lines.clone(),
        next: 0,
        cur: Vec::new(),
        user: CString::new(if with_auth { "u" } else { "" }).unwrap(),
        pass: CString::new(if with_auth { "p" } else { "" }).unwrap(),
        name: CString::new("srv").unwrap(),
        writes: 0,
    };
    let mut auth = Auth::default();
    let mut io = mk(sel & 1 != 0);
    if sel & 2 == 0 {
        let _ = nntp::handshake(&mut io, &mut auth);
    } else {
        let mut io = mk(true);
        let _ = nntp::authenticate(&mut io, &mut auth);
    }
    let first = lines.first().map(|l| l.as_bytes());
    let _ = nntp::article_dl_is_join_group_ok(first);
});
