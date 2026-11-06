//! Local TLS cipher policy. Native crypto and provider I/O are injected separately.
use std::collections::BTreeMap;
pub mod job;

pub const RECORD_BYTES: u64 = 16_384;
pub const SAMPLE_NS: u64 = 200_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol { Tls12, Tls13 }

#[derive(Clone, Debug)]
pub struct Suite {
    pub name: String,
    pub algorithm: String,
    pub protocol: Protocol,
    pub security_bits: u16,
    pub aead: bool,
    pub ephemeral: bool,
    pub certificate: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub round: u8,
    pub bytes: u64,
    pub elapsed_ns: u64,
    pub correct: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support { Supported, Unsupported, Unknown }

#[derive(Clone, Debug, PartialEq)]
pub struct Choice { pub cipher: String, pub bytes_per_second: f64 }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub &'static str);

pub fn eligible(suite: &Suite) -> bool {
    suite.security_bits >= 128 && suite.aead && suite.certificate &&
        (suite.protocol == Protocol::Tls13 || suite.ephemeral)
}

pub fn throughput(samples: &[Sample]) -> Result<f64, Error> {
    if samples.len() != 5 { return Err(Error("five rounds are required")); }
    let mut seen = [false; 5];
    let mut rates = [0.0; 5];
    for (i, sample) in samples.iter().enumerate() {
        let round = usize::from(sample.round);
        if round >= 5 || seen[round] || !sample.correct || sample.elapsed_ns < SAMPLE_NS ||
            sample.bytes == 0 || sample.bytes % RECORD_BYTES != 0 {
            return Err(Error("invalid or incomplete measurement"));
        }
        seen[round] = true;
        rates[i] = sample.bytes as f64 * 1_000_000_000.0 / sample.elapsed_ns as f64;
    }
    rates.sort_by(f64::total_cmp);
    Ok(rates[2])
}

pub fn choose(suites: &[Suite], rates: &BTreeMap<String, f64>,
    support: &BTreeMap<String, Support>, current: &str) -> Result<Option<Choice>, Error> {
    let mut candidates = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for suite in suites.iter().filter(|s| eligible(s)) {
        if !names.insert(&suite.name) { return Err(Error("duplicate cipher suite")); }
        match support.get(&suite.name) {
            Some(Support::Supported) => (),
            Some(Support::Unsupported) => continue,
            _ => return Err(Error("server compatibility is incomplete")),
        }
        let rate = *rates.get(&suite.algorithm).ok_or(Error("unmeasured algorithm"))?;
        if !rate.is_finite() || rate <= 0.0 { return Err(Error("invalid algorithm rate")); }
        candidates.push((suite, rate));
    }
    candidates.sort_by(|(a, ar), (b, br)| {
        br.total_cmp(ar)
            .then_with(|| (b.name == current).cmp(&(a.name == current)))
            .then_with(|| (b.protocol == Protocol::Tls13).cmp(&(a.protocol == Protocol::Tls13)))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(candidates.first().map(|(s, rate)| Choice { cipher: s.name.clone(), bytes_per_second: *rate }))
}

pub(crate) fn option(line: &[u8]) -> Option<(&[u8], &[u8], usize)> {
    let eq = line.iter().position(|&b| b == b'=')?;
    let name = trim(&line[..eq]);
    if name.starts_with(b"#") { return None; }
    Some((name, trim(&line[eq + 1..]), eq))
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|b| !b.is_ascii_whitespace()).map_or(start, |i| i + 1);
    &bytes[start..end]
}

pub(crate) fn server_option(name: &[u8], suffix: &[u8]) -> Option<u32> {
    if name.len() <= 7 + suffix.len() || !name[..6].eq_ignore_ascii_case(b"server") { return None; }
    let dot = name.iter().position(|&b| b == b'.')?;
    if !name.get(dot + 1..)?.eq_ignore_ascii_case(suffix) { return None; }
    let id = std::str::from_utf8(name.get(6..dot)?).ok()?.parse::<u32>().ok()?;
    (id > 0 && name.get(6..dot)? == id.to_string().as_bytes()).then_some(id)
}

/// Change only the requested cipher lines; do not parse/re-emit other values.
pub fn patch_config(original: &[u8], changes: &BTreeMap<u32, String>) -> Result<Vec<u8>, Error> {
    if original.len() > 16 * 1024 * 1024 { return Err(Error("configuration exceeds size limit")); }
    if changes.is_empty() { return Ok(original.to_vec()); }
    let hosts: std::collections::BTreeSet<_> = original.split(|&b| b == b'\n')
        .filter_map(option).filter_map(|(name, _, _)| server_option(name, b"host")).collect();
    for (id, value) in changes {
        if !hosts.contains(id) || value.is_empty() || value.len() > 128 ||
            !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return Err(Error("invalid cipher patch"));
        }
    }
    let newline: &[u8] = if original.windows(2).any(|s| s == b"\r\n") { b"\r\n" } else { b"\n" };
    let mut out = Vec::with_capacity(original.len());
    let mut written = std::collections::BTreeSet::new();
    for line in original.split_inclusive(|&b| b == b'\n') {
        if let Some((name, _, eq)) = option(line) {
            if let Some((id, value)) = server_option(name, b"cipher").and_then(|id| changes.get(&id).map(|v| (id, v))) {
                out.extend_from_slice(&line[..=eq]);
                out.extend_from_slice(value.as_bytes());
                if line.ends_with(b"\r\n") { out.extend_from_slice(b"\r\n"); }
                else if line.ends_with(b"\n") { out.push(b'\n'); }
                written.insert(id);
                continue;
            }
        }
        out.extend_from_slice(line);
    }
    for (id, value) in changes.iter().filter(|(id, _)| !written.contains(id)) {
        if !out.is_empty() && !out.ends_with(b"\n") { out.extend_from_slice(newline); }
        out.extend_from_slice(format!("Server{id}.Cipher={value}").as_bytes());
        out.extend_from_slice(newline);
    }
    if out.len() > 16 * 1024 * 1024 { return Err(Error("patched configuration exceeds size limit")); }
    Ok(out)
}

/// Returns whether `TlsSocket::Start` should verify the peer certificate.
pub fn tls_should_verify_peer(has_store: bool, cert_verif_level: i32, cv_none: i32) -> bool {
    has_store && cert_verif_level > cv_none
}

/// Returns whether an OpenSSL error code represents an explicit TLS handshake rejection.
#[allow(clippy::too_many_arguments)]
pub fn tls_is_explicit_handshake_rejection(
    err_lib: i32,
    err_lib_ssl: i32,
    reason: i32,
    r_handshake_fail: i32,
    r_proto_ver: i32,
    r_no_shared_cipher: i32,
    r_unsupported_proto: i32,
) -> bool {
    err_lib == err_lib_ssl
        && (reason == r_handshake_fail
            || reason == r_proto_ver
            || reason == r_no_shared_cipher
            || reason == r_unsupported_proto)
}

/// Returns whether `TlsSocket::ValidateCert` should perform hostname verification.
pub fn tls_should_check_host(cert_verif_level: i32, cv_minimal: i32, host_empty: bool) -> bool {
    cert_verif_level > cv_minimal && !host_empty
}

/// Classifies an `SSL_get_error` code for `SSL_ERROR_WANT_READ` / `SSL_ERROR_WANT_WRITE`.
/// Returns `(retryable, want_read, want_write)`.
pub fn tls_classify_want_io(
    ssl_err: i32,
    want_read: i32,
    want_write: i32,
) -> (bool, bool, bool) {
    let r = ssl_err == want_read;
    let w = ssl_err == want_write;
    (r || w, r, w)
}

/// Returns `true` when `cipher_name` contains `"CCM8"`.
pub fn tls_cipher_has_ccm8(cipher_name: &[u8]) -> bool {
    cipher_name.windows(4).any(|w| w == b"CCM8")
}

/// Formats `"<err_msg>: <err_str>"` for `TlsSocket::ReportError`.
pub fn tls_format_report_err(err_msg: &[u8], err_str: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(err_msg.len() + 2 + err_str.len());
    out.extend_from_slice(err_msg);
    out.extend_from_slice(b": ");
    out.extend_from_slice(err_str);
    out
}

/// Formats `"TLS certificate verification failed for <host>: <verify_err_str>. For more info visit https://nzbget.com/documentation/certificate-verification/"`
/// for `TlsSocket::Start` and `TlsSocket::ValidateCert`.
pub fn tls_format_cert_verify_err(host: &[u8], verify_err_str: &[u8]) -> Vec<u8> {
    const PREFIX: &[u8] = b"TLS certificate verification failed for ";
    const SUFFIX: &[u8] =
        b". For more info visit https://nzbget.com/documentation/certificate-verification/";
    let mut out = Vec::with_capacity(PREFIX.len() + host.len() + 2 + verify_err_str.len() + SUFFIX.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(host);
    out.extend_from_slice(b": ");
    out.extend_from_slice(verify_err_str);
    out.extend_from_slice(SUFFIX);
    out
}

/// Formats `"TLS handshake failed for <host>"` for `TlsSocket::Start`.
pub fn tls_format_handshake_err(host: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(host.len() + 25);
    out.extend_from_slice(b"TLS handshake failed for ");
    out.extend_from_slice(host);
    out
}

/// Formats `"TLS certificate verification failed for <host>: certificate hostname mismatch (<cert_host>). For more info visit https://nzbget.com/documentation/certificate-verification/"`
/// for `TlsSocket::ValidateCert`.
pub fn tls_format_hostname_mismatch_err(host: &[u8], cert_host: &[u8]) -> Vec<u8> {
    const PREFIX: &[u8] = b"TLS certificate verification failed for ";
    const MID: &[u8] = b": certificate hostname mismatch (";
    const SUFFIX: &[u8] =
        b"). For more info visit https://nzbget.com/documentation/certificate-verification/";
    let mut out = Vec::with_capacity(
        PREFIX.len() + host.len() + MID.len() + cert_host.len() + SUFFIX.len(),
    );
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(host);
    out.extend_from_slice(MID);
    out.extend_from_slice(cert_host);
    out.extend_from_slice(SUFFIX);
    out
}

/// Selects the static TLS read/write error string for `TlsSocket::Send` and `TlsSocket::Recv`.
pub fn tls_select_io_err_msg(is_write: bool, peek_err_zero: bool) -> &'static [u8] {
    match (is_write, peek_err_zero) {
        (true, true) => b"Could not write to TLS-Socket: Connection closed by remote host\0",
        (true, false) => b"Could not write to TLS-Socket\0",
        (false, true) => b"Could not read from TLS-Socket: Connection closed by remote host\0",
        (false, false) => b"Could not read from TLS-Socket\0",
    }
}

/// Formats `"nzbget_disktest_<epoch_ticks>.bin"` for `Benchmark::GetUniqueFilename`.
pub fn benchmark_format_unique_filename(epoch_ticks: i64) -> Vec<u8> {
    format!("nzbget_disktest_{epoch_ticks}.bin").into_bytes()
}

/// Resolves the benchmark block size (`default_block_size` when `block_size_bytes == 0`).
pub fn benchmark_resolve_block_size(
    block_size_bytes: usize,
    default_block_size: usize,
) -> usize {
    if block_size_bytes == 0 {
        default_block_size
    } else {
        block_size_bytes
    }
}

/// Returns `true` when `ver == b"TLSv1.3"` for `TlsCipherBackend::Suites` and `TlsSocket::ApplyCipherPin`.
pub fn tls_is_tls13_version_str(ver: &[u8]) -> bool {
    ver == b"TLSv1.3"
}

/// Resolves effective cipher bits (`64` when `cipher_name` contains `"CCM8"`, else `raw_bits`).
pub fn tls_resolve_cipher_bits(cipher_name: &[u8], raw_bits: u32) -> u32 {
    if tls_cipher_has_ccm8(cipher_name) {
        64
    } else {
        raw_bits
    }
}

/// Formats the bulk cipher name (`bulk_name` or `"unavailable"`, plus `"@8"` if `cipher_name` contains `"CCM8"`).
pub fn tls_format_bulk_cipher_name(
    bulk_name: Option<&[u8]>,
    cipher_name: &[u8],
) -> Vec<u8> {
    let base = bulk_name.unwrap_or(b"unavailable");
    let has_ccm8 = tls_cipher_has_ccm8(cipher_name);
    let mut out = Vec::with_capacity(base.len() + if has_ccm8 { 2 } else { 0 });
    out.extend_from_slice(base);
    if has_ccm8 {
        out.extend_from_slice(b"@8");
    }
    out
}

/// Computes the 3-bit TLS suite capability mask (`1 = AEAD`, `2 = cert auth`, `4 = ephemeral KX`).
pub fn tls_compute_suite_flags(
    is_aead: bool,
    tls13: bool,
    auth_is_cert: bool,
    kx_is_ephemeral: bool,
) -> u32 {
    let mut flags = 0u32;
    if is_aead {
        flags |= 1;
    }
    if tls13 || auth_is_cert {
        flags |= 2;
    }
    if tls13 || kx_is_ephemeral {
        flags |= 4;
    }
    flags
}

/// Resolves the probe outcome in `TlsCipherBackend::Probe`.
pub fn tls_resolve_probe_result(
    connected: bool,
    negotiated_cipher: &[u8],
    expected_cipher: &[u8],
    negotiated_version: i32,
    protocol: i32,
    tls13_ver: i32,
    tls12_ver: i32,
    auth_error: bool,
    tls_probe_failure: i32,
) -> i32 {
    let expected_ver = if protocol == 13 { tls13_ver } else { tls12_ver };
    if connected && negotiated_cipher == expected_cipher && negotiated_version == expected_ver {
        1
    } else if auth_error {
        -5
    } else {
        tls_probe_failure
    }
}

/// Returns `true` when `cipher` contains `:` (a multi-suite cipher list rather than a single suite).
pub fn tls_is_cipher_list(cipher: &[u8]) -> bool {
    cipher.contains(&b':')
}

/// Returns true when `TlsSocket::Start` should load both certificate chain and private key files (`!cert_empty && !key_empty`).
pub fn tls_has_cert_and_key_files(cert_empty: bool, key_empty: bool) -> bool {
    !cert_empty && !key_empty
}

/// Returns true when `TlsSocket::Start` should set the SNI hostname (`is_client && !host_empty`).
pub fn tls_should_set_sni_hostname(is_client: bool, host_empty: bool) -> bool {
    is_client && !host_empty
}

/// Returns true when `TlsSocket::Start` should run `ValidateCert()` after handshake (`is_client && should_verify`).
pub fn tls_should_validate_client_cert(is_client: bool, should_verify: bool) -> bool {
    is_client && should_verify
}

/// Returns true when a TLS protocol version falls outside configured `[minimum, maximum]` bounds (`(minimum != 0 && version < minimum) || (maximum != 0 && version > maximum)`).
pub fn tls_is_proto_version_out_of_bounds(version: i32, minimum: i32, maximum: i32) -> bool {
    (minimum != 0 && version < minimum) || (maximum != 0 && version > maximum)
}

/// Returns true when `TlsSocket::Close` should perform `SSL_shutdown` (`has_session && connected && !has_deadline`).
pub fn tls_should_send_close_notify(
    has_session: bool,
    connected: bool,
    has_deadline: bool,
) -> bool {
    has_session && connected && !has_deadline
}

/// Populates the deterministic AEAD benchmark plaintext buffer (`(i * 31 + i / 251) & 255`) in `TlsCipherBackend.cpp`.
pub fn tls_cipher_fill_test_plaintext(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((i * 31 + i / 251) & 255) as u8;
    }
}

/// Returns true when `Crypto::Prepare` batch arguments in `TlsCipherBackend.cpp` are valid (`count > 0 && count <= 64 && sequence <= u64::MAX - u64::from(count)`).
pub fn tls_cipher_is_prepare_batch_valid(sequence: u64, count: u32) -> bool {
    count > 0 && count <= 64 && sequence <= u64::MAX - u64::from(count)
}

/// Writes a 12-byte TLS record nonce (`prefix[0..4]` followed by big-endian `seq`) in `TlsCipherBackend.cpp`.
pub fn tls_cipher_write_record_nonce(prefix: &[u8; 4], seq: u64, out_nonce: &mut [u8; 12]) {
    out_nonce[..4].copy_from_slice(prefix);
    out_nonce[4..12].copy_from_slice(&seq.to_be_bytes());
}

/// Returns true when `sequence` is within `[first, first + count)` in `Crypto::Record` (`TlsCipherBackend.cpp`).
pub fn tls_cipher_is_record_seq_in_range(sequence: u64, first: u64, count: usize) -> bool {
    sequence >= first && (sequence - first) < (count as u64)
}

/// Prechecks `Probe` arguments in `TlsCipherBackend.cpp`:
/// `0` = proceed, `-6` = cancelled, `-3` = zero timeout, `-1` = timeout > 10000ms, `-4` = missing cert store when verification enabled.
pub fn tls_cipher_precheck_probe_args(
    cancelled: bool,
    milliseconds: u64,
    verification: bool,
    has_cert_store: bool,
) -> i32 {
    if cancelled {
        -6
    } else if milliseconds == 0 {
        -3
    } else if milliseconds > 10000 {
        -1
    } else if verification && !has_cert_store {
        -4
    } else {
        0
    }
}

/// Returns true when `Probe` in `TlsCipherBackend.cpp` has a valid resolved address count (`addr_count > 0 && addr_count <= 16`).
pub fn tls_cipher_is_probe_addr_list_valid(addr_count: usize) -> bool {
    addr_count > 0 && addr_count <= 16
}

/// Returns true when `TlsSocket::Start` should configure peer verification against `m_X509Store` (`is_client && has_x509_store`).
pub fn tls_should_verify_client_x509_store(is_client: bool, has_x509_store: bool) -> bool {
    is_client && has_x509_store
}

/// Returns true when `TlsSocket::Start` should inspect `SSL_get_verify_result` after a handshake failure (`is_client && should_verify`).
pub fn tls_should_check_verify_result_on_handshake_fail(
    is_client: bool,
    should_verify: bool,
) -> bool {
    is_client && should_verify
}

/// Resolves the final probe result in `TlsCipherBackend::Probe` after checking cancellation (`-6`) and deadline expiration (`-3`).
pub fn tls_cipher_classify_probe_deadline_override(
    current_result: i32,
    is_cancelled: bool,
    deadline_expired: bool,
) -> i32 {
    if is_cancelled {
        -6
    } else if deadline_expired {
        -3
    } else {
        current_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite(name: &str, algorithm: &str, protocol: Protocol) -> Suite {
        Suite { name: name.into(), algorithm: algorithm.into(), protocol,
            security_bits: 128, aead: true, ephemeral: true, certificate: true }
    }
    fn suites() -> Vec<Suite> {
        vec![suite("TLS_AES_128_GCM_SHA256", "AES-128-GCM", Protocol::Tls13),
            suite("TLS_CHACHA20_POLY1305_SHA256", "CHACHA20-POLY1305", Protocol::Tls13),
            suite("ECDHE-RSA-AES128-GCM-SHA256", "AES-128-GCM", Protocol::Tls12)]
    }
    fn rates() -> BTreeMap<String, f64> {
        BTreeMap::from([("AES-128-GCM".into(), 300.0), ("CHACHA20-POLY1305".into(), 200.0)])
    }
    fn supported() -> BTreeMap<String, Support> {
        suites().iter().map(|s| (s.name.clone(), Support::Supported)).collect()
    }
    fn samples() -> Vec<Sample> {
        (0..5).map(|round| Sample { round, bytes: RECORD_BYTES * (round as u64 + 1),
            elapsed_ns: SAMPLE_NS, correct: true }).collect()
    }

    #[test]
    fn eligible_secure_suites() {
        for s in suites() { assert!(eligible(&s), "secure suite must be eligible: {}", s.name); }
        let mut s = suites()[0].clone();
        s.ephemeral = false; // TLS 1.3 has its own key exchange policy.
        assert!(eligible(&s));
    }
    #[test]
    fn excludes_unsafe_suites() {
        for field in 0..4 {
            let mut s = suites()[2].clone();
            match field { 0 => s.security_bits = 127, 1 => s.aead = false,
                2 => s.ephemeral = false, _ => s.certificate = false }
            assert!(!eligible(&s));
        }
    }
    #[test]
    fn requires_five_unique_valid_rounds() {
        let rate = throughput(&samples()).expect("five complete samples must produce a rate");
        assert_eq!(rate, (RECORD_BYTES * 3) as f64 * 5.0);
    }
    #[test]
    fn rejects_invalid_samples() {
        let mut s = samples();
        s[0].elapsed_ns = 199_999_999;
        assert!(throughput(&s).is_err());
        s = samples(); s[4].round = 3;
        assert!(throughput(&s).is_err());
        s = samples(); s[2].correct = false;
        assert!(throughput(&s).is_err());
        s = samples(); s[2].bytes = RECORD_BYTES - 1;
        assert!(throughput(&s).is_err());
        assert!(throughput(&samples()[..4]).is_err());
    }
    #[test]
    fn selects_fastest_per_server() {
        let s = suites(); let r = rates(); let mut support = supported();
        assert_eq!(choose(&s, &r, &support, "").unwrap().unwrap().cipher, s[0].name);
        support.insert(s[0].name.clone(), Support::Unsupported);
        support.insert(s[2].name.clone(), Support::Unsupported);
        assert_eq!(choose(&s, &r, &support, "").unwrap().unwrap().cipher, s[1].name);
        support.insert(s[1].name.clone(), Support::Unsupported);
        support.insert(s[2].name.clone(), Support::Supported);
        assert_eq!(choose(&s, &r, &support, "").unwrap().unwrap().cipher, s[2].name);
    }
    #[test]
    fn unknown_prevents_choice() {
        let s = suites(); let mut support = supported();
        support.insert(s[1].name.clone(), Support::Unknown);
        assert!(choose(&s, &rates(), &support, "").is_err());
        support.remove(&s[1].name);
        assert!(choose(&s, &rates(), &support, "").is_err());
    }
    #[test]
    fn rejects_missing_or_nonfinite_rates() {
        for rate in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let mut r = rates(); r.insert("AES-128-GCM".into(), rate);
            assert!(choose(&suites(), &r, &supported(), "").is_err());
        }
        assert!(choose(&suites(), &BTreeMap::new(), &supported(), "").is_err());
    }
    #[test]
    fn unsupported_unmeasured_suites_do_not_block_known_fallback() {
        let s = suites();
        // A confirmed rejected suite cannot win for this server, regardless
        // of whether its algorithm could be measured on this computer.
        for unusable_rate in [None, Some(f64::NAN), Some(f64::INFINITY), Some(0.0), Some(-1.0)] {
            let mut r = rates();
            r.remove("CHACHA20-POLY1305");
            if let Some(value) = unusable_rate { r.insert("CHACHA20-POLY1305".into(), value); }
            let mut support = supported();
            support.insert(s[1].name.clone(), Support::Unsupported);
            assert_eq!(choose(&s, &r, &support, "").unwrap().unwrap().cipher, s[0].name);
            // An unmeasured possible winner remains inconclusive.
            support.insert(s[1].name.clone(), Support::Supported);
            assert!(choose(&s, &r, &support, "").is_err());
            support.insert(s[1].name.clone(), Support::Unknown);
            assert!(choose(&s, &r, &support, "").is_err());
            support.remove(&s[1].name);
            assert!(choose(&s, &r, &support, "").is_err());
        }
        let rejected = s.iter().map(|suite| (suite.name.clone(), Support::Unsupported)).collect();
        assert_eq!(choose(&s, &BTreeMap::new(), &rejected, "").unwrap(), None);
    }
    #[test]
    fn ties_retain_current() {
        let s = suites();
        assert_eq!(choose(&s, &rates(), &supported(), &s[2].name).unwrap().unwrap().cipher, s[2].name);
    }
    #[test]
    fn ties_prefer_tls13_then_name() {
        let mut r = rates(); r.insert("CHACHA20-POLY1305".into(), 300.0);
        assert_eq!(choose(&suites(), &r, &supported(), "").unwrap().unwrap().cipher, suites()[0].name);
    }
    #[test]
    fn no_supported_cipher_leaves_server_unchanged() {
        let support = suites().iter().map(|s| (s.name.clone(), Support::Unsupported)).collect();
        assert_eq!(choose(&suites(), &rates(), &support, "old cipher").unwrap(), None);
    }

    #[test]
    fn patch_preserves_unrelated_bytes() {
        let original = b"# private \xff\r\nServer1.Host=news.example.invalid\r\nServer1.Password=secret\r\nServer1.Cipher=old\r\nOther=x\r\n";
        let actual = patch_config(original, &BTreeMap::from([(1, "TLS_AES_128_GCM_SHA256".into())])).unwrap();
        assert_eq!(actual, b"# private \xff\r\nServer1.Host=news.example.invalid\r\nServer1.Password=secret\r\nServer1.Cipher=TLS_AES_128_GCM_SHA256\r\nOther=x\r\n");
    }
    #[test]
    fn patches_duplicate_case_insensitive_cipher_lines() {
        let original = b"Server1.Host=example.invalid\nserver1.cipher=old\n Server1.Cipher =wrong\n#Server1.Cipher=comment\n";
        assert_eq!(patch_config(original, &BTreeMap::from([(1,"NEW_SUITE".into())])).unwrap(),
            b"Server1.Host=example.invalid\nserver1.cipher=NEW_SUITE\n Server1.Cipher =NEW_SUITE\n#Server1.Cipher=comment\n");
    }
    #[test]
    fn appends_missing_cipher_with_matching_newline() {
        let original = b"Server1.Host=example.invalid\r\n# tail";
        let result = patch_config(original, &BTreeMap::from([(1,"NEW_SUITE".into())])).unwrap();
        assert!(result.starts_with(original));
        assert!(result.ends_with(b"# tail\r\nServer1.Cipher=NEW_SUITE\r\n"));
    }
    #[test]
    fn rejects_invalid_patch_values() {
        let original = b"Server1.Host=example.invalid\n";
        for value in ["", "a:b", "a\nOther=yes", "a\0b"] {
            assert!(patch_config(original, &BTreeMap::from([(1,value.into())])).is_err());
        }
        assert!(patch_config(original, &BTreeMap::from([(2,"NEW_SUITE".into())])).is_err());
        assert!(patch_config(original, &BTreeMap::from([(0,"NEW_SUITE".into())])).is_err());
    }
}
