//! Self-update support (`Maintenance`: check for updates, run the update script,
//! verify package signatures).

use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Extracts the string value for `"<key>"` from `package-info.json` in `Maintenance::ReadPackageInfoStr`.
/// Returns `Some((value_start, value_len))` if found and `value_len < max_len`.
pub fn maintenance_extract_package_info_str(
    json: &[u8],
    key: &[u8],
    max_len: usize,
) -> Option<(usize, usize)> {
    let mut quoted_key = Vec::with_capacity(key.len() + 2);
    quoted_key.push(b'"');
    let key_trunc = if key.len() > 97 { &key[..97] } else { key };
    quoted_key.extend_from_slice(key_trunc);
    if quoted_key.len() < 99 {
        quoted_key.push(b'"');
    }

    let key_pos = json
        .windows(quoted_key.len())
        .position(|w| w == quoted_key.as_slice())?;
    let after_key = key_pos + quoted_key.len();
    let quote_open_rel = json[after_key..].iter().position(|&b| b == b'"')?;
    let val_start = after_key + quote_open_rel + 1;
    let quote_close_rel = json[val_start..].iter().position(|&b| b == b'"')?;
    if quote_close_rel >= max_len {
        return None;
    }
    Some((val_start, quote_close_rel))
}

/// Decodes a hex-encoded RSA-SHA256 signature line tail in `Signature::ReadSignature`
/// (starting after `sigTitle`, trimming 2 trailing bytes when `sig_len > 2`, and decoding
/// up to 256 bytes into `out_sig`). Returns `true` iff exactly 256 bytes were decoded.
pub fn maintenance_decode_hex_signature(hex_sig: &[u8], out_sig: &mut [u8; 256]) -> bool {
    let eff_len = if hex_sig.len() > 2 {
        hex_sig.len() - 2
    } else {
        hex_sig.len()
    };
    let slice = &hex_sig[..eff_len];
    let mut in_idx = 0;
    let mut out_idx = 0;
    while in_idx + 1 < slice.len() && out_idx < 256 {
        let c1 = slice[in_idx];
        let c2 = slice[in_idx + 1];
        in_idx += 2;
        let n1 = match c1 {
            b'0'..=b'9' => c1 - b'0',
            b'A'..=b'F' => c1 - b'A' + 10,
            b'a'..=b'f' => c1 - b'a' + 10,
            _ => 0,
        };
        let n2 = match c2 {
            b'0'..=b'9' => c2 - b'0',
            b'A'..=b'F' => c2 - b'A' + 10,
            b'a'..=b'f' => c2 - b'a' + 10,
            _ => 0,
        };
        out_sig[out_idx] = (n1 << 4).wrapping_add(n2);
        out_idx += 1;
    }
    out_idx == 256
}

fn load_regular_file_bytes(path_bytes: &[u8], add_trailing_null: bool) -> Option<Vec<u8>> {
    use std::io::Read;
    let p = bytes_to_path(path_bytes);
    let meta = std::fs::metadata(&p).ok()?;
    if !meta.is_file() {
        return None;
    }
    let file_size = i64::try_from(meta.len()).ok()?;
    let (size, buf_size) =
        crate::util::fs_validate_load_file_size(file_size, i32::from(add_trailing_null))?;
    let size_usize = size as usize;
    let mut file = std::fs::File::open(&p).ok()?;
    let mut buf = vec![0u8; buf_size as usize];
    if size_usize > 0 {
        file.read_exact(&mut buf[..size_usize]).ok()?;
    }
    if add_trailing_null {
        buf.truncate(size_usize);
    }
    Some(buf)
}

/// Loads `<web_dir>/package-info.json` (using `LoadFileIntoBuffer` regular-file rules),
/// extracts `"<key>"` and JSON-decodes the string value.
/// Returns `Ok(decoded_value)` on success, or `Err((1, file_name))` when loading fails,
/// or `Err((2, file_name))` when parsing fails.
pub fn maintenance_read_package_info_str(
    web_dir: &[u8],
    sep: u8,
    key: &[u8],
) -> Result<Vec<u8>, (i32, Vec<u8>)> {
    let mut file_name = crate::systemhealth::maintenance_format_package_info_path(web_dir, sep);
    if file_name.len() > 1023 {
        file_name.truncate(1023);
    }
    let package_info =
        load_regular_file_bytes(&file_name, true).ok_or_else(|| (1, file_name.clone()))?;
    let c_len = package_info
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(package_info.len());
    let (val_start, val_len) =
        maintenance_extract_package_info_str(&package_info[..c_len], key, 1024)
            .ok_or_else(|| (2, file_name))?;
    let mut val = package_info[val_start..val_start + val_len].to_vec();
    let decoded_len = crate::decode::json_decode(&mut val);
    val.truncate(decoded_len);
    Ok(val)
}

/// Computes the SHA-256 digest of `in_filename`, extracts and decodes the 256-byte
/// signature from `sig_filename`, and loads `pub_key_filename` bytes for
/// `Maintenance::Signature::Verify`.
pub fn maintenance_prepare_signature_inputs(
    in_filename: &[u8],
    sig_filename: &[u8],
    pub_key_filename: &[u8],
) -> Option<([u8; 32], [u8; 256], Vec<u8>)> {
    use std::io::{BufRead, BufReader, Read};

    let mut in_file = std::fs::File::open(bytes_to_path(in_filename)).ok()?;
    let mut hasher = crate::archive_native::Sha256State::new();
    let mut chunk = vec![0u8; 32 * 1024];
    loop {
        let n = in_file.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
    }
    let in_hash = hasher.finalize();
    drop(in_file);

    let base_idx = crate::paths::base_file_name(in_filename);
    let mut sig_title = crate::systemhealth::maintenance_format_sig_title(&in_filename[base_idx..]);
    if sig_title.len() > 1023 {
        sig_title.truncate(1023);
    }

    let sig_file = std::fs::File::open(bytes_to_path(sig_filename)).ok()?;
    let mut reader = BufReader::new(sig_file);
    let mut signature = [0u8; 256];
    let mut sig_ok = false;
    loop {
        let mut line_buf = Vec::with_capacity(1022);
        let n = reader
            .by_ref()
            .take(1022)
            .read_until(b'\n', &mut line_buf)
            .ok()?;
        if n == 0 {
            break;
        }
        let c_len = line_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(line_buf.len());
        let line_slice = &line_buf[..c_len];
        if let Some(hex_offset) =
            crate::systemhealth::maintenance_match_sig_line_prefix(line_slice, &sig_title)
        {
            sig_ok = maintenance_decode_hex_signature(&line_slice[hex_offset..], &mut signature);
            break;
        }
    }
    drop(reader);
    if !sig_ok {
        return None;
    }

    let pub_key = load_regular_file_bytes(pub_key_filename, false)?;
    Some((in_hash, signature, pub_key))
}

// --- Maintenance coordinator ---

#[cfg(test)]
#[repr(C)]
pub(crate) struct NzbgetRsMaintenanceEnv {
    pub ctx: *mut c_void,
    pub p1: usize,
    pub a1: c_int,
    pub a2: c_int,
    pub a3: c_int,
    pub a4: c_int,
    pub t1: i64,
    pub s1: *const c_char,
    pub s2: *const c_char,
    pub s3: *const c_char,
    pub out_buf: *mut crate::ffi::RsBuf,
    pub emit_ctx: *mut c_void,
    pub emit_cb: Option<crate::xmlrpc_format::NzbgetRsXmlRpcEmitFn>,
    #[cfg(unix)]
    pub script_host_cb: Option<crate::procrun::NzbgetRsScriptHostFn>,
    #[cfg(not(unix))]
    pub script_host_cb: usize,
    pub action_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            *const c_char,
            *const c_char,
            *const u8,
            usize,
        ) -> usize,
    >,
}

fn cbuf_from_bytes(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&bytes[..end]);
    v.push(0);
    v
}

#[cfg(test)]
unsafe fn opt_cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        std::ffi::CStr::from_ptr(p).to_bytes()
    }
}

struct MaintMessage {
    id: u32,
    kind: c_int,
    time: c_int,
    text: Vec<u8>,
}

pub struct NzbgetRsMaintenance {
    messages: std::sync::Mutex<(u32, Vec<MaintMessage>)>,
    #[cfg(unix)]
    controller: std::sync::Mutex<Option<std::sync::Arc<crate::procrun::ScriptRunner>>>,
    #[cfg(not(unix))]
    #[allow(dead_code)]
    controller: std::sync::Mutex<Option<usize>>,
}

static DAEMON_MAINTENANCE: std::sync::RwLock<Option<std::sync::Arc<NzbgetRsMaintenance>>> =
    std::sync::RwLock::new(None);

pub(crate) fn create_daemon_maintenance() {
    if let Ok(mut g) = DAEMON_MAINTENANCE.write() {
        *g = Some(std::sync::Arc::new(NzbgetRsMaintenance::new()));
    }
}

pub(crate) fn active_maintenance() -> Option<std::sync::Arc<NzbgetRsMaintenance>> {
    DAEMON_MAINTENANCE.read().ok().and_then(|g| g.clone())
}

pub(crate) fn shutdown_daemon_maintenance() {
    if let Some(m) = active_maintenance() {
        #[cfg(unix)]
        {
            let active = m
                .controller
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone();
            if let Some(runner) = active {
                if let Some(hcb) = crate::ffi::script_host_cb() {
                    runner.detach(std::ptr::null_mut(), hcb);
                }
                while m
                    .controller
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .is_some()
                {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
    }
    if let Ok(mut g) = DAEMON_MAINTENANCE.write() {
        *g = None;
    }
}

pub(crate) fn format_maint_script_prefix(
    is_info_script: bool,
    base_script: &[u8],
    max_prefix_len: usize,
) -> (Vec<u8>, Vec<u8>, i32) {
    let mut info_name = if is_info_script {
        b"update-info-script ".to_vec()
    } else {
        b"update-script ".to_vec()
    };
    info_name.extend_from_slice(base_script);

    let bounded_len = base_script.len().min(max_prefix_len);
    let mut log_prefix = base_script[..bounded_len].to_vec();
    if let Some(dot) = log_prefix.iter().rposition(|&b| b == b'.') {
        log_prefix.truncate(dot);
    }
    let prefix_len = (log_prefix.len() + 2) as i32;
    (info_name, log_prefix, prefix_len)
}

#[cfg(unix)]
fn read_pkg_info_active(web_dir: &[u8], sep: u8, key: &[u8]) -> Option<Vec<u8>> {
    match maintenance_read_package_info_str(web_dir, sep, key) {
        Ok(val) => Some(val),
        Err((err_kind, fn_bytes)) => {
            let mut m = if err_kind == 1 {
                b"Could not load file ".to_vec()
            } else {
                b"Could not parse file ".to_vec()
            };
            m.extend_from_slice(&fn_bytes);
            crate::log::log_active_bytes(2, &m);
            None
        }
    }
}

pub(crate) unsafe fn collect_maint_messages_active(
    id_from: c_int,
    nr_entries: c_int,
    emit_ctx: *mut c_void,
    emit_cb: crate::xmlrpc_format::NzbgetRsXmlRpcEmitFn,
) {
    let mut entries = Vec::new();
    let kept = active_maintenance();
    let guard = kept
        .as_deref()
        .map(|m| m.messages.lock().unwrap_or_else(|err| err.into_inner()));
    if let Some(ref g) = guard {
        let list = &g.1;
        let len = list.len() as i32;
        let mut start = len;
        if nr_entries > 0 {
            start = len - nr_entries.min(len);
        }
        if id_from > 0 && !list.is_empty() {
            start = (id_from - list[0].id as i32).max(0);
        }
        if (start as usize) < list.len() {
            entries.reserve(list.len() - start as usize);
            for msg in &list[start as usize..] {
                entries.push(crate::xmlrpc_format::NzbgetRsRpcLogEntry {
                    id: msg.id as c_int,
                    kind: msg.kind,
                    time: msg.time,
                    text: msg.text.as_ptr() as *const c_char,
                });
            }
        }
    }
    emit_cb(
        emit_ctx,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
        entries.as_ptr() as *const c_void,
        entries.len(),
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
    );
}

#[cfg(unix)]
pub(crate) unsafe fn start_update_active(branch: c_int) -> bool {
    let (Some(m), Some(hcb), Some(opts)) = (
        active_maintenance(),
        crate::ffi::script_host_cb(),
        crate::options::active_options(),
    ) else {
        return false;
    };
    let mut ctrl = m.controller.lock().unwrap_or_else(|err| err.into_inner());
    if ctrl.is_some() {
        crate::log::log_active_bytes(
            2,
            b"Could not start update-script: update-script is already running",
        );
        return false;
    }
    let sep = crate::paths::PATH_SEPARATOR as u8;
    let Some(mut script_path) = read_pkg_info_active(opts.str_bytes(8), sep, b"install-script")
    else {
        return false;
    };
    if script_path.first().copied() != Some(sep) {
        let app_dir = opts.str_bytes(1);
        let mut full = Vec::with_capacity(app_dir.len() + 1 + script_path.len());
        full.extend_from_slice(app_dir);
        full.push(sep);
        full.extend_from_slice(&script_path);
        script_path = full;
    }
    {
        let mut mg = m.messages.lock().unwrap_or_else(|err| err.into_inner());
        mg.1.clear();
    }
    let runner = crate::procrun::ScriptRunner::new(false);
    *ctrl = Some(std::sync::Arc::clone(&runner));
    drop(ctrl);
    crate::procrun::spawn_background_job(
        crate::procrun::BackgroundScriptJob::MaintenanceUpdate {
            maint: m,
            runner,
            script_path,
            branch,
            daemon_mode: opts.daemon_mode(),
        },
        std::ptr::null_mut(),
        hcb,
    );
    true
}

#[cfg(unix)]
pub(crate) unsafe fn check_updates_active() -> Option<Vec<u8>> {
    let (Some(hcb), Some(opts)) = (
        crate::ffi::script_host_cb(),
        crate::options::active_options(),
    ) else {
        return None;
    };
    let sep = crate::paths::PATH_SEPARATOR as u8;
    let info_script = read_pkg_info_active(opts.str_bytes(8), sep, b"update-info-script")?;
    let base = &info_script[crate::paths::base_file_name(&info_script)..];
    let mut det = b"Executing update-info-script ".to_vec();
    det.extend_from_slice(base);
    crate::log::log_active_bytes(4, &det);

    let (info_name, log_prefix, prefix_len) = format_maint_script_prefix(true, base, 1023);
    let acc = crate::procrun::run_maint_info_script(
        &info_script,
        &info_name,
        &log_prefix,
        prefix_len.max(0) as usize,
        std::ptr::null_mut(),
        hcb,
    );
    if acc.is_empty() {
        None
    } else {
        Some(acc)
    }
}

fn decode_strict_base64(clean: &[u8]) -> Option<Vec<u8>> {
    if clean.is_empty() || clean.len() % 4 != 0 {
        return None;
    }
    fn b64_val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    let chunks = clean.len() / 4;
    for (idx, q) in clean.chunks_exact(4).enumerate() {
        let last = idx + 1 == chunks;
        let a = b64_val(q[0])?;
        let b = b64_val(q[1])?;
        if q[2] == b'=' {
            if !last || q[3] != b'=' || (b & 0x0f) != 0 {
                return None;
            }
            out.push((a << 2) | (b >> 4));
        } else if q[3] == b'=' {
            if !last {
                return None;
            }
            let c = b64_val(q[2])?;
            if (c & 0x03) != 0 {
                return None;
            }
            out.push((a << 2) | (b >> 4));
            out.push((b << 4) | (c >> 2));
        } else {
            let c = b64_val(q[2])?;
            let d = b64_val(q[3])?;
            out.push((a << 2) | (b >> 4));
            out.push((b << 4) | (c >> 2));
            out.push((c << 6) | d);
        }
    }
    Some(out)
}

fn parse_spki_pem(pem: &[u8]) -> Option<Vec<u8>> {
    const BEGIN: &[u8] = b"-----BEGIN PUBLIC KEY-----";
    const END: &[u8] = b"-----END PUBLIC KEY-----";
    let mut in_body = false;
    let mut b64 = Vec::new();
    for raw_line in pem.split(|&b| b == b'\n') {
        let mut line = raw_line;
        while let Some(&last) = line.last() {
            if matches!(last, b'\r' | b' ' | b'\t') {
                line = &line[..line.len() - 1];
            } else {
                break;
            }
        }
        if !in_body {
            if line == BEGIN {
                in_body = true;
            }
        } else if line == END {
            return decode_strict_base64(&b64);
        } else {
            for &ch in line {
                if !matches!(ch, b' ' | b'\t' | b'\r') {
                    b64.push(ch);
                }
            }
        }
    }
    None
}

fn read_der_tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    if input.len() < 2 {
        return None;
    }
    let tag = input[0];
    let len_byte = input[1];
    let (hdr_len, content_len) = if (len_byte & 0x80) == 0 {
        (2usize, len_byte as usize)
    } else {
        let num_octets = (len_byte & 0x7f) as usize;
        if num_octets == 0 || num_octets > 4 || input.len() < 2 + num_octets {
            return None;
        }
        if input[2] == 0 {
            return None;
        }
        let mut len = 0usize;
        for &b in &input[2..2 + num_octets] {
            len = (len << 8) | (b as usize);
        }
        if len < 0x80 {
            return None;
        }
        (2 + num_octets, len)
    };
    if input.len() < hdr_len.checked_add(content_len)? {
        return None;
    }
    Some((
        tag,
        &input[hdr_len..hdr_len + content_len],
        &input[hdr_len + content_len..],
    ))
}

fn read_der_positive_int(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let (tag, content, rest) = read_der_tlv(input)?;
    if tag != 0x02 || content.is_empty() || (content[0] & 0x80) != 0 {
        return None;
    }
    if content.len() > 1 && content[0] == 0x00 && (content[1] & 0x80) == 0 {
        return None;
    }
    let mag = if content[0] == 0x00 {
        &content[1..]
    } else {
        content
    };
    if mag.is_empty() {
        return None;
    }
    Some((mag, rest))
}

fn parse_rsa2048_spki_der(der: &[u8]) -> Option<([u8; 256], Vec<u8>)> {
    const OID_RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
    let (outer_tag, spki, outer_rest) = read_der_tlv(der)?;
    if outer_tag != 0x30 || !outer_rest.is_empty() {
        return None;
    }
    let (alg_tag, alg_body, spki_rest) = read_der_tlv(spki)?;
    if alg_tag != 0x30 {
        return None;
    }
    let (oid_tag, oid_bytes, alg_params) = read_der_tlv(alg_body)?;
    if oid_tag != 0x06 || oid_bytes != OID_RSA_ENCRYPTION {
        return None;
    }
    if !alg_params.is_empty() && alg_params != [0x05, 0x00] {
        return None;
    }
    let (bs_tag, bs_body, spki_tail) = read_der_tlv(spki_rest)?;
    if bs_tag != 0x03 || !spki_tail.is_empty() || bs_body.first().copied() != Some(0x00) {
        return None;
    }
    let (rsa_tag, rsa_body, rsa_tail) = read_der_tlv(&bs_body[1..])?;
    if rsa_tag != 0x30 || !rsa_tail.is_empty() {
        return None;
    }
    let (n_mag, rsa_rest1) = read_der_positive_int(rsa_body)?;
    let (e_mag, rsa_rest2) = read_der_positive_int(rsa_rest1)?;
    if !rsa_rest2.is_empty() || n_mag.len() != 256 || n_mag[0] == 0 || (n_mag[255] & 1) == 0 {
        return None;
    }
    if e_mag.is_empty()
        || e_mag.len() > 256
        || (e_mag[e_mag.len() - 1] & 1) == 0
        || (e_mag.len() == 1 && e_mag[0] < 3)
        || (e_mag.len() == 256 && e_mag >= n_mag)
    {
        return None;
    }
    let mut n = [0u8; 256];
    n.copy_from_slice(n_mag);
    Some((n, e_mag.to_vec()))
}

fn limbs_from_be_256(bytes: &[u8; 256]) -> [u64; 32] {
    let mut limbs = [0u64; 32];
    for (i, chunk) in bytes.rchunks_exact(8).enumerate() {
        let mut arr = [0u8; 8];
        arr.copy_from_slice(chunk);
        limbs[i] = u64::from_be_bytes(arr);
    }
    limbs
}

fn limbs_to_be_256(limbs: &[u64; 32]) -> [u8; 256] {
    let mut out = [0u8; 256];
    for (i, &limb) in limbs.iter().rev().enumerate() {
        out[i * 8..(i + 1) * 8].copy_from_slice(&limb.to_be_bytes());
    }
    out
}

fn ge_2048(a: &[u64; 32], b: &[u64; 32]) -> bool {
    for i in (0..32).rev() {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

fn sub_assign_2048(a: &mut [u64; 32], b: &[u64; 32]) {
    let mut borrow = 0u64;
    for i in 0..32 {
        let (d1, b1) = a[i].overflowing_sub(b[i]);
        let (d2, b2) = d1.overflowing_sub(borrow);
        a[i] = d2;
        borrow = u64::from(b1 || b2);
    }
}

fn add_assign_2048(a: &mut [u64; 32], b: &[u64; 32]) -> bool {
    let mut carry = 0u64;
    for i in 0..32 {
        let (s1, c1) = a[i].overflowing_add(b[i]);
        let (s2, c2) = s1.overflowing_add(carry);
        a[i] = s2;
        carry = u64::from(c1 || c2);
    }
    carry != 0
}

fn shl1_2048(a: &mut [u64; 32]) -> bool {
    let mut carry = 0u64;
    for limb in a.iter_mut() {
        let next = *limb >> 63;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
    carry != 0
}

fn mul_mod_2048(a: &[u64; 32], b: &[u64; 32], n: &[u64; 32]) -> [u64; 32] {
    let mut acc = [0u64; 32];
    for bit in (0..2048).rev() {
        let c1 = shl1_2048(&mut acc);
        if c1 || ge_2048(&acc, n) {
            sub_assign_2048(&mut acc, n);
        }
        if ((b[bit / 64] >> (bit % 64)) & 1) != 0 {
            let c2 = add_assign_2048(&mut acc, a);
            if c2 || ge_2048(&acc, n) {
                sub_assign_2048(&mut acc, n);
            }
        }
    }
    acc
}

/// Verifies an RSA-2048 RSASSA-PKCS1-v1_5 SHA-256 signature against a PEM-encoded
/// X.509 SubjectPublicKeyInfo public key (`pubkey.pem`).
pub fn verify_rsa2048_pkcs1_sha256(
    in_hash: &[u8; 32],
    sig: &[u8; 256],
    pub_key_pem: &[u8],
) -> bool {
    const DIGEST_INFO_SHA256_NULL: [u8; 19] = [
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    const DIGEST_INFO_SHA256_NO_NULL: [u8; 17] = [
        0x30, 0x2f, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x04, 0x20,
    ];

    let Some(der) = parse_spki_pem(pub_key_pem) else {
        return false;
    };
    let Some((n_be, e_be)) = parse_rsa2048_spki_der(&der) else {
        return false;
    };
    if sig[..] >= n_be[..] {
        return false;
    }

    let n = limbs_from_be_256(&n_be);
    let base = limbs_from_be_256(sig);
    let mut result = [0u64; 32];
    result[0] = 1;

    let mut started = false;
    for &byte in &e_be {
        for shift in (0..8).rev() {
            let bit = (byte >> shift) & 1;
            if !started {
                if bit != 0 {
                    started = true;
                    result = base;
                }
                continue;
            }
            result = mul_mod_2048(&result, &result, &n);
            if bit != 0 {
                result = mul_mod_2048(&result, &base, &n);
            }
        }
    }

    let em = limbs_to_be_256(&result);
    let mut expected_null = [0xffu8; 256];
    expected_null[0] = 0x00;
    expected_null[1] = 0x01;
    expected_null[204] = 0x00;
    expected_null[205..224].copy_from_slice(&DIGEST_INFO_SHA256_NULL);
    expected_null[224..256].copy_from_slice(in_hash);
    if em == expected_null {
        return true;
    }

    let mut expected_no_null = [0xffu8; 256];
    expected_no_null[0] = 0x00;
    expected_no_null[1] = 0x01;
    expected_no_null[206] = 0x00;
    expected_no_null[207..224].copy_from_slice(&DIGEST_INFO_SHA256_NO_NULL);
    expected_no_null[224..256].copy_from_slice(in_hash);
    em == expected_no_null
}

#[cfg(unix)]
pub(crate) fn verify_signature_active(
    in_filename: &[u8],
    sig_filename: &[u8],
    pub_key_filename: &[u8],
) -> bool {
    if crate::ffi::build_tls_disabled() {
        return false;
    }
    let Some((in_hash, sig, pub_key)) =
        maintenance_prepare_signature_inputs(in_filename, sig_filename, pub_key_filename)
    else {
        return false;
    };
    verify_rsa2048_pkcs1_sha256(&in_hash, &sig, &pub_key)
}

#[cfg(test)]
unsafe fn emit_maint_log(env: &NzbgetRsMaintenanceEnv, kind: c_int, msg: &[u8]) {
    #[cfg(unix)]
    if let Some(hcb) = env.script_host_cb {
        crate::procrun::host_log(std::ptr::null_mut(), hcb, kind, msg);
    } else {
        crate::log::log_active_bytes(kind, msg);
    }
    #[cfg(not(unix))]
    let _ = (env, kind, msg);
}

#[cfg(test)]
unsafe fn read_pkg_info_logged(
    env: &NzbgetRsMaintenanceEnv,
    web_dir: &[u8],
    sep: u8,
    key: &[u8],
) -> Option<Vec<u8>> {
    match maintenance_read_package_info_str(web_dir, sep, key) {
        Ok(val) => Some(val),
        Err((err_kind, fn_bytes)) => {
            let mut m = if err_kind == 1 {
                b"Could not load file ".to_vec()
            } else {
                b"Could not parse file ".to_vec()
            };
            m.extend_from_slice(&fn_bytes);
            emit_maint_log(env, 2, &m);
            None
        }
    }
}

impl NzbgetRsMaintenance {
    fn new() -> Self {
        Self {
            messages: std::sync::Mutex::new((0, Vec::new())),
            controller: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn add_message_pub(&self, kind: c_int, time: i64, text: &[u8]) {
        self.add_message(kind, time, text);
    }

    pub(crate) fn clear_update_controller(&self) {
        *self.controller.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn add_message(&self, kind: c_int, mut time: i64, text: &[u8]) {
        if time == 0 {
            #[cfg(unix)]
            unsafe {
                time = libc::time(std::ptr::null_mut()) as i64;
            }
        }
        let mut g = self.messages.lock().unwrap_or_else(|e| e.into_inner());
        g.0 = g.0.wrapping_add(1);
        let id = g.0;
        g.1.push(MaintMessage {
            id,
            kind,
            time: time as c_int,
            text: cbuf_from_bytes(text),
        });
    }
}

#[cfg(test)]
pub(crate) fn nzbget_rs_maintenance_new() -> *mut NzbgetRsMaintenance {
    std::sync::Arc::into_raw(std::sync::Arc::new(NzbgetRsMaintenance::new())).cast_mut()
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_maintenance_free(h: *mut NzbgetRsMaintenance) {
    if !h.is_null() {
        drop(std::sync::Arc::from_raw(h.cast_const()));
    }
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_maintenance_exec(
    h: *mut NzbgetRsMaintenance,
    op: c_int,
    env: *const NzbgetRsMaintenanceEnv,
) -> c_int {
    if op == 8 {
        let Some(e) = env.as_ref() else {
            return 0;
        };
        let Some((in_hash, sig, pub_key)) = maintenance_prepare_signature_inputs(
            opt_cstr_bytes(e.s1),
            opt_cstr_bytes(e.s2),
            opt_cstr_bytes(e.s3),
        ) else {
            return 0;
        };
        if let Some(cb) = e.action_cb {
            let r = cb(
                e.ctx,
                6,
                in_hash.as_ptr() as *const c_char,
                sig.as_ptr() as *const c_char,
                pub_key.as_ptr(),
                pub_key.len(),
            );
            return c_int::from(r != 0);
        }
        return c_int::from(verify_rsa2048_pkcs1_sha256(&in_hash, &sig, &pub_key));
    }

    let Some(m) = h.as_ref() else {
        return 0;
    };
    match op {
        0 => {
            #[cfg(unix)]
            {
                let active = m
                    .controller
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .clone();
                if let Some(runner) = active {
                    if let Some(e) = env.as_ref() {
                        if let Some(hcb) = e.script_host_cb {
                            runner.detach(std::ptr::null_mut(), hcb);
                        }
                    }
                    while m
                        .controller
                        .lock()
                        .unwrap_or_else(|err| err.into_inner())
                        .is_some()
                    {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
            }
            #[cfg(not(unix))]
            let _ = env;
            1
        }
        2 => {
            if let Some(e) = env.as_ref() {
                m.add_message(e.a1, e.t1, opt_cstr_bytes(e.s1));
            }
            1
        }
        3 => {
            let Some(e) = env.as_ref() else {
                return 0;
            };
            let Some(emit) = e.emit_cb else {
                return 0;
            };
            let g = m.messages.lock().unwrap_or_else(|err| err.into_inner());
            let list = &g.1;
            let len = list.len() as i32;
            let mut start = len;
            if e.a4 > 0 {
                start = len - e.a4.min(len);
            }
            if e.a3 > 0 && !list.is_empty() {
                start = (e.a3 - list[0].id as i32).max(0);
            }
            let mut entries = Vec::new();
            if (start as usize) < list.len() {
                entries.reserve(list.len() - start as usize);
                for msg in &list[start as usize..] {
                    entries.push(crate::xmlrpc_format::NzbgetRsRpcLogEntry {
                        id: msg.id as c_int,
                        kind: msg.kind,
                        time: msg.time,
                        text: msg.text.as_ptr() as *const c_char,
                    });
                }
            }
            emit(
                e.emit_ctx,
                2,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                entries.as_ptr() as *const c_void,
                entries.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            );
            1
        }
        4 => {
            #[cfg(unix)]
            {
                let Some(e) = env.as_ref() else {
                    return 0;
                };
                let Some(hcb) = e.script_host_cb else {
                    return 0;
                };
                let mut ctrl = m.controller.lock().unwrap_or_else(|err| err.into_inner());
                if ctrl.is_some() {
                    emit_maint_log(
                        e,
                        2,
                        b"Could not start update-script: update-script is already running",
                    );
                    return 0;
                }
                let sep = e.a2 as u8;
                let Some(mut script_path) =
                    read_pkg_info_logged(e, opt_cstr_bytes(e.s1), sep, b"install-script")
                else {
                    return 0;
                };
                if script_path.first().copied() != Some(sep) {
                    let app_dir = opt_cstr_bytes(e.s2);
                    let mut full = Vec::with_capacity(app_dir.len() + 1 + script_path.len());
                    full.extend_from_slice(app_dir);
                    full.push(sep);
                    full.extend_from_slice(&script_path);
                    script_path = full;
                }
                {
                    let mut mg = m.messages.lock().unwrap_or_else(|err| err.into_inner());
                    mg.1.clear();
                }
                let runner = crate::procrun::ScriptRunner::new(false);
                *ctrl = Some(std::sync::Arc::clone(&runner));
                drop(ctrl);
                std::sync::Arc::increment_strong_count(h.cast_const());
                let maint_arc = std::sync::Arc::from_raw(h.cast_const());
                crate::procrun::spawn_background_job(
                    crate::procrun::BackgroundScriptJob::MaintenanceUpdate {
                        maint: maint_arc,
                        runner,
                        script_path,
                        branch: e.a1,
                        daemon_mode: e.a3 != 0,
                    },
                    std::ptr::null_mut(),
                    hcb,
                );
                1
            }
            #[cfg(not(unix))]
            {
                let _ = env;
                0
            }
        }
        5 => {
            #[cfg(unix)]
            {
                let Some(e) = env.as_ref() else {
                    return 0;
                };
                let Some(hcb) = e.script_host_cb else {
                    return 0;
                };
                let sep = e.a2 as u8;
                let Some(info_script) =
                    read_pkg_info_logged(e, opt_cstr_bytes(e.s1), sep, b"update-info-script")
                else {
                    return 0;
                };
                let base = &info_script[crate::paths::base_file_name(&info_script)..];
                let mut det = b"Executing update-info-script ".to_vec();
                det.extend_from_slice(base);
                emit_maint_log(e, 4, &det);

                let (info_name, log_prefix, prefix_len) =
                    format_maint_script_prefix(true, base, 1023);
                let acc = crate::procrun::run_maint_info_script(
                    &info_script,
                    &info_name,
                    &log_prefix,
                    prefix_len.max(0) as usize,
                    std::ptr::null_mut(),
                    hcb,
                );
                if acc.is_empty() {
                    return 0;
                }
                if !e.out_buf.is_null() {
                    *e.out_buf = crate::ffi::into_buf(acc);
                }
                1
            }
            #[cfg(not(unix))]
            {
                let _ = env;
                0
            }
        }
        _ => 0,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_read_package_info_str_truncates_json_decoded_escapes() {
        let dir = std::env::temp_dir().join(format!("nzbget_rs_pkginfo_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pkg_path = dir.join("package-info.json");
        std::fs::write(
            &pkg_path,
            br#"{"update-info-script": "https:\/\/example.com\/update.sh"}"#,
        )
        .unwrap();
        let val = maintenance_read_package_info_str(
            dir.to_string_lossy().as_bytes(),
            std::path::MAIN_SEPARATOR as u8,
            b"update-info-script",
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(val, b"https://example.com/update.sh");
    }

    #[test]
    fn maintenance_exec_lifecycle() {
        unsafe extern "C" fn mock_maint_emit(
            emit_ctx: *mut c_void,
            sub: c_int,
            _a1: c_int,
            _a2: c_int,
            _a3: c_int,
            _a4: c_int,
            _a5: c_int,
            _a6: c_int,
            _i1: i64,
            _i2: i64,
            _i3: i64,
            _s1: *const c_char,
            _s2: *const c_char,
            _s3: *const c_char,
            _s4: *const c_char,
            ptr1: *const c_void,
            len1: usize,
            _ptr2: *const c_void,
            _len2: usize,
            _ptr3: *const c_void,
            _len3: usize,
            _ptr4: *const c_void,
            _len4: usize,
        ) {
            assert_eq!(sub, 2);
            let out = &mut *(emit_ctx as *mut Vec<(i32, i32, Vec<u8>)>);
            let slice = std::slice::from_raw_parts(
                ptr1 as *const crate::xmlrpc_format::NzbgetRsRpcLogEntry,
                len1,
            );
            for e in slice {
                let b = std::ffi::CStr::from_ptr(e.text).to_bytes().to_vec();
                out.push((e.id, e.kind, b));
            }
        }

        let dir = std::env::temp_dir().join(format!("nzbget_rs_qslife_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        unsafe {
            let m = nzbget_rs_maintenance_new();
            assert!(!m.is_null());
            unsafe extern "C" fn mock_maint_action(
                _ctx: *mut c_void,
                _sub: c_int,
                _s1: *const c_char,
                _s2: *const c_char,
                _bytes: *const u8,
                _len: usize,
            ) -> usize {
                1
            }
            let m1 = b"first line\0";
            let m2 = b"second line\0";
            let mut menv = NzbgetRsMaintenanceEnv {
                ctx: std::ptr::null_mut(),
                p1: 0,
                a1: 0,
                a2: 0,
                a3: 0,
                a4: 0,
                t1: 1000,
                s1: m1.as_ptr() as *const c_char,
                s2: std::ptr::null(),
                s3: std::ptr::null(),
                out_buf: std::ptr::null_mut(),
                emit_ctx: std::ptr::null_mut(),
                emit_cb: None,
                #[cfg(unix)]
                script_host_cb: None,
                #[cfg(not(unix))]
                script_host_cb: 0,
                action_cb: Some(mock_maint_action),
            };
            assert_eq!(nzbget_rs_maintenance_exec(m, 2, &menv), 1);
            menv.a1 = 1;
            menv.s1 = m2.as_ptr() as *const c_char;
            assert_eq!(nzbget_rs_maintenance_exec(m, 2, &menv), 1);

            let mut collected: Vec<(i32, i32, Vec<u8>)> = Vec::new();
            menv.a3 = 0;
            menv.a4 = 10;
            menv.emit_ctx = (&mut collected as *mut Vec<(i32, i32, Vec<u8>)>).cast();
            menv.emit_cb = Some(mock_maint_emit);
            assert_eq!(nzbget_rs_maintenance_exec(m, 3, &menv), 1);
            assert_eq!(collected.len(), 2);
            assert_eq!(collected[0], (1, 0, b"first line".to_vec()));
            assert_eq!(collected[1], (2, 1, b"second line".to_vec()));

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                static LEGACY_HOST_OP_CALLED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                unsafe extern "C" fn strict_prod_script_host_cb(
                    _ctx: *mut c_void,
                    op: c_int,
                    _a1: c_int,
                    _a2: c_int,
                    _p1: usize,
                    _p2: usize,
                    _s1: *const c_char,
                    _s2: *const c_char,
                    _ints: *mut c_int,
                    _strs: *mut *const c_char,
                ) -> usize {
                    if op == 0 || op == 2 {
                        LEGACY_HOST_OP_CALLED.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    0
                }

                let script_path = dir.join("update-info.sh");
                std::fs::write(
                    &script_path,
                    b"#!/bin/sh\necho '[NZB] [UPDATEINFO]version=27.0'\n",
                )
                .unwrap();
                std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                    .unwrap();
                let pkg_json = format!(
                    "{{\"update-info-script\":\"{}\"}}",
                    script_path.to_str().unwrap()
                );
                std::fs::write(dir.join("package-info.json"), pkg_json.as_bytes()).unwrap();

                let mut web_dir_c = dir.to_str().unwrap().as_bytes().to_vec();
                web_dir_c.push(0);
                let mut out_buf = crate::ffi::RsBuf {
                    data: std::ptr::null_mut(),
                    len: 0,
                    cap: 0,
                };
                let mut sentinel = 1u8;
                LEGACY_HOST_OP_CALLED.store(false, std::sync::atomic::Ordering::SeqCst);
                let check_env = NzbgetRsMaintenanceEnv {
                    ctx: (&mut sentinel as *mut u8).cast(),
                    p1: 0,
                    a1: 0,
                    a2: b'/' as c_int,
                    a3: 0,
                    a4: 0,
                    t1: 0,
                    s1: web_dir_c.as_ptr().cast(),
                    s2: std::ptr::null(),
                    s3: std::ptr::null(),
                    out_buf: &mut out_buf,
                    emit_ctx: std::ptr::null_mut(),
                    emit_cb: None,
                    script_host_cb: Some(strict_prod_script_host_cb),
                    action_cb: None,
                };
                assert_eq!(nzbget_rs_maintenance_exec(m, 5, &check_env), 1);
                assert!(
                    !LEGACY_HOST_OP_CALLED.load(std::sync::atomic::Ordering::SeqCst),
                    "CheckUpdates must use active runtime options/logging even with non-null env.ctx"
                );
                let out_str =
                    std::slice::from_raw_parts(out_buf.data.cast::<u8>(), out_buf.len).to_vec();
                crate::ffi::nzbget_rs_free(out_buf);
                assert!(
                    out_str
                        .windows(b"version=27.0".len())
                        .any(|w| w == b"version=27.0"),
                    "unexpected CheckUpdates output: {:?}",
                    out_str
                );
            }

            nzbget_rs_maintenance_free(m);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_verify_rsa2048_signature_end_to_end() {
        const PUB_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA2BlAPZqJ3S+PgPuCPtxJ\n\
PNi9ext3OeyPMhAa1w2LSGGwsJQh0s2tiHKTvDt8Wx4u10CGAeRZ5JEL2HWptxLP\n\
tPtw1vjXQ66nT+mNiQlCxaFK5QGwPnnflklrlxk8gncF8vT8iDYqo6Q49ToVS00D\n\
+UAp3KepTzXnzSjVvrYgtCOAJ0DUMclY3lhrJkbKxK79x+yciaCoIRk7foUTaAVu\n\
a0tBmtjbEVz6emPeX9nb6o13TJ23SX0MsCqF9l9/f2GUvVr6fkQIREjP/QdQBKiC\n\
8+yg51ygeuhM2uPd+d/vSnl0i79K3CBB4b9GZSQYu/wy7CwixNRMzioFa3qtHCXU\n\
5wIDAQAB\n\
-----END PUBLIC KEY-----\n";
        const SIG_HEX: &[u8] = b"05a927b3549d9225a695eeb2bc3f2c1dbb87d236b226da565451b8eb0b7b5db5\
9d4d08a86abc4a4ea9bfeaf7987d51ed5a2cf6e0184aee23d24d3f0c4de96131\
f92cede14cd5cff044e1ce25a2d1231b05bb5caabeb7163dfb46d8a01ba27806\
26462b95ce92c0c31bdb376dbb280a84f3efbcda9f98e595e1c79ef23d8a6d39\
89736872b16199a1d2e7a18973c9487714c3135c417bed23122037dd78f74099\
5ba52fab0eb227191612dcb7cd6ef425a3dcee2ca9162ba5a1a792c8d2f89743\
cf99de5cd44e3f1d97c5aa1183310f8eb72d89cccbbc7c091de074556823da77\
001eddd3e196e54ebca418a5f93a1f06c93ab3a9f6a033402b6d46e8adbf8026";
        const PAYLOAD: &[u8] =
            b"RustyGet installer payload for RSA-2048 PKCS#1 v1.5 SHA-256 test\n";

        let dir = std::env::temp_dir().join(format!(
            "rustyget_sig_verify_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let in_path = dir.join("nzbget-27.0-bin-linux.run");
        let sig_path = dir.join("nzbget-27.0.sig.txt");
        let pub_path = dir.join("pubkey.pem");
        std::fs::write(&in_path, PAYLOAD).unwrap();
        let mut sig_file_bytes = b"\"RSA-SHA256(nzbget-27.0-bin-linux.run)\" : \"".to_vec();
        sig_file_bytes.extend_from_slice(SIG_HEX);
        sig_file_bytes.extend_from_slice(b"\"\n");
        std::fs::write(&sig_path, &sig_file_bytes).unwrap();
        std::fs::write(&pub_path, PUB_PEM).unwrap();

        let (hash, mut sig, pub_key) = maintenance_prepare_signature_inputs(
            in_path.to_str().unwrap().as_bytes(),
            sig_path.to_str().unwrap().as_bytes(),
            pub_path.to_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert!(verify_rsa2048_pkcs1_sha256(&hash, &sig, &pub_key));

        #[cfg(unix)]
        assert!(verify_signature_active(
            in_path.to_str().unwrap().as_bytes(),
            sig_path.to_str().unwrap().as_bytes(),
            pub_path.to_str().unwrap().as_bytes(),
        ));

        let mut bad_hash = hash;
        bad_hash[0] ^= 0x01;
        assert!(!verify_rsa2048_pkcs1_sha256(&bad_hash, &sig, &pub_key));

        sig[255] ^= 0x01;
        assert!(!verify_rsa2048_pkcs1_sha256(&hash, &sig, &pub_key));
        sig[255] ^= 0x01;

        let max_sig = [0xffu8; 256];
        assert!(!verify_rsa2048_pkcs1_sha256(&hash, &max_sig, &pub_key));

        let repo_pubkey = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("pubkey.pem");
        let repo_pem = std::fs::read(repo_pubkey).unwrap();
        assert!(parse_rsa2048_spki_der(&parse_spki_pem(&repo_pem).unwrap()).is_some());
        assert!(!verify_rsa2048_pkcs1_sha256(&hash, &sig, &repo_pem));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
