//! RustyGet's news servers as an nntp-rs pool: the server options mapped onto
//! `nntp_rs::ServerConfig`, and backend identification reported in the log.
//!
//! A server with a `Backbone` keeps it (nntp-rs groups by that name unless
//! the evidence contradicts it); a server without one is left to nntp-rs's
//! detection (`backbone` "auto"), so servers reading one article store are
//! found without configuration and a 430 on one skips its siblings.

use crate::net::RsNewsServer;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// `ServerN.*` as an nntp-rs server. `light_cap` is `LightConnections`.
pub fn server_config(srv: &RsNewsServer, light_cap: usize) -> nntp_rs::ServerConfig {
    let text = |c: &std::ffi::CString| String::from_utf8_lossy(c.to_bytes()).into_owned();
    let mut cfg = nntp_rs::ServerConfig::new(text(&srv.host))
        .with_name(text(&srv.name))
        .with_port(u16::try_from(srv.port).unwrap_or(119))
        .with_tls(if srv.tls {
            nntp_rs::TlsMode::OpenSsl
        } else {
            nntp_rs::TlsMode::None
        })
        .with_max_connections(usize::try_from(srv.max_connections).unwrap_or(1).max(1))
        .with_level(u32::try_from(srv.level).unwrap_or(0))
        .with_light_connections(light_cap)
        // a server with no connections was never used: it stays out
        .with_active(srv.active.load(Ordering::SeqCst) && srv.max_connections > 0)
        .with_group(u32::try_from(srv.configured_group).unwrap_or(0))
        .with_retention_days(u32::try_from(srv.retention).unwrap_or(0));
    let user = srv.user.to_bytes();
    if !user.is_empty() {
        cfg = cfg.with_auth(
            String::from_utf8_lossy(user).into_owned(),
            text(&srv.password),
        );
    }
    cfg = cfg
        .with_cert_verification(match srv.cert_verif_level {
            0 => nntp_rs::CertVerification::None,
            1 => nntp_rs::CertVerification::Minimal,
            _ => nntp_rs::CertVerification::Strict,
        })
        .with_cipher(text(&srv.cipher))
        .with_ip_version(match srv.ip_version {
            4 => nntp_rs::IpVersion::V4,
            6 => nntp_rs::IpVersion::V6,
            _ => nntp_rs::IpVersion::Auto,
        });
    #[cfg(unix)]
    if crate::addrpick::wants_plan(&cfg.host) {
        cfg = cfg.with_address_plan(std::sync::Arc::new(AddrPlan::new(srv)));
    }
    cfg.optional = srv.optional;
    // what the user typed, never a backbone identification made earlier
    cfg.backbone = Some(
        crate::backbones::name_of(srv.configured_group).unwrap_or_else(|| "auto".to_string()),
    );
    cfg.io_timeout = Duration::from_secs(60);
    cfg
}

/// `ServerN.Address` for nntp-rs (`crate::addrpick`): the addresses of the
/// host fastest first, or the pinned one first; network-level failures move
/// an address back. nntp-rs asks on its I/O threads, which must not block, so
/// the order is worked out (looked up and timed) on a thread of its own and
/// kept; until the first one is ready nntp-rs resolves the host itself.
#[cfg(unix)]
struct AddrPlan {
    id: i32,
    name: String,
    host: String,
    port: i32,
    ip_version: i32,
    tls: bool,
    cert_verif_level: u32,
    cached: std::sync::Arc<std::sync::Mutex<(Vec<std::net::SocketAddr>, Option<std::time::Instant>, bool)>>,
}

#[cfg(unix)]
impl AddrPlan {
    /// How long a worked-out order is used before it is worked out again.
    const KEEP: Duration = Duration::from_secs(60);

    fn new(srv: &RsNewsServer) -> Self {
        let text = |c: &std::ffi::CString| String::from_utf8_lossy(c.to_bytes()).into_owned();
        Self {
            id: srv.id,
            name: text(&srv.name),
            host: text(&srv.host),
            port: srv.port,
            ip_version: srv.ip_version,
            tls: srv.tls,
            cert_verif_level: srv.cert_verif_level,
            cached: Default::default(),
        }
    }

    fn refresh(&self, port: u16) {
        let (id, name, host, p, ipv, tls, cv) = (
            self.id,
            self.name.clone(),
            self.host.clone(),
            self.port,
            self.ip_version,
            self.tls,
            self.cert_verif_level,
        );
        let cached = std::sync::Arc::clone(&self.cached);
        let retry = std::sync::Arc::clone(&self.cached);
        let spawned = std::thread::Builder::new().name("addrplan".into()).spawn(move || {
            let order = crate::addrpick::plan(&crate::addrpick::ServerFacts {
                id,
                name: &name,
                host: &host,
                port: p,
                ip_version: ipv,
                tls,
                cert_verif_level: cv,
            });
            let addrs: Vec<std::net::SocketAddr> = order
                .iter()
                .filter_map(|a| a.trim_matches(|c| c == '[' || c == ']').parse::<std::net::IpAddr>().ok())
                .map(|ip| std::net::SocketAddr::new(ip, port))
                .collect();
            if let Ok(mut g) = cached.lock() {
                *g = (addrs, Some(std::time::Instant::now()), false);
            }
        });
        if spawned.is_err() {
            if let Ok(mut g) = retry.lock() {
                g.2 = false;
            }
        }
    }
}

#[cfg(unix)]
impl nntp_rs::AddressPlan for AddrPlan {
    fn addresses(&self, _host: &str, port: u16) -> Vec<std::net::SocketAddr> {
        let Ok(mut g) = self.cached.lock() else {
            return Vec::new();
        };
        let stale = g.1.is_none_or(|t| t.elapsed() >= Self::KEEP);
        if stale && !g.2 {
            g.2 = true;
            drop(g);
            self.refresh(port);
            return self.cached.lock().map(|g| g.0.clone()).unwrap_or_default();
        }
        g.0.clone()
    }

    fn report(&self, addr: std::net::SocketAddr, ok: bool) {
        crate::addrpick::report(self.id, &addr.ip().to_string(), ok);
        if !ok {
            // the next connect works the order out again
            if let Ok(mut g) = self.cached.lock() {
                g.1 = None;
            }
        }
    }
}

/// What identification learned about one server's article numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Not asked (inactive, or no idle connection to lend), or the probe
    /// failed (connect, login, TLS, timeout).
    NotProbed,
    /// Asked, but it gave no article numbers that another probed server
    /// also gave, so nothing could be compared.
    NothingToCompare,
    /// Its article numbers were compared with another server's.
    Compared,
}

/// [`Probe`] for each server from `fingerprints()` (`None`: not probed): a
/// server counts as compared only when it gave watermarks for a group that
/// another server also gave.
pub fn probe_states(prints: &[Option<nntp_rs::Fingerprint>], n: usize) -> Vec<Probe> {
    (0..n)
        .map(|i| match prints.get(i).and_then(Option::as_ref) {
            None => Probe::NotProbed,
            Some(fp) => {
                let shared = fp.watermarks.keys().any(|g| {
                    prints.iter().enumerate().any(|(j, other)| {
                        j != i && other.as_ref().is_some_and(|o| o.watermarks.contains_key(g))
                    })
                });
                if shared {
                    Probe::Compared
                } else {
                    Probe::NothingToCompare
                }
            }
        })
        .collect()
}

/// One log line per server saying which servers share its article store,
/// from `identify_backends`' verdicts (in server order). A server that shares
/// with none and whose numbers were not compared is reported as such, never
/// as "on its own" (unknown stays unknown).
pub fn backend_log_lines(names: &[String], probes: &[Probe], verdicts: &[nntp_rs::Verdict]) -> Vec<String> {
    use nntp_rs::backend::{Contradiction, Finding};
    names
        .iter()
        .zip(verdicts)
        .enumerate()
        .map(|(i, (name, v))| {
            if v.peers.is_empty() {
                match probes.get(i).copied().unwrap_or(Probe::NotProbed) {
                    Probe::NotProbed => {
                        return format!(
                            "Backend of {name}: not probed (inactive, no idle connection, or no answer, login or TLS failure)"
                        )
                    }
                    Probe::NothingToCompare => {
                        return format!("Backend of {name}: nothing to compare (no article numbers another server also gave)")
                    }
                    Probe::Compared => {}
                }
            }
            let how = match &v.finding {
                Finding::Detected => "detected".to_string(),
                Finding::Confirmed => "configured backbone, confirmed".to_string(),
                Finding::Unchecked => "configured backbone, nothing to compare".to_string(),
                Finding::Misconfigured { against, why } => {
                    let why = match why {
                        Contradiction::Numbers => "their article numbers differ",
                        Contradiction::Retention => "their retention differs",
                        Contradiction::Articles { .. } => "the evidence differs",
                    };
                    format!("its configured backbone puts it with server {against}, but {why}")
                }
                Finding::Alone => "no other server shares its store".to_string(),
            };
            if v.peers.is_empty() {
                format!("Backend of {name}: store {} on its own ({how})", v.identity)
            } else {
                format!(
                    "Backend of {name}: store {} shared with {} ({how})",
                    v.identity,
                    v.peers.join(", ")
                )
            }
        })
        .collect()
}

/// The backbone each server should be on after identification, given its
/// configured backbone (`groups`, `ServerN.Backbone`, 0 = none) and the
/// verdicts, in server order; `intern` names a new backbone. The servers of
/// one store all go on one backbone: the first configured one a member has
/// that the evidence does not contradict, else a new one named after the
/// store's first server. A server alone in its store keeps its configured
/// backbone unless the evidence contradicts it (a wrong grouping would skip
/// servers that hold the article).
pub fn identified_groups(
    names: &[String],
    groups: &[i32],
    verdicts: &[nntp_rs::Verdict],
    mut intern: impl FnMut(&str) -> i32,
) -> Vec<i32> {
    // a new backbone's id must not be one a configured backbone or another
    // store of this run already has: server names need not be unique
    let mut taken: Vec<i32> = groups.iter().copied().filter(|&g| g > 0).collect();
    let mut fresh = |first: usize, taken: &mut Vec<i32>| -> Option<i32> {
        let mut name = format!("Store of {}", names[first]);
        for k in 2..=64 {
            let id = intern(&name);
            if id <= 0 {
                return None; // the backbone table is unusable: leave this store alone
            }
            if !taken.contains(&id) {
                taken.push(id);
                return Some(id);
            }
            name = format!("Store of {} ({k})", names[first]);
        }
        None
    };
    use nntp_rs::backend::Finding;
    let n = names.len().min(groups.len()).min(verdicts.len());
    let mut out: Vec<i32> = groups[..n].to_vec();
    let mut identities: Vec<usize> = verdicts[..n].iter().map(|v| v.identity).collect();
    identities.sort_unstable();
    identities.dedup();
    for identity in identities {
        let members: Vec<usize> = (0..n)
            .filter(|&i| verdicts[i].identity == identity)
            .collect();
        let contradicted = |i: usize| matches!(verdicts[i].finding, Finding::Misconfigured { .. });
        if members.len() < 2 {
            for &i in &members {
                if contradicted(i) {
                    out[i] = 0;
                }
            }
            continue;
        }
        let kept = members
            .iter()
            .copied()
            .find(|&i| groups[i] > 0 && !contradicted(i))
            .map(|i| groups[i]);
        let target = match kept.or_else(|| fresh(members[0], &mut taken)) {
            Some(g) => g,
            None => continue,
        };
        for &i in &members {
            out[i] = target;
        }
    }
    out
}

/// What one server answered, as an [`AttemptLog`] keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seen {
    Have,
    /// `430`/`423`: proof the server (and its backend) lacks the article.
    Missing,
    /// `451`: removed, or a proxy whose backend timed out. Not found on
    /// this server for the attempt, but no 430 proof (as the per-server
    /// download took it: not found, not a backbone-wide miss).
    Removed,
    /// A reseller could not reach its backend; says nothing about the article.
    Unreachable(String),
    /// Any other failure, as text.
    Failed(String),
}

/// How a failed attempt failed, for the download's status.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FailKind {
    /// The body did not decode (CRC error, cut short, no yEnc data, ...).
    Decode(nntp_rs::decoder::Status),
    Timeout,
    /// The connection limit (a refused connection).
    Login,
    /// Login refused (credentials rejected): the server is passed over,
    /// never waited for (`authrejected`).
    Auth,
    Other,
}

/// One server attempt of a fetch through the shared pool.
#[derive(Clone, Debug)]
pub struct Rec {
    /// nntp-rs server index.
    pub server: usize,
    pub outcome: Seen,
    pub fail: Option<FailKind>,
    /// The server's status line, when it sent one.
    pub reply: Option<String>,
    pub wire_bytes: u64,
    /// The connection was opened for this attempt.
    pub fresh: bool,
    /// The attempt had its connection (nntp-rs called `started` for it):
    /// false for a connect, login or connection-limit failure, where the
    /// per-connection download never connected.
    pub connected: bool,
}

/// Collects a fetch's server attempts (nntp-rs calls the observer on its own
/// threads; the caller logs them on its own afterwards, in order) and books
/// each attempt's bytes on the server's statistics as they happen.
#[derive(Clone, Default)]
pub struct AttemptLog(std::sync::Arc<std::sync::Mutex<Vec<Rec>>>);

#[cfg(unix)]
impl AttemptLog {
    /// The observer for one fetch's `FetchOptions::observer`.
    pub fn observer(&self, shared: &Shared) -> nntp_rs::Observe {
        self.observer_with(shared, None, None, None)
    }

    /// [`Self::observer`], also calling `on_miss` after every attempt that
    /// brought no article and `on_start` when an attempt has its connection
    /// (both quickly: they run on nntp-rs's I/O threads), and keeping `wait`
    /// in step with whether the fetch has a connection.
    pub fn observer_with(
        &self,
        shared: &Shared,
        on_miss: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
        on_start: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
        wait: Option<WaitMark>,
    ) -> nntp_rs::Observe {
        nntp_rs::Observe::new(LogObserver {
            log: self.clone(),
            ids: shared.servers.iter().map(|s| s.id).collect(),
            on_miss,
            on_start,
            wait,
            in_hand: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// The attempts so far, oldest first; the log is emptied.
    pub fn take(&self) -> Vec<Rec> {
        self.0.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default()
    }
}

/// [`AttemptLog`]'s observer.
#[cfg(unix)]
struct LogObserver {
    log: AttemptLog,
    ids: Vec<i32>,
    on_miss: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    on_start: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    wait: Option<WaitMark>,
    /// Per server, the attempts that have their connection (`started`) and
    /// have not reported yet (more than one in a pipelined batch).
    in_hand: std::sync::Mutex<Vec<usize>>,
}

#[cfg(unix)]
impl nntp_rs::Observer for LogObserver {
    fn attempt(&self, a: &nntp_rs::Attempt<'_>) {
        use nntp_rs::Outcome;
        // until the next started(), the fetch waits for a connection again
        // (the next server's, or a retry's)
        if let Some(w) = &self.wait {
            w.set_waiting(true);
        }
        let (outcome, fail) = match &a.outcome {
            Outcome::Have { .. } => (Seen::Have, None),
            Outcome::Missing { removed: false } => (Seen::Missing, None),
            Outcome::Missing { removed: true } => (Seen::Removed, None),
            Outcome::Unreachable(t) => (Seen::Unreachable((*t).to_string()), None),
            Outcome::Failed(e) => {
                let kind = match e {
                    nntp_rs::Error::Decode(st) => FailKind::Decode(*st),
                    nntp_rs::Error::Timeout(_) => FailKind::Timeout,
                    nntp_rs::Error::Auth { .. } => FailKind::Auth,
                    nntp_rs::Error::ConnectionLimit { .. } => FailKind::Login,
                    _ => FailKind::Other,
                };
                (Seen::Failed(e.to_string()), Some(kind))
            }
        };
        if a.wire_bytes > 0 {
            if let (Some(sm), Some(&id)) = (crate::statmeter::active_statmeter(), self.ids.get(a.server)) {
                let off = crate::ffi::active_workstate().map_or(0, |w| w.local_time_offset());
                sm.add_server_stats(i32::try_from(a.wire_bytes).unwrap_or(i32::MAX), 0, 0, id, off);
            }
        }
        if !matches!(a.outcome, Outcome::Have { .. }) {
            if let Some(f) = &self.on_miss {
                f();
            }
        }
        let connected = self.in_hand.lock().is_ok_and(|mut h| match h.get_mut(a.server) {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        });
        if let Ok(mut g) = self.log.0.lock() {
            g.push(Rec {
                server: a.server,
                outcome,
                fail,
                reply: a.reply.map(str::to_string),
                wire_bytes: a.wire_bytes,
                fresh: a.fresh,
                connected,
            });
        }
    }

    fn started(&self, server: usize, _id: &str, _fresh: bool) {
        if let Ok(mut h) = self.in_hand.lock() {
            if h.len() <= server {
                h.resize(server + 1, 0);
            }
            h[server] += 1;
        }
        if let Some(w) = &self.wait {
            w.set_waiting(false);
        }
        if let Some(f) = &self.on_start {
            f();
        }
    }
}

/// Counts one fetch in a "waiting for a connection" counter (a stream
/// repair's watchdog stops its clock while any of its fetches wait): from
/// [`WaitMark::start`] to nntp-rs's first `started()`, and again from each
/// attempt to the next `started()`, until [`WaitMark::finish`]. Cheap to
/// clone; every clone is the same mark.
#[derive(Clone)]
pub struct WaitMark(std::sync::Arc<WaitMarkInner>);

struct WaitMarkInner {
    counter: std::sync::Arc<std::sync::atomic::AtomicI32>,
    /// WAITING (counted), RUNNING (has a connection) or FINISHED.
    state: std::sync::atomic::AtomicU8,
}

impl WaitMark {
    const WAITING: u8 = 0;
    const RUNNING: u8 = 1;
    const FINISHED: u8 = 2;

    /// Counts the fetch as waiting.
    pub fn start(counter: std::sync::Arc<std::sync::atomic::AtomicI32>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(std::sync::Arc::new(WaitMarkInner {
            counter,
            state: std::sync::atomic::AtomicU8::new(Self::WAITING),
        }))
    }

    /// Whether the fetch is counted as waiting now.
    pub fn is_waiting(&self) -> bool {
        self.0.state.load(Ordering::SeqCst) == Self::WAITING
    }

    /// Marks the fetch waiting (true) or holding a connection (false); a
    /// finished mark stays finished (nntp-rs may report late).
    pub fn set_waiting(&self, waiting: bool) {
        let (from, to) = if waiting {
            (Self::RUNNING, Self::WAITING)
        } else {
            (Self::WAITING, Self::RUNNING)
        };
        if self
            .0
            .state
            .compare_exchange(from, to, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if waiting {
                self.0.counter.fetch_add(1, Ordering::SeqCst);
            } else {
                self.0.counter.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    /// The fetch is over: it no longer counts, whatever nntp-rs reports later.
    pub fn finish(&self) {
        if self.0.state.swap(Self::FINISHED, Ordering::SeqCst) == Self::WAITING {
            self.0.counter.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Whether a fetch that ended in "not found" proves the article missing on
/// every server, as the per-server download required: every server that
/// could be asked (active, with connections, not blocked) answered 430
/// itself, or shares a backbone with one that did and keeps articles no
/// longer (`crate::nntp::backbone_miss_covers`); and no attempt failed any
/// other way. A server passed over for retention or because it was blocked
/// leaves the question open.
#[cfg(unix)]
pub fn missing_everywhere(shared: &Shared, recs: &[Rec]) -> bool {
    if recs.is_empty() || recs.iter().any(|r| r.outcome != Seen::Missing) {
        return false;
    }
    let pool = crate::net::active_server_pool();
    let misses: Vec<crate::nntp::BackboneMiss> = recs
        .iter()
        .filter_map(|r| shared.server(r.server))
        .map(|s| crate::nntp::BackboneMiss { group: s.group(), retention: s.retention })
        .collect();
    shared.servers.iter().enumerate().all(|(i, s)| {
        let usable = s.active.load(Ordering::SeqCst) && s.max_connections > 0;
        if !usable {
            return false; // never asked: it may have it
        }
        if pool.as_ref().is_some_and(|p| p.is_server_blocked(s)) {
            return false;
        }
        recs.iter().any(|r| r.server == i)
            || misses.iter().any(|m| crate::nntp::backbone_miss_covers(m, s.group(), s.retention))
    })
}

/// Whether every server that is switched on and has connections answered 430
/// or 451 for the article (itself, or through its backbone), and no attempt
/// failed any other way: not found for this fetch, as the per-server download
/// took a 451. nntp-rs ends such a walk with `Unavailable` when a server
/// is switched off (it might have the article); RustyGet never asked a
/// switched-off server, so for it the article is not found.
#[cfg(unix)]
pub fn missing_on_usable(shared: &Shared, recs: &[Rec]) -> bool {
    if recs.is_empty() || recs.iter().any(|r| !matches!(r.outcome, Seen::Missing | Seen::Removed)) {
        return false;
    }
    let pool = crate::net::active_server_pool();
    let misses: Vec<crate::nntp::BackboneMiss> = recs
        .iter()
        .filter_map(|r| shared.server(r.server))
        .map(|s| crate::nntp::BackboneMiss { group: s.group(), retention: s.retention })
        .collect();
    shared.servers.iter().enumerate().all(|(i, s)| {
        let usable = s.active.load(Ordering::SeqCst) && s.max_connections > 0;
        if !usable {
            return true;
        }
        if pool.as_ref().is_some_and(|p| p.is_server_blocked(s)) {
            return false;
        }
        recs.iter().any(|r| r.server == i)
            || misses.iter().any(|m| crate::nntp::backbone_miss_covers(m, s.group(), s.retention))
    })
}

/// Whether a fetch that failed with `unavailable` (nntp-rs found no server
/// it could ask) or with the attempts `recs` never got a connection, and a
/// required (not optional) server is among those it could not reach: the
/// news server is down or blocked, which says nothing about the article.
/// The per-connection download did not spend an article retry on a
/// connection it could not establish; it waited for the blocked server
/// (an optional one it passed over). Only a server switched on with
/// connections counts.
#[cfg(unix)]
pub fn only_unreachable_required(shared: &Shared, recs: &[Rec], unavailable: bool) -> bool {
    let required = |s: &RsNewsServer| s.active.load(Ordering::SeqCst) && s.max_connections > 0 && !s.optional;
    if recs.is_empty() {
        return unavailable && shared.servers.iter().any(|s| required(s));
    }
    // a refused login is the per-connection download's auth_rejected: that
    // server is passed over as failed, never waited for
    recs.iter().all(|r| !r.connected && matches!(r.outcome, Seen::Failed(_)) && r.fail != Some(FailKind::Auth))
        && recs.iter().any(|r| shared.server(r.server).is_some_and(|s| required(s)))
}

/// Servers switched on with connections: the ones a fetch can ask.
#[cfg(unix)]
pub fn usable_count(shared: &Shared) -> usize {
    shared
        .servers
        .iter()
        .filter(|s| s.active.load(Ordering::SeqCst) && s.max_connections > 0)
        .count()
}

/// The process-wide nntp-rs pool: every news server connection of the
/// daemon (downloads, repair and donor fetches, checks, probes) goes
/// through it. Built from the server pool's servers for one server
/// configuration generation; a server change builds a new one.
#[cfg(unix)]
pub struct Shared {
    pub pool: nntp_rs::BlockingPool,
    /// The server pool generation this pool serves (moves on when only a
    /// server's Active flag changed).
    generation: std::sync::atomic::AtomicI32,
    /// The RustyGet server behind each nntp-rs server index.
    servers: Vec<std::sync::Arc<RsNewsServer>>,
}

#[cfg(unix)]
impl Shared {
    /// The server pool generation this pool serves.
    pub fn generation(&self) -> i32 {
        self.generation.load(Ordering::SeqCst)
    }

    /// The nntp-rs index of the RustyGet server `id`.
    pub fn index_of(&self, id: i32) -> Option<usize> {
        self.servers.iter().position(|s| s.id == id)
    }

    /// How many servers the pool has.
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    /// The RustyGet server at nntp-rs index `idx`.
    pub fn server(&self, idx: usize) -> Option<&std::sync::Arc<RsNewsServer>> {
        self.servers.get(idx)
    }

    /// "Name (host)", as RustyGet's log lines name a server's connection.
    pub fn conn_name(&self, idx: usize) -> Vec<u8> {
        self.servers.get(idx).map_or_else(Vec::new, |s| {
            crate::artwritercore::article_dl_format_connection_name(
                s.name.to_bytes(),
                s.host.to_bytes(),
            )
        })
    }
}

#[cfg(unix)]
static SHARED: std::sync::RwLock<Option<std::sync::Arc<Shared>>> = std::sync::RwLock::new(None);

/// The current nntp-rs pool, if the daemon has news servers.
#[cfg(unix)]
pub fn shared() -> Option<std::sync::Arc<Shared>> {
    SHARED.read().ok().and_then(|g| g.clone())
}

/// Recently fetched bodies kept for callers that come back for the same
/// article (dupe, fleet and repair borrows), besides single-flight for
/// concurrent ones: decoded bytes.
pub const BODY_CACHE_BYTES: usize = 64 << 20;

/// A pool event, handed from nntp-rs's I/O tasks (which must not block) to
/// [`EventLog`]'s thread.
#[cfg(unix)]
enum PoolEvent {
    Blocked { server: usize, secs: u64, reason: String },
    CapLowered { server: usize, cap: usize },
}

/// Logs nntp-rs's pool events as RustyGet's server pool did, and blocks the
/// server in RustyGet's pool too (the Status page and the level rules read
/// that). The work runs on a thread of its own that ends with the pool.
#[cfg(unix)]
struct EventLog {
    tx: std::sync::Mutex<std::sync::mpsc::Sender<PoolEvent>>,
}

#[cfg(unix)]
impl EventLog {
    fn new(servers: Vec<std::sync::Arc<RsNewsServer>>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<PoolEvent>();
        let _ = std::thread::Builder::new().name("pool-events".into()).spawn(move || {
            for ev in rx {
                let label = |i: usize| {
                    servers.get(i).map(|s| {
                        (
                            std::sync::Arc::clone(s),
                            format!(
                                "{} ({})",
                                String::from_utf8_lossy(s.name.to_bytes()),
                                String::from_utf8_lossy(s.host.to_bytes())
                            ),
                        )
                    })
                };
                match ev {
                    PoolEvent::Blocked { server, secs, reason } => {
                        let Some((srv, name)) = label(server) else { continue };
                        if let Some(pool) = crate::net::active_server_pool() {
                            pool.block_server(&srv);
                        }
                        let msg = format!("{name}: not used for {secs} s: {reason}");
                        crate::log::log_active_bytes(crate::log::MK_DEBUG, msg.as_bytes());
                    }
                    PoolEvent::CapLowered { server, cap } => {
                        let Some((_, name)) = label(server) else { continue };
                        let msg = format!(
                            "{name} refused a connection at its limit; using at most {cap} connections for a while"
                        );
                        crate::log::log_active_bytes(crate::log::MK_WARNING, msg.as_bytes());
                    }
                }
            }
        });
        Self { tx: std::sync::Mutex::new(tx) }
    }

    fn send(&self, ev: PoolEvent) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(ev);
        }
    }
}

#[cfg(unix)]
impl nntp_rs::PoolEvents for EventLog {
    fn server_blocked(&self, server: usize, for_: Duration, reason: &nntp_rs::Error) {
        self.send(PoolEvent::Blocked { server, secs: for_.as_secs(), reason: reason.to_string() });
    }

    fn cap_lowered(&self, server: usize, cap: usize) {
        self.send(PoolEvent::CapLowered { server, cap });
    }
}

/// The nntp-rs pool configuration for `servers` and the current options.
#[cfg(unix)]
pub fn pool_config(
    servers: &[std::sync::Arc<RsNewsServer>],
    retry_interval: i32,
) -> nntp_rs::PoolConfig {
    let opts = crate::options::active_options();
    let (light_cap, article_timeout) = opts
        .as_deref()
        .map_or((crate::light_pool::LIGHT_MAX as i32, 60), |o| {
            let s = o.scalars();
            (s.light_connections, s.article_timeout)
        });
    let light_cap = usize::try_from(light_cap).unwrap_or(0);
    let io_timeout = Duration::from_secs(u64::try_from(article_timeout.max(1)).unwrap_or(60));
    let configs = servers
        .iter()
        .map(|s| {
            let mut c = server_config(s, light_cap);
            c.io_timeout = io_timeout;
            c
        })
        .collect();
    nntp_rs::PoolConfig {
        servers: configs,
        limits: nntp_rs::Limits {
            body_cache_bytes: BODY_CACHE_BYTES,
            // no deadline for a whole fetch (a slot wait plus a paced body can be
            // long under a speed limit): each read waits at most ArticleTimeout,
            // and the hanging-download check catches a stalled one, as before
            article_timeout: Duration::from_secs(24 * 3600),
            ..Default::default()
        },
        policy: nntp_rs::PoolPolicy {
            retry_interval: Duration::from_secs(u64::try_from(retry_interval.max(1)).unwrap_or(10)),
            ..Default::default()
        },
        events: Some(nntp_rs::Events::new(EventLog::new(servers.to_vec()))),
        ..Default::default()
    }
}

/// Builds the nntp-rs pool for the server pool's servers (start-up and every
/// server change) and starts backend identification on it.
#[cfg(unix)]
pub fn rebuild(pool: &crate::net::NzbgetRsServerPool) {
    let servers = pool.servers_snapshot();
    let generation = pool.generation();
    if let Some(cur) = shared() {
        let same = cur.servers.len() == servers.len()
            && cur.servers.iter().zip(&servers).all(|(a, b)| std::sync::Arc::ptr_eq(a, b));
        if same {
            // only Active flags can have changed: switch them in place
            for (i, s) in servers.iter().enumerate() {
                cur.pool
                    .set_server_active(i, s.active.load(Ordering::SeqCst) && s.max_connections > 0);
            }
            cur.generation.fetch_max(generation, Ordering::SeqCst);
            spawn_identification(generation);
            return;
        }
    }
    let next = if servers.is_empty() {
        None
    } else {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 8));
        match nntp_rs::BlockingPool::new(pool_config(&servers, pool.retry_interval()), threads) {
            Ok(p) => Some(std::sync::Arc::new(Shared {
                pool: p,
                generation: std::sync::atomic::AtomicI32::new(generation),
                servers,
            })),
            Err(e) => {
                crate::log::log_active_bytes(
                    crate::log::MK_ERROR,
                    format!("Could not set up the news server connections: {e}").as_bytes(),
                );
                None
            }
        }
    };
    let ready = next.is_some();
    let old = SHARED.write().ok().and_then(|mut g| std::mem::replace(&mut *g, next));
    if let Some(old) = old {
        // the old pool opens no new connection and closes its idle ones now, so
        // an account never has the old pool's sockets beside the new pool's;
        // requests still running on it finish on their own Arc
        old.pool.retire();
    }
    if ready {
        spawn_identification(generation);
    }
}

/// Identifies which servers read one article store (nntp-rs
/// `identify_backends`, backbone "auto" for servers without one) on a thread
/// of its own, logs the grouping and puts the servers of one store on one
/// RustyGet backbone too. nntp-rs then shares a 430 within each store; the
/// RustyGet backbone keeps the rest of the daemon (dupe health ranks, the
/// Status page) in step. `generation` is the pool's; a result for an older
/// one is dropped.
#[cfg(unix)]
pub fn spawn_identification(generation: i32) {
    // one run at a time: a request while one runs makes it run again for the
    // pool current then. Recording a request and the worker going idle happen
    // under one lock, so no request is lost between the worker's last check
    // and its exit.
    {
        let mut st = ID_STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if st.running {
            st.again = true;
            return;
        }
        st.running = true;
        st.again = false;
    }
    let spawned = std::thread::Builder::new().name("backend-id".into()).spawn(move || {
        // a panicking run must not leave identification marked running forever
        struct Idle;
        impl Drop for Idle {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    ID_STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).running = false;
                }
            }
        }
        let _idle = Idle;
        let mut generation = generation;
        loop {
            identify(generation);
            // the worker stays `running` until it finds no request under the lock
            loop {
                let mut st = ID_STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if !st.again {
                    st.running = false;
                    return;
                }
                st.again = false;
                drop(st);
                if let Some(s) = shared() {
                    generation = s.generation();
                    break;
                }
            }
        }
    });
    if spawned.is_err() {
        ID_STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).running = false;
    }
}

/// Backend identification's worker: one at a time (`running`), and whether
/// a request came while it ran (`again`).
#[cfg(unix)]
struct IdState {
    running: bool,
    again: bool,
}

#[cfg(unix)]
static ID_STATE: std::sync::Mutex<IdState> = std::sync::Mutex::new(IdState { running: false, again: false });

#[cfg(unix)]
fn identify(generation: i32) {
    use crate::log::{log_active_bytes, MK_INFO};
    let Some(s) = shared() else {
        return;
    };
    if s.generation() != generation
        || s.servers
            .iter()
            .filter(|srv| srv.active.load(Ordering::SeqCst))
            .count()
            < 2
    {
        return;
    }
    let verdicts = s.pool.identify_backends();
    let prints = s.pool.pool().fingerprints();
    if shared().is_none_or(|now| now.generation() != generation) {
        return;
    }
    let probes = probe_states(&prints, s.servers.len());
    let names: Vec<String> = s
        .servers
        .iter()
        .map(|srv| String::from_utf8_lossy(srv.name.to_bytes()).into_owned())
        .collect();
    for line in backend_log_lines(&names, &probes, &verdicts) {
        log_active_bytes(MK_INFO, line.as_bytes());
    }
    let configured: Vec<i32> = s.servers.iter().map(|srv| srv.configured_group).collect();
    let next = identified_groups(&names, &configured, &verdicts, crate::backbones::intern);
    let mut changed = false;
    for (i, srv) in s.servers.iter().enumerate() {
        if shared().is_none_or(|now| now.generation() != generation) {
            break;
        }
        if srv.group() != next[i] {
            srv.set_group(next[i]);
            changed = true;
            let backbone = crate::backbones::name_of(next[i]).unwrap_or_else(|| "none".into());
            let msg = format!(
                "{}: backbone {backbone} (backend identification); a 430 there is not asked again on its other servers",
                names[i]
            );
            log_active_bytes(MK_INFO, msg.as_bytes());
        }
    }
    if changed {
        // what a backbone's 430 ruled out may differ now (after the new groups
        // are set, so no miss recorded under the old ones survives)
        crate::articlefetcher::forget_missing_everywhere();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_mark_counts_only_while_waiting() {
        let c = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
        let w = WaitMark::start(std::sync::Arc::clone(&c));
        assert_eq!(c.load(Ordering::SeqCst), 1);
        assert!(w.is_waiting());
        w.set_waiting(false);
        w.set_waiting(false);
        assert_eq!(c.load(Ordering::SeqCst), 0);
        w.clone().set_waiting(true);
        w.set_waiting(true);
        assert_eq!(c.load(Ordering::SeqCst), 1);
        w.finish();
        assert_eq!(c.load(Ordering::SeqCst), 0);
        // late reports after the fetch ended change nothing
        w.set_waiting(true);
        w.set_waiting(false);
        w.finish();
        assert_eq!(c.load(Ordering::SeqCst), 0);
        // finishing while it holds a connection leaves the count alone
        let w2 = WaitMark::start(std::sync::Arc::clone(&c));
        w2.set_waiting(false);
        w2.finish();
        assert_eq!(c.load(Ordering::SeqCst), 0);
    }

    #[cfg(unix)]
    #[test]
    fn observer_waits_until_started_and_after_each_attempt() {
        use nntp_rs::Observer;
        let c = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
        let w = WaitMark::start(std::sync::Arc::clone(&c));
        let log = AttemptLog::default();
        let obs = LogObserver { log: log.clone(), ids: vec![3, 4], on_miss: None, on_start: None, wait: Some(w.clone()), in_hand: std::sync::Mutex::new(Vec::new()) };
        obs.started(0, "<a@b>", true);
        assert_eq!(c.load(Ordering::SeqCst), 0, "a connection in hand is the server's time");
        obs.attempt(&nntp_rs::Attempt {
            id: "<a@b>",
            server: 0,
            outcome: nntp_rs::Outcome::Missing { removed: false },
            wire_bytes: 0,
            elapsed: Duration::ZERO,
            reply: Some("430 no such article"),
            fresh: true,
        });
        assert_eq!(c.load(Ordering::SeqCst), 1, "waiting for the next server's connection");
        obs.started(1, "<a@b>", false);
        assert_eq!(c.load(Ordering::SeqCst), 0);
        w.finish();
        assert_eq!(c.load(Ordering::SeqCst), 0);
        assert_eq!(log.take().len(), 1);
    }

    fn server(group: i32, user: &[u8]) -> RsNewsServer {
        RsNewsServer::new(
            3,
            true,
            b"main",
            b"news.example.com",
            563,
            0,
            user,
            b"secret",
            false,
            true,
            b"",
            40,
            3000,
            1,
            group,
            true,
            2,
        )
    }

    #[test]
    fn maps_server_options() {
        let c = server(0, b"user");
        let cfg = server_config(&c, 4);
        assert_eq!(cfg.name, "main");
        assert_eq!(cfg.host, "news.example.com");
        assert_eq!(cfg.port, 563);
        assert_eq!(cfg.tls, nntp_rs::TlsMode::OpenSsl);
        assert_eq!(cfg.max_connections, 40);
        assert_eq!(cfg.level, 1);
        assert_eq!(cfg.light_connections, 4);
        assert!(cfg.optional && cfg.active);
        assert_eq!(cfg.retention_days, 3000);
        assert_eq!(cfg.username.as_deref(), Some("user"));
        assert_eq!(cfg.password.as_deref(), Some("secret"));
        // no Backbone: detected
        assert_eq!(cfg.backbone.as_deref(), Some("auto"));
        assert_eq!(cfg.group, 0);
    }

    #[test]
    fn keeps_a_configured_backbone_and_no_login_without_user() {
        let id = crate::backbones::intern("nntprs-pool-test-backbone");
        let cfg = server_config(&server(id, b""), 0);
        assert_eq!(cfg.backbone.as_deref(), Some("nntprs-pool-test-backbone"));
        assert_eq!(cfg.group, id as u32);
        assert!(cfg.username.is_none() && cfg.password.is_none());
    }

    #[test]
    fn backend_lines_name_peers() {
        use nntp_rs::backend::Finding;
        let names = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let v = vec![
            nntp_rs::Verdict {
                identity: 0,
                finding: Finding::Detected,
                peers: vec!["b".into()],
            },
            nntp_rs::Verdict {
                identity: 0,
                finding: Finding::Detected,
                peers: vec!["a".into()],
            },
            nntp_rs::Verdict {
                identity: 1,
                finding: Finding::Alone,
                peers: vec![],
            },
        ];
        use Probe::*;
        assert_eq!(
            backend_log_lines(&names, &[Compared, Compared, Compared], &v),
            vec![
                "Backend of a: store 0 shared with b (detected)",
                "Backend of b: store 0 shared with a (detected)",
                "Backend of c: store 1 on its own (no other server shares its store)",
            ]
        );
        // a server whose numbers were never read or compared is not "on its own"
        assert!(backend_log_lines(&names, &[Compared, Compared, NotProbed], &v)[2]
            .starts_with("Backend of c: not probed"));
        assert!(backend_log_lines(&names, &[Compared, Compared, NothingToCompare], &v)[2]
            .starts_with("Backend of c: nothing to compare"));
    }

    fn verdict(identity: usize, finding: nntp_rs::backend::Finding) -> nntp_rs::Verdict {
        nntp_rs::Verdict { identity, finding, peers: vec![] }
    }

    #[test]
    fn detected_store_gets_one_backbone() {
        use nntp_rs::backend::Finding::*;
        let names: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let v = vec![verdict(0, Detected), verdict(0, Detected), verdict(1, Alone)];
        let mut made = Vec::new();
        let got = identified_groups(&names, &[0, 0, 0], &v, |n| {
            made.push(n.to_string());
            77
        });
        assert_eq!(got, vec![77, 77, 0]);
        assert_eq!(made, vec!["Store of a"]);
    }

    #[test]
    fn unconfigured_server_joins_a_configured_backbone() {
        use nntp_rs::backend::Finding::*;
        let names: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let v = vec![verdict(0, Detected), verdict(0, Unchecked)];
        let got = identified_groups(&names, &[0, 5], &v, |_| panic!("no new backbone"));
        assert_eq!(got, vec![5, 5]);
    }

    #[test]
    fn contradicted_backbone_is_dropped() {
        use nntp_rs::backend::{Contradiction, Finding::*};
        let names: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let mis = Misconfigured { against: "a".into(), why: Contradiction::Numbers };
        let v = vec![verdict(0, Unchecked), verdict(1, mis)];
        let got = identified_groups(&names, &[5, 5], &v, |_| panic!("no new backbone"));
        assert_eq!(got, vec![5, 0]);
    }

    #[test]
    fn stable_on_a_second_run() {
        // each run starts from the configured backbones (none here) and names
        // the store the same way, so it lands on the same backbone
        use nntp_rs::backend::Finding::*;
        let names: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let v = vec![verdict(0, Detected), verdict(0, Detected)];
        let mut ids = std::collections::HashMap::new();
        let mut intern = |n: &str| {
            let next = ids.len() as i32 + 1;
            *ids.entry(n.to_string()).or_insert(next)
        };
        let first = identified_groups(&names, &[0, 0], &v, &mut intern);
        let second = identified_groups(&names, &[0, 0], &v, &mut intern);
        assert_eq!(first, second);
        assert!(first[0] > 0 && first[0] == first[1]);
    }

    #[test]
    fn same_named_stores_never_share_a_backbone() {
        // two stores whose first servers have one name, and a configured
        // backbone with the name a store would get
        use nntp_rs::backend::Finding::*;
        let names: Vec<String> = ["main", "x", "main", "y", "z"].iter().map(|s| s.to_string()).collect();
        let v = vec![
            verdict(0, Detected),
            verdict(0, Detected),
            verdict(1, Detected),
            verdict(1, Detected),
            verdict(2, Alone),
        ];
        let mut ids: Vec<String> = vec!["store of main".into()]; // id 1, configured on z
        let mut intern = |n: &str| {
            let key = n.to_lowercase();
            match ids.iter().position(|k| *k == key) {
                Some(p) => p as i32 + 1,
                None => {
                    ids.push(key);
                    ids.len() as i32
                }
            }
        };
        let got = identified_groups(&names, &[0, 0, 0, 0, 1], &v, &mut intern);
        assert_eq!(got[0], got[1]);
        assert_eq!(got[2], got[3]);
        assert_ne!(got[0], got[2], "two stores, two backbones");
        assert!(got[0] != 1 && got[2] != 1, "not the configured backbone of z");
        assert_eq!(got[4], 1);
    }

    #[test]
    fn probe_states_need_a_group_in_common() {
        let fp = |groups: &[&str]| {
            let mut f = nntp_rs::Fingerprint::default();
            for g in groups {
                f.watermarks.insert(g.to_string(), nntp_rs::Watermarks { low: 1, high: 9 });
            }
            Some(f)
        };
        let prints = vec![fp(&["a.b"]), fp(&["a.b", "c.d"]), fp(&[]), None, fp(&["e.f"])];
        use Probe::*;
        assert_eq!(
            probe_states(&prints, 5),
            vec![Compared, Compared, NothingToCompare, NotProbed, NothingToCompare]
        );
    }

    /// A Shared over `servers` (no connection is made until a request).
    fn shared_of(servers: Vec<std::sync::Arc<RsNewsServer>>) -> Shared {
        let pool = nntp_rs::BlockingPool::new(pool_config(&servers, 10), 1).expect("pool");
        Shared {
            pool,
            generation: std::sync::atomic::AtomicI32::new(1),
            servers,
        }
    }

    fn srv(id: i32, active: bool, conns: i32, group: i32) -> std::sync::Arc<RsNewsServer> {
        std::sync::Arc::new(RsNewsServer::new(
            id, active, b"s", b"127.0.0.1", 119, 0, b"", b"", false, false, b"", conns, 0, 0, group, false, 2,
        ))
    }

    fn rec(server: usize, outcome: Seen) -> Rec {
        Rec { server, outcome, fail: None, reply: None, wire_bytes: 0, fresh: false, connected: true }
    }

    #[test]
    fn missing_rules_follow_the_servers_that_could_be_asked() {
        // 0 and 1 on one backbone, 2 on its own, 3 switched off
        let sh = shared_of(vec![srv(1, true, 2, 7), srv(2, true, 2, 7), srv(3, true, 2, 0), srv(4, false, 2, 0)]);
        let both = [rec(0, Seen::Missing), rec(2, Seen::Missing)];
        // server 1 is covered by its backbone's 430, server 3 is off: missing on
        // every usable server, but not everywhere (3 was never asked)
        assert!(missing_on_usable(&sh, &both));
        assert!(!missing_everywhere(&sh, &both));
        // a 451 is not found for this fetch, but no 430 proof
        let removed = [rec(0, Seen::Missing), rec(2, Seen::Removed)];
        assert!(missing_on_usable(&sh, &removed));
        let sh_on = shared_of(vec![srv(1, true, 2, 7), srv(3, true, 2, 0)]);
        assert!(missing_everywhere(&sh_on, &[rec(0, Seen::Missing), rec(1, Seen::Missing)]));
        assert!(!missing_everywhere(&sh_on, &[rec(0, Seen::Missing), rec(1, Seen::Removed)]));
        // server 2 not asked: open
        assert!(!missing_on_usable(&sh, &[rec(0, Seen::Missing)]));
        // any other kind of answer leaves it open
        assert!(!missing_on_usable(&sh, &[rec(0, Seen::Missing), rec(2, Seen::Failed("x".into()))]));
        assert!(!missing_on_usable(&sh, &[]));
        assert_eq!(usable_count(&sh), 3);
        // with every server on, both 430s cover all of them
        let on = shared_of(vec![srv(1, true, 2, 7), srv(2, true, 2, 7), srv(3, true, 2, 0)]);
        assert!(missing_everywhere(&on, &both));
        // a server with no connections is never usable
        let zero = shared_of(vec![srv(1, true, 2, 0), srv(2, true, 0, 0)]);
        assert!(missing_on_usable(&zero, &[rec(0, Seen::Missing)]));
        assert!(!missing_everywhere(&zero, &[rec(0, Seen::Missing)]));
    }

    #[test]
    fn a_server_without_connections_or_switched_off_is_inactive_in_nntp_rs() {
        assert!(!server_config(&srv(1, true, 0, 0), 0).active);
        assert!(!server_config(&srv(1, false, 4, 0), 0).active);
        assert!(server_config(&srv(1, true, 4, 0), 0).active);
    }
}
