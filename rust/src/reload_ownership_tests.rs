//! Config reloads during live news-server traffic (the 6790 SIGSEGV of
//! 2026-11-26 02:40:45 CST: a reload freed the connection pool while a live
//! stream repair's batch workers still fetched on its connections).
//!
//! The stress test runs in a child process of the test binary, so the pool
//! generations it installs and retires never meet other tests' globals.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::articlefetcher::{ArticleFetcher, BatchFetcherPool, NativeNntpHost, NntpHost};
use crate::net::{NzbgetRsServerPool, RsNewsServer};
use crate::nntp::BatchRequest;

const CHILD_ENV: &str = "RG_RELOAD_STRESS_CHILD";
const ROUNDS_ENV: &str = "RG_RELOAD_STRESS_ROUNDS";

fn payload(id: &[u8]) -> Vec<u8> {
    // never starts with a byte that encodes to '.', so no dot-stuffing is needed
    let seed = id
        .iter()
        .fold(7u32, |a, &b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    (0..3000u32)
        .map(|i| ((seed.wrapping_add(i * 13) % 200) as u8) + 20)
        .collect()
}

fn yenc_body(data: &[u8]) -> Vec<u8> {
    let crc = crate::decoder::yenc_crc(data, 0);
    let mut out = format!(
        "=ybegin part=1 total=1 line=128 size={0} name=r.bin\r\n=ypart begin=1 end={0}\r\n",
        data.len()
    )
    .into_bytes();
    for &b in data {
        let enc = b.wrapping_add(42);
        if matches!(enc, 0 | b'\n' | b'\r' | b'=' | b'.') {
            out.push(b'=');
            out.push(enc.wrapping_add(64));
        } else {
            out.push(enc);
        }
    }
    out.extend_from_slice(
        format!(
            "\r\n=yend size={} part=1 pcrc32={crc:08x}\r\n.\r\n",
            data.len()
        )
        .as_bytes(),
    );
    out
}

/// A minimal news server: every article exists; replies come after a short
/// pause so requests are in flight when the reloads hit.
fn serve(stream: TcpStream, served: Arc<AtomicUsize>) {
    let _ = stream.set_nodelay(true);
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut out = stream;
    let mut lines = BufReader::new(read_half);
    if out.write_all(b"200 test server ready\r\n").is_err() {
        return;
    }
    let mut line = Vec::new();
    loop {
        line.clear();
        match lines.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line).trim_end().to_string();
        let (verb, arg) = text.split_once(' ').unwrap_or((text.as_str(), ""));
        std::thread::sleep(Duration::from_micros(
            300 + (served.load(Ordering::Relaxed) % 7) as u64 * 200,
        ));
        let reply: Vec<u8> = match verb.to_ascii_uppercase().as_str() {
            "BODY" => {
                served.fetch_add(1, Ordering::Relaxed);
                let mut r = format!("222 0 {arg}\r\n").into_bytes();
                r.extend_from_slice(&yenc_body(&payload(arg.as_bytes())));
                r
            }
            "STAT" => format!("223 0 {arg}\r\n").into_bytes(),
            "GROUP" => format!("211 1 1 1 {arg}\r\n").into_bytes(),
            "QUIT" => {
                let _ = out.write_all(b"205 bye\r\n");
                return;
            }
            _ => b"500 what\r\n".to_vec(),
        };
        if out.write_all(&reply).is_err() {
            return;
        }
    }
}

fn start_server() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let served = Arc::new(AtomicUsize::new(0));
    let s = Arc::clone(&served);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let s = Arc::clone(&s);
            std::thread::spawn(move || serve(stream, s));
        }
    });
    (port, served)
}

/// What a reload builds: a new generation with the same server settings.
fn install_generation(port: u16, connections: i32) -> Arc<NzbgetRsServerPool> {
    let pool = crate::net::install_server_pool();
    pool.add_server(Arc::new(RsNewsServer::new(
        1,
        true,
        b"reload-test",
        b"127.0.0.1",
        i32::from(port),
        0,
        b"",
        b"",
        false,
        false,
        b"",
        connections,
        0,
        0,
        0,
        false,
        0,
    )));
    pool.init_connections();
    pool.set_light_cap(2);
    pool
}

fn rounds() -> usize {
    std::env::var(ROUNDS_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120)
}

/// Reloads the server pool over and over while a live stream repair's batch
/// fetch (the crashing path: batch worker, `ArticleFetcher::fetch`, the
/// connection's `request`), borrow-wave fetches on light connections, fleet
/// probe reads and a held connection (a fleet's, across several reloads) all
/// keep fetching. Under the old ownership the first fetch still running on a
/// freed generation touched freed memory; now each finishes on its own
/// generation, which is freed after its last connection is back.
#[test]
#[ignore = "run by reload_during_fetches_never_frees_a_connection_in_use in a child process"]
fn reload_stress_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let (port, served) = start_server();
    let mut current = install_generation(port, 12);
    let stop = Arc::new(AtomicBool::new(false));
    let fetched = Arc::new(AtomicUsize::new(0));
    let held_across = Arc::new(AtomicUsize::new(0));
    let generations = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();

    // the live stream repair: batches of BODY fetches on 4 workers
    for batch_no in 0..2 {
        let (stop, fetched) = (Arc::clone(&stop), Arc::clone(&fetched));
        workers.push(std::thread::spawn(move || {
            let groups = Arc::new(vec![b"alt.binaries.test".to_vec()]);
            let mut round = 0usize;
            while !stop.load(Ordering::SeqCst) {
                let pool = BatchFetcherPool::new(&NativeNntpHost);
                pool.set_worker_count(4);
                let requests = (0..24)
                    .map(|i| BatchRequest {
                        message_id: format!("<r{batch_no}-{round}-{i}@test>").into_bytes(),
                        groups: Arc::clone(&groups),
                    })
                    .collect();
                pool.begin(requests);
                while let Some(got) = pool.next() {
                    if got.success {
                        fetched.fetch_add(1, Ordering::Relaxed);
                    }
                }
                pool.stop();
                round += 1;
            }
        }));
    }
    // borrow waves on light connections, and fleet probe reads
    let hosts: [&'static dyn NntpHost; 2] = [
        &crate::articlefetcher::LIGHT_HOST,
        &crate::fleetprobe::PROBE_HOST,
    ];
    for (n, host) in hosts.into_iter().enumerate() {
        let (stop, fetched) = (Arc::clone(&stop), Arc::clone(&fetched));
        workers.push(std::thread::spawn(move || {
            let fetcher = ArticleFetcher::new(host);
            let mut i = 0usize;
            while !stop.load(Ordering::SeqCst) {
                let id = format!("<h{n}-{i}@test>").into_bytes();
                if fetcher.fetch(&id, &[]).success {
                    fetched.fetch_add(1, Ordering::Relaxed);
                }
                i += 1;
            }
        }));
    }
    // a held connection that lives through reloads
    {
        let (stop, held_across, generations) = (
            Arc::clone(&stop),
            Arc::clone(&held_across),
            Arc::clone(&generations),
        );
        workers.push(std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let Some(got) = NativeNntpHost.get_connection(0, &[]) else {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                };
                let before = generations.load(Ordering::SeqCst);
                let mut ok = NativeNntpHost.connect(got.conn);
                for _ in 0..4 {
                    std::thread::sleep(Duration::from_millis(8));
                    ok &= NativeNntpHost
                        .request(got.conn, b"STAT <held@test>\r\n")
                        .is_some();
                }
                if ok && generations.load(Ordering::SeqCst) != before {
                    held_across.fetch_add(1, Ordering::Relaxed);
                }
                NativeNntpHost.release(got.conn, true);
            }
        }));
    }

    let started = Instant::now();
    let mut retired_seen = 0usize;
    for round in 0..rounds() {
        std::thread::sleep(Duration::from_millis(3 + (round % 9) as u64 * 2));
        // the reload: the daemon lets go of the old generation first ...
        crate::net::release_server_pool(current);
        generations.fetch_add(1, Ordering::SeqCst);
        retired_seen = retired_seen.max(crate::net::retired_pools_alive());
        std::thread::sleep(Duration::from_millis(1));
        // ... and the reloaded one builds the next (other settings each time)
        current = install_generation(port, 8 + (round % 3) as i32 * 4);
    }
    stop.store(true, Ordering::SeqCst);
    for w in workers {
        w.join().expect("a fetch thread panicked");
    }
    crate::net::release_server_pool(current);
    // every retired generation is freed once its connections are back
    let deadline = Instant::now() + Duration::from_secs(10);
    while crate::net::retired_pools_alive() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let alive = crate::net::retired_pools_alive();
    let (fetched, held_across) = (
        fetched.load(Ordering::SeqCst),
        held_across.load(Ordering::SeqCst),
    );
    println!(
        "reload-stress: rounds={} secs={:.1} articles_served={} fetched_ok={fetched} \
         held_across_reload={held_across} max_retired_alive={retired_seen} retired_left={alive}",
        rounds(),
        started.elapsed().as_secs_f64(),
        served.load(Ordering::SeqCst),
    );
    assert_eq!(alive, 0, "a retired generation was never freed");
    assert!(fetched > 0, "nothing was fetched");
    assert!(held_across > 0, "no held connection lived through a reload");
    assert!(
        retired_seen > 0,
        "no reload happened while a connection was out"
    );
}

#[test]
fn reload_during_fetches_never_frees_a_connection_in_use() {
    let exe = std::env::current_exe().expect("test binary");
    let out = std::process::Command::new(exe)
        .args([
            "--exact",
            "reload_ownership_tests::reload_stress_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, "1")
        .output()
        .expect("run the child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("{stdout}");
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "child failed ({:?}):\n{stdout}\n{stderr}",
        out.status
    );
}
