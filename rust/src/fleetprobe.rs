//! Where a fleet probe's article requests get their connections. Every probe
//! request (the STAT sampling of a copy's alive share and the few BODY reads
//! of par2 and archive headers) takes its connection through
//! [`probe_connection`]: a light connection of the server
//! ([`crate::light_pool`], F17) when one is free, else a check connection of
//! the main pool.
//!
//! The probe stays light: on the main pool, while downloads run, it takes at
//! most [`probe_share`] of a server's connections (the primary's download
//! keeps the rest; with downloads held it may take them all), and each request
//! waits at most [`PROBE_IO_TIMEOUT_SEC`]; a request that timed out leaves its
//! article unknown, never missing (F6).
//!
//! While downloads want every connection, the light reserve shrinks to one
//! connection per server, which used to probe a whole fleet one read at a
//! time (122 s to the primary on 6790 with a busy queue). Probe requests keep
//! a floor of [`PROBE_READ_FLOOR`] at once per server ([`probe_reads`]): the
//! light connections, plus idle connections of the probe share the main pool
//! already grants them, taken even while a download was refused one (never a
//! connection a download holds). A flaky server stays at one, a server with
//! few connections at its light count.

/// How long one probe request may wait for the server, in seconds.
pub const PROBE_IO_TIMEOUT_SEC: i32 = 5;

/// The connections of a server with `max_connections` the probes may hold at
/// once: half of them, at least one.
pub fn probe_share(max_connections: i32) -> usize {
    (max_connections.max(1) / 2).max(1) as usize
}

/// The probe requests a server keeps at once while downloads want every
/// connection and its light reserve shrank to one.
pub const PROBE_READ_FLOOR: i32 = 5;

/// The probe requests one server may have at once: its light connections
/// (`light`), raised to [`PROBE_READ_FLOOR`] with check connections of its
/// [`probe_share`]. A flaky server (`flaky`: recent errors or timeouts, as
/// the light reserve measures them) keeps one; a server with at most
/// [`crate::light_pool::FEW_CONNECTIONS`] keeps its light count; 0 for a
/// server with no light reserve (the main pool serves its probes as any
/// check, [`crate::light_pool::parallel_jobs`]).
pub fn probe_reads(light: i32, max_connections: i32, flaky: bool) -> i32 {
    if light <= 0 {
        0
    } else if flaky {
        1
    } else if max_connections <= crate::light_pool::FEW_CONNECTIONS {
        light
    } else {
        light.max(PROBE_READ_FLOOR.min(probe_share(max_connections) as i32))
    }
}

/// The check connections of a server a probe may hold even while a download
/// was refused a connection (`crate::net::check_connection_allowed`): what
/// [`probe_reads`] adds to the light connections, at least the one every
/// check may take.
pub fn probe_check_floor(light: i32, max_connections: i32, flaky: bool) -> usize {
    (probe_reads(light, max_connections, flaky) - light.max(0)).max(1) as usize
}

/// How many probe reads to run at once out of at most `max`
/// ([`crate::light_pool::parallel_jobs`] with the [`probe_reads`] of the
/// primary servers now): the rest queue instead of polling for a connection.
#[cfg(unix)]
pub fn probe_reads_now(max: usize) -> usize {
    let reads = crate::net::active_server_pool().map_or(0, |p| p.primary_probe_reads());
    crate::light_pool::parallel_jobs(max, reads)
}

/// A free connection of `srv` for a probe request, or null when none is
/// free: a light connection first, else a check connection of the main pool
/// while the probes hold less than their share of it (up to
/// [`probe_check_floor`] of them even while downloads want every
/// connection). THE place probe requests get connections.
#[cfg(unix)]
pub fn probe_connection(
    pool: &crate::net::NzbgetRsServerPool,
    srv: &crate::net::RsNewsServer,
) -> *mut crate::net::PooledNntpConn {
    use std::sync::atomic::Ordering;
    let light = pool.get_light_connection(srv);
    if !light.is_null() {
        return light;
    }
    // with downloads held (paused, or a client's fleet holding them for its
    // check) no download needs the connections, so the probes may take them all
    let downloads_held = crate::ffi::active_workstate().is_some_and(|w| w.download_held());
    if !downloads_held && srv.check_in_use.load(Ordering::SeqCst) >= probe_share(srv.max_connections) {
        return std::ptr::null_mut();
    }
    pool.get_probe_connection(srv)
}

/// Shortens a probe connection's request timeout once it is connected (a
/// light connection keeps the light pool's own short timeout).
#[cfg(unix)]
pub fn probe_connected(conn: &crate::net::PooledNntpConn) {
    if !crate::light_pool::is_light(conn) {
        conn.set_io_timeout(PROBE_IO_TIMEOUT_SEC);
    }
}

/// Gives a probe connection back to its pool with its own timeout again.
#[cfg(unix)]
pub fn probe_connection_done(
    pool: &crate::net::NzbgetRsServerPool,
    conn_ptr: *mut crate::net::PooledNntpConn,
    broken: bool,
) {
    // SAFETY: the caller hands back a connection probe_connection gave it
    if let Some(conn) = unsafe { conn_ptr.as_ref() } {
        if broken {
            unsafe { conn.disconnect() };
        } else if !crate::light_pool::is_light(conn) {
            conn.set_io_timeout(0);
        }
    }
    pool.free_connection(conn_ptr, true);
}

/// The [`crate::articlefetcher::NntpHost`] probe BODY reads go through: the
/// daemon's own, except that connections come from [`probe_connection`] and
/// each request waits at most [`PROBE_IO_TIMEOUT_SEC`].
#[cfg(unix)]
pub struct ProbeNntpHost;

#[cfg(unix)]
pub static PROBE_HOST: ProbeNntpHost = ProbeNntpHost;

#[cfg(unix)]
impl ProbeNntpHost {
    /// Whether a connection this host handed out is a light one.
    fn conn_is_light(&self, conn: crate::articlefetcher::ConnHandle) -> bool {
        conn_light(conn)
    }
}

/// Whether `conn` is a light connection ([`crate::light_pool::is_light`]).
#[cfg(unix)]
fn conn_light(conn: crate::articlefetcher::ConnHandle) -> bool {
    // SAFETY: a handle this host handed out stays valid until released
    unsafe { conn.pooled().as_ref() }.is_some_and(crate::light_pool::is_light)
}

#[cfg(unix)]
impl crate::articlefetcher::NntpHost for ProbeNntpHost {
    fn status(&self) -> crate::articlefetcher::HostStatus {
        crate::articlefetcher::NativeNntpHost.status()
    }

    fn server(&self, index: usize) -> Option<crate::articlefetcher::ServerInfo> {
        crate::articlefetcher::NativeNntpHost.server(index)
    }

    fn get_connection(
        &self,
        level: i32,
        excluded: &[crate::articlefetcher::ServerHandle],
    ) -> Option<crate::articlefetcher::ConnInfo> {
        let native = &crate::articlefetcher::NativeNntpHost;
        let pool = crate::net::active_server_pool()?;
        for idx in 0..native.status().server_count {
            let Some(info) = native.server(idx) else {
                continue;
            };
            if !info.active || info.norm_level != level || excluded.contains(&info.handle) {
                continue;
            }
            let Some(srv) = pool.server_by_id(info.id) else {
                continue;
            };
            let conn_ptr = probe_connection(&pool, &srv);
            // SAFETY: a non-null connection from the pool stays valid until freed
            let Some(conn) = (unsafe { conn_ptr.as_ref() }) else {
                continue;
            };
            return Some(crate::articlefetcher::ConnInfo {
                // SAFETY: just handed out by the pool, freed only by `release`
                conn: unsafe { crate::articlefetcher::ConnHandle::from_pooled(conn_ptr) },
                reused: conn.is_connected(),
                server: info,
            });
        }
        None
    }

    fn release(&self, conn: crate::articlefetcher::ConnHandle, keep_connected: bool) {
        // restores a main-pool connection's own timeout before the pool gets it back
        if !self.conn_is_light(conn) {
            crate::articlefetcher::NativeNntpHost.set_io_timeout(conn, 0);
        }
        crate::articlefetcher::NativeNntpHost.release(conn, keep_connected);
    }

    fn conn_server(
        &self,
        conn: crate::articlefetcher::ConnHandle,
    ) -> Option<crate::articlefetcher::ServerInfo> {
        crate::articlefetcher::NativeNntpHost.conn_server(conn)
    }

    fn connect(&self, conn: crate::articlefetcher::ConnHandle) -> bool {
        let native = &crate::articlefetcher::NativeNntpHost;
        let ok = native.connect(conn);
        if ok && !self.conn_is_light(conn) {
            native.set_io_timeout(conn, PROBE_IO_TIMEOUT_SEC);
        }
        ok
    }

    fn join_group(&self, conn: crate::articlefetcher::ConnHandle, group: &[u8]) -> Option<Vec<u8>> {
        crate::articlefetcher::NativeNntpHost.join_group(conn, group)
    }

    fn request(&self, conn: crate::articlefetcher::ConnHandle, cmd: &[u8]) -> Option<Vec<u8>> {
        crate::articlefetcher::NativeNntpHost.request(conn, cmd)
    }

    fn read(&self, conn: crate::articlefetcher::ConnHandle, buf: &mut [u8]) -> Option<usize> {
        crate::articlefetcher::NativeNntpHost.read(conn, buf)
    }

    fn disconnect(&self, conn: crate::articlefetcher::ConnHandle) {
        crate::articlefetcher::NativeNntpHost.disconnect(conn)
    }

    fn cancel(&self, conn: crate::articlefetcher::ConnHandle) {
        crate::articlefetcher::NativeNntpHost.cancel(conn)
    }

    fn set_io_timeout(&self, conn: crate::articlefetcher::ConnHandle, secs: i32) {
        // a probe request never waits longer than its own short timeout (a
        // light connection keeps the light pool's)
        let secs = if self.conn_is_light(conn) {
            secs
        } else if secs <= 0 {
            PROBE_IO_TIMEOUT_SEC
        } else {
            secs.min(PROBE_IO_TIMEOUT_SEC)
        };
        crate::articlefetcher::NativeNntpHost.set_io_timeout(conn, secs)
    }

    fn light_host(&self) -> Option<&'static dyn crate::articlefetcher::NntpHost> {
        Some(&PROBE_HOST)
    }

    fn detail(&self, msg: &[u8]) {
        crate::articlefetcher::NativeNntpHost.detail(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probes never take more than half of a server's connections, and a
    /// server with one connection still lends it.
    #[test]
    fn probes_take_at_most_half_of_the_connections() {
        assert_eq!(probe_share(1), 1);
        assert_eq!(probe_share(2), 1);
        assert_eq!(probe_share(8), 4);
        assert_eq!(probe_share(50), 25);
        assert_eq!(probe_share(0), 1);
        assert!(PROBE_IO_TIMEOUT_SEC <= 5);
    }

    /// 6790 (#717 follow-up): with downloads using every connection the
    /// light reserve is 1, and the probes read one article at a time. They
    /// keep a floor of 5 reads per server from the probe share; never more
    /// than half the server's connections, a flaky server stays at 1, a
    /// server with few connections at its light count, and with no reserve
    /// the main pool serves the probes as before.
    #[test]
    fn probe_reads_keep_a_floor_beside_a_saturating_download() {
        use crate::light_pool::LIGHT_MAX;
        // 60 connections, reserve shrunk to 1: 5 reads, 4 from the probe share
        assert_eq!(probe_reads(1, 60, false), PROBE_READ_FLOOR);
        assert_eq!(probe_check_floor(1, 60, false), 4);
        // a full reserve needs nothing from the main pool past the usual one
        assert_eq!(probe_reads(LIGHT_MAX, 60, false), LIGHT_MAX);
        assert_eq!(probe_check_floor(LIGHT_MAX, 60, false), 1);
        // a flaky server: one read, and the usual one check connection
        assert_eq!(probe_reads(1, 60, true), 1);
        assert_eq!(probe_check_floor(1, 60, true), 1);
        // few connections: its light count only
        assert_eq!(probe_reads(1, 8, false), 1);
        assert_eq!(probe_check_floor(1, 8, false), 1);
        // never more than the probe share (half the connections)
        assert_eq!(probe_reads(1, 9, false), 4);
        assert_eq!(probe_check_floor(1, 9, false), 3);
        // no reserve (off, or too few connections): unchanged
        assert_eq!(probe_reads(0, 60, false), 0);
        assert_eq!(crate::light_pool::parallel_jobs(16, probe_reads(0, 60, false)), 16);
        assert_eq!(crate::light_pool::parallel_jobs(16, probe_reads(1, 60, false)), 5);
    }
}
