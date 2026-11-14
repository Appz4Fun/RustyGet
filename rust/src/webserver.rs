//! The web server's request handling decisions (WebProcessor): what a request
//! header line means, the URL's prefix and credentials, the credential check
//! and the authorized-IP list. They match the C++ code; header names compare
//! as the C library's strncasecmp and numbers parse as its atoi (current
//! locale). Reading the request and answering it stay C++.

use crate::bytes::{find, find_byte};
use crate::wildmask::{wild_match, Lower};

/// sizeof(m_authInfo) and TOKEN_SIZE of WebServer.h.
pub const AUTH_INFO_SIZE: usize = 256 + 1;
pub const TOKEN_SIZE: usize = 48 + 1;

fn prefix(line: &[u8], lit: &str) -> bool {
    line.len() >= lit.len() && line[..lit.len()].eq_ignore_ascii_case(lit.as_bytes())
}

/// What a header line (its last '\r' cut off) does in ParseHeaders.
#[derive(Debug, PartialEq)]
pub enum Header<'a> {
    ContentLength(i32),
    /// base64 credentials for m_authInfo
    Auth(&'a [u8]),
    /// credentials too long: an error, and the headers end
    AuthTooBig,
    AcceptEncoding {
        gzip: bool,
    },
    Origin(&'a [u8]),
    /// the Auth-Token cookie's value as copied (up to TOKEN_SIZE - 1 bytes)
    AuthToken(&'a [u8]),
    ForwardedFor(&'a [u8]),
    IfNoneMatch(&'a [u8]),
    KeepAlive,
    /// the empty line: the headers end
    End,
    Other,
}

/// ParseHeaders for one line; `auth_info_empty`: no credentials yet (only
/// then does "Authorization:" count, else the line goes on to the next tests).
fn auth(b64: &[u8]) -> Header<'_> {
    if b64.len() > AUTH_INFO_SIZE {
        Header::AuthTooBig
    } else {
        Header::Auth(b64)
    }
}

fn c_bytes(b: &[u8]) -> &[u8] {
    &b[..find_byte(b, 0).unwrap_or(b.len())]
}

pub fn header(buffer: &[u8], auth_info_empty: bool) -> Header<'_> {
    let line = c_bytes(buffer);
    // An unusual locale can make strncasecmp match at an early NUL. C++
    // still takes the value at the fixed offset, possibly beyond that NUL.
    // Keep the initialized buffer available, and bound every value read.
    let value = |at: usize| match line.get(at..) {
        Some(value) => value,
        None => c_bytes(&buffer[at.min(buffer.len())..]),
    };
    if prefix(line, "Content-Length: ") {
        Header::ContentLength(crate::feed::c_atoi_bytes(value(16)))
    } else if prefix(line, "Authorization: Basic ") && auth_info_empty {
        auth(value(21))
    } else if prefix(line, "X-Authorization: Basic ") {
        auth(value(23))
    } else if prefix(line, "Accept-Encoding: ") {
        Header::AcceptEncoding {
            gzip: find(line, b"gzip").is_some(),
        }
    } else if prefix(line, "Origin: ") {
        Header::Origin(value(8))
    } else if prefix(line, "Cookie: ") {
        match find(line, b"Auth-Token=").map(|k| &line[k + 11..]) {
            Some(tok) if !tok.is_empty() && tok[0] != b';' => {
                Header::AuthToken(&tok[..tok.len().min(TOKEN_SIZE - 1)])
            }
            _ => Header::Other,
        }
    } else if prefix(line, "X-Forwarded-For: ") {
        Header::ForwardedFor(value(17))
    } else if prefix(line, "If-None-Match: ") {
        Header::IfNoneMatch(value(15))
    } else if prefix(line, "Connection: keep-alive") {
        Header::KeepAlive
    } else if line.is_empty() {
        Header::End
    } else {
        Header::Other
    }
}

/// What ParseUrl makes of the request URL.
#[derive(Debug, PartialEq)]
pub enum Url {
    /// "/nzbget": a redirect to this location
    Redirect(Vec<u8>),
    /// the URL to dispatch, and credentials given in it (URL-decoded, as the
    /// C string they made: up to a NUL)
    Go { url: Vec<u8>, auth: Option<Vec<u8>> },
}

/// WebProcessor::ParseUrl.
pub fn parse_url(url: &[u8]) -> Url {
    // Like the header parser, preserve the original C string boundary even
    // when a bounded FFI input includes bytes after its terminator.
    let url = c_bytes(url);
    let url = if url.starts_with(b"/rustyget/") {
        &url[9..]
    } else if url.starts_with(b"/nzbget/") {
        &url[7..]
    } else {
        url
    };
    if url == b"/rustyget" || url == b"/nzbget" {
        // BString<1024>("%s/")
        let mut loc = [url, b"/"].concat();
        loc.truncate(1023);
        return Url::Redirect(loc);
    }
    // "/username:password/jsonrpc"
    let rest = url.get(1..).unwrap_or_default();
    let colon = find_byte(rest, b':');
    let slash = find_byte(rest, b'/');
    if let (Some(c), Some(s)) = (colon, slash) {
        if c < s {
            let mut auth = rest[..s.min(AUTH_INFO_SIZE - 1)].to_vec();
            let len = crate::text::url_decode(&mut auth);
            auth.truncate(len);
            if let Some(n) = find_byte(&auth, 0) {
                auth.truncate(n);
            }
            let url = url[1 + s..].to_vec();
            return Url::Go {
                url,
                auth: Some(auth),
            };
        }
    }
    Url::Go {
        url: url.to_vec(),
        auth: None,
    }
}

/// The options CheckCredentials compares with (null C strings as None).
pub struct Credentials<'a> {
    pub control_username: Option<&'a [u8]>,
    pub control_password: Option<&'a [u8]>,
    pub restricted_username: Option<&'a [u8]>,
    pub restricted_password: Option<&'a [u8]>,
    pub add_username: Option<&'a [u8]>,
    pub add_password: Option<&'a [u8]>,
    pub authorized_ip: Option<&'a [u8]>,
}

/// The result of CheckCredentials.
#[derive(Debug, PartialEq)]
pub struct Check {
    pub authorized: bool,
    /// 0 control, 1 restricted, 2 add (as EUserAccess); None: unchanged
    pub access: Option<i32>,
    /// where the C++ cut m_authInfo (its ':' became a NUL)
    pub auth_cut: Option<usize>,
    /// the "username or password invalid" warning
    pub warn: bool,
}

fn empty(s: Option<&[u8]>) -> bool {
    s.is_none_or(|v| v.is_empty())
}

/// strcmp(a, b) == 0 for C strings (a null one compares as the C++ would
/// crash; the C++ never passed one here: callers check EmptyStr first).
fn eq(a: &[u8], b: Option<&[u8]>) -> bool {
    b.is_some_and(|b| a == b)
}

/// WebProcessor::IsAuthorizedIp: the address matches one of the option's
/// wildcard patterns (',' or ';' separated).
pub fn is_authorized_ip(option: &[u8], remote: &[u8], lower: &Lower<'_>) -> bool {
    crate::util::tokens(option, b",;").any(|ip| wild_match(lower, ip, remote, None))
}

/// WebProcessor::CheckCredentials: `auth_info` as received (up to its NUL),
/// `auth_token` the cookie's, `server_tokens` the three access levels'.
pub fn check_credentials(
    o: &Credentials<'_>,
    remote: &[u8],
    auth_info: &[u8],
    auth_token: &[u8],
    server_tokens: [&[u8]; 3],
    lower: &Lower<'_>,
) -> Check {
    let mut r = Check {
        authorized: true,
        access: None,
        auth_cut: None,
        warn: false,
    };
    let ip_ok = !empty(o.authorized_ip)
        && is_authorized_ip(o.authorized_ip.unwrap_or_default(), remote, lower);
    if empty(o.control_password) || ip_ok {
        return r;
    }
    if auth_info.is_empty() {
        // the X-Auth-Token
        match server_tokens.iter().position(|t| *t == auth_token) {
            Some(j) => r.access = Some(j as i32),
            None => r.authorized = false,
        }
        return r;
    }
    // "username:password"
    let (user, pw) = match find_byte(auth_info, b':') {
        Some(k) => {
            r.auth_cut = Some(k);
            (&auth_info[..k], Some(&auth_info[k + 1..]))
        }
        None => (auth_info, None),
    };
    let pw_is = |p: Option<&[u8]>| pw.is_some_and(|pw| eq(pw, p));
    if (empty(o.control_username) || eq(user, o.control_username)) && pw_is(o.control_password) {
        r.access = Some(0);
    } else if !empty(o.restricted_username)
        && eq(user, o.restricted_username)
        && pw_is(o.restricted_password)
    {
        r.access = Some(1);
    } else if !empty(o.add_username) && eq(user, o.add_username) && pw_is(o.add_password) {
        r.access = Some(2);
    } else {
        r.authorized = false;
        r.warn = true;
    }
    r
}

fn truncate_bstring_1024(mut v: Vec<u8>) -> Vec<u8> {
    if v.len() > 1023 {
        v.truncate(1023);
    }
    v
}

/// Formats the HTTP 401 Unauthorized response header (`WebProcessor::SendAuthResponse`).
pub fn format_auth_response(form_auth: bool, keep_alive: bool, version: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(b"HTTP/1.1 401 Unauthorized\r\n");
    if !form_auth {
        out.extend_from_slice(b"WWW-Authenticate: Basic realm=\"RustyGet\"\r\n");
    }
    out.extend_from_slice(b"Connection: ");
    out.extend_from_slice(if keep_alive { b"keep-alive" } else { b"close" });
    out.extend_from_slice(
        b"\r\n\
Content-Type: text/plain\r\n\
Content-Length: 0\r\n\
Server: RustyGet-",
    );
    out.extend_from_slice(version);
    out.extend_from_slice(b"\r\n\r\n");
    truncate_bstring_1024(out)
}

/// Formats the HTTP 200 OK CORS preflight response header (`WebProcessor::SendOptionsResponse`).
pub fn format_options_response(keep_alive: bool, origin: &[u8], version: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(320);
    out.extend_from_slice(b"HTTP/1.1 200 OK\r\nConnection: ");
    out.extend_from_slice(if keep_alive { b"keep-alive" } else { b"close" });
    out.extend_from_slice(
        b"\r\n\
Content-Length: 0\r\n\
Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Origin: ",
    );
    out.extend_from_slice(origin);
    out.extend_from_slice(
        b"\r\n\
Access-Control-Allow-Credentials: true\r\n\
Access-Control-Max-Age: 86400\r\n\
Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
Server: RustyGet-",
    );
    out.extend_from_slice(version);
    out.extend_from_slice(b"\r\n\r\n");
    truncate_bstring_1024(out)
}

/// Formats the HTTP error response header and HTML body (`WebProcessor::SendErrorResponse`).
pub fn format_error_response(
    err_code: &[u8],
    keep_alive: bool,
    version: &[u8],
) -> (Vec<u8>, Vec<u8>) {
    let mut body = Vec::with_capacity(128);
    body.extend_from_slice(b"<html><head><title>");
    body.extend_from_slice(err_code);
    body.extend_from_slice(b"</title></head><body>Error: ");
    body.extend_from_slice(err_code);
    body.extend_from_slice(b"</body></html>");
    body = truncate_bstring_1024(body);

    let mut header = Vec::with_capacity(256);
    header.extend_from_slice(b"HTTP/1.1 ");
    header.extend_from_slice(err_code);
    header.extend_from_slice(b"\r\nConnection: ");
    header.extend_from_slice(if keep_alive { b"keep-alive" } else { b"close" });
    header.extend_from_slice(b"\r\nContent-Length: ");
    header.extend_from_slice((body.len() as i32).to_string().as_bytes());
    header.extend_from_slice(b"\r\nContent-Type: text/html\r\nServer: RustyGet-");
    header.extend_from_slice(version);
    header.extend_from_slice(b"\r\n\r\n");
    header = truncate_bstring_1024(header);

    (header, body)
}

/// Formats the HTTP 301 redirect response header (`WebProcessor::SendRedirectResponse`).
pub fn format_redirect_response(url: &[u8], keep_alive: bool, version: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(b"HTTP/1.1 301 Moved Permanently\r\nLocation: ");
    out.extend_from_slice(url);
    out.extend_from_slice(b"\r\nConnection: ");
    out.extend_from_slice(if keep_alive { b"keep-alive" } else { b"close" });
    out.extend_from_slice(b"\r\nContent-Length: 0\r\nServer: RustyGet-");
    out.extend_from_slice(version);
    out.extend_from_slice(b"\r\n\r\n");
    truncate_bstring_1024(out)
}

/// Formats the HTTP response header for `WebProcessor::SendBodyResponse`.
#[allow(clippy::too_many_arguments)]
pub fn format_body_response_header(
    unchanged: bool,
    keep_alive: bool,
    origin: &[u8],
    form_auth: bool,
    auth_token: &[u8],
    body_len: i32,
    content_type: Option<&[u8]>,
    gzip: bool,
    etag: Option<&[u8]>,
    version: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    out.extend_from_slice(if unchanged {
        b"HTTP/1.1 304 Not Modified\r\n"
    } else {
        b"HTTP/1.1 200 OK\r\n"
    });
    out.extend_from_slice(b"Connection: ");
    out.extend_from_slice(if keep_alive { b"keep-alive" } else { b"close" });
    out.extend_from_slice(
        b"\r\n\
Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Origin: ",
    );
    out.extend_from_slice(origin);
    out.extend_from_slice(
        b"\r\n\
Access-Control-Allow-Credentials: true\r\n\
Access-Control-Max-Age: 86400\r\n\
Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
Set-Cookie: Auth-Type=",
    );
    out.extend_from_slice(if form_auth { b"form" } else { b"http" });
    out.extend_from_slice(b"; SameSite=Lax\r\nSet-Cookie: Auth-Token=");
    out.extend_from_slice(auth_token);
    out.extend_from_slice(b"; HttpOnly; SameSite=Lax\r\nContent-Length: ");
    out.extend_from_slice(body_len.to_string().as_bytes());
    out.extend_from_slice(b"\r\n");
    if let Some(ct) = content_type {
        let mut ct_hdr = Vec::with_capacity(ct.len() + 18);
        ct_hdr.extend_from_slice(b"Content-Type: ");
        ct_hdr.extend_from_slice(ct);
        ct_hdr.extend_from_slice(b"\r\n");
        let ct_hdr = truncate_bstring_1024(ct_hdr);
        out.extend_from_slice(&ct_hdr);
    }
    if gzip {
        out.extend_from_slice(b"Content-Encoding: gzip\r\n");
    }
    if let Some(et) = etag {
        let mut et_hdr = Vec::with_capacity(et.len() + 10);
        et_hdr.extend_from_slice(b"ETag: ");
        et_hdr.extend_from_slice(et);
        et_hdr.extend_from_slice(b"\r\n");
        let et_hdr = truncate_bstring_1024(et_hdr);
        out.extend_from_slice(&et_hdr);
    }
    out.extend_from_slice(b"Server: RustyGet-");
    out.extend_from_slice(version);
    out.extend_from_slice(b"\r\n\r\n");
    truncate_bstring_1024(out)
}

/// Validates URL characters in `WebProcessor::Dispatch`:
/// allows only `0..9 A..Z a..z . - + _ / ?` and rejects `..`.
pub fn web_is_safe_url_chars(url: &[u8]) -> bool {
    for i in 0..url.len() {
        let c = url[i];
        let allowed = c.is_ascii_alphanumeric()
            || c == b'.'
            || c == b'-'
            || c == b'+'
            || c == b'?'
            || c == b'_'
            || c == b'/';
        if !allowed || (c == b'.' && i + 1 < url.len() && url[i + 1] == b'.') {
            return false;
        }
    }
    true
}

/// Classifies static GET route in `WebProcessor::Dispatch`:
/// - `0`: invalid URL characters (404 Not Found with warning)
/// - `3`: `/combined.` with `?` (multi-file response)
/// - `4`: single-file response
pub fn web_classify_get_dispatch(url: &[u8]) -> i32 {
    if !web_is_safe_url_chars(url) {
        return 0;
    }
    if url.starts_with(b"/combined.") && url.contains(&b'?') {
        3
    } else {
        4
    }
}

/// Checks whether a missing static file 404 is ignorable in `WebProcessor::SendSingleFileResponse`.
pub fn web_is_ignorable_404(base_name: &[u8]) -> bool {
    base_name == b"package-info.json"
        || base_name == b"favicon.ico"
        || base_name.starts_with(b"apple-touch-icon")
}

/// Maps filename extension to MIME content-type in `WebProcessor::DetectContentType`.
/// Returns `None` if unknown or no extension.
pub fn web_detect_content_type(filename: &[u8]) -> Option<&'static [u8]> {
    let dot = filename.iter().rposition(|&b| b == b'.')?;
    let ext = &filename[dot..];
    if ext.eq_ignore_ascii_case(b".css") {
        Some(b"text/css\0")
    } else if ext.eq_ignore_ascii_case(b".html") {
        Some(b"text/html\0")
    } else if ext.eq_ignore_ascii_case(b".js") {
        Some(b"application/javascript\0")
    } else if ext.eq_ignore_ascii_case(b".png") {
        Some(b"image/png\0")
    } else if ext.eq_ignore_ascii_case(b".jpeg") {
        Some(b"image/jpeg\0")
    } else if ext.eq_ignore_ascii_case(b".gif") {
        Some(b"image/gif\0")
    } else if ext.eq_ignore_ascii_case(b".svg") {
        Some(b"image/svg+xml\0")
    } else if ext.eq_ignore_ascii_case(b".json") {
        Some(b"application/json\0")
    } else {
        None
    }
}

pub const HM_POST: i32 = 0;
pub const HM_GET: i32 = 1;
pub const HM_OPTIONS: i32 = 2;

const ERR_HTTP_BAD_REQUEST: &[u8] = b"400 Bad Request";
const ERR_HTTP_NOT_FOUND: &[u8] = b"404 Not Found";
const ERR_HTTP_SERVICE_UNAVAILABLE: &[u8] = b"503 Service Unavailable";
const MAX_UNCOMPRESSED_SIZE: usize = 500;

static SERVER_AUTH_TOKENS: std::sync::OnceLock<[[u8; 48]; 3]> = std::sync::OnceLock::new();

fn generate_tokens_from_urandom() -> Result<[[u8; 48]; 3], std::io::Error> {
    use std::io::Read;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut f = std::fs::File::open("/dev/urandom")?;
    let mut raw = [0u8; 72];
    f.read_exact(&mut raw)?;
    let mut out = [[0u8; 48]; 3];
    for (j, tok) in out.iter_mut().enumerate() {
        let slice = &raw[j * 24..(j + 1) * 24];
        for (i, &b) in slice.iter().enumerate() {
            tok[i * 2] = HEX[(b >> 4) as usize];
            tok[i * 2 + 1] = HEX[(b & 0x0f) as usize];
        }
    }
    Ok(out)
}

/// Initializes the three server session `Auth-Token` values (`WebProcessor::Init`).
pub fn init_server_tokens<F>(mut on_error: F) -> &'static [[u8; 48]; 3]
where
    F: FnMut(&[u8]),
{
    SERVER_AUTH_TOKENS.get_or_init(|| match generate_tokens_from_urandom() {
        Ok(tokens) => tokens,
        Err(e) => {
            let msg = format!("Could not generate secure authentication token: {e}");
            on_error(msg.as_bytes());
            std::process::exit(1);
        }
    })
}

/// Compresses `body` in gzip format when smaller than `body.len()` (`ZLib::GZip`).
pub fn gzip_compress_body(body: &[u8]) -> Option<Vec<u8>> {
    let n = body.len() as u32;
    let out_cap = n
        .saturating_add(n >> 12)
        .saturating_add(n >> 14)
        .saturating_add(n >> 25)
        .saturating_add(32) as usize;
    let mut out = vec![0u8; out_cap];
    let mut comp = flate2::Compress::new_gzip(flate2::Compression::default(), 15);
    match comp.compress(body, &mut out, flate2::FlushCompress::Finish) {
        Ok(flate2::Status::StreamEnd) => {
            let len = comp.total_out() as usize;
            if len > 0 && len < body.len() {
                out.truncate(len);
                Some(out)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(unix)]
fn load_file_bytes(path_bytes: &[u8]) -> Option<Vec<u8>> {
    if path_bytes.is_empty() || path_bytes.contains(&0) {
        return None;
    }
    let mut c_path = Vec::with_capacity(path_bytes.len() + 1);
    c_path.extend_from_slice(path_bytes);
    c_path.push(0);
    let cstr = std::ffi::CStr::from_bytes_with_nul(&c_path).ok()?;
    let mut buf = Vec::new();
    let ok = crate::fsops::load_file_into_buffer(cstr, true, &crate::fsops::posix_stat, |sz| {
        if sz <= 0 || buf.try_reserve_exact(sz as usize).is_err() {
            return std::ptr::null_mut();
        }
        buf.resize(sz as usize, 0);
        buf.as_mut_ptr()
    });
    if ok {
        Some(buf)
    } else {
        None
    }
}

fn compute_etag(body: &[u8]) -> Vec<u8> {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(body);
    format!("\"{:x}\"", hasher.finish()).into_bytes()
}

fn content_type_no_nul(filename: &[u8]) -> Option<&'static [u8]> {
    crate::binrpc::detect_content_type(filename).map(|c| c.to_bytes())
}

/// Configuration and host callbacks for serving one HTTP request (`RequestProcessor::ServWebRequest` + `WebProcessor`).
pub struct WebServeConfig<'a> {
    pub signature: &'a [u8],
    pub remote_addr: &'a [u8],
    pub version: &'a [u8],
    pub web_dir: &'a [u8],
    pub credentials: Credentials<'a>,
    pub control_port: i32,
    pub form_auth: bool,
    pub gzip_enabled: bool,
    pub debug_build: bool,
}

/// Serves a single HTTP request starting after the 4-byte `signature` (`"POST"`, `"GET "`, `"OPTI"`).
/// Returns `true` when the connection should be kept alive for another request (`WebProcessor::GetKeepAlive`).
#[cfg(unix)]
pub fn serve_web_request<RL, RV, SD, LG, RPC>(
    cfg: &WebServeConfig<'_>,
    lower: &Lower<'_>,
    mut read_line: RL,
    mut recv_exact: RV,
    mut send_all: SD,
    mut log_msg: LG,
    mut run_rpc: RPC,
) -> bool
where
    RL: FnMut(&mut [u8]) -> bool,
    RV: FnMut(&mut [u8]) -> bool,
    SD: FnMut(&[u8]) -> bool,
    LG: FnMut(i32, &[u8]),
    RPC: FnMut(&mut [u8], i32, i32, &[u8], &mut dyn FnMut(&[u8], Option<&[u8]>, bool)),
{
    let server_tokens_arr = init_server_tokens(|m| log_msg(2, m));
    let server_tokens: [&[u8]; 3] = [
        &server_tokens_arr[0],
        &server_tokens_arr[1],
        &server_tokens_arr[2],
    ];

    let mut req_line = [0u8; 1024];
    if !read_line(&mut req_line) {
        return false;
    }
    let Some((http_method, url_offset)) =
        crate::binrpc::parse_web_request_line(cfg.signature, &mut req_line)
    else {
        return false;
    };
    let mut url = c_bytes(&req_line[url_offset..]).to_vec();
    {
        let mut msg = Vec::with_capacity(5 + url.len());
        msg.extend_from_slice(b"url: ");
        msg.extend_from_slice(&url);
        log_msg(0, &msg);
    }

    let mut gzip_req = false;
    let mut user_access: usize = 0;
    let mut auth_info = [0u8; AUTH_INFO_SIZE];
    let mut auth_token = [0u8; TOKEN_SIZE];
    let mut origin: Vec<u8> = Vec::new();
    let mut forwarded_for: Vec<u8> = Vec::new();
    let mut old_etag: Vec<u8> = Vec::new();
    let mut keep_alive = false;
    let mut content_len: i32 = 0;

    // ParseHeaders: preserve the 1024-byte buffer across lines.
    {
        let mut hdr_buf = [0u8; 1024];
        let mut ended_early_err = false;
        while read_line(&mut hdr_buf) {
            let nul_pos = find_byte(&hdr_buf, 0).unwrap_or(hdr_buf.len());
            if let Some(cr) = hdr_buf[..nul_pos].iter().rposition(|&b| b == b'\r') {
                hdr_buf[cr] = 0;
            }
            {
                let line = c_bytes(&hdr_buf);
                let mut msg = Vec::with_capacity(7 + line.len());
                msg.extend_from_slice(b"header=");
                msg.extend_from_slice(line);
                log_msg(0, &msg);
            }

            match header(&hdr_buf, auth_info[0] == 0) {
                Header::ContentLength(n) => content_len = n,
                Header::Auth(b64) => {
                    let dec_len = unsafe {
                        crate::decode::base64_in_place(
                            b64.as_ptr(),
                            b64.len(),
                            auth_info.as_mut_ptr(),
                        )
                    };
                    auth_info[dec_len.min(AUTH_INFO_SIZE - 1)] = 0;
                }
                Header::AuthTooBig => {
                    log_msg(2, b"Invalid-request: auth-info too big");
                    ended_early_err = true;
                    break;
                }
                Header::AcceptEncoding { gzip } => gzip_req = gzip,
                Header::Origin(v) => origin = c_bytes(v).to_vec(),
                Header::AuthToken(v) => {
                    let v = c_bytes(v);
                    let n = v.len().min(TOKEN_SIZE - 1);
                    auth_token.fill(0);
                    auth_token[..n].copy_from_slice(&v[..n]);
                }
                Header::ForwardedFor(v) => forwarded_for = c_bytes(v).to_vec(),
                Header::IfNoneMatch(v) => old_etag = c_bytes(v).to_vec(),
                Header::KeepAlive => keep_alive = true,
                Header::End => break,
                Header::Other => {}
            }
        }
        if !ended_early_err {
            let mut msg = Vec::with_capacity(4 + url.len());
            msg.extend_from_slice(b"URL=");
            msg.extend_from_slice(&url);
            log_msg(0, &msg);
        }
    }

    let send_error = |err_code: &[u8],
                      print_warning: bool,
                      cur_url: &[u8],
                      ka: bool,
                      send: &mut SD,
                      log: &mut LG| {
        if print_warning {
            let mut w = Vec::with_capacity(24 + err_code.len() + cur_url.len());
            w.extend_from_slice(b"Web-Server: ");
            w.extend_from_slice(err_code);
            w.extend_from_slice(b", Resource: ");
            w.extend_from_slice(cur_url);
            log(1, &w);
        }
        let (hdr, body) = format_error_response(err_code, ka, cfg.version);
        let _ = send(&hdr);
        let _ = send(&body);
    };

    let send_auth = |ka: bool, send: &mut SD, log: &mut LG| {
        let hdr = format_auth_response(cfg.form_auth, ka, cfg.version);
        let mut msg = Vec::with_capacity(15 + hdr.len());
        msg.extend_from_slice(b"ResponseHeader=");
        msg.extend_from_slice(&hdr);
        log(0, &msg);
        let _ = send(&hdr);
    };

    if http_method == HM_POST && content_len <= 0 {
        log_msg(2, b"Invalid-request: content length is 0");
        send_error(
            ERR_HTTP_BAD_REQUEST,
            false,
            &url,
            false,
            &mut send_all,
            &mut log_msg,
        );
        return false;
    }

    if http_method == HM_OPTIONS {
        let hdr = format_options_response(keep_alive, &origin, cfg.version);
        let mut msg = Vec::with_capacity(15 + hdr.len());
        msg.extend_from_slice(b"ResponseHeader=");
        msg.extend_from_slice(&hdr);
        log_msg(0, &msg);
        let _ = send_all(&hdr);
        return keep_alive;
    }

    match parse_url(&url) {
        Url::Redirect(target) => {
            let hdr = format_redirect_response(c_bytes(&target), keep_alive, cfg.version);
            let mut msg = Vec::with_capacity(15 + hdr.len());
            msg.extend_from_slice(b"ResponseHeader=");
            msg.extend_from_slice(&hdr);
            log_msg(0, &msg);
            let _ = send_all(&hdr);
            return keep_alive;
        }
        Url::Go {
            url: target,
            auth: maybe_auth,
        } => {
            if let Some(a) = maybe_auth {
                let n = a.len().min(AUTH_INFO_SIZE - 1);
                auth_info.fill(0);
                auth_info[..n].copy_from_slice(&a[..n]);
            }
            url = c_bytes(&target).to_vec();
            let mut msg = Vec::with_capacity(10 + url.len());
            msg.extend_from_slice(b"Final URL=");
            msg.extend_from_slice(&url);
            log_msg(0, &msg);
        }
    }

    let rpc_request = crate::rpcroute::protocol(&url) != crate::rpcroute::RP_UNDEFINED;
    let check = check_credentials(
        &cfg.credentials,
        cfg.remote_addr,
        c_bytes(&auth_info),
        c_bytes(&auth_token),
        server_tokens,
        lower,
    );
    if let Some(cut) = check.auth_cut {
        if cut < auth_info.len() {
            auth_info[cut] = 0;
        }
    }
    if let Some(acc) = check.access {
        if (0..3).contains(&acc) {
            user_access = acc as usize;
        }
    }
    if check.warn {
        let user_part = c_bytes(&auth_info);
        let mut w = Vec::with_capacity(128);
        w.extend_from_slice(b"Request received on port ");
        w.extend_from_slice(cfg.control_port.to_string().as_bytes());
        w.extend_from_slice(b" from ");
        w.extend_from_slice(cfg.remote_addr);
        if !forwarded_for.is_empty() {
            w.extend_from_slice(b" (forwarded for: ");
            w.extend_from_slice(&forwarded_for);
            w.push(b')');
        }
        w.extend_from_slice(b", but username (");
        w.extend_from_slice(user_part);
        w.extend_from_slice(b") or password invalid");
        log_msg(1, &w);
    }
    let authorized = check.authorized;

    if (!cfg.form_auth || rpc_request) && !authorized {
        send_auth(keep_alive, &mut send_all, &mut log_msg);
        return keep_alive;
    }

    let mut request_body: Vec<u8> = Vec::new();
    if http_method == HM_POST {
        let max_len = if authorized {
            768 * 1024 * 1024
        } else {
            64 * 1024
        };
        if content_len < 0 || content_len > max_len {
            send_error(
                ERR_HTTP_BAD_REQUEST,
                true,
                &url,
                keep_alive,
                &mut send_all,
                &mut log_msg,
            );
            return keep_alive;
        }
        let body_len = content_len as usize;
        if request_body.try_reserve_exact(body_len + 1).is_err() {
            send_error(
                ERR_HTTP_SERVICE_UNAVAILABLE,
                true,
                &url,
                keep_alive,
                &mut send_all,
                &mut log_msg,
            );
            return keep_alive;
        }
        request_body.resize(body_len + 1, 0);
        if !recv_exact(&mut request_body[..body_len]) {
            log_msg(2, b"Invalid-request: could not read data");
            return false;
        }
        request_body[body_len] = 0;
        let req_str = c_bytes(&request_body);
        let mut msg = Vec::with_capacity(8 + req_str.len());
        msg.extend_from_slice(b"Request=");
        msg.extend_from_slice(req_str);
        log_msg(0, &msg);
    } else {
        request_body.push(0);
    }

    {
        let mut msg = Vec::with_capacity(22 + cfg.remote_addr.len());
        msg.extend_from_slice(b"request received from ");
        msg.extend_from_slice(cfg.remote_addr);
        log_msg(0, &msg);
    }

    // Dispatch
    if url.first() != Some(&b'/') {
        send_error(
            ERR_HTTP_BAD_REQUEST,
            true,
            &url,
            keep_alive,
            &mut send_all,
            &mut log_msg,
        );
        return keep_alive;
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_body_response<S, L>(
        cfg: &WebServeConfig<'_>,
        url: &[u8],
        old_etag: &[u8],
        origin: &[u8],
        token_out: &[u8],
        keep_alive: bool,
        gzip_req: bool,
        raw_body: &[u8],
        content_type: Option<&[u8]>,
        cachable: bool,
        send_all: &mut S,
        log_msg: &mut L,
    ) where
        S: FnMut(&[u8]) -> bool,
        L: FnMut(i32, &[u8]),
    {
        let mut etag_opt: Option<Vec<u8>> = None;
        let mut unchanged = false;
        let mut body_slice = raw_body;

        if cachable {
            let new_etag = compute_etag(body_slice);
            if !old_etag.is_empty() && old_etag == new_etag {
                unchanged = true;
                body_slice = b"";
            }
            etag_opt = Some(new_etag);
        }

        let compressed = if cfg.gzip_enabled && gzip_req && body_slice.len() > MAX_UNCOMPRESSED_SIZE
        {
            gzip_compress_body(body_slice)
        } else {
            None
        };
        let use_gzip = compressed.is_some();
        if let Some(ref gbuf) = compressed {
            body_slice = gbuf;
        }

        let hdr = format_body_response_header(
            unchanged,
            keep_alive,
            origin,
            cfg.form_auth,
            token_out,
            body_slice.len() as i32,
            content_type,
            use_gzip,
            etag_opt.as_deref(),
            cfg.version,
        );
        {
            let mut dbg = Vec::with_capacity(8 + url.len() + old_etag.len() + hdr.len());
            dbg.push(b'[');
            dbg.extend_from_slice(url);
            dbg.extend_from_slice(b"] (");
            dbg.extend_from_slice(old_etag);
            dbg.extend_from_slice(b") ");
            dbg.extend_from_slice(&hdr);
            log_msg(0, &dbg);
        }
        let _ = send_all(&hdr);
        let _ = send_all(body_slice);
    }

    let token_out: &[u8] = if authorized {
        server_tokens[user_access]
    } else {
        b""
    };

    if rpc_request {
        let mut url_c = Vec::with_capacity(url.len() + 1);
        url_c.extend_from_slice(&url);
        url_c.push(0);
        let mut send_rpc_body = |raw_body: &[u8], content_type: Option<&[u8]>, cachable: bool| {
            emit_body_response(
                cfg,
                &url,
                &old_etag,
                &origin,
                token_out,
                keep_alive,
                gzip_req,
                raw_body,
                content_type,
                cachable,
                &mut send_all,
                &mut log_msg,
            );
        };
        run_rpc(
            &mut request_body,
            http_method,
            user_access as i32,
            &url_c,
            &mut send_rpc_body,
        );
        return keep_alive;
    }

    if http_method != HM_GET {
        send_error(
            ERR_HTTP_BAD_REQUEST,
            true,
            &url,
            keep_alive,
            &mut send_all,
            &mut log_msg,
        );
        return keep_alive;
    }

    let get_route = web_classify_get_dispatch(&url);
    if get_route == 0 {
        send_error(
            ERR_HTTP_NOT_FOUND,
            true,
            &url,
            keep_alive,
            &mut send_all,
            &mut log_msg,
        );
        return keep_alive;
    }

    if cfg.web_dir.is_empty() {
        send_error(
            ERR_HTTP_SERVICE_UNAVAILABLE,
            true,
            &url,
            keep_alive,
            &mut send_all,
            &mut log_msg,
        );
        return keep_alive;
    }

    if get_route == 3 {
        // SendMultiFileResponse: "/combined.js?file1.js+file2.js"
        {
            let mut dbg = Vec::with_capacity(24 + url.len());
            dbg.extend_from_slice(b"serving multiple files: ");
            dbg.extend_from_slice(&url);
            log_msg(0, &dbg);
        }
        let Some(qpos) = find_byte(&url, b'?') else {
            send_error(
                ERR_HTTP_NOT_FOUND,
                false,
                &url,
                keep_alive,
                &mut send_all,
                &mut log_msg,
            );
            return keep_alive;
        };
        let base_url = url[..qpos].to_vec();
        let filelist = url[qpos + 1..].to_vec();
        // C++ mutated `*filelist++ = '\0'` in `m_url` before any error response!
        url = base_url;

        let mut combined: Vec<u8> = Vec::new();
        for part in crate::util::tokens(&filelist, b"+") {
            let mut disk_filename = Vec::with_capacity(cfg.web_dir.len() + 1 + part.len());
            disk_filename.extend_from_slice(cfg.web_dir);
            disk_filename.push(crate::paths::PATH_SEPARATOR);
            disk_filename.extend_from_slice(part);

            let Some(buf) = load_file_bytes(&disk_filename) else {
                let mut w = Vec::with_capacity(32 + part.len());
                w.extend_from_slice(b"Web-Server: ");
                w.extend_from_slice(ERR_HTTP_NOT_FOUND);
                w.extend_from_slice(b", Resource: /");
                w.extend_from_slice(part);
                log_msg(1, &w);
                send_error(
                    ERR_HTTP_NOT_FOUND,
                    false,
                    &url,
                    keep_alive,
                    &mut send_all,
                    &mut log_msg,
                );
                return keep_alive;
            };
            // StringBuilder::Append(const char*) stops at the first NUL byte.
            combined.extend_from_slice(c_bytes(&buf));
        }
        let ct = content_type_no_nul(&url);
        emit_body_response(
            cfg,
            &url,
            &old_etag,
            &origin,
            token_out,
            keep_alive,
            gzip_req,
            &combined,
            ct,
            true,
            &mut send_all,
            &mut log_msg,
        );
    } else {
        // SendSingleFileResponse
        let def_res: &[u8] = if url.last() == Some(&b'/') {
            b"index.html"
        } else {
            b""
        };
        let mut filename = Vec::with_capacity(cfg.web_dir.len() + url.len() + def_res.len());
        filename.extend_from_slice(cfg.web_dir);
        filename.extend_from_slice(&url);
        filename.extend_from_slice(def_res);

        {
            let mut dbg = Vec::with_capacity(14 + filename.len());
            dbg.extend_from_slice(b"serving file: ");
            dbg.extend_from_slice(&filename);
            log_msg(0, &dbg);
        }

        let Some(mut buf) = load_file_bytes(&filename) else {
            let base = &filename[crate::paths::base_file_name(&filename)..];
            let ignorable = web_is_ignorable_404(base);
            send_error(
                ERR_HTTP_NOT_FOUND,
                !ignorable,
                &url,
                keep_alive,
                &mut send_all,
                &mut log_msg,
            );
            return keep_alive;
        };

        let ct = content_type_no_nul(&filename);
        let mut len = buf.len().saturating_sub(1);
        if cfg.debug_build && ct == Some(b"text/html") {
            unsafe {
                let p = buf.as_mut_ptr().cast();
                crate::util::reduce_str(p, c"<!-- %if-debug%".as_ptr(), c"".as_ptr());
                crate::util::reduce_str(p, c"<!-- %if-not-debug% -->".as_ptr(), c"<!--".as_ptr());
                crate::util::reduce_str(p, c"<!-- %end% -->".as_ptr(), c"-->".as_ptr());
                crate::util::reduce_str(p, c"%end% -->".as_ptr(), c"".as_ptr());
            }
            len = c_bytes(&buf).len();
        }

        emit_body_response(
            cfg,
            &url,
            &old_etag,
            &origin,
            token_out,
            keep_alive,
            gzip_req,
            &buf[..len],
            ct,
            true,
            &mut send_all,
            &mut log_msg,
        );
    }

    keep_alive
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_int;

    fn ascii(b: u8) -> c_int {
        b.to_ascii_lowercase() as c_int
    }

    #[test]
    fn headers() {
        assert_eq!(
            header(b"content-length: 42", true),
            Header::ContentLength(42)
        );
        assert_eq!(
            header(b"Authorization: Basic YWJj", true),
            Header::Auth(b"YWJj")
        );
        assert_eq!(header(b"Authorization: Basic YWJj", false), Header::Other);
        assert_eq!(
            header(b"Cookie: a=1; Auth-Token=xyz; b=2", true),
            Header::AuthToken(b"xyz; b=2")
        );
        assert_eq!(header(b"Cookie: Auth-Token=;", true), Header::Other);
        assert_eq!(header(b"", true), Header::End);
    }

    #[test]
    fn urls() {
        assert_eq!(parse_url(b"/nzbget"), Url::Redirect(b"/nzbget/".to_vec()));
        assert_eq!(
            parse_url(b"/nzbget/u:p%40/jsonrpc"),
            Url::Go {
                url: b"/jsonrpc".to_vec(),
                auth: Some(b"u:p@".to_vec())
            }
        );
        assert_eq!(
            parse_url(b"/jsonrpc?x=a:b"),
            Url::Go {
                url: b"/jsonrpc?x=a:b".to_vec(),
                auth: None
            }
        );
        assert_eq!(
            parse_url(b"/nzbget\0/u:p/jsonrpc"),
            Url::Redirect(b"/nzbget/".to_vec())
        );
        assert_eq!(
            parse_url(b"/u:p\0/jsonrpc"),
            Url::Go {
                url: b"/u:p".to_vec(),
                auth: None
            }
        );
    }

    #[test]
    fn credentials() {
        let l = Lower::Fold(&ascii);
        let o = Credentials {
            control_username: Some(b"nzbget"),
            control_password: Some(b"secret"),
            restricted_username: None,
            restricted_password: None,
            add_username: None,
            add_password: None,
            authorized_ip: Some(b"192.168.1.*"),
        };
        let tokens: [&[u8]; 3] = [b"t0", b"t1", b"t2"];
        assert!(check_credentials(&o, b"192.168.1.93", b"", b"", tokens, &l).authorized);
        let c = check_credentials(&o, b"10.0.0.1", b"nzbget:secret", b"", tokens, &l);
        assert_eq!(
            (c.authorized, c.access, c.auth_cut),
            (true, Some(0), Some(6))
        );
        assert!(check_credentials(&o, b"10.0.0.1", b"nzbget:wrong", b"", tokens, &l).warn);
        assert_eq!(
            check_credentials(&o, b"10.0.0.1", b"", b"t2", tokens, &l).access,
            Some(2)
        );
    }

    #[test]
    fn response_headers() {
        let auth = format_auth_response(false, true, b"26.0");
        assert!(auth.starts_with(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"RustyGet\"\r\nConnection: keep-alive\r\n"));
        assert!(auth.ends_with(b"Server: RustyGet-26.0\r\n\r\n"));

        let (err_hdr, err_body) = format_error_response(b"404 Not Found", false, b"26.0");
        assert_eq!(
            err_body,
            b"<html><head><title>404 Not Found</title></head><body>Error: 404 Not Found</body></html>"
        );
        assert!(err_hdr
            .starts_with(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 87\r\n"));

        let body_hdr = format_body_response_header(
            false,
            true,
            b"*",
            true,
            b"tok123",
            42,
            Some(b"application/json"),
            true,
            Some(b"\"abc\""),
            b"26.0",
        );
        let text = String::from_utf8(body_hdr).unwrap();
        assert!(text.contains("Set-Cookie: Auth-Type=form; SameSite=Lax\r\n"));
        assert!(text.contains("Set-Cookie: Auth-Token=tok123; HttpOnly; SameSite=Lax\r\n"));
        assert!(text.contains(
            "Content-Type: application/json\r\nContent-Encoding: gzip\r\nETag: \"abc\"\r\n"
        ));
    }

    #[test]
    fn test_web_functions() {
        assert!(web_is_safe_url_chars(b"/index.html?a+1_2-3"));
        assert!(!web_is_safe_url_chars(b"/../secret"));
        assert_eq!(web_classify_get_dispatch(b"/extensions/myext/script.js"), 4);
        assert_eq!(web_classify_get_dispatch(b"/combined.js?a.js+b.js"), 3);
        assert!(web_is_ignorable_404(b"apple-touch-icon-120x120.png"));
        assert!(!web_is_ignorable_404(b"index.html"));
        assert_eq!(
            web_detect_content_type(b"app.CSS"),
            Some(&b"text/css\0"[..])
        );
    }

    #[cfg(unix)]
    #[test]
    fn serve_static_gzip_etag_and_rpc() {
        use std::io::Read;
        use std::os::unix::ffi::OsStrExt;

        struct TempGuard(std::path::PathBuf);
        impl Drop for TempGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp_root =
            std::env::temp_dir().join(format!("rustyget_web_test_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(&tmp_root).unwrap();
        let _tmp_guard = TempGuard(tmp_root.clone());

        let web_dir = tmp_root.join("webui");
        std::fs::create_dir_all(&web_dir).unwrap();

        let big_js = "console.log('rustyget');\n".repeat(40);
        std::fs::write(web_dir.join("index.html"), b"<html>ok</html>").unwrap();
        std::fs::write(web_dir.join("a.js"), big_js.as_bytes()).unwrap();
        std::fs::write(web_dir.join("b.js"), b"var x = 1;\n").unwrap();

        let run_req = |raw_req: &[u8], gzip_enabled: bool| -> (bool, Vec<u8>) {
            let sig = &raw_req[..4];
            let rest = std::cell::Cell::new(&raw_req[4..]);
            let cfg = WebServeConfig {
                signature: sig,
                remote_addr: b"127.0.0.1",
                version: b"26.0",
                web_dir: web_dir.as_os_str().as_bytes(),
                credentials: Credentials {
                    control_username: Some(b"admin"),
                    control_password: Some(b"pass"),
                    restricted_username: None,
                    restricted_password: None,
                    add_username: None,
                    add_password: None,
                    authorized_ip: None,
                },
                control_port: 6789,
                form_auth: false,
                gzip_enabled,
                debug_build: false,
            };
            let lower = Lower::Fold(&ascii);
            let mut out = Vec::new();
            let ka = serve_web_request(
                &cfg,
                &lower,
                |dst| {
                    let cur = rest.get();
                    if cur.is_empty() {
                        return false;
                    }
                    let nl = cur.iter().position(|&b| b == b'\n');
                    let take = nl.map_or(cur.len(), |i| i + 1).min(dst.len() - 1);
                    dst[..take].copy_from_slice(&cur[..take]);
                    dst[take] = 0;
                    rest.set(&cur[take..]);
                    true
                },
                |dst| {
                    let cur = rest.get();
                    if cur.len() < dst.len() {
                        return false;
                    }
                    dst.copy_from_slice(&cur[..dst.len()]);
                    rest.set(&cur[dst.len()..]);
                    true
                },
                |chunk| {
                    out.extend_from_slice(chunk);
                    true
                },
                |_, _| {},
                |req, method, access, url, emit| {
                    let body = format!(
                        "{{\"url\":\"{}\",\"m\":{method},\"a\":{access},\"len\":{}}}",
                        std::str::from_utf8(c_bytes(url)).unwrap(),
                        c_bytes(req).len()
                    );
                    emit(body.as_bytes(), Some(b"application/json"), false);
                },
            );
            (ka, out)
        };

        // 1. Unauthenticated request gets 401
        let (ka, resp) = run_req(b"GET / HTTP/1.1\r\nConnection: keep-alive\r\n\r\n", true);
        assert!(ka);
        assert!(resp.starts_with(b"HTTP/1.1 401 Unauthorized\r\n"));

        // 2. Basic auth: YWRtaW46cGFzcw== ("admin:pass"), gzipped a.js + ETag 304 round-trip
        let (ka, resp) = run_req(
            b"GET /a.js HTTP/1.1\r\nAuthorization: Basic YWRtaW46cGFzcw==\r\nAccept-Encoding: gzip\r\nConnection: keep-alive\r\n\r\n",
            true,
        );
        assert!(ka);
        let sep = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let hdr_str = std::str::from_utf8(&resp[..sep]).unwrap();
        assert!(hdr_str.contains("Content-Encoding: gzip\r\n"));
        let etag_line = hdr_str.lines().find(|l| l.starts_with("ETag: ")).unwrap();
        let etag_val = &etag_line[6..];
        let mut dec = flate2::read::GzDecoder::new(&resp[sep + 4..]);
        let mut gunzipped = String::new();
        dec.read_to_string(&mut gunzipped).unwrap();
        assert_eq!(gunzipped, big_js);

        let req_304 = format!(
            "GET /a.js HTTP/1.1\r\nAuthorization: Basic YWRtaW46cGFzcw==\r\nIf-None-Match: {etag_val}\r\n\r\n"
        );
        let (_, resp_304) = run_req(req_304.as_bytes(), true);
        assert!(resp_304.starts_with(b"HTTP/1.1 304 Not Modified\r\n"));
        assert!(resp_304.ends_with(b"\r\n\r\n"));

        // 3. Combined multi-file response
        let (_, resp_comb) = run_req(
            b"GET /combined.js?a.js+b.js HTTP/1.1\r\nAuthorization: Basic YWRtaW46cGFzcw==\r\n\r\n",
            false,
        );
        let sep = resp_comb.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        assert_eq!(
            &resp_comb[sep + 4..],
            format!("{big_js}var x = 1;\n").as_bytes()
        );

        // 4. The removed extension route is an ordinary (missing) static file
        let (_, resp_ext) = run_req(
            b"GET /extensions/myext/ui.js HTTP/1.1\r\nAuthorization: Basic YWRtaW46cGFzcw==\r\n\r\n",
            false,
        );
        assert!(resp_ext.starts_with(b"HTTP/1.1 404 Not Found\r\n"));

        // 5. POST JSON-RPC with URL credentials
        let (_, resp_rpc) = run_req(
            b"POST /admin:pass/jsonrpc HTTP/1.1\r\nContent-Length: 14\r\n\r\n{\"method\":\"v\"}",
            false,
        );
        let sep = resp_rpc.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        assert_eq!(
            &resp_rpc[sep + 4..],
            b"{\"url\":\"/jsonrpc\",\"m\":0,\"a\":0,\"len\":14}"
        );
    }
}
