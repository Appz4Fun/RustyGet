//! C-shaped helpers left from the C++ port. Nothing outside this crate calls
//! them any more: strings come in as NUL-terminated C strings and results go
//! out as a `Vec<u8>`. They are being replaced by plain Rust APIs.

use crate::queuesubsys_host::NzbRef;
use crate::cstr;
use crate::options::OptStr;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

#[path = "tlscipher/native.rs"]
pub(crate) mod tls_cipher_native;

pub extern "C" fn nzbget_rs_tls_cipher_pin(protocol: c_int, bits: u32, flags: u32) -> c_int {
    use crate::tlscipher::{eligible, Protocol, Suite};
    let version = match protocol {
        12 => Protocol::Tls12,
        13 => Protocol::Tls13,
        _ => return 0,
    };
    if bits > u16::MAX.into() {
        return 0;
    }
    let suite = Suite {
        name: String::new(),
        algorithm: String::new(),
        protocol: version,
        security_bits: bits as u16,
        aead: flags & 1 != 0,
        certificate: flags & 2 != 0,
        ephemeral: flags & 4 != 0,
    };
    if eligible(&suite) {
        protocol
    } else {
        0
    }
}

fn cipher_response(result: Result<String, crate::tlscipher::Error>) -> Vec<u8> {
    match result {
        Ok(json) => json.into_bytes(),
        Err(error) => {
            let mut message = Vec::new();
            crate::escape::json_encode(error.0.as_bytes(), &mut message);
            let mut out = b"{\"Error\":\"".to_vec();
            out.extend(message);
            out.extend_from_slice(b"\"}");
            out
        }
    }
}

/// # Safety
/// backend is null (built-in backend) or points to a complete callback table. Its context transfers
/// to Rust exactly once, including rejected/duplicate starts. Callbacks are
/// thread-safe for the documented single-worker use and must never unwind.
pub unsafe fn nzbget_rs_tls_cipher_start(backend: *const tls_cipher_native::Io) -> Vec<u8> {
    // A null table selects the built-in OpenSSL backend.
    let io = if backend.is_null() {
        crate::tlssock::cipher::backend()
    } else {
        unsafe { *backend }
    };
    cipher_response(tls_cipher_native::start(io, None))
}
/// Starts the built-in backend for one saved server (`ServerN`), leaving the
/// other servers' ciphers alone. A server ID below 1 means every server.
pub fn nzbget_rs_tls_cipher_start_server(server: c_int) -> Vec<u8> {
    let only = u32::try_from(server).ok().filter(|id| *id > 0);
    cipher_response(tls_cipher_native::start(
        crate::tlssock::cipher::backend(),
        only,
    ))
}
/// # Safety
/// id is null or a readable NUL-terminated string; null/empty queries current job.
pub unsafe fn nzbget_rs_tls_cipher_status(id: *const c_char) -> Vec<u8> {
    let id = if id.is_null() {
        None
    } else {
        let Ok(value) = unsafe { CStr::from_ptr(id) }.to_str() else {
            return cipher_response(Err(crate::tlscipher::Error("Invalid cipher job ID.")));
        };
        (!value.is_empty()).then_some(value)
    };
    cipher_response(tls_cipher_native::service().status(id))
}
/// # Safety
/// id points to a readable NUL-terminated string.
pub unsafe fn nzbget_rs_tls_cipher_cancel(id: *const c_char) -> Vec<u8> {
    if id.is_null() {
        return cipher_response(Err(crate::tlscipher::Error("Missing cipher job ID.")));
    }
    let Ok(id) = unsafe { CStr::from_ptr(id) }.to_str() else {
        return cipher_response(Err(crate::tlscipher::Error("Invalid cipher job ID.")));
    };
    cipher_response(
        tls_cipher_native::service()
            .cancel(id)
            .and_then(|()| tls_cipher_native::service().status(Some(id))),
    )
}

#[cfg(test)]
mod tls_cipher_abi_tests {
    use super::*;
    #[test]
    fn cipher_pin_validates_protocol_security_and_authentication_flags() {
        assert_eq!(nzbget_rs_tls_cipher_pin(13, 128, 3), 13);
        assert_eq!(nzbget_rs_tls_cipher_pin(12, 128, 7), 12);
        for (protocol, bits, flags) in [
            (11, 256, 7),
            (12, 128, 3),
            (13, 64, 7),
            (13, 128, 1),
            (13, 128, 2),
            (13, u32::MAX, 7),
        ] {
            assert_eq!(nzbget_rs_tls_cipher_pin(protocol, bits, flags), 0);
        }
    }
    #[test]
    fn invalid_utf8_status_id_is_an_error_instead_of_current_job() {
        let bytes = [255u8, 0];
        let response = unsafe { nzbget_rs_tls_cipher_status(bytes.as_ptr().cast()) };
        assert!(response.starts_with(b"{\"Error\":"));
    }
}

pub(crate) unsafe fn input<'a>(raw: *const c_char) -> &'a [u8] {
    if raw.is_null() {
        &[]
    } else {
        CStr::from_ptr(raw).to_bytes()
    }
}

// ---- twins and alts (twin.rs); lists cross the ABI as text lines ----

/// Runs an in-place text helper on the NUL-terminated `raw` and puts the NUL
/// at the new end.
#[cfg(test)]
unsafe fn in_place(raw: *mut c_char, f: impl FnOnce(&mut [u8]) -> usize) {
    if raw.is_null() {
        return;
    }
    let len = CStr::from_ptr(raw).to_bytes().len();
    let n = f(std::slice::from_raw_parts_mut(raw.cast::<u8>(), len));
    *raw.add(n) = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parrenamer_ffi_nulls() {
        let mut is_stopped = || false;
        let mut on_progress = |_: &[u8], _: i32| {};
        let mut on_message = |_: crate::parrenamer::Kind, _: &[u8]| {};
        let mut on_parred = |_: &[u8]| {};
        let mut on_renamed = |_: &[u8], _: &[u8]| {};
        let st = crate::parrenamer::execute_all(
            b"",
            b"",
            b"",
            false,
            &mut is_stopped,
            &mut on_progress,
            &mut on_message,
            &mut on_parred,
            &mut on_renamed,
        );
        assert_eq!(
            (
                st.renamed_count,
                st.has_missed_files,
                st.has_damaged_par_files
            ),
            (0, false, false)
        );
    }

    #[test]
    fn rarrenamer_nulls_and_borrowed_callback_strings() {
        struct Probe(Vec<(Vec<u8>, c_int)>);
        impl crate::rarrenamer::RunCallbacks for Probe {
            fn stopped(&mut self) -> Result<bool, ()> {
                Ok(false)
            }
            fn progress(&mut self, label: &[u8], stage: i32) -> Result<(), ()> {
                self.0.push((label.to_vec(), stage));
                Ok(())
            }
            fn message(&mut self, _kind: i32, _text: &[u8]) -> Result<(), ()> {
                Ok(())
            }
            fn register_renamed(&mut self, _old: &[u8], _new: &[u8]) -> Result<(), ()> {
                Ok(())
            }
        }
        let mut probe = Probe(Vec::new());
        let mut renamed = 0;
        assert!(crate::rarrenamer::execute_all(
            b"/nonexistent_dir_12345",
            b"job",
            b"",
            b"",
            true,
            &mut renamed,
            &mut probe,
        )
        .is_ok());
        assert_eq!(renamed, 0);
        assert_eq!(probe.0, vec![(b"Checkingrar-files for job".to_vec(), 0)]);
    }

    #[test]
    fn parparser_bounded_input_and_outputs() {
        assert!(crate::parparser::parse_par_filename(b"bad", true).is_none());
        assert_eq!(
            crate::parparser::parse_par_filename(b"a.vol+2.par2", true),
            Some((1, 2))
        );
        assert_eq!(
            crate::parparser::parse_par_filename(b"a.par2", true),
            Some((1, 0))
        );
        let mut name = [b'a'; crate::parparser::BSTRING_MAX];
        name[1018..].copy_from_slice(b".par2");
        assert_eq!(
            crate::parparser::parse_par_filename(&name, true),
            Some((1018, 0))
        );
        assert!(crate::parparser::same_par_collection(&name, &name, true));
    }

    #[test]
    fn split_command_line_keeps_quoted_words() {
        assert!(crate::util::split_command_line(b"").is_empty());
        assert_eq!(
            crate::util::split_command_line(b"a 'b c'"),
            [b"a".to_vec(), b"b c".to_vec()]
        );
    }

    #[test]
    fn rpc_route_null_inputs_borrowing_and_method_capacity() {
        assert_eq!(crate::rpcroute::protocol(b""), 0);
        for get in [false, true] {
            for protocol in [0, 1, 2, 3] {
                let r = crate::rpcroute::route(b"", b"", get, protocol);
                assert!(r.method.is_empty());
                assert_eq!(r.params, if get { Some(0) } else { None });
                assert!(r.id.is_none());
            }
        }
        let mut url = b"/jsonrpc/".to_vec();
        url.extend_from_slice(&[b'x'; 200]);
        url.extend_from_slice(b"?a=1");
        let r = crate::rpcroute::route(&url, b"", true, 2);
        assert_eq!(r.method, vec![b'x'; 99]);
        assert_eq!(r.params, Some(210));
        assert!(r.id.is_none());
        let request = b"\"id\":},\"method\":\"\"";
        let r = crate::rpcroute::route(b"", request, false, 2);
        assert!(r.params.is_none());
        assert_eq!(r.id, Some((5, 0)));
        assert_eq!(r.method, b"\"");
    }

    #[test]
    fn rpc_decide_outputs() {
        assert_eq!(crate::rpcroute::access(3, 0), 1);
        assert_eq!(crate::rpcroute::access(2, 0), 0);
        let d = crate::rpcroute::decide(3, 1, false, false, true);
        assert_eq!(d.dispatch, 1);
        assert_eq!(
            (d.code, d.text),
            (
                4,
                Some(
                    c"Not safe procedure for JSON-P. Use JSON-RPC or XML-RPC with HTTP-Method POST instead"
                )
            )
        );
        let d2 = crate::rpcroute::decide(2, 0, false, false, false);
        assert_eq!((d2.dispatch, d2.code, d2.text), (0, 0, None));
        assert_eq!(
            crate::rpcroute::decide(3, 0, true, false, false).dispatch,
            1
        );
    }

    #[test]
    fn rpc_envelope_owns_both_outputs() {
        let (head, tail) = crate::rpcroute::envelope(3, false, Some(b"callback"), Some(b"7"));
        assert_eq!(
            head,
            b"callback({\n\"version\" : \"1.1\",\n\"id\" : 7,\n\"result\" : "
        );
        assert_eq!(tail, b"\n})");
    }

    #[test]
    fn rpc_null_arguments_and_unknown_kind() {
        use crate::rpcparams::{Kind, Raw};
        let mut req = b"{\"params\":[12,true,\"x y\"]}\0".to_vec();
        let off = crate::rpcparams::skip_to_params(&mut req);
        unsafe {
            let raw = Raw::new(req.as_mut_ptr().add(off));
            let (p1, v1) = raw.next_int(Kind::Json);
            assert_eq!(v1, Some(12));
            let raw2 = Raw::new(req.as_mut_ptr().add(off + p1));
            let (p2, v2) = raw2.next_bool(Kind::Json, true);
            assert_eq!(v2, Some(true));
            let raw3 = Raw::new(req.as_mut_ptr().add(off + p1 + p2));
            let (_p3, v3) = raw3.next_str(Kind::Json);
            assert!(v3.is_some());
        }
    }

    #[test]
    fn webutil_nulls_borrowing_and_ownership() {
        assert!(crate::webutil::xml_find_tag(b"", b"").is_none());
        assert!(crate::webutil::xml_find_tag(b"", b"x").is_none());
        assert_eq!(
            crate::webutil::xml_find_tag(b"<x>abc</x>", b"x"),
            Some((3, 3))
        );
    }

    #[test]
    fn text_null_inputs_and_callback() {
        unsafe {
            in_place(std::ptr::null_mut(), |b| {
                crate::text::xml_decode(b, &crate::util::byte_tolower_locale)
            });
            let mut xml = *b"&#xA;\0tail";
            in_place(xml.as_mut_ptr().cast(), |b| {
                crate::text::xml_decode(b, &crate::util::byte_tolower_locale)
            });
            assert_eq!(&xml[..2], b"\n\0");
        }
    }

    #[test]
    fn decoder_in_place_buffers_and_borrowed_value_pointer() {
        unsafe {
            in_place(std::ptr::null_mut(), crate::decode::json_decode);
            let mut json = *b"\\ud83d\\ude00\\u0000\0!";
            in_place(json.as_mut_ptr().cast(), crate::decode::json_decode);
            assert_eq!(&json[..8], b"\xf0\x9f\x98\x80\xef\xbf\xbd\0");
            assert_eq!(json[19], b'!');
        }
    }
}

// ============================================================================
// Daemon Lifecycle Coordination C ABI (Requirement R2, Milestone M5)
// ============================================================================

#[cfg(test)]
mod lifecycle_ffi_tests {
    #[test]
    fn test_lifecycle_ffi_queries() {
        let _guard = crate::lifecycle::TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mgr = crate::lifecycle::LifecycleManager::global();
        mgr.set_phase(1.into());
        assert_eq!(mgr.get_phase() as u32, 1);
        mgr.set_phase(0.into());
        assert_eq!(mgr.get_phase() as u32, 0);

        assert!(!mgr.is_reloading());
        assert!(!mgr.is_stopping());

        mgr.notify_signal(0);
    }
}

// ============================================================================
// DupeArticleFallback, ParDamage, DupeCoordinator & QueueCoordinator C ABI
// ============================================================================

#[cfg(unix)]
static ACTIVE_QUEUE_COORD: std::sync::RwLock<
    Option<std::sync::Weak<crate::queuecoord::NzbgetRsQueueCoordinator>>,
> = std::sync::RwLock::new(None);

#[cfg(unix)]
pub(crate) fn set_active_queue_coord(
    opt: Option<std::sync::Weak<crate::queuecoord::NzbgetRsQueueCoordinator>>,
) {
    if let Ok(mut g) = ACTIVE_QUEUE_COORD.write() {
        *g = opt;
    }
}

#[cfg(unix)]
pub(crate) fn active_queue_coord(
) -> Option<std::sync::Arc<crate::queuecoord::NzbgetRsQueueCoordinator>> {
    ACTIVE_QUEUE_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

#[cfg(unix)]
pub(crate) fn queue_host() -> Option<crate::queuesubsys_host::QueueSubsysHost> {
    // the daemon's queue host exists once start-up set the build environment
    // (never in unit tests, which use mock hosts)
    BUILD_ENV.read().ok().and_then(|g| {
        g.as_ref()
            .map(|_| crate::queuesubsys_host::QueueSubsysHost::active())
    })
}

#[cfg(unix)]
pub(crate) fn active_queue_coord_and_host() -> Option<(
    std::sync::Arc<crate::queuecoord::NzbgetRsQueueCoordinator>,
    crate::queuesubsys_host::QueueSubsysHost,
)> {
    let arc = active_queue_coord()?;
    let qh = queue_host()?;
    Some((arc, qh))
}

#[cfg(unix)]
pub(crate) fn log_active_queue_coord_debug_info() {
    let Some((qc, qh)) = active_queue_coord_and_host() else {
        return;
    };
    qc.log_debug_info(&qh);
}

// ============================================================================
// QueueCoordinator, ArticleWriter, HistoryCoordinator, QueueEditor &
// DownloadInfo Hot-Path FFI (`rust/src/queuehotpath.rs`)
// ============================================================================

// ============================================================================
// DiskState, PrePostProcessor, Scanner & TwinCheck Hot-Path FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

// ============================================================================
// DupeSearch, Fleet, DonorHealth, Newznab, FeedCoordinator, FeedInfo &
// FeedFile Hot-Path FFI (`rust/src/dupesearch_nzb.rs` / `rust/src/feed.rs`)
// ============================================================================

// ============================================================================
// StatMeter, WebProcessor, UrlCoordinator, DirectRenamer, NzbFile,
// ScriptConfig & ScriptController FFI
// (`rust/src/statmeter.rs` / `rust/src/configfile.rs` / `rust/src/procrun.rs`)
// ============================================================================

// ============================================================================
// XmlRpc, RemoteServer, Connection, WebDownloader, Options,
// Log & NString hot-path FFI
// ============================================================================

// ============================================================================
// System, NServ, DupeSearch, Connect, PostRenamer & SystemHealth Hot-Path FFI
// ============================================================================

// ============================================================================
// Consolidated ArticleDownloader & ServerPool Hot-Path FFI (`rust/src/nntp.rs`)
// ============================================================================

// ============================================================================
// QueueCoordinator & Options consolidated FFI exports
// ============================================================================

// ============================================================================
// DiskState Consolidated Validation & State FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

// ============================================================================
// Consolidated WebDownloader & StatMeter FFI (`rust/src/webdownload.rs`, `rust/src/statmeter.rs`)
// ============================================================================

// ============================================================================
// DiskState Consolidated Parsing & Matching FFI (`rust/src/diskstatepp.rs`)
// ============================================================================

// ============================================================================
// SystemHealth (`rust/src/systemhealth.rs`)
// ============================================================================

#[cfg(unix)]
pub(crate) fn system_health_diagnose_bytes(format_mode: c_int) -> Vec<u8> {
    let Some(opts) = crate::options::active_options() else {
        return Vec::new();
    };
    let pool = crate::net::active_server_pool();
    let feed_coord = crate::feed::active_feed_coord();
    let sched = crate::scheduler::active_scheduler();
    let log = crate::log::active_log();
    let daemon_mode = opts.daemon_mode();
    let sc = opts.scalars();
    let to_path = |b: &[u8]| crate::cleanup::bytes_to_path_buf(b);

    let main_p = to_path(opts.str_bytes(OptStr::MainDir));
    let dest_p = to_path(opts.str_bytes(OptStr::DestDir));
    let inter_p = to_path(opts.str_bytes(OptStr::InterDir));
    let nzb_p = to_path(opts.str_bytes(OptStr::NzbDir));
    let queue_p = to_path(opts.str_bytes(OptStr::QueueDir));
    let web_p = to_path(opts.str_bytes(OptStr::WebDir));
    let temp_p = to_path(opts.str_bytes(OptStr::TempDir));
    let cfg_tpl_p = to_path(opts.str_bytes(OptStr::ConfigTemplate));
    let log_bytes = log
        .as_ref()
        .map_or_else(Vec::new, |l| l.log_filename_bytes());
    let log_p = to_path(&log_bytes);
    let opt_log_p = to_path(opts.str_bytes(OptStr::LogFile));
    let cert_bytes = opts.str_bytes(OptStr::CertStore);
    let cert_p = to_path(cert_bytes);
    let cfg_p = to_path(opts.str_bytes(OptStr::ConfigFile));
    let lock_p = to_path(opts.str_bytes(OptStr::LockFile));
    let secure_cert_p = to_path(opts.str_bytes(OptStr::SecureCert));
    let secure_key_p = to_path(opts.str_bytes(OptStr::SecureKey));
    let unrar_p = to_path(opts.str_bytes(OptStr::UnrarPath));
    let seven_zip_p = to_path(opts.str_bytes(OptStr::SevenZipPath));
    let pass_p = to_path(opts.str_bytes(OptStr::UnpackPassFile));

    let raw_servers = pool
        .as_ref()
        .map_or_else(Vec::new, |p| p.collect_health_servers());
    let servers: Vec<crate::systemhealth::HealthServerInput<'_>> = raw_servers
        .iter()
        .map(|s| crate::systemhealth::HealthServerInput {
            id: s.id,
            section: crate::systemhealth::NewsServerSectionInput {
                active: s.active,
                name: s.name.as_slice(),
                level: s.level,
                optional: s.optional,
                group: s.group,
                host: s.host.as_slice(),
                port: s.port,
                username: s.username.as_slice(),
                password: s.password.as_slice(),
                tls: s.tls,
                cipher: s.cipher.as_slice(),
                max_connections: s.max_connections,
                retention: s.retention,
                cert_level: s.cert_level,
                ip_version: s.ip_version,
                dupe_search: sc.dupe_search != 0,
            },
        })
        .collect();

    let tasks = sched
        .as_ref()
        .map_or_else(Vec::new, |s| s.collect_health_tasks());

    let categories: Vec<crate::systemhealth::HealthCategoryInput<'_>> = opts
        .categories()
        .iter()
        .map(|cat| crate::systemhealth::HealthCategoryInput {
            name: cat.name.to_bytes(),
            unpack: cat.unpack,
        })
        .collect();

    let raw_feeds = feed_coord
        .as_ref()
        .map_or_else(Vec::new, |fc| fc.collect_health_feeds());
    let feeds: Vec<crate::systemhealth::HealthFeedInput<'_>> = raw_feeds
        .iter()
        .map(
            |(id, name, url, interval, cert_level)| crate::systemhealth::HealthFeedInput {
                id: *id,
                name: name.as_slice(),
                url: url.as_slice(),
                interval: *interval,
                cert_level: *cert_level,
            },
        )
        .collect();

    let diag_input = crate::systemhealth::SystemHealthDiagnoseInput {
        paths: crate::systemhealth::PathsSectionInput {
            main_dir: &main_p,
            dest_dir: &dest_p,
            inter_dir: &inter_p,
            nzb_dir: &nzb_p,
            queue_dir: &queue_p,
            web_dir: &web_p,
            temp_dir: &temp_p,
            config_template: &cfg_tpl_p,
            log_file: &log_p,
            cert_store: &cert_p,
            cert_store_path: &cert_p,
            config_file: &cfg_p,
            lock_file: &lock_p,
            write_log: sc.write_log,
            cert_check: sc.cert_check != 0,
            daemon_mode,
        },
        servers: &servers,
        output_mode: sc.output_mode,
        update_interval: sc.update_interval,
        curses_nzb_name: sc.curses_nzb_name != 0,
        curses_group: sc.curses_group != 0,
        curses_time: sc.curses_time != 0,
        rotate_log: sc.rotate_log,
        log_buffer: sc.log_buffer,
        crash_dump: sc.crash_dump != 0,
        time_correction: sc.time_correction,
        article_retries: sc.article_retries,
        hc_dupe: sc.health_check == 4,
        dupe_search: sc.dupe_search != 0,
        article_interval: sc.article_interval,
        article_timeout: sc.article_timeout,
        article_read_chunk_size: sc.article_read_chunk_size,
        url_retries: sc.url_retries,
        url_interval: sc.url_interval,
        url_timeout: sc.url_timeout,
        url_connections: sc.url_connections,
        remote_timeout: sc.remote_timeout,
        download_rate: sc.download_rate,
        monthly_quota: sc.monthly_quota,
        quota_start_day: sc.quota_start_day,
        daily_quota: sc.daily_quota,
        flush_queue: sc.flush_queue != 0,
        continue_partial: sc.continue_partial != 0,
        propagation_delay: sc.propagation_delay,
        article_cache: sc.article_cache,
        direct_write: sc.direct_write != 0,
        write_buffer: sc.write_buffer,
        file_naming: sc.file_naming,
        post_strategy: sc.post_strategy,
        disk_space: sc.disk_space,
        nzb_cleanup_disk: sc.nzb_cleanup_disk != 0,
        keep_history: sc.keep_history,
        feed_history: sc.feed_history,
        skip_write: sc.skip_write != 0,
        raw_article: sc.raw_article != 0,
        security: crate::systemhealth::SecuritySectionInput {
            control_ip: opts.str_bytes(OptStr::ControlIp),
            control_port: sc.control_port,
            control_username: opts.str_bytes(OptStr::ControlUsername),
            control_password: opts.str_bytes(OptStr::ControlPassword),
            restricted_username: opts.str_bytes(OptStr::RestrictedUsername),
            restricted_password: opts.str_bytes(OptStr::RestrictedPassword),
            add_username: opts.str_bytes(OptStr::AddUsername),
            add_password: opts.str_bytes(OptStr::AddPassword),
            form_auth: sc.form_auth != 0,
            secure_control: sc.secure_control != 0,
            secure_cert: &secure_cert_p,
            secure_key: &secure_key_p,
            secure_port: sc.secure_port,
            cert_check: sc.cert_check != 0,
            cert_store_path_empty: cert_bytes.is_empty(),
            daemon_mode,
            daemon_username: opts.str_bytes(OptStr::DaemonUsername),
            umask: sc.umask,
        },
        append_category_dir: sc.append_category_dir != 0,
        nzb_dir_interval: sc.nzb_dir_interval,
        nzb_dir_file_age: sc.nzb_dir_file_age,
        dupe_check: sc.dupe_check != 0,
        health_check: sc.health_check,
        crc_check: sc.crc_check != 0,
        par_check: sc.par_check,
        par_repair: sc.par_repair != 0,
        par_quick: sc.par_quick != 0,
        par_buffer: sc.par_buffer,
        par_threads: sc.par_threads,
        hw_threads: std::thread::available_parallelism().map_or(0, usize::from),
        par_rename: sc.par_rename != 0,
        rar_rename: sc.rar_rename != 0,
        direct_rename: sc.direct_rename != 0,
        hard_linking: sc.hard_linking != 0,
        par_time_limit: sc.par_time_limit,
        unpack: sc.unpack != 0,
        direct_unpack: sc.direct_unpack != 0,
        unpack_pause_queue: sc.unpack_pause_queue != 0,
        unrar_path: &unrar_p,
        seven_zip_path: &seven_zip_p,
        ext_cleanup_disk: opts.str_bytes(OptStr::ExtCleanupDisk),
        unpack_pass_file: &pass_p,
        unpack_ignore_ext: opts.str_bytes(OptStr::UnpackIgnoreExt),
        options_log_file: &opt_log_p,
        tasks: &tasks,
        categories: &categories,
        feeds: &feeds,
    };

    crate::systemhealth::diagnose_system_health(&diag_input, format_mode, |sev, msg| {
        if let Some(lg) = log.as_deref() {
            if sev == 2 {
                lg.emit_bytes(2, Some(&opts), msg);
            } else if sev == 1 {
                lg.emit_bytes(1, Some(&opts), msg);
            }
        }
    })
}

// ============================================================================
// NzbFileParser, UrlCoordinator & DirectRenamer End-to-End FFI
// ============================================================================

// ============================================================================
// DownloadInfo, DupeProbe & DupeCoordinator End-to-End FFI
// ============================================================================

// =========================================================================
// Consolidated RepairController (`repair.rs`), ContentMap (`streamrepair.rs`),
// and TwinCheck (`twin.rs`) FFI
// =========================================================================

#[cfg(unix)]
struct BuildEnvRuntime {
    ssl_lib_name: CString,
    ssl_raw_version: CString,
}

static BUILD_FLAGS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static BUILD_VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
static QUEUE_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
static BUILD_ENV: std::sync::RwLock<Option<BuildEnvRuntime>> = std::sync::RwLock::new(None);

/// The daemon's version (`VERSION`, also `version` in the justfile).
pub(crate) const DAEMON_VERSION: &str = "27.0";

/// Build flags: 1 debug build, 2 TLS disabled, 4 gzip disabled, 8 take the
/// TLS version after the first space, 16 par-check disabled, 32 curses
/// disabled. TLS, gzip, par-check and curses are always built in.
const BUILD_FLAG_DEBUG: c_int = 1;
const BUILD_FLAG_SSL_VERSION_AFTER_SPACE: c_int = 8;

/// The TLS library's name and version text for SystemInfo, and whether the
/// version is to be taken after the first space (a full "OpenSSL 1.1.1w ..."
/// or "LibreSSL 3.9.0" text rather than OpenSSL 3's bare "3.5.0").
#[cfg(unix)]
fn tls_library_version() -> (&'static [u8], Vec<u8>, bool) {
    const OPENSSL_VERSION: c_int = 0;
    const OPENSSL_FULL_VERSION_STRING: c_int = 7;
    extern "C" {
        fn OpenSSL_version(t: c_int) -> *const c_char;
    }
    let text = |t: c_int| {
        let p = unsafe { OpenSSL_version(t) };
        if p.is_null() {
            Vec::new()
        } else {
            unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
        }
    };
    let full = text(OPENSSL_VERSION);
    if full.starts_with(b"LibreSSL") {
        (b"LibreSSL", full, true)
    } else if openssl::version::number() >= 0x3000_0000 {
        (b"OpenSSL", text(OPENSSL_FULL_VERSION_STRING), false)
    } else {
        (b"OpenSSL", full, true)
    }
}

/// Sets the daemon's build environment once at start-up (the former C++
/// `Util::Init`): the build flags, the version, the TLS library's version
/// text, and with it the daemon's queue host.
#[cfg(unix)]
pub(crate) fn init_build_env() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut flags = 0;
        if cfg!(debug_assertions) {
            flags |= BUILD_FLAG_DEBUG;
        }
        let (ssl_lib, ssl_ver, after_space) = tls_library_version();
        if after_space {
            flags |= BUILD_FLAG_SSL_VERSION_AFTER_SPACE;
        }
        BUILD_FLAGS.store(flags, std::sync::atomic::Ordering::SeqCst);
        let _ = BUILD_VERSION.set(cstr::to_cstring(DAEMON_VERSION.as_bytes()));
        if let Ok(mut g) = BUILD_ENV.write() {
            *g = Some(BuildEnvRuntime {
                ssl_lib_name: cstr::to_cstring(ssl_lib),
                ssl_raw_version: cstr::to_cstring(&ssl_ver),
            });
        }
    });
}

pub(crate) fn build_debug_enabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 1) != 0
}

pub(crate) fn build_tls_disabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 2) != 0
}

pub(crate) fn build_gzip_disabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 4) != 0
}

fn build_ssl_extract_after_space() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 8) != 0
}

pub(crate) fn build_parcheck_enabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 16) == 0
}

pub(crate) fn build_curses_disabled() -> bool {
    (BUILD_FLAGS.load(std::sync::atomic::Ordering::Relaxed) & 32) != 0
}

pub(crate) fn is_queue_loaded() -> bool {
    QUEUE_LOADED.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) fn set_queue_loaded(loaded: bool) {
    QUEUE_LOADED.store(loaded, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn build_version_cstr() -> &'static CStr {
    BUILD_VERSION.get().map_or(c"", CString::as_c_str)
}

pub(crate) fn build_version_bytes() -> &'static [u8] {
    build_version_cstr().to_bytes()
}

/// Connection host callback: TLS, deadlines, stop checks and logging, all in Rust.
#[cfg(unix)]
pub(crate) fn tls_host() -> crate::net::ConnHost {
    crate::net::ConnHost::new(std::sync::Arc::new(crate::tlssock::TlsHost))
}

#[cfg(unix)]
pub(crate) fn stop_ssl_thread() {
    if !build_tls_disabled() {
        crate::tlssock::stop_ssl_thread();
    }
}

/// Writes to the C library's stdout (which `printf` also uses, so output
/// keeps its order), flushing when asked. Nothing is written before
/// start-up set the build environment (nor in unit tests).
#[cfg(unix)]
pub(crate) fn write_stdout_bytes(bytes: &[u8], flush: bool) {
    if !BUILD_ENV.read().is_ok_and(|g| g.is_some()) {
        return;
    }
    extern "C" {
        #[cfg_attr(target_vendor = "apple", link_name = "__stdoutp")]
        static mut stdout: *mut libc::FILE;
    }
    unsafe {
        let out = std::ptr::addr_of!(stdout).read();
        if !bytes.is_empty() {
            libc::fwrite(bytes.as_ptr().cast(), 1, bytes.len(), out);
        }
        if flush {
            libc::fflush(out);
        }
    }
}

static THREAD_COUNT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);

std::thread_local! {
    static TL_RUST_THREAD_STOPPED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn current_thread_count() -> c_int {
    THREAD_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) fn is_rust_thread_stopped() -> bool {
    TL_RUST_THREAD_STOPPED.with(|c| {
        let ptr = c.get();
        if ptr == 0 {
            false
        } else {
            unsafe {
                (&*(ptr as *const std::sync::atomic::AtomicBool))
                    .load(std::sync::atomic::Ordering::SeqCst)
            }
        }
    })
}

pub(crate) struct RustThreadGuard {
    prev: usize,
}

impl RustThreadGuard {
    pub(crate) fn enter(stopped: &std::sync::atomic::AtomicBool) -> Self {
        THREAD_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let ptr = (stopped as *const std::sync::atomic::AtomicBool) as usize;
        let prev = TL_RUST_THREAD_STOPPED.with(|c| c.replace(ptr));
        Self { prev }
    }
}

impl Drop for RustThreadGuard {
    fn drop(&mut self) {
        TL_RUST_THREAD_STOPPED.with(|c| c.set(self.prev));
        THREAD_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Accounts for a worker thread that was cancelled and so never runs its
/// `RustThreadGuard` drop.
pub(crate) fn note_thread_cancelled() {
    THREAD_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(unix)]
pub(crate) unsafe fn format_system_info_bytes(mode: c_int, with_network: bool) -> Vec<u8> {
    let conn_host = tls_host();
    let (ssl_lib, ssl_raw) = BUILD_ENV
        .read()
        .ok()
        .and_then(|g| {
            g.as_ref().map(|e| {
                (
                    e.ssl_lib_name.to_bytes().to_vec(),
                    e.ssl_raw_version.to_bytes().to_vec(),
                )
            })
        })
        .unwrap_or_default();
    let (pub_ip, priv_ip) = if with_network {
        match crate::systemhealth::system_info_fetch_network(
            "ip.nzbget.com",
            !build_tls_disabled(),
            !build_gzip_disabled(),
            |fd, host_cstr, req| {
                let suppress = std::sync::atomic::AtomicBool::new(true);
                // SAFETY: no deadline, and `suppress` outlives the socket,
                // which is closed before this closure returns.
                let started =
                    unsafe { conn_host.tls_start(fd, 1, 0, &suppress, host_cstr, c"", c"", c"") };
                let tls = match started.handle {
                    Some(tls) if started.ok => tls,
                    handle => {
                        if let Some(tls) = handle {
                            conn_host.tls_close(tls, true);
                        }
                        stop_ssl_thread();
                        return Err("TLS handshake failed".to_string());
                    }
                };
                let mut written = 0usize;
                while written < req.len() {
                    let n = conn_host.tls_send(&tls, &req[written..]);
                    let Ok(n) = usize::try_from(n) else {
                        break;
                    };
                    if n == 0 {
                        break;
                    }
                    written += n;
                }
                let mut raw_resp = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = conn_host.tls_recv(&tls, &mut chunk);
                    let Ok(n) = usize::try_from(n) else {
                        break;
                    };
                    if n == 0 {
                        break;
                    }
                    raw_resp.extend_from_slice(&chunk[..n]);
                }
                conn_host.tls_close(tls, true);
                stop_ssl_thread();
                Ok(raw_resp)
            },
        ) {
            Ok((pub_v, priv_v)) => (pub_v, priv_v),
            Err(err) => {
                if !err.is_empty() {
                    let mut m = b"Failed to get public and private IP: ".to_vec();
                    m.extend_from_slice(err.as_bytes());
                    crate::log::log_active_bytes(4, &m);
                }
                (Vec::new(), Vec::new())
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };
    crate::systemhealth::system_info_format(
        mode,
        &[],
        &[],
        &[],
        &ssl_lib,
        &ssl_raw,
        build_ssl_extract_after_space(),
        &pub_ip,
        &priv_ip,
    )
}

pub struct NzbgetRsWorkState {
    resume_time: std::sync::atomic::AtomicI64,
    local_time_offset: std::sync::atomic::AtomicI32,
    speed_limit: std::sync::atomic::AtomicI32,
    temp_pause_download: std::sync::atomic::AtomicBool,
    fleet_hold: std::sync::atomic::AtomicI32,
    temp_pause_postprocess: std::sync::atomic::AtomicBool,
    pause_download: std::sync::atomic::AtomicBool,
    pause_post_process: std::sync::atomic::AtomicBool,
    pause_scan: std::sync::atomic::AtomicBool,
    pause_frontend: std::sync::atomic::AtomicBool,
    downloading: std::sync::atomic::AtomicBool,
    quota_reached: std::sync::atomic::AtomicBool,
    wait_lock: (std::sync::Mutex<u64>, std::sync::Condvar),
}

static ACTIVE_WORKSTATE: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsWorkState>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_workstate() -> Option<std::sync::Arc<NzbgetRsWorkState>> {
    ACTIVE_WORKSTATE
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

impl NzbgetRsWorkState {
    fn notify(&self) {
        self.wake_waiters();
        #[cfg(unix)]
        {
            crate::scheduler::wake_active_disk_service();
            crate::feed::notify_active_feed_work_state();
            crate::queuecoord::notify_from_workstate();
        }
    }

    pub(crate) fn wake_waiters(&self) {
        let mut g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        *g = g.wrapping_add(1);
        self.wait_lock.1.notify_all();
    }

    pub(crate) fn wait_for_change(&self, timeout_ms: u64) {
        let g = self.wait_lock.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .wait_lock
            .1
            .wait_timeout(g, std::time::Duration::from_millis(timeout_ms));
    }

    pub(crate) fn pause_download(&self) -> bool {
        self.pause_download
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn pause_post_process(&self) -> bool {
        self.pause_post_process
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn pause_scan(&self) -> bool {
        self.pause_scan.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn pause_frontend(&self) -> bool {
        self.pause_frontend
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn downloading(&self) -> bool {
        self.downloading.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn quota_reached(&self) -> bool {
        self.quota_reached.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn temp_pause_download(&self) -> bool {
        self.temp_pause_download
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn temp_pause_postprocess(&self) -> bool {
        self.temp_pause_postprocess
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn download_held(&self) -> bool {
        self.temp_pause_download
            .load(std::sync::atomic::Ordering::SeqCst)
            || self.fleet_hold.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    pub(crate) fn speed_limit(&self) -> i32 {
        self.speed_limit.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn resume_time(&self) -> i64 {
        self.resume_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn local_time_offset(&self) -> i32 {
        self.local_time_offset
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn set_pause_download(&self, val: bool) {
        self.pause_download
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_pause_post_process(&self, val: bool) {
        self.pause_post_process
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_pause_scan(&self, val: bool) {
        self.pause_scan
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_temp_pause_download(&self, val: bool) {
        self.temp_pause_download
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_temp_pause_postprocess(&self, val: bool) {
        self.temp_pause_postprocess
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_speed_limit(&self, val: i32) {
        self.speed_limit
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_resume_time(&self, val: i64) {
        self.resume_time
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_local_time_offset(&self, val: i32) {
        self.local_time_offset
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_quota_reached(&self, val: bool) {
        self.quota_reached
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn set_downloading(&self, val: bool) {
        self.downloading
            .store(val, std::sync::atomic::Ordering::SeqCst);
        self.notify();
    }

    pub(crate) fn hold_download_for_fleet(&self, hold: bool) {
        self.fleet_hold.fetch_add(
            if hold { 1 } else { -1 },
            std::sync::atomic::Ordering::SeqCst,
        );
        self.notify();
    }
}

impl NzbgetRsWorkState {
    fn new() -> Self {
        use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64};
        Self {
            resume_time: AtomicI64::new(0),
            local_time_offset: AtomicI32::new(0),
            speed_limit: AtomicI32::new(0),
            temp_pause_download: AtomicBool::new(true),
            fleet_hold: AtomicI32::new(0),
            temp_pause_postprocess: AtomicBool::new(true),
            pause_download: AtomicBool::new(false),
            pause_post_process: AtomicBool::new(false),
            pause_scan: AtomicBool::new(false),
            pause_frontend: AtomicBool::new(false),
            downloading: AtomicBool::new(false),
            quota_reached: AtomicBool::new(false),
            wait_lock: (std::sync::Mutex::new(0), std::sync::Condvar::new()),
        }
    }
}

/// Creates a work state and makes it the one [`active_workstate`] returns.
/// The caller owns it; the active slot only holds a weak reference.
pub(crate) fn install_workstate() -> std::sync::Arc<NzbgetRsWorkState> {
    let ws = std::sync::Arc::new(NzbgetRsWorkState::new());
    if let Ok(mut g) = ACTIVE_WORKSTATE.write() {
        *g = Some(std::sync::Arc::downgrade(&ws));
    }
    ws
}

/// Clears the active work state if it is `ws`, so a newer one stays active.
pub(crate) fn uninstall_workstate(ws: &std::sync::Arc<NzbgetRsWorkState>) {
    if let Ok(mut g) = ACTIVE_WORKSTATE.write() {
        if g
            .as_ref()
            .is_some_and(|cur| std::ptr::eq(cur.as_ptr(), std::sync::Arc::as_ptr(ws)))
        {
            *g = None;
        }
    }
}

static ACTIVE_SCRIPT_TEMPLATES: std::sync::Mutex<Vec<CString>> = std::sync::Mutex::new(Vec::new());

fn load_script_template_cstrings() -> Option<Vec<CString>> {
    let opts = crate::options::active_options()?;
    let path_bytes = opts.str_bytes(OptStr::ConfigTemplate);
    if path_bytes.is_empty() {
        return None;
    }
    let raw = std::fs::read(crate::cleanup::bytes_to_path_buf(path_bytes)).ok()?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let mut cbuf = Vec::with_capacity(end + 1);
    cbuf.extend_from_slice(&raw[..end]);
    cbuf.push(0);
    let cs = unsafe { CString::from_vec_with_nul_unchecked(cbuf) };
    Some(vec![cs])
}

#[cfg(unix)]
pub(crate) unsafe fn script_config_emit_templates(
    load_descriptions: bool,
    out: &mut dyn crate::xmlrpc_format::RpcEmit,
) -> c_int {
    if load_descriptions {
        let Some(loaded) = load_script_template_cstrings() else {
            return 0;
        };
        let refs: Vec<&CStr> = loaded.iter().map(|c| c.as_c_str()).collect();
        out.config_templates(&refs);
        1
    } else {
        let Ok(g) = ACTIVE_SCRIPT_TEMPLATES.lock() else {
            return 0;
        };
        let refs: Vec<&CStr> = g.iter().map(|c| c.as_c_str()).collect();
        out.config_templates(&refs);
        1
    }
}

/// Reloads the script configuration templates into [`ACTIVE_SCRIPT_TEMPLATES`].
/// Returns false (and logs) when they cannot be read.
pub(crate) fn reload_script_templates() -> bool {
    match load_script_template_cstrings() {
        Some(v) => {
            if let Ok(mut g) = ACTIVE_SCRIPT_TEMPLATES.lock() {
                *g = v;
            }
            true
        }
        None => {
            #[cfg(unix)]
            crate::log::log_active_bytes(2, b"Could not read configuration templates");
            false
        }
    }
}

/// The active config file's bytes as the TLS cipher job sees them, or `None`
/// without options, a config file, or a readable snapshot.
pub(crate) fn cipher_config_snapshot() -> Option<Vec<u8>> {
    let opts = crate::options::active_options()?;
    let fn_bytes = opts.str_bytes(OptStr::ConfigFile);
    if fn_bytes.is_empty() {
        return None;
    }
    crate::configfile::script_config_read_cipher_snapshot(fn_bytes)
}

/// Replaces the cipher settings in the active config file when it still holds
/// `expected`. `None` without options or a config file; otherwise the commit
/// outcome. `cancelled` is polled while waiting.
pub(crate) fn commit_cipher_config(
    expected: &[u8],
    replacement: &[u8],
    cancelled: impl Fn() -> bool,
) -> Option<crate::configfile::RsCipherCommit> {
    let opts = crate::options::active_options()?;
    let fn_bytes = opts.str_bytes(OptStr::ConfigFile);
    if fn_bytes.is_empty() {
        return None;
    }
    Some(crate::configfile::script_config_commit_cipher_patch(
        fn_bytes,
        expected,
        replacement,
        cancelled,
        || {
            #[cfg(unix)]
            crate::log::log_active_bytes(
                1,
                b"TLS cipher settings were replaced, but the configuration directory could not be synchronized",
            );
        },
    ))
}

#[cfg(test)]
pub(crate) type DonorNextArticleFn = Option<
    unsafe extern "C" fn(
        *mut std::ffi::c_void,
        *mut c_int,
        *mut i64,
        *mut i64,
        *mut *const u8,
        *mut usize,
    ) -> c_int,
>;

#[cfg(test)]
pub(crate) type DonorBeginMemberBatchFn =
    Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const i32, usize)>;

#[cfg(test)]
pub(crate) struct NzbgetRsCrossPackSession(crate::streamrepair::CrossPackRepairSession);

#[cfg(test)]
pub(crate) type DonorFetchMemberPartFn = Option<
    unsafe extern "C" fn(
        *mut std::ffi::c_void,
        i32,
        i32,
        *mut c_int,
        *mut i64,
        *mut i64,
        *mut *const u8,
        *mut usize,
    ) -> c_int,
>;

#[cfg(test)]
struct FfiCrossPackDonorEnv {
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    fetch_part_cb: DonorFetchMemberPartFn,
    begin_prefetch_cb: DonorBeginMemberBatchFn,
    next_batch_cb: DonorNextArticleFn,
    cancel_batch_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    begin_materialize_member_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    set_progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, i32)>,
    set_materializing_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    add_recovered_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64, i32)>,
    sync_target_holes_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const usize,
            *const crate::dupefallback::NzbgetRsByteRange,
            usize,
        ),
    >,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_extractor_log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_debug_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
}

#[cfg(test)]
impl crate::streamrepair::CrossPackDonorEnv for FfiCrossPackDonorEnv {
    fn is_stopped(&mut self) -> bool {
        self.is_stopped_cb
            .is_some_and(|cb| unsafe { cb(self.ctx) != 0 })
    }

    fn has_batch_fetcher(&self) -> bool {
        self.begin_prefetch_cb.is_some() && self.next_batch_cb.is_some()
    }

    fn fetch_donor_part(
        &mut self,
        member_index: i32,
        part_index: i32,
    ) -> Option<crate::streamrepair::FetchedDonorArticle> {
        let cb = self.fetch_part_cb?;
        let mut success: c_int = 0;
        let mut offset: i64 = 0;
        let mut file_size: i64 = 0;
        let mut data_ptr: *const u8 = std::ptr::null();
        let mut data_len: usize = 0;
        let ok = unsafe {
            cb(
                self.ctx,
                member_index,
                part_index,
                &mut success,
                &mut offset,
                &mut file_size,
                &mut data_ptr,
                &mut data_len,
            )
        };
        if ok == 0 {
            return None;
        }
        let data = if data_ptr.is_null() || data_len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(data_ptr, data_len).to_vec() }
        };
        Some(crate::streamrepair::FetchedDonorArticle {
            success: success != 0,
            offset,
            file_size,
            data,
        })
    }

    fn begin_donor_prefetch(&mut self, member_index: i32, part_indices: &[i32]) {
        if let Some(cb) = self.begin_prefetch_cb {
            unsafe {
                cb(
                    self.ctx,
                    member_index,
                    part_indices.as_ptr(),
                    part_indices.len(),
                );
            }
        }
    }

    fn next_donor_prefetch(&mut self) -> Option<crate::streamrepair::FetchedDonorArticle> {
        let cb = self.next_batch_cb?;
        let mut success: c_int = 0;
        let mut offset: i64 = 0;
        let mut file_size: i64 = 0;
        let mut data_ptr: *const u8 = std::ptr::null();
        let mut data_len: usize = 0;
        let has_next = unsafe {
            cb(
                self.ctx,
                &mut success,
                &mut offset,
                &mut file_size,
                &mut data_ptr,
                &mut data_len,
            )
        };
        if has_next == 0 {
            return None;
        }
        let data = if data_ptr.is_null() || data_len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(data_ptr, data_len).to_vec() }
        };
        Some(crate::streamrepair::FetchedDonorArticle {
            success: success != 0,
            offset,
            file_size,
            data,
        })
    }

    fn cancel_donor_prefetch(&mut self) {
        if let Some(cb) = self.cancel_batch_cb {
            unsafe { cb(self.ctx) };
        }
    }

    fn begin_materialize_member(&mut self, member_index: i32) {
        if let Some(cb) = self.begin_materialize_member_cb {
            unsafe { cb(self.ctx, member_index) };
        }
    }

    fn next_materialize_article(&mut self) -> Option<crate::streamrepair::FetchedDonorArticle> {
        self.next_donor_prefetch()
    }

    fn cancel_materialize_member(&mut self) {
        self.cancel_donor_prefetch();
    }

    fn set_progress(&mut self, label: &[u8], stage_progress: i32) {
        let Some(cb) = self.set_progress_cb else {
            return;
        };
        let mut buf: Vec<u8> = label.iter().copied().filter(|&b| b != 0).collect();
        buf.push(0);
        unsafe { cb(self.ctx, buf.as_ptr() as *const c_char, stage_progress) };
    }

    fn set_materializing(&mut self, delta: i32) {
        if let Some(cb) = self.set_materializing_cb {
            unsafe { cb(self.ctx, delta) };
        }
    }

    fn add_recovered(&mut self, bytes: i64, holes: i32) {
        if let Some(cb) = self.add_recovered_cb {
            unsafe { cb(self.ctx, bytes, holes) };
        }
    }

    fn sync_target_holes(&mut self, target_holes: &[Vec<crate::dupefallback::NzbgetRsByteRange>]) {
        let Some(cb) = self.sync_target_holes_cb else {
            return;
        };
        let mut counts = Vec::with_capacity(target_holes.len());
        let mut flat = Vec::new();
        for th in target_holes {
            counts.push(th.len());
            flat.extend_from_slice(th);
        }
        unsafe {
            cb(self.ctx, counts.as_ptr(), flat.as_ptr(), counts.len());
        }
    }

    fn emit_message_bytes(&mut self, kind: crate::streamrepair::TargetMessageKind, text: &[u8]) {
        let Some(cb) = self.emit_message_cb else {
            return;
        };
        let mut buf: Vec<u8> = text.iter().copied().filter(|&b| b != 0).collect();
        buf.push(0);
        unsafe { cb(self.ctx, kind as i32, buf.as_ptr() as *const c_char) };
    }

    fn emit_extractor_log(&mut self, kind: i32, msg: &str) {
        if let Some(cb) = self.emit_extractor_log_cb {
            let mut buf: Vec<u8> = msg.bytes().filter(|&b| b != 0).collect();
            buf.push(0);
            unsafe { cb(self.ctx, kind, buf.as_ptr() as *const c_char) };
        }
    }

    fn on_rar_volume_parsed(
        &mut self,
        member_name: &[u8],
        vol: &crate::rarreader::Volume,
        files: &[crate::rarreader::RarFile],
    ) {
        let Some(cb) = self.emit_debug_cb else {
            return;
        };
        for mut line in crate::streamrepair::format_rar_volume_debug_lines(member_name, vol, files)
        {
            line.push(0);
            unsafe { cb(self.ctx, line.as_ptr() as *const c_char) };
        }
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_new(
    dest_dir: *const c_char,
    member_names: *const *const c_char,
    member_count: usize,
    target_filenames: *const *const c_char,
    target_decoded_sizes: *const i64,
    target_patch_eligible: *const u8,
    target_hole_counts: *const usize,
    target_holes_flat: *const crate::dupefallback::NzbgetRsByteRange,
    target_count: usize,
    target_password: *const c_char,
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_debug_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) -> *mut NzbgetRsCrossPackSession {
    let mut member_names_vec = Vec::with_capacity(member_count);
    if !member_names.is_null() && member_count > 0 {
        for &ptr in std::slice::from_raw_parts(member_names, member_count) {
            member_names_vec.push(input(ptr));
        }
    }
    let mut targets_vec = Vec::with_capacity(target_count);
    if target_count > 0
        && !target_filenames.is_null()
        && !target_decoded_sizes.is_null()
        && !target_patch_eligible.is_null()
        && !target_hole_counts.is_null()
    {
        let fnames = std::slice::from_raw_parts(target_filenames, target_count);
        let dsizes = std::slice::from_raw_parts(target_decoded_sizes, target_count);
        let peligible = std::slice::from_raw_parts(target_patch_eligible, target_count);
        let hcounts = std::slice::from_raw_parts(target_hole_counts, target_count);
        let total_holes: usize = hcounts.iter().copied().sum();
        let flat = if target_holes_flat.is_null() || total_holes == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(target_holes_flat, total_holes)
        };
        let mut cursor = 0usize;
        for t in 0..target_count {
            let cnt = hcounts[t];
            let end = cursor.saturating_add(cnt).min(flat.len());
            targets_vec.push(crate::streamrepair::CrossPackTargetInput {
                filename: input(fnames[t]).to_vec(),
                decoded_file_size: dsizes[t],
                encoded_size: 0,
                patch_eligible: peligible[t] != 0,
                holes: flat[cursor..end].to_vec(),
                all_articles_failed: false,
            });
            cursor = end;
        }
    }
    let pw = if target_password.is_null() {
        None
    } else {
        let s = input(target_password);
        (!s.is_empty()).then_some(s)
    };
    let aborted = || is_stopped_cb.is_some_and(|cb| unsafe { cb(ctx) != 0 });
    let mut emit_msg = |kind: crate::streamrepair::TargetMessageKind, text: &[u8]| {
        if let Some(cb) = emit_message_cb {
            let mut buf: Vec<u8> = text.iter().copied().filter(|&b| b != 0).collect();
            buf.push(0);
            unsafe { cb(ctx, kind as i32, buf.as_ptr() as *const c_char) };
        }
    };
    let rar_vol_cb = |member_name: &[u8],
                      vol: &crate::rarreader::Volume,
                      files: &[crate::rarreader::RarFile]| {
        if let Some(cb) = emit_debug_cb {
            for mut line in
                crate::streamrepair::format_rar_volume_debug_lines(member_name, vol, files)
            {
                line.push(0);
                unsafe { cb(ctx, line.as_ptr() as *const c_char) };
            }
        }
    };
    let rar_vol_ref: Option<
        &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
    > = emit_debug_cb.map(|_| {
        &rar_vol_cb as &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile])
    });
    let Some(session) = crate::streamrepair::CrossPackRepairSession::new(
        input(dest_dir),
        &member_names_vec,
        &targets_vec,
        pw,
        true,
        Some(&aborted),
        &mut emit_msg,
        rar_vol_ref,
    ) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(NzbgetRsCrossPackSession(session)))
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_free(session: *mut NzbgetRsCrossPackSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_has_inner_holes(
    session: *const NzbgetRsCrossPackSession,
) -> c_int {
    let Some(s) = session.as_ref() else {
        return 0;
    };
    c_int::from(s.0.has_inner_holes())
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_cross_pack_session_process_donor(
    session: *mut NzbgetRsCrossPackSession,
    donor_queued_filename: *const c_char,
    donor_info_name: *const c_char,
    donor_password: *const c_char,
    donor_file_names: *const *const c_char,
    donor_file_sizes: *const i64,
    donor_article_counts: *const usize,
    donor_article_sizes_flat: *const i32,
    donor_file_count: usize,
    allow_decompress: c_int,
    ctx: *mut std::ffi::c_void,
    is_stopped_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int>,
    fetch_part_cb: DonorFetchMemberPartFn,
    begin_prefetch_cb: DonorBeginMemberBatchFn,
    next_batch_cb: DonorNextArticleFn,
    cancel_batch_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
    begin_materialize_member_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    set_progress_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char, i32)>,
    set_materializing_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32)>,
    add_recovered_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i64, i32)>,
    sync_target_holes_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *const usize,
            *const crate::dupefallback::NzbgetRsByteRange,
            usize,
        ),
    >,
    emit_message_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_extractor_log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32, *const c_char)>,
    emit_debug_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) {
    let Some(s) = session.as_mut() else {
        return;
    };
    let mut file_names_vec = Vec::with_capacity(donor_file_count);
    if !donor_file_names.is_null() && donor_file_count > 0 {
        for &ptr in std::slice::from_raw_parts(donor_file_names, donor_file_count) {
            file_names_vec.push(input(ptr));
        }
    }
    let file_sizes_slice = if donor_file_sizes.is_null() || donor_file_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(donor_file_sizes, donor_file_count)
    };
    let mut article_sizes_vec = Vec::with_capacity(donor_file_count);
    if !donor_article_counts.is_null() && donor_file_count > 0 {
        let counts = std::slice::from_raw_parts(donor_article_counts, donor_file_count);
        let total_arts: usize = counts.iter().copied().sum();
        let flat = if donor_article_sizes_flat.is_null() || total_arts == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(donor_article_sizes_flat, total_arts)
        };
        let mut cursor = 0usize;
        for &cnt in counts {
            let end = cursor.saturating_add(cnt).min(flat.len());
            article_sizes_vec.push(flat[cursor..end].to_vec());
            cursor = end;
        }
    }
    let mut env = FfiCrossPackDonorEnv {
        ctx,
        is_stopped_cb,
        fetch_part_cb,
        begin_prefetch_cb,
        next_batch_cb,
        cancel_batch_cb,
        begin_materialize_member_cb,
        set_progress_cb,
        set_materializing_cb,
        add_recovered_cb,
        sync_target_holes_cb,
        emit_message_cb,
        emit_extractor_log_cb,
        emit_debug_cb,
    };
    s.0.process_donor(
        input(donor_queued_filename),
        input(donor_info_name),
        input(donor_password),
        &file_names_vec,
        file_sizes_slice,
        &article_sizes_vec,
        allow_decompress != 0,
        &mut env,
    );
}

#[cfg(unix)]
static ACTIVE_PREPOST: std::sync::RwLock<
    Option<std::sync::Weak<crate::prepost::NzbgetRsPrePostProcessor>>,
> = std::sync::RwLock::new(None);

#[cfg(unix)]
pub(crate) fn active_prepost() -> Option<std::sync::Arc<crate::prepost::NzbgetRsPrePostProcessor>> {
    ACTIVE_PREPOST
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

#[cfg(unix)]
pub(crate) fn set_active_prepost(pp: &std::sync::Arc<crate::prepost::NzbgetRsPrePostProcessor>) {
    if let Ok(mut g) = ACTIVE_PREPOST.write() {
        *g = Some(std::sync::Arc::downgrade(pp));
    }
}

#[cfg(unix)]
pub(crate) fn clear_active_prepost(pp: &std::sync::Arc<crate::prepost::NzbgetRsPrePostProcessor>) {
    let wk = std::sync::Arc::downgrade(pp);
    if let Ok(mut g) = ACTIVE_PREPOST.write() {
        if g.as_ref()
            .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
        {
            *g = None;
        }
    }
}

// The `libc` crate exports these only for Apple targets; the values differ per platform.
#[cfg(all(unix, target_vendor = "apple"))]
const PTHREAD_CANCEL_ENABLE: c_int = 1;
#[cfg(all(unix, target_vendor = "apple"))]
const PTHREAD_CANCEL_DISABLE: c_int = 0;
#[cfg(all(unix, not(target_vendor = "apple")))]
const PTHREAD_CANCEL_ENABLE: c_int = 0;
#[cfg(all(unix, not(target_vendor = "apple")))]
const PTHREAD_CANCEL_DISABLE: c_int = 1;

#[cfg(unix)]
pub(crate) struct PosixCancelGuard {
    old_state: c_int,
}

#[cfg(unix)]
impl PosixCancelGuard {
    pub(crate) unsafe fn disable() -> Self {
        extern "C" {
            fn pthread_setcancelstate(state: c_int, oldstate: *mut c_int) -> c_int;
        }
        let mut old_state = PTHREAD_CANCEL_ENABLE;
        pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &mut old_state);
        Self { old_state }
    }
}

#[cfg(unix)]
impl Drop for PosixCancelGuard {
    fn drop(&mut self) {
        extern "C" {
            fn pthread_setcancelstate(state: c_int, oldstate: *mut c_int) -> c_int;
        }
        unsafe {
            pthread_setcancelstate(self.old_state, std::ptr::null_mut());
        }
    }
}

// ---- WebProcessor / BinRpcProcessor / RemoteClient / Frontend FFI ----

#[cfg(unix)]
unsafe extern "C" fn web_rpc_log_bridge(_ctx: *mut c_void, lvl: c_int, msg: *const c_char) {
    if lvl == 0 && !build_debug_enabled() {
        return;
    }
    let kind = match lvl {
        0 => 3,
        1 => 1,
        2 => 2,
        3 => 0,
        _ => 4,
    };
    crate::log::log_active_bytes(kind, input(msg));
}

#[cfg(unix)]
unsafe extern "C" fn web_rpc_action_bridge(
    _ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    body: *const c_char,
    body_len: usize,
    ids: *const i32,
    ids_len: usize,
    params: *const crate::xmlrpc_format::NzbgetRsRpcParamItem,
    params_len: usize,
    out_u32: *mut u32,
    out_f64: *mut f64,
    out_buf: *mut Vec<u8>,
) -> c_int {
    match op {
        1 => {
            let Some(ws) = active_workstate() else {
                return 0;
            };
            if a1 == 0 {
                let prev = c_int::from(ws.pause_download());
                ws.set_pause_download(true);
                return prev;
            }
            ws.set_pause_download(a2 != 0);
            0
        }
        2 => {
            let Some(ws) = active_workstate() else {
                return 0;
            };
            ws.set_resume_time(0);
            match a2 {
                0 => ws.set_pause_download(a1 != 0),
                1 => ws.set_pause_post_process(a1 != 0),
                2 => ws.set_pause_scan(a1 != 0),
                _ => return 0,
            }
            1
        }
        3 => {
            if let Some(ws) = active_workstate() {
                ws.set_resume_time(libc::time(std::ptr::null_mut()) as i64 + i64::from(a1));
            }
            1
        }
        4 => {
            if let Some(hooks) = crate::lifecycle::get_hooks() {
                hooks.exit_proc();
                return 1;
            }
            0
        }
        5 => {
            if let Some(hooks) = crate::lifecycle::get_hooks() {
                hooks.reload();
                return 1;
            }
            0
        }
        6 => {
            if let Some(lg) = crate::log::active_log() {
                lg.log_debug_info_active();
            }
            1
        }
        7 => {
            if let Some(ws) = active_workstate() {
                ws.set_speed_limit(a1);
            }
            1
        }
        8 => {
            if !out_buf.is_null() {
                *out_buf = format_system_info_bytes(if a1 != 0 { 1 } else { 2 }, true);
            }
            1
        }
        9 => {
            if !crate::options::active_options()
                .is_some_and(|o| o.scalars().system_health_check != 0)
            {
                return 0;
            }
            if !out_buf.is_null() {
                *out_buf = system_health_diagnose_bytes(if a1 != 0 { 1 } else { 2 });
            }
            1
        }
        11 => {
            let add_top = (a1 & 1) != 0;
            let add_paused = (a1 & 2) != 0;
            let auto_cat = (a1 & 4) != 0;
            let is_url = (a1 & 8) != 0;
            let is_zip = (a1 & 16) != 0;
            let fn_b = if s1.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s1).to_bytes()
            };
            let cat_b = if s2.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s2).to_bytes()
            };
            let dk_b = if s3.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s3).to_bytes()
            };
            let mut param_pairs: Vec<(&[u8], &[u8])> = Vec::new();
            if !params.is_null() && params_len > 0 {
                for item in std::slice::from_raw_parts(params, params_len) {
                    param_pairs.push((item.name.to_bytes(), item.value.to_bytes()));
                }
            }
            if is_url {
                let url_b = if body.is_null() {
                    &[][..]
                } else {
                    CStr::from_ptr(body).to_bytes()
                };
                let Some(nzb_id) = crate::urlcoord::add_rpc_url_active(
                    url_b,
                    fn_b,
                    cat_b,
                    auto_cat,
                    a2,
                    add_paused,
                    add_top,
                    dk_b,
                    a3,
                    a4,
                    &param_pairs,
                ) else {
                    return 0;
                };
                if !out_u32.is_null() {
                    *out_u32 = nzb_id;
                }
                return 1;
            }
            let data_b = if body.is_null() || body_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(body.cast::<u8>(), body_len)
            };
            let Some(sc) = crate::scanner::active_scanner() else {
                return 0;
            };
            if is_zip {
                return c_int::from(
                    sc.add_archive_bytes_active(
                        fn_b,
                        cat_b,
                        auto_cat,
                        a2,
                        dk_b,
                        a3,
                        a4,
                        &param_pairs,
                        add_top,
                        add_paused,
                        data_b,
                    ) == crate::scanner::AS_SUCCESS,
                );
            }
            let (_, nzb_id) = sc.add_external_bytes_active(
                fn_b,
                cat_b,
                auto_cat,
                a2,
                dk_b,
                a3,
                a4,
                &param_pairs,
                add_top,
                add_paused,
                data_b,
            );
            if !out_u32.is_null() {
                *out_u32 = nzb_id as u32;
            }
            c_int::from(nzb_id > 0)
        }
        12 => {
            crate::log::clear_active_log();
            1
        }
        13 => {
            if let Some(sc) = crate::scanner::active_scanner() {
                sc.scan_nzb_dir_active(a1 != 0);
            }
            1
        }
        16 => {
            if let Some(fc) = crate::feed::active_feed_coord() {
                fc.fetch_feed_active(a1);
            }
            1
        }
        17 => {
            let Some(pool) = crate::net::active_server_pool() else {
                return 0;
            };
            c_int::from(
                crate::net::nzbget_rs_serverpool_ctl(
                    std::sync::Arc::as_ptr(&pool),
                    7,
                    0,
                    0,
                    ids as usize,
                    ids_len,
                ) != 0,
            )
        }
        18 => {
            let tdir = crate::options::active_options()
                .and_then(|o| o.str_cstr(OptStr::TempDir).map(|s| s.to_owned()));
            let tdir_ptr = tdir.as_ref().map_or(c"".as_ptr(), |s| s.as_ptr());
            let mut buf = Vec::new();
            let ok = crate::net::nzbget_rs_webdl_read_temp_url(
                if s1.is_null() { c"".as_ptr() } else { s1 },
                if s2.is_null() { c"".as_ptr() } else { s2 },
                tdir_ptr,
                &mut buf,
            );
            if ok != 0 && !out_buf.is_null() {
                *out_buf = buf;
            }
            c_int::from(ok != 0)
        }
        19 => match crate::maintenance::check_updates_active() {
            Some(info) => {
                let nul = info.iter().position(|&b| b == 0).unwrap_or(info.len());
                if !out_buf.is_null() {
                    *out_buf = info[..nul].to_vec();
                }
                1
            }
            None => 0,
        },
        20 => c_int::from(crate::maintenance::start_update_active(a1)),
        21 => {
            let Some(sm) = crate::statmeter::active_statmeter() else {
                return 0;
            };
            let tz = active_workstate().map_or(0, |w| w.local_time_offset());
            crate::statmeter::nzbget_rs_statmeter_rpc(
                std::sync::Arc::as_ptr(&sm),
                0,
                tz,
                a1,
                a2,
                0,
                0,
                0,
                None,
            )
        }
        23 => {
            let mut err = Vec::new();
            let ok = crate::net::nzbget_rs_serverpool_test(
                if s1.is_null() { c"".as_ptr() } else { s1 },
                a1,
                if s2.is_null() { c"".as_ptr() } else { s2 },
                if s3.is_null() { c"".as_ptr() } else { s3 },
                a2,
                if s4.is_null() { c"".as_ptr() } else { s4 },
                a3,
                a4 as u32,
                if body.is_null() || body_len == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(body.cast::<u8>(), body_len)
                },
                &mut err,
            );
            if ok == 0 && !err.is_empty() && !out_buf.is_null() {
                *out_buf = err;
            }
            ok
        }
        24 => {
            let Some(pool) = crate::net::active_server_pool() else {
                return 0;
            };
            let mut err_ptr: *const c_char = std::ptr::null();
            if crate::net::nzbget_rs_serverpool_ctl(
                std::sync::Arc::as_ptr(&pool),
                10,
                a1,
                0,
                (&mut err_ptr as *mut *const c_char) as usize,
                0,
            ) == 0
            {
                if !out_buf.is_null() && !err_ptr.is_null() {
                    *out_buf = CStr::from_ptr(err_ptr).to_bytes().to_vec();
                }
                return 0;
            }
            let url_b = if s1.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(s1).to_bytes()
            };
            c_int::from(crate::urlcoord::add_testserver_url_active(a1, url_b))
        }
        10 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            let id_slice = if ids.is_null() || ids_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(ids, ids_len)
            };
            let args_b = input(s1);
            c_int::from(qh.with_locked_queue(|dq_ptr| {
                qh.dq_edit_list(dq_ptr, Some(id_slice), None, 0, a1, args_b)
            }))
        }
        22 => {
            if queue_host().is_none() {
                return 0;
            }
            let reply = crate::qqueue::tls_cipher(a1, input(s1));
            if !out_buf.is_null() {
                *out_buf = reply;
            }
            1
        }
        25 => crate::net::nzbget_rs_net_speedtest_iter(body, body_len, a1, out_u32, out_f64),
        _ => 0,
    }
}

#[cfg(unix)]
unsafe fn emit_rpc_nzb(
    qh: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
    text_status: crate::qnzb::TextStatus,
    f_cb: impl FnOnce(
        &crate::queuesubsys_host::NzbSnap,
        &CStr,
        &crate::xmlrpc_format::NzbgetRsRpcNzbInfoInput,
        &[crate::xmlrpc_format::NzbgetRsRpcParamItem],
        &[crate::xmlrpc_format::NzbgetRsRpcScriptItem],
        &[crate::xmlrpc_format::NzbgetRsRpcServerStatItem],
    ),
) {
    let n = qh.nzb_snap_mode(nzb_ptr, text_status);
    let post_start_time = if n.post_info_ptr.is_some() {
        qh.post_snap(n.post_info_ptr).start_time
    } else {
        0
    };
    let cur_time = if post_start_time != 0 {
        libc::time(std::ptr::null_mut()) as i64
    } else {
        0
    };
    let c_name = cstr::to_cstring(&n.name);
    let c_url = cstr::to_cstring(&n.url);
    let c_filename = cstr::to_cstring(&n.filename);
    let c_dest_dir = cstr::to_cstring(&n.dest_dir);
    let c_final_dir = cstr::to_cstring(&n.final_dir);
    let c_category = cstr::to_cstring(&n.category);
    let c_dupe_key = cstr::to_cstring(&n.dupe_key);
    let c_text_status = cstr::to_cstring(&n.text_status);

    let f = crate::xmlrpc_format::NzbgetRsRpcNzbInfoInput {
        id: n.id,
        kind: n.kind,
        name: &c_name,
        url: &c_url,
        filename: &c_filename,
        dest_dir: &c_dest_dir,
        final_dir: &c_final_dir,
        category: &c_category,
        par_status: n.par_status.into(),
        unpack_status: n.unpack_status.into(),
        move_status: n.move_status.into(),
        script_status: n.script_total_status.into(),
        delete_status: n.delete_status.into(),
        mark_status: n.mark_status.into(),
        url_status: n.url_status.into(),
        size: n.size,
        file_count: n.file_count,
        min_time: n.min_time as c_int,
        max_time: n.max_time as c_int,
        total_articles: n.total_articles,
        current_success_articles: n.current_success_articles,
        current_failed_articles: n.current_failed_articles,
        dupe_recovered_articles: n.dupe_recovered_articles,
        dupe_recovered_bytes: n.dupe_recovered_bytes,
        dupe_recovered_holes: n.dupe_recovered_holes,
        live_repairing: c_int::from(n.has_live_repair_thread),
        health: n.health,
        critical_health: n.critical_health_exact,
        dupe_key: &c_dupe_key,
        dupe_score: n.dupe_score,
        dupe_mode: n.dupe_mode.into(),
        downloaded_size: n.downloaded_size,
        download_sec: n.download_sec,
        post_total_sec: n.post_total_sec,
        has_post_start_time: c_int::from(post_start_time != 0),
        cur_time,
        post_start_time,
        par_sec: n.par_sec,
        repair_sec: n.repair_sec,
        unpack_sec: n.unpack_sec,
        message_count: n.message_count,
        cached_message_count: n.cached_message_count,
        extra_par_blocks: n.extra_par_blocks,
    };

    let prm_cvecs: Vec<_> = qh
        .nzb_params(nzb_ptr)
        .iter()
        .map(|(k, v)| (cstr::to_cstring(k), cstr::to_cstring(v)))
        .collect();
    let prms: Vec<crate::xmlrpc_format::NzbgetRsRpcParamItem> = prm_cvecs
        .iter()
        .map(|(k, v)| crate::xmlrpc_format::NzbgetRsRpcParamItem { name: k, value: v })
        .collect();

    let scr_cvecs: Vec<_> = qh
        .nzb_script_statuses(nzb_ptr)
        .iter()
        .map(|(sname, st)| (cstr::to_cstring(sname), i32::from(*st)))
        .collect();
    let scrs: Vec<crate::xmlrpc_format::NzbgetRsRpcScriptItem> = scr_cvecs
        .iter()
        .map(|(sname, st)| crate::xmlrpc_format::NzbgetRsRpcScriptItem {
            name: sname,
            status: *st,
        })
        .collect();

    let sts: Vec<_> = qh
        .nzb_current_server_stats(nzb_ptr)
        .into_iter()
        .map(|st| crate::xmlrpc_format::NzbgetRsRpcServerStatItem {
            server_id: st.server_id,
            success_articles: st.success,
            failed_articles: st.failed,
        })
        .collect();

    f_cb(&n, &c_text_status, &f, &prms, &scrs, &sts);
}

#[cfg(unix)]
unsafe fn emit_rpc_post(
    qh: &crate::queuesubsys_host::QueueSubsysHost,
    post_ptr: crate::queuesubsys_host::PostRef,
    log_entries: c_int,
    post_queue: bool,
    out: &mut dyn crate::xmlrpc_format::RpcEmit,
) {
    if post_ptr.is_none() {
        out.post_info_end(&crate::xmlrpc_format::RpcPostInfo {
            is_post_queue: post_queue,
            has_post_info: false,
            progress_label: c"",
            stage_progress: 0,
            cur_time: 0,
            stage_time: 0,
            start_time: 0,
            logs: &[],
        });
        return;
    }
    let pi = qh.post_snap(post_ptr);
    let cur_time = libc::time(std::ptr::null_mut()) as i64;
    let c_label = cstr::to_cstring(&pi.progress_label);
    let msgs = if log_entries > 0 {
        qh.nzb_cached_messages_slice(pi.nzb_ptr, 0, log_entries)
    } else {
        Vec::new()
    };
    let entries: Vec<crate::xmlrpc_format::NzbgetRsRpcLogEntry> = msgs
        .iter()
        .map(|m| crate::xmlrpc_format::NzbgetRsRpcLogEntry {
            id: m.0,
            kind: m.1,
            time: m.2,
            text: &m.3,
        })
        .collect();
    out.post_info_end(&crate::xmlrpc_format::RpcPostInfo {
        is_post_queue: post_queue,
        has_post_info: true,
        progress_label: &c_label,
        stage_progress: pi.stage_progress,
        cur_time,
        stage_time: pi.stage_time,
        start_time: pi.start_time,
        logs: &entries,
    });
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
unsafe fn web_rpc_collect_bridge(
    _ctx: *mut c_void,
    kind: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    a4: c_int,
    a5: c_int,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    s4: *const c_char,
    s5: *const c_char,
    s6: *const c_char,
    p1: *const c_void,
    n1: usize,
    out: &mut dyn crate::xmlrpc_format::RpcEmit,
) -> c_int {
    match kind {
        1 => {
            let (post_job_count, url_count, remaining_size, forced_size) =
                if let Some(qh) = queue_host() {
                    qh.with_locked_queue(|dq_ptr| {
                        let mut pjc: c_int = 0;
                        let mut uc: c_int = 0;
                        let qlen = qh.queue_len(dq_ptr);
                        for i in 0..qlen {
                            let np = qh.queue_at(dq_ptr, i);
                            let n = qh.nzb_snap(np, false);
                            if n.has_post_info {
                                pjc += 1;
                            }
                            if n.kind == crate::queuesubsys_host::NK_URL {
                                uc += 1;
                            }
                        }
                        let (rem, forced) = qh.dq_calc_remaining_size(dq_ptr);
                        (pjc, uc, rem, forced)
                    })
                } else {
                    (0, 0, 0, 0)
                };
            let opts = crate::options::active_options();
            let ws = active_workstate();
            let sm = crate::statmeter::active_statmeter();
            let lg = crate::log::active_log();
            let pool = crate::net::active_server_pool();
            let fc = crate::feed::active_feed_coord();

            let (up_sec, dn_sec, all_bytes, stand_by) = sm
                .as_deref()
                .map_or((0, 0, 0, true), |s| s.total_stat_snapshot());
            let tz = ws.as_deref().map_or(0, |w| w.local_time_offset());
            let (month_bytes, day_bytes) = sm.as_deref().map_or((0, 0), |s| {
                s.calc_quota_usage(tz, opts.as_deref(), lg.as_deref())
            });

            let (free_disk, total_disk) =
                match opts.as_deref().and_then(|o| o.str_cstr(OptStr::DestDir)) {
                    Some(p) if !p.to_bytes().is_empty() => {
                        crate::fsops::get_disk_state(p).unwrap_or((0, 0))
                    }
                    _ => (0, 0),
                };
            let inter_cstr = opts.as_deref().and_then(|o| o.str_cstr(OptStr::InterDir));
            let inter_empty = inter_cstr.map_or(true, |p| p.to_bytes().is_empty());
            let (free_inter, total_inter) = match inter_cstr {
                Some(p) if !inter_empty => crate::fsops::get_disk_state(p).unwrap_or((0, 0)),
                _ => (0, 0),
            };

            let mm_active = crate::articledl::memory_monitor_active();
            let cfg_cache_mb = opts.as_deref().map_or(0, |o| o.scalars().article_cache);
            let cfg_par_mb = opts.as_deref().map_or(0, |o| o.scalars().par_buffer);

            let sf = crate::xmlrpc_format::NzbgetRsRpcStatusInput {
                remaining_size,
                forced_size,
                article_cache: crate::articledl::article_cache_allocated() as i64,
                memory_pressure: if mm_active {
                    crate::articledl::memory_monitor_tier_name_c()
                } else {
                    c"NORMAL"
                },
                effective_article_cache_mb: if mm_active {
                    crate::articledl::memory_monitor_effective_cache_mb()
                } else {
                    cfg_cache_mb
                },
                effective_par_buffer_mb: if mm_active {
                    crate::articledl::memory_monitor_effective_par_buffer_mb(cfg_par_mb)
                } else {
                    cfg_par_mb
                },
                download_rate: sm.as_deref().map_or(0, |s| s.current_download_speed()),
                download_limit: ws.as_deref().map_or(0, |w| w.speed_limit()),
                thread_count: current_thread_count() - 1,
                post_job_count,
                url_count,
                up_time_sec: up_sec,
                download_time_sec: dn_sec,
                all_bytes,
                month_bytes,
                day_bytes,
                download_paused: c_int::from(ws.as_deref().is_some_and(|w| w.pause_download())),
                server_stand_by: c_int::from(stand_by),
                post_paused: c_int::from(ws.as_deref().is_some_and(|w| w.pause_post_process())),
                scan_paused: c_int::from(ws.as_deref().is_some_and(|w| w.pause_scan())),
                quota_reached: c_int::from(ws.as_deref().is_some_and(|w| w.quota_reached())),
                free_disk_space: free_disk,
                total_disk_space: total_disk,
                inter_dir_empty: c_int::from(inter_empty),
                free_inter_disk_space: free_inter,
                total_inter_disk_space: total_inter,
                server_time: libc::time(std::ptr::null_mut()) as c_int,
                resume_time: ws.as_deref().map_or(0, |w| w.resume_time() as c_int),
                feed_active: c_int::from(fc.as_deref().is_some_and(|f| f.has_active_downloads())),
                queued_scripts: 0,
            };

            let mut srvs: Vec<crate::xmlrpc_format::NzbgetRsRpcServerStatus> = Vec::new();
            if let Some(p) = pool.as_deref() {
                let cnt = p.server_count();
                srvs.resize_with(cnt, || crate::xmlrpc_format::NzbgetRsRpcServerStatus {
                    id: 0,
                    active: 0,
                });
                let filled = crate::net::nzbget_rs_serverpool_ctl(
                    p,
                    9,
                    0,
                    0,
                    srvs.as_mut_ptr() as usize,
                    cnt,
                ) as usize;
                srvs.truncate(filled.min(cnt));
            }

            out.status(&sf, &srvs);
            1
        }
        2 => {
            if a1 == 3 {
                let disk_msgs = crate::diskstate_format::load_active_nzb_messages(a2);
                if disk_msgs.is_empty() {
                    let Some(qh) = queue_host() else {
                        return 0;
                    };
                    let cached_msgs = qh.with_locked_queue(|dq_ptr| {
                        let np = qh.nzb_find_in_queue_or_history(dq_ptr, a2);
                        if np.is_none() {
                            Vec::new()
                        } else {
                            qh.nzb_cached_messages_slice(np, a3, a4)
                        }
                    });
                    let entries: Vec<crate::xmlrpc_format::NzbgetRsRpcLogEntry> = cached_msgs
                        .iter()
                        .map(|m| crate::xmlrpc_format::NzbgetRsRpcLogEntry {
                            id: m.0,
                            kind: m.1,
                            time: m.2,
                            text: &m.3,
                        })
                        .collect();
                    out.log(&entries);
                    return 1;
                }
                // a download logged before NZB log files were dropped: its old file comes
                // first, then the messages kept since, numbered after the file's last one
                let mut disk_msgs = disk_msgs;
                if let Some(qh) = queue_host() {
                    let last_time = disk_msgs.last().map_or(i32::MIN, |m| m.2);
                    let offset = disk_msgs.iter().map(|m| m.0).max().unwrap_or(0);
                    let cached_msgs = qh.with_locked_queue(|dq_ptr| {
                        let np = qh.nzb_find_in_queue_or_history(dq_ptr, a2);
                        if np.is_none() {
                            Vec::new()
                        } else {
                            qh.nzb_cached_messages_slice(np, 0, i32::MAX)
                        }
                    });
                    disk_msgs.extend(cached_msgs.into_iter().filter(|m| m.2 > last_time).map(
                        |(id, kind, time, text)| (offset.saturating_add(id), kind, time, text),
                    ));
                }
                let mut start = disk_msgs.len() as i32;
                if a4 > 0 {
                    start = disk_msgs.len() as i32 - a4.min(disk_msgs.len() as i32);
                }
                if a3 > 0 {
                    // ids skip a gap where the cached messages begin, so find the first
                    // message at or after the asked id instead of using it as an offset
                    start = disk_msgs
                        .iter()
                        .position(|m| m.0 >= a3)
                        .unwrap_or(disk_msgs.len()) as i32;
                }
                let mut entries = Vec::new();
                if start >= 0 && (start as usize) < disk_msgs.len() {
                    let ustart = start as usize;
                    entries.reserve(disk_msgs.len() - ustart);
                    for m in &disk_msgs[ustart..] {
                        entries.push(crate::xmlrpc_format::NzbgetRsRpcLogEntry {
                            id: m.0,
                            kind: m.1,
                            time: m.2,
                            text: &m.3,
                        });
                    }
                }
                out.log(&entries);
                return 1;
            }
            if a1 == 1 {
                crate::maintenance::collect_maint_messages_active(a3, a4, out);
            } else {
                if let Some(lg) = crate::log::active_log() {
                    lg.collect_rpc(a3, a4, out);
                }
            }
            1
        }
        3 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let outer_n = qh.nzb_snap(np, false);
                    let c_outer_name = cstr::to_cstring(&outer_n.name);
                    let c_outer_fn = cstr::to_cstring(&outer_n.filename);
                    let c_outer_dest = cstr::to_cstring(&outer_n.dest_dir);
                    let c_outer_cat = cstr::to_cstring(&outer_n.category);
                    let flen = qh.nzb_file_len(np);
                    for fi in 0..flen {
                        let fp = qh.nzb_file_at(np, fi);
                        let f = qh.file_snap(fp);
                        let alt_n = if f.nzb_ptr == np {
                            None
                        } else {
                            Some(qh.nzb_snap(f.nzb_ptr, false))
                        };
                        let fnzb_id = alt_n.as_ref().map_or(outer_n.id, |an| an.id);
                        if !((a3 > 0 && a3 == fnzb_id)
                            || (a3 == 0 && (a1 == 0 || (a1 <= f.id && f.id <= a2))))
                        {
                            continue;
                        }
                        let denom = f.size - f.missed_size;
                        let progress = if (f.failed_size == 0 && f.success_size == 0) || denom <= 0
                        {
                            0
                        } else {
                            (1000 - f.remaining_size * 1000 / denom) as c_int
                        };
                        let alt_c = alt_n.as_ref().map(|an| {
                            (
                                cstr::to_cstring(&an.name),
                                cstr::to_cstring(&an.filename),
                                cstr::to_cstring(&an.dest_dir),
                                cstr::to_cstring(&an.category),
                                an.priority,
                            )
                        });
                        let c_subj = cstr::to_cstring(&f.subject);
                        let c_ffn = cstr::to_cstring(&f.filename);
                        let item = crate::xmlrpc_format::NzbgetRsRpcFileItem {
                            id: f.id,
                            file_size: f.size,
                            remaining_size: f.remaining_size,
                            post_time: f.time as c_int,
                            filename_confirmed: c_int::from(f.filename_confirmed),
                            paused: c_int::from(f.paused),
                            nzb_id: fnzb_id,
                            nzb_name: alt_c.as_ref().map_or(&c_outer_name, |x| &x.0),
                            nzb_filename: alt_c.as_ref().map_or(&c_outer_fn, |x| &x.1),
                            subject: &c_subj,
                            filename: &c_ffn,
                            dest_dir: alt_c.as_ref().map_or(&c_outer_dest, |x| &x.2),
                            category: alt_c.as_ref().map_or(&c_outer_cat, |x| &x.3),
                            priority: alt_c.as_ref().map_or(outer_n.priority, |x| x.4),
                            active_downloads: f.active_downloads,
                            progress,
                        };
                        out.file(&item);
                    }
                }
            });
            1
        }
        4 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let flen = qh.nzb_file_len(np) as c_int;
                    let mut post_ptr = crate::queuesubsys_host::PostRef::NONE;
                    emit_rpc_nzb(
                        &qh,
                        np,
                        crate::qnzb::TextStatus::Off,
                        |n, _, f, prms, scrs, sts| {
                            post_ptr = n.post_info_ptr;
                            let has_post = post_ptr.is_some();
                            let post_stage = if has_post { i32::from(n.post_stage) } else { 0 };
                            // The wire format keeps these 8 bits wide.
                            let group = crate::xmlrpc_format::RpcGroupItem {
                                nzb_id: n.id,
                                remaining_size: n.remaining_size,
                                paused_size: n.paused_size,
                                remaining_file_count: flen,
                                remaining_par_count: n.remaining_par_count,
                                priority: n.priority,
                                active_downloads: n.active_downloads,
                                has_post_info: has_post,
                                post_stage: post_stage & 0xff,
                                has_queue_script_job: false,
                                queue_script_active: false,
                                direct_unpack_status: i32::from(n.direct_unpack_status) & 0xff,
                                kind: n.kind & 0xff,
                            };
                            out.group(
                                &group,
                                &crate::xmlrpc_format::RpcNzbInfo {
                                    info: f,
                                    params: prms,
                                    scripts: scrs,
                                    stats: sts,
                                },
                            );
                        },
                    );
                    emit_rpc_post(&qh, post_ptr, a1, false, out);
                }
            });
            1
        }
        5 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let outer = qh.nzb_snap(np, false);
                    if outer.post_info_ptr.is_none() {
                        continue;
                    }
                    let pi = qh.post_snap(outer.post_info_ptr);
                    emit_rpc_nzb(
                        &qh,
                        pi.nzb_ptr,
                        crate::qnzb::TextStatus::Off,
                        |pn, _, f, prms, scrs, sts| {
                            let c_pname = cstr::to_cstring(&pn.name);
                            out.post_queue_item(
                                &crate::xmlrpc_format::RpcPostQueueItem {
                                    nzb_id: outer.id,
                                    info_name: &c_pname,
                                    stage: pi.stage.into(),
                                    file_progress: pi.file_progress,
                                },
                                &crate::xmlrpc_format::RpcNzbInfo {
                                    info: f,
                                    params: prms,
                                    scripts: scrs,
                                    stats: sts,
                                },
                            );
                        },
                    );
                    emit_rpc_post(&qh, outer.post_info_ptr, a1, true, out);
                }
            });
            1
        }
        6 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let hlen = qh.history_len(dq_ptr);
                for hi in 0..hlen {
                    let h = qh.history_at(dq_ptr, hi);
                    if h.kind == crate::queuesubsys_host::HK_DUP && a1 == 0 {
                        continue;
                    }
                    let c_hname = cstr::to_cstring(&h.name);
                    if h.kind == crate::queuesubsys_host::HK_NZB
                        || h.kind == crate::queuesubsys_host::HK_URL
                    {
                        let has_completed = c_int::from(qh.nzb_completed_files_len(h.nzb()) > 0);
                        emit_rpc_nzb(
                            &qh,
                            h.nzb(),
                            crate::qnzb::TextStatus::WithScript,
                            |n, status_ptr, f, prms, scrs, sts| {
                                out.history_nzb(
                                    &crate::xmlrpc_format::RpcHistoryNzb {
                                        id: h.id,
                                        name: &c_hname,
                                        remaining_file_count: n.parked_file_count,
                                        retry_data: has_completed != 0,
                                        history_time: h.time as c_int,
                                        status: status_ptr,
                                    },
                                    &crate::xmlrpc_format::RpcNzbInfo {
                                        info: f,
                                        params: prms,
                                        scripts: scrs,
                                        stats: sts,
                                    },
                                );
                            },
                        );
                    } else if h.kind == crate::queuesubsys_host::HK_DUP {
                        let d = qh.dup_snap(h.inner_ptr);
                        let c_dkey = cstr::to_cstring(&d.dupe_key);
                        out.history_dup(&crate::xmlrpc_format::RpcHistoryDup {
                            id: h.id,
                            name: &c_hname,
                            history_time: h.time as c_int,
                            size: d.size,
                            dupe_key: &c_dkey,
                            dupe_score: d.dupe_score,
                            dupe_mode: d.dupe_mode.into(),
                            dup_status: d.status.into(),
                        });
                    }
                }
            });
            1
        }
        7 => {
            let Some(qh) = queue_host() else {
                return 0;
            };
            qh.with_locked_queue(|dq_ptr| {
                let qlen = qh.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let np = qh.queue_at(dq_ptr, qi);
                    let n = qh.nzb_snap(np, false);
                    if n.kind == crate::queuesubsys_host::NK_URL {
                        let c_fn = cstr::to_cstring(&n.filename);
                        let c_url = cstr::to_cstring(&n.url);
                        let c_name = cstr::to_cstring(&n.name);
                        let c_cat = cstr::to_cstring(&n.category);
                        out.url_queue_item(&crate::xmlrpc_format::RpcUrlQueueItem {
                            id: n.id,
                            filename: &c_fn,
                            url: &c_url,
                            name: &c_name,
                            category: &c_cat,
                            priority: n.priority,
                        });
                    }
                }
            });
            1
        }
        8 => crate::dupesearch_coord::fleet_append(
            std::ptr::null_mut(),
            crate::dupesearch_coord::native_dupe_host,
            s1,
            s2,
            a1,
            a2,
            p1.cast(),
            n1,
            out,
        ),
        9 => {
            let opts = crate::options::active_options();
            let mut entries: Vec<crate::xmlrpc_format::NzbgetRsRpcConfigItem> = Vec::new();
            if let Some(o) = opts.as_deref() {
                o.for_each_option(|n, v, r| {
                    entries.push(crate::xmlrpc_format::NzbgetRsRpcConfigItem {
                        name: n,
                        value: v,
                        restricted: r,
                    });
                });
            }
            out.config_list(&entries);
            1
        }
        10 => script_config_emit_templates(a1 != 0, out),
        11 => {
            let Some(fc) = crate::feed::active_feed_coord() else {
                return 0;
            };
            unsafe extern "C" fn on_feed_item(
                raw: *mut c_void,
                item: *const crate::xmlrpc_format::NzbgetRsRpcFeedItem<'_>,
            ) {
                // SAFETY: `raw` is the `&mut dyn RpcEmit` below, alive for the
                // call; the feed passes a valid item.
                let out = unsafe { &mut *raw.cast::<&mut dyn crate::xmlrpc_format::RpcEmit>() };
                out.feed_item(unsafe { &*item });
            }
            let mut out_ref: &mut dyn crate::xmlrpc_format::RpcEmit = out;
            fc.emit_view_or_preview_active(
                a1,
                a2,
                a3,
                a4,
                a5,
                n1 as c_int,
                s1,
                s2,
                s3,
                s4,
                s5,
                s6,
                (&raw mut out_ref).cast(),
                Some(on_feed_item),
            )
        }
        12 => {
            let Some(sm) = crate::statmeter::active_statmeter() else {
                return 0;
            };
            let tz = active_workstate().map_or(0, |w| w.local_time_offset());
            crate::statmeter::nzbget_rs_statmeter_rpc(
                std::sync::Arc::as_ptr(&sm),
                1,
                tz,
                a1,
                a2,
                a3,
                a4,
                a5,
                Some(out),
            )
        }
        _ => 0,
    }
}

#[cfg(unix)]
pub(crate) unsafe fn serve_web_conn(conn: &crate::net::NzbgetRsConn, sig: &[u8; 4]) -> bool {
    let _cancel_guard = PosixCancelGuard::disable();
    let opts = crate::options::active_options();
    let opt_str = |id: OptStr| -> Option<&[u8]> {
        opts.as_deref()
            .and_then(|o| o.str_cstr(id).map(CStr::to_bytes))
    };
    let remote_addr = conn.remote_addr_bytes();
    let version = build_version_bytes();
    let web_dir = opts.as_deref().map_or(&[][..], |o| o.str_bytes(OptStr::WebDir));
    let control_port = opts.as_deref().map_or(0, |o| o.scalars().control_port);
    let form_auth = opts.as_deref().is_some_and(|o| o.scalars().form_auth != 0);
    let json_p = opts.as_deref().is_some_and(|o| o.scalars().json_p != 0);
    let gzip_enabled = !build_gzip_disabled();
    let debug_build = build_debug_enabled();

    let cfg = crate::webserver::WebServeConfig {
        signature: sig,
        remote_addr: &remote_addr,
        version: &version,
        web_dir,
        credentials: crate::webserver::Credentials {
            control_username: opt_str(OptStr::ControlUsername),
            control_password: opt_str(OptStr::ControlPassword),
            restricted_username: opt_str(OptStr::RestrictedUsername),
            restricted_password: opt_str(OptStr::RestrictedPassword),
            add_username: opt_str(OptStr::AddUsername),
            add_password: opt_str(OptStr::AddPassword),
            authorized_ip: opt_str(OptStr::AuthorizedIp),
        },
        control_port,
        form_auth,
        gzip_enabled,
        debug_build,
    };
    let lower = crate::wildmask::Lower::Fold(&crate::util::wild_mask_fold_locale);

    crate::webserver::serve_web_request(
        &cfg,
        &lower,
        |dst| conn.read_line_web(dst),
        |dst| conn.recv_exact_web(dst),
        |src| conn.send_web(src),
        |level, msg| {
            if level == 0 && !debug_build {
                return;
            }
            let kind = match level {
                0 => 3,
                1 => 1,
                2 => 2,
                3 => 0,
                _ => 4,
            };
            crate::log::log_active_bytes(kind, msg);
        },
        |req_body, http_method, user_access, url_c, emit| {
            let (resp, ct, safe) = crate::xmlrpc_format::serve_xmlrpc(
                std::ptr::null_mut(),
                req_body,
                http_method,
                user_access,
                json_p,
                debug_build,
                url_c,
                &version,
                opt_str(OptStr::ConfigFile),
                Some(web_rpc_log_bridge),
                Some(web_rpc_action_bridge),
                Some(web_rpc_collect_bridge),
            );
            let nul = resp.iter().position(|&b| b == 0).unwrap_or(resp.len());
            emit(&resp[..nul], ct, safe);
        },
    )
}

#[cfg(unix)]
pub(crate) unsafe fn serve_binrpc_conn(conn: &crate::net::NzbgetRsConn) {
    let _cancel_guard = PosixCancelGuard::disable();

    struct LogCollectCtx<'a> {
        lines: i32,
        id_from: u32,
        nr_entries: &'a mut u32,
        enc: &'a mut crate::binrpc::WireEntryEncoder,
    }

    unsafe extern "C" fn log_begin(
        enc_ptr: *mut c_void,
        msg_count: c_int,
        first_id: u32,
        out_start: *mut c_int,
    ) {
        let lc = &mut *enc_ptr.cast::<LogCollectCtx<'_>>();
        let (start, nr) = crate::binrpc::calc_log_slice(msg_count, lc.lines, lc.id_from, first_id);
        *lc.nr_entries = nr.max(0) as u32;
        if !out_start.is_null() {
            *out_start = start;
        }
    }

    unsafe extern "C" fn log_push(
        enc_ptr: *mut c_void,
        id: u32,
        kind: u32,
        time: u32,
        text: *const c_char,
    ) {
        let lc = &mut *enc_ptr.cast::<LogCollectCtx<'_>>();
        let entry = crate::binrpc::LogEntry {
            id,
            kind,
            time,
            texts: [crate::binrpc::Text::new(input(text))],
        };
        lc.enc.push(&entry);
    }

    let opts = crate::options::active_options();
    let remote_addr = conn.remote_addr_bytes();
    let control_user = opts.as_deref().map_or(&[][..], |o| o.str_bytes(OptStr::ControlUsername));
    let control_password = opts.as_deref().map_or(&[][..], |o| o.str_bytes(OptStr::ControlPassword));
    let control_port = opts.as_deref().map_or(0, |o| o.scalars().control_port);
    let version = build_version_bytes();

    crate::binrpc::serve_binrpc(
        &remote_addr,
        control_user,
        control_password,
        &version,
        control_port,
        |dst| conn.recv_exact_web(dst),
        |src| conn.send_web(src),
        |kind, msg| {
            crate::log::log_active_bytes(kind, msg);
        },
        |kind, a1, a2, a3, a4, s1, s2, s3, body, ids, names| {
            if kind == 1 {
                if (a1 & 8) != 0 {
                    return crate::urlcoord::add_binrpc_url_active(
                        body,
                        s1,
                        s2,
                        (a1 & 4) != 0,
                        a2,
                        (a1 & 2) != 0,
                        (a1 & 1) != 0,
                        s3,
                        a3,
                        a4,
                    );
                }
                let Some(sc) = crate::scanner::active_scanner() else {
                    return false;
                };
                return sc
                    .add_external_bytes_active(
                        s1,
                        s2,
                        (a1 & 4) != 0,
                        a2,
                        s3,
                        a3,
                        a4,
                        &[],
                        (a1 & 1) != 0,
                        (a1 & 2) != 0,
                        body,
                    )
                    .0
                    != crate::scanner::AS_FAILED;
            }
            if kind == 2 {
                if let Some(ws) = active_workstate() {
                    ws.set_resume_time(0);
                    match a2 {
                        1 => ws.set_pause_download(a1 != 0),
                        2 => ws.set_pause_post_process(a1 != 0),
                        3 => ws.set_pause_scan(a1 != 0),
                        _ => {}
                    }
                }
                return true;
            }
            if kind == 4 {
                if let Some(ws) = active_workstate() {
                    ws.set_speed_limit(a1);
                }
                return true;
            }
            if kind == 5 {
                if let Some(lg) = crate::log::active_log() {
                    lg.log_debug_info_active();
                }
                return true;
            }
            if kind == 6 {
                let Some(qh) = queue_host() else {
                    return false;
                };
                return qh.with_locked_queue(|dq_ptr| {
                    qh.dq_edit_list(
                        dq_ptr,
                        if ids.is_empty() { None } else { Some(ids) },
                        if names.is_empty() { None } else { Some(names) },
                        a2,
                        a1,
                        s1,
                    )
                });
            }
            if kind == 8 {
                if let Some(hooks) = crate::lifecycle::get_hooks() {
                    hooks.exit_proc();
                }
                return true;
            }
            if kind == 9 {
                if let Some(hooks) = crate::lifecycle::get_hooks() {
                    hooks.reload();
                }
                return true;
            }
            if kind == 13 {
                if let Some(sc) = crate::scanner::active_scanner() {
                    sc.scan_nzb_dir_active(a1 != 0);
                }
                return true;
            }
            false
        },
        |want_files, want_state, builder| {
            let mut post_jobs: c_int = 0;
            let mut rem_size: i64 = 0;
            if want_files || want_state {
                if let Some(qh) = queue_host() {
                    qh.with_locked_queue(|dq_ptr| {
                        let qlen = qh.queue_len(dq_ptr);
                        if want_files {
                            let mut queue_ptrs = Vec::with_capacity(qlen);
                            for i in 0..qlen {
                                queue_ptrs.push(qh.queue_at(dq_ptr, i));
                            }
                            let mut owner_index: u32 = 0;
                            for &np in &queue_ptrs {
                                owner_index += 1;
                                let n = qh.nzb_snap(np, false);
                                builder.push_nzb(
                                    n.id as u32,
                                    n.kind as u32,
                                    n.size,
                                    n.remaining_size,
                                    n.paused_size,
                                    n.paused_file_count as u32,
                                    n.remaining_par_count as u32,
                                    n.priority as u32,
                                    &n.filename,
                                    &n.name,
                                    &n.dest_dir,
                                    &n.category,
                                    &n.queued_filename,
                                );
                                for (pname, pval) in qh.nzb_params(np) {
                                    builder.push_param(owner_index, &pname, &pval);
                                }
                                let flen = qh.nzb_file_len(np);
                                for fi in 0..flen {
                                    let fp = qh.nzb_file_at(np, fi);
                                    let f = qh.file_snap(fp);
                                    let (nzb_index, fnzb_name_owned) = if f.nzb_ptr == np {
                                        (owner_index, None)
                                    } else {
                                        let mut idx: u32 = 0;
                                        for &qp in &queue_ptrs {
                                            idx += 1;
                                            if qp == f.nzb_ptr {
                                                break;
                                            }
                                        }
                                        (idx, Some(qh.nzb_snap(f.nzb_ptr, false).name))
                                    };
                                    let fnzb_name = fnzb_name_owned.as_deref().unwrap_or(&n.name);
                                    builder.push_file(
                                        f.id as u32,
                                        nzb_index,
                                        f.size,
                                        f.remaining_size,
                                        u32::from(f.paused),
                                        u32::from(f.filename_confirmed),
                                        f.active_downloads as u32,
                                        fnzb_name,
                                        &f.subject,
                                        &f.filename,
                                    );
                                }
                            }
                        }
                        if want_state {
                            for i in 0..qlen {
                                let np = qh.queue_at(dq_ptr, i);
                                if qh.nzb_snap(np, false).has_post_info {
                                    post_jobs += 1;
                                }
                            }
                            rem_size = qh.dq_calc_remaining_size(dq_ptr).0;
                        }
                    });
                }
            }
            if want_state {
                let sm = crate::statmeter::active_statmeter();
                let ws = active_workstate();
                let speed = sm.as_deref().map_or(0, |s| s.current_download_speed());
                let (up_sec, dn_sec, all_bytes, stand_by) = sm
                    .as_deref()
                    .map_or((0, 0, 0, true), |s| s.total_stat_snapshot());
                builder.remaining_size = rem_size;
                builder.download_rate = u32::try_from(speed).unwrap_or(0);
                builder.download_limit = ws.as_deref().map_or(0, |w| w.speed_limit());
                builder.download_paused =
                    c_int::from(ws.as_deref().is_some_and(|w| w.pause_download()));
                builder.post_paused =
                    c_int::from(ws.as_deref().is_some_and(|w| w.pause_post_process()));
                builder.scan_paused = c_int::from(ws.as_deref().is_some_and(|w| w.pause_scan()));
                builder.thread_count = current_thread_count() - 1;
                builder.post_job_count = post_jobs;
                builder.up_time_sec = up_sec;
                builder.dn_time_sec = dn_sec;
                builder.stand_by = c_int::from(stand_by);
                builder.downloaded_bytes = all_bytes;
                builder.state_populated = true;
            }
        },
        |lines, id_from, nr_entries, enc| {
            let mut lc = LogCollectCtx {
                lines,
                id_from,
                nr_entries,
                enc,
            };
            let lg = crate::log::active_log();
            crate::log::nzbget_rs_log_collect(
                2,
                lg.as_deref().map_or(std::ptr::null(), |l| l as *const _),
                0,
                0,
                0,
                std::ptr::null_mut(),
                (&mut lc as *mut LogCollectCtx<'_>).cast(),
                log_begin as *const () as usize,
                log_push as *const () as usize,
            );
        },
        |nr_entries, enc| {
            let Some(qh) = queue_host() else {
                return;
            };
            qh.with_locked_queue(|dq_ptr| {
                let cur_time = libc::time(std::ptr::null_mut()) as i64;
                let qlen = qh.queue_len(dq_ptr);
                for i in 0..qlen {
                    let np = qh.queue_at(dq_ptr, i);
                    let outer = qh.nzb_snap(np, false);
                    if outer.post_info_ptr.is_none() {
                        continue;
                    }
                    let outer_id = outer.id as u32;
                    let pi = qh.post_snap(outer.post_info_ptr);
                    let pn = if pi.nzb_ptr == np {
                        outer
                    } else {
                        qh.nzb_snap(pi.nzb_ptr, false)
                    };
                    *nr_entries = nr_entries.wrapping_add(1);
                    let entry = crate::binrpc::PostEntry {
                        id: outer_id,
                        stage: i32::from(pi.stage) as u32,
                        stage_progress: pi.stage_progress as u32,
                        file_progress: pi.file_progress as u32,
                        start_time: if pi.start_time != 0 {
                            (cur_time - pi.start_time) as u32
                        } else {
                            0
                        },
                        stage_time: if pi.stage_time != 0 {
                            (cur_time - pi.stage_time) as u32
                        } else {
                            0
                        },
                        texts: [
                            crate::binrpc::Text::new(&pn.filename),
                            crate::binrpc::Text::new(&pn.name),
                            crate::binrpc::Text::new(&pn.dest_dir),
                            crate::binrpc::Text::new(&pi.progress_label),
                        ],
                    };
                    enc.push(&entry);
                }
            });
        },
        |show_hidden, nr_entries, enc| {
            let Some(qh) = queue_host() else {
                return;
            };
            qh.with_locked_queue(|dq_ptr| {
                let hlen = qh.history_len(dq_ptr);
                for i in 0..hlen {
                    let h = qh.history_at(dq_ptr, i);
                    if h.kind == crate::queuesubsys_host::HK_DUP && !show_hidden {
                        continue;
                    }
                    let mut size: i64 = 0;
                    let mut file_cnt: u32 = 0;
                    let mut par_status: u32 = 0;
                    let mut script_status: u32 = 0;
                    let mut url_status: u32 = 0;
                    if h.kind == crate::queuesubsys_host::HK_NZB {
                        let n = qh.nzb_snap(h.nzb(), false);
                        size = n.size;
                        file_cnt = n.file_count as u32;
                        par_status = i32::from(n.par_status) as u32;
                        script_status = i32::from(n.script_total_status) as u32;
                    } else if h.kind == crate::queuesubsys_host::HK_DUP {
                        size = qh.dup_snap(h.inner_ptr).size;
                    } else if h.kind == crate::queuesubsys_host::HK_URL {
                        url_status = i32::from(qh.nzb_snap(h.nzb(), false).url_status) as u32;
                    }
                    *nr_entries = nr_entries.wrapping_add(1);
                    let size_hi = ((size >> 32) & 0xffff_ffff) as u32;
                    let size_lo = (size & 0xffff_ffff) as u32;
                    let entry = crate::binrpc::HistoryEntry {
                        id: h.id as u32,
                        kind: h.kind as u32,
                        time: (h.time as i32) as u32,
                        size_lo,
                        size_hi,
                        file_count: file_cnt,
                        par_status,
                        script_status,
                        url_status,
                        texts: [crate::binrpc::Text::new(&h.name)],
                    };
                    enc.push(&entry);
                }
            });
        },
    );
}

#[cfg(unix)]
pub(crate) struct FrontendHandle {
    mode: c_int,
    use_color: bool,
    exit_cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[cfg(unix)]
impl FrontendHandle {
    pub(crate) fn new(
        mode: c_int,
        exit_cb: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        if mode != 3 {
            crate::log::log_active_bytes(3, b"Creating Frontend");
        }
        let use_color = if mode == 2 {
            unsafe { crate::ncurses_format::frontend_op(0, std::ptr::null()) > 0 }
        } else {
            true
        };
        Self {
            mode,
            use_color,
            exit_cb,
            stopped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            handle: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn start(&self) {
        let mut g = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_some() {
            return;
        }
        self.stopped
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mode = self.mode;
        let use_color = self.use_color;
        let exit_cb = self.exit_cb.clone();
        let stopped = self.stopped.clone();
        let running = self.running.clone();
        if let Ok(h) = std::thread::Builder::new()
            .name("frontend".into())
            .spawn(move || {
                struct RunningReset(std::sync::Arc<std::sync::atomic::AtomicBool>);
                impl Drop for RunningReset {
                    fn drop(&mut self) {
                        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                let _reset = RunningReset(running);
                let _tg = RustThreadGuard::enter(&stopped);
                run_frontend_thread(mode, use_color, &stopped, exit_cb.as_ref());
            })
        {
            *g = Some(h);
        } else {
            self.running
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub(crate) fn stop(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(ws) = active_workstate() {
            ws.wake_waiters();
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn join(&self) {
        let h = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(h) = h {
            let _ = h.join();
        }
    }
}

#[cfg(unix)]
impl Drop for FrontendHandle {
    fn drop(&mut self) {
        self.stop();
        self.join();
        if self.mode == 2 {
            unsafe {
                crate::ncurses_format::frontend_op(1, std::ptr::null());
            }
            write_stdout_bytes(b"\n", true);
        }
    }
}

#[cfg(unix)]
fn run_frontend_thread(
    mode: c_int,
    use_color: bool,
    stopped: &std::sync::atomic::AtomicBool,
    exit_cb: Option<&std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    if mode == 0 || mode == 1 {
        crate::log::log_active_bytes(3, b"Entering LoggableFrontend-loop");
    } else if mode == 2 {
        crate::log::log_active_bytes(3, b"Entering NCursesFrontend-loop");
    }

    let opts = if mode != 3 {
        crate::options::active_options()
    } else {
        None
    };

    struct FeBridgeCtx<'a> {
        mode: c_int,
        stopped: &'a std::sync::atomic::AtomicBool,
        exit_cb: Option<&'a std::sync::Arc<dyn Fn() + Send + Sync>>,
    }

    unsafe extern "C" fn fe_stdout_cb(
        _raw: *mut c_void,
        ptr: *const c_char,
        len: usize,
        do_flush: c_int,
    ) {
        let slice = if ptr.is_null() || len == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(ptr.cast::<u8>(), len)
        };
        write_stdout_bytes(slice, do_flush != 0);
    }

    unsafe extern "C" fn fe_action_cb(
        raw: *mut c_void,
        act_op: c_int,
        a1: c_int,
        a2: c_int,
        a3: c_int,
    ) -> i64 {
        let br = &*raw.cast::<FeBridgeCtx<'_>>();
        match act_op {
            0 => i64::from(br.stopped.load(std::sync::atomic::Ordering::SeqCst)),
            1 => {
                if br.mode != 3 && active_workstate().is_some_and(|w| w.pause_frontend()) {
                    if let Some(ws) = active_workstate() {
                        ws.wait_for_change(2000);
                    }
                } else if a1 > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(a1 as u64));
                }
                0
            }
            2 => {
                br.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
                if let Some(ws) = active_workstate() {
                    ws.wake_waiters();
                }
                0
            }
            3 => {
                if a2 != 0 {
                    crate::log::log_active_bytes(
                        0,
                        if a1 != 0 {
                            b"Pausing download"
                        } else {
                            b"Unpausing download"
                        },
                    );
                }
                if let Some(ws) = active_workstate() {
                    ws.set_resume_time(0);
                    ws.set_pause_download(a1 != 0);
                }
                0
            }
            4 => {
                if let Some(ws) = active_workstate() {
                    ws.set_speed_limit(a1);
                }
                0
            }
            5 => {
                let Some(qh) = queue_host() else {
                    return 0;
                };
                let arg = a2.to_string();
                let mut ok = false;
                qh.with_locked_queue(|dq_ptr| {
                    if dq_ptr != 0 {
                        ok = qh.dq_edit_entry(dq_ptr, a3, a1, Some(arg.as_bytes()));
                    }
                });
                i64::from(ok)
            }
            6 => {
                if let Some(cb) = br.exit_cb {
                    cb();
                }
                0
            }
            7 => libc::time(std::ptr::null_mut()) as i64,
            _ => 0,
        }
    }

    unsafe extern "C" fn fe_summary_cb(_raw: *mut c_void, s: *mut crate::binrpc::FrontendSummary) {
        if s.is_null() {
            return;
        }
        let out = &mut *s;
        let sm = crate::statmeter::active_statmeter();
        let ws = active_workstate();
        let (up_sec, dn_sec, all_bytes, stand_by) = sm
            .as_deref()
            .map_or((0, 0, 0, false), |m| m.total_stat_snapshot());
        out.current_download_speed = sm.as_deref().map_or(0, |m| m.current_download_speed());
        out.pause_download = c_int::from(ws.as_deref().is_some_and(|w| w.pause_download()));
        out.download_limit = ws.as_deref().map_or(0, |w| w.speed_limit());
        out.thread_count = current_thread_count();
        out.up_time_sec = up_sec;
        out.dn_time_sec = dn_sec;
        out.all_bytes = all_bytes;
        out.stand_by = c_int::from(stand_by);
        out.post_job_count = 0;
        out.remaining_size = 0;
        if let Some(qh) = queue_host() {
            qh.with_locked_queue(|dq_ptr| {
                if dq_ptr == 0 {
                    return;
                }
                let q_len = qh.queue_len(dq_ptr);
                let mut post_jobs = 0;
                for idx in 0..q_len {
                    let nzb_ptr = qh.queue_at(dq_ptr, idx);
                    if nzb_ptr.is_some() && qh.nzb_snap(nzb_ptr, false).has_post_info {
                        post_jobs += 1;
                    }
                }
                out.post_job_count = post_jobs;
                let (rem, _) = qh.dq_calc_remaining_size(dq_ptr);
                out.remaining_size = rem;
            });
        }
    }

    unsafe extern "C" fn fe_messages_cb(
        _raw: *mut c_void,
        tail_mode: c_int,
        arg: u32,
        out_skipped: *mut c_int,
        emit_ctx: *mut c_void,
        emit: crate::ncurses_format::FrontendEmitMsgFn,
    ) {
        let lg = crate::log::active_log();
        crate::log::nzbget_rs_log_collect(
            0,
            lg.as_deref().map_or(std::ptr::null(), |l| l as *const _),
            tail_mode,
            0,
            arg,
            out_skipped,
            emit_ctx,
            emit as usize,
            0,
        );
    }

    unsafe extern "C" fn fe_queue_cb(
        _raw: *mut c_void,
        emit_ctx: *mut c_void,
        emit_nzb: crate::ncurses_format::FrontendEmitNzbFn,
        emit_file: crate::ncurses_format::FrontendEmitFileFn,
    ) {
        let Some(qh) = queue_host() else {
            return;
        };
        qh.with_locked_queue(|dq_ptr| {
            if dq_ptr == 0 {
                return;
            }
            let q_len = qh.queue_len(dq_ptr);
            for idx in 0..q_len {
                let nzb_ptr = qh.queue_at(dq_ptr, idx);
                if nzb_ptr.is_none() {
                    continue;
                }
                let ns = qh.nzb_snap(nzb_ptr, false);
                let name_c = cstr::to_cstring(&ns.name);
                emit_nzb(
                    emit_ctx,
                    ns.id,
                    ns.priority,
                    ns.active_downloads,
                    ns.size,
                    ns.remaining_size,
                    ns.paused_size,
                    ns.paused_file_count,
                    ns.remaining_par_count,
                    name_c.as_ptr(),
                );
                let f_len = qh.nzb_file_len(nzb_ptr);
                for f_idx in 0..f_len {
                    let f_ptr = qh.nzb_file_at(nzb_ptr, f_idx);
                    if f_ptr.is_none() {
                        continue;
                    }
                    let fs = qh.file_snap(f_ptr);
                    let fname_c = cstr::to_cstring(&fs.filename);
                    emit_file(
                        emit_ctx,
                        idx,
                        fs.id,
                        fs.active_downloads,
                        c_int::from(fs.paused),
                        fs.size,
                        fs.remaining_size,
                        fname_c.as_ptr(),
                    );
                }
            }
        });
    }

    let mut br = FeBridgeCtx {
        mode,
        stopped,
        exit_cb,
    };
    let env = crate::ncurses_format::NzbgetRsFrontendEnv {
        ctx: (&mut br as *mut FeBridgeCtx<'_>).cast(),
        remote_mode: c_int::from(opts.as_deref().is_some_and(|o| o.remote_client_mode())),
        update_interval: opts.as_deref().map_or(100, |o| o.scalars().update_interval),
        use_color: c_int::from(use_color),
        show_nzbname: c_int::from(
            opts.as_deref()
                .is_some_and(|o| o.scalars().curses_nzb_name != 0),
        ),
        show_timestamp: c_int::from(
            opts.as_deref()
                .is_some_and(|o| o.scalars().curses_time != 0),
        ),
        group_files: c_int::from(
            opts.as_deref()
                .is_some_and(|o| o.scalars().curses_group != 0),
        ),
        time_correction: opts.as_deref().map_or(0, |o| o.scalars().time_correction),
        path_sep: crate::paths::PATH_SEPARATOR as u8,
        control_ip: opts
            .as_deref()
            .and_then(|o| o.str_cstr(OptStr::ControlIp))
            .map_or(std::ptr::null(), CStr::as_ptr),
        control_port: opts.as_deref().map_or(0, |o| o.scalars().control_port),
        control_username: opts
            .as_deref()
            .and_then(|o| o.str_cstr(OptStr::ControlUsername))
            .map_or(std::ptr::null(), CStr::as_ptr),
        control_password: opts
            .as_deref()
            .and_then(|o| o.str_cstr(OptStr::ControlPassword))
            .map_or(std::ptr::null(), CStr::as_ptr),
        write_stdout_cb: Some(fe_stdout_cb),
        action_cb: Some(fe_action_cb),
        local_summary_cb: Some(fe_summary_cb),
        local_messages_cb: Some(fe_messages_cb),
        local_queue_cb: Some(fe_queue_cb),
    };
    unsafe {
        crate::ncurses_format::frontend_op(2 + mode, &env);
    }

    if mode == 0 || mode == 1 {
        crate::log::log_active_bytes(3, b"Exiting LoggableFrontend-loop");
    } else if mode == 2 {
        crate::log::log_active_bytes(3, b"Exiting NCursesFrontend-loop");
    }
}
