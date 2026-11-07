//! The web server's request handling decisions (WebProcessor): what a request
//! header line means, the URL's prefix and credentials, the credential check
//! and the authorized-IP list. They match the C++ code; header names compare
//! as the C library's strncasecmp and numbers parse as its atoi (current
//! locale). Reading the request and answering it stay C++.

use std::ffi::{c_char, CStr};

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
    AcceptEncoding { gzip: bool },
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
        Header::AcceptEncoding { gzip: find(line, b"gzip").is_some() }
    } else if prefix(line, "Origin: ") {
        Header::Origin(value(8))
    } else if prefix(line, "Cookie: ") {
        match find(line, b"Auth-Token=").map(|k| &line[k + 11..]) {
            Some(tok) if !tok.is_empty() && tok[0] != b';' => Header::AuthToken(&tok[..tok.len().min(TOKEN_SIZE - 1)]),
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
            return Url::Go { url, auth: Some(auth) };
        }
    }
    Url::Go { url: url.to_vec(), auth: None }
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
    let mut r = Check { authorized: true, access: None, auth_cut: None, warn: false };
    let ip_ok = !empty(o.authorized_ip) && is_authorized_ip(o.authorized_ip.unwrap_or_default(), remote, lower);
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
    } else if !empty(o.restricted_username) && eq(user, o.restricted_username) && pw_is(o.restricted_password) {
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

/// `WebProcessor::Execute`:
/// Checks whether an unauthorized HTTP request must immediately receive a 401 response.
#[inline]
pub fn web_proc_should_send_auth_response(
    form_auth: bool,
    rpc_request: bool,
    authorized: bool,
) -> bool {
    (!form_auth || rpc_request) && !authorized
}

/// `WebProcessor::SendBodyResponse`:
/// Checks whether `old_etag` is non-null and equal to `new_etag`.
///
/// # Safety
/// `old_etag` and `new_etag` must be either null or valid null-terminated C strings.
#[inline]
pub unsafe fn web_proc_is_etag_unchanged(
    old_etag: *const c_char,
    new_etag: *const c_char,
) -> bool {
    if old_etag.is_null() || new_etag.is_null() {
        return false;
    }
    CStr::from_ptr(old_etag).to_bytes() == CStr::from_ptr(new_etag).to_bytes()
}

/// Selects `SecurePort` when `tls` is enabled, otherwise `ControlPort`.
pub fn remote_server_select_port(tls: bool, secure_port: i32, control_port: i32) -> i32 {
    if tls {
        secure_port
    } else {
        control_port
    }
}

/// Evaluates whether `RemoteServer::Run` has reached the active request processor cap
/// and whether the rate-limited warning should be logged (`now - last_warn >= 60`).
pub fn remote_server_should_warn_cap(
    active_count: i32,
    max_processors: i32,
    now: i64,
    last_warn: i64,
) -> (bool, bool) {
    let at_cap = active_count >= max_processors;
    let should_warn = at_cap && now.saturating_sub(last_warn) >= 60;
    (at_cap, should_warn)
}

/// Returns the maximum allowed HTTP POST `Content-Length` for `WebProcessor::Execute`.
pub fn web_proc_max_post_content_len(authorized: bool) -> i32 {
    if authorized {
        768 * 1024 * 1024
    } else {
        64 * 1024
    }
}

/// Validates whether an HTTP POST `Content-Length` is within bounds for `authorized`.
pub fn web_proc_is_content_len_valid(content_len: i32, authorized: bool) -> bool {
    content_len >= 0 && content_len <= web_proc_max_post_content_len(authorized)
}

/// Returns whether `WebProcessor::SendBodyResponse` should attempt gzip compression.
pub fn web_proc_should_use_gzip(
    gzip_enabled: bool,
    body_len: i32,
    max_uncompressed_size: i32,
) -> bool {
    gzip_enabled && body_len > max_uncompressed_size
}

/// Returns `"index.html\0"` when `url` ends with `'/'`, else `"\0"`.
pub fn web_proc_default_resource(url: &[u8]) -> &'static [u8] {
    if url.last() == Some(&b'/') {
        b"index.html\0"
    } else {
        b"\0"
    }
}

/// Classifies the 4-byte connection signature in `RequestProcessor::Execute`.
pub fn remote_classify_signature(sig_bytes: &[u8; 4], nzb_magic_be: u32) -> i32 {
    if u32::from_be_bytes(*sig_bytes) == nzb_magic_be {
        1
    } else if sig_bytes == b"POST" || sig_bytes == b"GET " || sig_bytes == b"OPTI" {
        2
    } else {
        0
    }
}

/// Parses the HTTP method and URL slice from the remaining request line after the 4-byte
/// signature in `RequestProcessor::ServWebRequest`.
pub fn remote_parse_web_request_line(
    sig_bytes: &[u8; 4],
    line: &[u8],
) -> Option<(i32, usize, usize)> {
    let (http_method, start_offset) = if sig_bytes == b"POST" {
        if line.is_empty() {
            return None;
        }
        (0, 1usize)
    } else if sig_bytes == b"OPTI" && line.len() > 4 {
        (2, 4usize)
    } else if sig_bytes == b"GET " {
        (1, 0usize)
    } else {
        return None;
    };

    let rest = &line[start_offset..];
    let url_len = rest.iter().position(|&b| b == b' ').unwrap_or(rest.len());
    Some((http_method, start_offset, url_len))
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
/// - `1`: `/extensions/` route with invalid subpath (404 Not Found without warning)
/// - `2`: `/extensions/` route with valid subpath (`ext_offset = 12`)
/// - `3`: `/combined.` with `?` (multi-file response)
/// - `4`: single-file response
pub fn web_classify_get_dispatch(url: &[u8]) -> i32 {
    if !web_is_safe_url_chars(url) {
        return 0;
    }
    if url.starts_with(b"/extensions/") {
        let ext_url = &url[12..];
        match ext_url.iter().position(|&b| b == b'/') {
            None | Some(0) => 1,
            Some(_) => 2,
        }
    } else if url.starts_with(b"/combined.") && url.contains(&b'?') {
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

/// Formats `WebProcessor::SendBodyResponse` quoted hex ETag (`"\"%zx\""`).
pub fn web_format_quoted_hex_etag(hash: usize) -> Vec<u8> {
    format!("\"{hash:x}\"").into_bytes()
}

/// Evaluates whether gzipped output is smaller than the original body in `WebProcessor::SendBodyResponse`.
pub fn web_is_gzip_smaller(gzipped_len: i32, body_len: i32) -> bool {
    gzipped_len > 0 && gzipped_len < body_len
}

/// Formats `WebProcessor::SendSingleFileResponse` file path (`"%s%s%s"`, `web_dir`, `url`, `def_res`).
pub fn web_format_single_file_path(web_dir: &[u8], url: &[u8], def_res: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(web_dir.len() + url.len() + def_res.len());
    out.extend_from_slice(web_dir);
    out.extend_from_slice(url);
    out.extend_from_slice(def_res);
    out
}

/// Formats `" (forwarded for: <forwarded_for>)"` when non-empty else `""` for `WebProcessor::CheckCredentials`.
pub fn webserver_format_forwarded_for_suffix(forwarded_for: &[u8]) -> Vec<u8> {
    if forwarded_for.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(17 + forwarded_for.len() + 1);
    out.extend_from_slice(b" (forwarded for: ");
    out.extend_from_slice(forwarded_for);
    out.push(b')');
    out
}

/// Returns `true` when `content_type == Some(b"text/html")` for `WebProcessor::SendFileResponse`.
pub fn webserver_is_html_content_type(content_type: Option<&[u8]>) -> bool {
    content_type == Some(b"text/html")
}

/// Encodes `bytes` as lowercase hexadecimal ASCII bytes (`2 * bytes.len()`) for `GenerateSecureToken`.
pub fn webserver_encode_hex_token(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0x0f) as usize]);
    }
    out
}

/// Classifies the pre-URL HTTP method check in `WebProcessor::Execute`:
/// `1` = reject POST with `content_len <= 0`, `2` = answer OPTIONS request, `0` = continue.
pub fn webserver_classify_pre_parse_method(
    http_method: i32,
    hm_post: i32,
    hm_options: i32,
    content_len: i32,
) -> i32 {
    if http_method == hm_post && content_len <= 0 {
        1
    } else if http_method == hm_options {
        2
    } else {
        0
    }
}

/// Returns true when `RemoteServer::Run` detects a missing TLS cert or key file (`path.is_empty() || !file_exists`).
pub fn remote_server_is_cert_or_key_missing(path: &[u8], file_exists: bool) -> bool {
    path.is_empty() || !file_exists
}

/// Returns true when `RemoteServer::Run` should reset the listener and retry after a failed bind or accept (`!bind_ok || !has_accepted_conn`).
pub fn remote_server_should_reset_listener(bind_ok: bool, has_accepted_conn: bool) -> bool {
    !bind_ok || !has_accepted_conn
}

/// Returns true when `WebProcessor::Dispatch` routes a GET request to an extension resource (`get_route == 1 || get_route == 2`).
pub fn webserver_classify_ext_route(get_route: i32) -> bool {
    get_route == 1 || get_route == 2
}

/// Returns true when a canonicalized extension asset path resolves outside the script directory (`is_empty || first_char == b'.'`).
pub fn webserver_is_rel_path_outside_root(is_empty: bool, first_char: u8) -> bool {
    is_empty || first_char == b'.'
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
        assert_eq!(header(b"content-length: 42", true), Header::ContentLength(42));
        assert_eq!(header(b"Authorization: Basic YWJj", true), Header::Auth(b"YWJj"));
        assert_eq!(header(b"Authorization: Basic YWJj", false), Header::Other);
        assert_eq!(header(b"Cookie: a=1; Auth-Token=xyz; b=2", true), Header::AuthToken(b"xyz; b=2"));
        assert_eq!(header(b"Cookie: Auth-Token=;", true), Header::Other);
        assert_eq!(header(b"", true), Header::End);
    }

    #[test]
    fn urls() {
        assert_eq!(parse_url(b"/nzbget"), Url::Redirect(b"/nzbget/".to_vec()));
        assert_eq!(parse_url(b"/nzbget/u:p%40/jsonrpc"), Url::Go { url: b"/jsonrpc".to_vec(), auth: Some(b"u:p@".to_vec()) });
        assert_eq!(parse_url(b"/jsonrpc?x=a:b"), Url::Go { url: b"/jsonrpc?x=a:b".to_vec(), auth: None });
        assert_eq!(parse_url(b"/nzbget\0/u:p/jsonrpc"), Url::Redirect(b"/nzbget/".to_vec()));
        assert_eq!(parse_url(b"/u:p\0/jsonrpc"), Url::Go { url: b"/u:p".to_vec(), auth: None });
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
        assert_eq!((c.authorized, c.access, c.auth_cut), (true, Some(0), Some(6)));
        assert!(check_credentials(&o, b"10.0.0.1", b"nzbget:wrong", b"", tokens, &l).warn);
        assert_eq!(check_credentials(&o, b"10.0.0.1", b"", b"t2", tokens, &l).access, Some(2));
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
        assert!(err_hdr.starts_with(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 87\r\n"));

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
        assert!(text.contains("Content-Type: application/json\r\nContent-Encoding: gzip\r\nETag: \"abc\"\r\n"));
    }

    #[test]
    fn test_web_functions() {
        assert!(web_is_safe_url_chars(b"/index.html?a+1_2-3"));
        assert!(!web_is_safe_url_chars(b"/../secret"));
        assert_eq!(web_classify_get_dispatch(b"/extensions/myext/script.js"), 2);
        assert_eq!(web_classify_get_dispatch(b"/extensions//bad"), 1);
        assert_eq!(web_classify_get_dispatch(b"/combined.js?a.js+b.js"), 3);
        assert!(web_is_ignorable_404(b"apple-touch-icon-120x120.png"));
        assert!(!web_is_ignorable_404(b"index.html"));
        assert_eq!(web_detect_content_type(b"app.CSS"), Some(&b"text/css\0"[..]));
    }
}
