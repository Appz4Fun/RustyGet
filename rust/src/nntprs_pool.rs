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
        .with_active(srv.active.load(Ordering::SeqCst))
        .with_group(u32::try_from(srv.configured_group).unwrap_or(0))
        .with_retention_days(u32::try_from(srv.retention).unwrap_or(0));
    let user = srv.user.to_bytes();
    if !user.is_empty() {
        cfg = cfg.with_auth(
            String::from_utf8_lossy(user).into_owned(),
            text(&srv.password),
        );
    }
    cfg.optional = srv.optional;
    // what the user typed, never a backbone identification made earlier
    cfg.backbone = Some(
        crate::backbones::name_of(srv.configured_group).unwrap_or_else(|| "auto".to_string()),
    );
    cfg.io_timeout = Duration::from_secs(60);
    cfg
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

/// Identifies which servers read one article store (nntp-rs
/// `identify_backends`, backbone "auto" for servers without one) on a thread
/// of its own, logs the grouping and puts the servers of one store on one
/// backbone, so a 430 from one is not asked again on its siblings.
///
/// Each probed server lends one idle connection of its own: held and
/// disconnected while nntp-rs uses its slot, and freed only after the probe
/// pool is gone, so a server never has more sockets open than its
/// `ServerN.Connections`. A server with none idle is not probed (still
/// grouped by host and port or by its configured backbone). Only a weak
/// reference to the server pool is kept while the probe runs, so a reload
/// or shutdown is not held up; a result for a pool that changed meanwhile is
/// dropped.
#[cfg(unix)]
pub fn spawn_identification(pool: &std::sync::Arc<crate::net::NzbgetRsServerPool>) {
    // one run at a time: a request while one runs makes it run again for the
    // pool active then, instead of a second probe borrowing more connections
    if IDENTIFYING.swap(true, Ordering::SeqCst) {
        AGAIN.store(true, Ordering::SeqCst);
        return;
    }
    let first = (std::sync::Arc::downgrade(pool), pool.generation());
    let spawned = std::thread::Builder::new().name("backend-id".into()).spawn(move || {
        let mut job = Some(first);
        while let Some((weak, generation)) = job.take() {
            identify(weak, generation);
            if AGAIN.swap(false, Ordering::SeqCst) {
                job = crate::net::active_server_pool().map(|p| (std::sync::Arc::downgrade(&p), p.generation()));
            }
        }
        IDENTIFYING.store(false, Ordering::SeqCst);
        // a request between the last check and the store above
        if AGAIN.swap(false, Ordering::SeqCst) {
            if let Some(p) = crate::net::active_server_pool() {
                spawn_identification(&p);
            }
        }
    });
    if spawned.is_err() {
        IDENTIFYING.store(false, Ordering::SeqCst);
    }
}

#[cfg(unix)]
static IDENTIFYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
static AGAIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Held connections, as addresses (only compared by `free_connection`),
/// shared by the probe thread and its starter so whichever ends up with them
/// gives them back exactly once.
#[cfg(unix)]
type Held = std::sync::Arc<std::sync::Mutex<Vec<usize>>>;

#[cfg(unix)]
fn give_back(weak: &std::sync::Weak<crate::net::NzbgetRsServerPool>, held: &Held) {
    let conns = held.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    if let Some(pool) = weak.upgrade() {
        for conn in conns {
            pool.free_connection(conn as *mut crate::net::PooledNntpConn, false);
        }
    }
}

#[cfg(unix)]
fn identify(weak: std::sync::Weak<crate::net::NzbgetRsServerPool>, generation: i32) {
    use crate::log::{log_active_bytes, MK_INFO, MK_WARNING};
    let Some(pool) = weak.upgrade() else {
        return;
    };
    let servers = pool.servers_snapshot();
    if servers
        .iter()
        .filter(|s| s.active.load(Ordering::SeqCst))
        .count()
        < 2
    {
        return;
    }
    let held: Held = Default::default();
    let mut configs = Vec::with_capacity(servers.len());
    for srv in &servers {
        let mut cfg = server_config(srv, 0).with_max_connections(1);
        cfg.connect_timeout = Duration::from_secs(15);
        cfg.io_timeout = Duration::from_secs(20);
        cfg.adaptive_cap = false;
        if cfg.active {
            let conn = pool.get_check_connection(srv);
            // SAFETY: a non-null connection from the pool stays valid until freed
            match unsafe { conn.as_ref() } {
                Some(c) => {
                    if c.is_connected() {
                        // SAFETY: held by this thread until freed
                        unsafe { c.disconnect() };
                    }
                    if let Ok(mut g) = held.lock() {
                        g.push(conn as usize);
                    }
                }
                None => cfg.active = false,
            }
        }
        configs.push(cfg);
    }
    drop(pool);
    let names: Vec<String> = configs.iter().map(|c| c.name.clone()).collect();
    let (tx, rx) = std::sync::mpsc::channel();
    let config = nntp_rs::PoolConfig {
        servers: configs,
        ..Default::default()
    };
    let probe_weak = weak.clone();
    let probe_held = std::sync::Arc::clone(&held);
    let spawned = std::thread::Builder::new()
        .name("backend-probe".into())
        .spawn(move || {
            let got = nntp_rs::BlockingPool::new(config, 1).map(|p| {
                let verdicts = p.identify_backends();
                let prints = p.pool().fingerprints();
                drop(p); // its sockets are closed before the slots go back
                (verdicts, prints)
            });
            give_back(&probe_weak, &probe_held);
            let _ = tx.send(got);
        });
    if spawned.is_err() {
        // the probe never ran: give the held connections back here
        give_back(&weak, &held);
        return;
    }
    let (verdicts, prints) = match rx.recv_timeout(Duration::from_secs(600)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            log_active_bytes(
                MK_WARNING,
                format!("Backend identification failed: {e}").as_bytes(),
            );
            return;
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            log_active_bytes(
                MK_WARNING,
                b"Backend identification took more than 600 s; its result will not be used",
            );
            return;
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            log_active_bytes(MK_WARNING, b"Backend identification stopped without a result");
            return;
        }
    };
    let Some(pool) = weak.upgrade() else {
        return;
    };
    if pool.generation() != generation {
        return;
    }
    let probes = probe_states(&prints, servers.len());
    for line in backend_log_lines(&names, &probes, &verdicts) {
        log_active_bytes(MK_INFO, line.as_bytes());
    }
    let configured: Vec<i32> = servers.iter().map(|s| s.configured_group).collect();
    let next = identified_groups(&names, &configured, &verdicts, crate::backbones::intern);
    let mut changed = false;
    for (i, srv) in servers.iter().enumerate() {
        if pool.generation() != generation {
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
}
