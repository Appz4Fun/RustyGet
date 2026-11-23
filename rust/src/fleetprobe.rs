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

/// How long one probe request may wait for the server, in seconds.
pub const PROBE_IO_TIMEOUT_SEC: i32 = 5;

/// The connections of a server with `max_connections` the probes may hold at
/// once: half of them, at least one.
pub fn probe_share(max_connections: i32) -> usize {
    (max_connections.max(1) / 2).max(1) as usize
}

/// A free connection of `srv` for a probe request, or null when none is
/// free: a light connection first ([`crate::light_pool::check_connection`]),
/// else a check connection of the main pool while the probes hold less than
/// their share of it. THE place probe requests get connections.
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
    crate::light_pool::check_connection(pool, srv)
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
}
