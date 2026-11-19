//! Which address a news server connects to (`ServerN.Address`).
//!
//! A host name with several addresses is resolved here, every address is timed once, and
//! connections go to the fastest first (Auto). A pinned address (`ServerN.Address`) goes
//! first instead. Either way the host name is kept for TLS (SNI and certificate checks), and
//! when an address fails at the network level (connect, TLS, timeout, greeting) the next one is
//! tried until none are left. A login error never moves to another address: it would fail
//! the same way everywhere.
//!
//! Literal addresses, `localhost` and Unix sockets are left to the socket layer as before:
//! nothing is resolved or timed for them.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::log::{MK_INFO, MK_WARNING};

/// How long a resolved address list is used before the host is looked up again.
const DNS_TTL: Duration = Duration::from_secs(10 * 60);
/// How long an address that failed waits behind the others.
const DEAD_FOR: Duration = Duration::from_secs(5 * 60);
/// Time allowed for each address's timing connect.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// The least time between two timings of one host after failures.
const RERANK_GAP: Duration = Duration::from_secs(60);
/// How long a connect waits for the first lookup of its host by another thread.
const FIRST_LOOKUP_WAIT: Duration = Duration::from_secs(5);
/// A new fastest address takes over only when the current one is slower than this much.
const KEEP_FACTOR: f64 = 1.2;
const KEEP_SLACK_MS: f64 = 5.0;
/// The most addresses one connect tries, so a server that drops every packet
/// fails in a few timeouts instead of one timeout per address.
const MAX_TRIES: usize = 3;

#[derive(Default)]
struct Entry {
    host: String,
    port: u16,
    ip_version: i32,
    resolved_at: Option<Instant>,
    /// Addresses, fastest first; ones that did not answer the timing connect last.
    ranked: Vec<String>,
    latency: HashMap<String, Option<f64>>,
    dead: HashMap<String, Instant>,
    /// Another thread is resolving and timing right now.
    busy: bool,
    /// Time the addresses again at the next refresh (a failure or a changed list).
    rerank: bool,
    /// Look the host up again at the next connect, whatever its age.
    stale: bool,
    /// When the addresses were last timed.
    ranked_at: Option<Instant>,
    warned_pin: Option<String>,
}

fn pins() -> &'static Mutex<HashMap<i32, String>> {
    static PINS: OnceLock<Mutex<HashMap<i32, String>>> = OnceLock::new();
    PINS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn entries() -> &'static Mutex<HashMap<i32, Entry>> {
    static ENTRIES: OnceLock<Mutex<HashMap<i32, Entry>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// An address as `ServerN.Address` takes it: an IPv4 or IPv6 literal, brackets and an
/// IPv6 `%interface` allowed. Returns the normal form, or `None`.
pub fn normalize(value: &str) -> Option<String> {
    let v = value.trim();
    let v = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(v);
    if v.is_empty() {
        return None;
    }
    let (ip_part, scope) = match v.split_once('%') {
        Some((ip, scope)) => (ip, Some(scope)),
        None => (v, None),
    };
    let ip: IpAddr = ip_part.parse().ok()?;
    match (ip, scope) {
        (IpAddr::V4(_), Some(_)) => None,
        (_, Some(s)) if s.is_empty() || s.bytes().any(|b| b.is_ascii_whitespace() || b == b'/') => None,
        (_, Some(s)) => Some(format!("{ip}%{s}")),
        (_, None) => Some(ip.to_string()),
    }
}

/// Sets (or, with an empty value, clears) the pinned address of server `id`. Called while
/// the options load; an invalid value is the caller's to report.
pub fn configure(id: i32, pinned: &str) {
    let mut map = pins().lock().unwrap_or_else(|e| e.into_inner());
    match normalize(pinned) {
        Some(addr) => {
            map.insert(id, addr);
        }
        None => {
            map.remove(&id);
        }
    }
    drop(map);
    if let Some(e) = entries().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&id) {
        e.warned_pin = None;
    }
}

pub fn pinned(id: i32) -> Option<String> {
    pins().lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned()
}

/// Whether `host` gets an address plan: a name other than localhost.
pub fn wants_plan(host: &str) -> bool {
    let h = host.trim();
    if h.is_empty() || h.starts_with('/') {
        return false;
    }
    if normalize(h).is_some() {
        return false;
    }
    let lower = h.trim_end_matches('.').to_ascii_lowercase();
    !(lower == "localhost" || lower.ends_with(".localhost"))
}

fn family_ok(addr: &str, ip_version: i32) -> bool {
    let v6 = addr.contains(':');
    match ip_version {
        4 => !v6,
        6 => v6,
        _ => true,
    }
}

/// The order to try: the pin first (unless it failed recently, or it is no longer an
/// address of the host and may not be used outside it), then live addresses fastest
/// first, then the ones that failed recently. Returns the order and whether the pin was
/// dropped for having left the host's addresses.
fn order(
    pin: Option<&str>,
    pin_outside_ok: bool,
    ranked: &[String],
    is_dead: &dyn Fn(&str) -> bool,
) -> (Vec<String>, bool) {
    let mut out: Vec<String> = Vec::new();
    let mut dropped = false;
    let mut pin_late: Option<String> = None;
    if let Some(p) = pin {
        let known = ranked.iter().any(|a| a == p);
        if !known && !pin_outside_ok {
            dropped = true;
        } else if is_dead(p) {
            pin_late = Some(p.to_string());
        } else {
            out.push(p.to_string());
        }
    }
    for a in ranked {
        if !out.contains(a) && pin_late.as_deref() != Some(a.as_str()) && !is_dead(a) {
            out.push(a.clone());
        }
    }
    let live = out.len();
    if let Some(p) = pin_late {
        out.push(p);
    }
    for a in ranked {
        if !out.contains(a) {
            out.push(a.clone());
        }
    }
    // every address failed recently: try the best one (or the pin) once, not all of them
    out.truncate(if live == 0 { 1 } else { MAX_TRIES });
    (out, dropped)
}

/// Fastest first, unreachable last, keeping `prev_best` first while it is within
/// `KEEP_FACTOR`/`KEEP_SLACK_MS` of the fastest.
fn rank(addrs: &[String], latency: &HashMap<String, Option<f64>>, prev_best: Option<&str>) -> Vec<String> {
    let mut list: Vec<String> = addrs.to_vec();
    let key = |a: &String| latency.get(a).copied().flatten();
    list.sort_by(|a, b| match (key(a), key(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    if let (Some(prev), Some(best)) = (prev_best, list.first().and_then(|a| key(a))) {
        if let Some(pos) = list.iter().position(|a| a == prev) {
            if let Some(prev_ms) = key(&list[pos]) {
                if prev_ms <= best * KEEP_FACTOR + KEEP_SLACK_MS {
                    let p = list.remove(pos);
                    list.insert(0, p);
                }
            }
        }
    }
    list
}

fn time_addresses(addrs: &[SocketAddr]) -> Vec<Option<f64>> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = addrs
            .iter()
            .map(|a| {
                let a = *a;
                std::thread::Builder::new()
                    .name("addrpick".into())
                    .spawn_scoped(scope, move || crate::hostlookup::connect_ms(a, PROBE_TIMEOUT))
                    .map_err(|_| a)
            })
            .collect();
        handles
            .into_iter()
            .map(|h| match h {
                Ok(h) => h.join().ok().flatten(),
                Err(a) => crate::hostlookup::connect_ms(a, PROBE_TIMEOUT),
            })
            .collect()
    })
}

/// Server facts the plan needs.
pub struct ServerFacts<'a> {
    pub id: i32,
    pub name: &'a str,
    pub host: &'a str,
    pub port: i32,
    pub ip_version: i32,
    pub tls: bool,
    pub cert_verif_level: u32,
}

/// The addresses to try, in order, for one connect. Empty: let the socket layer resolve
/// the host itself (a literal, localhost, or a lookup that failed here, so its own error
/// reaches the log).
pub fn plan(s: &ServerFacts<'_>) -> Vec<String> {
    if !wants_plan(s.host) {
        return Vec::new();
    }
    let port = u16::try_from(s.port).ok().filter(|p| *p > 0).unwrap_or(119);
    let refresh = {
        let mut map = entries().lock().unwrap_or_else(|e| e.into_inner());
        let e = map.entry(s.id).or_default();
        if e.host != s.host || e.port != port || e.ip_version != s.ip_version {
            *e = Entry {
                host: s.host.to_string(),
                port,
                ip_version: s.ip_version,
                ..Entry::default()
            };
        }
        let due = e.stale
            || e.resolved_at.is_none_or(|t| t.elapsed() >= DNS_TTL)
            || (e.rerank && e.ranked_at.is_none_or(|t| t.elapsed() >= RERANK_GAP));
        if due && !e.busy {
            e.busy = true;
            true
        } else {
            false
        }
    };
    if refresh {
        refresh_entry(s, port);
    } else {
        // another thread is looking the host up for the first time: wait for it, so the
        // pin and the ranking apply to every connection from the start
        let start = Instant::now();
        while start.elapsed() < FIRST_LOOKUP_WAIT {
            let waiting = entries()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&s.id)
                .is_some_and(|e| e.busy && e.ranked.is_empty());
            if !waiting {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    let pin = pinned(s.id);
    let pin_outside_ok = s.tls && s.cert_verif_level >= 2;
    let mut map = entries().lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = map.get_mut(&s.id) else {
        return Vec::new();
    };
    let now = Instant::now();
    e.dead.retain(|_, t| now.duration_since(*t) < DEAD_FOR);
    if e.ranked.is_empty() {
        // not resolved yet (another thread is at it) or the lookup failed: a pin that may
        // stand alone is still honored, otherwise the socket layer resolves as before
        return match pin {
            Some(p) if pin_outside_ok && family_ok(&p, s.ip_version) => vec![p],
            _ => Vec::new(),
        };
    }
    let pin = pin.filter(|p| family_ok(p, s.ip_version));
    let dead = e.dead.clone();
    let (list, dropped) = order(pin.as_deref(), pin_outside_ok, &e.ranked, &|a| dead.contains_key(a));
    if dropped {
        let p = pin.unwrap_or_default();
        if e.warned_pin.as_deref() != Some(p.as_str()) {
            e.warned_pin = Some(p.clone());
            crate::log::log_active_bytes(
                MK_WARNING,
                format!(
                    "{}: pinned address {p} is no longer an address of {}; using the others. \
                     With encryption and strict certificate checks on, it would still be tried",
                    s.name, s.host
                )
                .as_bytes(),
            );
        }
    }
    list
}

fn same_target(e: &Entry, s: &ServerFacts<'_>, port: u16) -> bool {
    e.host == s.host && e.port == port && e.ip_version == s.ip_version
}

fn refresh_entry(s: &ServerFacts<'_>, port: u16) {
    // every address (a pin may be past the first few, and a rotating DNS answer must not
    // look like a changed list); only the first MAX_ADDRESSES are timed
    let found = crate::hostlookup::resolve_all(s.host, port).map(|ips| {
        ips.into_iter()
            .filter(|ip| match s.ip_version {
                4 => ip.is_ipv4(),
                6 => ip.is_ipv6(),
                _ => true,
            })
            .collect::<Vec<_>>()
    });
    let mut map = entries().lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = map.get_mut(&s.id).filter(|e| same_target(e, s, port)) else {
        // the server's host changed meanwhile: these results are for the old one
        return;
    };
    e.busy = false;
    e.stale = false;
    e.resolved_at = Some(Instant::now());
    let ips = match found {
        Ok(ips) if !ips.is_empty() => ips,
        // keep what was known; the next try after DNS_TTL (or RERANK_GAP) looks again
        _ => {
            if e.rerank {
                e.ranked_at = Some(Instant::now());
            }
            return;
        }
    };
    let names: Vec<String> = ips.iter().map(crate::hostlookup::shown_address).collect();
    let changed = {
        let mut a = names.clone();
        let mut b = e.ranked.clone();
        a.sort();
        b.sort();
        a != b
    };
    if !changed && !e.rerank {
        return;
    }
    let prev_best = e.ranked.first().cloned();
    if names.len() < 2 {
        e.ranked = names;
        e.rerank = false;
        e.ranked_at = Some(Instant::now());
        return;
    }
    e.busy = true;
    drop(map);
    let timed = crate::hostlookup::pick_addresses(ips.clone());
    let timed_times = time_addresses(&timed);
    let timed_names: Vec<String> = timed.iter().map(crate::hostlookup::shown_address).collect();
    let times: Vec<Option<f64>> = names
        .iter()
        .map(|n| {
            timed_names
                .iter()
                .position(|t| t == n)
                .and_then(|i| timed_times[i])
        })
        .collect();
    let mut map = entries().lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = map.get_mut(&s.id).filter(|e| same_target(e, s, port)) else {
        return;
    };
    e.busy = false;
    e.rerank = false;
    e.ranked_at = Some(Instant::now());
    e.latency = names.iter().cloned().zip(times).collect();
    let ranked = rank(&names, &e.latency, prev_best.as_deref());
    if ranked.first() != prev_best.as_ref() {
        if let Some(best) = ranked.first() {
            let ms = e.latency.get(best).copied().flatten();
            // nothing answered the timing: no "fastest" to report
            if ms.is_some() {
            crate::log::log_active_bytes(
                MK_INFO,
                format!(
                    "{}: connecting to {best} first, the fastest of {} addresses for {}{}",
                    s.name,
                    ranked.len(),
                    s.host,
                    ms.map(|m| format!(" ({m} ms)")).unwrap_or_default()
                )
                .as_bytes(),
            );
            }
        }
    }
    e.ranked = ranked;
}

/// The outcome of a connect to `addr`: `false` for a network-level failure (connect, TLS,
/// timeout, greeting). A failure moves the address behind the others for a while; when
/// every address has failed the host is looked up again at the next connect.
pub fn report(id: i32, addr: &str, ok: bool) {
    let mut map = entries().lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = map.get_mut(&id) else {
        return;
    };
    if ok {
        e.dead.remove(addr);
        return;
    }
    e.dead.insert(addr.to_string(), Instant::now());
    // time the addresses again, at most once per RERANK_GAP (a provider that is down
    // fails every address on every connect)
    let may_rerank = e.ranked_at.is_none_or(|t| t.elapsed() >= RERANK_GAP);
    if e.ranked.first().map(String::as_str) == Some(addr) && may_rerank {
        e.rerank = true;
    }
    if !e.ranked.is_empty() && e.ranked.iter().all(|a| e.dead.contains_key(a)) {
        e.stale = true;
        e.rerank |= may_rerank;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn normalize_accepts_literals_only() {
        assert_eq!(normalize(" 1.2.3.4 ").as_deref(), Some("1.2.3.4"));
        assert_eq!(normalize("[2001:db8::1]").as_deref(), Some("2001:db8::1"));
        assert_eq!(normalize("fe80::1%eth0").as_deref(), Some("fe80::1%eth0"));
        assert_eq!(normalize("1.2.3.4%eth0"), None);
        assert_eq!(normalize("news.example.com"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("1.2.3"), None);
    }

    #[test]
    fn plan_skips_literals_and_localhost() {
        assert!(!wants_plan("127.0.0.1"));
        assert!(!wants_plan("::1"));
        assert!(!wants_plan("localhost"));
        assert!(!wants_plan("proxy.localhost"));
        assert!(!wants_plan("/run/nntp.sock"));
        assert!(!wants_plan(""));
        assert!(wants_plan("news.example.com"));
        assert!(wants_plan("nas"));
    }

    #[test]
    fn order_pin_first_then_live_then_dead() {
        let ranked = s(&["a", "b", "c"]);
        let none = |_: &str| false;
        assert_eq!(order(None, false, &ranked, &none).0, ranked);
        assert_eq!(order(Some("c"), false, &ranked, &none), (s(&["c", "a", "b"]), false));
        let b_dead = |x: &str| x == "b";
        assert_eq!(order(None, false, &ranked, &b_dead).0, s(&["a", "c", "b"]));
        // a pin that failed recently waits behind the live ones, ahead of the other dead ones
        let ab_dead = |x: &str| x == "a" || x == "b";
        assert_eq!(order(Some("b"), false, &ranked, &ab_dead).0, s(&["c", "b", "a"]));
        // all dead: one try, the pin first when there is one
        let all_dead = |_: &str| true;
        assert_eq!(order(None, false, &ranked, &all_dead).0, s(&["a"]));
        assert_eq!(order(Some("c"), false, &ranked, &all_dead).0, s(&["c"]));
        // many addresses: a connect tries at most MAX_TRIES of them
        let many = s(&["a", "b", "c", "d", "e"]);
        assert_eq!(order(None, false, &many, &none).0.len(), MAX_TRIES);
    }

    #[test]
    fn pin_outside_dns_needs_strict_tls() {
        let ranked = s(&["a", "b"]);
        let none = |_: &str| false;
        assert_eq!(order(Some("z"), false, &ranked, &none), (s(&["a", "b"]), true));
        assert_eq!(order(Some("z"), true, &ranked, &none), (s(&["z", "a", "b"]), false));
    }

    #[test]
    fn rank_fastest_first_with_hysteresis() {
        let names = s(&["a", "b", "c"]);
        let lat: HashMap<String, Option<f64>> =
            [("a".into(), Some(30.0)), ("b".into(), Some(10.0)), ("c".into(), None)].into_iter().collect();
        assert_eq!(rank(&names, &lat, None), s(&["b", "a", "c"]));
        // a is 30 ms against 10: too slow to keep
        assert_eq!(rank(&names, &lat, Some("a")), s(&["b", "a", "c"]));
        let close: HashMap<String, Option<f64>> =
            [("a".into(), Some(14.0)), ("b".into(), Some(10.0)), ("c".into(), Some(50.0))].into_iter().collect();
        // a at 14 ms is within 10 * 1.2 + 5: it stays first
        assert_eq!(rank(&names, &close, Some("a")), s(&["a", "b", "c"]));
        // an unreachable previous best never stays first
        assert_eq!(rank(&names, &lat, Some("c")), s(&["b", "a", "c"]));
    }

    #[test]
    fn report_marks_dead_and_restales_when_all_fail() {
        let id = -4242;
        {
            let mut map = entries().lock().unwrap();
            map.insert(
                id,
                Entry {
                    ranked: s(&["a", "b"]),
                    resolved_at: Some(Instant::now()),
                    ..Entry::default()
                },
            );
        }
        report(id, "a", false);
        {
            let map = entries().lock().unwrap();
            let e = map.get(&id).unwrap();
            assert!(e.dead.contains_key("a") && e.rerank && !e.stale);
        }
        report(id, "b", false);
        assert!(entries().lock().unwrap().get(&id).unwrap().stale);
        report(id, "a", true);
        assert!(!entries().lock().unwrap().get(&id).unwrap().dead.contains_key("a"));
        entries().lock().unwrap().remove(&id);
    }

    #[test]
    fn a_failed_first_address_retimes_soon_but_not_too_often() {
        let id = -5151;
        let facts = ServerFacts {
            id,
            name: "t",
            host: "rerank.invalid",
            port: 119,
            ip_version: 0,
            tls: false,
            cert_verif_level: 2,
        };
        let long_ago = Instant::now() - Duration::from_secs(3);
        entries().lock().unwrap().insert(
            id,
            Entry {
                host: "rerank.invalid".into(),
                port: 119,
                ranked: s(&["192.0.2.1", "192.0.2.2"]),
                resolved_at: Some(long_ago),
                ..Entry::default()
            },
        );
        report(id, "192.0.2.1", false);
        // the failed address goes behind the other one at once
        assert_eq!(plan(&facts), s(&["192.0.2.2", "192.0.2.1"]));
        // and the host was looked up again (the lookup fails here: the list is kept)
        let (resolved, busy) = {
            let map = entries().lock().unwrap();
            let e = map.get(&id).unwrap();
            (e.resolved_at.unwrap(), e.busy)
        };
        assert!(resolved > long_ago && !busy);
        // another failure right after does not time the host again within RERANK_GAP
        {
            let mut map = entries().lock().unwrap();
            let e = map.get_mut(&id).unwrap();
            e.ranked_at = Some(Instant::now());
            e.resolved_at = Some(long_ago);
        }
        report(id, "192.0.2.1", false);
        plan(&facts);
        assert_eq!(entries().lock().unwrap().get(&id).unwrap().resolved_at, Some(long_ago));
        entries().lock().unwrap().remove(&id);
    }

    #[test]
    fn configure_sets_and_clears_pin() {
        configure(-777, "[2001:db8::5]");
        assert_eq!(pinned(-777).as_deref(), Some("2001:db8::5"));
        configure(-777, "");
        assert_eq!(pinned(-777), None);
    }
}
