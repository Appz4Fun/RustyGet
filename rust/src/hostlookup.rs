//! `hostlookup` RPC: the addresses a news server name resolves to, where each
//! address is (country and network, from the bundled DB-IP Lite databases) and
//! how long a TCP connect to it takes. Nothing leaves the machine except the DNS
//! query and the connect itself; the location data is read from local files.

use std::fmt::Write as _;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// File names of the free DB-IP Lite databases (CC BY 4.0, https://db-ip.com).
pub const COUNTRY_DB: &str = "dbip-country-lite.mmdb";
pub const ASN_DB: &str = "dbip-asn-lite.mmdb";
const MAX_ADDRESSES: usize = 8;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Connect-timing threads all lookups may use at once.
const LOOKUP_THREADS: usize = 32;
static LOOKUP_THREADS_IN_USE: AtomicUsize = AtomicUsize::new(0);

/// One resolved address and what is known about it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Address {
    pub ip: String,
    pub family: u8,
    pub country: String,
    pub country_name: String,
    pub asn: u32,
    pub org: String,
    pub latency_ms: Option<f64>,
}

/// Folders searched for the databases, first match wins.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("RUSTYGET_GEOIP_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin) = exe.parent() {
            dirs.push(bin.join("geoip"));
            dirs.push(bin.join("../share/rustyget/geoip"));
        }
    }
    dirs.push(PathBuf::from("/usr/share/rustyget/geoip"));
    dirs.push(PathBuf::from("/var/lib/rustyget/geoip"));
    dirs
}

fn open_db(name: &str) -> Option<mmdb::Reader> {
    search_dirs()
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .and_then(|path| mmdb::Reader::open(&path).ok())
}

fn databases() -> &'static (Option<mmdb::Reader>, Option<mmdb::Reader>) {
    static DBS: OnceLock<(Option<mmdb::Reader>, Option<mmdb::Reader>)> = OnceLock::new();
    DBS.get_or_init(|| (open_db(COUNTRY_DB), open_db(ASN_DB)))
}

/// Fills country and network fields from the databases that are present.
pub fn locate(address: &mut Address, country: Option<&mmdb::Reader>, asn: Option<&mmdb::Reader>) {
    let Ok(ip) = address.ip.parse::<IpAddr>() else {
        return;
    };
    if let Some(Some(record)) = country.map(|db| db.lookup(ip)) {
        if let Some(code) = record
            .path(&["country", "iso_code"])
            .and_then(mmdb::Value::as_str)
        {
            address.country = code.to_owned();
        }
        if let Some(name) = record
            .path(&["country", "names", "en"])
            .and_then(mmdb::Value::as_str)
        {
            address.country_name = name.to_owned();
        }
    }
    if let Some(Some(record)) = asn.map(|db| db.lookup(ip)) {
        if let Some(number) = record
            .path(&["autonomous_system_number"])
            .and_then(mmdb::Value::as_u64)
        {
            address.asn = u32::try_from(number).unwrap_or(0);
        }
        if let Some(org) = record
            .path(&["autonomous_system_organization"])
            .and_then(mmdb::Value::as_str)
        {
            address.org = org.to_owned();
        }
    }
}

/// Resolves `host` (a name or a literal address), at most `MAX_ADDRESSES`, IPv4 first.
/// An IPv6 link-local address keeps its interface (`fe80::1%eth0`), for timing and as Host.
pub fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let ips = pick_addresses(resolve_all(host, port)?);
    if ips.is_empty() {
        return Err(format!("{} has no addresses", host.trim()));
    }
    Ok(ips)
}

/// Every address of `host`, without duplicates, in the resolver's order.
pub fn resolve_all(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if host.is_empty()
        || host.len() > 253
        || host.bytes().any(|b| b.is_ascii_whitespace() || b == b'/')
    {
        return Err("Enter a host name".into());
    }
    let found = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("Could not resolve {host}: {e}"))?;
    let mut ips: Vec<SocketAddr> = Vec::new();
    for addr in found {
        if !ips.iter().any(|seen| shown_address(seen) == shown_address(&addr)) {
            ips.push(addr);
        }
    }
    Ok(ips)
}

/// At most `MAX_ADDRESSES`, IPv4 first, keeping room for both kinds when there are both:
/// a server set to IPv6 only still has its IPv6 addresses to pick from.
pub fn pick_addresses(ips: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let (v4, v6): (Vec<SocketAddr>, Vec<SocketAddr>) = ips.into_iter().partition(SocketAddr::is_ipv4);
    let v6_room = v6.len().min(MAX_ADDRESSES / 2).max(MAX_ADDRESSES.saturating_sub(v4.len()));
    let v4_take = v4.len().min(MAX_ADDRESSES - v6_room.min(v6.len()));
    let mut out: Vec<SocketAddr> = v4.into_iter().take(v4_take).collect();
    let left = MAX_ADDRESSES - out.len();
    out.extend(v6.into_iter().take(left));
    out
}

/// The address as Host takes it: an IPv6 address with a scope gets `%interface`.
pub fn shown_address(addr: &SocketAddr) -> String {
    match addr {
        SocketAddr::V6(v6) if v6.scope_id() != 0 => {
            let mut name = [0 as libc::c_char; libc::IF_NAMESIZE];
            // SAFETY: the buffer is IF_NAMESIZE long, as if_indextoname requires
            let found = unsafe { !libc::if_indextoname(v6.scope_id(), name.as_mut_ptr()).is_null() };
            let scope = if found {
                // SAFETY: if_indextoname wrote a NUL-terminated name into the buffer
                unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned()
            } else {
                v6.scope_id().to_string()
            };
            format!("{}%{scope}", v6.ip())
        }
        _ => addr.ip().to_string(),
    }
}

/// Time to open a TCP connection, in milliseconds; `None` when it fails or times out.
pub fn connect_ms(addr: SocketAddr, timeout: Duration) -> Option<f64> {
    let start = Instant::now();
    TcpStream::connect_timeout(&addr, timeout)
        .ok()
        .map(|_| (start.elapsed().as_secs_f64() * 10000.0).round() / 10.0)
}

fn quote(out: &mut String, value: &str) {
    let mut escaped = Vec::new();
    crate::escape::json_encode(value.as_bytes(), &mut escaped);
    out.push('"');
    out.push_str(&String::from_utf8_lossy(&escaped));
    out.push('"');
}

/// JSON reply: `{Host, Port, Error, GeoIP, Addresses: [...]}`.
pub fn to_json(host: &str, port: u16, error: &str, geoip: bool, addresses: &[Address]) -> String {
    let mut out = String::from("{\"Host\":");
    quote(&mut out, host);
    let _ = write!(out, ",\"Port\":{port},\"Error\":");
    quote(&mut out, error);
    let _ = write!(out, ",\"GeoIP\":{geoip},\"Addresses\":[");
    for (i, a) in addresses.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"IP\":");
        quote(&mut out, &a.ip);
        let _ = write!(out, ",\"Family\":{},\"Country\":", a.family);
        quote(&mut out, &a.country);
        out.push_str(",\"CountryName\":");
        quote(&mut out, &a.country_name);
        let _ = write!(out, ",\"ASN\":{},\"Org\":", a.asn);
        quote(&mut out, &a.org);
        match a.latency_ms {
            Some(ms) => {
                let _ = write!(out, ",\"LatencyMs\":{ms}}}");
            }
            None => out.push_str(",\"LatencyMs\":null}"),
        }
    }
    out.push_str("]}");
    out
}

/// Looks the host up: resolve, locate, and time a connect to each address in parallel.
pub fn lookup(host: &str, port: i32) -> String {
    let port = u16::try_from(port).ok().filter(|p| *p > 0).unwrap_or(563);
    let (country, asn) = databases();
    let geoip = country.is_some() || asn.is_some();
    let ips = match resolve(host, port) {
        Ok(ips) => ips,
        Err(error) => return to_json(host, port, &error, geoip, &[]),
    };
    let measure = |ip: SocketAddr| {
        let mut address = Address {
            ip: shown_address(&ip),
            family: if ip.is_ipv6() { 6 } else { 4 },
            latency_ms: connect_ms(ip, CONNECT_TIMEOUT),
            ..Address::default()
        };
        locate(&mut address, country.as_ref(), asn.as_ref());
        address
    };
    let measure = &measure;
    // Each address is timed on its own thread, up to LOOKUP_THREADS across all lookups at
    // once; past that, or if a thread can't be made, it is timed here instead, so a busy
    // daemon answers slower rather than aborting.
    let addresses: Vec<Address> = std::thread::scope(|scope| {
        let mut slots: Vec<Result<std::thread::ScopedJoinHandle<'_, Address>, SocketAddr>> = Vec::new();
        for ip in ips.iter().copied() {
            if LOOKUP_THREADS_IN_USE.fetch_add(1, Ordering::SeqCst) >= LOOKUP_THREADS {
                LOOKUP_THREADS_IN_USE.fetch_sub(1, Ordering::SeqCst);
                slots.push(Err(ip));
                continue;
            }
            let spawned = std::thread::Builder::new().name("hostlookup".into()).spawn_scoped(scope, move || {
                let address = measure(ip);
                LOOKUP_THREADS_IN_USE.fetch_sub(1, Ordering::SeqCst);
                address
            });
            match spawned {
                Ok(handle) => slots.push(Ok(handle)),
                Err(_) => {
                    LOOKUP_THREADS_IN_USE.fetch_sub(1, Ordering::SeqCst);
                    slots.push(Err(ip));
                }
            }
        }
        slots
            .into_iter()
            .filter_map(|slot| match slot {
                Ok(handle) => handle.join().ok(),
                Err(ip) => Some(measure(ip)),
            })
            .collect()
    });
    to_json(host, port, "", geoip, &addresses)
}

pub fn rpc_reply(is_json: bool, host: &[u8], port: i32) -> Vec<u8> {
    let json = lookup(&String::from_utf8_lossy(host), port);
    if is_json {
        json.into_bytes()
    } else {
        crate::sysmetrics::to_xml(&json).into_bytes()
    }
}

/// A small reader for MaxMind DB files (the format DB-IP Lite ships in).
pub mod mmdb {
    use std::collections::BTreeMap;
    use std::net::IpAddr;
    use std::path::Path;

    const METADATA_MARKER: &[u8] = b"\xAB\xCD\xEFMaxMind.com";

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Str(String),
        Uint(u64),
        Int(i64),
        Double(f64),
        Bool(bool),
        Bytes(Vec<u8>),
        Map(BTreeMap<String, Value>),
        Array(Vec<Value>),
    }

    impl Value {
        pub fn as_str(&self) -> Option<&str> {
            match self {
                Value::Str(s) => Some(s),
                _ => None,
            }
        }
        pub fn as_u64(&self) -> Option<u64> {
            match self {
                Value::Uint(v) => Some(*v),
                Value::Int(v) => u64::try_from(*v).ok(),
                _ => None,
            }
        }
        pub fn get(&self, key: &str) -> Option<&Value> {
            match self {
                Value::Map(map) => map.get(key),
                _ => None,
            }
        }
        pub fn path(&self, keys: &[&str]) -> Option<&Value> {
            keys.iter().try_fold(self, |value, key| value.get(key))
        }
    }

    pub struct Reader {
        data: Vec<u8>,
        node_count: usize,
        record_size: usize,
        ip_version: u64,
        tree_size: usize,
    }

    struct Decoder<'a> {
        data: &'a [u8],
        depth: usize,
    }

    impl Decoder<'_> {
        fn byte(&self, at: usize) -> Result<u8, ()> {
            self.data.get(at).copied().ok_or(())
        }
        fn uint(&self, at: usize, len: usize) -> Result<u64, ()> {
            if len > 8 || at + len > self.data.len() {
                return Err(());
            }
            Ok(self.data[at..at + len]
                .iter()
                .fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
        }
        /// Decodes the value at `at`, returning it and the offset after it.
        fn decode(&mut self, at: usize) -> Result<(Value, usize), ()> {
            self.depth += 1;
            if self.depth > 64 {
                return Err(());
            }
            let result = self.decode_inner(at);
            self.depth -= 1;
            result
        }
        fn decode_inner(&mut self, mut at: usize) -> Result<(Value, usize), ()> {
            let ctrl = self.byte(at)?;
            at += 1;
            let mut kind = ctrl >> 5;
            if kind == 1 {
                let size = usize::from((ctrl >> 3) & 3);
                let low = u64::from(ctrl & 7);
                let (pointer, len) = match size {
                    0 => ((low << 8) | self.uint(at, 1)?, 1),
                    1 => (((low << 16) | self.uint(at, 2)?) + 2048, 2),
                    2 => (((low << 24) | self.uint(at, 3)?) + 526_336, 3),
                    _ => (self.uint(at, 4)?, 4),
                };
                let (value, _) = self.decode(usize::try_from(pointer).map_err(|_| ())?)?;
                return Ok((value, at + len));
            }
            if kind == 0 {
                kind = 7 + self.byte(at)?;
                at += 1;
            }
            let mut size = usize::from(ctrl & 0x1f);
            if size >= 29 {
                let extra = size - 28;
                let v = usize::try_from(self.uint(at, extra)?).map_err(|_| ())?;
                at += extra;
                size = match extra {
                    1 => 29 + v,
                    2 => 285 + v,
                    _ => 65_821 + v,
                };
            }
            let end = at.checked_add(size).ok_or(())?;
            match kind {
                2 => {
                    let bytes = self.data.get(at..end).ok_or(())?;
                    Ok((Value::Str(String::from_utf8_lossy(bytes).into_owned()), end))
                }
                3 => {
                    let bits = self.uint(at, 8)?;
                    Ok((Value::Double(f64::from_bits(bits)), at + 8))
                }
                4 => Ok((
                    Value::Bytes(self.data.get(at..end).ok_or(())?.to_vec()),
                    end,
                )),
                5 | 6 | 9 => Ok((Value::Uint(self.uint(at, size)?), end)),
                10 => {
                    // uint128: keep the low 64 bits, enough for every field read here
                    let len = size.min(16);
                    let skip = len.saturating_sub(8);
                    Ok((Value::Uint(self.uint(at + skip, len - skip)?), end))
                }
                8 => {
                    let raw = self.uint(at, size)?;
                    let shift = 64 - 8 * size.clamp(1, 8) as u32;
                    Ok((Value::Int(((raw << shift) as i64) >> shift), end))
                }
                7 => {
                    let mut map = BTreeMap::new();
                    let mut pos = at;
                    for _ in 0..size {
                        let (key, next) = self.decode(pos)?;
                        let (value, after) = self.decode(next)?;
                        pos = after;
                        if let Value::Str(key) = key {
                            map.insert(key, value);
                        }
                    }
                    Ok((Value::Map(map), pos))
                }
                11 => {
                    let mut list = Vec::new();
                    let mut pos = at;
                    for _ in 0..size {
                        let (value, after) = self.decode(pos)?;
                        list.push(value);
                        pos = after;
                    }
                    Ok((Value::Array(list), pos))
                }
                14 => Ok((Value::Bool(size != 0), at)),
                15 => {
                    let bits = u32::try_from(self.uint(at, 4)?).map_err(|_| ())?;
                    Ok((Value::Double(f64::from(f32::from_bits(bits))), at + 4))
                }
                _ => Err(()),
            }
        }
    }

    impl Reader {
        pub fn open(path: &Path) -> Result<Reader, String> {
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            Reader::from_bytes(data)
        }

        pub fn from_bytes(data: Vec<u8>) -> Result<Reader, String> {
            let window = data.len().saturating_sub(128 * 1024);
            let marker = data[window..]
                .windows(METADATA_MARKER.len())
                .rposition(|w| w == METADATA_MARKER)
                .ok_or("not a MaxMind DB file")?
                + window;
            let meta_start = marker + METADATA_MARKER.len();
            let (meta, _) = Decoder {
                data: &data[meta_start..],
                depth: 0,
            }
            .decode(0)
            .map_err(|_| "unreadable metadata")?;
            let field = |name: &str| {
                meta.get(name)
                    .and_then(Value::as_u64)
                    .ok_or(format!("metadata has no {name}"))
            };
            let node_count = usize::try_from(field("node_count")?).map_err(|_| "bad node count")?;
            let record_size =
                usize::try_from(field("record_size")?).map_err(|_| "bad record size")?;
            let ip_version = field("ip_version")?;
            if ![24, 28, 32].contains(&record_size) {
                return Err("unsupported record size".into());
            }
            let tree_size = node_count * record_size / 4;
            if tree_size + 16 > marker {
                return Err("search tree is larger than the file".into());
            }
            Ok(Reader {
                data,
                node_count,
                record_size,
                ip_version,
                tree_size,
            })
        }

        fn record(&self, node: usize, right: bool) -> Option<usize> {
            let base = node * self.record_size / 4;
            let b = |i: usize| self.data.get(base + i).map(|v| usize::from(*v));
            Some(match (self.record_size, right) {
                (24, false) => (b(0)? << 16) | (b(1)? << 8) | b(2)?,
                (24, true) => (b(3)? << 16) | (b(4)? << 8) | b(5)?,
                (28, false) => ((b(3)? & 0xf0) << 20) | (b(0)? << 16) | (b(1)? << 8) | b(2)?,
                (28, true) => ((b(3)? & 0x0f) << 24) | (b(4)? << 16) | (b(5)? << 8) | b(6)?,
                (32, false) => (b(0)? << 24) | (b(1)? << 16) | (b(2)? << 8) | b(3)?,
                _ => (b(4)? << 24) | (b(5)? << 16) | (b(6)? << 8) | b(7)?,
            })
        }

        /// The record for `ip`, or `None` when the database has nothing for it.
        pub fn lookup(&self, ip: IpAddr) -> Option<Value> {
            let (bytes, bits): ([u8; 16], usize) = match ip {
                IpAddr::V4(v4) if self.ip_version == 6 => {
                    let mut b = [0u8; 16];
                    b[12..].copy_from_slice(&v4.octets());
                    (b, 128)
                }
                IpAddr::V4(v4) => {
                    let mut b = [0u8; 16];
                    b[..4].copy_from_slice(&v4.octets());
                    (b, 32)
                }
                IpAddr::V6(v6) if self.ip_version == 6 => (v6.octets(), 128),
                IpAddr::V6(_) => return None,
            };
            let mut node = 0usize;
            for i in 0..bits {
                if node >= self.node_count {
                    break;
                }
                let bit = (bytes[i / 8] >> (7 - i % 8)) & 1 == 1;
                node = self.record(node, bit)?;
            }
            if node <= self.node_count {
                return None;
            }
            let offset = node - self.node_count - 16;
            let data = self.data.get(self.tree_size + 16..)?;
            Decoder { data, depth: 0 }
                .decode(offset)
                .ok()
                .map(|(v, _)| v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::net::TcpListener;

    // A minimal MaxMind DB writer for tests: an IPv4 tree (record size 24) that
    // maps one /24 prefix to a record, with every other address unknown.
    fn encode(value: &mmdb::Value, out: &mut Vec<u8>) {
        fn head(kind: u8, size: usize, out: &mut Vec<u8>) {
            let (ctrl_kind, ext) = if kind > 7 {
                (0u8, Some(kind - 7))
            } else {
                (kind, None)
            };
            assert!(size < 285);
            out.push((ctrl_kind << 5) | size.min(29) as u8);
            if let Some(ext) = ext {
                out.push(ext);
            }
            if size >= 29 {
                out.push((size - 29) as u8);
            }
        }
        match value {
            mmdb::Value::Str(s) => {
                head(2, s.len(), out);
                out.extend_from_slice(s.as_bytes());
            }
            mmdb::Value::Uint(v) => {
                let bytes: Vec<u8> = v
                    .to_be_bytes()
                    .iter()
                    .copied()
                    .skip_while(|b| *b == 0)
                    .collect();
                head(6, bytes.len(), out);
                out.extend(bytes);
            }
            mmdb::Value::Map(map) => {
                head(7, map.len(), out);
                for (k, v) in map {
                    encode(&mmdb::Value::Str(k.clone()), out);
                    encode(v, out);
                }
            }
            _ => unreachable!(),
        }
    }

    fn map(pairs: Vec<(&str, mmdb::Value)>) -> mmdb::Value {
        mmdb::Value::Map(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn database(prefix: [u8; 3], record: &mmdb::Value) -> Vec<u8> {
        let nodes = 24usize;
        let mut data = Vec::new();
        encode(record, &mut data);
        let pointer = nodes + 16; // data offset 0
        let mut tree = Vec::new();
        for depth in 0..nodes {
            let bit = (prefix[depth / 8] >> (7 - depth % 8)) & 1 == 1;
            let next = if depth + 1 == nodes {
                pointer
            } else {
                depth + 1
            };
            let (left, right) = if bit { (nodes, next) } else { (next, nodes) };
            for v in [left, right] {
                tree.extend_from_slice(&(v as u32).to_be_bytes()[1..]);
            }
        }
        let mut file = tree;
        file.extend_from_slice(&[0u8; 16]);
        file.extend(data);
        file.extend_from_slice(b"\xAB\xCD\xEFMaxMind.com");
        encode(
            &map(vec![
                ("node_count", mmdb::Value::Uint(nodes as u64)),
                ("record_size", mmdb::Value::Uint(24)),
                ("ip_version", mmdb::Value::Uint(4)),
            ]),
            &mut file,
        );
        file
    }

    #[test]
    fn reads_country_and_network_from_maxmind_files() {
        let country = mmdb::Reader::from_bytes(database(
            [192, 0, 2],
            &map(vec![(
                "country",
                map(vec![
                    ("iso_code", mmdb::Value::Str("NL".into())),
                    (
                        "names",
                        map(vec![("en", mmdb::Value::Str("Netherlands".into()))]),
                    ),
                ]),
            )]),
        ))
        .unwrap();
        let asn = mmdb::Reader::from_bytes(database(
            [192, 0, 2],
            &map(vec![
                ("autonomous_system_number", mmdb::Value::Uint(64_500)),
                (
                    "autonomous_system_organization",
                    mmdb::Value::Str("Example Net".into()),
                ),
            ]),
        ))
        .unwrap();
        let mut address = Address {
            ip: "192.0.2.7".into(),
            family: 4,
            ..Address::default()
        };
        locate(&mut address, Some(&country), Some(&asn));
        assert_eq!(
            (
                address.country.as_str(),
                address.country_name.as_str(),
                address.asn,
                address.org.as_str()
            ),
            ("NL", "Netherlands", 64_500, "Example Net")
        );
        let mut other = Address {
            ip: "198.51.100.1".into(),
            ..Address::default()
        };
        locate(&mut other, Some(&country), Some(&asn));
        assert_eq!(
            other,
            Address {
                ip: "198.51.100.1".into(),
                ..Address::default()
            }
        );
        assert!(mmdb::Reader::from_bytes(b"not a database".to_vec()).is_err());
    }

    #[test]
    fn resolves_literals_and_times_a_connect() {
        assert_eq!(
            resolve("127.0.0.1", 563).unwrap(),
            vec!["127.0.0.1:563".parse::<SocketAddr>().unwrap()]
        );
        assert!(resolve("", 563).is_err());
        assert!(resolve("bad host", 563).is_err());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(connect_ms(SocketAddr::new("127.0.0.1".parse().unwrap(), port), Duration::from_secs(2)).is_some());
        drop(listener);
        let json = to_json(
            "news.example",
            563,
            "",
            true,
            &[Address {
                ip: "192.0.2.7".into(),
                family: 4,
                country: "NL".into(),
                country_name: "Netherlands".into(),
                asn: 64_500,
                org: "Example \"Net\"".into(),
                latency_ms: Some(12.5),
            }],
        );
        assert_eq!(json, "{\"Host\":\"news.example\",\"Port\":563,\"Error\":\"\",\"GeoIP\":true,\"Addresses\":[{\"IP\":\"192.0.2.7\",\"Family\":4,\"Country\":\"NL\",\"CountryName\":\"Netherlands\",\"ASN\":64500,\"Org\":\"Example \\\"Net\\\"\",\"LatencyMs\":12.5}]}");
    }

    #[test]
    fn a_link_local_address_keeps_its_interface() {
        let lo = std::ffi::CString::new("lo").unwrap();
        // SAFETY: a NUL-terminated name
        let index = unsafe { libc::if_nametoindex(lo.as_ptr()) };
        if index == 0 {
            return;
        }
        let scoped = SocketAddr::V6(std::net::SocketAddrV6::new("fe80::1".parse().unwrap(), 563, 0, index));
        assert_eq!(shown_address(&scoped), "fe80::1%lo");
        let found = resolve("fe80::1%lo", 563).unwrap();
        assert_eq!(found.iter().map(shown_address).collect::<Vec<_>>(), vec!["fe80::1%lo".to_string()]);
        assert_eq!(shown_address(&"[2001:db8::1]:563".parse().unwrap()), "2001:db8::1");
    }

    #[test]
    fn many_ipv4_addresses_leave_room_for_ipv6() {
        let v4 = |n: u8| SocketAddr::new(IpAddr::from([192, 0, 2, n]), 563);
        let v6 = |n: u16| SocketAddr::new(IpAddr::from([0x2001, 0xdb8, 0, 0, 0, 0, 0, n]), 563);
        let mixed: Vec<SocketAddr> = (1..=10).map(v4).chain((1..=2).map(v6)).collect();
        let picked = pick_addresses(mixed);
        assert_eq!(picked.len(), MAX_ADDRESSES);
        assert_eq!(picked.iter().filter(|ip| ip.is_ipv6()).count(), 2, "{picked:?}");
        assert!(picked[0].is_ipv4(), "IPv4 first");
        let many_v6: Vec<SocketAddr> = (1..=3).map(v4).chain((1..=10).map(v6)).collect();
        let picked = pick_addresses(many_v6);
        assert_eq!((picked.iter().filter(|ip| ip.is_ipv4()).count(), picked.len()), (3, MAX_ADDRESSES));
        assert_eq!(pick_addresses((1..=12).map(v4).collect()).len(), MAX_ADDRESSES);
        assert_eq!(pick_addresses((1..=12).map(v6).collect()).len(), MAX_ADDRESSES);
    }

    #[test]
    fn a_lookup_with_no_threads_left_times_addresses_itself() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = i32::from(listener.local_addr().unwrap().port());
        let spawned = lookup("127.0.0.1", port);
        assert!(spawned.contains("\"IP\":\"127.0.0.1\"") && !spawned.contains("null"), "{spawned}");
        LOOKUP_THREADS_IN_USE.fetch_add(LOOKUP_THREADS, Ordering::SeqCst);
        let inline = lookup("127.0.0.1", port);
        LOOKUP_THREADS_IN_USE.fetch_sub(LOOKUP_THREADS, Ordering::SeqCst);
        assert!(inline.contains("\"IP\":\"127.0.0.1\"") && !inline.contains("null"), "{inline}");
        assert_eq!(LOOKUP_THREADS_IN_USE.load(Ordering::SeqCst), 0, "every thread gave its slot back");
    }
}
