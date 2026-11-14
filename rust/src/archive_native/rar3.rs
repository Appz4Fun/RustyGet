//! RAR 2.9's compression (RAR 3.x and 4.x, version 29), from the front of a file's packed data: LZ blocks with
//! Huffman tables coded against the last ones, and the standard filters RAR's VM runs on the output. Written from
//! the format as unrar describes it. Every byte decoded is kept, soo the whole output is the dictionary; the limit
//! bounds it. PPM blocks go through our PPMd variant H.

use super::ppmd_h::Ppm;
use super::rarbits::{Bits, Table};

#[inline]
fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

const NC: usize = 299;
const DC: usize = 60;
const LDC: usize = 17;
const RC: usize = 28;
const BC: usize = 20;
const TABLE: usize = NC + DC + LDC + RC;
const LOW_DIST_REP: u32 = 16;
const MAX_FILTERS: usize = 8192;
/// The VM's memory: no filter's block is bigger
const VM_MEM: usize = 0x40000;

const LDECODE: [u8; 28] =
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224];
const LBITS: [u8; 28] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5];
const SDDECODE: [u8; 8] = [0, 4, 8, 16, 32, 64, 128, 192];
const SDBITS: [u8; 8] = [2, 2, 3, 4, 5, 6, 6, 6];

/// The distance slots: how many slots of each bit length.
fn distances() -> ([u32; DC], [u8; DC]) {
    const COUNTS: [usize; 19] = [4, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 14, 0, 12];
    let (mut base, mut bits) = ([0u32; DC], [0u8; DC]);
    let (mut dist, mut slot) = (0u32, 0);
    for (len, &count) in COUNTS.iter().enumerate() {
        for _ in 0..count {
            base[slot] = dist;
            bits[slot] = len as u8;
            dist += 1 << len;
            slot += 1;
        }
    }
    (base, bits)
}

/// The VM's standard filters, told apart by their code's length and CRC.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    E8,
    E8E9,
    Itanium,
    Delta,
    Rgb,
    Audio,
    /// a program of its own: it would need the VM itself
    Other,
}

fn kind_of(code: &[u8]) -> Kind {
    let xor = code.iter().skip(1).fold(0u8, |x, b| x ^ b);
    if code.first() != Some(&xor) {
        return Kind::Other;
    }
    let crc = crc32(code);
    match (code.len(), crc) {
        (53, 0xad57_6887) => Kind::E8,
        (57, 0x3cd7_e57e) => Kind::E8E9,
        (120, 0x3769_893f) => Kind::Itanium,
        (29, 0x0e06_077d) => Kind::Delta,
        (149, 0x1c2c_5dc8) => Kind::Rgb,
        (216, 0xbc85_e701) => Kind::Audio,
        _ => Kind::Other,
    }
}

/// A filter to run on one block of the output.
#[derive(Clone)]
struct Pending {
    kind: Kind,
    start: usize,
    len: usize,
    regs: [u32; 7],
}

#[derive(Default, Clone)]
struct Filters {
    /// the kind of each filter defined so far
    kinds: Vec<Kind>,
    old_lengths: Vec<u32>,
    last: usize,
    pending: Vec<Pending>,
    /// where the furthest pending block ends: past the limit, decoding goes on to here
    reach: usize,
    /// filter work still allowed, block bytes and program bytes alike: filters that don't overlap cover each byte
    /// once, so more than the output and one filter's memory is work no real file asks for
    work: usize,
}

#[derive(Clone)]
struct Tables {
    ld: Table,
    dd: Table,
    ldd: Table,
    rd: Table,
}

#[derive(Clone)]
pub struct Rar29Decoder {
    window: Vec<u8>,
    old_table: [u8; TABLE],
    reps: [usize; 4],
    last_len: usize,
    low_dist: u32,
    low_reps: u32,
    filters: Filters,
    ppm: Option<Box<Ppm>>,
    esc: u8,
    saved_tables: Option<Option<Tables>>,
    initialized: bool,
}

impl Default for Rar29Decoder {
    fn default() -> Self {
        Self {
            window: Vec::new(),
            old_table: [0; TABLE],
            reps: [0; 4],
            last_len: 0,
            low_dist: 0,
            low_reps: 0,
            filters: Filters::default(),
            ppm: None,
            esc: 2,
            saved_tables: None,
            initialized: false,
        }
    }
}

impl Rar29Decoder {
    pub fn decode(
        &mut self,
        data: &[u8],
        unpacked_size: usize,
        solid: bool,
    ) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(unpacked_size.min(16 * 1024 * 1024));
        let mut pos = 0usize;
        self.decode_stream(
            unpacked_size,
            solid,
            &mut |in_buf| {
                if pos >= data.len() {
                    return Ok(0);
                }
                let end = (pos + 256 * 1024).min(data.len());
                let n = end - pos;
                in_buf.extend_from_slice(&data[pos..end]);
                pos = end;
                Ok(n)
            },
            &mut |chunk| {
                out.extend_from_slice(chunk);
                Ok(())
            },
        )?;
        Ok(out)
    }

    pub fn decode_stream(
        &mut self,
        unpacked_size: usize,
        solid: bool,
        refill: &mut dyn FnMut(&mut Vec<u8>) -> Result<usize, String>,
        on_output: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if !solid || !self.initialized {
            *self = Self {
                esc: 2,
                initialized: true,
                ..Self::default()
            };
        }
        if self.window.len() > 0x40_0000 {
            let drop_len = self.window.len() - 0x40_0000;
            self.window.drain(..drop_len);
        }
        let file_start = self.window.len();
        let goal = file_start
            .checked_add(unpacked_size)
            .ok_or_else(|| "Unpacked size overflow".to_string())?;
        self.filters.pending.clear();
        self.filters.reach = file_start;

        let mut st = StreamState {
            addr: 0,
            bit: 0,
            old_table: self.old_table,
            reps: self.reps,
            last_len: self.last_len,
            low_dist: self.low_dist,
            low_reps: self.low_reps,
            filters: std::mem::take(&mut self.filters),
            ppm: self.ppm.take(),
            esc: self.esc,
            tables_left: 0,
        };
        let init_tables = self.saved_tables.take();
        let mut base_offset = 0usize;
        let mut flushed_to = file_start;
        let mut io_err: Option<String> = None;
        let res = run_member_stream(
            &mut st,
            init_tables,
            goal,
            file_start,
            &mut base_offset,
            &mut flushed_to,
            &mut self.window,
            refill,
            on_output,
            &mut io_err,
        );
        self.old_table = st.old_table;
        self.reps = st.reps;
        self.last_len = st.last_len;
        self.low_dist = st.low_dist;
        self.low_reps = st.low_reps;
        self.filters = st.filters;
        self.ppm = st.ppm;
        self.esc = st.esc;

        if let Some(err) = io_err {
            self.saved_tables = None;
            return Err(err);
        }

        match res {
            Ok(next_tables) => {
                self.saved_tables = next_tables;
            }
            Err(Stop::Cut) => {
                self.saved_tables = None;
            }
        }
        let logical_len = base_offset + self.window.len();
        if logical_len < goal {
            return Err(format!(
                "Unrar: Decompression produced {} bytes, expected {}",
                logical_len.saturating_sub(file_start),
                unpacked_size
            ));
        }
        flush_window(
            &mut self.window,
            &mut self.filters,
            &mut base_offset,
            &mut flushed_to,
            file_start,
            goal,
            true,
            on_output,
        )?;
        let tail_end = goal.saturating_sub(base_offset);
        self.window.truncate(tail_end);
        if self.window.len() > 0x40_0000 {
            let drop_len = self.window.len() - 0x40_0000;
            self.window.drain(..drop_len);
        }
        Ok(())
    }
}

struct StreamState {
    addr: usize,
    bit: u32,
    old_table: [u8; TABLE],
    reps: [usize; 4],
    last_len: usize,
    low_dist: u32,
    low_reps: u32,
    filters: Filters,
    ppm: Option<Box<Ppm>>,
    esc: u8,
    tables_left: usize,
}

#[allow(clippy::too_many_arguments)]
fn flush_window(
    out: &mut Vec<u8>,
    filters: &mut Filters,
    base_offset: &mut usize,
    flushed_to: &mut usize,
    file_start: usize,
    goal: usize,
    final_flush: bool,
    on_output: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let logical_len = *base_offset + out.len();
    let mut safe_end = if final_flush {
        goal.min(logical_len)
    } else {
        let Some(keep_dict) = logical_len.checked_sub(0x40_0000) else {
            return Ok(());
        };
        keep_dict.min(goal)
    };
    if !final_flush {
        for f in &filters.pending {
            if f.start + f.len > safe_end {
                safe_end = safe_end.min(f.start);
            }
        }
    }
    if safe_end <= *flushed_to {
        return Ok(());
    }
    let slice_start = *flushed_to - *base_offset;
    let slice_end = safe_end - *base_offset;
    if filters.pending.is_empty() {
        on_output(&out[slice_start..slice_end])?;
    } else {
        let ready_count = if final_flush {
            filters.pending.len()
        } else {
            filters
                .pending
                .iter()
                .take_while(|f| f.start + f.len <= safe_end)
                .count()
        };
        if ready_count == 0 {
            on_output(&out[slice_start..slice_end])?;
        } else {
            let win_end = if final_flush { out.len() } else { slice_end };
            let mut filtered = apply_filters_range(
                &out[slice_start..win_end],
                &filters.pending[..ready_count],
                *flushed_to,
                file_start,
            );
            filters.pending.drain(..ready_count);
            let want_len = safe_end - *flushed_to;
            if filtered.len() < want_len {
                return Err("Unrar: Filter execution failed".to_string());
            }
            filtered.truncate(want_len);
            on_output(&filtered)?;
        }
    }
    *flushed_to = safe_end;
    if !final_flush {
        let drop_len = safe_end - *base_offset;
        if drop_len > 0 {
            out.drain(..drop_len);
            *base_offset = safe_end;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_member_stream(
    st: &mut StreamState,
    init_tables: Option<Option<Tables>>,
    goal: usize,
    file_start: usize,
    base_offset: &mut usize,
    flushed_to: &mut usize,
    out: &mut Vec<u8>,
    refill: &mut dyn FnMut(&mut Vec<u8>) -> Result<usize, String>,
    on_output: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    io_err: &mut Option<String>,
) -> Result<Option<Option<Tables>>, Stop> {
    let (dbase, dbits) = distances();
    let target = goal.saturating_add(VM_MEM);
    st.filters.work = target.saturating_sub(out.len()).saturating_add(VM_MEM);
    st.tables_left = 1024 + target.saturating_sub(out.len()) / 4096;

    let mut in_buf: Vec<u8> = Vec::with_capacity(384 * 1024);
    let mut in_eof = false;
    let ensure_input = |in_buf: &mut Vec<u8>,
                        addr: &mut usize,
                        in_eof: &mut bool,
                        refill: &mut dyn FnMut(&mut Vec<u8>) -> Result<usize, String>,
                        io_err: &mut Option<String>|
     -> Result<(), Stop> {
        while !*in_eof && in_buf.len().saturating_sub(*addr) < 128 * 1024 {
            if *addr > 0 {
                in_buf.drain(..*addr);
                *addr = 0;
            }
            match refill(in_buf) {
                Ok(0) => *in_eof = true,
                Ok(_) => {}
                Err(e) => {
                    *io_err = Some(e);
                    return Err(Stop::Cut);
                }
            }
        }
        Ok(())
    };

    ensure_input(&mut in_buf, &mut st.addr, &mut in_eof, refill, io_err)?;
    let mut tables = match init_tables {
        Some(t) => t,
        None => {
            let mut sub = State {
                bits: Bits {
                    data: &in_buf,
                    addr: st.addr,
                    bit: st.bit,
                },
                safe: in_buf.len(),
                old_table: st.old_table,
                reps: st.reps,
                last_len: st.last_len,
                low_dist: st.low_dist,
                low_reps: st.low_reps,
                filters: std::mem::take(&mut st.filters),
                ppm: st.ppm.take(),
                esc: st.esc,
                tables_left: st.tables_left,
            };
            let res = read_tables(&mut sub);
            st.addr = sub.bits.addr;
            st.bit = sub.bits.bit;
            st.old_table = sub.old_table;
            st.reps = sub.reps;
            st.last_len = sub.last_len;
            st.low_dist = sub.low_dist;
            st.low_reps = sub.low_reps;
            st.filters = sub.filters;
            st.ppm = sub.ppm;
            st.esc = sub.esc;
            st.tables_left = sub.tables_left;
            res?
        }
    };

    loop {
        ensure_input(&mut in_buf, &mut st.addr, &mut in_eof, refill, io_err)?;
        if out.len() >= 0x80_0000 {
            if let Err(e) = flush_window(
                out,
                &mut st.filters,
                base_offset,
                flushed_to,
                file_start,
                goal,
                false,
                on_output,
            ) {
                *io_err = Some(e);
                return Err(Stop::Cut);
            }
        }
        let logical_len = *base_offset + out.len();
        if logical_len >= target || (logical_len >= goal && logical_len >= st.filters.reach) {
            let sub = State {
                bits: Bits {
                    data: &in_buf,
                    addr: st.addr,
                    bit: st.bit,
                },
                safe: in_buf.len(),
                old_table: st.old_table,
                reps: st.reps,
                last_len: st.last_len,
                low_dist: st.low_dist,
                low_reps: st.low_reps,
                filters: Filters::default(),
                ppm: None,
                esc: st.esc,
                tables_left: st.tables_left,
            };
            return Ok(finish_member_tables(&sub, tables));
        }

        let phys_target = target.saturating_sub(*base_offset);
        let mut sub = State {
            bits: Bits {
                data: &in_buf,
                addr: st.addr,
                bit: st.bit,
            },
            safe: in_buf.len(),
            old_table: st.old_table,
            reps: st.reps,
            last_len: st.last_len,
            low_dist: st.low_dist,
            low_reps: st.low_reps,
            filters: std::mem::take(&mut st.filters),
            ppm: st.ppm.take(),
            esc: st.esc,
            tables_left: st.tables_left,
        };

        let step_res = (|| -> Result<Option<Option<Option<Tables>>>, Stop> {
            if sub.bits.addr > sub.safe || sub.bits.past_end() {
                return Err(Stop::Cut);
            }
            let Some(lz) = &tables else {
                match ppm_step_at(&mut sub, out, phys_target, *base_offset)? {
                    PpmStep::Next => {}
                    PpmStep::Tables => tables = read_tables(&mut sub)?,
                    PpmStep::FileEnd => return Ok(Some(None)),
                }
                return Ok(None);
            };
            let bits = &mut sub.bits;
            let number = usize::from(lz.ld.decode(bits));
            if number < 256 {
                out.push(number as u8);
                return Ok(None);
            }
            if number >= 271 {
                let n = number - 271;
                let mut len = usize::from(LDECODE[n]) + 3 + bits.take(u32::from(LBITS[n])) as usize;
                let slot = usize::from(lz.dd.decode(bits)).min(DC - 1);
                let mut dist = dbase[slot] + 1;
                let b = u32::from(dbits[slot]);
                if b > 0 {
                    if slot > 9 {
                        if b > 4 {
                            dist += (bits.peek16() >> (20 - b)) << 4;
                            bits.skip(b - 4);
                        }
                        if sub.low_reps > 0 {
                            sub.low_reps -= 1;
                            dist += sub.low_dist;
                        } else {
                            let low = u32::from(lz.ldd.decode(bits));
                            if low == 16 {
                                sub.low_reps = LOW_DIST_REP - 1;
                                dist += sub.low_dist;
                            } else {
                                dist += low;
                                sub.low_dist = low;
                            }
                        }
                    } else {
                        dist += bits.take(b);
                    }
                }
                if dist >= 0x2000 {
                    len += 1;
                    if dist >= 0x40000 {
                        len += 1;
                    }
                }
                let dist = dist as usize;
                sub.reps = [dist, sub.reps[0], sub.reps[1], sub.reps[2]];
                sub.last_len = len;
                copy(out, dist, len, phys_target)?;
                return Ok(None);
            }
            match number {
                256 => {
                    let field = bits.peek16();
                    if field & 0x8000 == 0 {
                        let new_table = (field & 0x4000) != 0;
                        bits.skip(2);
                        return Ok(Some(if new_table { None } else { Some(tables.take()) }));
                    }
                    bits.skip(1);
                    tables = read_tables(&mut sub)?;
                }
                257 => read_vm_code(&mut sub, *base_offset + out.len())?,
                258 => {
                    if sub.last_len != 0 {
                        copy(out, sub.reps[0], sub.last_len, phys_target)?;
                    }
                }
                259..=262 => {
                    let n = number - 259;
                    let dist = sub.reps[n];
                    sub.reps.copy_within(0..n, 1);
                    sub.reps[0] = dist;
                    let slot = usize::from(lz.rd.decode(bits)).min(RC - 1);
                    let len = usize::from(LDECODE[slot]) + 2 + bits.take(u32::from(LBITS[slot])) as usize;
                    sub.last_len = len;
                    copy(out, dist, len, phys_target)?;
                }
                _ => {
                    let n = number - 263;
                    let dist = usize::from(SDDECODE[n]) + 1 + bits.take(u32::from(SDBITS[n])) as usize;
                    sub.reps = [dist, sub.reps[0], sub.reps[1], sub.reps[2]];
                    sub.last_len = 2;
                    copy(out, dist, 2, phys_target)?;
                }
            }
            Ok(None)
        })();

        st.addr = sub.bits.addr;
        st.bit = sub.bits.bit;
        st.old_table = sub.old_table;
        st.reps = sub.reps;
        st.last_len = sub.last_len;
        st.low_dist = sub.low_dist;
        st.low_reps = sub.low_reps;
        st.filters = sub.filters;
        st.ppm = sub.ppm;
        st.esc = sub.esc;
        st.tables_left = sub.tables_left;

        if let Some(done_tables) = step_res? {
            return Ok(done_tables);
        }
    }
}

struct State<'a> {
    bits: Bits<'a>,
    /// past here, a cut stream may be reading zeros it doesn't have
    safe: usize,
    old_table: [u8; TABLE],
    reps: [usize; 4],
    last_len: usize,
    low_dist: u32,
    low_reps: u32,
    filters: Filters,
    /// the PPM model, once a PPM block has started one
    ppm: Option<Box<Ppm>>,
    /// in PPM blocks, the byte that says what follows isn't a literal
    esc: u8,
    /// new tables, or a PPM model's start, still allowed: each is work that makes no output, so a stream of
    /// nothing else would run as long as its input
    tables_left: usize,
}

/// Why decoding stopped short of the end.
enum Stop {
    Cut,
}

fn finish_member_tables(st: &State<'_>, tables: Option<Tables>) -> Option<Option<Tables>> {
    if let Some(lz) = &tables {
        let mut peek = Bits {
            data: st.bits.data,
            addr: st.bits.addr,
            bit: st.bits.bit,
        };
        if !peek.past_end() && usize::from(lz.ld.decode(&mut peek)) == 256 {
            let field = peek.peek16();
            if field & 0x8000 == 0 {
                let new_table = (field & 0x4000) != 0;
                return if new_table { None } else { Some(tables) };
            }
            return None;
        }
    }
    Some(tables)
}

enum PpmStep {
    Next,
    /// the PPM block ended: new tables follow
    Tables,
    FileEnd,
}

fn ppm_step_at(
    st: &mut State<'_>,
    out: &mut Vec<u8>,
    target: usize,
    base_offset: usize,
) -> Result<PpmStep, Stop> {
    let ppm = st.ppm.as_mut().ok_or(Stop::Cut)?;
    let bits = &mut st.bits;
    let mut next = || ppm.decode_char(bits).ok_or(Stop::Cut);
    let ch = next()?;
    if ch != st.esc {
        out.push(ch);
        return Ok(PpmStep::Next);
    }
    match next()? {
        0 => return Ok(PpmStep::Tables),
        2 => return Ok(PpmStep::FileEnd),
        3 => {
            let first = next()?;
            let len = match (first & 7) + 1 {
                7 => usize::from(next()?) + 7,
                8 => usize::from(next()?) * 256 + usize::from(next()?),
                n => usize::from(n),
            };
            if len == 0 {
                return Err(Stop::Cut);
            }
            // decoding the program is work too, and none of it reaches the output
            st.filters.work = st.filters.work.checked_sub(len).ok_or(Stop::Cut)?;
            let code = (0..len).map(|_| next()).collect::<Result<Vec<u8>, Stop>>()?;
            add_vm_code(&mut st.filters, u32::from(first), &code, base_offset + out.len())?;
        }
        4 => {
            let mut dist = 0usize;
            for _ in 0..3 {
                dist = (dist << 8) | usize::from(next()?);
            }
            let len = usize::from(next()?);
            copy(out, dist + 2, len + 32, target)?;
        }
        5 => {
            let len = usize::from(next()?);
            copy(out, 1, len + 4, target)?;
        }
        // 1: the escape byte itself
        _ => out.push(ch),
    }
    Ok(PpmStep::Next)
}

fn copy(out: &mut Vec<u8>, dist: usize, len: usize, target: usize) -> Result<(), Stop> {
    if dist == 0 || dist > out.len() {
        return Err(Stop::Cut);
    }
    for _ in 0..len.min(target.saturating_sub(out.len())) {
        out.push(out[out.len() - dist]);
    }
    Ok(())
}

/// The tables for the next block: their code lengths, then each length as a change from the last tables'. None:
/// a PPM block, its model ready.
fn read_tables(st: &mut State<'_>) -> Result<Option<Tables>, Stop> {
    st.tables_left = st.tables_left.checked_sub(1).ok_or(Stop::Cut)?;
    let bits = &mut st.bits;
    bits.align();
    let field = bits.peek16();
    if field & 0x8000 != 0 {
        let ppm = st.ppm.get_or_insert_with(Box::default);
        return if ppm.init(bits, &mut st.esc) { Ok(None) } else { Err(Stop::Cut) };
    }
    st.low_dist = 0;
    st.low_reps = 0;
    if field & 0x4000 == 0 {
        st.old_table = [0; TABLE];
    }
    bits.skip(2);
    let mut lengths = [0u8; BC];
    let mut i = 0;
    while i < BC {
        let l = bits.take(4) as u8;
        if l == 15 {
            let zeros = bits.take(4) as usize;
            if zeros == 0 {
                lengths[i] = 15;
                i += 1;
            } else {
                let end = (i + zeros + 2).min(BC);
                lengths[i..end].fill(0);
                i = end;
            }
        } else {
            lengths[i] = l;
            i += 1;
        }
    }
    let bd = Table::new(&lengths).ok_or(Stop::Cut)?;
    let mut table = [0u8; TABLE];
    let mut i = 0;
    while i < TABLE {
        if bits.past_end() {
            return Err(Stop::Cut);
        }
        let n = bd.decode(bits);
        match n {
            0..=15 => {
                table[i] = (n as u8 + st.old_table[i]) & 15;
                i += 1;
            }
            16 | 17 => {
                let times = if n == 16 { bits.take(3) + 3 } else { bits.take(7) + 11 } as usize;
                let prev = *table.get(i.checked_sub(1).ok_or(Stop::Cut)?).ok_or(Stop::Cut)?;
                let end = (i + times).min(TABLE);
                table[i..end].fill(prev);
                i = end;
            }
            _ => {
                let times = if n == 18 { bits.take(3) + 3 } else { bits.take(7) + 11 } as usize;
                let end = (i + times).min(TABLE);
                table[i..end].fill(0);
                i = end;
            }
        }
    }
    if bits.past_end() {
        return Err(Stop::Cut);
    }
    st.old_table = table;
    let t = |r: std::ops::Range<usize>| Table::new(&table[r]).ok_or(Stop::Cut);
    Ok(Some(Tables {
        ld: t(0..NC)?,
        dd: t(NC..NC + DC)?,
        ldd: t(NC + DC..NC + DC + LDC)?,
        rd: t(NC + DC + LDC..TABLE)?,
    }))
}

/// A number in the VM's code: 4, 8, 16 or 32 bits, or a negative byte.
fn vm_number(bits: &mut Bits<'_>) -> u32 {
    let data = bits.peek16();
    match data & 0xc000 {
        0 => {
            bits.skip(6);
            (data >> 10) & 15
        }
        0x4000 => {
            if data & 0x3c00 == 0 {
                bits.skip(14);
                0xffff_ff00 | ((data >> 2) & 0xff)
            } else {
                bits.skip(10);
                (data >> 6) & 0xff
            }
        }
        0x8000 => {
            bits.skip(2);
            let v = bits.peek16();
            bits.skip(16);
            v
        }
        _ => {
            bits.skip(2);
            let high = bits.peek16() << 16;
            bits.skip(16);
            let v = high | bits.peek16();
            bits.skip(16);
            v
        }
    }
}

fn read_vm_code(st: &mut State<'_>, at: usize) -> Result<(), Stop> {
    let bits = &mut st.bits;
    let first = bits.take(8);
    let mut len = (first & 7) + 1;
    if len == 7 {
        len = bits.take(8) + 7;
    } else if len == 8 {
        len = bits.take(16);
    }
    if len == 0 {
        return Err(Stop::Cut);
    }
    st.filters.work = st.filters.work.checked_sub(len as usize).ok_or(Stop::Cut)?;
    let bits = &mut st.bits;
    let code: Vec<u8> = (0..len).map(|_| bits.take(8) as u8).collect();
    if bits.past_end() {
        return Err(Stop::Cut);
    }
    add_vm_code(&mut st.filters, first, &code, at)
}

fn add_vm_code(f: &mut Filters, first: u32, code: &[u8], at: usize) -> Result<(), Stop> {
    let mut bits = Bits { data: code, addr: 0, bit: 0 };
    let mut pos = if first & 0x80 != 0 {
        let n = vm_number(&mut bits) as usize;
        if n == 0 {
            f.kinds.clear();
            f.old_lengths.clear();
            f.last = 0;
            0
        } else {
            n - 1
        }
    } else {
        f.last
    };
    if pos > f.kinds.len() || pos > f.old_lengths.len() || f.pending.len() >= MAX_FILTERS {
        return Err(Stop::Cut);
    }
    f.last = pos;
    let new = pos == f.kinds.len();
    if new {
        if pos > MAX_FILTERS {
            return Err(Stop::Cut);
        }
        f.kinds.push(Kind::Other);
        f.old_lengths.push(0);
    }
    let mut start = vm_number(&mut bits) as usize;
    if first & 0x40 != 0 {
        start += 258;
    }
    let len = if first & 0x20 != 0 {
        let l = vm_number(&mut bits);
        f.old_lengths[pos] = l;
        l
    } else {
        f.old_lengths[pos]
    };
    let mut regs = [0u32; 7];
    regs[4] = len;
    if first & 0x10 != 0 {
        let mask = bits.peek16() >> 9;
        bits.skip(7);
        for (i, r) in regs.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                *r = vm_number(&mut bits);
            }
        }
    }
    if new {
        let size = vm_number(&mut bits) as usize;
        if size == 0 || size >= 0x10000 || bits.addr + size > code.len() {
            return Err(Stop::Cut);
        }
        let prg: Vec<u8> = (0..size).map(|_| bits.take(8) as u8).collect();
        f.kinds[pos] = kind_of(&prg);
    }
    pos = pos.min(f.kinds.len() - 1);
    f.work = f.work.checked_sub(len as usize).ok_or(Stop::Cut)?;
    f.reach = f.reach.max((at + start).saturating_add(len as usize));
    f.pending.push(Pending { kind: f.kinds[pos], start: at + start, len: len as usize, regs });
    Ok(())
}

fn apply_filters_range(
    window_slice: &[u8],
    pending: &[Pending],
    slice_logical_start: usize,
    file_start: usize,
) -> Vec<u8> {
    let mut out = window_slice.to_vec();
    if pending.is_empty() {
        return out;
    }
    let mut i = 0;
    while i < pending.len() {
        let f = &pending[i];
        let Some(win_rel) = f.start.checked_sub(slice_logical_start) else {
            out.clear();
            break;
        };
        let file_rel = f.start.saturating_sub(file_start);
        let src = win_rel
            .checked_add(f.len)
            .and_then(|end| window_slice.get(win_rel..end));
        let Some(mut data) = src.and_then(|src| filter(f, src.to_vec(), file_rel)) else {
            out.truncate(win_rel.min(out.len()));
            break;
        };
        while let Some(next) = pending
            .get(i + 1)
            .filter(|n| n.start == f.start && n.len == data.len())
        {
            let Some(again) = filter(next, data, file_rel) else {
                out.truncate(win_rel.min(out.len()));
                return out;
            };
            data = again;
            i += 1;
        }
        if data.len() != f.len {
            out.truncate(win_rel.min(out.len()));
            break;
        }
        out[win_rel..win_rel + f.len].copy_from_slice(&data);
        i += 1;
    }
    out
}

/// One standard filter on one block. None: it can't be run here.
fn filter(f: &Pending, mut data: Vec<u8>, rel_start: usize) -> Option<Vec<u8>> {
    let size = f.regs[4] as usize;
    if f.kind == Kind::Other || size != data.len() || size > VM_MEM {
        return None;
    }
    // what the VM's register 6 holds: how much of the file was written before this block
    let offset = rel_start as u32;
    match f.kind {
        Kind::E8 | Kind::E8E9 => {
            if size < 4 {
                return Some(data);
            }
            const FILE_SIZE: u32 = 0x100_0000;
            let mut i = 0usize;
            while i < size - 4 {
                let b = data[i];
                i += 1;
                if b == 0xe8 || (f.kind == Kind::E8E9 && b == 0xe9) {
                    let at = (i as u32).wrapping_add(offset);
                    let addr = u32::from_le_bytes(data[i..i + 4].try_into().ok()?);
                    if addr & 0x8000_0000 != 0 {
                        if addr.wrapping_add(at) & 0x8000_0000 == 0 {
                            data[i..i + 4].copy_from_slice(&addr.wrapping_add(FILE_SIZE).to_le_bytes());
                        }
                    } else if addr.wrapping_sub(FILE_SIZE) & 0x8000_0000 != 0 {
                        data[i..i + 4].copy_from_slice(&addr.wrapping_sub(at).to_le_bytes());
                    }
                    i += 4;
                }
            }
            Some(data)
        }
        Kind::Itanium => {
            if size < 21 {
                return Some(data);
            }
            const MASKS: [u8; 16] = [4, 4, 6, 6, 0, 0, 7, 7, 4, 4, 0, 0, 4, 4, 0, 0];
            let mut file = offset >> 4;
            let mut at = 0;
            while at < size - 21 {
                let b = i32::from(data[at] & 0x1f) - 0x10;
                if b >= 0 {
                    let mask = MASKS[b as usize];
                    for slot in 0..3 {
                        if mask & (1 << slot) != 0 {
                            let start = slot * 41 + 5;
                            if bundle_bits(&data[at..], start + 37, 4) == 5 {
                                let v = bundle_bits(&data[at..], start + 13, 20);
                                set_bundle_bits(&mut data[at..], v.wrapping_sub(file) & 0xfffff, start + 13, 20);
                            }
                        }
                    }
                }
                at += 16;
                file = file.wrapping_add(1);
            }
            Some(data)
        }
        Kind::Delta => {
            let channels = f.regs[0] as usize;
            if size > VM_MEM / 2 || channels == 0 || channels > 1024 {
                return Some(data);
            }
            let mut out = vec![0u8; size];
            let mut src = 0;
            for ch in 0..channels {
                let mut prev = 0u8;
                let mut d = ch;
                while d < size {
                    prev = prev.wrapping_sub(data[src]);
                    out[d] = prev;
                    src += 1;
                    d += channels;
                }
            }
            Some(out)
        }
        Kind::Rgb => {
            let width = (f.regs[0] as usize).wrapping_sub(3);
            let pos_r = f.regs[1] as usize;
            if !(3..=VM_MEM / 2).contains(&size) || width > size || pos_r > 2 {
                return Some(data);
            }
            let mut out = vec![0u8; size];
            let mut src = 0;
            for ch in 0..3 {
                let mut prev = 0u32;
                let mut i = ch;
                while i < size {
                    let predicted = if i >= width + 3 {
                        let upper = u32::from(out[i - width]);
                        let upper_left = u32::from(out[i - width - 3]);
                        let p = prev.wrapping_add(upper).wrapping_sub(upper_left);
                        let pa = (p.wrapping_sub(prev) as i32).unsigned_abs();
                        let pb = (p.wrapping_sub(upper) as i32).unsigned_abs();
                        let pc = (p.wrapping_sub(upper_left) as i32).unsigned_abs();
                        if pa <= pb && pa <= pc {
                            prev
                        } else if pb <= pc {
                            upper
                        } else {
                            upper_left
                        }
                    } else {
                        prev
                    };
                    let b = (predicted as u8).wrapping_sub(data[src]);
                    src += 1;
                    out[i] = b;
                    prev = u32::from(b);
                    i += 3;
                }
            }
            let mut i = pos_r;
            while i + 2 < size {
                let g = out[i + 1];
                out[i] = out[i].wrapping_add(g);
                out[i + 2] = out[i + 2].wrapping_add(g);
                i += 3;
            }
            Some(out)
        }
        Kind::Audio => {
            let channels = f.regs[0] as usize;
            if size > VM_MEM / 2 || channels == 0 || channels > 128 {
                return Some(data);
            }
            let mut out = vec![0u8; size];
            let mut src = 0;
            for ch in 0..channels {
                let (mut prev_byte, mut prev_delta) = (0u32, 0i32);
                let mut dif = [0u32; 7];
                let (mut d1, mut d2) = (0i32, 0i32);
                let (mut k1, mut k2, mut k3) = (0i32, 0i32, 0i32);
                let mut i = ch;
                let mut count = 0u32;
                while i < size {
                    let d3 = d2;
                    d2 = prev_delta - d1;
                    d1 = prev_delta;
                    let predicted = (8 * prev_byte as i32 + k1 * d1 + k2 * d2 + k3 * d3) as u32;
                    let predicted = (predicted >> 3) & 0xff;
                    let cur = data[src];
                    src += 1;
                    let b = predicted.wrapping_sub(u32::from(cur)) as u8;
                    out[i] = b;
                    prev_delta = i32::from(b.wrapping_sub(prev_byte as u8) as i8);
                    prev_byte = u32::from(b);
                    let d = i32::from(cur as i8) << 3;
                    dif[0] += d.unsigned_abs();
                    dif[1] += (d - d1).unsigned_abs();
                    dif[2] += (d + d1).unsigned_abs();
                    dif[3] += (d - d2).unsigned_abs();
                    dif[4] += (d + d2).unsigned_abs();
                    dif[5] += (d - d3).unsigned_abs();
                    dif[6] += (d + d3).unsigned_abs();
                    if count & 0x1f == 0 {
                        adapt(&mut dif, &mut [&mut k1, &mut k2, &mut k3]);
                    }
                    count += 1;
                    i += channels;
                }
            }
            Some(out)
        }
        Kind::Other => None,
    }
}

/// Every 32 bytes the audio predictors lean toward whichever coefficient would have erred least.
pub(super) fn adapt(dif: &mut [u32], k: &mut [&mut i32]) {
    let mut min = dif[0];
    let mut which = 0;
    dif[0] = 0;
    for (j, d) in dif.iter_mut().enumerate().skip(1) {
        if *d < min {
            min = *d;
            which = j;
        }
        *d = 0;
    }
    if which > 0 {
        let c = &mut *k[(which - 1) / 2];
        if which % 2 == 1 {
            if *c >= -16 {
                *c -= 1;
            }
        } else if *c < 16 {
            *c += 1;
        }
    }
}

fn bundle_bits(data: &[u8], pos: usize, count: u32) -> u32 {
    let at = pos / 8;
    let v = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) >> (pos & 7);
    v & (u32::MAX >> (32 - count))
}

fn set_bundle_bits(data: &mut [u8], value: u32, pos: usize, count: u32) {
    let at = pos / 8;
    let bit = pos & 7;
    let mut and = !((u32::MAX >> (32 - count)) << bit);
    let mut value = value << bit;
    for b in &mut data[at..at + 4] {
        *b = (u32::from(*b) & and) as u8 | value as u8;
        and = (and >> 8) | 0xff00_0000;
        value >>= 8;
    }
}
