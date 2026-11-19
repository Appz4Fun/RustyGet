//! Certificate tools for the Settings page: a file browser that sees what the daemon
//! user sees, checks for an HTTPS certificate and key, a self-signed certificate
//! maker, a place to save pasted PEM text, and a count of the certificates in a CA
//! store. Everything is written under `MainDir` (`https/` and `certs/`), never
//! anywhere the request names.

use crate::options::OptStr;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use openssl::asn1::{Asn1Integer, Asn1Time};
use openssl::bn::{BigNum, MsbOption};
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::x509::extension::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName,
};
use openssl::x509::{X509NameBuilder, X509};

use crate::sysmetrics::json_str;

/// Most entries one folder listing returns.
const MAX_ENTRIES: usize = 5000;
/// Largest PEM text accepted from the page.
const MAX_PEM: usize = 1 << 20;
/// Days a self-signed certificate is valid for.
const SELF_SIGNED_DAYS: u32 = 825;

pub const HTTPS_DIR: &str = "https";
pub const CA_DIR: &str = "certs";

fn obj(out: &mut String, fields: &[(&str, Field<'_>)]) {
    out.push('{');
    for (i, (name, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(out, name);
        out.push(':');
        match value {
            Field::Str(s) => json_str(out, s),
            Field::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Field::Num(n) => {
                let _ = write!(out, "{n}");
            }
            Field::Null => out.push_str("null"),
            Field::Raw(r) => out.push_str(r),
        }
    }
    out.push('}');
}

enum Field<'a> {
    Str(&'a str),
    Bool(bool),
    Num(u64),
    Null,
    Raw(&'a str),
}

fn error_json(message: &str) -> String {
    let mut out = String::new();
    obj(&mut out, &[("Error", Field::Str(message))]);
    out
}

fn readable(path: &Path, dir: bool) -> bool {
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    let mode = if dir { libc::R_OK | libc::X_OK } else { libc::R_OK };
    // SAFETY: a valid NUL-terminated path for the duration of the call.
    unsafe { libc::access(c.as_ptr(), mode) == 0 }
}

/// Lists one folder as the daemon user sees it. An empty path starts at `start`.
pub fn browse(path: &str, start: &Path) -> String {
    let requested = if path.trim().is_empty() {
        start.to_path_buf()
    } else {
        PathBuf::from(path.trim())
    };
    let mut dir = match fs::canonicalize(&requested) {
        Ok(p) => p,
        Err(e) => return browse_error(&requested, &e.to_string()),
    };
    if !dir.is_dir() {
        // a file was named: list its folder
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return browse_error(&requested, "not a folder"),
        }
    }
    let reader = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) => return browse_error(&dir, &e.to_string()),
    };
    let mut entries: Vec<(bool, String, u64, bool)> = Vec::new();
    let mut truncated = false;
    for entry in reader.flatten() {
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let full = entry.path();
        // follow links, so a link to a folder can be opened
        let meta = fs::metadata(&full).or_else(|_| entry.metadata());
        let (is_dir, size) = meta.map_or((false, 0), |m| (m.is_dir(), if m.is_dir() { 0 } else { m.len() }));
        entries.push((is_dir, name, size, readable(&full, is_dir)));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));

    let mut list = String::from("[");
    for (i, (is_dir, name, size, ok)) in entries.iter().enumerate() {
        if i > 0 {
            list.push(',');
        }
        obj(
            &mut list,
            &[
                ("Name", Field::Str(name)),
                ("Dir", Field::Bool(*is_dir)),
                ("Size", Field::Num(*size)),
                ("Readable", Field::Bool(*ok)),
            ],
        );
    }
    list.push(']');
    let shown = dir.to_string_lossy();
    let parent = dir.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    obj(
        &mut out,
        &[
            ("Path", Field::Str(&shown)),
            ("Parent", Field::Str(&parent)),
            ("Entries", Field::Raw(&list)),
            ("Truncated", Field::Bool(truncated)),
            ("Error", Field::Str("")),
        ],
    );
    out
}

fn browse_error(path: &Path, message: &str) -> String {
    let shown = path.to_string_lossy();
    let parent = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    obj(
        &mut out,
        &[
            ("Path", Field::Str(&shown)),
            ("Parent", Field::Str(&parent)),
            ("Entries", Field::Raw("[]")),
            ("Truncated", Field::Bool(false)),
            ("Error", Field::Str(message)),
        ],
    );
    out
}

/// Reads at most `MAX_PEM` bytes of a regular file. The file is opened without
/// blocking and checked through its descriptor, so a FIFO or a device such as
/// /dev/zero is refused instead of hanging or filling memory.
fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let shown = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NONBLOCK);
    }
    let file = opts.open(path).map_err(|e| shown(&e))?;
    let meta = file.metadata().map_err(|e| shown(&e))?;
    if !meta.is_file() {
        return Err(shown(&"not a regular file"));
    }
    let mut data = Vec::new();
    file.take(MAX_PEM as u64 + 1).read_to_end(&mut data).map_err(|e| shown(&e))?;
    if data.len() > MAX_PEM {
        return Err(shown(&"file is too large to be a certificate"));
    }
    Ok(data)
}

fn cert_names(cert: &X509) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(sans) = cert.subject_alt_names() {
        for san in &sans {
            if let Some(dns) = san.dnsname() {
                names.push(dns.to_string());
            } else if let Some(ip) = san.ipaddress() {
                let addr = match ip.len() {
                    4 => <[u8; 4]>::try_from(ip).ok().map(IpAddr::from),
                    16 => <[u8; 16]>::try_from(ip).ok().map(IpAddr::from),
                    _ => None,
                };
                if let Some(addr) = addr {
                    names.push(addr.to_string());
                }
            }
        }
    }
    names
}

fn subject_cn(cert: &X509) -> String {
    cert.subject_name()
        .entries_by_nid(openssl::nid::Nid::COMMONNAME)
        .next()
        .and_then(|e| e.data().to_string().ok())
        .unwrap_or_default()
}

/// Reads a certificate (and its chain) and, when given, the private key, and says
/// whether they belong together, who the certificate is for and when it expires.
pub fn check_cert(cert_path: &str, key_path: &str) -> String {
    let cert_path = cert_path.trim();
    let key_path = key_path.trim();
    if cert_path.is_empty() {
        return error_json("No certificate file is set.");
    }
    let pem = match read_limited(Path::new(cert_path)) {
        Ok(p) => p,
        Err(e) => return error_json(&e),
    };
    let chain = match X509::stack_from_pem(&pem) {
        Ok(c) if !c.is_empty() => c,
        _ => return error_json("The certificate file holds no PEM certificate."),
    };
    let leaf = &chain[0];
    let key_matches = if key_path.is_empty() {
        None
    } else {
        let key_pem = match read_limited(Path::new(key_path)) {
            Ok(p) => p,
            Err(e) => return error_json(&e),
        };
        let Ok(key) = PKey::private_key_from_pem(&key_pem) else {
            return error_json("The key file holds no unencrypted PEM private key.");
        };
        Some(leaf.public_key().is_ok_and(|public| public.public_eq(&key)))
    };
    let now = Asn1Time::days_from_now(0).ok();
    let expired = now.as_ref().is_some_and(|n| leaf.not_after() < n);
    let not_yet = now.as_ref().is_some_and(|n| leaf.not_before() > n);
    let self_signed = matches!(leaf.subject_name().try_cmp(leaf.issuer_name()), Ok(std::cmp::Ordering::Equal))
        && leaf.public_key().is_ok_and(|public| leaf.verify(&public).unwrap_or(false));
    let mut names = String::from("[");
    for (i, n) in cert_names(leaf).iter().enumerate() {
        if i > 0 {
            names.push(',');
        }
        json_str(&mut names, n);
    }
    names.push(']');
    let subject = subject_cn(leaf);
    let not_after = leaf.not_after().to_string();
    let ok = !expired && !not_yet && key_matches != Some(false);
    let mut out = String::new();
    obj(
        &mut out,
        &[
            ("Ok", Field::Bool(ok)),
            ("Subject", Field::Str(&subject)),
            ("Names", Field::Raw(&names)),
            ("NotAfter", Field::Str(&not_after)),
            ("Expired", Field::Bool(expired)),
            ("NotYetValid", Field::Bool(not_yet)),
            ("SelfSigned", Field::Bool(self_signed)),
            ("Chain", Field::Num(chain.len() as u64)),
            ("KeyMatches", key_matches.map_or(Field::Null, Field::Bool)),
            ("Error", Field::Str("")),
        ],
    );
    out
}

fn count_certs(path: &Path) -> usize {
    read_limited(path)
        .ok()
        .and_then(|pem| X509::stack_from_pem(&pem).ok())
        .map_or(0, |c| c.len())
}

/// Counts the certificates in a CA store, one PEM file, the way the TLS code loads it.
/// Without a store, news server certificates are not checked, which is an error here.
pub fn check_ca_store(path: &str) -> String {
    let path = path.trim();
    let mut out = String::new();
    if path.is_empty() {
        obj(
            &mut out,
            &[
                ("Count", Field::Num(0)),
                ("System", Field::Bool(false)),
                (
                    "Error",
                    Field::Str("No CA certificates are set, so news server certificates are not checked. Upload or paste a PEM file."),
                ),
            ],
        );
        return out;
    }
    let p = Path::new(path);
    let meta = match fs::metadata(p) {
        Ok(m) => m,
        Err(e) => return error_json(&format!("{path}: {e}")),
    };
    // the TLS code loads CertStore as one file, so a folder would fail there
    if meta.is_dir() {
        return error_json(&format!("{path} is a folder. Pick one PEM file that holds the CA certificates."));
    }
    let (count, files) = {
        let n = count_certs(p);
        (n, usize::from(n > 0))
    };
    if count == 0 {
        return error_json(&format!("{path}: no PEM certificates found"));
    }
    obj(
        &mut out,
        &[
            ("Count", Field::Num(count as u64)),
            ("Files", Field::Num(files as u64)),
            ("System", Field::Bool(false)),
            ("Error", Field::Str("")),
        ],
    );
    out
}

/// Writes a new certificate and/or key as `<base>-YYYYMMDD-HHMMSS.crt/.key` (then `-2`,
/// `-3`... when taken), so a pair in use is never overwritten: Settings switches to the new
/// one on save. Each file is created exclusively, so two requests at once can't share a
/// name; if a write fails, neither new file is left.
fn write_new_pair(
    dir: &Path,
    base: &str,
    cert: Option<&[u8]>,
    key: Option<&[u8]>,
) -> Result<(Option<PathBuf>, Option<PathBuf>), String> {
    let files: Vec<(&str, &[u8], bool)> = [cert.map(|c| ("crt", c, false)), key.map(|k| ("key", k, true))]
        .into_iter()
        .flatten()
        .collect();
    let mut paths = write_new_set(dir, base, &files)?.into_iter();
    let cert_path = cert.and_then(|_| paths.next());
    let key_path = key.and_then(|_| paths.next());
    Ok((cert_path, key_path))
}

/// Writes each `(extension, data, private)` file as `<base>-YYYYMMDD-HHMMSS.<ext>` in `dir`,
/// all with one stem (`-2`, `-3`... when any name is taken). Files are created exclusively
/// and fully written before their paths are returned; on failure none are left.
fn write_new_set(dir: &Path, base: &str, files: &[(&str, &[u8], bool)]) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = crate::statmeter::days_to_ymd(now.div_euclid(86_400));
    let secs = now.rem_euclid(86_400);
    let stamp = format!("{base}-{y:04}{m:02}{d:02}-{:02}{:02}{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    'stems: for n in 1..1000 {
        let stem = if n == 1 { stamp.clone() } else { format!("{stamp}-{n}") };
        let mut made: Vec<PathBuf> = Vec::new();
        for (ext, data, private) in files {
            let path = dir.join(format!("{stem}.{ext}"));
            match write_new_file(&path, data, *private) {
                Ok(()) => made.push(path),
                Err(e) => {
                    for p in &made {
                        let _ = fs::remove_file(p);
                    }
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        continue 'stems;
                    }
                    return Err(format!("{}: {e}", path.display()));
                }
            }
        }
        return Ok(made);
    }
    Err(format!("{}: no free name for a new certificate", dir.display()))
}

/// Creates `path`, failing if it exists; a partly written file is removed.
fn write_new_file(path: &Path, data: &[u8], private: bool) -> std::io::Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(if private { 0o600 } else { 0o644 });
    }
    let mut f = opts.open(path)?;
    f.write_all(data).inspect_err(|_| {
        let _ = fs::remove_file(path);
    })
}

/// The host names and addresses a self-signed certificate should cover, from text
/// separated by commas, spaces or new lines.
pub fn parse_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for raw in text.split(|c: char| c == ',' || c.is_whitespace()) {
        let name = raw.trim().trim_start_matches('[').trim_end_matches(']').to_lowercase();
        if name.is_empty() || name.len() > 253 || names.contains(&name) {
            continue;
        }
        let valid = name.parse::<IpAddr>().is_ok()
            || name.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && (label == "*" || label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            });
        if valid {
            names.push(name);
        }
    }
    names
}

fn build_self_signed(names: &[String]) -> Result<(X509, PKey<Private>), openssl::error::ErrorStack> {
    let key = PKey::from_rsa(Rsa::generate(2048)?)?;
    let mut subject = X509NameBuilder::new()?;
    subject.append_entry_by_nid(openssl::nid::Nid::COMMONNAME, &names[0])?;
    subject.append_entry_by_nid(openssl::nid::Nid::ORGANIZATIONNAME, "RustyGet")?;
    let subject = subject.build();
    let mut builder = X509::builder()?;
    builder.set_version(2)?;
    let mut serial = BigNum::new()?;
    serial.rand(127, MsbOption::MAYBE_ZERO, false)?;
    let serial = Asn1Integer::from_bn(&serial)?;
    builder.set_serial_number(&serial)?;
    builder.set_subject_name(&subject)?;
    builder.set_issuer_name(&subject)?;
    builder.set_pubkey(&key)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(SELF_SIGNED_DAYS)?;
    builder.set_not_before(&not_before)?;
    builder.set_not_after(&not_after)?;
    builder.append_extension(BasicConstraints::new().critical().build()?)?;
    builder.append_extension(KeyUsage::new().critical().digital_signature().key_encipherment().build()?)?;
    builder.append_extension(ExtendedKeyUsage::new().server_auth().build()?)?;
    let mut san = SubjectAlternativeName::new();
    for name in names {
        if name.parse::<IpAddr>().is_ok() {
            san.ip(name);
        } else {
            san.dns(name);
        }
    }
    let san = san.build(&builder.x509v3_context(None, None))?;
    builder.append_extension(san)?;
    builder.sign(&key, MessageDigest::sha256())?;
    Ok((builder.build(), key))
}

/// Makes a self-signed certificate for `names` and writes it with its key to
/// `MainDir/https/`. Answers with the two paths and the check of the new pair.
pub fn make_cert(names_text: &str, main_dir: &Path) -> String {
    let names = parse_names(names_text);
    if names.is_empty() {
        return error_json("Give at least one host name or address for the certificate.");
    }
    let (cert, key) = match build_self_signed(&names) {
        Ok(pair) => pair,
        Err(e) => return error_json(&format!("Could not make the certificate: {e}")),
    };
    let dir = main_dir.join(HTTPS_DIR);
    let pems = cert
        .to_pem()
        .and_then(|c| key.private_key_to_pem_pkcs8().map(|k| (c, k)))
        .map_err(|e| e.to_string());
    match pems.and_then(|(c, k)| write_new_pair(&dir, "rustyget", Some(&c), Some(&k))) {
        Ok((cert_path, key_path)) => saved_reply(cert_path.as_deref(), key_path.as_deref()),
        Err(e) => error_json(&e),
    }
}

fn saved_reply(cert: Option<&Path>, key: Option<&Path>) -> String {
    let cert_s = cert.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let key_s = key.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let check = if cert.is_some() { check_cert(&cert_s, &key_s) } else { String::from("null") };
    let mut out = String::new();
    obj(
        &mut out,
        &[
            ("Cert", Field::Str(&cert_s)),
            ("Key", Field::Str(&key_s)),
            ("Check", Field::Raw(&check)),
            ("Error", Field::Str("")),
        ],
    );
    out
}

/// Splits pasted PEM text into its certificates and its private key.
fn split_pem(text: &str) -> (Vec<X509>, Option<PKey<Private>>) {
    let mut certs = Vec::new();
    let mut key = None;
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN ") {
        let block = &rest[start..];
        let Some(label_end) = block[11..].find("-----") else { break };
        let label = &block[11..11 + label_end];
        let end_marker = format!("-----END {label}-----");
        let Some(end) = block.find(&end_marker) else { break };
        let pem = &block[..end + end_marker.len()];
        if label == "CERTIFICATE" || label == "TRUSTED CERTIFICATE" {
            if let Ok(c) = X509::from_pem(pem.as_bytes()) {
                certs.push(c);
            }
        } else if label.ends_with("PRIVATE KEY") && key.is_none() {
            key = PKey::private_key_from_pem(pem.as_bytes()).ok();
        }
        rest = &block[end + end_marker.len()..];
    }
    (certs, key)
}

fn safe_name(name: &str) -> String {
    let stem: String = name
        .trim()
        .trim_end_matches(".pem")
        .trim_end_matches(".crt")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let stem = stem.trim_matches(|c| c == '.' || c == '_').chars().take(64).collect::<String>();
    if stem.is_empty() { String::from("ca") } else { stem }
}

/// Saves pasted or uploaded PEM text under `MainDir`.
/// `kind` "https": a certificate and/or private key for the web server, written to
/// a new `https/server-YYYYMMDD-HHMMSS.crt` and `.key`. `kind` "ca": certificates for checking
/// news servers, written to a new `certs/<name>-YYYYMMDD-HHMMSS.pem`.
pub fn save_pem(kind: &str, name: &str, text: &str, main_dir: &Path) -> String {
    if text.len() > MAX_PEM {
        return error_json("The text is too large to be a certificate.");
    }
    let (certs, key) = split_pem(text);
    match kind {
        "https" => {
            if certs.is_empty() && key.is_none() {
                return error_json("No PEM certificate or private key was found in the text.");
            }
            if let (Some(c), Some(k)) = (certs.first(), key.as_ref()) {
                if !c.public_key().is_ok_and(|p| p.public_eq(k)) {
                    return error_json("The private key does not belong to the certificate.");
                }
            }
            let dir = main_dir.join(HTTPS_DIR);
            let mut cert_pem = Vec::new();
            for c in &certs {
                match c.to_pem() {
                    Ok(b) => cert_pem.extend_from_slice(&b),
                    Err(e) => return error_json(&e.to_string()),
                }
            }
            let key_pem = match key.as_ref().map(|k| k.private_key_to_pem_pkcs8()).transpose() {
                Ok(k) => k,
                Err(e) => return error_json(&e.to_string()),
            };
            let cert_arg = (!certs.is_empty()).then_some(&cert_pem[..]);
            let (cert_path, key_path) = match write_new_pair(&dir, "server", cert_arg, key_pem.as_deref()) {
                Ok(paths) => paths,
                Err(e) => return error_json(&e),
            };
            saved_reply(cert_path.as_deref(), key_path.as_deref())
        }
        "ca" => {
            if certs.is_empty() {
                return error_json("No PEM certificate was found in the text.");
            }
            let mut pem = Vec::new();
            for c in &certs {
                match c.to_pem() {
                    Ok(b) => pem.extend_from_slice(&b),
                    Err(e) => return error_json(&e.to_string()),
                }
            }
            // A fresh file each time: the bundle CertStore points at now stays as it is
            // until Settings is saved with the new path.
            let path = match write_new_set(&main_dir.join(CA_DIR), &safe_name(name), &[("pem", &pem, false)]) {
                Ok(mut paths) => paths.remove(0),
                Err(e) => return error_json(&e),
            };
            let shown = path.to_string_lossy();
            let mut out = String::new();
            obj(
                &mut out,
                &[
                    ("Path", Field::Str(&shown)),
                    ("Count", Field::Num(certs.len() as u64)),
                    ("Error", Field::Str("")),
                ],
            );
            out
        }
        _ => error_json("Unknown kind."),
    }
}

fn main_dir() -> PathBuf {
    crate::options::active_options()
        .map(|o| PathBuf::from(String::from_utf8_lossy(o.str_bytes(OptStr::MainDir)).into_owned()))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn reply(is_json: bool, json: String) -> Vec<u8> {
    if is_json {
        json.into_bytes()
    } else {
        crate::sysmetrics::to_xml(&json).into_bytes()
    }
}

/// Paths in Settings may name `${MainDir}`, `${AppDir}` or any other option, as the
/// configuration file does (Docker's CertStore is `${AppDir}/cacert.pem`); they are read
/// with the running options' values, and `main_dir` for MainDir.
pub fn expand_main_dir(path: &str, main_dir: &Path) -> String {
    // `~/` first, then `${...}`, in the order the configuration loader uses
    let homed;
    let path = if path.starts_with("~/") {
        homed = std::ffi::CString::new(path)
            .map(|c| String::from_utf8_lossy(&crate::fsops::expand_home_path_auto(Some(&c))).into_owned())
            .unwrap_or_else(|_| path.to_owned());
        homed.as_str()
    } else {
        path
    };
    let main = main_dir.to_string_lossy().into_owned();
    let running = crate::options::active_options();
    let out = crate::options::expand_option_variables(path.as_bytes(), |name| {
        if name.to_bytes().eq_ignore_ascii_case(b"MainDir") {
            return Some(main.as_bytes().to_vec());
        }
        let mut found = None;
        if let Some(options) = running.as_ref() {
            options.for_each_option(|opt, value, _| {
                if found.is_none() && opt.to_bytes().eq_ignore_ascii_case(name.to_bytes()) {
                    found = Some(value.to_bytes().to_vec());
                }
            });
        }
        found
    });
    String::from_utf8_lossy(&out).into_owned()
}

/// `browsefiles(path)`
pub fn rpc_browse(is_json: bool, path: &str) -> Vec<u8> {
    let main = main_dir();
    reply(is_json, browse(&expand_main_dir(path, &main), &main))
}

/// `checkcert(certfile, keyfile)`
pub fn rpc_check_cert(is_json: bool, cert: &str, key: &str) -> Vec<u8> {
    let main = main_dir();
    reply(is_json, check_cert(&expand_main_dir(cert, &main), &expand_main_dir(key, &main)))
}

/// `checkcastore(path)`
pub fn rpc_check_ca_store(is_json: bool, path: &str) -> Vec<u8> {
    reply(is_json, check_ca_store(&expand_main_dir(path, &main_dir())))
}

/// `makecert(names)`
pub fn rpc_make_cert(is_json: bool, names: &str) -> Vec<u8> {
    reply(is_json, make_cert(names, &main_dir()))
}

/// `savepem(kind, name, text)`
pub fn rpc_save_pem(is_json: bool, kind: &str, name: &str, text: &str) -> Vec<u8> {
    reply(is_json, save_pem(kind, name, text, &main_dir()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(reply: &str, name: &str) -> PathBuf {
        let start = reply.find(&format!("\"{name}\":\"")).unwrap() + name.len() + 4;
        PathBuf::from(&reply[start..start + reply[start..].find('"').unwrap()])
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rg-certtools-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn main_dir_is_expanded_in_paths() {
        assert_eq!(expand_main_dir("${MainDir}/https/a.crt", Path::new("/data")), "/data/https/a.crt");
        assert_eq!(expand_main_dir("${maindir}/x", Path::new("/d")), "/d/x");
        if let Some(home) = std::env::var_os("HOME") {
            let want = format!("{}/https/a.crt", home.to_string_lossy().trim_end_matches('/'));
            assert_eq!(expand_main_dir("~/https/a.crt", Path::new("/d")), want);
        }
        assert_eq!(expand_main_dir("/etc/ssl/${Other}/x", Path::new("/d")), "/etc/ssl/${Other}/x");
    }

    #[test]
    fn names_are_split_checked_and_deduplicated() {
        assert_eq!(
            parse_names("NAS.local, 192.168.1.5\n nas.local [::1] bad_name -x-.example *.lan"),
            vec!["nas.local", "192.168.1.5", "::1", "-x-.example", "*.lan"]
        );
        assert!(parse_names(" , ").is_empty());
    }

    #[test]
    fn a_self_signed_certificate_covers_its_names_and_matches_its_key() {
        let dir = temp_dir("make");
        let reply = make_cert("nas.local 10.0.0.2", &dir);
        assert!(reply.contains("\"Error\":\"\""), "{reply}");
        let cert = field(&reply, "Cert");
        let key = field(&reply, "Key");
        assert!(cert.starts_with(dir.join("https")) && cert.to_str().unwrap().contains("/rustyget-"), "{reply}");
        assert_eq!(cert.with_extension("key"), key);
        // making another leaves the first pair as it was
        let first = fs::read(&cert).unwrap();
        let again = make_cert("other.local", &dir);
        assert_ne!(field(&again, "Cert"), cert, "{again}");
        assert_eq!(fs::read(&cert).unwrap(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let check = check_cert(cert.to_str().unwrap(), key.to_str().unwrap());
        assert!(check.contains("\"Ok\":true"), "{check}");
        assert!(check.contains("\"Names\":[\"nas.local\",\"10.0.0.2\"]"), "{check}");
        assert!(check.contains("\"KeyMatches\":true"), "{check}");
        assert!(check.contains("\"SelfSigned\":true"), "{check}");
        assert!(check.contains("\"Subject\":\"nas.local\""), "{check}");
        assert!(make_cert("  ", &dir).contains("at least one host name"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pairs_made_at_once_get_their_own_names() {
        let dir = temp_dir("race");
        let replies: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..6).map(|i| {
                let dir = &dir;
                scope.spawn(move || make_cert(&format!("n{i}.local"), dir))
            }).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let mut certs: Vec<PathBuf> = replies.iter().map(|r| field(r, "Cert")).collect();
        for r in &replies {
            assert!(r.contains("\"KeyMatches\":true"), "{r}");
        }
        certs.sort();
        certs.dedup();
        assert_eq!(certs.len(), 6, "{replies:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_key_from_another_pair_does_not_match() {
        let a = temp_dir("pair-a");
        let b = temp_dir("pair-b");
        let ra = make_cert("a.local", &a);
        let rb = make_cert("b.local", &b);
        let check = check_cert(field(&ra, "Cert").to_str().unwrap(), field(&rb, "Key").to_str().unwrap());
        assert!(check.contains("\"KeyMatches\":false") && check.contains("\"Ok\":false"), "{check}");
        let missing = check_cert(a.join("nope.crt").to_str().unwrap(), "");
        assert!(missing.contains("nope.crt"), "{missing}");
        fs::remove_dir_all(&a).unwrap();
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn pasted_text_with_both_parts_is_split_into_cert_and_key() {
        let src = temp_dir("paste-src");
        let made = make_cert("paste.local", &src);
        let cert = fs::read_to_string(field(&made, "Cert")).unwrap();
        let key = fs::read_to_string(field(&made, "Key")).unwrap();
        let dir = temp_dir("paste");
        let reply = save_pem("https", "", &format!("junk\n{key}\nmore\n{cert}"), &dir);
        assert!(reply.contains("/server-") && reply.contains(".crt") && reply.contains(".key"), "{reply}");
        assert!(reply.contains("\"KeyMatches\":true"), "{reply}");
        let only_cert = save_pem("https", "", &cert, &dir);
        assert!(only_cert.contains("\"Key\":\"\""), "{only_cert}");
        let other = temp_dir("paste-other");
        let other_made = make_cert("other.local", &other);
        let other_key = fs::read_to_string(field(&other_made, "Key")).unwrap();
        assert!(save_pem("https", "", &format!("{cert}{other_key}"), &dir).contains("does not belong"));
        assert!(save_pem("https", "", "hello", &dir).contains("No PEM"));
        for d in [src, dir, other] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn a_ca_store_counts_its_certificates() {
        let src = temp_dir("ca-src");
        let made = make_cert("ca1.local", &src);
        let cert = fs::read_to_string(field(&made, "Cert")).unwrap();
        let dir = temp_dir("ca");
        let saved = save_pem("ca", "../My CA.pem", &format!("{cert}{cert}"), &dir);
        assert!(saved.contains("certs/My_CA-") && saved.contains("\"Count\":2"), "{saved}");
        let again = save_pem("ca", "../My CA.pem", &cert, &dir);
        assert_ne!(field(&saved, "Path"), field(&again, "Path"), "a new paste never replaces the bundle in use");
        assert!(check_ca_store(field(&saved, "Path").to_str().unwrap()).contains("\"Count\":2"));
        let store = dir.join("certs");
        let folder = check_ca_store(store.to_str().unwrap());
        assert!(folder.contains("is a folder"), "the TLS code loads one file: {folder}");
        let file = check_ca_store(field(&saved, "Path").to_str().unwrap());
        assert!(file.contains("\"Count\":2") && file.contains("\"Files\":1"), "{file}");
        let none = check_ca_store("");
        assert!(none.contains("\"System\":false") && none.contains("not checked"), "{none}");
        let not_pem = check_ca_store(field(&made, "Key").to_str().unwrap());
        assert!(not_pem.contains("no PEM certificates"), "{not_pem}");
        #[cfg(unix)]
        {
            // a device or FIFO is refused rather than read without end or waited on
            assert!(check_cert("/dev/zero", "").contains("not a regular file"));
            let fifo = dir.join("pipe");
            let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            assert!(check_cert(fifo.to_str().unwrap(), "").contains("not a regular file"));
        }
        assert!(save_pem("ca", "x", "nothing", &dir).contains("No PEM"));
        fs::remove_dir_all(&src).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn browsing_lists_folders_first_and_reads_a_file_as_its_folder() {
        let dir = temp_dir("browse");
        fs::create_dir(dir.join("zeta")).unwrap();
        fs::write(dir.join("alpha.pem"), b"x").unwrap();
        let listing = browse(dir.to_str().unwrap(), Path::new("/"));
        let z = listing.find("\"zeta\"").unwrap();
        let a = listing.find("\"alpha.pem\"").unwrap();
        assert!(z < a, "folders first: {listing}");
        assert!(listing.contains("\"Size\":1"), "{listing}");
        let via_file = browse(dir.join("alpha.pem").to_str().unwrap(), Path::new("/"));
        assert!(via_file.contains("\"zeta\""), "{via_file}");
        let start = browse("", &dir);
        assert!(start.contains("\"alpha.pem\""), "{start}");
        let missing = browse(dir.join("none").to_str().unwrap(), Path::new("/"));
        assert!(missing.contains("\"Error\":\"No such file"), "{missing}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
