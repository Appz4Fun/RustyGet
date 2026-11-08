//! LZMA and LZMA2, decoded from the front only: as much output as the bytes read so far give, up to a limit.
//! Ours rather than a crate's, because the probe holds just a prefix of the stream, and a decoder that gives up on
//! a cut-off stream without handing back what it had decoded is no use for that.

/// lc, lp and pb, the literal context, literal position and position bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Props {
    pub lc: u32,
    pub lp: u32,
    pub pb: u32,
}

impl Props {
    /// The one byte form, `(pb * 5 + lp) * 9 + lc`.
    pub fn from_byte(b: u8) -> Option<Props> {
        if b >= 9 * 5 * 5 {
            return None;
        }
        let b = u32::from(b);
        Some(Props { lc: b % 9, lp: (b / 9) % 5, pb: b / 45 })
    }
}

/// How a decode ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The stream said it ended, by an end marker, its size, or LZMA2's last chunk.
    Done,
    /// The output reached the limit asked for.
    Limit,
    /// The input ran out, or what came next made no sense: the output so far still stands.
    Cut,
}

enum RcSource<'a> {
    Slice { data: &'a [u8], pos: usize },
    Reader {
        reader: &'a mut dyn std::io::Read,
        buf: Box<[u8; 8192]>,
        pos: usize,
        cap: usize,
    },
}

impl RcSource<'_> {
    #[inline]
    fn next_byte(&mut self) -> Option<u8> {
        match self {
            RcSource::Slice { data, pos } => {
                let b = *data.get(*pos)?;
                *pos += 1;
                Some(b)
            }
            RcSource::Reader {
                reader,
                buf,
                pos,
                cap,
            } => {
                if *pos >= *cap {
                    *pos = 0;
                    loop {
                        match reader.read(&mut buf[..]) {
                            Ok(0) => return None,
                            Ok(n) => {
                                *cap = n;
                                break;
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(_) => return None,
                        }
                    }
                }
                let b = buf[*pos];
                *pos += 1;
                Some(b)
            }
        }
    }
}

struct Rc<'a> {
    src: RcSource<'a>,
    range: u32,
    code: u32,
}

impl<'a> Rc<'a> {
    fn new(data: &'a [u8]) -> Option<Rc<'a>> {
        // the first byte is always zero
        if *data.first()? != 0 {
            return None;
        }
        let code = u32::from_be_bytes(data.get(1..5)?.try_into().ok()?);
        Some(Rc {
            src: RcSource::Slice { data, pos: 5 },
            range: u32::MAX,
            code,
        })
    }

    fn from_reader(reader: &'a mut dyn std::io::Read) -> Option<Rc<'a>> {
        let mut src = RcSource::Reader {
            reader,
            buf: Box::new([0u8; 8192]),
            pos: 0,
            cap: 0,
        };
        if src.next_byte()? != 0 {
            return None;
        }
        let init = [
            src.next_byte()?,
            src.next_byte()?,
            src.next_byte()?,
            src.next_byte()?,
        ];
        Some(Rc {
            src,
            range: u32::MAX,
            code: u32::from_be_bytes(init),
        })
    }

    fn normalize(&mut self) -> Option<()> {
        if self.range < 1 << 24 {
            let b = self.src.next_byte()?;
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(b);
        }
        Some(())
    }

    fn bit(&mut self, prob: &mut u16) -> Option<u32> {
        let bound = (self.range >> 11) * u32::from(*prob);
        let bit = if self.code < bound {
            self.range = bound;
            *prob += (2048 - *prob) >> 5;
            0
        } else {
            self.range -= bound;
            self.code -= bound;
            *prob -= *prob >> 5;
            1
        };
        self.normalize()?;
        Some(bit)
    }

    fn direct(&mut self, count: u32) -> Option<u32> {
        let mut out = 0u32;
        for _ in 0..count {
            self.range >>= 1;
            let bit = if self.code >= self.range {
                self.code -= self.range;
                1
            } else {
                0
            };
            out = (out << 1) | bit;
            self.normalize()?;
        }
        Some(out)
    }

    fn tree(&mut self, probs: &mut [u16], bits: u32) -> Option<u32> {
        let mut m = 1usize;
        for _ in 0..bits {
            m = (m << 1) | self.bit(probs.get_mut(m)?)? as usize;
        }
        Some(m as u32 - (1 << bits))
    }

    fn reverse(&mut self, probs: &mut [u16], bits: u32) -> Option<u32> {
        let mut m = 1usize;
        let mut out = 0;
        for i in 0..bits {
            let bit = self.bit(probs.get_mut(m)?)?;
            m = (m << 1) | bit as usize;
            out |= bit << i;
        }
        Some(out)
    }
}

const INIT: u16 = 1024;

struct LenDecoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 16],
    mid: [[u16; 8]; 16],
    high: [u16; 256],
}

impl LenDecoder {
    fn new() -> LenDecoder {
        LenDecoder { choice: INIT, choice2: INIT, low: [[INIT; 8]; 16], mid: [[INIT; 8]; 16], high: [INIT; 256] }
    }

    fn decode(&mut self, rc: &mut Rc<'_>, pos_state: usize) -> Option<u32> {
        if rc.bit(&mut self.choice)? == 0 {
            return rc.tree(&mut self.low[pos_state], 3);
        }
        if rc.bit(&mut self.choice2)? == 0 {
            return Some(8 + rc.tree(&mut self.mid[pos_state], 3)?);
        }
        Some(16 + rc.tree(&mut self.high, 8)?)
    }
}

/// An LZMA decoder's state, which LZMA2 keeps from one chunk to the next.
pub struct Decoder {
    props: Props,
    literal: Vec<u16>,
    is_match: [u16; 192],
    is_rep: [u16; 12],
    is_rep0: [u16; 12],
    is_rep1: [u16; 12],
    is_rep2: [u16; 12],
    is_rep0_long: [u16; 192],
    pos_slot: [[u16; 64]; 4],
    pos_special: [u16; 115],
    align: [u16; 16],
    len: LenDecoder,
    rep_len: LenDecoder,
    state: usize,
    reps: [u32; 4],
}

impl Decoder {
    pub fn new(props: Props) -> Decoder {
        Decoder {
            props,
            literal: vec![INIT; 0x300 << (props.lc + props.lp)],
            is_match: [INIT; 192],
            is_rep: [INIT; 12],
            is_rep0: [INIT; 12],
            is_rep1: [INIT; 12],
            is_rep2: [INIT; 12],
            is_rep0_long: [INIT; 192],
            pos_slot: [[INIT; 64]; 4],
            pos_special: [INIT; 115],
            align: [INIT; 16],
            len: LenDecoder::new(),
            rep_len: LenDecoder::new(),
            state: 0,
            reps: [0; 4],
        }
    }

    fn distance(&mut self, rc: &mut Rc<'_>, len: u32) -> Option<u32> {
        let slot = rc.tree(&mut self.pos_slot[len.min(3) as usize], 6)?;
        if slot < 4 {
            return Some(slot);
        }
        let direct = (slot >> 1) - 1;
        let mut dist = (2 | (slot & 1)) << direct;
        if slot < 14 {
            let base = (dist - slot) as usize;
            dist += rc.reverse(self.pos_special.get_mut(base..)?, direct)?;
        } else {
            dist += rc.direct(direct - 4)? << 4;
            dist += rc.reverse(&mut self.align, 4)?;
        }
        Some(dist)
    }

    /// Decodes into `out` until it holds `target` bytes, the stream's end marker, or `limit`. Distances reach back
    /// no further than `dict_start`, where LZMA2 last emptied the dictionary.
    fn run(&mut self, rc: &mut Rc<'_>, out: &mut Vec<u8>, dict_start: usize, target: usize, limit: usize) -> End {
        let stop = target.min(limit);
        let pb_mask = (1usize << self.props.pb) - 1;
        let lp_mask = (1usize << self.props.lp) - 1;
        let lc = self.props.lc;
        while out.len() < stop {
            match self.one(rc, out, dict_start, stop, pb_mask, lp_mask, lc) {
                Some(true) => {}
                Some(false) => return End::Done,
                None => return End::Cut,
            }
        }
        if out.len() >= target { End::Done } else { End::Limit }
    }

    /// One literal or match. Some(false): the end marker.
    #[allow(clippy::too_many_arguments)]
    fn one(
        &mut self,
        rc: &mut Rc<'_>,
        out: &mut Vec<u8>,
        dict_start: usize,
        stop: usize,
        pb_mask: usize,
        lp_mask: usize,
        lc: u32,
    ) -> Option<bool> {
        let pos = out.len() - dict_start;
        let pos_state = pos & pb_mask;
        let s = self.state;
        if rc.bit(&mut self.is_match[(s << 4) + pos_state])? == 0 {
            let prev = if pos > 0 { u32::from(out[out.len() - 1]) } else { 0 };
            let lit_state = ((pos & lp_mask) << lc) + (prev >> (8 - lc)) as usize;
            let probs = self.literal.get_mut(0x300 * lit_state..0x300 * (lit_state + 1))?;
            let mut symbol = 1usize;
            if s >= 7 {
                let back = self.reps[0] as usize + 1;
                let mut match_byte = if back <= pos { u32::from(out[out.len() - back]) } else { 0 };
                while symbol < 0x100 {
                    let match_bit = ((match_byte >> 7) & 1) as usize;
                    match_byte <<= 1;
                    let bit = rc.bit(&mut probs[((1 + match_bit) << 8) + symbol])? as usize;
                    symbol = (symbol << 1) | bit;
                    if match_bit != bit {
                        break;
                    }
                }
            }
            while symbol < 0x100 {
                symbol = (symbol << 1) | rc.bit(&mut probs[symbol])? as usize;
            }
            out.push(symbol as u8);
            self.state = if s < 4 {
                0
            } else if s < 10 {
                s - 3
            } else {
                s - 6
            };
            return Some(true);
        }
        let len;
        if rc.bit(&mut self.is_rep[s])? == 0 {
            len = self.len.decode(rc, pos_state)?;
            self.state = if s < 7 { 7 } else { 10 };
            let dist = self.distance(rc, len)?;
            if dist == u32::MAX {
                return Some(false);
            }
            self.reps = [dist, self.reps[0], self.reps[1], self.reps[2]];
        } else {
            if rc.bit(&mut self.is_rep0[s])? == 0 {
                if rc.bit(&mut self.is_rep0_long[(s << 4) + pos_state])? == 0 {
                    self.state = if s < 7 { 9 } else { 11 };
                    copy(out, dict_start, self.reps[0], 1)?;
                    return Some(true);
                }
            } else {
                let dist;
                if rc.bit(&mut self.is_rep1[s])? == 0 {
                    dist = self.reps[1];
                } else {
                    if rc.bit(&mut self.is_rep2[s])? == 0 {
                        dist = self.reps[2];
                    } else {
                        dist = self.reps[3];
                        self.reps[3] = self.reps[2];
                    }
                    self.reps[2] = self.reps[1];
                }
                self.reps[1] = self.reps[0];
                self.reps[0] = dist;
            }
            len = self.rep_len.decode(rc, pos_state)?;
            self.state = if s < 7 { 8 } else { 11 };
        }
        let want = (len as usize + 2).min(stop - out.len());
        copy(out, dict_start, self.reps[0], want)?;
        Some(true)
    }
}

/// `count` bytes from `dist + 1` back. None: further back than the dictionary holds, a broken stream.
fn copy(out: &mut Vec<u8>, dict_start: usize, dist: u32, count: usize) -> Option<()> {
    let back = dist as usize + 1;
    if back > out.len() - dict_start {
        return None;
    }
    for _ in 0..count {
        out.push(out[out.len() - back]);
    }
    Some(())
}

/// A raw LZMA stream, as 7z, zip, `.lzma` and lzip hold it. `size` is the unpacked size, when known.
pub fn lzma(data: &[u8], props: Props, size: Option<u64>, limit: usize) -> (Vec<u8>, End) {
    let mut out = Vec::new();
    let Some(mut rc) = Rc::new(data) else { return (out, End::Cut) };
    let target = size.and_then(|s| usize::try_from(s).ok()).unwrap_or(usize::MAX);
    if props.lc + props.lp > 12 {
        return (out, End::Cut);
    }
    let mut dec = Decoder::new(props);
    let end = dec.run(&mut rc, &mut out, 0, target, limit);
    (out, end)
}

/// An LZMA2 stream, its chunks one after another.
pub fn lzma2(data: &[u8], limit: usize) -> (Vec<u8>, End) {
    let mut out = Vec::new();
    let mut dec: Option<Decoder> = None;
    let mut dict_start = 0;
    let mut p = 0usize;
    loop {
        if out.len() >= limit {
            out.truncate(limit);
            return (out, End::Limit);
        }
        let Some(&control) = data.get(p) else { return (out, End::Cut) };
        p += 1;
        match control {
            0 => return (out, End::Done),
            1 | 2 => {
                let Some(size) = be16(data, p) else { return (out, End::Cut) };
                p += 2;
                if control == 1 {
                    dict_start = out.len();
                }
                let size = usize::from(size) + 1;
                let take = size.min(data.len() - p).min(limit - out.len());
                out.extend_from_slice(&data[p..p + take]);
                if take < size {
                    let end = if out.len() >= limit { End::Limit } else { End::Cut };
                    return (out, end);
                }
                p += size;
            }
            0x80..=0xff => {
                let (Some(low), Some(packed)) = (be16(data, p), be16(data, p + 2)) else { return (out, End::Cut) };
                p += 4;
                let unpacked = ((usize::from(control & 0x1f)) << 16) + usize::from(low) + 1;
                let packed = usize::from(packed) + 1;
                let mode = (control >> 5) & 3;
                if mode >= 2 {
                    let Some(props) = data.get(p).and_then(|b| Props::from_byte(*b)) else { return (out, End::Cut) };
                    p += 1;
                    if props.lc + props.lp > 4 {
                        return (out, End::Cut);
                    }
                    dec = Some(Decoder::new(props));
                } else if mode == 1 {
                    let Some(props) = dec.as_ref().map(|d| d.props) else { return (out, End::Cut) };
                    dec = Some(Decoder::new(props));
                }
                if mode == 3 {
                    dict_start = out.len();
                }
                let Some(d) = dec.as_mut() else { return (out, End::Cut) };
                let chunk = &data[p.min(data.len())..(p + packed).min(data.len())];
                let Some(mut rc) = Rc::new(chunk) else { return (out, End::Cut) };
                let target = out.len() + unpacked;
                match d.run(&mut rc, &mut out, dict_start, target, limit) {
                    End::Done if out.len() == target => {}
                    End::Limit => return (out, End::Limit),
                    _ => return (out, End::Cut),
                }
                p += packed;
            }
            _ => return (out, End::Cut),
        }
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// Decodes a raw LZMA stream from `reader`, emitting decoded chunks via `emit` and keeping at most
/// `dict_cap` bytes of dictionary history in memory.
pub fn lzma_from_reader<R: std::io::Read>(
    reader: &mut R,
    props: Props,
    dict_cap: usize,
    size: Option<u64>,
    emit: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if props.lc + props.lp > 12 {
        return Err("invalid LZMA lc+lp".to_string());
    }
    let dict_cap = dict_cap.max(64 * 1024);
    let mut rc =
        Rc::from_reader(reader).ok_or_else(|| "corrupt LZMA range coder init".to_string())?;
    let mut dec = Decoder::new(props);
    let mut out = Vec::new();
    let mut total_emitted = 0u64;
    let target_total = size.unwrap_or(u64::MAX);
    let pb_mask = (1usize << props.pb) - 1;
    let lp_mask = (1usize << props.lp) - 1;
    let lc = props.lc;

    while total_emitted < target_total {
        let prev_len = out.len();
        let mut reached_eos = false;
        while out.len() - prev_len < 64 * 1024
            && total_emitted.saturating_add((out.len() - prev_len) as u64) < target_total
        {
            let rem_to_target =
                target_total.saturating_sub(total_emitted + (out.len() - prev_len) as u64);
            let step_stop = if size.is_some() && rem_to_target <= 273 {
                out.len().saturating_add(rem_to_target as usize)
            } else {
                usize::MAX
            };
            match dec.one(&mut rc, &mut out, 0, step_stop, pb_mask, lp_mask, lc) {
                Some(true) => {}
                Some(false) => {
                    reached_eos = true;
                    break;
                }
                None => return Err("corrupt or truncated LZMA stream".to_string()),
            }
        }
        let made = out.len() - prev_len;
        if made > 0 {
            emit(&out[prev_len..])?;
            total_emitted += made as u64;
        }
        if reached_eos {
            if let Some(expected) = size {
                if total_emitted != expected {
                    return Err("LZMA stream ended before expected size".to_string());
                }
            }
            return Ok(());
        }
        if out.len() > dict_cap.saturating_add(2 << 20) {
            let drop_n = (out.len() - dict_cap) & !0xffff;
            if drop_n > 0 {
                out.drain(..drop_n);
            }
        }
    }
    Ok(())
}

/// Decodes an LZMA2 stream from `reader`, emitting decoded chunks via `emit` and keeping at most
/// `dict_cap` bytes of dictionary history in memory. Returns the total compressed bytes read
/// (including the terminating `0x00` control byte).
pub fn lzma2_from_reader<R: std::io::Read>(
    reader: &mut R,
    dict_cap: usize,
    emit: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<u64, String> {
    let dict_cap = dict_cap.max(64 * 1024);
    let mut out = Vec::new();
    let mut dec: Option<Decoder> = None;
    let mut chunk_buf = Vec::new();
    let mut comp_bytes = 0u64;

    loop {
        let mut ctrl = [0u8; 1];
        reader
            .read_exact(&mut ctrl)
            .map_err(|e| format!("corrupt LZMA2 stream: {e}"))?;
        comp_bytes += 1;
        let control = ctrl[0];
        match control {
            0 => return Ok(comp_bytes),
            1 | 2 => {
                let mut sz_bytes = [0u8; 2];
                reader
                    .read_exact(&mut sz_bytes)
                    .map_err(|e| format!("corrupt LZMA2 uncompressed chunk header: {e}"))?;
                comp_bytes += 2;
                let size = usize::from(u16::from_be_bytes(sz_bytes)) + 1;
                if control == 1 {
                    out.clear();
                }
                let prev_len = out.len();
                out.resize(prev_len + size, 0);
                reader
                    .read_exact(&mut out[prev_len..])
                    .map_err(|e| format!("corrupt LZMA2 uncompressed chunk data: {e}"))?;
                comp_bytes += size as u64;
                emit(&out[prev_len..])?;
            }
            0x80..=0xff => {
                let mut hdr = [0u8; 4];
                reader
                    .read_exact(&mut hdr)
                    .map_err(|e| format!("corrupt LZMA2 compressed chunk header: {e}"))?;
                comp_bytes += 4;
                let low = u16::from_be_bytes([hdr[0], hdr[1]]);
                let packed = usize::from(u16::from_be_bytes([hdr[2], hdr[3]])) + 1;
                let unpacked = ((usize::from(control & 0x1f)) << 16) + usize::from(low) + 1;
                let mode = (control >> 5) & 3;
                if mode >= 2 {
                    let mut pb = [0u8; 1];
                    reader
                        .read_exact(&mut pb)
                        .map_err(|e| format!("corrupt LZMA2 properties byte: {e}"))?;
                    comp_bytes += 1;
                    let props = Props::from_byte(pb[0])
                        .ok_or_else(|| "invalid LZMA2 properties".to_string())?;
                    if props.lc + props.lp > 4 {
                        return Err("invalid LZMA2 lc+lp".to_string());
                    }
                    dec = Some(Decoder::new(props));
                } else if mode == 1 {
                    let props = dec
                        .as_ref()
                        .map(|d| d.props)
                        .ok_or_else(|| "missing prior LZMA2 properties".to_string())?;
                    dec = Some(Decoder::new(props));
                }
                if mode == 3 {
                    out.clear();
                }
                let d = dec
                    .as_mut()
                    .ok_or_else(|| "uninitialized LZMA2 decoder state".to_string())?;
                chunk_buf.resize(packed, 0);
                reader
                    .read_exact(&mut chunk_buf)
                    .map_err(|e| format!("corrupt LZMA2 chunk payload: {e}"))?;
                comp_bytes += packed as u64;
                let mut rc =
                    Rc::new(&chunk_buf).ok_or_else(|| "corrupt LZMA2 range coder init".to_string())?;
                let prev_len = out.len();
                let target = prev_len + unpacked;
                match d.run(&mut rc, &mut out, 0, target, target) {
                    End::Done if out.len() == target => {}
                    _ => return Err("corrupt LZMA2 compressed chunk".to_string()),
                }
                emit(&out[prev_len..])?;
            }
            _ => return Err("invalid LZMA2 control byte".to_string()),
        }
        if out.len() > dict_cap.saturating_add(2 << 20) {
            let drop_n = (out.len() - dict_cap) & !0xffff;
            if drop_n > 0 {
                out.drain(..drop_n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk_never_panics() {
        let mut seed = 0x1234_5678u32;
        for n in 0..2000 {
            let data: Vec<u8> = (0..(n % 300))
                .map(|i| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    if i == 0 { 0 } else { (seed >> 16) as u8 }
                })
                .collect();
            let _ = lzma(&data, Props { lc: 3, lp: 0, pb: 2 }, None, 1 << 16);
            let _ = lzma2(&data, 1 << 16);
        }
    }

    #[test]
    fn lzma_from_reader_preserves_matches_crossing_chunk_boundaries() {
        let raw: &[u8] = &[
            0, 32, 239, 251, 191, 254, 163, 177, 94, 229, 248, 63, 178, 170, 38, 85, 248, 104,
            112, 65, 112, 21, 15, 141, 253, 30, 76, 27, 138, 66, 183, 25, 244, 105, 24, 113, 174,
            102, 35, 138, 138, 77, 47, 163, 13, 217, 127, 166, 227, 140, 35, 17, 83, 224, 89, 24,
            197, 117, 138, 226, 119, 248, 182, 148, 127, 12, 106, 192, 222, 116, 73, 100, 226,
            233, 92, 83, 178, 4, 216, 247, 68, 12, 171, 95, 13, 109, 70, 233, 229, 195, 118, 136,
            183, 127, 63, 238, 38, 207, 255, 255, 250, 63, 0, 0,
        ];
        let props = Props::from_byte(93).unwrap();
        let (expected, end) = lzma(raw, props, Some(200_000), 200_000);
        assert_eq!(end, End::Done);
        assert_eq!(expected.len(), 200_000);
        assert!(expected.iter().all(|&b| b == b'A'));

        for size_opt in [Some(200_000u64), None] {
            let mut cursor = std::io::Cursor::new(raw);
            let mut decoded = Vec::new();
            lzma_from_reader(&mut cursor, props, 64 * 1024, size_opt, &mut |chunk| {
                decoded.extend_from_slice(chunk);
                Ok(())
            })
            .unwrap();
            assert_eq!(decoded, expected);
        }
    }
}
