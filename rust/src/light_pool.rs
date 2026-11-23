//! The light-work reserve: a few connections of each news server kept for
//! checks that wait on latency, not bandwidth (STAT probes, fleet and
//! TwinCheck samples, archive header and footer reads, par2 index reads,
//! donor checks), so they never queue behind the download threads.
//!
//! The reserve lives inside each server's own connection limit
//! (`ServerN.Connections`): downloads may hold at most `Connections - size`
//! connections of the server, light jobs at most `size`. Light connections
//! are ordinary pooled connections taken in light mode; nothing extra is
//! opened.
//!
//! # Sizing rule
//!
//! Recomputed for every server every [`RECOMPUTE_MS`] from live pool stats
//! ([`next_size`]):
//!
//! * `LightConnections=0` turns the reserve off; a server with at most
//!   [`TOO_FEW_CONNECTIONS`] connections gets none (a third or half of its
//!   connections would be a lasting loss for downloads).
//! * The ceiling is one light connection per [`CONNECTIONS_PER_LIGHT`]
//!   server connections, at most [`LIGHT_MAX`] and at most
//!   `LightConnections`; never more than `Connections - 1`.
//! * At most 1 for a flaky server (connect, request and timeout failures
//!   above [`FLAKY_ERROR_PERCENT`] in a window, or a blocked server; held
//!   for [`FLAKY_HOLD_MS`]) and for a server with at most
//!   [`FEW_CONNECTIONS`] connections.
//! * Growing only takes connections the download left idle the whole window
//!   (the fewest idle download connections seen), so it never takes one a
//!   download was using.
//! * When the download wants every connection it may hold (none idle all
//!   window and a download was refused a connection) and more connections
//!   could still raise its speed (no speed limit it already reaches), the
//!   reserve shrinks by one per window: down to 1 on a server with more than
//!   [`FEW_CONNECTIONS`] connections, down to 0 on a smaller one (it comes
//!   back once the download leaves a connection idle).

/// The most light connections per news server.
pub const LIGHT_MAX: i32 = 5;
/// A server with at most this many connections keeps at most one light
/// connection, and gives it back while the download needs every connection.
pub const FEW_CONNECTIONS: i32 = 8;
/// A server with at most this many connections keeps none.
pub const TOO_FEW_CONNECTIONS: i32 = 3;
/// One light connection per this many server connections, up to [`LIGHT_MAX`].
pub const CONNECTIONS_PER_LIGHT: i32 = 8;
/// How often the sizes are recomputed (milliseconds).
pub const RECOMPUTE_MS: i64 = 5_000;
/// A window with more failed requests than this share (percent) makes a server flaky...
pub const FLAKY_ERROR_PERCENT: u64 = 5;
/// ... once at least this many requests failed in it.
pub const FLAKY_MIN_ERRORS: u64 = 3;
/// How long a server counts as flaky after its last flaky window (milliseconds).
pub const FLAKY_HOLD_MS: i64 = 60_000;
/// Read and write timeout of a light connection (seconds), or `ArticleTimeout`
/// when that is shorter; connecting keeps `ArticleTimeout`. A light job that
/// times out counts as unverified, never as missing, and a light connection
/// that cannot connect proves nothing about the server.
pub const LIGHT_IO_TIMEOUT_SECS: i32 = 15;
/// A download at this share (percent) of its speed limit cannot go faster
/// with more connections.
pub const SPEED_LIMIT_BOUND_PERCENT: i64 = 90;

/// Why a server's reserve has the size it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// `LightConnections=0`.
    Disabled,
    /// At most [`TOO_FEW_CONNECTIONS`] connections: the download keeps them.
    TooFewConnections,
    /// At most [`FEW_CONNECTIONS`] connections.
    FewConnections,
    /// Recent errors or timeouts, or blocked.
    Flaky,
    /// The download left connections idle all window.
    Spare,
    /// The download wants every connection it may hold.
    DownloadSaturated,
    /// Down to the ceiling (configuration or connection count changed).
    Ceiling,
    /// No change.
    Steady,
}

impl Reason {
    pub fn text(self) -> &'static str {
        match self {
            Reason::Disabled => "turned off (LightConnections=0)",
            Reason::TooFewConnections => "too few connections, all kept for downloads",
            Reason::FewConnections => "few connections",
            Reason::Flaky => "recent errors or timeouts",
            Reason::Spare => "download connections left idle",
            Reason::DownloadSaturated => "the download needs every connection",
            Reason::Ceiling => "limit",
            Reason::Steady => "unchanged",
        }
    }
}

/// Live stats of one server for [`next_size`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SizingInput {
    /// `ServerN.Connections`.
    pub max_connections: i32,
    /// `LightConnections` (0 = off).
    pub cap: i32,
    /// Recent errors or timeouts above the threshold, or blocked.
    pub flaky: bool,
    /// The reserve now.
    pub current: i32,
    /// Fewest download connections left idle at any time in the window.
    pub spare: i32,
    /// A download was refused a connection in the window.
    pub download_starved: bool,
    /// The download already runs at its speed limit: more connections would
    /// not make it faster.
    pub bandwidth_bound: bool,
}

/// The most light connections a server may have, and why it is not more.
pub fn ceiling(max_connections: i32, cap: i32, flaky: bool) -> (i32, Reason) {
    if cap <= 0 {
        return (0, Reason::Disabled);
    }
    if max_connections <= TOO_FEW_CONNECTIONS {
        return (0, Reason::TooFewConnections);
    }
    if flaky {
        return (1, Reason::Flaky);
    }
    if max_connections <= FEW_CONNECTIONS {
        return (1, Reason::FewConnections);
    }
    let top = (max_connections / CONNECTIONS_PER_LIGHT)
        .clamp(1, LIGHT_MAX)
        .min(cap)
        .min(max_connections - 1);
    (top, Reason::Ceiling)
}

/// The next reserve of a server and the reason for a change (see the module
/// documentation for the rule).
pub fn next_size(inp: &SizingInput) -> (i32, Reason) {
    let (top, top_reason) = ceiling(inp.max_connections, inp.cap, inp.flaky);
    if top == 0 {
        return (
            0,
            if inp.current == 0 {
                Reason::Steady
            } else {
                top_reason
            },
        );
    }
    let spare = inp.spare.max(0);
    let saturated = spare == 0 && inp.download_starved && !inp.bandwidth_bound;
    let wanted = if saturated {
        inp.current - 1
    } else {
        inp.current.saturating_add(spare)
    };
    // a small server hands its one light connection to a download that needs it
    let floor = if inp.max_connections <= FEW_CONNECTIONS {
        0
    } else {
        1
    };
    let size = wanted.clamp(floor, top);
    let reason = if size == inp.current {
        Reason::Steady
    } else if size > inp.current {
        if inp.current <= 0 || spare > 0 {
            Reason::Spare
        } else {
            Reason::Ceiling
        }
    } else if size == top && wanted > top {
        top_reason
    } else {
        Reason::DownloadSaturated
    };
    (size, reason)
}

/// Whether a window's request outcomes make a server flaky.
pub fn window_flaky(ok: u64, errors: u64) -> bool {
    errors >= FLAKY_MIN_ERRORS
        && errors.saturating_mul(100)
            > ok.saturating_add(errors)
                .saturating_mul(FLAKY_ERROR_PERCENT)
}

/// Whether a download at `cur_speed` runs at its speed limit (both bytes/s).
pub fn bandwidth_bound(speed_limit: i64, cur_speed: i64) -> bool {
    speed_limit > 0
        && cur_speed.saturating_mul(100) >= speed_limit.saturating_mul(SPEED_LIMIT_BOUND_PERCENT)
}

/// One server's reserve and the stats of the window being measured.
#[derive(Clone, Debug, Default)]
pub struct ServerLight {
    pub server_id: i32,
    pub size: i32,
    /// Fewest idle download connections seen in this window.
    pub min_spare: i32,
    pub ok_mark: usize,
    pub err_mark: usize,
    pub flaky_until_ms: i64,
}

/// The reserves of all servers, kept under the server pool's lock.
#[derive(Clone, Debug, Default)]
pub struct LightState {
    pub servers: Vec<ServerLight>,
    pub last_ms: i64,
}

impl LightState {
    pub fn get(&self, server_id: i32) -> Option<&ServerLight> {
        self.servers.iter().find(|s| s.server_id == server_id)
    }

    pub fn get_mut(&mut self, server_id: i32) -> Option<&mut ServerLight> {
        self.servers.iter_mut().find(|s| s.server_id == server_id)
    }

    pub fn size_of(&self, server_id: i32) -> i32 {
        self.get(server_id).map_or(0, |s| s.size)
    }

    /// Notes that a server now has `spare` idle download connections.
    pub fn note_spare(&mut self, server_id: i32, spare: i32) {
        if let Some(s) = self.get_mut(server_id) {
            s.min_spare = s.min_spare.min(spare.max(0));
        }
    }
}

/// The log line for a size change of the server `name`.
pub fn change_message(name: &[u8], from: i32, to: i32, reason: Reason, detail: &str) -> Vec<u8> {
    let mut msg = b"Light connections of ".to_vec();
    msg.extend_from_slice(name);
    msg.extend_from_slice(format!(": {from} -> {to} ({}", reason.text()).as_bytes());
    if !detail.is_empty() {
        msg.extend_from_slice(b", ");
        msg.extend_from_slice(detail.as_bytes());
    }
    msg.push(b')');
    msg
}

/// A connection of `srv` for a STAT or other light check: a light one when
/// the server has a free one, else a check connection of the main pool
/// (downloads keep first claim there). Freed with
/// `NzbgetRsServerPool::free_connection` as usual.
#[cfg(unix)]
pub fn check_connection(
    pool: &crate::net::NzbgetRsServerPool,
    srv: &crate::net::RsNewsServer,
) -> *mut crate::net::PooledNntpConn {
    let light = pool.get_light_connection(srv);
    if !light.is_null() {
        return light;
    }
    pool.get_check_connection(srv)
}

/// [`check_connection`] by server id.
#[cfg(unix)]
pub fn check_connection_by_server_id(
    pool: &crate::net::NzbgetRsServerPool,
    server_id: i32,
) -> *mut crate::net::PooledNntpConn {
    match pool.server_by_id(server_id) {
        Some(srv) => check_connection(pool, &srv),
        None => std::ptr::null_mut(),
    }
}

/// The light variant of an article fetcher's host: the daemon's host takes
/// light connections first; any other host (a test's) is kept as it is.
#[cfg(unix)]
pub fn light(
    host: &'static dyn crate::articlefetcher::NntpHost,
) -> &'static dyn crate::articlefetcher::NntpHost {
    host.light_host().unwrap_or(host)
}

/// Whether `conn` is checked out as a light connection. What such a
/// connection could not do (connect, answer in time) is unknown, never proof
/// against the server: it must not block the server or count it unreachable.
#[cfg(unix)]
pub fn is_light(conn: &crate::net::PooledNntpConn) -> bool {
    conn.light.load(std::sync::atomic::Ordering::SeqCst)
}

/// Bulk fetch workers that fit the connections downloads and bulk fetches may
/// use: `workers`, but at most `primary - light` (the primary servers'
/// connections less their light reserve), at least 1. A worker left waiting
/// for a connection would stop a stream repair's clock.
pub fn fit_workers(workers: i32, primary: i32, light: i32) -> i32 {
    if light <= 0 {
        return workers;
    }
    workers.min(primary - light).max(1)
}

/// How many light jobs to run at once out of at most `max`: no more than the
/// light connections there are (`light`), so the rest queue instead of
/// polling for one; `max` when there are none (the main pool serves them).
pub fn parallel_jobs(max: usize, light: i32) -> usize {
    if light <= 0 {
        max.max(1)
    } else {
        max.min(light as usize).max(1)
    }
}

/// [`parallel_jobs`] for the light connections of the primary servers now.
#[cfg(unix)]
pub fn parallel_jobs_now(max: usize) -> usize {
    let light = crate::net::active_server_pool().map_or(0, |p| p.primary_light_connections());
    parallel_jobs(max, light)
}

/// A checked-out connection that goes back to the pool when dropped (an early
/// return or an unwinding panic), disconnected since its state is unknown.
/// [`Checkout::release`] returns it the normal way.
#[cfg(unix)]
pub struct Checkout<'a> {
    pool: &'a crate::net::NzbgetRsServerPool,
    conn: *mut crate::net::PooledNntpConn,
}

#[cfg(unix)]
impl<'a> Checkout<'a> {
    pub fn new(
        pool: &'a crate::net::NzbgetRsServerPool,
        conn: *mut crate::net::PooledNntpConn,
    ) -> Self {
        Self { pool, conn }
    }

    /// Returns the connection to the pool as it is.
    pub fn release(mut self, used: bool) {
        let conn = std::mem::replace(&mut self.conn, std::ptr::null_mut());
        self.pool.free_connection(conn, used);
    }
}

#[cfg(unix)]
impl Drop for Checkout<'_> {
    fn drop(&mut self) {
        if self.conn.is_null() {
            return;
        }
        // SAFETY: the connection is still checked out to this guard
        unsafe { (*self.conn).disconnect() };
        self.pool.free_connection(self.conn, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workers_fit_the_main_pool() {
        // 4 connections, 1 light: 3 workers, not 4
        assert_eq!(fit_workers(4, 4, 1), 3);
        // a big pool keeps its workers
        assert_eq!(fit_workers(30, 60, 5), 30);
        assert_eq!(fit_workers(4, 60, 5), 4);
        assert_eq!(fit_workers(2, 4, 1), 2);
        // no reserve: unchanged, even past the pool (as before)
        assert_eq!(fit_workers(4, 2, 0), 4);
        // never none
        assert_eq!(fit_workers(4, 1, 1), 1);
    }

    #[test]
    fn light_jobs_run_as_many_as_light_connections() {
        assert_eq!(parallel_jobs(16, 2), 2);
        assert_eq!(parallel_jobs(16, 40), 16);
        assert_eq!(parallel_jobs(16, 0), 16);
        assert_eq!(parallel_jobs(0, 0), 1);
    }

    fn inp(max: i32, current: i32, spare: i32) -> SizingInput {
        SizingInput {
            max_connections: max,
            cap: LIGHT_MAX,
            flaky: false,
            current,
            spare,
            download_starved: false,
            bandwidth_bound: false,
        }
    }

    #[test]
    fn many_connections_with_headroom_reach_five() {
        // a fresh 50-connection server, all idle: straight to 5
        assert_eq!(next_size(&inp(50, 0, 50)), (5, Reason::Spare));
        // 40 connections: one per 8, so 5
        assert_eq!(next_size(&inp(40, 1, 39)), (5, Reason::Spare));
        // 100 connections still stop at 5
        assert_eq!(next_size(&inp(100, 5, 95)).0, 5);
        // grows only by what the download left idle
        assert_eq!(next_size(&inp(50, 1, 2)), (3, Reason::Spare));
        // 20 connections: 2
        assert_eq!(next_size(&inp(20, 1, 10)).0, 2);
    }

    #[test]
    fn flaky_server_holds_one() {
        let mut i = inp(50, 5, 40);
        i.flaky = true;
        assert_eq!(next_size(&i), (1, Reason::Flaky));
        i.current = 1;
        assert_eq!(next_size(&i), (1, Reason::Steady));
        // the error-rate threshold
        assert!(!window_flaky(1000, 2));
        assert!(!window_flaky(1000, 50));
        assert!(window_flaky(1000, 60));
        assert!(window_flaky(0, 3));
        assert!(!window_flaky(0, 0));
    }

    #[test]
    fn few_connections_hold_one() {
        for max in (TOO_FEW_CONNECTIONS + 1)..=FEW_CONNECTIONS {
            assert_eq!(
                next_size(&inp(max, 0, max)),
                (1, Reason::Spare),
                "max={max}"
            );
            assert_eq!(
                next_size(&inp(max, 1, max - 1)),
                (1, Reason::Steady),
                "max={max}"
            );
        }
        // up to 3 connections all stay with the download
        for max in 1..=TOO_FEW_CONNECTIONS {
            assert_eq!(
                next_size(&inp(max, 0, max)),
                (0, Reason::Steady),
                "max={max}"
            );
            assert_eq!(
                next_size(&inp(max, 1, 0)),
                (0, Reason::TooFewConnections),
                "max={max}"
            );
        }
        // 9 to 15 connections: still 1
        assert_eq!(next_size(&inp(15, 1, 14)).0, 1);
        assert_eq!(next_size(&inp(16, 1, 14)).0, 2);
    }

    #[test]
    fn saturating_download_shrinks_toward_one() {
        let mut i = inp(50, 5, 0);
        i.download_starved = true;
        let mut sizes = Vec::new();
        for _ in 0..6 {
            let (size, reason) = next_size(&i);
            if size < i.current {
                assert_eq!(reason, Reason::DownloadSaturated);
            }
            i.current = size;
            sizes.push(size);
        }
        assert_eq!(sizes, vec![4, 3, 2, 1, 1, 1]);
        // no connection idle but no download waiting either: hold
        let mut i = inp(50, 4, 0);
        assert_eq!(next_size(&i), (4, Reason::Steady));
        // a download already at its speed limit does not need more connections
        i.download_starved = true;
        i.bandwidth_bound = true;
        assert_eq!(next_size(&i), (4, Reason::Steady));
        // and once connections are idle again it grows back
        let i = inp(50, 1, 6);
        assert_eq!(next_size(&i), (5, Reason::Spare));
    }

    #[test]
    fn cap_limits_and_disables() {
        let mut i = inp(50, 5, 40);
        i.cap = 2;
        assert_eq!(next_size(&i), (2, Reason::Ceiling));
        i.cap = 0;
        assert_eq!(next_size(&i), (0, Reason::Disabled));
        i.current = 0;
        assert_eq!(next_size(&i), (0, Reason::Steady));
        // never a small server's connections
        assert_eq!(ceiling(3, LIGHT_MAX, false).0, 0);
        assert_eq!(ceiling(4, LIGHT_MAX, false).0, 1);
    }

    #[test]
    fn small_server_hands_its_light_connection_to_a_saturating_download() {
        for max in (TOO_FEW_CONNECTIONS + 1)..=FEW_CONNECTIONS {
            let mut i = inp(max, 1, 0);
            i.download_starved = true;
            assert_eq!(next_size(&i), (0, Reason::DownloadSaturated), "max={max}");
            i.current = 0;
            assert_eq!(next_size(&i), (0, Reason::Steady), "max={max}");
            // back once the download leaves a connection idle
            let i = inp(max, 0, 1);
            assert_eq!(next_size(&i), (1, Reason::Spare), "max={max}");
        }
        // a flaky small server too
        let mut i = inp(6, 1, 0);
        i.flaky = true;
        i.download_starved = true;
        assert_eq!(next_size(&i).0, 0);
        // a bigger server keeps at least 1
        let mut i = inp(16, 1, 0);
        i.download_starved = true;
        assert_eq!(next_size(&i), (1, Reason::Steady));
    }

    #[test]
    fn speed_limit_bound() {
        assert!(!bandwidth_bound(0, 1_000_000));
        assert!(bandwidth_bound(1_000_000, 950_000));
        assert!(!bandwidth_bound(1_000_000, 500_000));
    }

    #[test]
    fn change_message_names_server_and_reason() {
        let msg = change_message(b"news", 2, 5, Reason::Spare, "12 idle");
        assert_eq!(
            String::from_utf8(msg).unwrap(),
            "Light connections of news: 2 -> 5 (download connections left idle, 12 idle)"
        );
    }
}
